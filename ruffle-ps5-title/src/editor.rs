//! PS5_RUFFLE_EDITOR_V2 — the controls editor: what each button does, for one game or for all.
//!
//! Opened from the menu (Triangle on a game: that game; Settings > Controls for all games: every
//! game) and from the quick menu in a game. A list of the pad's buttons and stick settings with what
//! each does now; Cross on a button opens a picker of everything it can do (keys, the mouse
//! buttons, the on-screen keyboard, nothing): Cross sets that alone, Square adds it to or takes it
//! from the button (several keys at once). Triangle puts a row back to what it would be without this
//! mapping. Circle leaves and saves.
//!
//! What is saved is a mapping file in Custom Mappings written the way the shipped presets are:
//! a comment, then only the lines that differ from what the game gets without it
//! (src/controls.rs `file_text`): `<movie>.controls.txt` for a game, `_All Games.controls.txt` for all.
//!
//! V2 (1.000.031): the on-screen keyboard's page — its size, where it opens, the game under it or
//! beside it, how solid its background is (src/controls.rs `KeyboardPrefs`), with ready-made
//! settings for kinds of game (src/keyboard.rs `PRESETS`) and a map of the screen showing where the
//! keyboard and the game go. Square shows the keyboard at full size where it will open (over a game,
//! the game itself moves out of its way: `keyboard_preview`); there L1 / R1 move it, Left / Right
//! size it, Up / Down set its background and Triangle puts the game under it or beside it. The page
//! is a row of the controls list ("On-screen keyboard"), and opens by itself from Settings
//! (On-screen keyboard, for all games) and the quick menu (Keyboard for this game).
// SPDX-License-Identifier: GPL-3.0-or-later

use crate::controls::{self, choice_label, describe_short, parse_action, Button, Controls, CursorMode, KbGame, KbPos, KbSize, KeyboardPrefs, StickRole, CHOICES, KB_OPACITIES};
use crate::input::{BTN_CIRCLE, BTN_CROSS, BTN_DOWN, BTN_L1, BTN_LEFT, BTN_R1, BTN_RIGHT, BTN_SQUARE, BTN_TRIANGLE, BTN_UP};
use crate::keyboard::{self, Osk, PRESETS};
use crate::paths::Paths;
use crate::ui::{Align, Face, Rect, Scene, Ui, H, W};
use crate::widgets::{card, centred, draw_glyph, draw_hints, hints_width, Glyph, Smooth, BLACK, BLUE_2, BLUE_4, BLUE_6, BLUE_9, INK, NIGHT, ON_ORANGE, ORANGE, ORANGE_3, WHITE};

/// Whose controls are being edited.
#[derive(Clone, Debug, PartialEq)]
pub enum Target {
    AllGames,
    Game { stem: String, name: String },
}

#[derive(Clone, Copy, PartialEq, Debug)]
enum ERow {
    Button(Button),
    LeftStick,
    RightStick,
    PointerSpeed,
    Cursor,
    /// V2: opens the on-screen keyboard's page.
    Keyboard,
    ResetAll,
    // the on-screen keyboard's page
    KbKind,
    KbSize,
    KbPosition,
    KbGame,
    KbOpacity,
    KbReset,
}

const KB_ROWS: [ERow; 6] = [ERow::KbKind, ERow::KbSize, ERow::KbPosition, ERow::KbGame, ERow::KbOpacity, ERow::KbReset];

#[derive(Clone, Copy, PartialEq, Debug)]
enum Page {
    Controls,
    Keyboard,
}

const ORDER: [Button; 16] = [
    Button::Cross,
    Button::Circle,
    Button::Square,
    Button::Triangle,
    Button::Up,
    Button::Down,
    Button::Left,
    Button::Right,
    Button::L1,
    Button::R1,
    Button::L2,
    Button::R2,
    Button::L3,
    Button::R3,
    Button::Options,
    Button::Touchpad,
];

fn controls_rows() -> Vec<ERow> {
    let mut rows: Vec<ERow> = ORDER.iter().map(|b| ERow::Button(*b)).collect();
    rows.extend([ERow::LeftStick, ERow::RightStick, ERow::PointerSpeed, ERow::Cursor, ERow::Keyboard, ERow::ResetAll]);
    rows
}

pub fn button_glyph(b: Button) -> Glyph {
    match b {
        Button::Cross => Glyph::Cross,
        Button::Circle => Glyph::Circle,
        Button::Square => Glyph::Square,
        Button::Triangle => Glyph::Triangle,
        Button::Up => Glyph::DPad(0),
        Button::Right => Glyph::DPad(1),
        Button::Down => Glyph::DPad(2),
        Button::Left => Glyph::DPad(3),
        Button::L1 => Glyph::Chip("L1"),
        Button::R1 => Glyph::Chip("R1"),
        Button::L2 => Glyph::Chip("L2"),
        Button::R2 => Glyph::Chip("R2"),
        Button::L3 => Glyph::Chip("L3"),
        Button::R3 => Glyph::Chip("R3"),
        Button::Options => Glyph::Chip("OPTIONS"),
        Button::Touchpad => Glyph::Chip("PAD"),
    }
}

fn stick_label(r: StickRole) -> &'static str {
    match r {
        StickRole::Dpad => "D-pad (arrows)",
        StickRole::Mouse => "Mouse pointer",
        StickRole::Off => "Off",
    }
}

fn cursor_label(c: CursorMode) -> &'static str {
    match c {
        CursorMode::Auto => "Shown while it moves",
        CursorMode::On => "Always shown",
        CursorMode::Off => "Never shown",
    }
}

fn opacity_label(o: u8) -> String {
    if o >= 100 {
        "Solid".to_string()
    } else {
        format!("{o}%")
    }
}

/// The value `dir` steps from `v` in `all`, round.
fn step<T: Copy + PartialEq>(all: &[T], v: T, dir: i32) -> T {
    let i = all.iter().position(|x| *x == v).unwrap_or(0) as i32;
    all[(i + dir).rem_euclid(all.len() as i32) as usize]
}

/// A picture of shape `aspect` as large as fits in `r`, centred.
fn fit_in(r: Rect, aspect: f32) -> Rect {
    let (w, h) = if r.w / r.h > aspect { (r.h * aspect, r.h) } else { (r.w, r.w / aspect) };
    Rect::new(r.x + (r.w - w) * 0.5, r.y + (r.h - h) * 0.5, w, h)
}

/// The keyboard sits in the lower half of the screen.
fn pr_centre_below(panel: Rect) -> bool {
    panel.cy() > H * 0.5
}

/// What a game whose shape is unknown is drawn as on the map: Flash's default stage, 550x400.
const DEFAULT_ASPECT: f32 = 550.0 / 400.0;

const PICK_COLS: usize = 6;
const ROW_H: f32 = 56.0;
const LIST: Rect = Rect::new(96.0, 268.0, 980.0, 700.0);
const SIDE: Rect = Rect::new(1124.0, 268.0, 700.0, 700.0);
/// The keyboard page's map of the screen, at the bottom of the side card.
const MAP: Rect = Rect::new(SIDE.x + (SIDE.w - 512.0) * 0.5, SIDE.y + SIDE.h - 32.0 - 288.0, 512.0, 288.0);

pub struct Picker {
    index: usize,
    scroll: Smooth,
}

pub struct Editor {
    pub target: Target,
    pub controls: Controls,
    base: Controls,
    rows: Vec<ERow>,
    focus: usize,
    scroll: Smooth,
    picker: Option<Picker>,
    pub dirty: bool,
    time: f32,
    file: String,
    /// The list's highlight, gliding between rows.
    hl: Smooth,
    page: Page,
    /// Opened on the keyboard's page by itself (Settings, the quick menu): Circle there leaves.
    keyboard_only: bool,
    /// The controls page's focus and scroll while the keyboard's page is open.
    back: (usize, f32),
    /// Square on the keyboard's page: the keyboard at full size where it opens.
    preview: Option<Osk>,
    /// The player's own keyboard settings from before a ready-made one replaced them (offered
    /// again at the end of the ready-made ones).
    custom: Option<KeyboardPrefs>,
    /// Over a running game (its picture is behind the editor).
    over_game: bool,
    /// The game's shape (width / height) for the map and the preview.
    aspect: f32,
}

impl Editor {
    /// The editor for `target`, starting from what applies now.
    pub fn open(paths: &Paths, target: Target, movie_path: Option<&str>) -> Editor {
        let (controls, base, file) = match &target {
            Target::AllGames => {
                let base = Controls::base_all(paths);
                let mut c = base.clone();
                if let Ok(text) = std::fs::read_to_string(controls::mapping_file(paths, None)) {
                    c.apply(&text, "all games");
                }
                (c, base, controls::mapping_file(paths, None))
            }
            Target::Game { stem, .. } => {
                let base = Controls::base_game(paths);
                let c = match movie_path {
                    Some(p) => Controls::load(paths, p),
                    None => {
                        let mut c = base.clone();
                        if let Ok(text) = std::fs::read_to_string(controls::mapping_file(paths, Some(stem))) {
                            c.apply(&text, stem);
                        }
                        c
                    }
                };
                (c, base, controls::mapping_file(paths, Some(stem)))
            }
        };
        Editor {
            target,
            controls,
            base,
            rows: controls_rows(),
            focus: 0,
            scroll: Smooth::default(),
            picker: None,
            dirty: false,
            time: 0.0,
            file,
            hl: Smooth::default(),
            page: Page::Controls,
            keyboard_only: false,
            back: (0, 0.0),
            preview: None,
            custom: None,
            over_game: false,
            aspect: DEFAULT_ASPECT,
        }
    }

    /// The editor opened on the on-screen keyboard's page (Circle there leaves it).
    pub fn open_keyboard(paths: &Paths, target: Target, movie_path: Option<&str>) -> Editor {
        let mut e = Editor::open(paths, target, movie_path);
        e.keyboard_only = true;
        e.show_keyboard_page();
        e
    }

    /// Over a running game whose picture is `aspect` wide for each unit of height.
    pub fn over_game(mut self, aspect: f32) -> Editor {
        self.over_game = true;
        if aspect.is_finite() && aspect > 0.0 {
            self.aspect = aspect;
        }
        self
    }

    /// The keyboard shown at full size (Square on the keyboard's page), for the game's loop to
    /// move the game out of its way as the keyboard will.
    pub fn keyboard_preview(&self) -> Option<&Osk> {
        self.preview.as_ref()
    }

    pub fn title(&self) -> String {
        match &self.target {
            Target::AllGames => "All games".to_string(),
            Target::Game { name, .. } => name.clone(),
        }
    }

    fn show_keyboard_page(&mut self) {
        self.back = (self.focus, self.scroll.target);
        self.page = Page::Keyboard;
        self.rows = KB_ROWS.to_vec();
        self.focus = 0;
        self.hl.snap(0.0);
        self.scroll.snap(0.0);
    }

    fn show_controls_page(&mut self) {
        self.page = Page::Controls;
        self.rows = controls_rows();
        self.focus = self.back.0.min(self.rows.len() - 1);
        self.hl.snap(self.focus as f32);
        self.scroll.snap(self.back.1);
    }

    fn changed(&self, row: ERow) -> bool {
        let (k, b) = (&self.controls.keyboard, &self.base.keyboard);
        match row {
            ERow::Button(b) => self.controls.actions(b) != self.base.actions(b),
            ERow::LeftStick => self.controls.left_stick != self.base.left_stick,
            ERow::RightStick => self.controls.right_stick != self.base.right_stick,
            ERow::PointerSpeed => self.controls.pointer_speed != self.base.pointer_speed,
            ERow::Cursor => self.controls.cursor != self.base.cursor,
            ERow::Keyboard => k != b,
            ERow::KbSize => k.size != b.size,
            ERow::KbPosition => k.position != b.position,
            ERow::KbGame => k.game != b.game,
            ERow::KbOpacity => k.opacity != b.opacity,
            ERow::ResetAll | ERow::KbKind | ERow::KbReset => false,
        }
    }

    fn value(&self, row: ERow) -> String {
        let k = &self.controls.keyboard;
        match row {
            ERow::Button(b) => describe_short(&self.controls.actions(b)),
            ERow::LeftStick => stick_label(self.controls.left_stick).to_string(),
            ERow::RightStick => stick_label(self.controls.right_stick).to_string(),
            ERow::PointerSpeed => format!("{:.0} px/s", self.controls.pointer_speed),
            ERow::Cursor => cursor_label(self.controls.cursor).to_string(),
            ERow::Keyboard => {
                let mut v = format!("{}, {}", k.size.label(), k.position.label().to_lowercase());
                if k.game == KbGame::Beside {
                    v.push_str(", beside");
                }
                v
            }
            ERow::KbKind => keyboard::preset_of(*k).map(|i| PRESETS[i].0.to_string()).unwrap_or_else(|| "Your own".to_string()),
            ERow::KbSize => k.size.label().to_string(),
            ERow::KbPosition => k.position.label().to_string(),
            ERow::KbGame => k.game.label().to_string(),
            ERow::KbOpacity => opacity_label(k.opacity),
            ERow::ResetAll | ERow::KbReset => String::new(),
        }
    }

    /// The keyboard's settings changed: the preview follows.
    fn keyboard_changed(&mut self) {
        self.dirty = true;
        let k = self.controls.keyboard;
        if let Some(p) = self.preview.as_mut() {
            p.set_prefs(k);
        }
    }

    fn reset_row(&mut self, row: ERow) {
        let b = self.base.keyboard;
        match row {
            ERow::Button(btn) => {
                self.controls.bindings.insert(btn, self.base.actions(btn));
            }
            ERow::LeftStick => self.controls.left_stick = self.base.left_stick,
            ERow::RightStick => self.controls.right_stick = self.base.right_stick,
            ERow::PointerSpeed => self.controls.pointer_speed = self.base.pointer_speed,
            ERow::Cursor => self.controls.cursor = self.base.cursor,
            ERow::Keyboard | ERow::KbKind | ERow::KbReset => self.controls.keyboard = b,
            ERow::KbSize => self.controls.keyboard.size = b.size,
            ERow::KbPosition => self.controls.keyboard.position = b.position,
            ERow::KbGame => self.controls.keyboard.game = b.game,
            ERow::KbOpacity => self.controls.keyboard.opacity = b.opacity,
            ERow::ResetAll => {
                let report = std::mem::take(&mut self.controls.report);
                self.controls = self.base.clone();
                self.controls.report = report;
            }
        }
        self.keyboard_changed();
    }

    /// Left / Right on "Made for": the next ready-made settings; the player's own come back after
    /// the last of them.
    fn cycle_preset(&mut self, dir: i32) {
        let k = self.controls.keyboard;
        if keyboard::preset_of(k).is_none() {
            self.custom = Some(k);
        }
        let mut choices: Vec<KeyboardPrefs> = PRESETS.iter().map(|p| p.1).collect();
        if let Some(c) = self.custom {
            choices.push(c);
        }
        let i = choices.iter().position(|c| *c == k).unwrap_or(choices.len() - 1);
        self.controls.keyboard = choices[(i as i32 + dir).rem_euclid(choices.len() as i32) as usize];
    }

    fn cycle(&mut self, row: ERow, dir: i32) {
        let roles = [StickRole::Dpad, StickRole::Mouse, StickRole::Off];
        let k = &mut self.controls.keyboard;
        match row {
            ERow::LeftStick => self.controls.left_stick = step(&roles, self.controls.left_stick, dir),
            ERow::RightStick => self.controls.right_stick = step(&roles, self.controls.right_stick, dir),
            ERow::PointerSpeed => {
                self.controls.pointer_speed = (self.controls.pointer_speed + 100.0 * dir as f64).clamp(300.0, 4000.0);
            }
            ERow::Cursor => self.controls.cursor = step(&[CursorMode::Auto, CursorMode::On, CursorMode::Off], self.controls.cursor, dir),
            ERow::KbKind => self.cycle_preset(dir),
            ERow::KbSize => k.size = step(&KbSize::ALL, k.size, dir),
            ERow::KbPosition => k.position = step(&KbPos::ALL, k.position, dir),
            ERow::KbGame => k.game = step(&[KbGame::Under, KbGame::Beside], k.game, dir),
            ERow::KbOpacity => k.opacity = step(&KB_OPACITIES, k.opacity, dir),
            _ => return,
        }
        self.keyboard_changed();
    }

    /// The full-size preview has the pad: L1 / R1 move the keyboard, Left / Right size it, Up /
    /// Down set its background, Triangle puts the game under it or beside it; Circle or Square
    /// go back to the list.
    fn update_preview(&mut self, nav: u32) {
        let k = &mut self.controls.keyboard;
        let before = *k;
        if nav & BTN_L1 != 0 {
            k.position = k.position.next();
        }
        if nav & BTN_R1 != 0 {
            k.position = k.position.opposite();
        }
        if nav & BTN_LEFT != 0 {
            k.size = step(&KbSize::ALL, k.size, -1);
        }
        if nav & BTN_RIGHT != 0 {
            k.size = step(&KbSize::ALL, k.size, 1);
        }
        // the opacities run from solid down
        if nav & BTN_UP != 0 {
            k.opacity = step(&KB_OPACITIES, k.opacity, -1);
        }
        if nav & BTN_DOWN != 0 {
            k.opacity = step(&KB_OPACITIES, k.opacity, 1);
        }
        if nav & BTN_TRIANGLE != 0 {
            k.game = step(&[KbGame::Under, KbGame::Beside], k.game, 1);
        }
        if *k != before {
            self.keyboard_changed();
        }
        if nav & (BTN_CIRCLE | BTN_SQUARE) != 0 {
            self.preview = None;
        }
    }

    /// Act on the pad. `nav` are new presses with the d-pad's repeats. True when the editor is done
    /// (Circle with no picker open): save with `save`.
    pub fn update(&mut self, dt: f32, nav: u32) -> bool {
        self.time += dt;
        self.scroll.step(dt);
        self.hl.target = self.focus as f32;
        self.hl.step(dt);
        if let Some(p) = self.picker.as_mut() {
            p.scroll.step(dt);
        }
        if self.preview.is_some() {
            self.update_preview(nav);
            return false;
        }
        let row = self.rows[self.focus];
        if let Some(mut p) = self.picker.take() {
            let n = CHOICES.len();
            if nav & BTN_LEFT != 0 && p.index % PICK_COLS > 0 {
                p.index -= 1;
            }
            if nav & BTN_RIGHT != 0 && p.index % PICK_COLS + 1 < PICK_COLS && p.index + 1 < n {
                p.index += 1;
            }
            if nav & BTN_UP != 0 && p.index >= PICK_COLS {
                p.index -= PICK_COLS;
            }
            if nav & BTN_DOWN != 0 {
                p.index = (p.index + PICK_COLS).min(n - 1);
            }
            let mut keep = true;
            if let ERow::Button(b) = row {
                let name = CHOICES[p.index];
                if nav & BTN_CROSS != 0 {
                    let actions = if name == "none" { Vec::new() } else { parse_action(name).into_iter().collect() };
                    self.controls.bindings.insert(b, actions);
                    self.dirty = true;
                    keep = false;
                }
                if nav & BTN_SQUARE != 0 && name != "none" {
                    if let Some(a) = parse_action(name) {
                        let mut list = self.controls.actions(b);
                        if let Some(i) = list.iter().position(|x| *x == a) {
                            list.remove(i);
                        } else if list.len() < 4 {
                            list.push(a);
                        }
                        self.controls.bindings.insert(b, list);
                        self.dirty = true;
                    }
                }
            }
            if nav & BTN_CIRCLE != 0 {
                keep = false;
            }
            let prow = (p.index / PICK_COLS) as f32;
            let visible_rows = ((SIDE.h - 150.0) / (46.0 + 8.0)).floor();
            let top = p.scroll.target;
            if prow < top {
                p.scroll.target = prow;
            } else if prow > top + visible_rows - 1.0 {
                p.scroll.target = prow - visible_rows + 1.0;
            }
            if keep {
                self.picker = Some(p);
            }
            return false;
        }
        if nav & BTN_CIRCLE != 0 {
            if self.page == Page::Keyboard && !self.keyboard_only {
                self.show_controls_page();
                return false;
            }
            return true;
        }
        if nav & BTN_UP != 0 && self.focus > 0 {
            self.focus -= 1;
        }
        if nav & BTN_DOWN != 0 && self.focus + 1 < self.rows.len() {
            self.focus += 1;
        }
        let row = self.rows[self.focus];
        if nav & BTN_TRIANGLE != 0 && !matches!(row, ERow::ResetAll | ERow::KbReset) {
            self.reset_row(row);
        }
        if self.page == Page::Keyboard && nav & BTN_SQUARE != 0 {
            self.preview = Some(Osk::preview(self.controls.keyboard));
            return false;
        }
        match row {
            ERow::Button(b) => {
                if nav & BTN_CROSS != 0 {
                    let first = self.controls.actions(b).first().map(|a| a.name()).unwrap_or_else(|| "none".to_string());
                    let index = CHOICES.iter().position(|c| *c == first).unwrap_or(0);
                    let mut scroll = Smooth::default();
                    scroll.snap(((index / PICK_COLS) as f32 - 3.0).max(0.0));
                    self.picker = Some(Picker { index, scroll });
                }
            }
            ERow::Keyboard => {
                if nav & BTN_CROSS != 0 {
                    self.show_keyboard_page();
                    return false;
                }
            }
            ERow::ResetAll | ERow::KbReset => {
                if nav & BTN_CROSS != 0 {
                    self.reset_row(row);
                }
            }
            other => {
                if nav & BTN_LEFT != 0 {
                    self.cycle(other, -1);
                }
                if nav & (BTN_RIGHT | BTN_CROSS) != 0 {
                    self.cycle(other, 1);
                }
            }
        }
        // keep the focused row in view
        let visible = (LIST.h - 24.0) / ROW_H;
        let f = self.focus as f32;
        if f < self.scroll.target {
            self.scroll.target = f;
        } else if f > self.scroll.target + visible - 1.0 {
            self.scroll.target = f - visible + 1.0;
        }
        false
    }

    /// Write the mapping file (only when something changed). The message is for a toast.
    pub fn save(&self, log: fn(&str)) -> Option<String> {
        if !self.dirty {
            return None;
        }
        let heading = match &self.target {
            Target::AllGames => "Ruffle5 controls for every game (a game's own mapping is applied over these)".to_string(),
            Target::Game { stem, name } => format!("{name} ({stem}.swf) — Ruffle5 custom mapping"),
        };
        let text = self.controls.file_text(&self.base, &heading);
        let what = if self.keyboard_only { "Keyboard settings" } else { "Controls" };
        match crate::fsutil::write_durably(&self.file, text.as_bytes()) {
            Ok(()) => {
                log(&format!("controls: saved {} ({} line(s) differ)", self.file, self.controls.diff_lines(&self.base).len()));
                Some(format!("{what} saved"))
            }
            Err(e) => {
                log(&format!("controls: cannot write {}: {e}", self.file));
                Some(format!("{what} not saved (see ruffle.log)"))
            }
        }
    }

    /// The side card's words for a row.
    fn help(&self, row: ERow) -> (String, String) {
        let k = self.controls.keyboard;
        fn s(t: &str) -> String {
            t.to_string()
        }
        match row {
            ERow::Button(b) => (b.title().to_string(), s("Cross: choose what it does. In the list, Square adds a second key (both are pressed together). Triangle puts it back.")),
            ERow::LeftStick | ERow::RightStick => (
                s("Sticks"),
                s("D-pad: the stick presses the arrow buttons (what Up/Down/Left/Right do). Mouse pointer: it moves the pointer. Left/Right to change."),
            ),
            ERow::PointerSpeed => (s("Pointer speed"), s("How fast a stick moves the pointer at full tilt. Left/Right to change.")),
            ERow::Cursor => (s("Pointer"), s("When the arrow is drawn. Games that draw their own pointer usually look best with it hidden.")),
            ERow::Keyboard => (
                s("On-screen keyboard"),
                s("Cross: its size, where it opens, the game under it or beside it, and how solid it is, with ready-made settings for kinds of game. Triangle puts them back."),
            ),
            ERow::ResetAll => (s("Reset"), s("Cross: every row back to what the game would do without this mapping. Leave with Circle to save.")),
            ERow::KbKind => match keyboard::preset_of(k) {
                Some(i) => (s(PRESETS[i].0), s(PRESETS[i].2)),
                None => (
                    s("Your own"),
                    s("Not one of the ready-made settings. Left and Right go through them, one for each kind of game; your own come back after the last."),
                ),
            },
            ERow::KbSize => {
                let (w, h) = keyboard::panel_size(k.size);
                (
                    s("Size"),
                    format!(
                        "Large: big keys, easy to read from the sofa (as the keyboard always was). Medium and Small leave more of the game in view. Mini is a strip without the title and the button hints, for when you know them. {}: {w:.0} x {h:.0} of the 1920 x 1080 screen.",
                        k.size.label()
                    ),
                )
            }
            ERow::KbPosition => (
                s("Opens at"),
                s("A corner keeps the middle of the screen free; the top keeps a game's bottom bar in view; a side suits a game whose action is in the middle. On the open keyboard, L1 moves it on round the screen and R1 to the opposite side, for the rest of the game."),
            ),
            ERow::KbGame => (
                s("The game"),
                s("Under the keyboard: the game keeps the whole screen and the keyboard covers part of it. Beside it: while the keyboard is open, the game is shown smaller in the part of the screen the keyboard leaves free, none of it hidden. The game keeps running either way."),
            ),
            ERow::KbOpacity => (
                s("Background"),
                s("How solid the keyboard's background is. Below Solid, the game shows through it where the keyboard covers it (with the game under the keyboard)."),
            ),
            ERow::KbReset => (s("Reset"), s("Cross: size, place, the game and the background back to what they would be without this mapping.")),
        }
    }

    /// The keyboard page's map of the screen: the game and the keyboard where they will be.
    fn draw_map(&self, ui: &mut Ui, scene: &mut Scene, r: Rect) {
        let k = self.controls.keyboard;
        let (panel, keys) = Osk::preview(k).layout();
        let s = r.w / W;
        let map = |q: Rect| Rect::new(r.x + q.x * s, r.y + q.y * s, q.w * s, q.h * s);
        scene.rect(r, 10.0, BLACK);
        let game = match k.game {
            KbGame::Beside => keyboard::free_area(panel, self.aspect),
            KbGame::Under => fit_in(Rect::new(0.0, 0.0, W, H), self.aspect),
        };
        let g = map(game);
        scene.gradient_v(g, 4.0, BLUE_6, BLUE_9);
        // the word where the keyboard leaves the game visible
        let label_y = match k.game {
            KbGame::Beside => g.cy(),
            KbGame::Under if pr_centre_below(panel) => g.y + g.h * 0.2,
            KbGame::Under => g.bottom() - g.h * 0.2,
        };
        scene.text(ui, Face::SemiBold, 18.0, "Game", g.cx(), centred(label_y, 18.0), INK.with_alpha(0.75), Align::Center, 0.0);
        let solid = k.opacity as f32 / 100.0;
        let pr = map(panel);
        scene.rect(pr, 5.0, NIGHT.with_alpha(0.95 * solid));
        scene.stroke(pr, 5.0, 1.0, ORANGE.with_alpha(0.6));
        for key in keys {
            scene.rect(map(key), 1.5, WHITE.with_alpha(0.25));
        }
        scene.stroke(r, 10.0, 1.5, WHITE.with_alpha(0.3));
        let caption = match k.game {
            KbGame::Beside => format!("Keyboard {:.0} x {:.0}, the game {:.0} x {:.0}", panel.w, panel.h, game.w, game.h),
            KbGame::Under => format!("Keyboard {:.0} x {:.0}, over the game", panel.w, panel.h),
        };
        scene.text(ui, Face::Regular, 19.0, &caption, r.x, r.y - 14.0, INK.with_alpha(0.6), Align::Left, 0.0);
    }

    /// Square on the keyboard's page: the keyboard at full size, and its controls.
    fn draw_preview(&self, ui: &mut Ui, scene: &mut Scene, osk: &Osk) {
        let k = self.controls.keyboard;
        let (panel, _) = osk.layout();
        if !self.over_game {
            // in the menu there is no game behind: one is drawn where it would be
            scene.rect(Rect::new(0.0, 0.0, W, H), 0.0, NIGHT.with_alpha(0.8));
            let game = match k.game {
                KbGame::Beside => keyboard::free_area(panel, self.aspect),
                KbGame::Under => fit_in(Rect::new(0.0, 0.0, W, H), self.aspect),
            };
            scene.gradient_v(game, 12.0, BLUE_6, BLUE_9);
            scene.text(ui, Face::Display, 44.0, "Your game", game.cx(), centred(game.cy(), 44.0), INK.with_alpha(0.55), Align::Center, 0.0);
        }
        osk.draw(ui, scene, "Keyboard");
        let hints = [(Glyph::Chip("L1 R1"), "Move"), (Glyph::DPad(1), "Size"), (Glyph::DPad(0), "Background"), (Glyph::Triangle, "Under / beside"), (Glyph::Circle, "Back")];
        let w = hints_width(ui, &hints, 1.0) + 56.0;
        // at the top while the keyboard is in the lower half, else at the bottom
        let y = if panel.cy() > H * 0.5 { 32.0 } else { H - 32.0 - 60.0 };
        let pill = Rect::new((W - w) * 0.5, y, w, 60.0);
        scene.rect(pill, 30.0, NIGHT.with_alpha(0.92));
        scene.stroke(pill, 30.0, 1.5, ORANGE.with_alpha(0.4));
        draw_hints(ui, scene, &hints, pill.right() - 28.0, pill.cy());
        let label = format!("{}, {}, {}, {}", k.size.label(), k.position.label().to_lowercase(), k.game.label().to_lowercase(), opacity_label(k.opacity).to_lowercase());
        scene.text(ui, Face::SemiBold, 22.0, &label, W * 0.5, if y < H * 0.5 { pill.bottom() + 38.0 } else { pill.y - 20.0 }, ORANGE, Align::Center, 0.0);
    }

    pub fn draw(&mut self, ui: &mut Ui, scene: &mut Scene, alpha: f32, scrim: bool) {
        scene.push_opacity(alpha);
        if let Some(osk) = &self.preview {
            self.draw_preview(ui, scene, osk);
            scene.pop_opacity();
            return;
        }
        if scrim {
            scene.rect(Rect::new(0.0, 0.0, W, H), 0.0, NIGHT.with_alpha(0.78));
        } else {
            scene.gradient_h(Rect::new(0.0, 0.0, W, H), 0.0, NIGHT.with_alpha(0.86), NIGHT.with_alpha(0.55));
        }
        // the heading
        crate::widgets::ribbon_fold(scene, Rect::new(98.0, 142.0, 28.0, 6.0), ORANGE, 1.0);
        let kicker = if self.page == Page::Keyboard { "ON-SCREEN KEYBOARD" } else { "CONTROLS" };
        scene.text(ui, Face::SemiBold, 18.0, kicker, 136.0, 154.0, ORANGE, Align::Left, 4.0);
        let title = ui.fit(Face::Display, 56.0, &self.title(), 1500.0);
        scene.text(ui, Face::Display, 56.0, &title, 92.0, 222.0, INK, Align::Left, 0.0);
        let file = self.file.rsplit('/').take(2).collect::<Vec<_>>().into_iter().rev().collect::<Vec<_>>().join("/");
        let sub = ui.fit(Face::Regular, 22.0, &format!("Saved to {file} when you leave"), 900.0);
        scene.text(ui, Face::Regular, 22.0, &sub, W - 96.0, 222.0, BLUE_2, Align::Right, 0.0);

        // the list
        card(scene, LIST, 22.0, 1.0);
        let inner = LIST.inset(12.0);
        scene.push_clip(inner);
        let top = inner.y - self.scroll.value * ROW_H;
        // the highlight glides between rows
        let h = Rect::new(inner.x, top + self.hl.value * ROW_H, inner.w, ROW_H - 4.0);
        scene.rect(h, 14.0, WHITE.with_alpha(if self.picker.is_some() { 0.06 } else { 0.1 }));
        scene.rect(Rect::new(h.x + 4.0, h.y + 12.0, 5.0, h.h - 24.0), 2.5, ORANGE);
        for (i, row) in self.rows.clone().into_iter().enumerate() {
            let y = top + i as f32 * ROW_H;
            if y + ROW_H < inner.y || y > inner.bottom() {
                continue;
            }
            let r = Rect::new(inner.x, y, inner.w, ROW_H - 4.0);
            let cy = r.cy();
            match row {
                ERow::Button(b) => {
                    draw_glyph(ui, scene, button_glyph(b), r.x + 24.0, cy, 34.0);
                    scene.text(ui, Face::SemiBold, 24.0, b.title(), r.x + 24.0 + 130.0, centred(cy, 24.0), INK, Align::Left, 0.0);
                }
                ERow::ResetAll => {
                    scene.text(ui, Face::SemiBold, 24.0, "Reset everything to what it would be without this mapping", r.x + 24.0, centred(cy, 24.0), ORANGE_3, Align::Left, 0.0);
                }
                ERow::KbReset => {
                    scene.text(ui, Face::SemiBold, 24.0, "Reset the keyboard to what it would be without this mapping", r.x + 24.0, centred(cy, 24.0), ORANGE_3, Align::Left, 0.0);
                }
                other => {
                    let label = match other {
                        ERow::LeftStick => "Left stick",
                        ERow::RightStick => "Right stick",
                        ERow::PointerSpeed => "Pointer speed (sticks)",
                        ERow::Keyboard => "On-screen keyboard  \u{203a}",
                        ERow::KbKind => "Made for",
                        ERow::KbSize => "Size",
                        ERow::KbPosition => "Opens at",
                        ERow::KbGame => "The game",
                        ERow::KbOpacity => "Background",
                        _ => "Pointer",
                    };
                    scene.text(ui, Face::SemiBold, 24.0, label, r.x + 24.0, centred(cy, 24.0), INK, Align::Left, 0.0);
                }
            }
            if !matches!(row, ERow::ResetAll | ERow::KbReset) {
                let value = self.value(row);
                let value = ui.fit(Face::Regular, 24.0, &value, 420.0);
                let changed = self.changed(row);
                let vx = r.right() - 24.0;
                scene.text(ui, Face::Regular, 24.0, &value, vx, centred(cy, 24.0), if changed { ORANGE } else { INK.with_alpha(0.78) }, Align::Right, 0.0);
                if changed {
                    let w = ui.measure(Face::Regular, 24.0, &value, 0.0);
                    scene.circle(vx - w - 16.0, cy, 4.0, ORANGE);
                }
            }
        }
        scene.pop_clip();

        // the side: help, or the picker
        card(scene, SIDE, 22.0, 1.0);
        let row = self.rows[self.focus];
        match (&self.picker, row) {
            (Some(p), ERow::Button(b)) => {
                let current = self.controls.actions(b);
                scene.text(ui, Face::SemiBold, 26.0, &format!("{} does", b.title()), SIDE.x + 32.0, SIDE.y + 52.0, INK, Align::Left, 0.0);
                let now = ui.fit(Face::Regular, 22.0, &describe_short(&current), SIDE.w - 64.0);
                scene.text(ui, Face::Regular, 22.0, &now, SIDE.x + 32.0, SIDE.y + 88.0, ORANGE, Align::Left, 0.0);
                let grid = Rect::new(SIDE.x + 6.0, SIDE.y + 112.0, SIDE.w - 12.0, SIDE.h - 124.0);
                scene.push_clip(grid);
                let tw = (SIDE.w - 64.0 - (PICK_COLS as f32 - 1.0) * 8.0) / PICK_COLS as f32;
                let th = 46.0;
                let gy = grid.y + 8.0 - p.scroll.value * (th + 8.0);
                for (k, name) in CHOICES.iter().enumerate() {
                    let (col, line) = ((k % PICK_COLS) as f32, (k / PICK_COLS) as f32);
                    let t = Rect::new(SIDE.x + 32.0 + col * (tw + 8.0), gy + line * (th + 8.0), tw, th);
                    if t.bottom() < grid.y || t.y > grid.bottom() {
                        continue;
                    }
                    let selected = k == p.index;
                    let on = parse_action(name).map(|a| current.contains(&a)).unwrap_or(current.is_empty());
                    if selected {
                        scene.shadow(t, 12.0, 18.0, ORANGE.with_alpha(0.4));
                        scene.rect(t, 12.0, ORANGE);
                    } else {
                        scene.rect(t, 12.0, if on { BLUE_4.with_alpha(0.55) } else { WHITE.with_alpha(0.07) });
                    }
                    let label = choice_label(name);
                    let size = if label.chars().count() > 9 { 15.0 } else if label.chars().count() > 3 { 18.0 } else { 22.0 };
                    let label = ui.fit(Face::SemiBold, size, &label, tw - 10.0);
                    scene.text(ui, Face::SemiBold, size, &label, t.cx(), centred(t.cy(), size), if selected { ON_ORANGE } else { INK }, Align::Center, 0.0);
                }
                scene.pop_clip();
            }
            _ => {
                let (head, body) = self.help(row);
                scene.text(ui, Face::SemiBold, 28.0, &head, SIDE.x + 32.0, SIDE.y + 58.0, INK, Align::Left, 0.0);
                for (i, line) in ui.wrap(Face::Regular, 23.0, &body, SIDE.w - 64.0).iter().enumerate() {
                    scene.text(ui, Face::Regular, 23.0, line, SIDE.x + 32.0, SIDE.y + 104.0 + 34.0 * i as f32, BLUE_2, Align::Left, 0.0);
                }
                if self.page == Page::Keyboard {
                    self.draw_map(ui, scene, MAP);
                } else {
                    let base_name = match self.target {
                        Target::AllGames => "the built-in defaults",
                        Target::Game { .. } => "the controls for all games",
                    };
                    let note = format!("Orange values differ from {base_name}.");
                    scene.text(ui, Face::Regular, 21.0, &note, SIDE.x + 32.0, SIDE.bottom() - 36.0, INK.with_alpha(0.6), Align::Left, 0.0);
                }
            }
        }

        // hints
        let mut hints: Vec<(Glyph, &str)> = Vec::new();
        if self.picker.is_some() {
            hints.push((Glyph::Cross, "Set"));
            hints.push((Glyph::Square, "Add / remove"));
            hints.push((Glyph::Circle, "Back"));
        } else {
            match row {
                ERow::Button(_) => hints.push((Glyph::Cross, "Change")),
                ERow::Keyboard => hints.push((Glyph::Cross, "Open")),
                ERow::ResetAll => hints.push((Glyph::Cross, "Reset all")),
                ERow::KbReset => hints.push((Glyph::Cross, "Reset")),
                _ => hints.push((Glyph::DPad(4), "Change")),
            }
            if !matches!(row, ERow::ResetAll | ERow::KbReset) {
                hints.push((Glyph::Triangle, "Reset"));
            }
            if self.page == Page::Keyboard {
                hints.push((Glyph::Square, "Full-size preview"));
            }
            let leave = if self.page == Page::Keyboard && !self.keyboard_only {
                "Back"
            } else if self.dirty {
                "Save and leave"
            } else {
                "Leave"
            };
            hints.push((Glyph::Circle, leave));
        }
        draw_hints(ui, scene, &hints, W - 96.0, 1012.0);
        scene.pop_opacity();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn paths(tag: &str) -> Paths {
        let root = std::env::temp_dir().join(format!("ruffle5-editor-{tag}-{}", std::process::id()));
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

    fn noop(_: &str) {}

    #[test]
    fn picking_a_key_writes_a_preset_style_file() {
        let paths = paths("pick");
        let target = Target::Game { stem: "dadnme".into(), name: "Dad N Me".into() };
        let mut e = Editor::open(&paths, target, None);
        // Cross on Cross opens the picker at its current key; move to "a" and set it.
        assert!(!e.update(0.016, BTN_CROSS));
        let a = CHOICES.iter().position(|c| *c == "a").unwrap();
        e.picker.as_mut().unwrap().index = a;
        e.update(0.016, BTN_CROSS);
        assert!(e.picker.is_none());
        // Square on Square: add "s" to Square's own z
        e.focus = 2;
        e.update(0.016, BTN_CROSS);
        let s = CHOICES.iter().position(|c| *c == "s").unwrap();
        e.picker.as_mut().unwrap().index = s;
        e.update(0.016, BTN_SQUARE);
        e.update(0.016, BTN_CIRCLE);
        assert!(e.update(0.016, BTN_CIRCLE), "Circle leaves");
        assert_eq!(e.save(noop).as_deref(), Some("Controls saved"));
        let text = std::fs::read_to_string(controls::mapping_file(&paths, Some("dadnme"))).unwrap();
        assert!(text.contains("\ncross = a\n"), "{text}");
        assert!(text.contains("\nsquare = z, s\n"), "{text}");
        assert!(!text.contains("circle ="), "{text}");
        // and the game now gets it
        let c = Controls::load(&paths, &format!("{}/dadnme.swf", paths.games));
        assert_eq!(describe_short(&c.actions(Button::Cross)), "A");
    }

    #[test]
    fn reset_all_and_nothing_to_save() {
        let paths = paths("reset");
        let mut e = Editor::open(&paths, Target::AllGames, None);
        assert_eq!(e.save(noop), None, "unchanged: nothing written");
        e.focus = e.rows.iter().position(|r| *r == ERow::LeftStick).unwrap();
        e.update(0.016, BTN_RIGHT);
        assert_eq!(e.controls.left_stick, StickRole::Mouse);
        e.focus = e.rows.len() - 1;
        e.update(0.016, BTN_CROSS);
        assert_eq!(e.controls.left_stick, StickRole::Dpad);
    }

    #[test]
    fn the_keyboard_page_from_the_controls_list() {
        let paths = paths("kbpage");
        let target = Target::Game { stem: "rpg".into(), name: "An RPG".into() };
        let mut e = Editor::open(&paths, target, None);
        let kb_row = e.rows.iter().position(|r| *r == ERow::Keyboard).unwrap();
        e.focus = kb_row;
        e.update(0.016, BTN_CROSS);
        assert_eq!(e.page, Page::Keyboard);
        assert_eq!(e.rows, KB_ROWS.to_vec());
        // Size: Large -> Medium
        e.update(0.016, BTN_DOWN);
        e.update(0.016, BTN_RIGHT);
        assert_eq!(e.controls.keyboard.size, KbSize::Medium);
        // Circle goes back to the list, on the keyboard's row; Circle again leaves
        assert!(!e.update(0.016, BTN_CIRCLE));
        assert_eq!(e.page, Page::Controls);
        assert_eq!(e.focus, kb_row);
        assert!(e.update(0.016, BTN_CIRCLE));
        assert_eq!(e.save(noop).as_deref(), Some("Controls saved"));
        let text = std::fs::read_to_string(controls::mapping_file(&paths, Some("rpg"))).unwrap();
        assert!(text.contains("\nkeyboard_size = medium\n"), "{text}");
        assert!(!text.contains("keyboard_position"), "only what differs: {text}");
        let c = Controls::load(&paths, &format!("{}/rpg.swf", paths.games));
        assert_eq!(c.keyboard.size, KbSize::Medium);
    }

    #[test]
    fn ready_made_settings_and_your_own() {
        let paths = paths("kbpreset");
        let mut e = Editor::open_keyboard(&paths, Target::AllGames, None);
        assert_eq!(e.page, Page::Keyboard);
        assert_eq!(e.value(ERow::KbKind), PRESETS[0].0, "the defaults are the first ready-made settings");
        // make them the player's own: Background 80%
        e.focus = KB_ROWS.iter().position(|r| *r == ERow::KbOpacity).unwrap();
        e.update(0.016, BTN_RIGHT);
        let own = e.controls.keyboard;
        assert_eq!(own.opacity, 80);
        assert_eq!(e.value(ERow::KbKind), "Your own");
        // "Made for": Right goes to the first ready-made settings, round them, and back to the own
        e.focus = 0;
        e.update(0.016, BTN_RIGHT);
        assert_eq!(e.controls.keyboard, PRESETS[0].1);
        e.update(0.016, BTN_RIGHT);
        assert_eq!(e.controls.keyboard, PRESETS[1].1, "RPG and turn-based");
        assert_eq!(e.controls.keyboard.game, KbGame::Beside);
        for _ in 2..PRESETS.len() {
            e.update(0.016, BTN_RIGHT);
        }
        e.update(0.016, BTN_RIGHT);
        assert_eq!(e.controls.keyboard, own, "your own come back after the last");
        e.update(0.016, BTN_LEFT);
        assert_eq!(e.controls.keyboard, PRESETS[PRESETS.len() - 1].1);
        // Circle leaves straight away when opened on this page
        assert!(e.update(0.016, BTN_CIRCLE));
        assert_eq!(e.save(noop).as_deref(), Some("Keyboard settings saved"));
        let c = Controls::base_game(&paths);
        assert_eq!(c.keyboard, PRESETS[PRESETS.len() - 1].1, "every game gets them");
    }

    #[test]
    fn the_full_size_preview() {
        let paths = paths("kbpreview");
        let mut e = Editor::open_keyboard(&paths, Target::AllGames, None).over_game(4.0 / 3.0);
        assert!(e.keyboard_preview().is_none());
        e.update(0.016, BTN_SQUARE);
        let p = e.keyboard_preview().expect("Square shows it");
        assert_eq!(p.panel(), Osk::preview(KeyboardPrefs::default()).panel(), "where it opens, at full size");
        // L1 moves it round, R1 across, Left / Right size it, Triangle puts the game beside it
        e.update(0.016, BTN_L1);
        assert_eq!(e.controls.keyboard.position, KbPos::BottomLeft);
        e.update(0.016, BTN_R1);
        assert_eq!(e.controls.keyboard.position, KbPos::TopRight);
        e.update(0.016, BTN_RIGHT);
        assert_eq!(e.controls.keyboard.size, KbSize::Medium);
        e.update(0.016, BTN_TRIANGLE);
        assert_eq!(e.controls.keyboard.game, KbGame::Beside);
        e.update(0.016, BTN_DOWN);
        assert_eq!(e.controls.keyboard.opacity, 80);
        let p = e.keyboard_preview().unwrap();
        assert_eq!(p.prefs(), e.controls.keyboard, "the preview follows");
        let area = p.game_area(4.0 / 3.0).expect("the game moves out of its way");
        assert!(!area.intersects(&p.panel()));
        // Circle closes the preview, not the editor
        assert!(!e.update(0.016, BTN_CIRCLE));
        assert!(e.keyboard_preview().is_none());
        assert!(e.dirty);
        assert!(e.update(0.016, BTN_CIRCLE));
    }
}
