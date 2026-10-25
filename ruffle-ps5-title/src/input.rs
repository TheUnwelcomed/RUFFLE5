//! PS5_RUFFLE_INPUT_V4 — the DualSense (and a USB keyboard and mouse) as Ruffle's keyboard and
//! mouse, and as the menus' pad.
//!
//! The pad is read through shell/pad.c (scePad, the hardware-verified sample layout). Each pad
//! button does what the controls say (src/controls.rs): one or more keys, sent to Ruffle as ordinary
//! key events, a mouse click at the pointer, or the on-screen keyboard. The left stick is a d-pad by
//! default; the touchpad is the screen (its surface reports 1920x1080 coordinates), a touch moves
//! the mouse there; the right stick moves it too. The pointer is drawn by src/cursor.rs from the
//! state this module keeps up to date.
//!
//! V4: the pointer lives in the game's own pixels (`set_viewport`: 1920x1080, or 3840x2160 for a
//! game in 4K), sticks moving it at the same speed either way. A USB keyboard and mouse (src/hid.rs)
//! play too: keys and typed characters go to the game, the mouse points, clicks and scrolls, Esc
//! asks for the quick menu (`menu_request`). In the menus (`game` false) the keyboard's arrows,
//! Enter, Esc and Tab act as the pad's d-pad, Cross, Circle and R1, and what is typed is in `typed`.
//!
//! A key held by two buttons at once is pressed once and released when the last button lets go.
//! `raw_down` / `raw_pressed` carry the plain button bits of the last poll for the title's own
//! screens, which read the pad directly rather than through a game's mapping.
// SPDX-License-Identifier: GPL-3.0-or-later

use crate::controls::{Action, Button, Controls, CursorMode, Key, StickRole};
use crate::cursor::SharedCursor;
use crate::hid::{self, Hid, HidFrame};
use ruffle_core::events::{MouseButton, MouseWheelDelta, PlayerEvent};
use std::collections::HashMap;

#[repr(C)]
#[derive(Clone, Copy, Default)]
pub struct PadState {
    pub buttons: u32,
    pub left_x: u8,
    pub left_y: u8,
    pub right_x: u8,
    pub right_y: u8,
    pub left_trigger: u8,
    pub right_trigger: u8,
    pub touch_count: u8,
    pub touch_id: u8,
    pub touch_x: u16,
    pub touch_y: u16,
    pub connected: i32,
}

extern "C" {
    fn ps5_pad_init(detail: *mut i32) -> i32;
    fn ps5_pad_open(detail: *mut i32) -> i32;
    fn ps5_pad_read(out: *mut PadState) -> i32;
}

// The console's button bits (input_ps5.cpp, verified on hardware).
pub const BTN_L3: u32 = 0x000002;
pub const BTN_R3: u32 = 0x000004;
pub const BTN_OPTIONS: u32 = 0x000008;
pub const BTN_UP: u32 = 0x000010;
pub const BTN_RIGHT: u32 = 0x000020;
pub const BTN_DOWN: u32 = 0x000040;
pub const BTN_LEFT: u32 = 0x000080;
pub const BTN_L1: u32 = 0x000400;
pub const BTN_R1: u32 = 0x000800;
pub const BTN_TRIANGLE: u32 = 0x001000;
pub const BTN_CIRCLE: u32 = 0x002000;
pub const BTN_CROSS: u32 = 0x004000;
pub const BTN_SQUARE: u32 = 0x008000;
pub const BTN_TOUCHPAD: u32 = 0x100000;
/// Set while the system has the pad (the PS button overlay); nothing means anything then.
const BTN_INTERCEPTED: u32 = 0x8000_0000;
// Our own bits for the triggers, from their analog values (the layout's trigger bytes are the
// verified ones; the digital L2/R2 bits are not relied on).
pub const BTN_L2: u32 = 0x0100;
pub const BTN_R2: u32 = 0x0200;
pub const DPAD: u32 = BTN_UP | BTN_DOWN | BTN_LEFT | BTN_RIGHT;

const STICK_CENTRE: i32 = 128;
const STICK_DEADZONE: i32 = 40;
/// The touchpad's coordinate space (DualSense: 1920x1080).
const TOUCHPAD: (f64, f64) = (1920.0, 1080.0);
const TRIGGER_ON: u8 = 96;

const PAIRS: [(u32, Button); 16] = [
    (BTN_UP, Button::Up),
    (BTN_DOWN, Button::Down),
    (BTN_LEFT, Button::Left),
    (BTN_RIGHT, Button::Right),
    (BTN_CROSS, Button::Cross),
    (BTN_CIRCLE, Button::Circle),
    (BTN_SQUARE, Button::Square),
    (BTN_TRIANGLE, Button::Triangle),
    (BTN_L1, Button::L1),
    (BTN_R1, Button::R1),
    (BTN_L2, Button::L2),
    (BTN_R2, Button::R2),
    (BTN_L3, Button::L3),
    (BTN_R3, Button::R3),
    (BTN_OPTIONS, Button::Options),
    (BTN_TOUCHPAD, Button::Touchpad),
];

/// The pad as a stream of Ruffle events.
pub struct Input {
    controls: Controls,
    cursor: SharedCursor,
    open: bool,
    last: PadState,
    /// Buttons currently reported down (after the stick-as-d-pad merge and the trigger threshold).
    down: u32,
    /// How many held buttons press each key / mouse button right now.
    keys_held: HashMap<Key, u32>,
    left_held: u32,
    right_held: u32,
    pointer: (f64, f64),
    viewport: (f64, f64),
    touching: bool,
    touch_id: u8,
    /// Seconds since the pointer last moved (for hiding the cursor).
    idle: f64,
    user: i32,
    pub hid: Hid,
    /// The USB keyboard and mouse's last read.
    pub hid_frame: HidFrame,
    pub report: String,
    /// The last poll's buttons (BTN_ bits; the sticks as d-pad, the triggers and, in the menus, the
    /// keyboard's arrows/Enter/Esc/Tab merged in).
    pub raw_down: u32,
    /// The buttons that went down in the last poll.
    pub raw_pressed: u32,
    /// Buttons drive the game (true) or only the title's screens.
    pub game: bool,
    /// The pad's buttons reach the game (false while the on-screen keyboard has them).
    pub pad_to_game: bool,
    /// A button bound to the on-screen keyboard was pressed.
    pub keyboard_toggle: bool,
    /// Esc on a USB keyboard: the quick menu, please.
    pub menu_request: bool,
    /// Characters typed on a USB keyboard this poll (the menus' search), and Backspace presses.
    pub typed: Vec<char>,
    pub backspace: bool,
    /// USB mouse: game pixels per count (Settings).
    pub mouse_speed: f64,
    /// The pointer moved this poll (overlays and the cursor need a fresh frame).
    pub moved: bool,
    /// `raw_pressed` plus the d-pad's repeats while held (lists in the menus).
    pub nav: u32,
    repeat_bits: u32,
    repeat_at: f64,
}

/// A held d-pad direction repeats after this, then every `REPEAT_EVERY` seconds.
const REPEAT_AFTER: f64 = 0.36;
const REPEAT_EVERY: f64 = 0.085;

impl Input {
    /// Initialise the pad services; the controller itself is opened lazily (`poll` retries).
    pub fn new(width: u32, height: u32, controls: Controls, cursor: SharedCursor) -> Self {
        let mut detail = 0i32;
        let rc = unsafe { ps5_pad_init(&mut detail) };
        let (report, user) = if rc == 0 {
            (format!("pad services up, user {detail}"), detail)
        } else {
            (format!("pad services: step {rc} failed with 0x{:08x}", detail as u32), -1)
        };
        let pointer = (width as f64 / 2.0, height as f64 / 2.0);
        if let Ok(mut c) = cursor.lock() {
            c.x = pointer.0 as f32;
            c.y = pointer.1 as f32;
            c.scale = controls.cursor_scale;
            c.visible = false;
            c.space_w = width as f32;
            c.space_h = height as f32;
        }
        Self {
            controls,
            cursor,
            open: false,
            last: PadState::default(),
            down: 0,
            keys_held: HashMap::new(),
            left_held: 0,
            right_held: 0,
            pointer,
            viewport: (width as f64, height as f64),
            touching: false,
            touch_id: 0,
            idle: 0.0,
            user,
            hid: Hid::none(),
            hid_frame: HidFrame::default(),
            report,
            raw_down: 0,
            raw_pressed: 0,
            game: false,
            pad_to_game: true,
            keyboard_toggle: false,
            menu_request: false,
            typed: Vec::new(),
            backspace: false,
            mouse_speed: 1.2,
            moved: false,
            nav: 0,
            repeat_bits: 0,
            repeat_at: 0.0,
        }
    }

    /// Open the USB keyboard and mouse (once, at start); lines for the log.
    pub fn open_usb(&mut self) -> Vec<String> {
        self.hid = Hid::open(self.user);
        std::mem::take(&mut self.hid.report)
    }

    /// A new game's mapping. Nothing is held any more: the player the holds were for is gone.
    pub fn set_controls(&mut self, controls: Controls) {
        self.keys_held.clear();
        self.left_held = 0;
        self.right_held = 0;
        if let Ok(mut c) = self.cursor.lock() {
            c.scale = controls.cursor_scale;
            c.visible = controls.cursor == CursorMode::On;
        }
        self.idle = 0.0;
        self.controls = controls;
    }

    /// The game's pixel size: where the pointer moves. The pointer keeps its place on the screen.
    pub fn set_viewport(&mut self, width: u32, height: u32) {
        let (ow, oh) = self.viewport;
        let (w, h) = (width.max(1) as f64, height.max(1) as f64);
        self.pointer = (self.pointer.0 / ow * w, self.pointer.1 / oh * h);
        self.viewport = (w, h);
        if let Ok(mut c) = self.cursor.lock() {
            c.space_w = w as f32;
            c.space_h = h as f32;
            c.x = self.pointer.0 as f32;
            c.y = self.pointer.1 as f32;
        }
    }

    /// The part of the display the game's coordinates cover ([x, y, w, h] in display pixels), for
    /// drawing the pointer; None: all of it.
    pub fn set_area(&mut self, area: Option<[f32; 4]>) {
        if let Ok(mut c) = self.cursor.lock() {
            c.area = area.unwrap_or([0.0; 4]);
        }
    }

    /// Hide the pointer (the menu draws none) and forget the holds.
    pub fn reset(&mut self) {
        self.keys_held.clear();
        self.left_held = 0;
        self.right_held = 0;
        self.game = false;
        self.pad_to_game = true;
        self.keyboard_toggle = false;
        self.menu_request = false;
        if let Ok(mut c) = self.cursor.lock() {
            c.visible = false;
            c.area = [0.0; 4];
        }
    }

    /// Show or hide the pointer whatever the controls say (the quick menu hides it).
    pub fn set_cursor_visible(&mut self, visible: bool) {
        if let Ok(mut c) = self.cursor.lock() {
            c.visible = visible;
        }
    }

    /// Release every key and button the game believes is held (leaving a game, opening a menu
    /// over it, the on-screen keyboard taking the pad).
    pub fn release_all(&mut self, events: &mut Vec<PlayerEvent>) {
        for (key, count) in self.keys_held.drain() {
            if count > 0 {
                events.push(PlayerEvent::KeyUp { key: key.descriptor() });
            }
        }
        let (x, y) = self.pointer;
        if self.left_held > 0 {
            events.push(PlayerEvent::MouseUp { x, y, button: MouseButton::Left });
        }
        if self.right_held > 0 {
            events.push(PlayerEvent::MouseUp { x, y, button: MouseButton::Right });
        }
        self.left_held = 0;
        self.right_held = 0;
        for usage in self.hid.held_keys() {
            if let Some((key, _)) = hid::key_for(usage, false, false) {
                events.push(PlayerEvent::KeyUp { key });
            }
        }
    }

    fn try_open(&mut self) -> Option<String> {
        if self.open {
            return None;
        }
        let mut detail = 0i32;
        if unsafe { ps5_pad_open(&mut detail) } == 0 {
            self.open = true;
            Some(format!("controller open (handle {detail})"))
        } else {
            None
        }
    }

    /// Read the pad (and the USB devices) and translate what changed since the last poll into
    /// events. `dt` paces the stick-driven pointer and the cursor's hide timer. Lines worth logging
    /// (the controller appearing) come back too.
    pub fn poll(&mut self, dt: f64, events: &mut Vec<PlayerEvent>) -> Option<String> {
        let note = self.try_open();
        self.raw_pressed = 0;
        self.keyboard_toggle = false;
        self.menu_request = false;
        self.typed.clear();
        self.backspace = false;
        self.moved = false;
        self.hid_frame = self.hid.read();
        for line in std::mem::take(&mut self.hid.report) {
            crate::say(&format!("     {line}"));
        }

        let mut s = PadState::default();
        if self.open {
            let got = unsafe { ps5_pad_read(&mut s) };
            if got <= 0 {
                // nothing new (or a refused read): keep moving the pointer with the last stick reading
                s = self.last;
                if got < 0 {
                    s.buttons = 0;
                }
            }
        } else {
            s.left_x = STICK_CENTRE as u8;
            s.left_y = STICK_CENTRE as u8;
            s.right_x = STICK_CENTRE as u8;
            s.right_y = STICK_CENTRE as u8;
        }
        if s.buttons & BTN_INTERCEPTED != 0 {
            s.buttons = 0; // the system overlay has the pad: release everything
            s.left_trigger = 0;
            s.right_trigger = 0;
        }

        // --- the pointer first, so a click lands where the pointer is this frame ---
        let mut moved = false;
        if s.touch_count > 0 {
            let x = (s.touch_x as f64 / TOUCHPAD.0 * self.viewport.0).clamp(0.0, self.viewport.0);
            let y = (s.touch_y as f64 / TOUCHPAD.1 * self.viewport.1).clamp(0.0, self.viewport.1);
            if !self.touching || self.touch_id != s.touch_id || (x, y) != self.pointer {
                self.pointer = (x, y);
                moved = true;
            }
            self.touching = true;
            self.touch_id = s.touch_id;
        } else {
            self.touching = false;
        }
        let left_axes = (s.left_x as i32 - STICK_CENTRE, s.left_y as i32 - STICK_CENTRE);
        let right_axes = (s.right_x as i32 - STICK_CENTRE, s.right_y as i32 - STICK_CENTRE);
        // The stick speed is in 1080p pixels per second: the same feel in a 4K game.
        let speed = self.controls.pointer_speed * self.viewport.0 / TOUCHPAD.0;
        for (role, (ax, ay)) in [(self.controls.left_stick, left_axes), (self.controls.right_stick, right_axes)] {
            if role != StickRole::Mouse || (ax.abs() <= STICK_DEADZONE && ay.abs() <= STICK_DEADZONE) {
                continue;
            }
            let scale = |v: i32| {
                let v = (if v.abs() > STICK_DEADZONE { v } else { 0 }) as f64;
                let sign = v.signum();
                let mag = ((v.abs() - STICK_DEADZONE as f64) / (127.0 - STICK_DEADZONE as f64)).clamp(0.0, 1.0);
                sign * mag * mag * speed * dt
            };
            let nx = (self.pointer.0 + scale(ax)).clamp(0.0, self.viewport.0);
            let ny = (self.pointer.1 + scale(ay)).clamp(0.0, self.viewport.1);
            if (nx, ny) != self.pointer {
                self.pointer = (nx, ny);
                moved = true;
            }
        }
        // A USB mouse.
        if self.hid_frame.mouse_moved() {
            let k = self.mouse_speed * self.viewport.0 / TOUCHPAD.0;
            let nx = (self.pointer.0 + self.hid_frame.dx as f64 * k).clamp(0.0, self.viewport.0);
            let ny = (self.pointer.1 + self.hid_frame.dy as f64 * k).clamp(0.0, self.viewport.1);
            if (nx, ny) != self.pointer {
                self.pointer = (nx, ny);
                moved = true;
            }
        }
        let (x, y) = self.pointer;
        if moved {
            if self.game {
                events.push(PlayerEvent::MouseMove { x, y });
            }
            self.idle = 0.0;
            self.moved = true;
        } else {
            self.idle += dt;
        }
        if self.game {
            let mouse_plugged = self.hid.mouse_connected();
            let visible = match self.controls.cursor {
                CursorMode::On => true,
                CursorMode::Off => false,
                CursorMode::Auto => self.touching || self.idle < self.controls.cursor_hide || mouse_plugged,
            };
            if let Ok(mut c) = self.cursor.lock() {
                c.x = x as f32;
                c.y = y as f32;
                c.visible = visible;
            }
        }

        // --- buttons: the pad's bits, the sticks as d-pads, the triggers past their threshold ---
        let mut held = s.buttons & !BTN_INTERCEPTED & !(BTN_L2 | BTN_R2);
        for (i, (role, (ax, ay))) in [(self.controls.left_stick, left_axes), (self.controls.right_stick, right_axes)].into_iter().enumerate() {
            // In the menus the left stick always moves the selection.
            let as_dpad = role == StickRole::Dpad || (!self.game && i == 0);
            if !as_dpad {
                continue;
            }
            if ax < -STICK_DEADZONE {
                held |= BTN_LEFT;
            }
            if ax > STICK_DEADZONE {
                held |= BTN_RIGHT;
            }
            if ay < -STICK_DEADZONE {
                held |= BTN_UP;
            }
            if ay > STICK_DEADZONE {
                held |= BTN_DOWN;
            }
        }
        if s.left_trigger >= TRIGGER_ON {
            held |= BTN_L2;
        }
        if s.right_trigger >= TRIGGER_ON {
            held |= BTN_R2;
        }
        let changed = held ^ self.down;
        if self.game && self.pad_to_game && changed != 0 {
            // Releases first, then presses: a button swapped for another holding the same key
            // keeps the key down throughout.
            for (bit, button) in PAIRS {
                if changed & bit != 0 && held & bit == 0 {
                    self.apply(button, false, events);
                }
            }
            for (bit, button) in PAIRS {
                if changed & bit != 0 && held & bit != 0 {
                    self.apply(button, true, events);
                }
            }
        }
        self.down = held;
        self.last = s;

        // --- the USB keyboard and mouse ---
        let hf = std::mem::take(&mut self.hid_frame);
        let mut raw = held;
        let mut pressed = changed & held;
        if self.game {
            if hf.key_pressed(hid::KEY_ESCAPE) {
                self.menu_request = true;
            }
            hid::key_events(&hf, events);
            for (bit, button) in [(hid::MOUSE_LEFT, MouseButton::Left), (hid::MOUSE_RIGHT, MouseButton::Right), (hid::MOUSE_MIDDLE, MouseButton::Middle)] {
                if hf.pressed & bit != 0 {
                    events.push(PlayerEvent::MouseDown { x, y, button, index: None });
                }
                if hf.released & bit != 0 {
                    events.push(PlayerEvent::MouseUp { x, y, button });
                }
            }
            if hf.wheel != 0 {
                events.push(PlayerEvent::MouseWheel { delta: MouseWheelDelta::Lines(hf.wheel as f64) });
            }
        } else {
            for &(usage, down) in &hf.keys {
                let bit = match usage {
                    hid::KEY_UP => BTN_UP,
                    hid::KEY_DOWN => BTN_DOWN,
                    hid::KEY_LEFT => BTN_LEFT,
                    hid::KEY_RIGHT => BTN_RIGHT,
                    hid::KEY_ENTER => BTN_CROSS,
                    hid::KEY_ESCAPE => BTN_CIRCLE,
                    hid::KEY_TAB | hid::KEY_PAGE_DOWN => BTN_R1,
                    hid::KEY_PAGE_UP => BTN_L1,
                    hid::KEY_F2 => BTN_SQUARE,
                    hid::KEY_F3 => BTN_TRIANGLE,
                    hid::KEY_BACKSPACE => {
                        if down {
                            self.backspace = true;
                        }
                        0
                    }
                    _ => 0,
                };
                if down && bit != 0 {
                    pressed |= bit;
                    raw |= bit;
                }
            }
            for usage in self.hid.held_keys() {
                raw |= match usage {
                    hid::KEY_UP => BTN_UP,
                    hid::KEY_DOWN => BTN_DOWN,
                    hid::KEY_LEFT => BTN_LEFT,
                    hid::KEY_RIGHT => BTN_RIGHT,
                    hid::KEY_ESCAPE => BTN_CIRCLE,
                    _ => 0,
                };
            }
            self.typed = hf.typed().into_iter().filter(|c| !c.is_control()).collect();
        }
        self.raw_down = raw;
        self.raw_pressed = pressed;
        self.hid_frame = hf;

        // --- the d-pad's repeats, for lists ---
        self.nav = pressed;
        let dirs = raw & DPAD;
        if pressed & DPAD != 0 {
            self.repeat_bits = pressed & DPAD;
            self.repeat_at = REPEAT_AFTER;
        } else if dirs & self.repeat_bits != 0 {
            self.repeat_at -= dt;
            if self.repeat_at <= 0.0 {
                self.nav |= self.repeat_bits & dirs;
                self.repeat_at = REPEAT_EVERY;
            }
        } else {
            self.repeat_bits = 0;
        }
        note
    }

    /// A button went down or up: press or release each of its actions, counting the holds so a
    /// key shared by two buttons is sent once.
    fn apply(&mut self, button: Button, pressed: bool, events: &mut Vec<PlayerEvent>) {
        let actions = match self.controls.bindings.get(&button) {
            Some(a) => a.clone(),
            None => return,
        };
        let (x, y) = self.pointer;
        for action in actions {
            match action {
                Action::Key(key) => {
                    let count = self.keys_held.entry(key).or_insert(0);
                    if pressed {
                        *count += 1;
                        if *count == 1 {
                            events.push(PlayerEvent::KeyDown { key: key.descriptor() });
                        }
                    } else if *count > 0 {
                        *count -= 1;
                        if *count == 0 {
                            events.push(PlayerEvent::KeyUp { key: key.descriptor() });
                        }
                    }
                }
                Action::Click(mouse) => {
                    let count = if mouse == MouseButton::Right { &mut self.right_held } else { &mut self.left_held };
                    if pressed {
                        *count += 1;
                        if *count == 1 {
                            events.push(PlayerEvent::MouseDown { x, y, button: mouse, index: None });
                        }
                    } else if *count > 0 {
                        *count -= 1;
                        if *count == 0 {
                            events.push(PlayerEvent::MouseUp { x, y, button: mouse });
                        }
                    }
                }
                Action::Keyboard => {
                    if pressed {
                        self.keyboard_toggle = true;
                    }
                }
            }
        }
    }
}
