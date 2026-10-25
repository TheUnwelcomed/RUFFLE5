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
//! 1.000.021 (2026-10-24): Shockwave / Director games (.dcr .dir .dxr, or a game's folder) played
//! by DirPlayer (src/shockwave.rs, src/dirinfo.rs), listed in the menu beside the Flash games.
//!
//! 1.000.022 (2026-10-24): USB keyboard and mouse load as system modules first thing in main() and
//! are reached through libSceSysmodule's handle (shell/hid.c V2); a game folder is also found one
//! folder down, and the log names every folder the menu skips (src/menu.rs V4).
//!
//! 1.000.023 (2026-10-24): USB keyboard and mouse functions imported directly (shell/hid.c V3; the
//! console has no sceSysmoduleGetModuleHandleInternal); a one-time CPU/memory benchmark and per-stage
//! draw costs in the log for Shockwave games (src/shockwave.rs), to find why drawing is slow there.
//!
//! 1.000.024 (2026-10-24): Shockwave games run one score frame, then the stage (Director's model;
//! a game that paces itself, as Battle Blitz does, showed only one frame in three); DirPlayer's
//! per-frame steps after a frame — the sound channels' tick (a sound's end, so channels free up and
//! music loops: sound stopped about a minute in), `idle`, cue points — run off the web too; sprites
//! drawn upright take a per-row path (3 to 4 times faster); the log names the first USB key and
//! mouse movement.
//!
//! 1.000.025 (2026-10-24): USB keyboard and mouse V4 (src/hid.rs, shell/hid.c): the log shows what
//! the console answers each handle (kinds of read answer, first records with their bytes,
//! connections, the system holding the keyboard); `sceKeyboardReadState` is tried while
//! `sceKeyboardRead` gives nothing; handles are opened again every 5 s while no device has shown
//! up on them (a keyboard plugged in after start did nothing in 1.000.023).
//!
//! 1.000.026 (2026-10-24): Shockwave film loops (DirPlayer patch): a film loop a script puts on a
//! sprite takes the loop's own size (Battle Blitz's opponent portrait was drawn 2.5 times too large:
//! the pink block over the fighter grid), and a see-through layer inside a film loop stays
//! see-through (the pulsing highlight over the chosen fighters hid their portraits under a dark box).
//!
//! 1.000.027 (2026-10-24): the Online tab (src/online.rs, net/): Flash games and movies from the
//! Internet Archive's Flash collection and Flash Museum, searched and fetched into the games folder
//! from the menu, over HTTPS (rustls with RustCrypto) on the console's sockets; `sceNetInit` from the
//! shell (V6) before the first socket; a rescan reuses the GPU pictures it replaces.
//!
//! 1.000.028 (2026-10-25): Shockwave (DirPlayer patch): text in fonts a movie does not carry drawn
//! with the Liberation fonts as a browser's canvas measures it, DirPlayer's system font, the event
//! loop after each frame (mouseEnter / mouseLeave / mouseWithin), a film loop's own `go`, what a
//! movie asks of a browser answered with a notice instead of stopping the title, setPref/getPref in
//! the game's saves folder, file names found whatever their case. The Online tab: Flashpoint
//! Archive's Shockwave games (net/src/fp.rs; its game data zips checked against their SHA-256 and
//! unpacked, net/src/zip.rs), started as the archive starts them (online.txt `launch`, `url`,
//! `webroot`: src/shockwave.rs); archive.org without its sub-collections; Flash Museum through its
//! player's settings. The menu: OPTIONS deletes a game; Settings › Library › Keep online games.
//!
//! 1.000.029 (2026-10-25): the Online tab is Flash Browser, last before Settings. Its names resolve:
//! the title's getaddrinfo is the console's resolver (the kit's evo_dns.c, sceNetResolver*) — the
//! RADV link recipe had bound it to the platform layer's stand-in, which answers every lookup
//! EAI_FAIL (build-title.sh V10) — and the resolver's lines are in ruffle.log (shell V7).
//!
//! 1.000.030 (2026-10-25): Shockwave 3D. DirPlayer moves to its cham/w3d-fullscreen branch
//! (b9b9466: main plus its Shockwave 3D work), and the patch adds `rendering_gpu/softgl`: DirPlayer's
//! own WebGL2 3D renderer (scene3d.rs, unchanged but for its imports) runs on a software WebGL2 —
//! the GL pipeline in Rust, its four GLSL programs ported — and the stage compositor puts each 3D
//! sprite where DirPlayer's WebGL compositor does. Checked by replaying a game's recorded WebGL
//! calls in Chromium's WebGL2 with the original GLSL (`dcr-probe --softgl-replay`). The pixels are
//! written by up to 8 threads (the CPUs the CPU layout leaves the game); ruffle.log gets a
//! "Shockwave 3D" line with the per-frame times. Supersonic RC: the ceiling slabs drew black — a
//! lightmap named "...LightingMap" behind an empty first texture slot was taken for the diffuse
//! texture (DirPlayer fix in the patch).
//!
//! 1.000.031 (2026-10-25): the on-screen keyboard's own settings, per game or for every game in the
//! mapping files (`keyboard_size`, `keyboard_position`, `keyboard_game`, `keyboard_opacity`):
//! four sizes, eight places, the game under it or shown smaller beside it (src/shell.rs V4: the
//! game's frame drawn into the free part of the screen, a Flash stage without its letterbox), and
//! how solid it is; ready-made settings for kinds of game (typing a name, RPG and turn-based, typing
//! games, a key now and then). The editor's keyboard page (Settings › On-screen keyboard, the quick
//! menu's Keyboard for this game, a row of each controls editor) has a map of the screen and a
//! full-size preview. L1 on the open keyboard moves it on round the screen, R1 across. Shockwave 3D:
//! fewer uniform lookups per draw, the bone palette only for skinned draws, and the texture blend
//! in SSE2 (the same pixels, bit for bit, on the Supersonic RC replay).
//!
//! 1.000.032 (2026-10-25): what the console logs had shown. Shockwave film loops are sized as
//! DirPlayer's WebGL2 renderer sizes them (the loop's own rect): Trick or Treat Beat!'s powers —
//! each a film loop on the character — drew squeezed to a few dots, and the character vanished
//! while one played. A sprite whose foreColor/backColor are set but left at black and white takes
//! the upright blit fast path with a 32-bit picture (Supersonic RC's stage: 11–13 ms to 2 ms a frame
//! on a PC; the same pixels, compared frame for frame). The console refused every removal of a
//! game folder (Delete, Keep online games Off, a failed fetch: "Operation not permitted"), while
//! renaming works: what cannot be removed is moved into the games folder's `.ruffle5-trash`, which
//! the menu does not list and each start tries to empty; the log says, call by call, what removing
//! answers (src/fsutil.rs). ruffle.log keeps the runs before (shell V8: trimmed to its last 512 KB
//! past 1 MB, each run headed with its date); a watchdog names the step of a game's loop that takes
//! 5 s or more (src/watchdog.rs). A folder with a Director movie and a .swf plays the movie.
//! Flash Browser lists the Internet Archive and Flashpoint: Flash Museum's file server refused
//! the movies (403 Forbidden), so it is no longer a source.
//!
//! 1.000.033 (2026-10-25): Flashpoint's Flash games in Flash Browser ("Flashpoint Flash", beside
//! "Flashpoint Shockwave"), those Flashpoint's own launcher plays with Ruffle marked "Ruffle". A
//! game from its game data plays as if from its address (online.txt `url`, ahead of the Pretend
//! Site setting now), and the files it loads by web address are read from the game data's
//! `content/` (src/navigator.rs), as Flashpoint's proxy serves them. A Flashpoint search that
//! finds nothing is asked again without the platform and says what the archive has and where
//! (1.000.032 searched Shockwave only). The menu's categories (R2): All, Flash, Shockwave,
//! Shockwave 3D (src/dir3d.rs) and Animations (Flashpoint's, and an Animations folder). The search
//! keyboard shows what is typed and has settings of its own (Settings > Search keyboard).
//!
//! 1.000.034 (2026-10-25): what the 1.000.032 console logs had shown. The console refuses `lstat`
//! to Ruffle5 in the games folder and /data/ruffle5 (EPERM), while stat, readdir, unlink, rmdir and
//! rename work. 1.000.032 told a folder from a file, and whether a removal had worked, by `lstat`,
//! so a refused `lstat` read as "gone": Delete and Keep online games Off logged "menu: deleted ..."
//! and left the game there (the menu's count stayed the same). Removing now lists the folder and
//! unlinks and rmdirs what is in it, and a game is gone only when stat no longer finds it
//! (src/fsutil.rs). A Flashpoint game's files that are not on the console are asked of Flashpoint's
//! server when the game asks for them, and kept (src/fpfetch.rs), as Flashpoint's own game server
//! does: Golfish and Golf Ace's wrapper loads gameloader.dcr from beside it ("[go] eager mount of
//! gameloader.dcr FAILED"). DirPlayer: mouseEnter / mouseLeave go to the front-most sprite with a
//! script (Teen Titans Battle Blitz's select screen flickered: a fighter's highlight, shown on
//! mouseEnter, covered its tile); `the idleHandlerPeriod` and the other idle* movie properties are
//! kept (Virus Raider's startMovie stopped at "Symbol 'idleHandlerPeriod' is not a built-in
//! symbol"); a file a movie asks for that is not there is named in ruffle.log.
//!
//! 1.000.035 (2026-10-25): Pretend to be on for Shockwave games too, Shockwave.com among the sites,
//! and Automatic: each game is told it runs on the site its own file names, read from the movie
//! (src/pretend.rs). DirPlayer: under a movie address, a movie the first one goes to keeps its own
//! name (`the movieName` stayed the wrapper's: Battle Blitz given an address stopped at its title),
//! and `getStreamStatus` / `netDone` by address find what `preloadNetThing` asked for. Keep online
//! games Off deletes only a game fetched in this run (1.000.034 also deleted games fetched before:
//! Pacman).
//!
//! Progress is written to /data/ruffle5/ruffle.log by the shell's `ps5_log`.
// SPDX-License-Identifier: GPL-3.0-or-later

use std::ffi::{c_char, c_int, CStr};
use std::time::{Duration, Instant};

/// The title's version, as Settings shows it.
pub const APP_VERSION: &str = "1.000.035";
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
mod dir3d;
#[cfg(feature = "ruffle")]
mod dirinfo;
#[cfg(feature = "ruffle")]
mod fpfetch;
#[cfg(feature = "ruffle")]
mod pretend;
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
mod navigator;
#[cfg(feature = "ruffle")]
mod online;
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
mod watchdog;
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
    // 1.000.032: what removing answers in the games folder (Delete, a failed fetch) and in Ruffle5's
    // own folder, call by call: one line each. What waits in a trash folder is tried again.
    say(&format!("     {}", fsutil::check_removal(&paths.games)));
    say(&format!("     {}", fsutil::check_removal(&paths.data)));
    for root in menu::scan_roots(&paths.games) {
        if let Some(line) = fsutil::empty_trash(&root) {
            say(&format!("     {line}"));
        }
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
                Some(menu::Action::EditKeyboard) => editor = Some(editor::Editor::open_keyboard(&app.paths, editor::Target::AllGames, None)),
                Some(menu::Action::EditSearchKeyboard) => editor = Some(editor::Editor::open_keyboard(&app.paths, editor::Target::Search, None)),
                Some(menu::Action::Rescan) => {
                    rescan(&mut app);
                    let found = format!("{} game(s) found", app.menu.games.len());
                    app.menu.toast(&found);
                }
                Some(menu::Action::Added(_)) => rescan(&mut app),
                Some(menu::Action::PlayFile(path)) => {
                    rescan(&mut app);
                    if !app.menu.launch_path(&path) {
                        say(&format!("menu: {path} is not among the games after the rescan"));
                        app.menu.toast("That game is not in the list (see ruffle.log)");
                    }
                }
                Some(menu::Action::ClearRecent) => {}
                Some(menu::Action::Setting(row)) => apply_setting(&mut app, row, &mut bg),
                Some(menu::Action::Delete(gi)) => {
                    delete_game(&mut app, gi, "");
                }
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
                app.menu.poll_online(&mut ui, &app.display.device, &app.display.queue);
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
        // 1.000.028: Settings › Library › Keep online games Off: a game the Online tab fetched goes
        // when it is left. 1.000.035: one fetched in this run only — a game already in the library
        // stays (1.000.034 deleted Pacman, fetched in an earlier run, when it was left)
        let fetched_now = app.menu.games.get(gi).is_some_and(|g| g.online && app.menu.online.fetched_in_this_run(&g.path));
        if let Some(g) = app.menu.games.get(gi).filter(|g| !app.settings.keep_online && !fetched_now && g.online) {
            say(&format!("menu: {} kept: Keep online games is Off, but it was in the library before this run", g.name));
        }
        if !app.settings.keep_online && fetched_now {
            delete_game(&mut app, gi, " (Keep online games is Off)");
        }
    }
}

/// Delete game `gi` (Menu::delete_game: its folder or file; saves stay), look again, and say so.
#[cfg(feature = "ruffle")]
fn delete_game(app: &mut App, gi: usize, why: &str) {
    let Some(name) = app.menu.games.get(gi).map(|g| g.name.clone()) else {
        return;
    };
    match app.menu.delete_game(gi) {
        Ok((gone, trashed)) => {
            say(&format!("menu: deleted {name}{why}: {gone}"));
            let focus = app.menu.focus();
            rescan(app);
            app.menu.set_focus(focus);
            if trashed {
                // 1.000.032: the console refused removing the files; they wait in the trash folder
                app.menu.toast(&format!("Deleted {name}{why}: its files wait in games/{} (see ruffle.log)", fsutil::TRASH));
            } else {
                app.menu.toast(&format!("Deleted {name}{why}"));
            }
        }
        Err(e) => {
            say(&format!("warn menu: {name} not deleted: {e}"));
            app.menu.toast(&format!("{name} was not deleted (see ruffle.log)"));
        }
    }
}

/// Look for games again (the Settings row, or a game the Online tab fetched).
#[cfg(feature = "ruffle")]
fn rescan(app: &mut App) {
    app.menu.scan(&app.paths);
    if let Ok(mut ui) = app.display.ui.lock() {
        app.menu.load_pictures(&mut ui, &app.display.device, &app.display.queue);
    }
    for line in &app.menu.report {
        say(&format!("     {line}"));
    }
    let live = app.menu.live(&app.library);
    app.badges.check(&live);
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
