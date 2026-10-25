//! PS5_RUFFLE_HID_V2 — a USB keyboard and mouse plugged into the PS5 (libSceKeyboard, libSceMouse).
//!
//! Ported from Ruffle-Flash-PS5 (src/hid.rs, GPL-3.0, by ZiZc3), whose record layouts come from the
//! device-tested ps5-native-gamepad-input-research notes. The libraries are not loaded for a title:
//! shell/hid.c loads them by their sysmodule ids (V2: first thing in main(), and the functions are
//! found through libSceSysmodule's module handle) and looks their four functions up, so nothing is
//! imported that could be missing; if any step fails the log says which and there is no keyboard or
//! mouse, nothing else changes.
//!
//! The keyboard plays (keys and typed characters to the game; Esc opens the quick menu) and drives
//! the menus (arrows, Enter, Esc, Backspace, typing to search); the mouse points, clicks and scrolls.
// SPDX-License-Identifier: GPL-3.0-or-later

use ruffle_core::events::{KeyDescriptor, KeyLocation, LogicalKey, NamedKey, PhysicalKey, PlayerEvent};
use std::time::{Duration, Instant};

const KEYBOARD: i32 = 0;
const MOUSE: i32 = 1;
/// Device indexes tried for each; one USB receiver used keyboard 1, mouse 0.
const INDEXES: i32 = 4;
/// Held keys repeat after this, then every REPEAT_EVERY (text boxes).
const REPEAT_AFTER: Duration = Duration::from_millis(450);
const REPEAT_EVERY: Duration = Duration::from_millis(35);

#[repr(C)]
#[derive(Clone, Copy)]
struct KeyboardData {
    timestamp: u64,
    intercepted: u8,
    _r0: [u8; 7],
    connected: u8,
    _r1: [u8; 3],
    length: i32,
    leds: u32,
    modifiers: u32,
    keycodes: [u16; 16],
    _r2: [u8; 32],
}

#[repr(C)]
#[derive(Clone, Copy)]
struct MouseData {
    timestamp: u64,
    connected: u8,
    _r0: [u8; 3],
    buttons: u32,
    x: i32,
    y: i32,
    wheel: i32,
    tilt: i32,
    _r1: [u8; 8],
}

const _: () = assert!(core::mem::size_of::<KeyboardData>() == 96);
const _: () = assert!(core::mem::size_of::<MouseData>() == 40);

const MOUSE_INTERCEPTED: u32 = 1 << 31;
pub const MOUSE_LEFT: u32 = 1;
pub const MOUSE_RIGHT: u32 = 2;
pub const MOUSE_MIDDLE: u32 = 4;

extern "C" {
    fn ps5_hid_load(which: i32, detail: *mut i32) -> i32;
    fn ps5_hid_sysmodule_rc(which: i32) -> i32;
    fn ps5_hid_source(which: i32) -> i32;
    fn ps5_hid_init(which: i32) -> i32;
    fn ps5_hid_open(which: i32, user: i32, index: i32) -> i32;
    fn ps5_hid_read(which: i32, handle: i32, data: *mut u8, count: i32) -> i32;
}

/// What the keyboard and mouse did since the last read.
#[derive(Default)]
pub struct HidFrame {
    /// Keys, in order: (USB HID usage, down). Repeats come as extra downs.
    pub keys: Vec<(u16, bool)>,
    /// Mouse motion in its own counts, and wheel notches (up = positive).
    pub dx: i32,
    pub dy: i32,
    pub wheel: i32,
    /// Mouse buttons held, pressed and released (MOUSE_LEFT...).
    pub buttons: u32,
    pub pressed: u32,
    pub released: u32,
    /// Shift held and Caps Lock on, for typed characters.
    pub shift: bool,
    pub caps: bool,
    /// "Keyboard connected" and the like, for a toast.
    pub notices: Vec<&'static str>,
}

impl HidFrame {
    pub fn key_pressed(&self, usage: u16) -> bool {
        self.keys.iter().any(|&(u, down)| u == usage && down)
    }
    pub fn mouse_moved(&self) -> bool {
        self.dx != 0 || self.dy != 0
    }
    /// Characters typed (for search boxes), in order.
    pub fn typed(&self) -> Vec<char> {
        self.keys.iter().filter(|(_, d)| *d).filter_map(|&(u, _)| key_for(u, self.shift, self.caps).and_then(|(_, c)| c)).collect()
    }
}

struct KeyboardPort {
    handle: i32,
    index: i32,
    held: Vec<u16>,
    seen: bool,
    connected: bool,
}

struct MousePort {
    handle: i32,
    index: i32,
    buttons: u32,
    seen: bool,
    connected: bool,
}

pub struct Hid {
    keyboards: Vec<KeyboardPort>,
    mice: Vec<MousePort>,
    caps: bool,
    shift: bool,
    repeat: Option<(u16, Instant)>,
    buttons: u32,
    pub report: Vec<String>,
}

impl Hid {
    /// No devices (before `open`, or when the pad services have no user).
    pub fn none() -> Hid {
        Hid {
            keyboards: Vec::new(),
            mice: Vec::new(),
            caps: false,
            shift: false,
            repeat: None,
            buttons: 0,
            report: Vec::new(),
        }
    }

    /// Loads both libraries and opens every index they accept; devices plugged in later show up on
    /// those handles.
    pub fn open(user: i32) -> Hid {
        let mut hid = Hid::none();
        if user < 0 {
            hid.report.push("usb: no user yet; keyboard and mouse off".into());
            return hid;
        }
        for which in [KEYBOARD, MOUSE] {
            let what = if which == KEYBOARD { "keyboard" } else { "mouse" };
            let mut detail = 0i32;
            let rc = unsafe { ps5_hid_load(which, &mut detail) };
            // shell/hid.c V2: the sysmodule load's own answer (main() makes it first), then the
            // handle lookup's or the step that failed.
            let sysmodule = unsafe { ps5_hid_sysmodule_rc(which) } as u32;
            if rc != 0 {
                let step = match rc {
                    -3 => "functions not bound: no direct imports, no handle lookup",
                    -1 => "no module handle",
                    -2 => "functions not found",
                    _ => "failed",
                };
                hid.report.push(format!(
                    "usb {what}: library not available: sysmodule load 0x{sysmodule:08x}; step {rc} ({step}), 0x{:08x}",
                    detail as u32
                ));
                continue;
            }
            let how = match unsafe { ps5_hid_source(which) } {
                1 => "direct imports".to_string(),
                2 => format!("module handle {detail}"),
                _ => "?".to_string(),
            };
            hid.report.push(format!("usb {what}: sysmodule load 0x{sysmodule:08x}, functions via {how}"));
            let init = unsafe { ps5_hid_init(which) };
            let mut opened = Vec::new();
            for index in 0..INDEXES {
                let handle = unsafe { ps5_hid_open(which, user, index) };
                if handle >= 0 {
                    opened.push((index, handle));
                } else if index > 1 {
                    break;
                }
            }
            hid.report.push(format!("usb {what}: init 0x{:x}, open(user {user}) -> {opened:?}", init as u32));
            for (index, handle) in opened {
                if which == KEYBOARD {
                    hid.keyboards.push(KeyboardPort { handle, index, held: Vec::new(), seen: false, connected: false });
                } else {
                    hid.mice.push(MousePort { handle, index, buttons: 0, seen: false, connected: false });
                }
            }
        }
        hid
    }

    pub fn read(&mut self) -> HidFrame {
        let mut frame = HidFrame::default();
        if self.keyboards.is_empty() && self.mice.is_empty() {
            return frame;
        }
        self.read_keyboards(&mut frame);
        self.read_mice(&mut frame);
        // Auto-repeat of the last key pressed (not modifiers).
        let now = Instant::now();
        if let Some(&(usage, true)) = frame.keys.iter().rev().find(|(u, _)| !is_modifier(*u)) {
            self.repeat = Some((usage, now + REPEAT_AFTER));
        }
        if let Some((usage, at)) = self.repeat {
            let held = self.keyboards.iter().any(|k| k.held.contains(&usage));
            if !held {
                self.repeat = None;
            } else if now >= at {
                frame.keys.push((usage, true));
                self.repeat = Some((usage, now + REPEAT_EVERY));
            }
        }
        frame.shift = self.shift;
        frame.caps = self.caps;
        frame
    }

    fn read_keyboards(&mut self, frame: &mut HidFrame) {
        let mut records: [KeyboardData; 16] = unsafe { core::mem::zeroed() };
        let mut seen_now = Vec::new();
        for port in &mut self.keyboards {
            let n = unsafe { ps5_hid_read(KEYBOARD, port.handle, records.as_mut_ptr() as *mut u8, 16) };
            if n <= 0 {
                continue;
            }
            for r in &records[..(n as usize).min(16)] {
                if (r.connected != 0) != port.connected {
                    port.connected = r.connected != 0;
                    frame.notices.push(if port.connected { "Keyboard connected" } else { "Keyboard disconnected" });
                }
                let mut now: Vec<u16> = Vec::new();
                if r.connected != 0 && r.intercepted == 0 {
                    for bit in 0..8 {
                        if r.modifiers & (1 << bit) != 0 {
                            now.push(0xE0 + bit as u16);
                        }
                    }
                    for &k in &r.keycodes {
                        // 0 = none, 1..3 = rollover/error codes.
                        if k > 3 && !now.contains(&k) {
                            now.push(k);
                        }
                    }
                    self.caps = r.leds & 2 != 0;
                    if !port.seen {
                        port.seen = true;
                        seen_now.push(port.index);
                    }
                }
                for &k in &port.held {
                    if !now.contains(&k) {
                        frame.keys.push((k, false));
                    }
                }
                for &k in &now {
                    if !port.held.contains(&k) {
                        frame.keys.push((k, true));
                    }
                }
                port.held = now;
            }
        }
        for index in seen_now {
            self.report.push(format!("usb keyboard on index {index}"));
        }
        self.shift = self.keyboards.iter().any(|k| k.held.iter().any(|&u| u == 0xE1 || u == 0xE5));
    }

    fn read_mice(&mut self, frame: &mut HidFrame) {
        let mut records: [MouseData; 64] = unsafe { core::mem::zeroed() };
        let mut seen_now = Vec::new();
        for port in &mut self.mice {
            let n = unsafe { ps5_hid_read(MOUSE, port.handle, records.as_mut_ptr() as *mut u8, 64) };
            if n <= 0 {
                continue;
            }
            for r in &records[..(n as usize).min(64)] {
                if (r.connected != 0) != port.connected {
                    port.connected = r.connected != 0;
                    frame.notices.push(if port.connected { "Mouse connected" } else { "Mouse disconnected" });
                }
                if r.connected == 0 || r.buttons & MOUSE_INTERCEPTED != 0 {
                    port.buttons = 0;
                    continue;
                }
                if !port.seen && (r.x != 0 || r.y != 0 || r.buttons != 0 || r.wheel != 0) {
                    port.seen = true;
                    seen_now.push(port.index);
                }
                frame.dx += r.x;
                frame.dy += r.y;
                frame.wheel += r.wheel;
                port.buttons = r.buttons & 0x1f;
                // A click shorter than one read still counts.
                frame.pressed |= port.buttons & !self.buttons;
            }
        }
        for index in seen_now {
            self.report.push(format!("usb mouse on index {index}"));
        }
        let buttons = self.mice.iter().fold(0, |b, m| b | m.buttons);
        frame.pressed |= buttons & !self.buttons;
        frame.released = self.buttons & !buttons;
        frame.buttons = buttons;
        self.buttons = buttons;
    }

    pub fn mouse_connected(&self) -> bool {
        self.mice.iter().any(|m| m.connected)
    }

    pub fn keyboard_connected(&self) -> bool {
        self.keyboards.iter().any(|k| k.connected)
    }

    /// Keys still held, to let go of when leaving a game.
    pub fn held_keys(&self) -> Vec<u16> {
        let mut keys: Vec<u16> = self.keyboards.iter().flat_map(|k| k.held.iter().copied()).collect();
        keys.sort_unstable();
        keys.dedup();
        keys
    }
}

fn is_modifier(usage: u16) -> bool {
    (0xE0..=0xE7).contains(&usage)
}

/// A HID usage as Ruffle's key, with the character it types (US layout).
pub fn key_for(usage: u16, shift: bool, caps: bool) -> Option<(KeyDescriptor, Option<char>)> {
    use NamedKey as N;
    use PhysicalKey as P;
    const LETTERS: [P; 26] = [
        P::KeyA, P::KeyB, P::KeyC, P::KeyD, P::KeyE, P::KeyF, P::KeyG, P::KeyH, P::KeyI, P::KeyJ, P::KeyK, P::KeyL, P::KeyM, P::KeyN, P::KeyO, P::KeyP,
        P::KeyQ, P::KeyR, P::KeyS, P::KeyT, P::KeyU, P::KeyV, P::KeyW, P::KeyX, P::KeyY, P::KeyZ,
    ];
    const DIGITS: [P; 10] = [P::Digit1, P::Digit2, P::Digit3, P::Digit4, P::Digit5, P::Digit6, P::Digit7, P::Digit8, P::Digit9, P::Digit0];
    const NUMPAD: [P; 10] = [P::Numpad1, P::Numpad2, P::Numpad3, P::Numpad4, P::Numpad5, P::Numpad6, P::Numpad7, P::Numpad8, P::Numpad9, P::Numpad0];
    const F_KEYS: [(P, N); 12] = [
        (P::F1, N::F1),
        (P::F2, N::F2),
        (P::F3, N::F3),
        (P::F4, N::F4),
        (P::F5, N::F5),
        (P::F6, N::F6),
        (P::F7, N::F7),
        (P::F8, N::F8),
        (P::F9, N::F9),
        (P::F10, N::F10),
        (P::F11, N::F11),
        (P::F12, N::F12),
    ];
    let ch = |physical: P, c: char| (physical, LogicalKey::Character(c), KeyLocation::Standard);
    let named = |physical: P, n: N| (physical, LogicalKey::Named(n), KeyLocation::Standard);
    let pick = |plain: char, shifted: char| if shift { shifted } else { plain };

    let (physical, logical, location) = match usage {
        0x04..=0x1d => {
            let c = (b'a' + (usage - 0x04) as u8) as char;
            ch(LETTERS[(usage - 0x04) as usize], if shift != caps { c.to_ascii_uppercase() } else { c })
        }
        0x1e..=0x27 => {
            let i = (usage - 0x1e) as usize;
            ch(DIGITS[i], if shift { b"!@#$%^&*()"[i] as char } else { b"1234567890"[i] as char })
        }
        0x28 => named(P::Enter, N::Enter),
        0x29 => named(P::Escape, N::Escape),
        0x2a => named(P::Backspace, N::Backspace),
        0x2b => named(P::Tab, N::Tab),
        0x2c => ch(P::Space, ' '),
        0x2d => ch(P::Minus, pick('-', '_')),
        0x2e => ch(P::Equal, pick('=', '+')),
        0x2f => ch(P::BracketLeft, pick('[', '{')),
        0x30 => ch(P::BracketRight, pick(']', '}')),
        0x31 => ch(P::Backslash, pick('\\', '|')),
        0x33 => ch(P::Semicolon, pick(';', ':')),
        0x34 => ch(P::Quote, pick('\'', '"')),
        0x35 => ch(P::Backquote, pick('`', '~')),
        0x36 => ch(P::Comma, pick(',', '<')),
        0x37 => ch(P::Period, pick('.', '>')),
        0x38 => ch(P::Slash, pick('/', '?')),
        0x39 => named(P::CapsLock, N::CapsLock),
        0x3a..=0x45 => {
            let (p, n) = F_KEYS[(usage - 0x3a) as usize];
            named(p, n)
        }
        0x46 => named(P::PrintScreen, N::PrintScreen),
        0x47 => named(P::ScrollLock, N::ScrollLock),
        0x48 => named(P::Pause, N::Pause),
        0x49 => named(P::Insert, N::Insert),
        0x4a => named(P::Home, N::Home),
        0x4b => named(P::PageUp, N::PageUp),
        0x4c => named(P::Delete, N::Delete),
        0x4d => named(P::End, N::End),
        0x4e => named(P::PageDown, N::PageDown),
        0x4f => named(P::ArrowRight, N::ArrowRight),
        0x50 => named(P::ArrowLeft, N::ArrowLeft),
        0x51 => named(P::ArrowDown, N::ArrowDown),
        0x52 => named(P::ArrowUp, N::ArrowUp),
        0x53 => named(P::NumLock, N::NumLock),
        0x54 => (P::NumpadDivide, LogicalKey::Character('/'), KeyLocation::Numpad),
        0x55 => (P::NumpadMultiply, LogicalKey::Character('*'), KeyLocation::Numpad),
        0x56 => (P::NumpadSubtract, LogicalKey::Character('-'), KeyLocation::Numpad),
        0x57 => (P::NumpadAdd, LogicalKey::Character('+'), KeyLocation::Numpad),
        0x58 => (P::NumpadEnter, LogicalKey::Named(N::Enter), KeyLocation::Numpad),
        0x59..=0x62 => {
            let i = (usage - 0x59) as usize;
            (NUMPAD[i], LogicalKey::Character(b"1234567890"[i] as char), KeyLocation::Numpad)
        }
        0x63 => (P::NumpadDecimal, LogicalKey::Character('.'), KeyLocation::Numpad),
        0x65 => named(P::ContextMenu, N::ContextMenu),
        0xE0 => (P::ControlLeft, LogicalKey::Named(N::Control), KeyLocation::Left),
        0xE1 => (P::ShiftLeft, LogicalKey::Named(N::Shift), KeyLocation::Left),
        0xE2 => (P::AltLeft, LogicalKey::Named(N::Alt), KeyLocation::Left),
        0xE3 => (P::SuperLeft, LogicalKey::Named(N::Super), KeyLocation::Left),
        0xE4 => (P::ControlRight, LogicalKey::Named(N::Control), KeyLocation::Right),
        0xE5 => (P::ShiftRight, LogicalKey::Named(N::Shift), KeyLocation::Right),
        0xE6 => (P::AltRight, LogicalKey::Named(N::Alt), KeyLocation::Right),
        0xE7 => (P::SuperRight, LogicalKey::Named(N::Super), KeyLocation::Right),
        _ => return None,
    };
    let typed = match logical {
        LogicalKey::Character(c) => Some(c),
        _ => None,
    };
    Some((KeyDescriptor { physical_key: physical, logical_key: logical, key_location: location }, typed))
}

/// Ruffle events for keys from the keyboard (text boxes get the characters). Esc is the title's:
/// it opens the quick menu.
pub fn key_events(frame: &HidFrame, events: &mut Vec<PlayerEvent>) {
    for &(usage, down) in &frame.keys {
        if usage == KEY_ESCAPE {
            continue;
        }
        let Some((key, typed)) = key_for(usage, frame.shift, frame.caps) else {
            continue;
        };
        if down {
            events.push(PlayerEvent::KeyDown { key });
            if let Some(c) = typed {
                events.push(PlayerEvent::TextInput { codepoint: c });
            }
        } else {
            events.push(PlayerEvent::KeyUp { key });
        }
    }
}

pub const KEY_ENTER: u16 = 0x28;
pub const KEY_ESCAPE: u16 = 0x29;
pub const KEY_BACKSPACE: u16 = 0x2a;
pub const KEY_TAB: u16 = 0x2b;
pub const KEY_RIGHT: u16 = 0x4f;
pub const KEY_LEFT: u16 = 0x50;
pub const KEY_DOWN: u16 = 0x51;
pub const KEY_UP: u16 = 0x52;
pub const KEY_PAGE_UP: u16 = 0x4b;
pub const KEY_PAGE_DOWN: u16 = 0x4e;
pub const KEY_F2: u16 = 0x3b;
pub const KEY_F3: u16 = 0x3c;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn letters_digits_and_shift() {
        assert_eq!(key_for(0x04, false, false).unwrap().1, Some('a'));
        assert_eq!(key_for(0x04, true, false).unwrap().1, Some('A'));
        assert_eq!(key_for(0x04, true, true).unwrap().1, Some('a'));
        assert_eq!(key_for(0x1e, true, false).unwrap().1, Some('!'));
        assert_eq!(key_for(0x27, false, false).unwrap().1, Some('0'));
        assert_eq!(key_for(KEY_ENTER, false, false).unwrap().1, None);
        assert!(key_for(0x32, false, false).is_none());
    }
}
