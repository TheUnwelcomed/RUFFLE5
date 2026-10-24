//! PS5_RUFFLE_V6 — Ruffle5: Ruffle as a PlayStation 5 title, with its own menu.
//!
//! The C shell (shell/main.c) calls `sceAgcInit(8)` from the executable, then `ruffle_ps5_main`.
//! Two phases, so the novel GPU seam is proven before Ruffle rides on it:
//!   * 3a (the default build): bring up wgpu on the static RADV and clear the display to an
//!     animated colour for a few seconds.
//!   * 3b (feature `ruffle`): the title proper. It opens on a menu (src/menu.rs, drawn by
//!     src/ui.rs over ruffle.rs's logo animation, src/background.rs) listing the games folder;
//!     Cross plays a game (src/game.rs) through ruffle_render_wgpu on the shared display
//!     (src/shell.rs), Triangle edits its controls (src/editor.rs), Settings holds Ruffle's player
//!     options and Ruffle5's own (src/settings.rs). Holding Circle in the menu ends the title.
//!
//! V6 (1.000.020, 2026-10-24): the display at 3840x2160 and games in 4K or 1080p; Ruffle's colours
//! and logo; the controls editor and "Custom Mappings"; saves on disk; the quick menu, on-screen
//! keyboard, covers, favourites, play time, search, badges, menu sounds, USB keyboard and mouse,
//! and Ruffle's player options — most after Ruffle-Flash-PS5 by ZiZc3 (GPL-3.0); the frame-time
//! log in its format; the sound on a CPU core of its own; data in /data/ruffle5.
//!
//! Progress is written to /data/ruffle5/ruffle.log by the shell's `ps5_log`.
// SPDX-License-Identifier: GPL-3.0-or-later

use std::ffi::{c_char, c_int, CStr};
use std::time::{Duration, Instant};

/// The title's version, as Settings shows it.
pub const APP_VERSION: &str = "1.000.020";
pub const APP_NAME: &str = "Ruffle5";
pub const AUTHOR: &str = "@TheUnwelcomed";

#[cfg(feature = "ruffle")]
mod audio;
#[cfg(feature = "ruffle")]
mod background;
#[cfg(feature = "ruffle")]
mod badges;
#[cfg(feature = "ruffle")]
mod controls;
#[cfg(feature = "ruffle")]
mod covers;
#[cfg(feature = "ruffle")]
mod cpu;
#[cfg(feature = "ruffle")]
mod cursor;
#[cfg(feature = "ruffle")]
mod dirinfo;
#[cfg(feature = "ruffle")]
mod editor;
#[cfg(feature = "ruffle")]
mod fsutil;
#[cfg(feature = "ruffle")]
mod game;
mod gpu;
#[cfg(feature = "ruffle")]
mod hid;
#[cfg(feature = "ruffle")]
mod input;
#[cfg(feature = "ruffle")]
mod keyboard;
#[cfg(feature = "ruffle")]
mod library;
#[cfg(feature = "ruffle")]
mod menu;
#[cfg(feature = "ruffle")]
mod paths;
#[cfg(feature = "ruffle")]
mod ps5ui;
#[cfg(feature = "ruffle")]
mod quickmenu;
mod random;
#[cfg(feature = "ruffle")]
mod saves;
#[cfg(feature = "ruffle")]
mod settings;
#[cfg(feature = "ruffle")]
mod shell;
#[cfg(feature = "ruffle")]
mod shockwave;
#[cfg(feature = "ruffle")]
mod sounds;
#[cfg(feature = "ruffle")]
mod swfinfo;
#[cfg(feature = "ruffle")]
mod ui;
#[cfg(feature = "ruffle")]
mod widgets;

/// For `cargo test` on a PC, the shell's C functions have stand-ins.
#[cfg(test)]
mod host;

extern "C" {
    fn ps5_notify(msg: *const c_char);
    fn ps5_log(msg: *const c_char);
    fn ps5_local_hour() -> c_int;
}

/// One line to ruffle.log. Serialised (the audio, writer and cover threads log too), and a line
/// that repeats the one before is only counted (games can warn the same thing every frame).
static LOG_LOCK: std::sync::Mutex<(String, u32)> = std::sync::Mutex::new((String::new(), 0));
fn write_log(s: &str) {
    if let Ok(c) = std::ffi::CString::new(s.replace('\0', " ")) {
        unsafe { ps5_log(c.as_ptr()) };
    }
}
pub(crate) fn say(s: &str) {
    let mut last = LOG_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    if last.0 == s {
        last.1 += 1;
        return;
    }
    if last.1 > 0 {
        write_log(&format!("    (repeated {} more times)", last.1));
    }
    last.0 = s.to_string();
    last.1 = 0;
    write_log(s);
}

/// wgpu-hal says why it declines an adapter, which extensions it lacks, what the swapchain does —
/// all through `log`, at debug level. Into ruffle.log, so the console tells us instead of us guessing.
struct Ps5Logger;
impl log::Log for Ps5Logger {
    fn enabled(&self, m: &log::Metadata) -> bool {
        if m.target().starts_with("avm_trace") {
            // a game's own trace() output would flood the log
            false
        } else if m.target().starts_with("wgpu_hal") {
            m.level() <= log::Level::Debug
        } else if m.target().starts_with("symphonia") {
            // Every MP3 event sound starts with "invalid main_data_begin, underflow by N bytes":
            // its first frame references bit-reservoir bytes from before the cut, and symphonia
            // zeroes that granule — a few ms of silence Flash's SeekSamples skip anyway.
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

pub(crate) fn toast(s: &str) {
    if let Ok(c) = std::ffi::CString::new(s.replace('\0', " ")) {
        unsafe { ps5_notify(c.as_ptr()) };
    }
}

/// The console's local hour (0-23), or -1.
pub(crate) fn local_hour() -> i32 {
    unsafe { ps5_local_hour() }
}

/// Entry from the C shell. `data_dir` is Ruffle5's own folder (log, settings, saves), `games_dir`
/// where the menu looks for SWFs, `mappings_dir` the "Custom Mappings" folder. Returns the number of
/// failures (0 = ok), which the shell logs.
#[no_mangle]
pub extern "C" fn ruffle_ps5_main(data_dir: *const c_char, games_dir: *const c_char, mappings_dir: *const c_char) -> c_int {
    let data_dir = unsafe { CStr::from_ptr(data_dir) }.to_string_lossy().into_owned();
    let games_dir = unsafe { CStr::from_ptr(games_dir) }.to_string_lossy().into_owned();
    let mappings_dir = unsafe { CStr::from_ptr(mappings_dir) }.to_string_lossy().into_owned();
    say(&format!("ruffle-ps5 (PS5_RUFFLE_V6, {APP_NAME} {APP_VERSION}) starting; data {data_dir}, games {games_dir}, mappings {mappings_dir}"));
    if log::set_logger(&LOGGER).is_ok() {
        log::set_max_level(log::LevelFilter::Debug);
    }
    std::panic::set_hook(Box::new(|info| {
        let msg = info
            .payload()
            .downcast_ref::<&str>()
            .map(|s| s.to_string())
            .or_else(|| info.payload().downcast_ref::<String>().cloned())
            .unwrap_or_else(|| "(no message)".into());
        let place = info.location().map(|l| format!("{}:{}", l.file(), l.line())).unwrap_or_default();
        say(&format!("*** panic at {place}: {msg}"));
        toast(&format!("Ruffle5 stopped: {msg}"));
    }));

    let t_start = Instant::now();
    let gpu = match gpu::create() {
        Ok(g) => {
            say(&format!("ok   wgpu on RADV in {:.2} s: {}x{} {:?}; {}", t_start.elapsed().as_secs_f64(), g.width, g.height, g.format, g.report));
            say(&format!("     adapter: {:?}", g.adapter.get_info()));
            say(&format!("     features: {:?}", g.device.features()));
            g
        }
        Err(e) => {
            say(&format!("FAIL wgpu on RADV: {e}"));
            toast("Ruffle5: the GPU did not start (see ruffle.log)");
            return 1;
        }
    };

    #[cfg(not(feature = "ruffle"))]
    let result = {
        let _ = (&data_dir, &games_dir, &mappings_dir);
        run_clear(&gpu)
    };
    #[cfg(feature = "ruffle")]
    let result = run_app(gpu, paths::Paths::new(&data_dir, &games_dir, &mappings_dir));
    match result {
        Ok(()) => 0,
        Err(e) => {
            say(&format!("FAIL {e}"));
            toast("Ruffle5: failed (see ruffle.log)");
            1
        }
    }
}

/// 3a: animate a clear colour for 8 seconds, proving present works end to end.
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
            say(&format!("ok   first frame presented after {:.3} s", t0.elapsed().as_secs_f64()));
        }
        frames += 1;
    }
    let fps = frames as f64 / t0.elapsed().as_secs_f64();
    say(&format!("ok   {frames} frames presented in {:.1} s, {fps:.1} fps", t0.elapsed().as_secs_f64()));
    toast(&format!("ruffle-ps5: RADV present OK, {fps:.0} fps"));
    Ok(())
}

/// Everything the title's screens and games share.
#[cfg(feature = "ruffle")]
pub(crate) struct App {
    pub display: std::sync::Arc<shell::Display>,
    pub descriptors: std::sync::Arc<ruffle_render_wgpu::descriptors::Descriptors>,
    pub input: input::Input,
    pub paths: paths::Paths,
    pub settings: settings::Settings,
    pub library: library::Library,
    pub badges: badges::Badges,
    pub menu: menu::Menu,
}

#[cfg(feature = "ruffle")]
static GPU_ERRORS: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);

/// The title proper: the menu, and a player per game chosen from it.
#[cfg(feature = "ruffle")]
fn run_app(gpu: gpu::Gpu, paths: paths::Paths) -> Result<(), String> {
    use std::sync::Arc;

    let descriptors = gpu::descriptors(&gpu);
    let gpu::Gpu { surface, device, queue, width, height, config, .. } = gpu;
    // A GPU validation error is written to the log instead of ending the title (wgpu's default
    // handler panics, and a title's panic is an abort).
    device.on_uncaptured_error(Arc::new(|e: wgpu::Error| {
        let n = GPU_ERRORS.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        if n < 20 {
            say(&format!("warn wgpu error #{}: {e}", n + 1));
        }
    }));
    let cursor = cursor::shared(width as f32 / 2.0, height as f32 / 2.0, 1.75);
    let display = shell::Display::new(surface, config, device.clone(), queue.clone(), Arc::clone(&cursor))?;
    say(&format!("ok   display {}x{} {:?}; ui fonts loaded (scale {})", display.width, display.height, display.format, width as f32 / ui::W));
    log::set_max_level(log::LevelFilter::Info);

    // Files: Ruffle5's own folder, Custom Mappings, the old places moved over once.
    let _ = std::fs::create_dir_all(&paths.data);
    let _ = std::fs::create_dir_all(&paths.mappings);
    for line in controls::migrate(&paths) {
        say(&format!("     {line}"));
    }
    let settings = settings::Settings::load(&paths.file("settings.txt"));
    say(&format!("     settings: {settings:?}"));
    let mut library = library::Library::load(&paths.file("library.txt"), &paths.file("session.txt"));
    if let Some((key, secs)) = library.recover_session() {
        say(&format!("     counted {secs} s of {key} from a session that was cut off"));
    }
    let mut badges = badges::Badges::load(&paths.file("stats.txt"), &paths.file("badges.txt"));
    badges.enabled = settings.badges;
    say(&format!("     {}", cpu::init()));
    say(&format!("     {}", sounds::init()));
    sounds::set_enabled(settings.ui_sounds);

    // The controller (the menus' pad; a game's mapping is set when it starts), and USB devices.
    let mut input = input::Input::new(1920, 1080, controls::Controls::base_game(&paths), cursor);
    say(&format!("     {}", input.report));
    for line in input.open_usb() {
        say(&format!("     {line}"));
    }
    input.mouse_speed = settings.mouse_multiplier();

    let mut menu = menu::Menu::new(&paths);
    {
        let mut ui = display.ui.lock().map_err(|_| "ui lock")?;
        menu.load_pictures(&mut ui, &display.device, &display.queue);
    }
    for line in &menu.report {
        say(&format!("     {line}"));
    }
    let live = menu.live(&library);
    badges.check(&live);

    let mut app = App { display, descriptors, input, paths, settings, library, badges, menu };
    let mut events = Vec::new();
    loop {
        // ---- the menu ----
        app.input.reset();
        app.menu.enter(app.settings.animated_background);
        let mut bg = make_background(&app);
        let mut editor: Option<editor::Editor> = None;
        let mut last = Instant::now();
        let _ = app.display.stats.take();
        let chosen = loop {
            let now = Instant::now();
            let dt = now.duration_since(last).as_secs_f32().min(0.1);
            last = now;
            events.clear();
            if let Some(note) = app.input.poll(dt as f64, &mut events) {
                say(&format!("ok   {note}"));
            }
            for notice in std::mem::take(&mut app.input.hid_frame.notices) {
                app.menu.toast(notice);
            }
            if let Some(mut ed) = editor.take() {
                if ed.update(dt, app.input.nav) {
                    if let Some(msg) = ed.save(say) {
                        app.menu.toast(&msg);
                        app.badges.stats.controls += 1;
                        let live = app.menu.live(&app.library);
                        app.badges.check(&live);
                    }
                    app.menu.refresh_mappings();
                    sounds::play(sounds::Sfx::Back);
                } else {
                    editor = Some(ed);
                }
            }
            let action = if editor.is_none() {
                let typed = app.input.typed.clone();
                let pad = menu::PadIn {
                    nav: app.input.nav,
                    pressed: app.input.raw_pressed,
                    down: app.input.raw_down,
                    typed: &typed,
                    backspace: app.input.backspace,
                };
                app.menu.update(dt, &pad, &mut app.settings, &mut app.library, &mut app.badges)
            } else {
                None
            };
            match action {
                Some(menu::Action::Play(i)) => break Some(i),
                Some(menu::Action::Quit) => break None,
                Some(menu::Action::EditControls(i)) => {
                    if let Some(g) = app.menu.games.get(i) {
                        let target = editor::Target::Game { stem: g.stem.clone(), name: g.name.clone() };
                        let path = g.controls_path.clone();
                        editor = Some(editor::Editor::open(&app.paths, target, Some(&path)));
                    }
                }
                Some(menu::Action::EditAllControls) => editor = Some(editor::Editor::open(&app.paths, editor::Target::AllGames, None)),
                Some(menu::Action::Rescan) => {
                    app.menu.scan(&app.paths);
                    if let Ok(mut ui) = app.display.ui.lock() {
                        app.menu.load_pictures(&mut ui, &app.display.device, &app.display.queue);
                    }
                    for line in &app.menu.report {
                        say(&format!("     {line}"));
                    }
                    let found = format!("{} game(s) found", app.menu.games.len());
                    app.menu.toast(&found);
                    let live = app.menu.live(&app.library);
                    app.badges.check(&live);
                }
                Some(menu::Action::ClearRecent) => {}
                Some(menu::Action::Setting(row)) => apply_setting(&mut app, row, &mut bg),
                None => {}
            }
            if app.input.nav != 0 {
                if let Some(b) = bg.as_mut() {
                    b.nudge();
                }
            }
            let mut scene = ui::Scene::new();
            {
                let mut ui = app.display.ui.lock().map_err(|_| "ui lock")?;
                app.menu.poll_cover(dt, &mut ui, &app.display.device, &app.display.queue);
                app.menu.draw(&mut ui, &mut scene, dt, &app.settings, &app.library, &mut app.badges, bg.is_some());
                if let Some(ed) = editor.as_mut() {
                    ed.draw(&mut ui, &mut scene, 1.0, false);
                }
            }
            match bg.as_mut() {
                Some(b) => b.frame(&app.display, dt as f64, scene),
                None => app.display.present_scene(&scene, widgets::BLUE)?,
            }
        };
        drop(bg);
        app.display.set_overlay(ui::Scene::new());
        let Some(gi) = chosen else {
            say("menu: closing (Circle held)");
            app.settings.save(&app.paths.file("settings.txt"));
            app.library.save();
            app.badges.save();
            // let the writer thread finish
            std::thread::sleep(Duration::from_millis(300));
            return Ok(());
        };

        // ---- a game (again, while the quick menu says restart) ----
        let shockwave = app.menu.games.get(gi).is_some_and(|g| g.kind == menu::Kind::Shockwave);
        loop {
            let end = if shockwave { shockwave::play(&mut app, gi) } else { game::play(&mut app, gi) };
            if end != game::GameEnd::Restart {
                break;
            }
            say("game: restarting");
        }
    }
}

#[cfg(feature = "ruffle")]
fn make_background(app: &App) -> Option<background::Background> {
    if !app.settings.animated_background {
        return None;
    }
    match background::Background::new(&app.display, &app.descriptors) {
        Ok(b) => Some(b),
        Err(e) => {
            say(&format!("warn menu background: {e} (a still one instead)"));
            None
        }
    }
}

/// A setting changed in the menu: save it, and apply what applies at once.
#[cfg(feature = "ruffle")]
fn apply_setting(app: &mut App, row: settings::Row, bg: &mut Option<background::Background>) {
    use settings::Row;
    match row {
        Row::UiSounds => sounds::set_enabled(app.settings.ui_sounds),
        Row::Badges => app.badges.enabled = app.settings.badges,
        Row::MouseSpeed => app.input.mouse_speed = app.settings.mouse_multiplier(),
        Row::AnimatedBackground => {
            if app.settings.animated_background {
                *bg = make_background(app);
            } else {
                *bg = None;
            }
        }
        _ => {}
    }
    app.settings.save(&app.paths.file("settings.txt"));
    app.badges.stats.settings += 1;
    let live = app.menu.live(&app.library);
    app.badges.check(&live);
    say(&format!("settings: {} = {}", row.label(), row.value(&app.settings)));
}
