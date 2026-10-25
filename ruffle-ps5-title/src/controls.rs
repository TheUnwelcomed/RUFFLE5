//! PS5_RUFFLE_CONTROLS_V4 — what each pad button does, from text files in "Custom Mappings".
//!
//! Flash games have no common control scheme: one wants Space to jump, the next wants Z, a third
//! A/S/D or the mouse. So the mapping is data, layered, generic first and the game's own last:
//!   1. the built-in defaults (`Controls::default`);
//!   2. `/app0/controls.txt` — shipped with the title (all comments: an example);
//!   3. `Custom Mappings/_All Games.controls.txt` — every game (Settings > Controls for all games);
//!   4. the game's own: `Custom Mappings/<movie>.controls.txt` (the editor writes it; presets such as
//!      dadnme.controls.txt ship there), else the copy shipped in the title's own folder;
//!   5. `<movie>.controls.txt` beside the SWF, if there is one (still honoured).
//! Every line a file sets, and every line it gets wrong, is written to ruffle.log.
//!
//! V3: "Custom Mappings" (the title folder, next to games/) holds every mapping; files from the old
//! places (/data/ruffle/controls.txt, /data/ruffle/<movie>.controls.txt, games/<movie>.controls.txt)
//! are moved there once (`migrate`). The controls editor (src/editor.rs) writes a file the way the
//! shipped presets are written: a comment, then only the lines that differ from what the game would
//! get without it (`Controls::file_text`). A button can now open the on-screen keyboard
//! (`keyboard`, L2 by default, where `rightclick` was).
//!
//! File format, one setting per line, `#` starts a comment:
//!
//! ```text
//! cross = space            # a button = the keys it presses, comma-separated: several at once is fine
//! square = z, a            # Square presses Z and A together
//! r2 = click               # click / rightclick = the mouse buttons, at the pointer
//! l2 = keyboard            # opens the on-screen keyboard
//! circle = none            # does nothing
//! up = up, w               # up/down/left/right are the d-pad (and the left stick in dpad mode)
//! left_stick = dpad        # dpad | mouse | off
//! right_stick = mouse      # mouse | dpad | off
//! pointer_speed = 1100     # stick-driven pointer, pixels per second at full deflection (1080p)
//! cursor = auto            # auto (shown while the pointer moves, hidden after cursor_hide seconds) | on | off
//! cursor_hide = 3
//! cursor_scale = 1.75
//! keyboard_size = large    # the on-screen keyboard: large | medium | small | mini
//! keyboard_position = bottom   # bottom | bottom-left | left | top-left | top | top-right | right | bottom-right
//! keyboard_game = under    # under (the keyboard covers the game) | beside (the game shrinks out of its way)
//! keyboard_opacity = 100   # 100 | 80 | 60 | 40: how solid the keyboard's background is
//! ```
//!
//! V4 (1.000.031): the on-screen keyboard's own settings (`keyboard_*`), per game or for all games
//! like everything else here; the controls editor has them as a submenu (Settings › On-screen
//! keyboard for all games, the quick menu's Keyboard for this game for one).
//!
//! Buttons: cross circle square triangle l1 r1 l2 r2 l3 r3 options touchpad up down left right.
//! Keys: a single character (letters, digits, - = [ ] \ ; ' , . / `), or space enter escape(esc)
//! shift control(ctrl) alt tab backspace up down left right pageup pagedown home end insert delete
//! f1..f12 numpad0..numpad9.
// SPDX-License-Identifier: GPL-3.0-or-later

use crate::paths::Paths;
use ruffle_core::events::{KeyDescriptor, KeyLocation, LogicalKey, MouseButton, NamedKey, PhysicalKey};
use std::collections::HashMap;

/// The file in Custom Mappings that applies to every game.
pub const ALL_GAMES_FILE: &str = "_All Games.controls.txt";

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

    /// The name the editor shows.
    pub fn title(self) -> &'static str {
        match self {
            Button::Up => "D-pad Up",
            Button::Down => "D-pad Down",
            Button::Left => "D-pad Left",
            Button::Right => "D-pad Right",
            Button::Cross => "Cross",
            Button::Circle => "Circle",
            Button::Square => "Square",
            Button::Triangle => "Triangle",
            Button::L1 => "L1",
            Button::R1 => "R1",
            Button::L2 => "L2",
            Button::R2 => "R2",
            Button::L3 => "L3 (left stick press)",
            Button::R3 => "R3 (right stick press)",
            Button::Options => "Options",
            Button::Touchpad => "Touchpad press",
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
    /// Opens or closes the on-screen keyboard.
    Keyboard,
}

impl Action {
    pub fn name(&self) -> String {
        match self {
            Action::Key(k) => k.name.to_string(),
            Action::Click(MouseButton::Right) => "rightclick".into(),
            Action::Click(_) => "click".into(),
            Action::Keyboard => "keyboard".into(),
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum StickRole {
    Off,
    Dpad,
    Mouse,
}

impl StickRole {
    pub fn name(self) -> &'static str {
        match self {
            StickRole::Off => "off",
            StickRole::Dpad => "dpad",
            StickRole::Mouse => "mouse",
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum CursorMode {
    Auto,
    On,
    Off,
}

impl CursorMode {
    pub fn name(self) -> &'static str {
        match self {
            CursorMode::Auto => "auto",
            CursorMode::On => "on",
            CursorMode::Off => "off",
        }
    }
}

/// The on-screen keyboard's size (src/keyboard.rs draws each).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum KbSize {
    Large,
    Medium,
    Small,
    Mini,
}

impl KbSize {
    pub const ALL: [KbSize; 4] = [KbSize::Large, KbSize::Medium, KbSize::Small, KbSize::Mini];

    pub fn name(self) -> &'static str {
        match self {
            KbSize::Large => "large",
            KbSize::Medium => "medium",
            KbSize::Small => "small",
            KbSize::Mini => "mini",
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            KbSize::Large => "Large",
            KbSize::Medium => "Medium",
            KbSize::Small => "Small",
            KbSize::Mini => "Mini",
        }
    }

    fn parse(s: &str) -> Option<KbSize> {
        KbSize::ALL.into_iter().find(|k| k.name() == s)
    }
}

/// Where the on-screen keyboard opens. `ALL` goes clockwise round the screen from the bottom (the
/// way the hands of a clock go: bottom, left, top, right); L1 on the open keyboard moves it along.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum KbPos {
    Bottom,
    BottomLeft,
    Left,
    TopLeft,
    Top,
    TopRight,
    Right,
    BottomRight,
}

impl KbPos {
    pub const ALL: [KbPos; 8] = [KbPos::Bottom, KbPos::BottomLeft, KbPos::Left, KbPos::TopLeft, KbPos::Top, KbPos::TopRight, KbPos::Right, KbPos::BottomRight];

    pub fn name(self) -> &'static str {
        match self {
            KbPos::Bottom => "bottom",
            KbPos::BottomLeft => "bottom-left",
            KbPos::Left => "left",
            KbPos::TopLeft => "top-left",
            KbPos::Top => "top",
            KbPos::TopRight => "top-right",
            KbPos::Right => "right",
            KbPos::BottomRight => "bottom-right",
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            KbPos::Bottom => "Bottom",
            KbPos::BottomLeft => "Bottom left",
            KbPos::Left => "Left",
            KbPos::TopLeft => "Top left",
            KbPos::Top => "Top",
            KbPos::TopRight => "Top right",
            KbPos::Right => "Right",
            KbPos::BottomRight => "Bottom right",
        }
    }

    fn parse(s: &str) -> Option<KbPos> {
        let s = s.replace(['_', ' '], "-");
        KbPos::ALL.into_iter().find(|k| k.name() == s)
    }

    /// The next place clockwise (L1 on the open keyboard).
    pub fn next(self) -> KbPos {
        let i = KbPos::ALL.iter().position(|p| *p == self).unwrap_or(0);
        KbPos::ALL[(i + 1) % KbPos::ALL.len()]
    }

    /// The place across the screen (R1 on the open keyboard: bottom and top swap, as before).
    pub fn opposite(self) -> KbPos {
        let i = KbPos::ALL.iter().position(|p| *p == self).unwrap_or(0);
        KbPos::ALL[(i + 4) % KbPos::ALL.len()]
    }
}

/// What the game does while the on-screen keyboard is open.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum KbGame {
    /// The keyboard is drawn over the game.
    Under,
    /// The game is shown smaller, in the part of the screen the keyboard leaves free.
    Beside,
}

impl KbGame {
    pub fn name(self) -> &'static str {
        match self {
            KbGame::Under => "under",
            KbGame::Beside => "beside",
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            KbGame::Under => "Under the keyboard",
            KbGame::Beside => "Beside it (smaller)",
        }
    }
}

/// The background opacities the keyboard offers, in percent.
pub const KB_OPACITIES: [u8; 4] = [100, 80, 60, 40];

/// The on-screen keyboard's settings for a game.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct KeyboardPrefs {
    pub size: KbSize,
    pub position: KbPos,
    pub game: KbGame,
    /// 40..=100 (`KB_OPACITIES`).
    pub opacity: u8,
}

impl Default for KeyboardPrefs {
    /// As the keyboard was before it had settings: large, at the bottom, over the game, solid.
    fn default() -> Self {
        KeyboardPrefs { size: KbSize::Large, position: KbPos::Bottom, game: KbGame::Under, opacity: 100 }
    }
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
    /// The on-screen keyboard's size and place (V4).
    pub keyboard: KeyboardPrefs,
    /// What happened while loading: files found or not, lines applied, lines refused.
    pub report: Vec<String>,
    /// The file that gave the game its own mapping, if one did (for the menu to say so).
    pub source: Option<String>,
}

impl PartialEq for Controls {
    /// The same mapping (the report and where it came from aside).
    fn eq(&self, other: &Controls) -> bool {
        Button::ALL.iter().all(|b| self.bindings.get(b).cloned().unwrap_or_default() == other.bindings.get(b).cloned().unwrap_or_default())
            && self.left_stick == other.left_stick
            && self.right_stick == other.right_stick
            && self.pointer_speed == other.pointer_speed
            && self.cursor == other.cursor
            && self.cursor_hide == other.cursor_hide
            && self.cursor_scale == other.cursor_scale
            && self.keyboard == other.keyboard
    }
}

impl Default for Controls {
    fn default() -> Self {
        let mut bindings = HashMap::new();
        let mut bind = |b: Button, keys: &[&str]| {
            bindings.insert(b, keys.iter().map(|k| parse_action(k).expect("built-in default binding")).collect());
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
        bind(Button::L2, &["keyboard"]);
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
            keyboard: KeyboardPrefs::default(),
            report: Vec::new(),
            source: None,
        }
    }
}

/// A movie's own extension: .swf, or a Director movie's (.dcr .dir .dxr). Its mapping files are
/// named without it.
fn is_movie_ext(ext: &str) -> bool {
    ext.eq_ignore_ascii_case("swf") || crate::dirinfo::MOVIE_EXTS.iter().any(|e| ext.eq_ignore_ascii_case(e))
}

/// The stem the mapping files of a movie are named after ("dadnme" for .../dadnme.swf,
/// "ttbattleblitz" for .../ttbattleblitz.dcr -- the menu's stem, which the editor writes under).
pub fn movie_stem(movie_path: &str) -> String {
    let file_name = movie_path.rsplit('/').next().unwrap_or(movie_path);
    match file_name.rsplit_once('.') {
        Some((stem, ext)) if is_movie_ext(ext) => stem.to_string(),
        _ => file_name.to_string(),
    }
}

/// 1.000.033: the menu's search keyboard (L2 in Games and Flash Browser) has settings of its own:
/// those of the on-screen keyboard for all games, then this file's (Settings > Controls & Sound >
/// Search keyboard). Only its size, place and background count: it is drawn over the menu.
pub const SEARCH_KEYBOARD_FILE: &str = "_Menu Search.keyboard.txt";

pub fn search_keyboard_file(paths: &Paths) -> String {
    format!("{}/{SEARCH_KEYBOARD_FILE}", paths.mappings)
}

/// The mapping file the editor writes for a movie (or for all games, `stem` None).
pub fn mapping_file(paths: &Paths, stem: Option<&str>) -> String {
    match stem {
        Some(stem) => format!("{}/{stem}.controls.txt", paths.mappings),
        None => format!("{}/{ALL_GAMES_FILE}", paths.mappings),
    }
}

/// A shipped preset for the movie, in the title's own folder (read-only), if any.
fn shipped_preset(paths: &Paths, stem: &str) -> Option<String> {
    [format!("{}/Custom Mappings/{stem}.controls.txt", paths.app), format!("{}/{stem}.controls.txt", paths.app)]
        .into_iter()
        .find(|p| std::path::Path::new(p).is_file())
}

impl Controls {
    fn layer(&mut self, path: &str) -> bool {
        match std::fs::read_to_string(path) {
            Ok(text) => {
                self.report.push(format!("controls: reading {path}"));
                self.apply(&text, path);
                true
            }
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                self.report.push(format!("controls: no {path}"));
                false
            }
            Err(e) => {
                self.report.push(format!("controls: cannot read {path}: {e}"));
                false
            }
        }
    }

    /// What every game starts from: the defaults and the shipped example file.
    pub fn base_all(paths: &Paths) -> Controls {
        let mut c = Controls::default();
        c.layer(&format!("{}/controls.txt", paths.app));
        c
    }

    /// What a game gets before its own mapping: `base_all` and the all-games mapping.
    pub fn base_game(paths: &Paths) -> Controls {
        let mut c = Controls::base_all(paths);
        c.layer(&mapping_file(paths, None));
        c
    }

    /// What the search keyboard starts from: the all-games settings, over the menu (never beside).
    pub fn search_base(paths: &Paths) -> Controls {
        let mut c = Controls::base_game(paths);
        c.keyboard.game = KbGame::Under;
        c
    }

    /// The search keyboard's settings: `search_base`, then its own file.
    pub fn search_keyboard(paths: &Paths) -> Controls {
        let mut c = Controls::search_base(paths);
        c.layer(&search_keyboard_file(paths));
        c.keyboard.game = KbGame::Under;
        c
    }

    /// Everything that applies to `movie_path`, in order (module comment).
    pub fn load(paths: &Paths, movie_path: &str) -> Controls {
        let mut c = Controls::base_game(paths);
        let stem = movie_stem(movie_path);
        let own = mapping_file(paths, Some(&stem));
        if c.layer(&own) {
            c.source = Some(own.clone());
        } else if let Some(preset) = shipped_preset(paths, &stem) {
            c.layer(&preset);
            c.source = Some(preset);
        }
        let beside = match movie_path.rsplit_once('.') {
            Some((p, ext)) if is_movie_ext(ext) => format!("{p}.controls.txt"),
            _ => format!("{movie_path}.controls.txt"),
        };
        if beside != own && std::path::Path::new(&beside).is_file() && c.layer(&beside) {
            c.source = Some(beside);
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

    pub fn set(&mut self, key: &str, value: &str) -> Result<String, String> {
        if let Some(button) = Button::parse(key) {
            let mut actions = Vec::new();
            if value != "none" && value != "nothing" && !value.is_empty() {
                for part in value.split(',') {
                    let part = part.trim();
                    if part.is_empty() {
                        continue;
                    }
                    let action = parse_action(part).ok_or_else(|| format!("unknown key `{part}`"))?;
                    if !actions.contains(&action) {
                        actions.push(action);
                    }
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
            "keyboard_size" => {
                self.keyboard.size = KbSize::parse(value).ok_or("expected large, medium, small or mini")?;
                Ok(format!("keyboard_size = {}", self.keyboard.size.name()))
            }
            "keyboard_position" => {
                self.keyboard.position = KbPos::parse(value).ok_or("expected bottom, bottom-left, left, top-left, top, top-right, right or bottom-right")?;
                Ok(format!("keyboard_position = {}", self.keyboard.position.name()))
            }
            "keyboard_game" => {
                self.keyboard.game = match value {
                    "under" | "covered" => KbGame::Under,
                    "beside" | "smaller" | "shrink" => KbGame::Beside,
                    _ => return Err("expected under or beside".into()),
                };
                Ok(format!("keyboard_game = {}", self.keyboard.game.name()))
            }
            "keyboard_opacity" => {
                let v = value.trim_end_matches('%').trim().parse::<u32>().map_err(|_| "expected a percentage")?;
                // the nearest of the offered steps
                self.keyboard.opacity = KB_OPACITIES.into_iter().min_by_key(|o| (*o as i64 - v as i64).abs()).unwrap_or(100);
                Ok(format!("keyboard_opacity = {}", self.keyboard.opacity))
            }
            _ => Err("unknown setting".into()),
        }
    }

    pub fn actions(&self, b: Button) -> Vec<Action> {
        self.bindings.get(&b).cloned().unwrap_or_default()
    }

    /// The whole mapping in effect, one line per button, for the log.
    pub fn summary(&self) -> Vec<String> {
        let mut out = Vec::new();
        for b in Button::ALL {
            out.push(describe(b, &self.actions(b)));
        }
        out.push(format!(
            "left_stick = {:?}, right_stick = {:?}, pointer_speed = {}, cursor = {:?} (hide after {} s, scale {})",
            self.left_stick, self.right_stick, self.pointer_speed, self.cursor, self.cursor_hide, self.cursor_scale
        ));
        let k = &self.keyboard;
        out.push(format!(
            "keyboard: {} at the {}, the game {}, {}% opaque",
            k.size.name(),
            k.position.name(),
            match k.game {
                KbGame::Under => "under it",
                KbGame::Beside => "beside it",
            },
            k.opacity
        ));
        out
    }

    /// The lines that turn `base` into this mapping, in the files' format.
    pub fn diff_lines(&self, base: &Controls) -> Vec<String> {
        let mut out = Vec::new();
        for b in Button::ALL {
            let mine = self.actions(b);
            if mine != base.actions(b) {
                out.push(describe(b, &mine));
            }
        }
        if self.left_stick != base.left_stick {
            out.push(format!("left_stick = {}", self.left_stick.name()));
        }
        if self.right_stick != base.right_stick {
            out.push(format!("right_stick = {}", self.right_stick.name()));
        }
        if self.pointer_speed != base.pointer_speed {
            out.push(format!("pointer_speed = {}", self.pointer_speed));
        }
        if self.cursor != base.cursor {
            out.push(format!("cursor = {}", self.cursor.name()));
        }
        if self.cursor_hide != base.cursor_hide {
            out.push(format!("cursor_hide = {}", self.cursor_hide));
        }
        if self.cursor_scale != base.cursor_scale {
            out.push(format!("cursor_scale = {}", self.cursor_scale));
        }
        let (k, b) = (&self.keyboard, &base.keyboard);
        if k.size != b.size {
            out.push(format!("keyboard_size = {}", k.size.name()));
        }
        if k.position != b.position {
            out.push(format!("keyboard_position = {}", k.position.name()));
        }
        if k.game != b.game {
            out.push(format!("keyboard_game = {}", k.game.name()));
        }
        if k.opacity != b.opacity {
            out.push(format!("keyboard_opacity = {}", k.opacity));
        }
        out
    }

    /// A mapping file as the editor writes it: a heading, then only the lines that differ from
    /// `base` (as the shipped presets are written).
    pub fn file_text(&self, base: &Controls, heading: &str) -> String {
        let mut text = String::new();
        for line in heading.lines() {
            text.push_str("# ");
            text.push_str(line);
            text.push('\n');
        }
        text.push_str("# Written by Ruffle5's controls editor; edit it there or by hand (format: controls.txt in the title folder).\n");
        let lines = self.diff_lines(base);
        if lines.is_empty() {
            text.push_str("# (nothing differs: the buttons do what they do in every game)\n");
        }
        for line in lines {
            text.push_str(&line);
            text.push('\n');
        }
        text
    }
}

pub fn describe(button: Button, actions: &[Action]) -> String {
    if actions.is_empty() {
        format!("{} = none", button.name())
    } else {
        format!("{} = {}", button.name(), actions.iter().map(|a| a.name()).collect::<Vec<_>>().join(", "))
    }
}

/// What the editor shows for a button's actions: "Space", "Z + A", "Mouse click", "Nothing".
pub fn describe_short(actions: &[Action]) -> String {
    if actions.is_empty() {
        return "Nothing".to_string();
    }
    actions.iter().map(|a| choice_label(&a.name())).collect::<Vec<_>>().join(" + ")
}

pub fn parse_action(s: &str) -> Option<Action> {
    match s {
        "click" | "leftclick" | "mouse" => Some(Action::Click(MouseButton::Left)),
        "rightclick" => Some(Action::Click(MouseButton::Right)),
        "keyboard" | "osk" => Some(Action::Keyboard),
        _ => key_by_name(s).map(Action::Key),
    }
}

/// Everything a button can be set to in the editor, in the order its picker shows them (file names).
pub const CHOICES: [&str; 91] = [
    "click", "rightclick", "keyboard", "none", "space", "enter", //
    "up", "down", "left", "right", "shift", "control", //
    "alt", "tab", "escape", "backspace", "delete", "insert", //
    "a", "b", "c", "d", "e", "f", //
    "g", "h", "i", "j", "k", "l", //
    "m", "n", "o", "p", "q", "r", //
    "s", "t", "u", "v", "w", "x", //
    "y", "z", "0", "1", "2", "3", //
    "4", "5", "6", "7", "8", "9", //
    "-", "=", "[", "]", ";", "'", //
    ",", ".", "/", "\\", "`", "home", //
    "end", "pageup", "pagedown", "f1", "f2", "f3", //
    "f4", "f5", "f6", "f7", "f8", "f9", //
    "f10", "f11", "f12", "numpad0", "numpad1", "numpad2", //
    "numpad3", "numpad4", "numpad5", "numpad6", "numpad7", "numpad8", //
    "numpad9",
];

/// How a choice reads on screen.
pub fn choice_label(name: &str) -> String {
    match name {
        "click" => "Mouse click".into(),
        "rightclick" => "Right click".into(),
        "keyboard" => "On-screen keyboard".into(),
        "none" => "Nothing".into(),
        "space" => "Space".into(),
        "enter" => "Enter".into(),
        "up" => "Arrow \u{2191}".into(),
        "down" => "Arrow \u{2193}".into(),
        "left" => "Arrow \u{2190}".into(),
        "right" => "Arrow \u{2192}".into(),
        "shift" => "Shift".into(),
        "control" => "Ctrl".into(),
        "alt" => "Alt".into(),
        "tab" => "Tab".into(),
        "escape" => "Esc".into(),
        "backspace" => "Backspace".into(),
        "delete" => "Delete".into(),
        "insert" => "Insert".into(),
        "home" => "Home".into(),
        "end" => "End".into(),
        "pageup" => "Page Up".into(),
        "pagedown" => "Page Down".into(),
        n if n.starts_with("numpad") => format!("Num {}", &n[6..]),
        n if n.starts_with('f') && n.len() > 1 && n[1..].chars().all(|c| c.is_ascii_digit()) => n.to_uppercase(),
        n if n.chars().count() == 1 => n.to_uppercase(),
        n => n.to_string(),
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

/// Move mapping files from where older Ruffle5 builds read them into Custom Mappings, once:
/// /data/ruffle/controls.txt (every game) -> _All Games.controls.txt, /data/ruffle/<movie>.controls.txt
/// and games/<movie>.controls.txt -> <movie>.controls.txt. A file already in Custom Mappings is kept
/// (the old one stays where it is). Returns lines for the log.
pub fn migrate(paths: &Paths) -> Vec<String> {
    let mut report = Vec::new();
    let _ = std::fs::create_dir_all(&paths.mappings);
    let mut moves: Vec<(String, String)> = Vec::new();
    moves.push((format!("{}/controls.txt", paths.legacy), mapping_file(paths, None)));
    for dir in [&paths.legacy, &paths.games] {
        if let Ok(entries) = std::fs::read_dir(dir) {
            for e in entries.flatten() {
                let name = e.file_name().to_string_lossy().into_owned();
                if let Some(stem) = name.strip_suffix(".controls.txt") {
                    if !stem.is_empty() {
                        moves.push((format!("{dir}/{name}"), mapping_file(paths, Some(stem))));
                    }
                }
            }
        }
    }
    for (from, to) in moves {
        if !std::path::Path::new(&from).is_file() {
            continue;
        }
        if std::path::Path::new(&to).exists() {
            report.push(format!("controls: {from} not moved: {to} already exists"));
            continue;
        }
        if crate::fsutil::move_file(&from, &to) {
            report.push(format!("controls: moved {from} -> {to}"));
        } else {
            report.push(format!("controls: could not move {from} -> {to}"));
        }
    }
    report
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_paths(tag: &str) -> Paths {
        let root = std::env::temp_dir().join(format!("ruffle5-controls-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        let p = |s: &str| {
            let d = root.join(s);
            std::fs::create_dir_all(&d).unwrap();
            d.to_string_lossy().into_owned()
        };
        let mut paths = Paths::new(&p("data"), &p("games"), &p("Custom Mappings"));
        paths.app = p("app0");
        paths.legacy = p("legacy");
        paths
    }

    #[test]
    fn every_choice_parses_and_has_a_label() {
        for c in CHOICES {
            if c != "none" {
                assert!(parse_action(c).is_some(), "{c}");
            }
            assert!(!choice_label(c).is_empty());
        }
        assert_eq!(choice_label("f11"), "F11");
        assert_eq!(choice_label("numpad7"), "Num 7");
        assert_eq!(choice_label("q"), "Q");
        assert_eq!(choice_label("up"), "Arrow \u{2191}");
    }

    #[test]
    fn defaults_have_the_keyboard_on_l2() {
        let c = Controls::default();
        assert_eq!(c.actions(Button::L2), vec![Action::Keyboard]);
        assert_eq!(describe_short(&c.actions(Button::Cross)), "Space");
    }

    #[test]
    fn a_file_lists_only_what_differs_and_reads_back_the_same() {
        let base = Controls::default();
        let mut c = base.clone();
        c.set("cross", "a").unwrap();
        c.set("square", "s, d").unwrap();
        c.set("left_stick", "mouse").unwrap();
        let text = c.file_text(&base, "Dad 'n Me (dadnme.swf)");
        assert!(text.starts_with("# Dad 'n Me (dadnme.swf)\n"));
        assert!(text.contains("\ncross = a\n"));
        assert!(text.contains("\nsquare = s, d\n"));
        assert!(text.contains("\nleft_stick = mouse\n"));
        assert!(!text.contains("circle"));
        let mut back = base.clone();
        back.apply(&text, "test");
        assert_eq!(back, c);
        assert!(base.file_text(&base, "x").contains("nothing differs"));
    }

    #[test]
    fn layers_apply_in_order_and_the_game_file_wins() {
        let paths = temp_paths("layers");
        std::fs::write(format!("{}/controls.txt", paths.app), "cross = enter\n").unwrap();
        std::fs::write(mapping_file(&paths, None), "cross = z\ncircle = x\n").unwrap();
        std::fs::write(mapping_file(&paths, Some("game")), "cross = a\n").unwrap();
        let movie = format!("{}/game.swf", paths.games);
        let c = Controls::load(&paths, &movie);
        assert_eq!(describe_short(&c.actions(Button::Cross)), "A");
        assert_eq!(describe_short(&c.actions(Button::Circle)), "X");
        assert_eq!(c.source.as_deref(), Some(mapping_file(&paths, Some("game")).as_str()));
        // without its own file a game gets the all-games mapping
        let d = Controls::load(&paths, &format!("{}/other.swf", paths.games));
        assert_eq!(describe_short(&d.actions(Button::Cross)), "Z");
        assert_eq!(d.source, None);
        // a shipped preset applies when Custom Mappings has none
        std::fs::create_dir_all(format!("{}/Custom Mappings", paths.app)).unwrap();
        std::fs::write(format!("{}/Custom Mappings/other.controls.txt", paths.app), "square = q\n").unwrap();
        let e = Controls::load(&paths, &format!("{}/other.swf", paths.games));
        assert_eq!(describe_short(&e.actions(Button::Square)), "Q");
    }

    #[test]
    fn director_movies_use_the_menu_stem() {
        assert_eq!(movie_stem("/g/ttbattleblitz/ttbattleblitz.dcr"), "ttbattleblitz");
        assert_eq!(movie_stem("/g/Movie.DIR"), "Movie");
        assert_eq!(movie_stem("/g/dadnme.swf"), "dadnme");
        assert_eq!(movie_stem("/g/odd.bin"), "odd.bin");
        let paths = temp_paths("director");
        std::fs::write(mapping_file(&paths, Some("blitz")), "cross = x\n").unwrap();
        let c = Controls::load(&paths, &format!("{}/blitz/blitz.dcr", paths.games));
        assert_eq!(describe_short(&c.actions(Button::Cross)), "X");
    }

    #[test]
    fn shipped_presets_read_cleanly() {
        let dir = concat!(env!("CARGO_MANIFEST_DIR"), "/Custom Mappings");
        let mut n = 0;
        for e in std::fs::read_dir(dir).unwrap().filter_map(|e| e.ok()) {
            let path = e.path().to_string_lossy().into_owned();
            if !path.ends_with(".controls.txt") {
                continue;
            }
            let mut c = Controls::default();
            for raw in std::fs::read_to_string(&path).unwrap().lines() {
                let line = raw.split('#').next().unwrap_or("").trim();
                if line.is_empty() {
                    continue;
                }
                let (k, v) = line.split_once('=').unwrap_or_else(|| panic!("{path}: not `name = value`: {raw}"));
                if let Err(e) = c.set(&k.trim().to_ascii_lowercase(), &v.trim().to_ascii_lowercase()) {
                    panic!("{path}: {e}: {raw}");
                }
            }
            n += 1;
        }
        assert!(n >= 2);
    }

    #[test]
    fn old_files_move_into_custom_mappings() {
        let paths = temp_paths("migrate");
        std::fs::write(format!("{}/controls.txt", paths.legacy), "cross = a\n").unwrap();
        std::fs::write(format!("{}/dadnme.controls.txt", paths.legacy), "square = s\n").unwrap();
        std::fs::write(format!("{}/x.controls.txt", paths.games), "r1 = q\n").unwrap();
        std::fs::write(mapping_file(&paths, Some("keep")), "l1 = z\n").unwrap();
        std::fs::write(format!("{}/keep.controls.txt", paths.games), "l1 = y\n").unwrap();
        let report = migrate(&paths);
        assert!(std::path::Path::new(&mapping_file(&paths, None)).is_file());
        assert!(std::path::Path::new(&mapping_file(&paths, Some("dadnme"))).is_file());
        assert!(std::path::Path::new(&mapping_file(&paths, Some("x"))).is_file());
        assert!(!std::path::Path::new(&format!("{}/controls.txt", paths.legacy)).exists());
        // an existing mapping is not overwritten
        assert_eq!(std::fs::read_to_string(mapping_file(&paths, Some("keep"))).unwrap(), "l1 = z\n");
        assert!(report.iter().any(|l| l.contains("already exists")));
    }

    #[test]
    fn keyboard_settings_parse_layer_and_write_back() {
        let paths = temp_paths("keyboard");
        // for every game, then one game's own
        std::fs::write(mapping_file(&paths, None), "keyboard_size = small\nkeyboard_position = bottom-right\n").unwrap();
        std::fs::write(mapping_file(&paths, Some("rpg")), "keyboard_game = beside\nkeyboard_opacity = 75%\nkeyboard_position = Top_Left\n").unwrap();
        let c = Controls::load(&paths, &format!("{}/rpg.swf", paths.games));
        assert_eq!(c.keyboard, KeyboardPrefs { size: KbSize::Small, position: KbPos::TopLeft, game: KbGame::Beside, opacity: 80 });
        // another game gets only the all-games lines
        let other = Controls::load(&paths, &format!("{}/other.swf", paths.games));
        assert_eq!(other.keyboard, KeyboardPrefs { size: KbSize::Small, position: KbPos::BottomRight, ..KeyboardPrefs::default() });
        // the editor writes only what differs from what the game would get anyway
        let base = Controls::base_game(&paths);
        let lines = c.diff_lines(&base);
        assert!(lines.contains(&"keyboard_position = top-left".to_string()), "{lines:?}");
        assert!(lines.contains(&"keyboard_game = beside".to_string()));
        assert!(lines.contains(&"keyboard_opacity = 80".to_string()));
        assert!(!lines.iter().any(|l| l.starts_with("keyboard_size")), "{lines:?}");
        let mut again = base.clone();
        again.apply(&c.file_text(&base, "rpg"), "rpg");
        assert_eq!(again.keyboard, c.keyboard);
        // a wrong value is refused and leaves the setting as it was
        let mut d = Controls::default();
        assert!(d.set("keyboard_size", "huge").is_err());
        assert!(d.set("keyboard_position", "middle").is_err());
        assert_eq!(d.keyboard, KeyboardPrefs::default());
        // L1 goes round all eight places, clockwise: from the bottom to the left
        let mut p = KbPos::Bottom;
        for _ in 0..8 {
            p = p.next();
        }
        assert_eq!(p, KbPos::Bottom);
        assert_eq!(KbPos::Bottom.next(), KbPos::BottomLeft);
        assert_eq!(KbPos::Left.next(), KbPos::TopLeft);
        assert_eq!(KbPos::Top.next(), KbPos::TopRight);
        assert_eq!(KbPos::Right.next(), KbPos::BottomRight);
        assert_eq!(KbPos::Bottom.opposite(), KbPos::Top);
        assert_eq!(KbPos::Left.opposite(), KbPos::Right);
        assert_eq!(KbPos::BottomRight.opposite(), KbPos::TopLeft);
    }
}
