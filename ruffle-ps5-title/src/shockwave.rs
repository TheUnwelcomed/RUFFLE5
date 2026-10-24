//! PS5_RUFFLE_SHOCKWAVE_V1 — Director and Shockwave games (.dcr, .dir, .dxr), played by DirPlayer.
//!
//! Ruffle plays Flash; a Macromedia Director movie (Shockwave on the web, a projector on a PC) is a
//! different format with its own language, Lingo. DirPlayer (github.com/igorlira/dirplayer-rs,
//! GPL-3.0) is a Director player in Rust; Ruffle5 builds it in (vendor/dirplayer, with
//! tools/dirplayer-native.patch: its native embedding API and a sound mixer without a browser) and
//! runs a game here the way src/game.rs runs a Flash one:
//!
//!   * DirPlayer loads the movie the menu chose (for a game folder, the one its projector starts),
//!     its casts and the movies it goes to from the same folder;
//!   * every score frame (at the movie's tempo) runs its Lingo; the stage is rendered on the CPU
//!     into RGBA and shown by src/shell.rs (`present_picture`): scaled to the display in whole
//!     multiples, fitted or stretched (Settings › Display › Scale mode: Unscaled = whole multiples,
//!     Zoom to Fit = fitted, Stretch = stretched, Crop = fitted), sharp at Quality Low/Medium,
//!     smooth from High up;
//!   * the pad does what the game's controls say, as for Flash (src/controls.rs) — keys go to the
//!     movie as a browser's key events would, the touchpad and the right stick move its mouse;
//!   * sound: DirPlayer's mixer (`native_audio`), drained by a thread on its own AudioOut port;
//!   * the quick menu (hold OPTIONS), the controls editor, the on-screen keyboard (L2), covers, the
//!     FPS counter and play time work as in a Flash game. The frame-time lines in the log use the
//!     same `[Game]` format ("game" is the frame's Lingo, "draw" rendering the stage).
//!
//! Everything DirPlayer does runs on this thread, inside its executor (`block_on`): its helper tasks
//! (loading casts, timeouts) only move while this loop awaits it.
// SPDX-License-Identifier: GPL-3.0-or-later

use crate::controls::Controls;
use crate::editor::{Editor, Target};
use crate::game::{self, GameEnd, Session, AUTO_COVER_AFTER, FADE_IN, FADE_OUT, PROGRESS_EVERY, QUICK_DELAY, QUICK_FILL, STATS_EVERY};
use crate::input::BTN_OPTIONS;
use crate::keyboard::Osk;
use crate::quickmenu::{self, QuickMenu};
use crate::shell::{picture_rect, Fit};
use crate::ui::{Align, Face, Rect, Scene, W};
use crate::widgets::{self, centred, draw_hold_bar, ease, Glyph, BLACK, INK, NIGHT, ORANGE};
use crate::{say, App};
use ruffle_core::events::{KeyDescriptor, LogicalKey, MouseButton, NamedKey, PlayerEvent};
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};
use vm_rust::player::native::{block_on, NativePlayer};
use vm_rust::player::native_audio;

/// Frames per second when the score sets no tempo (Director's default for a new movie is 15, but
/// DirPlayer's own loop and most Shockwave games run at 30).
const DEFAULT_TEMPO: u32 = 30;
/// At most this many score frames per loop, so a slow stretch does not snowball.
const MAX_STEPS: u32 = 3;

/// A browser `KeyboardEvent`'s (key, keyCode) for a Ruffle key: what DirPlayer takes.
pub fn js_key(k: &KeyDescriptor) -> Option<(String, u16)> {
    let named = |n: &str, c: u16| Some((n.to_string(), c));
    match k.logical_key {
        LogicalKey::Character(c) => {
            let code = match c {
                'a'..='z' => c.to_ascii_uppercase() as u16,
                'A'..='Z' | '0'..='9' => c as u16,
                ' ' => 32,
                '-' | '_' => 189,
                '=' | '+' => 187,
                '[' | '{' => 219,
                ']' | '}' => 221,
                '\\' | '|' => 220,
                ';' | ':' => 186,
                '\'' | '"' => 222,
                ',' | '<' => 188,
                '.' | '>' => 190,
                '/' | '?' => 191,
                '`' | '~' => 192,
                _ => 0,
            };
            Some((c.to_string(), code))
        }
        LogicalKey::Named(n) => match n {
            NamedKey::Enter => named("Enter", 13),
            NamedKey::Tab => named("Tab", 9),
            NamedKey::Backspace => named("Backspace", 8),
            NamedKey::Escape => named("Escape", 27),
            NamedKey::Shift => named("Shift", 16),
            NamedKey::Control => named("Control", 17),
            NamedKey::Alt => named("Alt", 18),
            NamedKey::ArrowLeft => named("ArrowLeft", 37),
            NamedKey::ArrowUp => named("ArrowUp", 38),
            NamedKey::ArrowRight => named("ArrowRight", 39),
            NamedKey::ArrowDown => named("ArrowDown", 40),
            NamedKey::PageUp => named("PageUp", 33),
            NamedKey::PageDown => named("PageDown", 34),
            NamedKey::End => named("End", 35),
            NamedKey::Home => named("Home", 36),
            NamedKey::Insert => named("Insert", 45),
            NamedKey::Delete => named("Delete", 46),
            NamedKey::F1 => named("F1", 112),
            NamedKey::F2 => named("F2", 113),
            NamedKey::F3 => named("F3", 114),
            NamedKey::F4 => named("F4", 115),
            NamedKey::F5 => named("F5", 116),
            NamedKey::F6 => named("F6", 117),
            NamedKey::F7 => named("F7", 118),
            NamedKey::F8 => named("F8", 119),
            NamedKey::F9 => named("F9", 120),
            NamedKey::F10 => named("F10", 121),
            NamedKey::F11 => named("F11", 122),
            NamedKey::F12 => named("F12", 123),
            _ => None,
        },
        LogicalKey::Unknown => None,
    }
}

/// DirPlayer's mixer on the speaker: a thread mixing 48 kHz stereo grains into its own AudioOut
/// port (as src/audio.rs does for Ruffle). Paused, it plays silence and the voices do not move.
struct DcrAudio {
    stop: Arc<AtomicBool>,
    paused: Arc<AtomicBool>,
    volume: Arc<AtomicU32>,
    thread: Option<std::thread::JoinHandle<()>>,
}

extern "C" {
    fn ps5_audio_open(frames: u32, detail: *mut i32) -> i32;
    fn ps5_audio_output(port: i32, frames: *const i16) -> i32;
    fn ps5_audio_close(port: i32) -> i32;
}

impl DcrAudio {
    fn new(volume: u8) -> Result<DcrAudio, String> {
        let mut detail = 0i32;
        let mut port = -1;
        let mut grain = 0u32;
        for g in [1024u32, 512, 256] {
            port = unsafe { ps5_audio_open(g, &mut detail) };
            if port >= 0 {
                grain = g;
                break;
            }
        }
        if port < 0 {
            return Err(format!("AudioOut refused every grain (step {port}, 0x{:08x})", detail as u32));
        }
        let stop = Arc::new(AtomicBool::new(false));
        let paused = Arc::new(AtomicBool::new(false));
        let gain = Arc::new(AtomicU32::new((volume as f32 / 10.0).to_bits()));
        let (s, p, v) = (Arc::clone(&stop), Arc::clone(&paused), Arc::clone(&gain));
        let thread = std::thread::Builder::new()
            .name("dcr-audio".into())
            .spawn(move || {
                if let Some(note) = crate::cpu::pin_audio_thread() {
                    say(&note);
                }
                let frames = grain as usize;
                let mut mix = vec![0f32; frames * 2];
                let mut out = vec![0i16; frames * 2];
                while !s.load(Ordering::Relaxed) {
                    mix.iter_mut().for_each(|x| *x = 0.0);
                    if !p.load(Ordering::Relaxed) {
                        native_audio::mix(&mut mix, 48_000);
                    }
                    let g = f32::from_bits(v.load(Ordering::Relaxed));
                    for (o, x) in out.iter_mut().zip(mix.iter()) {
                        *o = ((x * g).clamp(-1.0, 1.0) * 32767.0) as i16;
                    }
                    if unsafe { ps5_audio_output(port, out.as_ptr()) } < 0 {
                        std::thread::sleep(Duration::from_millis(frames as u64 * 1000 / 48_000));
                    }
                }
                let rc = unsafe { ps5_audio_close(port) };
                say(&format!("shockwave: sound port {port} closed (0x{:x})", rc as u32));
            })
            .map_err(|e| format!("sound thread: {e}"))?;
        say(&format!("ok   shockwave sound: AudioOut port {port}, {grain}-frame grains"));
        Ok(DcrAudio { stop, paused, volume: gain, thread: Some(thread) })
    }

    fn set_paused(&self, paused: bool) {
        self.paused.store(paused, Ordering::Relaxed);
    }

    fn set_volume(&self, volume: u8) {
        self.volume.store((volume as f32 / 10.0).to_bits(), Ordering::Relaxed);
    }
}

impl Drop for DcrAudio {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
        if let Some(t) = self.thread.take() {
            let _ = t.join();
        }
    }
}

/// Play Director game `gi` of the menu's list until it is left.
pub fn play(app: &mut App, gi: usize) -> GameEnd {
    let Some(g) = app.menu.games.get(gi) else {
        return GameEnd::Menu;
    };
    let (name, thumb, accent) = (g.name.clone(), g.thumb, g.accent);
    if let Err(e) = event_queue_ok() {
        say(&format!("FAIL {name}: {e}"));
        app.menu.toast(&format!("{name}: Shockwave cannot run here ({e})"));
        return GameEnd::Menu;
    }
    // DirPlayer reads the movie on this thread: show the loading screen for a moment first.
    let shown = Instant::now();
    let mut events = Vec::new();
    while shown.elapsed() < Duration::from_millis(350) {
        events.clear();
        let _ = app.input.poll(0.016, &mut events);
        let scene = game::loading_scene(app, &name, thumb, accent, shown.elapsed().as_secs_f32());
        let _ = app.display.present_scene(&scene, crate::widgets::BLUE_9);
    }
    block_on(run(app, gi, shown))
}

/// DirPlayer's timers (`async_std::task::sleep`, `future::timeout`) run on async-io, whose reactor
/// is a kqueue woken by an `EVFILT_USER` event (the `polling` crate's kqueue backend on FreeBSD).
/// If the console refused that, async-io would panic on its own thread, and a panic aborts the
/// title. So the same calls are made here once, the way `polling` makes them (EV_ADD|EV_RECEIPT|
/// EV_CLEAR, then a NOTE_TRIGGER, then a zero-timeout wait that must return the event), through
/// the same `kevent@FBSD_1.0` symbol the `libc` crate binds: a game that cannot run goes back to
/// the menu with the reason, written to ruffle.log.
pub fn event_queue_ok() -> Result<(), String> {
    static CHECKED: std::sync::OnceLock<Result<(), String>> = std::sync::OnceLock::new();
    CHECKED
        .get_or_init(|| {
            let r = kqueue_user_event();
            say(&format!("shockwave: kqueue EVFILT_USER check: {}", match &r { Ok(()) => "ok".to_string(), Err(e) => e.clone() }));
            r
        })
        .clone()
}

#[cfg(target_os = "freebsd")]
fn kqueue_user_event() -> Result<(), String> {
    use std::ffi::c_void;
    use std::ptr::{null, null_mut};
    /// FreeBSD 11's `struct kevent` (the libc crate's, with `libc_unstable_freebsd_version="11"`).
    #[repr(C)]
    struct KEvent {
        ident: usize,
        filter: i16,
        flags: u16,
        fflags: u32,
        data: isize,
        udata: *mut c_void,
    }
    #[repr(C)]
    struct Timespec {
        tv_sec: i64,
        tv_nsec: i64,
    }
    extern "C" {
        fn kqueue() -> i32;
        #[link_name = "kevent@FBSD_1.0"]
        fn kevent(kq: i32, changes: *const KEvent, nchanges: i32, events: *mut KEvent, nevents: i32, timeout: *const Timespec) -> i32;
        fn close(fd: i32) -> i32;
    }
    const EVFILT_USER: i16 = -11;
    const EV_ADD: u16 = 0x0001;
    const EV_CLEAR: u16 = 0x0020;
    const EV_RECEIPT: u16 = 0x0040;
    const EV_ERROR: u16 = 0x4000;
    const NOTE_TRIGGER: u32 = 0x0100_0000;
    let ev = |flags: u16, fflags: u32| KEvent { ident: 0, filter: EVFILT_USER, flags, fflags, data: 0, udata: null_mut() };
    let zero = Timespec { tv_sec: 0, tv_nsec: 0 };
    let os = || std::io::Error::last_os_error().to_string();
    // SAFETY: plain syscalls on a kqueue this function owns and closes; every pointer is a live local.
    unsafe {
        let kq = kqueue();
        if kq < 0 {
            return Err(format!("kqueue: {}", os()));
        }
        let change = |what: &str, e: KEvent| -> Result<(), String> {
            let mut got = ev(0, 0);
            let n = kevent(kq, &e, 1, &mut got, 1, &zero);
            if n < 0 {
                return Err(format!("{what}: {}", os()));
            }
            if n == 1 && got.flags & EV_ERROR != 0 && got.data != 0 {
                return Err(format!("{what}: {}", std::io::Error::from_raw_os_error(got.data as i32)));
            }
            Ok(())
        };
        let mut r = change("EVFILT_USER add", ev(EV_ADD | EV_RECEIPT | EV_CLEAR, 0));
        if r.is_ok() {
            r = change("EVFILT_USER trigger", ev(EV_ADD | EV_RECEIPT, NOTE_TRIGGER));
        }
        if r.is_ok() {
            let mut got = ev(0, 0);
            let n = kevent(kq, null(), 0, &mut got, 1, &zero);
            if n != 1 || got.filter != EVFILT_USER {
                r = Err(format!("EVFILT_USER wait: {n} events (filter {}){}", got.filter, if n < 0 { format!(": {}", os()) } else { String::new() }));
            }
        }
        close(kq);
        r
    }
}

#[cfg(not(target_os = "freebsd"))]
fn kqueue_user_event() -> Result<(), String> {
    Ok(())
}

async fn run(app: &mut App, gi: usize, shown: Instant) -> GameEnd {
    let Some(g) = app.menu.games.get(gi) else {
        return GameEnd::Menu;
    };
    let (path, controls_path, name, key, stem, own_picture) = (g.path.clone(), g.controls_path.clone(), g.name.clone(), g.key.clone(), g.stem.clone(), g.own_picture);
    say(&format!("game: {name} (Shockwave, {path})"));

    // ---- the movie ----
    let mut player = NativePlayer::new();
    if let Err(e) = player.load(&path).await {
        say(&format!("FAIL {name}: {e}"));
        app.menu.toast(&format!("{name}: {e}"));
        drop(player);
        return GameEnd::Menu;
    }
    player.init().await;
    let (mut stage_w, mut stage_h) = player.stage_size();
    say(&format!(
        "     stage {stage_w}x{stage_h}, tempo {}, loaded and started in {:.2} s",
        player.tempo(),
        shown.elapsed().as_secs_f64()
    ));

    // ---- how it is shown ----
    let s = app.settings.clone();
    let fit = match s.scale_mode {
        0 => Fit::Whole,
        2 => Fit::Stretch,
        _ => Fit::Fit,
    };
    let smooth = s.quality >= 2;
    let (dw, dh) = (app.display.width, app.display.height);
    app.input.set_viewport(stage_w, stage_h);
    app.input.set_area(Some(picture_rect(stage_w, stage_h, dw, dh, fit)));

    // ---- the controls ----
    let controls = Controls::load(&app.paths, &controls_path);
    for line in &controls.report {
        say(&format!("     {line}"));
    }
    say("     controls in effect:");
    for line in controls.summary() {
        say(&format!("       {line}"));
    }
    app.input.set_controls(controls);
    app.input.game = true;
    app.input.mouse_speed = s.mouse_multiplier();

    // ---- sound ----
    let audio = match DcrAudio::new(s.volume) {
        Ok(a) => Some(a),
        Err(e) => {
            say(&format!("warn no sound: {e}"));
            None
        }
    };

    // ---- what playing counts toward ----
    app.library.mark_started(&key);
    app.badges.stats.launches += 1;
    let hour = crate::local_hour();
    if (0..5).contains(&hour) {
        app.badges.stats.night += 1;
    } else if (5..8).contains(&hour) {
        app.badges.stats.morning += 1;
    }
    let live = app.menu.live(&app.library);
    app.badges.check(&live);
    app.badges.enabled = s.badges;

    // ---- the loop ----
    let started = Instant::now();
    let mut last = Instant::now();
    let mut paused_for = Duration::ZERO;
    let mut paused_at: Option<Instant> = None;
    let mut quick: Option<QuickMenu> = None;
    let mut editor: Option<Editor> = None;
    let mut osk = Osk::new();
    let mut hold = 0.0f32;
    let mut options_armed = true;
    let mut session = Session::default();
    let mut end = GameEnd::Menu;
    let mut toast_msg: Option<(String, f32)> = None;
    let mut need_auto_cover = s.auto_covers && !own_picture && !std::path::Path::new(&app.paths.cover(&key)).is_file();
    let mut cover_wanted = false;
    let mut cover_manual = false;
    let mut cover_saved = false;
    let mut fps_counter = s.fps_counter;
    let mut volume = s.volume;
    let mut fps_shown = 0.0f32;
    let mut fps_frames = 0u32;
    let mut fps_since = Instant::now();
    let mut last_progress = Instant::now();
    let mut last_stats = Instant::now();
    let (mut ticks, mut steps, mut drawn) = (0u64, 0u64, 0u64);
    // ms summed over the 5 s: game (score frames + input), draw (stage render), overlay
    let mut spent = [0.0f64; 3];
    let ms = |since: Instant| since.elapsed().as_secs_f64() * 1000.0;
    let _ = app.display.stats.take();
    let mut fading_out: Option<f32> = None;
    let mut owed = 0.0f64;
    let mut stopped = false;
    let mut first_frame = true;
    let mut picture: Vec<u8> = Vec::new();
    let mut events: Vec<PlayerEvent> = Vec::new();

    loop {
        let loop_start = Instant::now();
        let dt = loop_start.duration_since(last).min(Duration::from_millis(100));
        let dtf = dt.as_secs_f32();
        last = loop_start;
        events.clear();
        let menu_open = quick.is_some() || editor.is_some() || fading_out.is_some();
        app.input.pad_to_game = !osk.is_open() && !menu_open;
        if let Some(note) = app.input.poll(dt.as_secs_f64(), &mut events) {
            say(&format!("ok   {note}"));
        }
        for notice in std::mem::take(&mut app.input.hid_frame.notices) {
            toast_msg = Some((notice.to_string(), 0.0));
        }
        if !menu_open {
            session.keyboard |= app.input.hid_frame.keys.iter().any(|&(_, d)| d);
            session.mouse |= app.input.hid_frame.mouse_moved() || app.input.hid_frame.pressed != 0;
        } else {
            events.retain(|e| !matches!(e, PlayerEvent::MouseMove { .. }));
        }
        if let Some((_, t)) = toast_msg.as_mut() {
            *t += dtf;
        }

        // ---- the quick menu and the editor over the paused game ----
        if let Some(mut ed) = editor.take() {
            if ed.update(dtf, app.input.nav) {
                if let Some(msg) = ed.save(say) {
                    toast_msg = Some((msg, 0.0));
                    app.badges.stats.controls += 1;
                }
                app.input.set_controls(Controls::load(&app.paths, &controls_path));
                app.menu.refresh_mappings();
            } else {
                editor = Some(ed);
            }
        } else if let Some(mut q) = quick.take() {
            match q.update(dtf, app.input.nav) {
                quickmenu::Action::None => quick = Some(q),
                quickmenu::Action::Volume(v) => {
                    volume = v;
                    if let Some(a) = &audio {
                        a.set_volume(v);
                    }
                    quick = Some(q);
                }
                quickmenu::Action::Fps(on) => {
                    fps_counter = on;
                    quick = Some(q);
                }
                quickmenu::Action::Controls => {
                    editor = Some(Editor::open(&app.paths, Target::Game { stem: stem.clone(), name: name.clone() }, Some(&controls_path)));
                    quick = Some(q);
                }
                quickmenu::Action::Resume => {
                    resume(&audio, &mut paused_for, &mut paused_at);
                    options_armed = false;
                }
                quickmenu::Action::Cover => {
                    resume(&audio, &mut paused_for, &mut paused_at);
                    options_armed = false;
                    cover_wanted = true;
                    cover_manual = true;
                    need_auto_cover = false;
                    session.covers += 1;
                }
                quickmenu::Action::Restart => {
                    end = GameEnd::Restart;
                    fading_out = Some(0.0);
                }
                quickmenu::Action::Library => {
                    end = GameEnd::Menu;
                    fading_out = Some(0.0);
                }
            }
        }

        // ---- OPTIONS held (or Esc): the quick menu ----
        if quick.is_none() && editor.is_none() && fading_out.is_none() {
            if app.input.raw_down & BTN_OPTIONS != 0 && options_armed {
                hold += dtf;
            } else {
                hold = 0.0;
                if app.input.raw_down & BTN_OPTIONS == 0 {
                    options_armed = true;
                }
            }
            if hold >= QUICK_DELAY + QUICK_FILL || app.input.menu_request {
                hold = 0.0;
                options_armed = false;
                app.input.release_all(&mut events);
                events.extend(osk.release_all());
                send_events(&mut player, &mut events).await;
                if let Some(a) = &audio {
                    a.set_paused(true);
                }
                paused_at = Some(Instant::now());
                let secs = started.elapsed().saturating_sub(paused_for).as_secs();
                app.library.write_session(&key, secs);
                session.quick_menus += 1;
                crate::sounds::play(crate::sounds::Sfx::Select);
                quick = Some(QuickMenu::new(&name, volume, fps_counter));
                say("game: quick menu");
            }
        }

        // ---- the on-screen keyboard (L2, or what the controls bind to `keyboard`) ----
        if quick.is_none() && editor.is_none() && fading_out.is_none() {
            if app.input.keyboard_toggle {
                let was = osk.is_open();
                osk.toggle();
                if !was {
                    app.input.release_all(&mut events);
                }
            }
            let before = events.len();
            osk.update(dtf, if osk.is_open() { app.input.nav } else { 0 }, &mut events);
            if events[before..].iter().any(|e| matches!(e, PlayerEvent::KeyDown { .. })) {
                session.osk = true;
            }
        }

        // ---- the game: input, then the score frames that are due ----
        let paused = quick.is_some() || editor.is_some();
        let t = Instant::now();
        let mut stepped = false;
        if !paused && !stopped {
            send_events(&mut player, &mut events).await;
            let tempo = match player.tempo() {
                0 => DEFAULT_TEMPO,
                t => t.min(120),
            };
            owed = (owed + dt.as_secs_f64()).min(MAX_STEPS as f64 / tempo as f64);
            let period = 1.0 / tempo as f64;
            let mut n = 0;
            while (owed >= period || first_frame) && n < MAX_STEPS {
                first_frame = false;
                owed = (owed - period).max(0.0);
                n += 1;
                steps += 1;
                if !player.frame().await {
                    say(&format!("game: {name}: the movie stopped playing"));
                    stopped = true;
                    break;
                }
            }
            stepped = n > 0;
        } else {
            events.clear();
        }
        spent[0] += ms(t);
        ticks += 1;

        // ---- the stage: rendered when the score moved ----
        let t = Instant::now();
        let mut upload: Option<(u32, u32)> = None;
        if stepped || picture.is_empty() {
            let (rgba, w, h) = player.render();
            picture.clear();
            picture.extend_from_slice(rgba);
            if (w, h) != (stage_w, stage_h) {
                (stage_w, stage_h) = (w, h);
                say(&format!("     the stage is now {w}x{h}"));
                app.input.set_viewport(w, h);
                app.input.set_area(Some(picture_rect(w, h, dw, dh, fit)));
            }
            upload = Some((w, h));
            drawn += 1;
            fps_frames += 1;
            if drawn == 1 {
                say(&format!("ok   first stage drawn after {:.2} s", started.elapsed().as_secs_f64()));
            }
        }
        spent[1] += ms(t);

        // ---- covers: the stage as it is ----
        if need_auto_cover && drawn > 0 && !paused && started.elapsed().saturating_sub(paused_for) >= AUTO_COVER_AFTER {
            need_auto_cover = false;
            cover_wanted = true;
            cover_manual = false;
        }
        if cover_wanted && !picture.is_empty() {
            cover_wanted = false;
            let frame = crate::shell::Captured { width: stage_w, height: stage_h, rgba: picture.clone() };
            crate::covers::save_in_background(frame, (0, 0, stage_w, stage_h), app.paths.cover(&key));
            cover_saved = true;
            if cover_manual {
                toast_msg = Some(("Cover saved".to_string(), 0.0));
            }
        }

        // ---- the overlay ----
        let t = Instant::now();
        let mut scene = Scene::new();
        let fade_in = 1.0 - ease(started.elapsed().as_secs_f32() / FADE_IN);
        {
            let Ok(mut ui) = app.display.ui.lock() else {
                break;
            };
            let ui = &mut *ui;
            if fps_counter && !paused {
                let secs = fps_since.elapsed().as_secs_f32();
                if secs >= 0.5 {
                    fps_shown = fps_frames as f32 / secs;
                    fps_frames = 0;
                    fps_since = Instant::now();
                }
                let label = format!("{fps_shown:.0} FPS");
                let w = ui.measure(Face::SemiBold, 22.0, &label, 0.0) + 32.0;
                let r = Rect::new(24.0, 24.0, w, 42.0);
                scene.rect(r, 21.0, NIGHT.with_alpha(0.78));
                scene.text(ui, Face::SemiBold, 22.0, &label, r.cx(), centred(r.cy(), 22.0), ORANGE, Align::Center, 0.0);
            }
            if hold > QUICK_DELAY {
                draw_hold_bar(ui, &mut scene, hold, QUICK_DELAY, QUICK_FILL, "Keep holding for the quick menu", Glyph::Chip("OPTIONS"), 842.0);
            }
            osk.draw(ui, &mut scene, "Keyboard");
            if let Some(q) = &quick {
                q.draw(ui, &mut scene);
            }
            if let Some(ed) = editor.as_mut() {
                ed.draw(ui, &mut scene, 1.0, true);
            }
            if let Some((msg, t)) = &toast_msg {
                widgets::toast(ui, &mut scene, msg, *t, 2.2);
            }
            app.badges.draw_popup(ui, &mut scene, dtf);
            if fade_in > 0.0 {
                scene.rect(Rect::new(0.0, 0.0, W, crate::ui::H), 0.0, BLACK.with_alpha(fade_in));
            }
            if let Some(f) = fading_out.as_mut() {
                *f += dtf;
                widgets::ribbon_wipe(&mut scene, *f / FADE_OUT);
            }
            if stopped && quick.is_none() && editor.is_none() && fading_out.is_none() {
                let msg = "The movie ended. Hold OPTIONS for the quick menu.";
                scene.text(ui, Face::SemiBold, 28.0, msg, W * 0.5, crate::ui::H - 80.0, INK, Align::Center, 0.0);
            }
        }
        app.display.set_overlay(scene);
        if paused || osk.is_open() {
            app.input.set_cursor_visible(false);
        }
        spent[2] += ms(t);

        // ---- shown: every loop (the overlay and the pointer move even when the stage does not) ----
        let pic = upload.map(|(w, h)| (&picture[..], w, h));
        if let Err(e) = app.display.present_picture(pic, fit, smooth) {
            say(&format!("warn present: {e}"));
        }

        // ---- progress, the frame-time log ----
        if last_progress.elapsed() >= PROGRESS_EVERY {
            let secs = started.elapsed().saturating_sub(paused_for).as_secs();
            app.library.write_session(&key, secs);
            last_progress = Instant::now();
        }
        if last_stats.elapsed() >= STATS_EVERY {
            let window = last_stats.elapsed().as_secs_f64();
            let d = app.display.stats.take();
            say(&format!(
                "[Game] 5 s: {ticks} ticks, {drawn} frames drawn, 0 read back, 0 readbacks failed, root frame Some({}), playing {}",
                player.current_frame(),
                !stopped && !paused
            ));
            let per = |v: f64| v / ticks.max(1) as f64;
            say(&format!(
                "[Game] ms per loop: game {:.1}, jobs {:.1}, draw {:.1} | per frame drawn: readback {:.1} | overlay {:.1}, present {:.1}",
                per(spent[0]),
                0.0,
                per(spent[1]),
                0.0,
                per(spent[2] + d.overlay_ms),
                per(d.acquire_ms + d.present_ms),
            ));
            say(&format!(
                "[Game] Shockwave stage {stage_w}x{stage_h} -> {}x{} ({:?}, {}): {:.1} score frames/s, {:.1} presents/s, {} sounds playing",
                dw,
                dh,
                fit,
                if smooth { "smooth" } else { "sharp" },
                steps as f64 / window,
                d.presents as f64 / window,
                native_audio::playing()
            ));
            (ticks, steps, drawn) = (0, 0, 0);
            spent = [0.0; 3];
            last_stats = Instant::now();
        }

        // ---- leaving ----
        if let Some(f) = fading_out {
            if f >= FADE_OUT {
                break;
            }
        }
    }

    // ---- let go of everything, end ----
    let mut release = Vec::new();
    app.input.release_all(&mut release);
    release.extend(osk.release_all());
    send_events(&mut player, &mut release).await;
    drop(audio);
    drop(player);
    app.display.set_overlay(Scene::new());
    app.display.release_offscreen();
    app.input.reset();
    let in_menu = paused_at.map(|t| t.elapsed()).unwrap_or(Duration::ZERO);
    let secs = started.elapsed().saturating_sub(paused_for + in_menu).as_secs();
    app.library.add_play_time(&key, secs);
    app.library.clear_session();
    let st = &mut app.badges.stats;
    st.play_secs += secs;
    st.longest_secs = st.longest_secs.max(secs);
    st.covers += session.covers;
    st.quick_menus += session.quick_menus;
    st.keyboard += session.keyboard as u64;
    st.mouse += session.mouse as u64;
    st.osk += session.osk as u64;
    if end == GameEnd::Restart {
        st.restarts += 1;
    }
    let live = app.menu.live(&app.library);
    app.badges.check(&live);
    if volume != app.settings.volume || fps_counter != app.settings.fps_counter {
        app.settings.volume = volume;
        app.settings.fps_counter = fps_counter;
        app.settings.save(&app.paths.file("settings.txt"));
    }
    if cover_saved {
        app.menu.expect_cover(gi, app.paths.cover(&key));
    }
    say(&format!("game: {name} closed after {secs} s ({:?})", end));
    end
}

/// The input of this loop to the movie: keys as browser key events, the left button as its mouse.
async fn send_events(player: &mut NativePlayer, events: &mut Vec<PlayerEvent>) {
    for e in events.drain(..) {
        match e {
            PlayerEvent::KeyDown { key } => {
                if let Some((k, c)) = js_key(&key) {
                    player.key_down(&k, c).await;
                }
            }
            PlayerEvent::KeyUp { key } => {
                if let Some((k, c)) = js_key(&key) {
                    player.key_up(&k, c).await;
                }
            }
            PlayerEvent::MouseMove { x, y } => player.mouse_move(x as i32, y as i32).await,
            PlayerEvent::MouseDown { x, y, button: MouseButton::Left, .. } => player.mouse_down(x as i32, y as i32).await,
            PlayerEvent::MouseUp { x, y, button: MouseButton::Left } => player.mouse_up(x as i32, y as i32).await,
            _ => {}
        }
    }
}

fn resume(audio: &Option<DcrAudio>, paused_for: &mut Duration, paused_at: &mut Option<Instant>) {
    if let Some(at) = paused_at.take() {
        *paused_for += at.elapsed();
    }
    if let Some(a) = audio {
        a.set_paused(false);
    }
    say("game: resumed");
}

#[cfg(test)]
mod tests {
    use super::*;
    use ruffle_core::events::{KeyLocation, PhysicalKey};

    fn key(logical: LogicalKey) -> KeyDescriptor {
        KeyDescriptor {
            physical_key: PhysicalKey::Unknown,
            logical_key: logical,
            key_location: KeyLocation::Standard,
        }
    }

    #[test]
    fn keys_reach_dirplayer_as_a_browser_names_them() {
        assert_eq!(js_key(&key(LogicalKey::Character('a'))), Some(("a".into(), 65)));
        assert_eq!(js_key(&key(LogicalKey::Character(' '))), Some((" ".into(), 32)));
        assert_eq!(js_key(&key(LogicalKey::Named(NamedKey::ArrowLeft))), Some(("ArrowLeft".into(), 37)));
        assert_eq!(js_key(&key(LogicalKey::Named(NamedKey::Enter))), Some(("Enter".into(), 13)));
        assert_eq!(js_key(&key(LogicalKey::Unknown)), None);
    }

    #[test]
    fn a_stage_fits_the_display() {
        // 4.5x fits; whole multiples: 4x, centred
        assert_eq!(picture_rect(640, 480, 3840, 2160, Fit::Whole), [640.0, 120.0, 2560.0, 1920.0]);
        assert_eq!(picture_rect(640, 480, 3840, 2160, Fit::Fit), [480.0, 0.0, 2880.0, 2160.0]);
        assert_eq!(picture_rect(640, 480, 3840, 2160, Fit::Stretch), [0.0, 0.0, 3840.0, 2160.0]);
    }
}
