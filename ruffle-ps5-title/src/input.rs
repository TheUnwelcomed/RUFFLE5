//! PS5_RUFFLE_INPUT_V1 — the DualSense as Ruffle's keyboard and mouse.
//!
//! The pad is read through shell/pad.c (scePad, the hardware-verified sample layout). Buttons go
//! to Ruffle as gamepad events, which its own input manager turns into key presses through the
//! mapping given to `PlayerBuilder::with_gamepad_button_mapping` (`mapping()` below); the left
//! stick doubles as the d-pad. The pointer: the touchpad is the screen (its surface reports
//! 1920x1080 coordinates, the viewport's own size), a touch moves the mouse there, the touchpad's
//! click is a left click; the right stick moves the pointer too, and R2/L2 are the left/right
//! buttons. Ruffle draws no cursor; the game's own hover feedback shows where it is.
//!
//! Defaults, meant for the usual Flash game: d-pad / left stick = arrows, Cross = Space, Circle =
//! Escape, Square = Z, Triangle = X, L1 = Shift, R1 = Control, Options = Enter.
// SPDX-License-Identifier: GPL-3.0-or-later

use ruffle_core::events::{GamepadButton, KeyCode, MouseButton, PlayerEvent};
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
const BTN_L3: u32 = 0x000002;
const BTN_R3: u32 = 0x000004;
const BTN_OPTIONS: u32 = 0x000008;
const BTN_UP: u32 = 0x000010;
const BTN_RIGHT: u32 = 0x000020;
const BTN_DOWN: u32 = 0x000040;
const BTN_LEFT: u32 = 0x000080;
const BTN_L1: u32 = 0x000400;
const BTN_R1: u32 = 0x000800;
const BTN_TRIANGLE: u32 = 0x001000;
const BTN_CIRCLE: u32 = 0x002000;
const BTN_CROSS: u32 = 0x004000;
const BTN_SQUARE: u32 = 0x008000;
const BTN_TOUCHPAD: u32 = 0x100000;
/// Set while the system has the pad (the PS button overlay); nothing means anything then.
const BTN_INTERCEPTED: u32 = 0x8000_0000;

const STICK_CENTRE: i32 = 128;
const STICK_DEADZONE: i32 = 40;
/// Pointer speed with the right stick fully deflected, in viewport pixels per second.
const POINTER_SPEED: f64 = 1100.0;
/// The touchpad's coordinate space (DualSense: 1920x1080).
const TOUCHPAD: (f64, f64) = (1920.0, 1080.0);
const TRIGGER_ON: u8 = 96;

/// Which Flash keys the pad's buttons produce (Ruffle maps them; a game sees ordinary key events).
pub fn mapping() -> HashMap<GamepadButton, KeyCode> {
    HashMap::from([
        (GamepadButton::DPadUp, KeyCode::UP),
        (GamepadButton::DPadDown, KeyCode::DOWN),
        (GamepadButton::DPadLeft, KeyCode::LEFT),
        (GamepadButton::DPadRight, KeyCode::RIGHT),
        (GamepadButton::South, KeyCode::SPACE),
        (GamepadButton::East, KeyCode::ESCAPE),
        (GamepadButton::West, KeyCode::Z),
        (GamepadButton::North, KeyCode::X),
        (GamepadButton::LeftTrigger, KeyCode::SHIFT),
        (GamepadButton::RightTrigger, KeyCode::CONTROL),
        (GamepadButton::Start, KeyCode::ENTER),
        (GamepadButton::Select, KeyCode::TAB),
    ])
}

/// The pad as a stream of Ruffle events.
pub struct Input {
    open: bool,
    last: PadState,
    /// Gamepad buttons currently reported down (after the stick-as-d-pad merge).
    down: u32,
    pointer: (f64, f64),
    viewport: (f64, f64),
    left_down: bool,
    right_down: bool,
    touching: bool,
    touch_id: u8,
    pub report: String,
}

impl Input {
    /// Initialise the pad services; the controller itself is opened lazily (`poll` retries).
    pub fn new(width: u32, height: u32) -> Self {
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
        Self {
            open: false,
            last: PadState::default(),
            down: 0,
            pointer: (width as f64 / 2.0, height as f64 / 2.0),
            viewport: (width as f64, height as f64),
            left_down: false,
            right_down: false,
            touching: false,
            touch_id: 0,
            report,
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
    /// stick-driven pointer. Lines worth logging (the controller appearing) come back too.
    pub fn poll(&mut self, dt: f64, events: &mut Vec<PlayerEvent>) -> Option<String> {
        let note = self.try_open();
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
        }

        // --- buttons (and the left stick as a d-pad) -> gamepad events, Ruffle maps them to keys ---
        let mut held = s.buttons & !BTN_INTERCEPTED;
        let lx = s.left_x as i32 - STICK_CENTRE;
        let ly = s.left_y as i32 - STICK_CENTRE;
        if lx < -STICK_DEADZONE {
            held |= BTN_LEFT;
        }
        if lx > STICK_DEADZONE {
            held |= BTN_RIGHT;
        }
        if ly < -STICK_DEADZONE {
            held |= BTN_UP;
        }
        if ly > STICK_DEADZONE {
            held |= BTN_DOWN;
        }
        let pairs: [(u32, GamepadButton); 13] = [
            (BTN_UP, GamepadButton::DPadUp),
            (BTN_DOWN, GamepadButton::DPadDown),
            (BTN_LEFT, GamepadButton::DPadLeft),
            (BTN_RIGHT, GamepadButton::DPadRight),
            (BTN_CROSS, GamepadButton::South),
            (BTN_CIRCLE, GamepadButton::East),
            (BTN_SQUARE, GamepadButton::West),
            (BTN_TRIANGLE, GamepadButton::North),
            (BTN_L1, GamepadButton::LeftTrigger),
            (BTN_R1, GamepadButton::RightTrigger),
            (BTN_OPTIONS, GamepadButton::Start),
            (BTN_L3, GamepadButton::Select),
            (BTN_R3, GamepadButton::Select),
        ];
        let changed = held ^ self.down;
        for (bit, button) in pairs {
            if changed & bit != 0 {
                events.push(if held & bit != 0 {
                    PlayerEvent::GamepadButtonDown { button }
                } else {
                    PlayerEvent::GamepadButtonUp { button }
                });
            }
        }
        self.down = held;

        // --- the pointer: touchpad (absolute) and right stick (relative) ---
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
        let rx = s.right_x as i32 - STICK_CENTRE;
        let ry = s.right_y as i32 - STICK_CENTRE;
        if rx.abs() > STICK_DEADZONE || ry.abs() > STICK_DEADZONE {
            let scale = |v: i32| {
                let v = (if v.abs() > STICK_DEADZONE { v } else { 0 }) as f64;
                let sign = v.signum();
                let mag = ((v.abs() - STICK_DEADZONE as f64) / (127.0 - STICK_DEADZONE as f64))
                    .clamp(0.0, 1.0);
                sign * mag * mag * POINTER_SPEED * dt
            };
            let nx = (self.pointer.0 + scale(rx)).clamp(0.0, self.viewport.0);
            let ny = (self.pointer.1 + scale(ry)).clamp(0.0, self.viewport.1);
            if (nx, ny) != self.pointer {
                self.pointer = (nx, ny);
                moved = true;
            }
        }
        let (x, y) = self.pointer;
        if moved {
            events.push(PlayerEvent::MouseMove { x, y });
        }

        // --- mouse buttons: touchpad click or R2 = left, L2 = right ---
        let left = s.buttons & BTN_TOUCHPAD != 0 || s.right_trigger >= TRIGGER_ON;
        let right = s.left_trigger >= TRIGGER_ON;
        if left != self.left_down {
            self.left_down = left;
            events.push(if left {
                PlayerEvent::MouseDown {
                    x,
                    y,
                    button: MouseButton::Left,
                    index: None,
                }
            } else {
                PlayerEvent::MouseUp {
                    x,
                    y,
                    button: MouseButton::Left,
                }
            });
        }
        if right != self.right_down {
            self.right_down = right;
            events.push(if right {
                PlayerEvent::MouseDown {
                    x,
                    y,
                    button: MouseButton::Right,
                    index: None,
                }
            } else {
                PlayerEvent::MouseUp {
                    x,
                    y,
                    button: MouseButton::Right,
                }
            });
        }
        self.last = s;
        note
    }
}
