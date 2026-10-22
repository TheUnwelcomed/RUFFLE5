//! PS5_RUFFLE_CONTROLS_V2 — what each pad button does, from a text file.
//!
//! Flash games have no common control scheme: one wants Space to jump, the next wants Z, a third
//! A/S/D or the mouse. So the mapping is data, layered, generic first and the game's own last:
//! the built-in defaults (`Controls::default`), `/app0/controls.txt` (shipped with the title),
//! `/data/ruffle/controls.txt` (yours, all games), `/app0/<movie>.controls.txt` (a preset shipped
//! for that game — `presets/` in the repo), `/data/ruffle/<movie>.controls.txt` (yours, that
//! game). Every line a file sets, and every line it gets wrong, is written to ruffle.log.
//!
//! File format, one setting per line, `#` starts a comment:
//!
//! ```text
//! cross = space            # a button = the keys it presses, comma-separated: several at once is fine
//! square = z, a            # Square presses Z and A together
//! r2 = click               # click / rightclick = the mouse buttons, at the pointer
//! circle = none            # does nothing
//! up = up, w               # up/down/left/right are the d-pad (and the left stick in dpad mode)
//! left_stick = dpad        # dpad | mouse | off
//! right_stick = mouse      # mouse | dpad | off
//! pointer_speed = 1100     # stick-driven pointer, pixels per second at full deflection
//! cursor = auto            # auto (shown while the pointer moves, hidden after cursor_hide seconds) | on | off
//! cursor_hide = 3
//! cursor_scale = 1.75
//! ```
//!
//! Buttons: cross circle square triangle l1 r1 l2 r2 l3 r3 options touchpad up down left right.
//! Keys: a single character (letters, digits, - = [ ] \ ; ' , . / `), or space enter escape(esc)
//! shift control(ctrl) alt tab backspace up down left right pageup pagedown home end insert delete
//! f1..f12 numpad0..numpad9.
// SPDX-License-Identifier: GPL-3.0-or-later

use ruffle_core::events::{KeyDescriptor, KeyLocation, LogicalKey, MouseButton, NamedKey, PhysicalKey};
use std::collections::HashMap;

/// A pad button, as the files name it (the order is the `NAMES` table's).
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub enum Button {
    Up,
    Down,
    Left,
    Right,
    Cross,
    Circle,
    Square,
    Triangle,
    L1,
    R1,
    L2,
    R2,
    L3,
    R3,
    Options,
    Touchpad,
}

impl Button {
    pub const ALL: [Button; 16] = [
        Button::Up,
        Button::Down,
        Button::Left,
        Button::Right,
        Button::Cross,
        Button::Circle,
        Button::Square,
        Button::Triangle,
        Button::L1,
        Button::R1,
        Button::L2,
        Button::R2,
        Button::L3,
        Button::R3,
        Button::Options,
        Button::Touchpad,
    ];

    pub fn name(self) -> &'static str {
        match self {
            Button::Up => "up",
            Button::Down => "down",
            Button::Left => "left",
            Button::Right => "right",
            Button::Cross => "cross",
            Button::Circle => "circle",
            Button::Square => "square",
            Button::Triangle => "triangle",
            Button::L1 => "l1",
            Button::R1 => "r1",
            Button::L2 => "l2",
            Button::R2 => "r2",
            Button::L3 => "l3",
            Button::R3 => "r3",
            Button::Options => "options",
            Button::Touchpad => "touchpad",
        }
    }

    fn parse(s: &str) -> Option<Button> {
        Button::ALL.into_iter().find(|b| b.name() == s)
    }
}

/// A key as Ruffle wants it: the physical key keeps KeyUp paired with its KeyDown (Ruffle drops
/// a KeyUp whose physical key is not down), the logical key becomes the Flash key code.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub struct Key {
    pub physical: PhysicalKey,
    pub logical: LogicalKey,
    pub location: KeyLocation,
    pub name: &'static str,
}

impl Key {
    pub fn descriptor(&self) -> KeyDescriptor {
        KeyDescriptor {
            physical_key: self.physical,
            logical_key: self.logical,
            key_location: self.location,
        }
    }
}

#[derive(Clone, Copy, PartialEq, Debug)]
pub enum Action {
    Key(Key),
    Click(MouseButton),
}

impl Action {
    pub fn name(&self) -> String {
        match self {
            Action::Key(k) => k.name.to_string(),
            Action::Click(MouseButton::Left) => "click".into(),
            Action::Click(MouseButton::Right) => "rightclick".into(),
            Action::Click(_) => "click".into(),
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum StickRole {
    Off,
    Dpad,
    Mouse,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum CursorMode {
    Auto,
    On,
    Off,
}

#[derive(Clone, Debug)]
pub struct Controls {
    pub bindings: HashMap<Button, Vec<Action>>,
    pub left_stick: StickRole,
    pub right_stick: StickRole,
    pub pointer_speed: f64,
    pub cursor: CursorMode,
    pub cursor_hide: f64,
    pub cursor_scale: f32,
    /// What happened while loading: files found or not, lines applied, lines refused.
    pub report: Vec<String>,
}

impl Default for Controls {
    fn default() -> Self {
        let mut bindings = HashMap::new();
        let mut bind = |b: Button, keys: &[&str]| {
            bindings.insert(
                b,
                keys.iter()
                    .map(|k| parse_action(k).expect("built-in default binding"))
                    .collect(),
            );
        };
        bind(Button::Up, &["up"]);
        bind(Button::Down, &["down"]);
        bind(Button::Left, &["left"]);
        bind(Button::Right, &["right"]);
        bind(Button::Cross, &["space"]);
        bind(Button::Circle, &["c"]);
        bind(Button::Square, &["z"]);
        bind(Button::Triangle, &["x"]);
        bind(Button::L1, &["shift"]);
        bind(Button::R1, &["control"]);
        bind(Button::L2, &["rightclick"]);
        bind(Button::R2, &["click"]);
        bind(Button::L3, &["a"]);
        bind(Button::R3, &["s"]);
        bind(Button::Options, &["enter"]);
        bind(Button::Touchpad, &["click"]);
        Self {
            bindings,
            left_stick: StickRole::Dpad,
            right_stick: StickRole::Mouse,
            pointer_speed: 1100.0,
            cursor: CursorMode::Auto,
            cursor_hide: 3.0,
            cursor_scale: 1.75,
            report: Vec::new(),
        }
    }
}

impl Controls {
    /// Defaults, then the generic files, then the movie's own (`<movie>.controls.txt`, the `.swf`
    /// replaced; the shipped preset is looked up by the movie's file name in the title folder),
    /// each layered over the previous.
    pub fn load(data_dir: &str, movie_path: &str) -> Self {
        let mut c = Controls::default();
        let file_name = movie_path.rsplit('/').next().unwrap_or(movie_path);
        let stem = match file_name.rsplit_once('.') {
            Some((stem, ext)) if ext.eq_ignore_ascii_case("swf") => stem,
            _ => file_name,
        };
        let per_game = match movie_path.rsplit_once('.') {
            Some((p, ext)) if ext.eq_ignore_ascii_case("swf") => format!("{p}.controls.txt"),
            _ => format!("{movie_path}.controls.txt"),
        };
        let layers = [
            "/app0/controls.txt".to_string(),
            format!("{data_dir}/controls.txt"),
            format!("/app0/{stem}.controls.txt"),
            per_game,
        ];
        for path in layers {
            match std::fs::read_to_string(&path) {
                Ok(text) => {
                    c.report.push(format!("controls: reading {path}"));
                    c.apply(&text, &path);
                }
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                    c.report.push(format!("controls: no {path}"));
                }
                Err(e) => c.report.push(format!("controls: cannot read {path}: {e}")),
            }
        }
        c
    }

    /// Apply one file's lines over the current settings.
    pub fn apply(&mut self, text: &str, path: &str) {
        for (n, raw) in text.lines().enumerate() {
            let line = raw.split('#').next().unwrap_or("").trim();
            if line.is_empty() {
                continue;
            }
            let Some((k, v)) = line.split_once('=') else {
                self.report.push(format!("controls: {path}:{}: not `name = value`: {raw}", n + 1));
                continue;
            };
            let key = k.trim().to_ascii_lowercase();
            let value = v.trim().to_ascii_lowercase();
            match self.set(&key, &value) {
                Ok(shown) => self.report.push(format!("controls: {path}:{}: {shown}", n + 1)),
                Err(e) => self.report.push(format!("controls: {path}:{}: {e}: {raw}", n + 1)),
            }
        }
    }

    fn set(&mut self, key: &str, value: &str) -> Result<String, String> {
        if let Some(button) = Button::parse(key) {
            let mut actions = Vec::new();
            if value != "none" && value != "nothing" && !value.is_empty() {
                for part in value.split(',') {
                    let part = part.trim();
                    if part.is_empty() {
                        continue;
                    }
                    actions.push(parse_action(part).ok_or_else(|| format!("unknown key `{part}`"))?);
                }
            }
            let shown = describe(button, &actions);
            self.bindings.insert(button, actions);
            return Ok(shown);
        }
        match key {
            "left_stick" | "right_stick" => {
                let role = match value {
                    "dpad" | "arrows" => StickRole::Dpad,
                    "mouse" | "pointer" => StickRole::Mouse,
                    "off" | "none" => StickRole::Off,
                    _ => return Err("expected dpad, mouse or off".into()),
                };
                if key == "left_stick" {
                    self.left_stick = role;
                } else {
                    self.right_stick = role;
                }
                Ok(format!("{key} = {value}"))
            }
            "pointer_speed" => {
                self.pointer_speed = value.parse::<f64>().map_err(|_| "expected a number")?.clamp(50.0, 10000.0);
                Ok(format!("pointer_speed = {}", self.pointer_speed))
            }
            "cursor" => {
                self.cursor = match value {
                    "auto" => CursorMode::Auto,
                    "on" | "always" => CursorMode::On,
                    "off" | "never" => CursorMode::Off,
                    _ => return Err("expected auto, on or off".into()),
                };
                Ok(format!("cursor = {value}"))
            }
            "cursor_hide" => {
                self.cursor_hide = value.parse::<f64>().map_err(|_| "expected seconds")?.clamp(0.2, 600.0);
                Ok(format!("cursor_hide = {}", self.cursor_hide))
            }
            "cursor_scale" => {
                self.cursor_scale = value.parse::<f32>().map_err(|_| "expected a number")?.clamp(0.5, 8.0);
                Ok(format!("cursor_scale = {}", self.cursor_scale))
            }
            _ => Err("unknown setting".into()),
        }
    }

    /// The whole mapping in effect, one line per button, for the log.
    pub fn summary(&self) -> Vec<String> {
        let mut out = Vec::new();
        for b in Button::ALL {
            let actions = self.bindings.get(&b).cloned().unwrap_or_default();
            out.push(describe(b, &actions));
        }
        out.push(format!(
            "left_stick = {:?}, right_stick = {:?}, pointer_speed = {}, cursor = {:?} (hide after {} s, scale {})",
            self.left_stick, self.right_stick, self.pointer_speed, self.cursor, self.cursor_hide, self.cursor_scale
        ));
        out
    }
}

fn describe(button: Button, actions: &[Action]) -> String {
    if actions.is_empty() {
        format!("{} = none", button.name())
    } else {
        format!(
            "{} = {}",
            button.name(),
            actions.iter().map(|a| a.name()).collect::<Vec<_>>().join(", ")
        )
    }
}

fn parse_action(s: &str) -> Option<Action> {
    match s {
        "click" | "leftclick" | "mouse" => Some(Action::Click(MouseButton::Left)),
        "rightclick" => Some(Action::Click(MouseButton::Right)),
        _ => key_by_name(s).map(Action::Key),
    }
}

/// The keys a file may name. Letters are sent as their lowercase character (Ruffle maps it to the
/// uppercase Flash key code, as a keyboard would); the physical key is the matching US-layout one,
/// distinct per key so KeyUp always finds its KeyDown.
pub fn key_by_name(s: &str) -> Option<Key> {
    use KeyLocation::Standard;
    use LogicalKey::{Character as Ch, Named};
    use PhysicalKey as P;
    let key = |name: &'static str, physical: PhysicalKey, logical: LogicalKey, location: KeyLocation| {
        Some(Key {
            physical,
            logical,
            location,
            name,
        })
    };
    let mut chars = s.chars();
    if let (Some(c), None) = (chars.next(), chars.next()) {
        let (name, physical): (&'static str, PhysicalKey) = match c {
            'a' => ("a", P::KeyA),
            'b' => ("b", P::KeyB),
            'c' => ("c", P::KeyC),
            'd' => ("d", P::KeyD),
            'e' => ("e", P::KeyE),
            'f' => ("f", P::KeyF),
            'g' => ("g", P::KeyG),
            'h' => ("h", P::KeyH),
            'i' => ("i", P::KeyI),
            'j' => ("j", P::KeyJ),
            'k' => ("k", P::KeyK),
            'l' => ("l", P::KeyL),
            'm' => ("m", P::KeyM),
            'n' => ("n", P::KeyN),
            'o' => ("o", P::KeyO),
            'p' => ("p", P::KeyP),
            'q' => ("q", P::KeyQ),
            'r' => ("r", P::KeyR),
            's' => ("s", P::KeyS),
            't' => ("t", P::KeyT),
            'u' => ("u", P::KeyU),
            'v' => ("v", P::KeyV),
            'w' => ("w", P::KeyW),
            'x' => ("x", P::KeyX),
            'y' => ("y", P::KeyY),
            'z' => ("z", P::KeyZ),
            '0' => ("0", P::Digit0),
            '1' => ("1", P::Digit1),
            '2' => ("2", P::Digit2),
            '3' => ("3", P::Digit3),
            '4' => ("4", P::Digit4),
            '5' => ("5", P::Digit5),
            '6' => ("6", P::Digit6),
            '7' => ("7", P::Digit7),
            '8' => ("8", P::Digit8),
            '9' => ("9", P::Digit9),
            '-' => ("-", P::Minus),
            '=' => ("=", P::Equal),
            '[' => ("[", P::BracketLeft),
            ']' => ("]", P::BracketRight),
            '\\' => ("\\", P::Backslash),
            ';' => (";", P::Semicolon),
            '\'' => ("'", P::Quote),
            ',' => (",", P::Comma),
            '.' => (".", P::Period),
            '/' => ("/", P::Slash),
            '`' => ("`", P::Backquote),
            _ => return None,
        };
        return key(name, physical, Ch(c), Standard);
    }
    match s {
        "space" => key("space", P::Space, Ch(' '), Standard),
        "enter" | "return" => key("enter", P::Enter, Named(NamedKey::Enter), Standard),
        "escape" | "esc" => key("escape", P::Escape, Named(NamedKey::Escape), Standard),
        "shift" => key("shift", P::ShiftLeft, Named(NamedKey::Shift), KeyLocation::Left),
        "control" | "ctrl" => key("control", P::ControlLeft, Named(NamedKey::Control), KeyLocation::Left),
        "alt" => key("alt", P::AltLeft, Named(NamedKey::Alt), KeyLocation::Left),
        "tab" => key("tab", P::Tab, Named(NamedKey::Tab), Standard),
        "backspace" => key("backspace", P::Backspace, Named(NamedKey::Backspace), Standard),
        "up" => key("up", P::ArrowUp, Named(NamedKey::ArrowUp), Standard),
        "down" => key("down", P::ArrowDown, Named(NamedKey::ArrowDown), Standard),
        "left" => key("left", P::ArrowLeft, Named(NamedKey::ArrowLeft), Standard),
        "right" => key("right", P::ArrowRight, Named(NamedKey::ArrowRight), Standard),
        "pageup" => key("pageup", P::PageUp, Named(NamedKey::PageUp), Standard),
        "pagedown" => key("pagedown", P::PageDown, Named(NamedKey::PageDown), Standard),
        "home" => key("home", P::Home, Named(NamedKey::Home), Standard),
        "end" => key("end", P::End, Named(NamedKey::End), Standard),
        "insert" => key("insert", P::Insert, Named(NamedKey::Insert), Standard),
        "delete" => key("delete", P::Delete, Named(NamedKey::Delete), Standard),
        "f1" => key("f1", P::F1, Named(NamedKey::F1), Standard),
        "f2" => key("f2", P::F2, Named(NamedKey::F2), Standard),
        "f3" => key("f3", P::F3, Named(NamedKey::F3), Standard),
        "f4" => key("f4", P::F4, Named(NamedKey::F4), Standard),
        "f5" => key("f5", P::F5, Named(NamedKey::F5), Standard),
        "f6" => key("f6", P::F6, Named(NamedKey::F6), Standard),
        "f7" => key("f7", P::F7, Named(NamedKey::F7), Standard),
        "f8" => key("f8", P::F8, Named(NamedKey::F8), Standard),
        "f9" => key("f9", P::F9, Named(NamedKey::F9), Standard),
        "f10" => key("f10", P::F10, Named(NamedKey::F10), Standard),
        "f11" => key("f11", P::F11, Named(NamedKey::F11), Standard),
        "f12" => key("f12", P::F12, Named(NamedKey::F12), Standard),
        "numpad0" => key("numpad0", P::Numpad0, Ch('0'), KeyLocation::Numpad),
        "numpad1" => key("numpad1", P::Numpad1, Ch('1'), KeyLocation::Numpad),
        "numpad2" => key("numpad2", P::Numpad2, Ch('2'), KeyLocation::Numpad),
        "numpad3" => key("numpad3", P::Numpad3, Ch('3'), KeyLocation::Numpad),
        "numpad4" => key("numpad4", P::Numpad4, Ch('4'), KeyLocation::Numpad),
        "numpad5" => key("numpad5", P::Numpad5, Ch('5'), KeyLocation::Numpad),
        "numpad6" => key("numpad6", P::Numpad6, Ch('6'), KeyLocation::Numpad),
        "numpad7" => key("numpad7", P::Numpad7, Ch('7'), KeyLocation::Numpad),
        "numpad8" => key("numpad8", P::Numpad8, Ch('8'), KeyLocation::Numpad),
        "numpad9" => key("numpad9", P::Numpad9, Ch('9'), KeyLocation::Numpad),
        _ => None,
    }
}
