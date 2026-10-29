//! PS5_RUFFLE_DIRFLASH_V1 — Flash cast members in Shockwave games, played by Ruffle (1.000.036).
//!
//! A Director movie can hold Flash movies as cast members, and Lingo talks to them through
//! Director's Flash Asset Xtra: it reads and sets their ActionScript variables, calls their
//! functions, moves their timelines, makes ActionScript objects in them, and they answer through
//! `getURL("event:…")`, `getURL("lingo:…")` and `fscommand`. Game loaders do this (Miniclip's,
//! in front of Golfish, Golf Ace and Paul Blart, waits on its Flash intro's `isReady`), and whole
//! menus are Flash movies (Paul Blart's start with a Flash check that calls
//! `getURL("event:flash_start_game")`). Until 1.000.036 such a member was drawn as nothing and
//! answered nothing, and these games waited forever.
//!
//! DirPlayer's web front end gives each Flash sprite a Ruffle player of its own, draws it off
//! screen and puts its picture in the sprite (its src/services/flashPlayerManager.ts). Ruffle5 does
//! the same with the Ruffle it plays Flash games with:
//!
//!   * DirPlayer asks through its Flash host (vendor/dirplayer vm-rust/src/player/flash_host.rs,
//!     tools/dirplayer-native.patch): [`MemberHost`] answers, with the meaning flashPlayerManager.ts
//!     gives each call; the ActionScript side is Ruffle's `Player::dirplayer_*`
//!     (vendor/ruffle core/src/dirplayer.rs, tools/ruffle-dirplayer.patch);
//!   * a sprite's player is made when DirPlayer shows the member (its `onFlashMemberLoaded`), runs
//!     the member's first frame at once (its frame-1 actions come before the Lingo calls made for
//!     the sprite before it existed, which are kept and made then, as the web build replays them),
//!     and goes when the sprite stops showing it;
//!   * it draws at the sprite's size on the stage, and never smaller than the Flash movie's own
//!     (as flashPlayerManager.ts), transparent where the movie draws nothing; DirPlayer puts the
//!     picture in the sprite with its ink and blend;
//!   * between score frames the players run at their own frame rates ([`FlashMembers::tick`]) and
//!     a picture changed is drawn and handed to DirPlayer ([`FlashMembers::draw`]);
//!   * the mouse: DirPlayer gives a Flash sprite the clicks on it (as on the web) and, natively,
//!     the moves over it, at the sprite's own point; keys go to the Flash sprite clicked last;
//!   * sound: each player mixes into the game's sound thread ([`Mixers`]);
//!   * files: a member's movie loads what it asks for as a Flash game does (the game's folder, its
//!     web root), its SharedObjects are kept in the game's saves.
//!
//! The module depends on Ruffle and DirPlayer only (the console's pieces come in a [`Setup`]), so
//! dcr-probe plays Flash members the same way on a PC.
// SPDX-License-Identifier: GPL-3.0-or-later

use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use ruffle_core::backend::audio::{
    swf, AudioBackend, AudioMixer, AudioMixerProxy, DecodeError, RegisterError, SoundHandle, SoundInstanceHandle, SoundStreamInfo, SoundTransform,
};
use ruffle_core::backend::navigator::{ErrorResponse, NavigationMethod, NavigatorBackend, NullExecutor, OwnedFuture, Request, SuccessResponse};
use ruffle_core::backend::storage::StorageBackend;
use ruffle_core::backend::ui::NullUiBackend;
use ruffle_core::dirplayer::LingoValue;
use ruffle_core::events::{MouseButton, PlayerEvent};
use ruffle_core::external::FsCommandProvider;
use ruffle_core::impl_audio_mixer_backend;
use ruffle_core::loader::Error as LoaderError;
use ruffle_core::socket::{SocketAction, SocketHandle};
use ruffle_core::tag_utils::SwfMovieData;
use ruffle_core::{FloatDuration, LoadBehavior, Player, PlayerBuilder, StageScaleMode, ViewportDimensions};
use ruffle_render::backend::RenderBackend;
use ruffle_render::quality::StageQuality;
use url::{ParseError, Url};
use vm_rust::player::flash_host::{self, FlashCall, FlashHost, FlashValue, MouseKind};

/// The largest a member's picture is drawn (a side, in pixels).
const MAX_SIDE: u32 = 4096;
/// The Flash → Lingo calls logged per game (then only counted).
const LOGGED_CALLS: u32 = 200;

/// Makes the renderer a Flash sprite draws with, and reads back what it drew.
pub trait Canvas {
    fn renderer(&self, width: u32, height: u32) -> Result<Box<dyn RenderBackend>, String>;
    /// The last frame drawn: straight-alpha RGBA, `width * height * 4` bytes.
    fn read(&self, renderer: &mut dyn RenderBackend) -> Option<(u32, u32, Vec<u8>)>;
}

/// The console's canvas: Ruffle's wgpu renderer into a texture of the size asked, on the device
/// the display uses, read back after each frame (Ruffle's `capture_frame`, as its exporter does).
pub struct WgpuCanvas {
    pub descriptors: Arc<ruffle_render_wgpu::descriptors::Descriptors>,
}

impl Canvas for WgpuCanvas {
    fn renderer(&self, width: u32, height: u32) -> Result<Box<dyn RenderBackend>, String> {
        use ruffle_render_wgpu::{backend::WgpuRenderBackend, target::TextureTarget};
        let target = TextureTarget::new(&self.descriptors.device, (width, height)).map_err(|e| e.to_string())?;
        let backend = WgpuRenderBackend::new(Arc::clone(&self.descriptors), target).map_err(|e| e.to_string())?;
        Ok(Box::new(backend))
    }

    fn read(&self, renderer: &mut dyn RenderBackend) -> Option<(u32, u32, Vec<u8>)> {
        use ruffle_render_wgpu::{backend::WgpuRenderBackend, target::TextureTarget};
        let backend = <dyn std::any::Any>::downcast_mut::<WgpuRenderBackend<TextureTarget>>(renderer)?;
        let image = backend.capture_frame()?;
        let (w, h) = image.dimensions();
        Some((w, h, image.into_raw()))
    }
}

/// The players' sound, for the game's sound thread to mix ([`Mixers::mix_into`]).
#[derive(Clone, Default)]
pub struct Mixers(Arc<Mutex<Vec<(u64, AudioMixerProxy)>>>);

impl Mixers {
    /// Adds every member's sound to `out` (48 kHz stereo, interleaved); `scratch` is reused.
    pub fn mix_into(&self, out: &mut [f32], scratch: &mut Vec<f32>) {
        let Ok(list) = self.0.lock() else {
            return;
        };
        scratch.resize(out.len(), 0.0);
        for (_, proxy) in list.iter() {
            scratch.iter_mut().for_each(|s| *s = 0.0);
            proxy.mix::<f32>(scratch);
            for (o, s) in out.iter_mut().zip(scratch.iter()) {
                *o += *s;
            }
        }
    }

    fn add(&self, id: u64, proxy: AudioMixerProxy) {
        if let Ok(mut list) = self.0.lock() {
            list.push((id, proxy));
        }
    }

    fn remove(&self, id: u64) {
        if let Ok(mut list) = self.0.lock() {
            list.retain(|(i, _)| *i != id);
        }
    }
}

/// One member's sound: Ruffle's mixer, its proxy mixed by the game's sound thread.
struct MemberAudio {
    mixer: AudioMixer,
}

impl AudioBackend for MemberAudio {
    impl_audio_mixer_backend!(mixer);

    fn play(&mut self) {}

    fn pause(&mut self) {}
}

/// What the game gives its Flash members.
pub struct Setup {
    pub canvas: Box<dyn Canvas>,
    /// A navigator for a member's movie: where its files come from (the game's folder, its web
    /// root, ...). Getting a web page opened asks it; `event:` and `lingo:` addresses never do.
    pub navigator: Box<dyn Fn(&NullExecutor) -> Box<dyn NavigatorBackend>>,
    /// Where a member's SharedObjects are kept.
    pub storage: Box<dyn Fn() -> Box<dyn StorageBackend>>,
    /// The players' sound goes here (None: no sound).
    pub mixers: Option<Mixers>,
    /// The Director movie's address: each member's movie plays from it.
    pub movie_url: String,
    /// The address the game is told it is on (Pretend to be on, online.txt), when there is one.
    pub spoofed_url: Option<String>,
    pub quality: StageQuality,
    pub max_execution: Duration,
    pub player_version: Option<u8>,
    pub log: fn(&str),
}

/// A Lingo call kept for a sprite whose player is not there yet (flashPlayerManager.ts
/// `queueOp`), made once it is.
enum Op {
    GotoPlay(String),
    FrameSet(String),
    Stop,
    Play,
    Rewind,
    SetVariable(String, String),
    Call(String, Vec<FlashValue>),
    New(String, Vec<FlashValue>, String),
}

struct Instance {
    id: u64,
    player: Arc<Mutex<Player>>,
    cast: (i32, i32),
    paused_at_start: bool,
    /// Lingo stopped the root timeline (flashPlayerManager.ts `instance.stopped`).
    stopped: bool,
    /// The size it is drawn at, the Flash movie's own, the sprite's on the stage.
    render: (u32, u32),
    native: (u32, u32),
    sprite_size: (i32, i32),
    /// Draw it, even if Ruffle has nothing new (a Lingo call moved it).
    dirty: bool,
    /// The last picture handed to DirPlayer: Ruffle draws every frame the movie runs, moving or
    /// not, and a picture the same as the last is not handed over (nor the stage drawn for it).
    last: Vec<u8>,
}

/// The game's Flash members: one Ruffle player per Flash sprite.
pub struct FlashMembers {
    setup: Setup,
    executor: NullExecutor,
    instances: HashMap<i32, Instance>,
    pending: HashMap<i32, Vec<Op>>,
    /// The Flash sprite clicked last: keys go to it.
    focus: Option<i32>,
    next_id: u64,
    // for the log
    made: u64,
    calls_logged: Arc<Mutex<u32>>,
    drawn: u64,
    read_ms: f64,
}

impl FlashMembers {
    pub fn new(setup: Setup) -> Rc<RefCell<FlashMembers>> {
        Rc::new(RefCell::new(FlashMembers {
            setup,
            executor: NullExecutor::new(),
            instances: HashMap::new(),
            pending: HashMap::new(),
            focus: None,
            next_id: 1,
            made: 0,
            calls_logged: Arc::new(Mutex::new(0)),
            drawn: 0,
            read_ms: 0.0,
        }))
    }

    /// How many Flash sprites play now.
    pub fn count(&self) -> usize {
        self.instances.len()
    }

    /// The players run for `dt` (at their own frame rates), and the jobs they started (loads).
    pub fn tick(&mut self, dt: Duration) {
        let dt = FloatDuration::from_millis(dt.as_secs_f64() * 1000.0);
        for inst in self.instances.values() {
            if let Ok(mut p) = inst.player.lock() {
                p.tick(dt);
            }
        }
        self.executor.run();
    }

    /// Each player whose picture changed draws it, and DirPlayer gets it for the sprite. True when
    /// one did (the stage has to be drawn again).
    pub fn draw(&mut self) -> bool {
        let mut any = false;
        let canvas = &self.setup.canvas;
        for (&sprite, inst) in self.instances.iter_mut() {
            let Ok(mut p) = inst.player.lock() else {
                continue;
            };
            if !(p.needs_render() || inst.dirty) {
                continue;
            }
            inst.dirty = false;
            p.render();
            let t = Instant::now();
            if let Some((w, h, rgba)) = canvas.read(p.renderer_mut()) {
                self.read_ms += t.elapsed().as_secs_f64() * 1000.0;
                if rgba == inst.last {
                    continue;
                }
                vm_rust::update_flash_frame(sprite, w, h, &rgba);
                inst.last = rgba;
                self.drawn += 1;
                any = true;
            }
        }
        any
    }

    /// A key (KeyDown, KeyUp, TextInput, TextControl) for the Flash sprite clicked last.
    pub fn key(&mut self, event: &PlayerEvent) {
        let Some(inst) = self.focus.and_then(|f| self.instances.get_mut(&f)) else {
            return;
        };
        if let Ok(mut p) = inst.player.lock() {
            p.handle_event(event.clone());
        }
    }

    /// For the log every few seconds: sprites, pictures drawn and the time reading them back.
    pub fn take_stats(&mut self) -> (usize, u64, f64) {
        let s = (self.instances.len(), self.drawn, self.read_ms);
        self.drawn = 0;
        self.read_ms = 0.0;
        s
    }

    fn remove(&mut self, sprite: i32) {
        if let Some(inst) = self.instances.remove(&sprite) {
            (self.setup.log)(&format!("     flash: sprite {sprite} no longer plays member {}:{}", inst.cast.0, inst.cast.1));
            if let Some(m) = &self.setup.mixers {
                m.remove(inst.id);
            }
            if self.focus == Some(sprite) {
                self.focus = None;
            }
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn load(&mut self, sprite: i32, cast_lib: i32, cast_member: i32, data: &[u8], width: u32, height: u32, paused_at_start: bool, asserted_frame: i32) {
        // a member swap: the sprite's last player goes (flashPlayerManager.ts destroyFlashInstance)
        self.remove(sprite);
        let log = self.setup.log;
        // Director may put a header before the movie: from its signature on
        let Some(start) = (0..data.len().saturating_sub(2)).find(|&i| matches!(data[i], b'F' | b'C' | b'Z') && data[i + 1] == b'W' && data[i + 2] == b'S') else {
            log(&format!("warn flash: sprite {sprite}: member {cast_lib}:{cast_member} holds no Flash movie ({} bytes)", data.len()));
            return;
        };
        let movie = match SwfMovieData::from_data(&data[start..], self.setup.movie_url.clone(), None, None) {
            Ok(m) => m,
            Err(e) => {
                log(&format!("warn flash: sprite {sprite}: member {cast_lib}:{cast_member} is not a movie Ruffle can read: {e}"));
                return;
            }
        };
        let native = (movie.width().to_pixels().round().max(0.0) as u32, movie.height().to_pixels().round().max(0.0) as u32);
        let (version, fps, as3) = (movie.version(), movie.frame_rate().to_f32(), movie.is_action_script_3());
        // the size it draws at: the sprite's, never below the movie's own (an off-stage 3D
        // texture: the size asked), as flashPlayerManager.ts
        let render = if sprite >= 0 { (width.max(native.0), height.max(native.1)) } else { (width, height) };
        let render = (render.0.clamp(1, MAX_SIDE), render.1.clamp(1, MAX_SIDE));
        let renderer = match self.setup.canvas.renderer(render.0, render.1) {
            Ok(r) => r,
            Err(e) => {
                log(&format!("warn flash: sprite {sprite}: no renderer at {}x{}: {e}", render.0, render.1));
                return;
            }
        };
        let id = self.next_id;
        self.next_id += 1;
        let audio: Box<dyn AudioBackend> = match &self.setup.mixers {
            Some(m) => {
                let mixer = AudioMixer::new(2, 48_000);
                m.add(id, mixer.proxy());
                Box::new(MemberAudio { mixer })
            }
            None => Box::new(ruffle_core::backend::audio::NullAudioBackend::new()),
        };
        let navigator = MemberNavigator {
            inner: (self.setup.navigator)(&self.executor),
            cast: (cast_lib, cast_member),
            sprite,
            logged: Arc::clone(&self.calls_logged),
            log,
        };
        let mut builder = PlayerBuilder::new()
            .with_boxed_renderer(renderer)
            .with_boxed_audio(audio)
            .with_video(ruffle_video_software::backend::SoftwareVideoBackend::new())
            .with_navigator(navigator)
            .with_storage((self.setup.storage)())
            .with_ui(NullUiBackend::new())
            .with_fs_commands(Box::new(MemberFsCommands { cast: (cast_lib, cast_member), sprite, logged: Arc::clone(&self.calls_logged), log }))
            .with_movie(movie)
            .with_viewport_dimensions(render.0, render.1, 1.0)
            .with_scale_mode(StageScaleMode::ShowAll, false)
            .with_quality(self.setup.quality)
            .with_load_behavior(LoadBehavior::Blocking)
            .with_max_execution_duration(self.setup.max_execution)
            .with_autoplay(true);
        if let Some(url) = &self.setup.spoofed_url {
            builder = builder.with_spoofed_url(Some(url.clone()));
        }
        if let Some(v) = self.setup.player_version {
            builder = builder.with_player_version(Some(v));
        }
        let player = builder.build();
        let mut inst = Instance {
            id,
            player,
            cast: (cast_lib, cast_member),
            paused_at_start,
            stopped: false,
            render,
            native,
            sprite_size: (width as i32, height as i32),
            dirty: true,
            last: Vec::new(),
        };
        if let Ok(mut p) = inst.player.lock() {
            // transparent where the movie draws nothing (flashPlayerManager.ts: wmode transparent)
            p.set_window_mode("transparent");
            // the first frame now: its actions run before the Lingo kept for the sprite
            p.run_frame();
            // the frame a script set for the sprite, else frame 1 for a member that waits there
            // (pausedAtStart), stopped: flashPlayerManager.ts's initial pin
            let pin = if asserted_frame >= 0 { Some(asserted_frame) } else if paused_at_start { Some(1) } else { None };
            if let Some(f) = pin {
                p.dirplayer_goto(f.clamp(1, u16::MAX as i32) as u16, true);
                inst.stopped = true;
            }
        }
        self.made += 1;
        log(&format!(
            "     flash: sprite {sprite} plays member {cast_lib}:{cast_member} ({} bytes, SWF {version}{}, {}x{} at {fps:.0} fps), drawn at {}x{}{}",
            data.len() - start,
            if as3 { ", ActionScript 3: Lingo cannot reach its variables" } else { "" },
            native.0,
            native.1,
            render.0,
            render.1,
            if asserted_frame >= 0 { format!(", held at frame {asserted_frame}") } else if paused_at_start { ", paused at start".to_string() } else { String::new() }
        ));
        // the Lingo calls made before it was there, in order (flushPendingGoto)
        let ops = self.pending.remove(&sprite).unwrap_or_default();
        let replayed = ops.len();
        for op in ops {
            apply(&mut inst, op);
        }
        if replayed > 0 {
            log(&format!("     flash: sprite {sprite}: {replayed} Lingo call(s) made before it was there, made now"));
        }
        // the frame a script set wins over what the replay did, unless that played it on
        if asserted_frame >= 0 && inst.stopped {
            if let Ok(mut p) = inst.player.lock() {
                p.dirplayer_goto(asserted_frame.clamp(1, u16::MAX as i32) as u16, true);
            }
        }
        self.instances.insert(sprite, inst);
    }

    fn queue_or_apply(&mut self, sprite: i32, op: Op) {
        match self.instances.get_mut(&sprite) {
            Some(inst) => apply(inst, op),
            None => self.pending.entry(sprite).or_default().push(op),
        }
    }

    fn with_player<R>(&mut self, sprite: i32, f: impl FnOnce(&mut Player) -> R) -> Option<R> {
        let inst = self.instances.get_mut(&sprite)?;
        inst.dirty = true;
        let mut p = inst.player.lock().ok()?;
        Some(f(&mut p))
    }

    /// A sprite-local point (in the sprite's size on the stage) as a point of the player's view.
    fn view_point(inst: &Instance, x: f64, y: f64, sprite_w: i32, sprite_h: i32) -> (f64, f64) {
        let (sw, sh) = if sprite_w > 0 && sprite_h > 0 { (sprite_w, sprite_h) } else { inst.sprite_size };
        let sx = if sw > 0 { inst.render.0 as f64 / sw as f64 } else { 1.0 };
        let sy = if sh > 0 { inst.render.1 as f64 / sh as f64 } else { 1.0 };
        (x * sx, y * sy)
    }
}

/// A frame number, as flashPlayerManager.ts tells one from a label (all digits, a sign allowed).
fn frame_number(s: &str) -> Option<u16> {
    let t = s.trim();
    let digits = t.strip_prefix('-').unwrap_or(t);
    if digits.is_empty() || !digits.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    Some(t.parse::<i64>().unwrap_or(1).clamp(1, u16::MAX as i64) as u16)
}

fn to_lingo(v: &FlashValue) -> LingoValue {
    match v {
        FlashValue::Undefined => LingoValue::Undefined,
        FlashValue::Null => LingoValue::Null,
        FlashValue::Bool(b) => LingoValue::Bool(*b),
        FlashValue::Number(n) => LingoValue::Number(*n),
        FlashValue::String(s) => LingoValue::String(s.clone()),
        FlashValue::Object(p) => LingoValue::Object(p.clone()),
        FlashValue::List(items) => LingoValue::List(items.iter().map(to_lingo).collect()),
    }
}

fn from_lingo(v: LingoValue) -> FlashValue {
    match v {
        LingoValue::Undefined => FlashValue::Undefined,
        LingoValue::Null => FlashValue::Null,
        LingoValue::Bool(b) => FlashValue::Bool(b),
        LingoValue::Number(n) => FlashValue::Number(n),
        LingoValue::String(s) => FlashValue::String(s),
        LingoValue::Object(p) => FlashValue::Object(p),
        LingoValue::List(items) => FlashValue::List(items.into_iter().map(from_lingo).collect()),
    }
}

/// A Lingo call on a sprite's player, as flashPlayerManager.ts makes it.
fn apply(inst: &mut Instance, op: Op) {
    inst.dirty = true;
    let Ok(mut p) = inst.player.lock() else {
        return;
    };
    match op {
        // `gotoFrame`: seek and play on (applyGotoPlay / applyGotoLabelPlay)
        Op::GotoPlay(x) => {
            inst.stopped = false;
            match frame_number(&x) {
                Some(n) => p.dirplayer_goto(n, false),
                None => {
                    p.dirplayer_call_function("_root.gotoAndPlay", &[LingoValue::String(x)]);
                }
            }
        }
        // `sprite.frame = x` (applyFrameSetting): a pausedAtStart member stops there, another plays on
        Op::FrameSet(x) => {
            if inst.paused_at_start {
                inst.stopped = true;
                match frame_number(&x) {
                    Some(n) => p.dirplayer_goto(n, true),
                    None => {
                        p.dirplayer_call_function("_root.gotoAndStop", &[LingoValue::String(x)]);
                    }
                }
            } else {
                inst.stopped = false;
                p.set_is_playing(true);
                match frame_number(&x) {
                    Some(n) => p.dirplayer_goto(n, false),
                    None => {
                        p.dirplayer_call_function("_root.gotoAndPlay", &[LingoValue::String(x)]);
                    }
                }
            }
        }
        // stopFlash: the root timeline, not the player (it still draws a frame Lingo moves it to)
        Op::Stop => {
            inst.stopped = true;
            p.dirplayer_stop();
        }
        // playFlash: on from where it is, past a stop() of its own
        Op::Play => {
            inst.stopped = false;
            p.set_is_playing(true);
            p.dirplayer_play();
        }
        Op::Rewind => {
            inst.stopped = true;
            p.dirplayer_goto(1, true);
        }
        Op::SetVariable(path, value) => {
            p.dirplayer_set_variable(&path, &value);
        }
        Op::Call(path, args) => {
            let args: Vec<LingoValue> = args.iter().map(to_lingo).collect();
            p.dirplayer_call_function(&path, &args);
        }
        Op::New(class, args, at) => {
            let args: Vec<LingoValue> = args.iter().map(to_lingo).collect();
            p.dirplayer_new_object(&class, &args, &at);
        }
    }
}

/// What DirPlayer asks of its Flash host (vm-rust player/flash_host.rs), answered by the game's
/// [`FlashMembers`].
pub struct MemberHost(pub Rc<RefCell<FlashMembers>>);

impl MemberHost {
    fn with<R>(&self, default: R, f: impl FnOnce(&mut FlashMembers) -> R) -> R {
        match self.0.try_borrow_mut() {
            Ok(mut m) => f(&mut m),
            Err(_) => default,
        }
    }
}

impl FlashHost for MemberHost {
    fn load(&mut self, sprite: i32, cast_lib: i32, cast_member: i32, swf: &[u8], width: u32, height: u32, paused_at_start: bool, asserted_frame: i32) {
        self.with((), |m| m.load(sprite, cast_lib, cast_member, swf, width, height, paused_at_start, asserted_frame));
    }

    fn unload(&mut self, sprite: i32) {
        self.with((), |m| m.remove(sprite));
    }

    fn reset(&mut self) {
        self.with((), |m| {
            let sprites: Vec<i32> = m.instances.keys().copied().collect();
            for s in sprites {
                m.remove(s);
            }
            m.pending.clear();
        });
    }

    fn set_size(&mut self, sprite: i32, width: i32, height: i32) {
        self.with((), |m| {
            let Some(inst) = m.instances.get_mut(&sprite) else {
                return;
            };
            inst.sprite_size = (width, height);
            if sprite < 0 {
                return;
            }
            let w = (width.max(1) as u32).max(inst.native.0).clamp(1, MAX_SIDE);
            let h = (height.max(1) as u32).max(inst.native.1).clamp(1, MAX_SIDE);
            // sub-2-pixel changes are not worth drawing again (setFlashSize)
            if w.abs_diff(inst.render.0) < 2 && h.abs_diff(inst.render.1) < 2 {
                return;
            }
            inst.render = (w, h);
            inst.dirty = true;
            if let Ok(mut p) = inst.player.lock() {
                p.set_viewport_dimensions(ViewportDimensions { width: w, height: h, scale_factor: 1.0 });
            }
        });
    }

    fn is_ready(&mut self, sprite: i32) -> bool {
        self.with(false, |m| m.instances.contains_key(&sprite))
    }

    fn goto_frame(&mut self, sprite: i32, frame_or_label: &str) {
        self.with((), |m| m.queue_or_apply(sprite, Op::GotoPlay(frame_or_label.to_string())));
    }

    fn goto_frame_and_stop(&mut self, sprite: i32, frame_or_label: &str) {
        self.with((), |m| m.queue_or_apply(sprite, Op::FrameSet(frame_or_label.to_string())));
    }

    fn stop(&mut self, sprite: i32) {
        self.with((), |m| m.queue_or_apply(sprite, Op::Stop));
    }

    fn play(&mut self, sprite: i32) {
        self.with((), |m| m.queue_or_apply(sprite, Op::Play));
    }

    fn rewind(&mut self, sprite: i32) {
        self.with((), |m| m.queue_or_apply(sprite, Op::Rewind));
    }

    /// Not kept for later (flashPlayerManager.ts `callFrame` returns without a player).
    fn call_frame(&mut self, sprite: i32, frame: i32) {
        self.with((), |m| {
            m.with_player(sprite, |p| p.dirplayer_call_frame(&frame.to_string()));
        });
    }

    /// Lingo's intent first (it stopped the sprite), then the movie's own: its root timeline
    /// stops at a `stop()` action, and Director's `playing` is FALSE then.
    fn is_playing(&mut self, sprite: i32) -> bool {
        self.with(false, |m| {
            let Some(inst) = m.instances.get(&sprite) else {
                return false;
            };
            if inst.stopped {
                return false;
            }
            let Ok(mut p) = inst.player.lock() else {
                return false;
            };
            p.is_playing() && p.dirplayer_root().is_some_and(|r| r.playing)
        })
    }

    fn frame_count(&mut self, sprite: i32) -> i32 {
        self.with(0, |m| {
            let Some(inst) = m.instances.get(&sprite) else {
                return 0;
            };
            inst.player.lock().ok().and_then(|mut p| p.dirplayer_root()).map_or(0, |r| r.total_frames as i32)
        })
    }

    fn current_frame(&mut self, sprite: i32) -> i32 {
        self.with(0, |m| {
            let Some(inst) = m.instances.get(&sprite) else {
                return 0;
            };
            inst.player.lock().ok().and_then(|mut p| p.dirplayer_root()).map_or(0, |r| r.current_frame as i32)
        })
    }

    fn get_variable(&mut self, sprite: i32, path: &str) -> FlashValue {
        self.with(FlashValue::Null, |m| {
            let Some(inst) = m.instances.get(&sprite) else {
                return FlashValue::Null;
            };
            match inst.player.lock() {
                Ok(mut p) => from_lingo(p.dirplayer_get_variable(path)),
                Err(_) => FlashValue::Null,
            }
        })
    }

    fn set_variable(&mut self, sprite: i32, path: &str, value: &str) -> bool {
        self.with(false, |m| {
            match m.instances.get_mut(&sprite) {
                Some(inst) => {
                    inst.dirty = true;
                    inst.player.lock().map(|mut p| p.dirplayer_set_variable(path, value)).unwrap_or(false)
                }
                // kept and made once it is there; true, as the web build answers
                None => {
                    m.pending.entry(sprite).or_default().push(Op::SetVariable(path.to_string(), value.to_string()));
                    true
                }
            }
        })
    }

    fn call_function(&mut self, sprite: i32, path: &str, args: &[FlashValue]) -> FlashValue {
        self.with(FlashValue::Null, |m| match m.instances.get_mut(&sprite) {
            Some(inst) => {
                inst.dirty = true;
                let args: Vec<LingoValue> = args.iter().map(to_lingo).collect();
                inst.player.lock().map(|mut p| from_lingo(p.dirplayer_call_function(path, &args))).unwrap_or(FlashValue::Null)
            }
            None => {
                m.pending.entry(sprite).or_default().push(Op::Call(path.to_string(), args.to_vec()));
                FlashValue::Null
            }
        })
    }

    fn new_object(&mut self, sprite: i32, class: &str, args: &[FlashValue], store_at: &str) -> bool {
        self.with(false, |m| match m.instances.get_mut(&sprite) {
            Some(inst) => {
                let args: Vec<LingoValue> = args.iter().map(to_lingo).collect();
                let made = inst.player.lock().map(|mut p| p.dirplayer_new_object(class, &args, store_at)).unwrap_or(false);
                if !made {
                    (m.setup.log)(&format!("warn flash: sprite {sprite}: newObject(\"{class}\") made nothing"));
                }
                made
            }
            None => {
                m.pending.entry(sprite).or_default().push(Op::New(class.to_string(), args.to_vec(), store_at.to_string()));
                true
            }
        })
    }

    fn hit_test(&mut self, sprite: i32, x: f64, y: f64) -> i32 {
        self.with(0, |m| {
            let Some(inst) = m.instances.get(&sprite) else {
                return 0;
            };
            let (vx, vy) = FlashMembers::view_point(inst, x, y, 0, 0);
            inst.player.lock().map(|mut p| p.dirplayer_hit_classify(vx, vy) as i32).unwrap_or(0)
        })
    }

    fn mouse(&mut self, sprite: i32, kind: MouseKind, x: i32, y: i32, sprite_w: i32, sprite_h: i32) -> bool {
        self.with(false, |m| {
            let Some(inst) = m.instances.get(&sprite) else {
                return false;
            };
            let (vx, vy) = FlashMembers::view_point(inst, x as f64, y as f64, sprite_w, sprite_h);
            let event = match kind {
                MouseKind::Move => PlayerEvent::MouseMove { x: vx, y: vy },
                MouseKind::Down => PlayerEvent::MouseDown { x: vx, y: vy, button: MouseButton::Left, index: None },
                MouseKind::Up => PlayerEvent::MouseUp { x: vx, y: vy, button: MouseButton::Left },
                MouseKind::Leave => PlayerEvent::MouseLeave,
            };
            if let Ok(mut p) = inst.player.lock() {
                p.handle_event(event);
            }
            if kind == MouseKind::Down {
                m.focus = Some(sprite);
            }
            true
        })
    }
}

/// Logs a Flash → Lingo call (the first [`LOGGED_CALLS`] of a game).
fn log_call(logged: &Mutex<u32>, log: fn(&str), line: &str) {
    if let Ok(mut n) = logged.lock() {
        *n += 1;
        if *n <= LOGGED_CALLS {
            log(line);
        } else if *n == LOGGED_CALLS + 1 {
            log(&format!("     flash: more than {LOGGED_CALLS} calls to Lingo; not logged from here"));
        }
    }
}

/// A member's navigator: `event:` and `lingo:` addresses go to Lingo (Director's Flash Asset
/// Xtra; flashPlayerManager.ts `registerEventUrlHandler`), anything else to the game's navigator.
struct MemberNavigator {
    inner: Box<dyn NavigatorBackend>,
    cast: (i32, i32),
    sprite: i32,
    logged: Arc<Mutex<u32>>,
    log: fn(&str),
}

fn strip_scheme<'a>(url: &'a str, scheme: &str) -> Option<&'a str> {
    let u = url.trim_start();
    (u.len() >= scheme.len() && u.is_char_boundary(scheme.len()) && u[..scheme.len()].eq_ignore_ascii_case(scheme)).then(|| &u[scheme.len()..])
}

impl NavigatorBackend for MemberNavigator {
    fn navigate_to_url(&self, url: &str, target: &str, vars_method: Option<(NavigationMethod, ruffle_core::indexmap::IndexMap<String, String>)>) {
        if let Some(body) = strip_scheme(url, "event:") {
            let body = body.trim().to_string();
            log_call(&self.logged, self.log, &format!("     flash: sprite {} (member {}:{}) to Lingo: event:{body}", self.sprite, self.cast.0, self.cast.1));
            flash_host::post(FlashCall::Event { cast_lib: self.cast.0, cast_member: self.cast.1, body });
            return;
        }
        if let Some(body) = strip_scheme(url, "lingo:") {
            let body = body.trim().to_string();
            log_call(&self.logged, self.log, &format!("     flash: sprite {} (member {}:{}) to Lingo: lingo:{body}", self.sprite, self.cast.0, self.cast.1));
            flash_host::post(FlashCall::Lingo(body));
            return;
        }
        self.inner.navigate_to_url(url, target, vars_method)
    }

    fn fetch(&self, request: Request) -> OwnedFuture<Box<dyn SuccessResponse>, ErrorResponse> {
        self.inner.fetch(request)
    }

    fn resolve_url(&self, url: &str) -> Result<Url, ParseError> {
        self.inner.resolve_url(url)
    }

    fn spawn_future(&mut self, future: OwnedFuture<(), LoaderError>) {
        self.inner.spawn_future(future)
    }

    fn pre_process_url(&self, url: Url) -> Url {
        self.inner.pre_process_url(url)
    }

    fn connect_socket(
        &mut self,
        host: String,
        port: u16,
        timeout: Duration,
        handle: SocketHandle,
        receiver: async_channel::Receiver<Vec<u8>>,
        sender: async_channel::Sender<SocketAction>,
    ) {
        self.inner.connect_socket(host, port, timeout, handle, receiver, sender)
    }
}

/// A member's `fscommand(command, args)`: to Lingo as "command args" (flashPlayerManager.ts
/// `registerFSCommandHandler`: the command is the handler, the arguments follow).
struct MemberFsCommands {
    cast: (i32, i32),
    sprite: i32,
    logged: Arc<Mutex<u32>>,
    log: fn(&str),
}

impl FsCommandProvider for MemberFsCommands {
    fn on_fs_command(&self, command: &str, args: &str) -> bool {
        let (command, args) = (command.trim(), args.trim());
        if command.is_empty() {
            return true;
        }
        let body = if args.is_empty() { command.to_string() } else { format!("{command} {args}") };
        log_call(&self.logged, self.log, &format!("     flash: sprite {} (member {}:{}) to Lingo: fscommand {body}", self.sprite, self.cast.0, self.cast.1));
        flash_host::post(FlashCall::Event { cast_lib: self.cast.0, cast_member: self.cast.1, body });
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ruffle_core::swf::avm1::types::{Action, DefineFunction, GetUrl, Push, Value as Av};
    use ruffle_core::swf::{Compression, FrameLabel, Header, Rectangle, SwfStr, Tag, Twips};

    fn s(text: &str) -> &SwfStr {
        SwfStr::from_utf8_str(text)
    }

    fn code(list: &[Action]) -> Vec<u8> {
        let mut out = Vec::new();
        let mut w = ruffle_core::swf::avm1::write::Writer::new(&mut out, 8);
        for a in list {
            w.write_action(a).unwrap();
        }
        w.write_action(&Action::End).unwrap();
        out
    }

    fn push<'a>(values: Vec<Av<'a>>) -> Action<'a> {
        Action::Push(Push { values })
    }

    /// A SWF 8 movie, 100x80 at 30 fps, one DoAction per frame (and a label where given).
    fn movie(frames: &[(Option<&str>, Vec<u8>)]) -> Vec<u8> {
        let header = Header {
            compression: Compression::None,
            version: 8,
            stage_size: Rectangle { x_min: Twips::ZERO, y_min: Twips::ZERO, x_max: Twips::from_pixels(100.0), y_max: Twips::from_pixels(80.0) },
            frame_rate: ruffle_core::swf::Fixed8::from_f32(30.0),
            num_frames: frames.len() as u16,
        };
        let mut tags = Vec::new();
        for (label, actions) in frames {
            if let Some(l) = label {
                tags.push(Tag::FrameLabel(FrameLabel { label: s(l), is_anchor: false }));
            }
            tags.push(Tag::DoAction(actions));
            tags.push(Tag::ShowFrame);
        }
        let mut out = Vec::new();
        ruffle_core::swf::write_swf(&header, &tags, &mut out).unwrap();
        out
    }

    /// The member this tests talks to: what Lingo meets in a game's Flash intro or checker.
    fn intro() -> Vec<u8> {
        let add_body = code(&[
            push(vec![Av::Str(s("a"))]),
            Action::GetVariable,
            push(vec![Av::Str(s("b"))]),
            Action::GetVariable,
            Action::Add2,
            push(vec![Av::Str(s("this"))]),
            Action::GetVariable,
            push(vec![Av::Str(s("x"))]),
            Action::GetMember,
            Action::Add2,
            Action::Return,
        ]);
        let me_body = code(&[push(vec![Av::Str(s("this"))]), Action::GetVariable, Action::Return]);
        let get_x_body = code(&[push(vec![Av::Str(s("obj"))]), Action::GetVariable, push(vec![Av::Str(s("x"))]), Action::GetMember, Action::Return]);
        // the function bodies end in an End; a function's code is its body without it
        let trim = |v: &[u8]| v[..v.len() - 1].to_vec();
        let (add_body, me_body, get_x_body) = (trim(&add_body), trim(&me_body), trim(&get_x_body));
        let frame1 = code(&[
            // isReady = "false"; n = 3; b = true
            push(vec![Av::Str(s("isReady")), Av::Str(s("false"))]),
            Action::SetVariable,
            push(vec![Av::Str(s("n")), Av::Int(3)]),
            Action::SetVariable,
            push(vec![Av::Str(s("b")), Av::Bool(true)]),
            Action::SetVariable,
            // o = new Object(); o.x = 5
            push(vec![Av::Str(s("o")), Av::Int(0), Av::Str(s("Object"))]),
            Action::NewObject,
            Action::SetVariable,
            push(vec![Av::Str(s("o"))]),
            Action::GetVariable,
            push(vec![Av::Str(s("x")), Av::Int(5)]),
            Action::SetMember,
            // o.add = function (a, b) { return a + b + this.x }; o.me = function () { return this }
            push(vec![Av::Str(s("o"))]),
            Action::GetVariable,
            push(vec![Av::Str(s("add"))]),
            Action::DefineFunction(DefineFunction { name: s(""), params: vec![s("a"), s("b")], actions: &add_body }),
            Action::SetMember,
            push(vec![Av::Str(s("o"))]),
            Action::GetVariable,
            push(vec![Av::Str(s("me"))]),
            Action::DefineFunction(DefineFunction { name: s(""), params: vec![], actions: &me_body }),
            Action::SetMember,
            // getX = function (obj) { return obj.x }
            push(vec![Av::Str(s("getX"))]),
            Action::DefineFunction(DefineFunction { name: s(""), params: vec![s("obj")], actions: &get_x_body }),
            Action::SetVariable,
            // getURL("event:flash_start 1, \"2\""); fscommand("ready", "now"); getURL("lingo:startGame()")
            Action::GetUrl(GetUrl { url: s("event:flash_start 1, \"2\""), target: s("") }),
            Action::GetUrl(GetUrl { url: s("FSCommand:ready"), target: s("now") }),
            Action::GetUrl(GetUrl { url: s("lingo:startGame()"), target: s("") }),
            Action::Stop,
        ]);
        let frame2 = code(&[push(vec![Av::Str(s("called")), Av::Str(s("yes"))]), Action::SetVariable]);
        let frame3 = code(&[push(vec![Av::Str(s("atEnd")), Av::Str(s("1"))]), Action::SetVariable, Action::Stop]);
        movie(&[(None, frame1), (None, frame2), (Some("end"), frame3)])
    }

    struct NoPixels;
    impl Canvas for NoPixels {
        fn renderer(&self, width: u32, height: u32) -> Result<Box<dyn RenderBackend>, String> {
            Ok(Box::new(ruffle_render::backend::null::NullRenderer::new(ViewportDimensions { width, height, scale_factor: 1.0 })))
        }
        fn read(&self, _: &mut dyn RenderBackend) -> Option<(u32, u32, Vec<u8>)> {
            None
        }
    }

    fn quiet(_: &str) {}

    fn setup(canvas: Box<dyn Canvas>) -> Setup {
        Setup {
            canvas,
            navigator: Box::new(|_| Box::new(ruffle_core::backend::navigator::NullNavigatorBackend::new())),
            storage: Box::new(|| Box::new(ruffle_core::backend::storage::MemoryStorageBackend::new())),
            mixers: None,
            movie_url: "file:///games/test/movie.dcr".to_string(),
            spoofed_url: None,
            quality: StageQuality::High,
            max_execution: Duration::from_secs(5),
            player_version: None,
            log: quiet,
        }
    }

    fn text(v: &str) -> FlashValue {
        FlashValue::String(v.to_string())
    }

    #[test]
    fn lingo_and_a_flash_member_talk_as_through_the_flash_asset_xtra() {
        let members = FlashMembers::new(setup(Box::new(NoPixels)));
        let mut host = MemberHost(Rc::clone(&members));
        let _ = flash_host::take_calls();

        // made before the sprite's player is there: kept, made after its first frame
        assert!(!host.is_ready(5));
        assert_eq!(host.get_variable(5, "_level0.isReady"), FlashValue::Null);
        assert!(host.set_variable(5, "_level0.progress", "40"));
        assert!(host.set_variable(5, "_level0.isReady", "queued"));

        host.load(5, 1, 4, &intro(), 120, 60, false, -1);
        assert!(host.is_ready(5));
        assert_eq!(host.get_variable(5, "_level0.progress"), text("40"));
        assert_eq!(host.get_variable(5, "_level0.isReady"), text("queued"), "frame 1 ran first, then the kept call");
        // what frame 1 said to Lingo, in order
        assert_eq!(
            flash_host::take_calls(),
            vec![
                FlashCall::Event { cast_lib: 1, cast_member: 4, body: "flash_start 1, \"2\"".into() },
                FlashCall::Event { cast_lib: 1, cast_member: 4, body: "ready now".into() },
                FlashCall::Lingo("startGame()".into()),
            ]
        );

        // getVariable: a primitive as ActionScript's toString makes it, an object by its path
        assert_eq!(host.get_variable(5, "_level0.n"), text("3"));
        assert_eq!(host.get_variable(5, "_level0.b"), text("true"));
        assert_eq!(host.get_variable(5, "_level0.o"), FlashValue::Object("_level0.o".into()));
        assert_eq!(host.get_variable(5, "_level0.o.x"), text("5"));
        assert_eq!(host.get_variable(5, "_level0.nothing"), FlashValue::Undefined);
        assert_eq!(host.get_variable(5, "/:n"), text("3"), "Flash 4 paths, as getFlashProperty uses");

        // functions: typed answers, `this` the object, objects kept for later calls
        assert_eq!(host.call_function(5, "_level0.o.add", &[FlashValue::Number(1.0), FlashValue::Number(2.0)]), FlashValue::Number(8.0));
        assert_eq!(host.call_function(5, "_level0.getX", &[FlashValue::Object("_level0.o".into())]), FlashValue::Number(5.0));
        let FlashValue::Object(kept) = host.call_function(5, "_level0.o.me", &[]) else {
            panic!("an object comes back as a handle");
        };
        assert!(kept.starts_with("_level0.__dirplayer_ref_"), "{kept}");
        assert_eq!(host.call_function(5, &format!("{kept}.add"), &[FlashValue::Number(10.0), FlashValue::Number(0.0)]), FlashValue::Number(15.0));
        assert_eq!(host.call_function(5, "_level0.missing", &[]), FlashValue::Undefined);

        // newObject: the object is made, where the handle says
        assert!(host.new_object(5, "Array", &[FlashValue::Number(1.0), FlashValue::Number(2.0), FlashValue::Number(3.0)], "_root.__dpObj_Array_1"));
        assert_eq!(host.get_variable(5, "_level0.__dpObj_Array_1.length"), text("3"));
        assert!(host.new_object(5, "Object", &[], "_root.__dpObj_Object_2"));
        assert!(host.set_variable(5, "_root.__dpObj_Object_2.y", "7"));
        assert_eq!(host.get_variable(5, "_level0.__dpObj_Object_2.y"), text("7"));
        assert!(!host.new_object(5, "NoSuchClass", &[], "_root.x"));

        // the timeline: frame 1 stopped itself (playing is FALSE then), 3 frames
        assert_eq!(host.frame_count(5), 3);
        assert_eq!(host.current_frame(5), 1);
        assert!(!host.is_playing(5));
        // callFrame runs frame 2's actions; the playhead stays
        host.call_frame(5, 2);
        assert_eq!(host.get_variable(5, "_level0.called"), text("yes"));
        assert_eq!(host.current_frame(5), 1);
        // play / stop
        host.play(5);
        assert!(host.is_playing(5));
        host.stop(5);
        assert!(!host.is_playing(5));
        // gotoFrame by label: its actions run (and stop it there)
        host.goto_frame(5, "end");
        assert_eq!(host.current_frame(5), 3);
        assert_eq!(host.get_variable(5, "_level0.atEnd"), text("1"));
        // `sprite.frame = 2` on a member that is not pausedAtStart: there, playing on
        host.goto_frame_and_stop(5, "2");
        assert_eq!(host.current_frame(5), 2);
        assert!(host.is_playing(5));
        host.rewind(5);
        assert_eq!(host.current_frame(5), 1);
        assert!(!host.is_playing(5));

        // nothing under a point of an empty stage
        assert_eq!(host.hit_test(5, 10.0, 10.0), 0);
        assert!(host.mouse(5, MouseKind::Down, 10, 10, 120, 60));
        assert!(!host.mouse(6, MouseKind::Down, 10, 10, 120, 60));

        // the sprite stops showing it
        host.unload(5);
        assert!(!host.is_ready(5));
        assert_eq!(host.get_variable(5, "_level0.n"), FlashValue::Null);
        assert_eq!(members.borrow().count(), 0);
    }

    #[test]
    fn a_paused_at_start_member_and_a_frame_a_script_set_wait_there() {
        let members = FlashMembers::new(setup(Box::new(NoPixels)));
        let mut host = MemberHost(Rc::clone(&members));
        let frames: Vec<(Option<&str>, Vec<u8>)> = (0..4).map(|_| (None, code(&[]))).collect();
        let swf = movie(&frames);
        host.load(1, 1, 1, &swf, 100, 80, true, -1);
        assert_eq!(host.current_frame(1), 1);
        assert!(!host.is_playing(1), "pausedAtStart: frame 1, stopped");
        host.goto_frame_and_stop(1, "3");
        assert_eq!(host.current_frame(1), 3);
        assert!(!host.is_playing(1), "pausedAtStart: `frame =` stops there");
        host.load(2, 1, 1, &swf, 100, 80, false, 4);
        assert_eq!(host.current_frame(2), 4);
        assert!(!host.is_playing(2), "the frame a script set for the sprite");
        host.load(3, 1, 1, &swf, 100, 80, false, -1);
        assert!(host.is_playing(3), "an animated member plays");
        members.borrow_mut().tick(Duration::from_millis(40));
        assert_eq!(host.current_frame(3), 2);
        // a member swap: the sprite's last player goes
        host.load(3, 1, 2, &intro(), 100, 80, false, -1);
        assert_eq!(host.frame_count(3), 3);
        assert_eq!(members.borrow().count(), 3);
        host.reset();
        assert_eq!(members.borrow().count(), 0);
        let _ = flash_host::take_calls();
    }

    /// On a GPU (dcr-probe and these tests on a PC: SwiftShader's Vulkan), the member's picture
    /// comes back with its art and, where it draws nothing, transparent.
    #[test]
    fn a_member_is_drawn_and_read_back_transparent_where_empty() {
        if std::env::var("UISHOT_DIR").is_err() {
            return;
        }
        let instance = wgpu::Instance::new({
            let mut d = wgpu::InstanceDescriptor::new_without_display_handle();
            d.backends = wgpu::Backends::VULKAN;
            d
        });
        let adapter = pollster_like(instance.request_adapter(&wgpu::RequestAdapterOptions::default())).expect("adapter");
        let (device, queue) = pollster_like(adapter.request_device(&wgpu::DeviceDescriptor::default())).expect("device");
        let descriptors = Arc::new(ruffle_render_wgpu::descriptors::Descriptors::new(instance, adapter, device, queue));
        let members = FlashMembers::new(setup(Box::new(WgpuCanvas { descriptors })));
        let mut host = MemberHost(Rc::clone(&members));
        host.load(2, 1, 9, include_bytes!("../test-swf/bounce.swf"), 160, 90, false, -1);
        let m = members.borrow();
        let inst = m.instances.get(&2).unwrap();
        let mut p = inst.player.lock().unwrap();
        p.render();
        let (w, h, rgba) = m.setup.canvas.read(p.renderer_mut()).expect("a picture");
        assert_eq!((w, h), inst.render);
        let clear = rgba.chunks(4).filter(|px| px[3] == 0).count();
        let drawn = rgba.chunks(4).filter(|px| px[3] == 255).count();
        assert!(clear > 0, "nothing is transparent");
        assert!(drawn > 0, "nothing was drawn");
    }

    fn pollster_like<F: std::future::Future>(f: F) -> F::Output {
        let mut f = std::pin::pin!(f);
        let waker = std::task::Waker::noop();
        let mut cx = std::task::Context::from_waker(waker);
        loop {
            if let std::task::Poll::Ready(v) = f.as_mut().poll(&mut cx) {
                return v;
            }
            std::thread::yield_now();
        }
    }

    #[test]
    fn frame_numbers_and_labels_are_told_apart_as_on_the_web() {
        assert_eq!(frame_number("12"), Some(12));
        assert_eq!(frame_number(" 7 "), Some(7));
        assert_eq!(frame_number("-3"), Some(1));
        assert_eq!(frame_number("0"), Some(1));
        assert_eq!(frame_number("3frame"), None);
        assert_eq!(frame_number("intro"), None);
        assert_eq!(frame_number(""), None);
        assert_eq!(frame_number("99999999"), Some(u16::MAX));
    }

    #[test]
    fn event_and_lingo_addresses_are_recognised_in_any_case() {
        assert_eq!(strip_scheme("event:flash_start_game", "event:"), Some("flash_start_game"));
        assert_eq!(strip_scheme("EVENT: send #done", "event:"), Some(" send #done"));
        assert_eq!(strip_scheme("lingo:startGame", "lingo:"), Some("startGame"));
        assert_eq!(strip_scheme("http://x/event:y", "event:"), None);
        assert_eq!(strip_scheme("ev", "event:"), None);
        assert_eq!(strip_scheme("évent:", "event:"), None);
    }
}
