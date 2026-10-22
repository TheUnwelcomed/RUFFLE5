//! PS5_RUFFLE_INPUT_V3 — the DualSense as Ruffle's keyboard and mouse (and the menu's pad).
//!
//! The pad is read through shell/pad.c (scePad, the hardware-verified sample layout). Each pad
//! button does what the controls say (src/controls.rs: the defaults, `/data/ruffle/controls.txt`,
//! `<movie>.controls.txt`): one or more keys, sent to Ruffle as ordinary key events, or a mouse
//! click at the pointer. The left stick is a d-pad by default; the touchpad is the screen (its
//! surface reports 1920x1080 coordinates), a touch moves the mouse there; the right stick moves it
//! too; the touchpad's click and R2 are the left button, L2 the right. The pointer is drawn by
//! src/cursor.rs from the state this module keeps up to date.
//!
//! A key held by two buttons at once is pressed once and released when the last button lets go.
//! `raw_down` / `raw_pressed` carry the plain button bits of the last poll for the shell's own
//! screens (src/menu.rs), which read the pad directly rather than through a game's mapping.
// SPDX-License-Identifier: GPL-3.0-or-later

use crate::controls::{Action, Button, Controls, CursorMode, Key, StickRole};
use crate::cursor::SharedCursor;
use ruffle_core::events::{MouseButton, PlayerEvent};
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
    pub report: String,
    /// The last poll's buttons (BTN_ bits; the sticks as d-pad and the triggers merged in).
    pub raw_down: u32,
    /// The buttons that went down in the last poll.
    pub raw_pressed: u32,
}

impl Input {
    /// Initialise the pad services; the controller itself is opened lazily (`poll` retries).
    pub fn new(width: u32, height: u32, controls: Controls, cursor: SharedCursor) -> Self {
        let mut detail = 0i32;
        let rc = unsafe { ps5_pad_init(&mut detail) };
        let report = if rc == 0 {
            format!("pad services up, user {detail}")
        } else {
            format!(
                "pad services: step {rc} failed with 0x{:08x}",
                detail as u32
            )
        };
        let pointer = (width as f64 / 2.0, height as f64 / 2.0);
        if let Ok(mut c) = cursor.lock() {
            c.x = pointer.0 as f32;
            c.y = pointer.1 as f32;
            c.scale = controls.cursor_scale;
            c.visible = controls.cursor == CursorMode::On;
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
            report,
            raw_down: 0,
            raw_pressed: 0,
        }
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

    /// Hide the pointer (the menu draws none) and forget the holds.
    pub fn reset(&mut self) {
        self.keys_held.clear();
        self.left_held = 0;
        self.right_held = 0;
        if let Ok(mut c) = self.cursor.lock() {
            c.visible = false;
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

    /// Read the pad and translate what changed since the last poll into events. `dt` paces the
    /// stick-driven pointer and the cursor's hide timer. Lines worth logging (the controller
    /// appearing) come back too.
    pub fn poll(&mut self, dt: f64, events: &mut Vec<PlayerEvent>) -> Option<String> {
        let note = self.try_open();
        self.raw_pressed = 0;
        if !self.open {
            return note;
        }
        let mut s = PadState::default();
        let got = unsafe { ps5_pad_read(&mut s) };
        if got <= 0 {
            // nothing new (or a refused read): keep moving the pointer with the last stick reading
            s = self.last;
            if got < 0 {
                s.buttons = 0;
            }
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
        for (role, (ax, ay)) in [
            (self.controls.left_stick, left_axes),
            (self.controls.right_stick, right_axes),
        ] {
            if role != StickRole::Mouse || (ax.abs() <= STICK_DEADZONE && ay.abs() <= STICK_DEADZONE) {
                continue;
            }
            let speed = self.controls.pointer_speed;
            let scale = |v: i32| {
                let v = (if v.abs() > STICK_DEADZONE { v } else { 0 }) as f64;
                let sign = v.signum();
                let mag = ((v.abs() - STICK_DEADZONE as f64) / (127.0 - STICK_DEADZONE as f64))
                    .clamp(0.0, 1.0);
                sign * mag * mag * speed * dt
            };
            let nx = (self.pointer.0 + scale(ax)).clamp(0.0, self.viewport.0);
            let ny = (self.pointer.1 + scale(ay)).clamp(0.0, self.viewport.1);
            if (nx, ny) != self.pointer {
                self.pointer = (nx, ny);
                moved = true;
            }
        }
        let (x, y) = self.pointer;
        if moved {
            events.push(PlayerEvent::MouseMove { x, y });
            self.idle = 0.0;
        } else {
            self.idle += dt;
        }
        let visible = match self.controls.cursor {
            CursorMode::On => true,
            CursorMode::Off => false,
            CursorMode::Auto => self.touching || self.idle < self.controls.cursor_hide,
        };
        if let Ok(mut c) = self.cursor.lock() {
            c.x = x as f32;
            c.y = y as f32;
            c.visible = visible;
        }

        // --- buttons: the pad's bits, the sticks as d-pads, the triggers past their threshold ---
        let mut held = s.buttons & !BTN_INTERCEPTED & !(BTN_L2 | BTN_R2);
        for (role, (ax, ay)) in [
            (self.controls.left_stick, left_axes),
            (self.controls.right_stick, right_axes),
        ] {
            if role != StickRole::Dpad {
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
        self.raw_down = held;
        self.raw_pressed = changed & held;
        if changed != 0 {
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
                            events.push(PlayerEvent::KeyDown {
                                key: key.descriptor(),
                            });
                        }
                    } else if *count > 0 {
                        *count -= 1;
                        if *count == 0 {
                            events.push(PlayerEvent::KeyUp {
                                key: key.descriptor(),
                            });
                        }
                    }
                }
                Action::Click(mouse) => {
                    let count = if mouse == MouseButton::Right {
                        &mut self.right_held
                    } else {
                        &mut self.left_held
                    };
                    if pressed {
                        *count += 1;
                        if *count == 1 {
                            events.push(PlayerEvent::MouseDown {
                                x,
                                y,
                                button: mouse,
                                index: None,
                            });
                        }
                    } else if *count > 0 {
                        *count -= 1;
                        if *count == 0 {
                            events.push(PlayerEvent::MouseUp {
                                x,
                                y,
                                button: mouse,
                            });
                        }
                    }
                }
            }
        }
    }
}
