//! PS5_RUFFLE_V5 — Ruffle5: Ruffle as a PlayStation 5 title, with its own menu (milestone 4).
//!
//! The C shell (shell/main.c) calls `sceAgcInit(8)` from the executable, then `ruffle_ps5_main`.
//! Two phases, so the novel GPU seam is proven before Ruffle rides on it:
//!   * 3a (the default build): bring up wgpu on the static RADV and clear the display to an
//!     animated colour for a few seconds. Colour on the TV means the static-RADV + VK_KHR_display +
//!     wgpu path works in a title — the thing the payload route could never get.
//!   * 3b (feature `ruffle`): the title proper. It opens on a menu (src/menu.rs, drawn by
//!     src/ui.rs) listing the games folder; Cross builds a Ruffle player for the chosen SWF and
//!     plays it through ruffle_render_wgpu on the shared display (src/shell.rs); holding OPTIONS
//!     in a game drops that player and returns to the menu; holding Circle in the menu ends the
//!     title. A game plays with the DualSense as keyboard and mouse (src/input.rs, mapping from
//!     src/controls.rs's files), a drawn pointer (src/cursor.rs) and sound (src/audio.rs).
//!
//! Progress is toasted on the TV and written to /data/ruffle/ruffle.log by the shell's `ps5_log`.
// SPDX-License-Identifier: GPL-3.0-or-later

use std::ffi::{c_char, c_int, CStr};
use std::time::{Duration, Instant};

/// The title's version, as Settings shows it.
pub const APP_VERSION: &str = "1.000.020";
pub const APP_NAME: &str = "Ruffle5";

#[cfg(feature = "ruffle")]
mod audio;
#[cfg(feature = "ruffle")]
mod controls;
#[cfg(feature = "ruffle")]
mod cursor;
mod gpu;
#[cfg(feature = "ruffle")]
mod input;
#[cfg(feature = "ruffle")]
mod menu;
mod random;
#[cfg(feature = "ruffle")]
mod shell;
#[cfg(feature = "ruffle")]
mod swfinfo;
#[cfg(feature = "ruffle")]
mod ui;

extern "C" {
    fn ps5_notify(msg: *const c_char);
    fn ps5_log(msg: *const c_char);
}

/// One line to ruffle.log. Serialised: the audio thread logs too, and the shell's `ps5_log` is
/// plain fprintf.
static LOG_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());
fn say(s: &str) {
    if let Ok(c) = std::ffi::CString::new(s) {
        let _guard = LOG_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        unsafe { ps5_log(c.as_ptr()) };
    }
}

/// wgpu-hal says why it declines an adapter, which extensions it lacks, what the swapchain does —
/// all through `log`, at debug level. Into ruffle.log, so the console tells us instead of us guessing.
struct Ps5Logger;
impl log::Log for Ps5Logger {
    fn enabled(&self, m: &log::Metadata) -> bool {
        if m.target().starts_with("wgpu_hal") {
            m.level() <= log::Level::Debug
        } else if m.target().starts_with("symphonia") {
            // Every MP3 event sound starts with "invalid main_data_begin, underflow by N bytes":
            // its first frame references bit-reservoir bytes from before the cut, and symphonia
            // zeroes that granule (layer3/mod.rs, read_main_data) — a few ms of silence Flash's
            // SeekSamples skip anyway, the same on every Ruffle frontend. Not worth a line per hit.
            m.level() <= log::Level::Error
        } else {
            m.level() <= log::Level::Info
        }
    }
    fn log(&self, r: &log::Record) {
        if self.enabled(r.metadata()) {
            say(&format!("[{} {}] {}", r.level(), r.target(), r.args()));
        }
    }
    fn flush(&self) {}
}
static LOGGER: Ps5Logger = Ps5Logger;
fn toast(s: &str) {
    if let Ok(c) = std::ffi::CString::new(s) {
        unsafe { ps5_notify(c.as_ptr()) };
    }
}

/// Entry from the C shell. `data_dir` is where the log and the controls live (/data/ruffle),
/// `games_dir` where the menu looks for SWFs. Returns the number of failures (0 = ok), which the
/// shell logs.
#[no_mangle]
pub extern "C" fn ruffle_ps5_main(data_dir: *const c_char, games_dir: *const c_char) -> c_int {
    let data_dir = unsafe { CStr::from_ptr(data_dir) }
        .to_string_lossy()
        .into_owned();
    let games_dir = unsafe { CStr::from_ptr(games_dir) }
        .to_string_lossy()
        .into_owned();
    say(&format!(
        "ruffle-ps5 (PS5_RUFFLE_V5, {APP_NAME} {APP_VERSION}) starting; data dir {data_dir}, games {games_dir}"
    ));
    if log::set_logger(&LOGGER).is_ok() {
        log::set_max_level(log::LevelFilter::Debug);
    }
    toast("ruffle-ps5: bringing up RADV");

    let t_start = Instant::now();
    let gpu = match gpu::create() {
        Ok(g) => {
            say(&format!(
                "ok   wgpu on RADV in {:.2} s: {}x{} {:?}; {}",
                t_start.elapsed().as_secs_f64(),
                g.width,
                g.height,
                g.format,
                g.report
            ));
            say(&format!("     adapter: {:?}", g.adapter.get_info()));
            say(&format!("     features: {:?}", g.device.features()));
            toast(&format!("ruffle-ps5: RADV up, {}x{}", g.width, g.height));
            g
        }
        Err(e) => {
            say(&format!("FAIL wgpu on RADV: {e}"));
            toast("ruffle-ps5: RADV bring-up failed (see ruffle.log)");
            return 1;
        }
    };

    #[cfg(not(feature = "ruffle"))]
    let result = run_clear(&gpu);
    #[cfg(feature = "ruffle")]
    let result = run_app(gpu, &data_dir, &games_dir);
    match result {
        Ok(()) => 0,
        Err(e) => {
            say(&format!("FAIL {e}"));
            toast("ruffle-ps5: failed (see ruffle.log)");
            1
        }
    }
}

/// 3a: animate a clear colour for 8 seconds, proving present works end to end. The first frame
/// is logged on its own: it is the one that creates the swapchain.
#[cfg(not(feature = "ruffle"))]
fn run_clear(gpu: &gpu::Gpu) -> Result<(), String> {
    let t0 = Instant::now();
    let mut frames = 0u64;
    while t0.elapsed() < Duration::from_secs(8) {
        let t = t0.elapsed().as_secs_f64();
        let r = 0.5 + 0.5 * (t * 1.1).sin();
        let g = 0.15;
        let b = 0.5 + 0.5 * (t * 0.7).cos();
        gpu::present_clear(gpu, r, g, b).map_err(|e| format!("present at frame {frames}: {e}"))?;
        if frames == 0 {
            say(&format!(
                "ok   first frame presented after {:.3} s",
                t0.elapsed().as_secs_f64()
            ));
        }
        frames += 1;
    }
    let fps = frames as f64 / t0.elapsed().as_secs_f64();
    say(&format!(
        "ok   {frames} frames presented in {:.1} s, {fps:.1} fps",
        t0.elapsed().as_secs_f64()
    ));
    toast(&format!("ruffle-ps5: RADV present OK, {fps:.0} fps"));
    Ok(())
}

/// The title proper: the menu, and a player per game chosen from it.
#[cfg(feature = "ruffle")]
fn run_app(gpu: gpu::Gpu, data_dir: &str, games_dir: &str) -> Result<(), String> {
    use ruffle_render_wgpu::target::SwapChainTarget;

    let descriptors = gpu::descriptors(&gpu);
    let gpu::Gpu {
        surface,
        adapter,
        device,
        queue,
        width,
        height,
        ..
    } = gpu;
    // One swapchain for the whole run (src/shell.rs); the pointer state the input side moves.
    let swap_chain = SwapChainTarget::new(surface, &adapter, (width, height), &device);
    let cursor = cursor::shared(width as f32 / 2.0, height as f32 / 2.0, 1.75);
    let display = shell::Display::new(swap_chain, device.clone(), queue.clone(), std::sync::Arc::clone(&cursor))?;
    say(&format!("ok   display shared: {}x{} {:?}; ui fonts loaded", display.width, display.height, display.format));

    // The controller, for the menu and every game (a game's mapping is set when it starts).
    let mut input = input::Input::new(width, height, controls::Controls::default(), cursor);
    say(&format!("     {}", input.report));

    // The menu: what the games folder holds.
    let mut menu = menu::Menu::new(games_dir, "/app0");
    {
        let mut ui = display.ui.lock().map_err(|_| "ui lock")?;
        menu.load_pictures(&mut ui, &display.device, &display.queue);
    }
    for line in &menu.report {
        say(&format!("     {line}"));
    }
    toast(&format!("Ruffle5: {} game(s)", menu.games.len()));

    let mut scene = ui::Scene::new();
    let mut last = Instant::now();
    let mut events = Vec::new();
    loop {
        // ---- the menu ----
        input.reset();
        let chosen = loop {
            let now = Instant::now();
            let dt = now.duration_since(last).as_secs_f64().min(0.1);
            last = now;
            events.clear();
            if let Some(note) = input.poll(dt, &mut events) {
                say(&format!("ok   {note}"));
            }
            match menu.update(dt as f32, input.raw_pressed, input.raw_down) {
                Some(menu::Action::Play(k)) => break Some(k),
                Some(menu::Action::Quit) => break None,
                None => {}
            }
            scene.clear();
            {
                let mut ui = display.ui.lock().map_err(|_| "ui lock")?;
                menu.draw(&mut ui, &mut scene, dt as f32);
            }
            display.present_scene(&scene, ui::Color::rgb(0x0e0f12))?;
        };
        let Some(k) = chosen else {
            say("menu: closing (Circle held)");
            toast("Ruffle5: bye");
            return Ok(());
        };
        let (swf_path, name) = match menu.games.get(k) {
            Some(g) => (g.path.clone(), g.name.clone()),
            None => continue,
        };

        // ---- a game ----
        say(&format!("menu: play {name} ({swf_path})"));
        toast(&format!("Ruffle5: {name}"));
        let outcome = play_game(&display, &descriptors, &mut input, data_dir, &swf_path, &mut events);
        display.set_overlay(ui::Scene::new());
        match outcome {
            Ok(()) => say(&format!("menu: back from {name}")),
            Err(e) => {
                say(&format!("FAIL {name}: {e}"));
                toast(&format!("Ruffle5: {name} failed (see ruffle.log)"));
            }
        }
        last = Instant::now();
    }
}

/// Play one SWF until OPTIONS is held: a fresh player on the shared display, its own audio
/// backend, the game's controls. Returns when the player is dropped.
#[cfg(feature = "ruffle")]
fn play_game(
    display: &std::sync::Arc<shell::Display>,
    descriptors: &std::sync::Arc<ruffle_render_wgpu::descriptors::Descriptors>,
    input: &mut input::Input,
    data_dir: &str,
    swf_path: &str,
    events: &mut Vec<ruffle_core::events::PlayerEvent>,
) -> Result<(), String> {
    use ruffle_core::backend::audio::AudioBackend;
    use ruffle_core::backend::audio::NullAudioBackend;
    use ruffle_core::backend::log::NullLogBackend;
    use ruffle_core::backend::navigator::{NullExecutor, NullNavigatorBackend};
    use ruffle_core::backend::storage::MemoryStorageBackend;
    use ruffle_core::backend::ui::NullUiBackend;
    use ruffle_core::tag_utils::SwfMovieData;
    use ruffle_core::PlayerBuilder;
    use ruffle_render_wgpu::backend::WgpuRenderBackend;

    let t0 = Instant::now();
    let data = std::fs::read(swf_path).map_err(|e| format!("read {swf_path}: {e}"))?;
    let movie = SwfMovieData::from_data(&data, format!("file://{swf_path}"), None, None)
        .map_err(|e| format!("parse swf: {e}"))?;
    // The game's folder is its base path: files it loads sit beside it.
    let base = std::path::Path::new(swf_path)
        .parent()
        .map(|p| p.to_string_lossy().into_owned())
        .unwrap_or_else(|| data_dir.to_string());

    // What the pad does for this movie: defaults, then the layers of files (src/controls.rs).
    let controls = controls::Controls::load(data_dir, swf_path);
    for line in &controls.report {
        say(&format!("     {line}"));
    }
    say("     controls in effect:");
    for line in controls.summary() {
        say(&format!("       {line}"));
    }
    input.set_controls(controls);

    let renderer = WgpuRenderBackend::new(std::sync::Arc::clone(descriptors), display.target())
        .map_err(|e| format!("renderer: {e}"))?;
    let audio: Box<dyn AudioBackend> = match audio::Ps5AudioBackend::new(say) {
        Ok(a) => {
            say(&format!("ok   AudioOut open: 48 kHz s16 stereo, {}-frame grains", a.grain));
            Box::new(a)
        }
        Err(e) => {
            say(&format!("warn no sound: {e}"));
            Box::new(NullAudioBackend::new())
        }
    };
    let mut executor = NullExecutor::new();
    let player = PlayerBuilder::new()
        .with_renderer(renderer)
        .with_boxed_audio(audio)
        .with_letterbox(ruffle_core::config::Letterbox::On)
        .with_navigator(
            NullNavigatorBackend::with_base_path(std::path::Path::new(&base), &executor)
                .map_err(|e| format!("navigator base path: {e}"))?,
        )
        .with_storage(Box::new(MemoryStorageBackend::new()))
        .with_ui(NullUiBackend::new())
        .with_log(NullLogBackend::new())
        .with_movie(movie)
        .with_viewport_dimensions(display.width, display.height, 1.0)
        .with_autoplay(true)
        .build();
    say(&format!("ok   player built in {:.2} s; entering frame loop", t0.elapsed().as_secs_f64()));

    let mut last = Instant::now();
    let mut hold = 0.0f32;
    let mut overlay = ui::Scene::new();
    let frame = Duration::from_micros(16_666);
    loop {
        let now = Instant::now();
        let dt = now.duration_since(last);
        last = now;
        events.clear();
        if let Some(note) = input.poll(dt.as_secs_f64(), events) {
            say(&format!("ok   {note}"));
        }
        // Holding OPTIONS: the bar appears, fills, and the game ends.
        if input.raw_down & input::BTN_OPTIONS != 0 {
            hold += dt.as_secs_f32();
        } else {
            hold = 0.0;
        }
        if hold >= menu::HOLD_TOTAL {
            say("game: OPTIONS held, returning to the menu");
            break;
        }
        overlay.clear();
        if hold > menu::HOLD_SHOW_AFTER {
            if let Ok(mut ui) = display.ui.lock() {
                menu::draw_hold_bar(&mut ui, &mut overlay, hold, "Keep holding to return to the menu", menu::Glyph::Chip("OPTIONS"));
            }
        }
        display.set_overlay(std::mem::take(&mut overlay));
        {
            let mut p = player.lock().unwrap();
            for event in events.drain(..) {
                p.handle_event(event);
            }
            p.tick(ruffle_core::FloatDuration::from_millis(dt.as_secs_f64() * 1000.0));
            if p.needs_render() || hold > 0.0 {
                p.render();
            }
        }
        executor.run();
        // The surface is FIFO: present paces the loop at the display's rate. A floor for a
        // movie that needs no render this frame.
        if dt < frame {
            std::thread::sleep(frame - dt);
        }
    }
    // The player goes (its renderer's handle to the display, its audio port) before the menu
    // draws again.
    drop(player);
    Ok(())
}
