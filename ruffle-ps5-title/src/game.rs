//! PS5_RUFFLE_GAME_V1 — one game, from the loading screen to the way back to the menu.
//!
//! The movie is read and parsed on a worker while the loading screen's spinner turns. The player
//! gets Ruffle's own options from Settings (quality, scale mode, letterbox, alignment, frame rate,
//! Flash version, runtime, load behaviour, script time limit, a pretend site for sitelocked games,
//! a dummy ExternalInterface), FLV video, saves on disk (src/saves.rs), the UI hooks
//! (src/ps5ui.rs), and renders at 3840x2160 (Settings > Resolution: 4K) or 1920x1080.
//!
//! While it plays: holding OPTIONS (or Esc on a USB keyboard) opens the quick menu over the paused
//! game (src/quickmenu.rs) — resume, restart, volume, this game's controls editor, a cover picture,
//! the FPS counter, back to games. A button bound to `keyboard` (L2) opens the on-screen keyboard
//! (src/keyboard.rs), and a game's text box does too. Saves are flushed every 3 s and when the quick
//! menu opens; the play time is kept every 30 s in case the console is switched off.
//!
//! Every 5 s the log gets the frame-time lines in Ruffle-Flash-PS5's format, so the two can be
//! compared on the same game:
//!   [Game] 5 s: N ticks, N frames drawn, N read back, N readbacks failed, root frame .., playing ..
//!   [Game] ms per loop: game x, jobs x, draw x | per frame drawn: readback x | overlay x, present x
//! "game" is Ruffle's tick (ActionScript, timeline), "jobs" its background executor, "draw" building
//! and submitting the frame, "readback" copying a frame to the CPU (only for a cover picture here),
//! "overlay" the pointer/menus/blit passes, "present" waiting for a swapchain image and presenting.
// SPDX-License-Identifier: GPL-3.0-or-later

use crate::controls::Controls;
use crate::editor::{Editor, Target};
use crate::input::BTN_OPTIONS;
use crate::keyboard::Osk;
use crate::quickmenu::{self, QuickMenu};
use crate::settings::{self, Settings};
use crate::ui::{Align, Face, Rect, Scene, W};
use crate::widgets::{self, centred, draw_hold_bar, ease, Glyph, BLACK, BLUE_2, BLUE_6, BLUE_9, INK, NIGHT, ORANGE};
use crate::{say, App};
use ruffle_core::backend::audio::{AudioBackend, NullAudioBackend};
use ruffle_core::backend::navigator::{NullExecutor, NullNavigatorBackend};
use ruffle_core::config::Letterbox;
use ruffle_core::external::{ExternalInterfaceProvider, Value as ExternalValue};
use ruffle_core::tag_utils::SwfMovieData;
use ruffle_core::{LoadBehavior, PlayerBuilder, PlayerRuntime, StageAlign, StageScaleMode};
use ruffle_render::quality::StageQuality;
use ruffle_render_wgpu::backend::WgpuRenderBackend;
use ruffle_video_software::backend::SoftwareVideoBackend;
use std::time::{Duration, Instant};

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum GameEnd {
    Menu,
    Restart,
}

/// Hold OPTIONS this long for the quick menu (the bar shows after the delay).
pub(crate) const QUICK_DELAY: f32 = 0.3;
pub(crate) const QUICK_FILL: f32 = 0.5;
pub(crate) const FADE_IN: f32 = 0.35;
pub(crate) const FADE_OUT: f32 = 0.35;
pub(crate) const AUTO_COVER_AFTER: Duration = Duration::from_secs(8);
const SAVE_EVERY: Duration = Duration::from_secs(3);
pub(crate) const PROGRESS_EVERY: Duration = Duration::from_secs(30);
pub(crate) const STATS_EVERY: Duration = Duration::from_secs(5);
/// A text box asking for the keyboard opens it after its focus settled this long.
const KEYBOARD_SETTLE: Duration = Duration::from_millis(400);

/// Ruffle desktop's dummy ExternalInterface (desktop/src/backends/external_interface.rs at the
/// pinned revision): tells games the web page is there and answers their calls with nothing,
/// except the page's address, which is the pretend site when one is set.
struct DummyExternalInterface {
    spoof_url: Option<String>,
}

fn is_location_href(code: &str) -> bool {
    matches!(code, "document.location.href" | "window.location.href" | "top.location.href")
}

impl ExternalInterfaceProvider for DummyExternalInterface {
    fn call_method(&self, _context: &mut ruffle_core::context::UpdateContext<'_>, name: &str, args: &[ExternalValue]) -> ExternalValue {
        if let Some(url) = &self.spoof_url {
            if let Some(stripped) = name.strip_suffix(".toString") {
                if is_location_href(stripped) {
                    return ExternalValue::String(url.clone());
                }
            }
            if name == "eval" {
                if let [ExternalValue::String(code)] = args {
                    if is_location_href(code) {
                        return ExternalValue::String(url.clone());
                    }
                }
            }
        }
        ExternalValue::Undefined
    }

    fn on_callback_available(&self, _name: &str) {}

    fn get_id(&self) -> Option<String> {
        None
    }
}

/// What one run of a game did, for play time and badges.
#[derive(Default, Clone, Copy)]
pub(crate) struct Session {
    pub(crate) covers: u64,
    pub(crate) quick_menus: u64,
    pub(crate) keyboard: bool,
    pub(crate) mouse: bool,
    pub(crate) osk: bool,
}

/// The loading screen: Ruffle blue, the game's picture glowing with ProsperoStore's breathing
/// halo and a light running round its border, its name, and a spinner.
pub(crate) fn loading_scene(app: &mut App, name: &str, thumb: Option<crate::ui::ImageId>, accent: crate::ui::Color, t: f32) -> Scene {
    let mut scene = Scene::new();
    if let Ok(mut ui) = app.display.ui.lock() {
        let ui = &mut *ui;
        let a = ease(t / 0.25);
        scene.gradient_v(Rect::new(0.0, 0.0, W, crate::ui::H), 0.0, BLUE_6, BLUE_9);
        scene.push_opacity(a);
        let cover = Rect::new(W * 0.5 - 200.0, 170.0 + 6.0 * (t * 1.6).sin(), 400.0, 225.0).scaled(0.94 + 0.06 * a);
        widgets::focus_halo(&mut scene, cover, 28.0, 1.0, t, crate::ui::Color::mix(accent, ORANGE, 0.5));
        match thumb {
            Some(id) => {
                let size = ui.image_size(id);
                scene.image_cover(id, size, cover, 28.0, crate::ui::Color::rgb(0xffffff));
            }
            None => {
                scene.gradient_v(cover, 28.0, crate::ui::Color::mix(accent, BLUE_6, 0.35), crate::ui::Color::mix(accent, NIGHT, 0.6));
                scene.text(ui, Face::Display, 96.0, &widgets::initials(name), cover.cx(), cover.cy() + 34.0, INK, Align::Center, 2.0);
            }
        }
        widgets::focus_ring(&mut scene, cover, 28.0, 1.0, t);
        let title = ui.fit(Face::Display, 60.0, name, W - 400.0);
        scene.text(ui, Face::Display, 60.0, &title, W * 0.5, 500.0, INK, Align::Center, 0.0);
        widgets::spinner(&mut scene, W * 0.5, 600.0, 34.0, t, ORANGE);
        scene.text(ui, Face::SemiBold, 22.0, "Loading", W * 0.5, 686.0, BLUE_2, Align::Center, 3.0);
        scene.pop_opacity();
        // the menu's ribbon wipe, running back off the screen
        widgets::ribbon_wipe(&mut scene, 1.0 - t / 0.4);
    }
    scene
}

/// Play game `gi` of the menu's list until it is left. `restarts` counts the quick menu's restarts.
pub fn play(app: &mut App, gi: usize) -> GameEnd {
    let Some(game) = app.menu.games.get(gi) else {
        return GameEnd::Menu;
    };
    let (path, name, key, stem, own_picture, thumb, accent) = (game.path.clone(), game.name.clone(), game.key.clone(), game.stem.clone(), game.own_picture, game.thumb, game.accent);
    say(&format!("game: {name} ({path})"));

    // ---- load on a worker, the spinner turning ----
    let job_path = path.clone();
    let job = std::thread::Builder::new().name("load".into()).spawn(move || -> Result<SwfMovieData, String> {
        let data = std::fs::read(&job_path).map_err(|e| format!("cannot read it: {e}"))?;
        SwfMovieData::from_data(&data, format!("file://{job_path}"), None, None).map_err(|e| format!("not a movie Ruffle can read: {e}"))
    });
    let job = match job {
        Ok(j) => j,
        Err(e) => {
            say(&format!("FAIL loader thread: {e}"));
            return GameEnd::Menu;
        }
    };
    let shown = Instant::now();
    let mut events = Vec::new();
    while !job.is_finished() || shown.elapsed() < Duration::from_millis(500) {
        events.clear();
        let _ = app.input.poll(0.016, &mut events);
        let scene = loading_scene(app, &name, thumb, accent, shown.elapsed().as_secs_f32());
        let _ = app.display.present_scene(&scene, BLUE_9);
    }
    let movie = match job.join() {
        Ok(Ok(m)) => m,
        Ok(Err(e)) => {
            say(&format!("FAIL {name}: {e}"));
            app.menu.toast(&format!("{name}: {e}"));
            return GameEnd::Menu;
        }
        Err(_) => {
            say(&format!("FAIL {name}: the loader crashed"));
            app.menu.toast(&format!("{name} could not be loaded"));
            return GameEnd::Menu;
        }
    };
    let movie_w = movie.width().to_pixels().max(1.0) as u32;
    let movie_h = movie.height().to_pixels().max(1.0) as u32;
    say(&format!("     movie {movie_w}x{movie_h} at {:.0} fps, SWF {}, loaded in {:.2} s", movie.frame_rate().to_f32(), movie.version(), shown.elapsed().as_secs_f64()));

    // ---- the size it renders at ----
    let s: Settings = app.settings.clone();
    let (dw, dh) = (app.display.width, app.display.height);
    let mut size = if s.uhd { (dw, dh) } else { (1920.min(dw), 1080.min(dh)) };

    // ---- the controls ----
    let controls = Controls::load(&app.paths, &path);
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

    // ---- the player ----
    let renderer = match WgpuRenderBackend::new(std::sync::Arc::clone(&app.descriptors), app.display.target(size.0, size.1)) {
        Ok(r) => r,
        Err(e) if size != (1920.min(dw), 1080.min(dh)) => {
            say(&format!("warn renderer at {}x{}: {e}; trying 1080p", size.0, size.1));
            size = (1920.min(dw), 1080.min(dh));
            match WgpuRenderBackend::new(std::sync::Arc::clone(&app.descriptors), app.display.target(size.0, size.1)) {
                Ok(r) => r,
                Err(e) => {
                    say(&format!("FAIL renderer: {e}"));
                    app.input.reset();
                    return GameEnd::Menu;
                }
            }
        }
        Err(e) => {
            say(&format!("FAIL renderer: {e}"));
            app.input.reset();
            return GameEnd::Menu;
        }
    };
    app.input.set_viewport(size.0, size.1);
    let uhd = size.0 > 1920;
    let audio: Box<dyn AudioBackend> = match crate::audio::Ps5AudioBackend::new(say) {
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
    let base = std::path::Path::new(&path).parent().map(|p| p.to_path_buf()).unwrap_or_default();
    let navigator = match NullNavigatorBackend::with_base_path(&base, &executor) {
        Ok(n) => n,
        Err(e) => {
            say(&format!("warn navigator base path {}: {e} (relative loads off)", base.display()));
            NullNavigatorBackend::new()
        }
    };
    let ui_state = std::sync::Arc::new(crate::ps5ui::UiState::default());
    let quality = [
        StageQuality::Low,
        StageQuality::Medium,
        StageQuality::High,
        StageQuality::Best,
        StageQuality::High8x8,
        StageQuality::High8x8Linear,
        StageQuality::High16x16,
        StageQuality::High16x16Linear,
    ][s.quality as usize];
    let scale_mode = [StageScaleMode::NoScale, StageScaleMode::ShowAll, StageScaleMode::ExactFit, StageScaleMode::NoBorder][s.scale_mode as usize];
    let align = [
        StageAlign::empty(),
        StageAlign::TOP,
        StageAlign::BOTTOM,
        StageAlign::LEFT,
        StageAlign::RIGHT,
        StageAlign::TOP | StageAlign::LEFT,
        StageAlign::TOP | StageAlign::RIGHT,
        StageAlign::BOTTOM | StageAlign::LEFT,
        StageAlign::BOTTOM | StageAlign::RIGHT,
    ][s.align as usize];
    let mut builder = PlayerBuilder::new()
        .with_renderer(renderer)
        .with_boxed_audio(audio)
        .with_video(SoftwareVideoBackend::new())
        .with_navigator(navigator)
        .with_storage(Box::new(crate::saves::DiskStorage::new(&app.paths.saves(&key))))
        .with_ui(crate::ps5ui::Ps5Ui { state: std::sync::Arc::clone(&ui_state) })
        .with_movie(movie)
        .with_viewport_dimensions(size.0, size.1, size.0 as f64 / W as f64)
        .with_quality(quality)
        .with_scale_mode(scale_mode, s.force_scale)
        .with_letterbox([Letterbox::On, Letterbox::Fullscreen, Letterbox::Off][s.letterbox as usize])
        .with_align(align, s.force_align)
        .with_player_runtime([PlayerRuntime::FlashPlayer, PlayerRuntime::AIR][s.runtime as usize])
        .with_load_behavior([LoadBehavior::Streaming, LoadBehavior::Delayed, LoadBehavior::Blocking][s.load_behavior as usize])
        .with_max_execution_duration(Duration::from_secs(settings::MAX_EXECUTION[s.max_execution as usize] as u64))
        // A console app is always full screen; games see that, and "Fullscreen Only" does what it says.
        .with_fullscreen(true)
        .with_autoplay(true);
    // The Pretend Site setting; else, for a game the Online tab fetched, the address its source
    // plays it from (online.txt `url`: Flash Museum's launchCommand).
    let spoof = s.spoofed_url(&path).or_else(|| {
        let a = crate::online::address_for(&path);
        if let Some(a) = &a {
            say(&format!("     address {a} (online.txt)"));
        }
        a
    });
    if spoof.is_some() {
        builder = builder.with_spoofed_url(spoof.clone());
    }
    if s.dummy_external_interface {
        builder = builder.with_external_interface(Box::new(DummyExternalInterface { spoof_url: spoof }));
    }
    match settings::FRAME_RATES[s.frame_rate as usize] {
        0 => {}
        fps => builder = builder.with_frame_rate(Some(fps as f64)),
    }
    match settings::PLAYER_VERSIONS[s.player_version as usize] {
        0 => {}
        v => builder = builder.with_player_version(Some(v)),
    }
    let player = builder.build();
    if let Ok(mut p) = player.lock() {
        p.set_volume(s.volume as f32 / 10.0);
    }
    say(&format!(
        "ok   player built at {}x{} ({}), quality {}, scale {}, in {:.2} s; entering frame loop",
        size.0,
        size.1,
        if uhd { "4K" } else { "1080p" },
        settings::QUALITIES[s.quality as usize],
        settings::SCALE_MODES[s.scale_mode as usize],
        shown.elapsed().as_secs_f64()
    ));

    // ---- what playing counts toward ----
    app.library.mark_started(&key);
    app.badges.stats.launches += 1;
    if uhd {
        app.badges.stats.uhd += 1;
    }
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
    let mut keyboard_request: Option<(bool, Instant)> = None;
    let mut auto_opened = false;
    let mut hold = 0.0f32;
    let mut options_armed = true;
    let mut session = Session::default();
    let mut end = GameEnd::Menu;
    let mut toast_msg: Option<(String, f32)> = None;
    let mut need_auto_cover = s.auto_covers && !own_picture && !std::path::Path::new(&app.paths.cover(&key)).is_file();
    let mut capture_pending = false;
    let mut capture_manual = false;
    let mut cover_saved = false;
    let mut fps_counter = s.fps_counter;
    let mut volume = s.volume;
    let mut fps_shown = 0.0f32;
    let mut fps_frames = 0u32;
    let mut fps_since = Instant::now();
    let mut renders = 0u64;
    let mut last_save = Instant::now();
    let mut last_progress = Instant::now();
    let mut last_stats = Instant::now();
    let (mut ticks, mut drawn, mut readbacks) = (0u64, 0u64, 0u64);
    // ms summed over the 5 s: game, jobs, draw, overlay (scenes built here)
    let mut spent = [0.0f64; 4];
    let ms = |since: Instant| since.elapsed().as_secs_f64() * 1000.0;
    let _ = app.display.stats.take();
    let frame = Duration::from_micros(16_666);
    let mut fading_out: Option<f32> = None;
    log::set_max_level(log::LevelFilter::Info);

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
            // the game does not see what the menus take
            events.retain(|e| !matches!(e, ruffle_core::events::PlayerEvent::MouseMove { .. }));
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
                let controls = Controls::load(&app.paths, &path);
                app.input.set_controls(controls);
                app.menu.refresh_mappings();
            } else {
                editor = Some(ed);
            }
        } else if let Some(mut q) = quick.take() {
            match q.update(dtf, app.input.nav) {
                quickmenu::Action::None => quick = Some(q),
                quickmenu::Action::Volume(v) => {
                    volume = v;
                    if let Ok(mut p) = player.lock() {
                        p.set_volume(v as f32 / 10.0);
                    }
                    quick = Some(q);
                }
                quickmenu::Action::Fps(on) => {
                    fps_counter = on;
                    quick = Some(q);
                }
                quickmenu::Action::Controls => {
                    editor = Some(Editor::open(&app.paths, Target::Game { stem: stem.clone(), name: name.clone() }, Some(&path)));
                    quick = Some(q);
                }
                quickmenu::Action::Resume => {
                    resume(&player, &mut paused_for, &mut paused_at);
                    options_armed = false;
                }
                quickmenu::Action::Cover => {
                    // The game's own pixels are read back on the next frame (the menus are drawn
                    // over the swapchain image, not into them).
                    resume(&player, &mut paused_for, &mut paused_at);
                    options_armed = false;
                    app.display.request_capture();
                    capture_pending = true;
                    capture_manual = true;
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
                if let Ok(mut p) = player.lock() {
                    for e in events.drain(..) {
                        p.handle_event(e);
                    }
                    p.set_is_playing(false);
                    p.flush_shared_objects();
                }
                last_save = Instant::now();
                paused_at = Some(Instant::now());
                let secs = started.elapsed().saturating_sub(paused_for).as_secs();
                app.library.write_session(&key, secs);
                session.quick_menus += 1;
                crate::sounds::play(crate::sounds::Sfx::Select);
                quick = Some(QuickMenu::new(&name, volume, fps_counter));
                say("game: quick menu");
            }
        }

        // ---- the on-screen keyboard ----
        if quick.is_none() && editor.is_none() && fading_out.is_none() {
            if app.input.keyboard_toggle {
                let was = osk.is_open();
                osk.toggle();
                if !was {
                    app.input.release_all(&mut events);
                }
            }
            if let Some(want) = ui_state.take_keyboard_request() {
                keyboard_request = Some((want, Instant::now()));
            }
            if let Some((want, at)) = keyboard_request {
                if at.elapsed() >= KEYBOARD_SETTLE {
                    keyboard_request = None;
                    let allowed = s.auto_keyboard && !osk.user_closed && !app.input.hid.keyboard_connected();
                    if allowed && want && !osk.is_open() {
                        say("game: a text box wants typing: on-screen keyboard");
                        osk.set_open(true);
                        app.input.release_all(&mut events);
                        auto_opened = true;
                    } else if !want && osk.is_open() && auto_opened {
                        osk.set_open(false);
                        auto_opened = false;
                    }
                }
            }
            let before = events.len();
            osk.update(dtf, if osk.is_open() { app.input.nav } else { 0 }, &mut events);
            if events[before..].iter().any(|e| matches!(e, ruffle_core::events::PlayerEvent::KeyDown { .. })) {
                session.osk = true;
            }
        }

        // ---- the game ----
        let t = Instant::now();
        let paused = quick.is_some() || editor.is_some();
        {
            let Ok(mut p) = player.lock() else {
                break;
            };
            for e in events.drain(..) {
                p.handle_event(e);
            }
            if !paused {
                p.tick(ruffle_core::FloatDuration::from_millis(dt.as_secs_f64() * 1000.0));
            }
        }
        spent[0] += ms(t);
        let t = Instant::now();
        executor.run();
        spent[1] += ms(t);
        ticks += 1;

        // ---- covers ----
        if need_auto_cover && !capture_pending && renders > 0 && started.elapsed().saturating_sub(paused_for) >= AUTO_COVER_AFTER && !paused {
            app.display.request_capture();
            capture_pending = true;
            capture_manual = false;
            need_auto_cover = false;
        }
        if capture_pending {
            if let Some(frame) = app.display.poll_capture() {
                capture_pending = false;
                readbacks += 1;
                let area = if s.scale_mode == 1 { crate::covers::movie_area(frame.width, frame.height, movie_w, movie_h) } else { (0, 0, frame.width, frame.height) };
                crate::covers::save_in_background(frame, area, app.paths.cover(&key));
                cover_saved = true;
                if capture_manual {
                    toast_msg = Some(("Cover saved".to_string(), 0.0));
                }
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
            // Leaving: Ruffle's ribbons sweep over the game (the menu then sweeps them away).
            if let Some(f) = fading_out.as_mut() {
                *f += dtf;
                widgets::ribbon_wipe(&mut scene, *f / FADE_OUT);
            }
        }
        let overlay_busy = !scene.is_empty();
        app.display.set_overlay(scene);
        // The game's own pointer, our pointer, or none (menus over the game).
        if paused || osk.is_open() || ui_state.mouse_hidden() {
            app.input.set_cursor_visible(false);
        }
        spent[3] += ms(t);

        // ---- draw: when the game changed, the overlay is up, or the pointer moved ----
        let t = Instant::now();
        let mut rendered = false;
        if let Ok(mut p) = player.lock() {
            if p.needs_render() || overlay_busy || app.input.moved || capture_pending {
                p.render();
                rendered = true;
            }
        }
        if rendered {
            spent[2] += ms(t);
            drawn += 1;
            renders += 1;
            fps_frames += 1;
            if renders == 1 {
                say(&format!("ok   first frame drawn after {:.2} s", started.elapsed().as_secs_f64()));
            }
        }

        // ---- saves, progress, the frame-time log ----
        if last_save.elapsed() >= SAVE_EVERY && !paused {
            if let Ok(mut p) = player.lock() {
                p.flush_shared_objects();
            }
            last_save = Instant::now();
        }
        if last_progress.elapsed() >= PROGRESS_EVERY {
            let secs = started.elapsed().saturating_sub(paused_for).as_secs();
            app.library.write_session(&key, secs);
            last_progress = Instant::now();
        }
        if last_stats.elapsed() >= STATS_EVERY {
            let window = last_stats.elapsed().as_secs_f64();
            let d = app.display.stats.take();
            let (frame_now, playing) = player.lock().map(|p| (p.current_frame(), p.is_playing())).unwrap_or((None, false));
            say(&format!("[Game] 5 s: {ticks} ticks, {drawn} frames drawn, {readbacks} read back, 0 readbacks failed, root frame {frame_now:?}, playing {playing}"));
            let per = |v: f64| v / ticks.max(1) as f64;
            // draw = render() wall time less what the display did inside it (acquire, overlay, present)
            let draw = (spent[2] - d.acquire_ms - d.overlay_ms - d.present_ms).max(0.0);
            say(&format!(
                "[Game] ms per loop: game {:.1}, jobs {:.1}, draw {:.1} | per frame drawn: readback {:.1} | overlay {:.1}, present {:.1}",
                per(spent[0]),
                per(spent[1]),
                per(draw),
                d.readback_ms / drawn.max(1) as f64,
                per(spent[3] + d.overlay_ms),
                per(d.acquire_ms + d.present_ms),
            ));
            say(&format!(
                "[Game] {}x{} {}: {:.1} fps drawn, {:.1} presents/s",
                size.0,
                size.1,
                if uhd { "4K" } else { "1080p" },
                drawn as f64 / window,
                d.presents as f64 / window
            ));
            (ticks, drawn, readbacks) = (0, 0, 0);
            spent = [0.0; 4];
            last_stats = Instant::now();
        }
        if renders > 0 && renders % 300 == 0 && rendered {
            let secs = started.elapsed().as_secs_f64();
            say(&format!("[Ruffle] {renders} game frames ({:.1} fps)", renders as f64 / secs));
        }

        // ---- leaving ----
        if let Some(f) = fading_out {
            if f >= FADE_OUT {
                break;
            }
        }
        // The surface is FIFO: a present paces the loop at the display's rate. A floor for frames
        // with nothing to draw.
        let took = loop_start.elapsed();
        if !rendered && took < frame {
            std::thread::sleep(frame - took);
        }
    }

    // ---- let go of everything, flush, end ----
    let mut release = Vec::new();
    app.input.release_all(&mut release);
    release.extend(osk.release_all());
    if let Ok(mut p) = player.lock() {
        for e in release {
            p.handle_event(e);
        }
        p.flush_shared_objects();
        p.set_is_playing(false);
    }
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
        // The cover thread may still be writing it: the menu picks it up when it is there.
        app.menu.expect_cover(gi, app.paths.cover(&key));
    }
    say(&format!("game: {name} closed after {secs} s ({:?})", end));
    end
}

/// Resume the paused game: playing again, the paused time not counted.
fn resume(player: &std::sync::Arc<std::sync::Mutex<ruffle_core::Player>>, paused_for: &mut Duration, paused_at: &mut Option<Instant>) {
    if let Some(at) = paused_at.take() {
        *paused_for += at.elapsed();
    }
    if let Ok(mut p) = player.lock() {
        p.set_is_playing(true);
    }
    crate::say("game: resumed");
}
