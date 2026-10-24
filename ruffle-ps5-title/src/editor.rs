//! PS5_RUFFLE_EDITOR_V1 — the controls editor: what each button does, for one game or for all.
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
// SPDX-License-Identifier: GPL-3.0-or-later

use crate::controls::{self, choice_label, describe_short, parse_action, Button, Controls, CursorMode, StickRole, CHOICES};
use crate::input::{BTN_CIRCLE, BTN_CROSS, BTN_DOWN, BTN_LEFT, BTN_RIGHT, BTN_SQUARE, BTN_TRIANGLE, BTN_UP};
use crate::paths::Paths;
use crate::ui::{Align, Face, Rect, Scene, Ui, H, W};
use crate::widgets::{card, centred, draw_glyph, draw_hints, Glyph, Smooth, BLUE_2, BLUE_4, INK, NIGHT, ON_ORANGE, ORANGE, ORANGE_3, WHITE};

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
    ResetAll,
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

const PICK_COLS: usize = 6;
const ROW_H: f32 = 56.0;
const LIST: Rect = Rect::new(96.0, 268.0, 980.0, 700.0);
const SIDE: Rect = Rect::new(1124.0, 268.0, 700.0, 700.0);

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
        let mut rows: Vec<ERow> = ORDER.iter().map(|b| ERow::Button(*b)).collect();
        rows.extend([ERow::LeftStick, ERow::RightStick, ERow::PointerSpeed, ERow::Cursor, ERow::ResetAll]);
        Editor {
            target,
            controls,
            base,
            rows,
            focus: 0,
            scroll: Smooth::default(),
            picker: None,
            dirty: false,
            time: 0.0,
            file,
            hl: Smooth::default(),
        }
    }

    pub fn title(&self) -> String {
        match &self.target {
            Target::AllGames => "All games".to_string(),
            Target::Game { name, .. } => name.clone(),
        }
    }

    fn changed(&self, row: ERow) -> bool {
        match row {
            ERow::Button(b) => self.controls.actions(b) != self.base.actions(b),
            ERow::LeftStick => self.controls.left_stick != self.base.left_stick,
            ERow::RightStick => self.controls.right_stick != self.base.right_stick,
            ERow::PointerSpeed => self.controls.pointer_speed != self.base.pointer_speed,
            ERow::Cursor => self.controls.cursor != self.base.cursor,
            ERow::ResetAll => false,
        }
    }

    fn value(&self, row: ERow) -> String {
        match row {
            ERow::Button(b) => describe_short(&self.controls.actions(b)),
            ERow::LeftStick => stick_label(self.controls.left_stick).to_string(),
            ERow::RightStick => stick_label(self.controls.right_stick).to_string(),
            ERow::PointerSpeed => format!("{:.0} px/s", self.controls.pointer_speed),
            ERow::Cursor => cursor_label(self.controls.cursor).to_string(),
            ERow::ResetAll => String::new(),
        }
    }

    fn reset_row(&mut self, row: ERow) {
        match row {
            ERow::Button(b) => {
                self.controls.bindings.insert(b, self.base.actions(b));
            }
            ERow::LeftStick => self.controls.left_stick = self.base.left_stick,
            ERow::RightStick => self.controls.right_stick = self.base.right_stick,
            ERow::PointerSpeed => self.controls.pointer_speed = self.base.pointer_speed,
            ERow::Cursor => self.controls.cursor = self.base.cursor,
            ERow::ResetAll => {
                let report = std::mem::take(&mut self.controls.report);
                self.controls = self.base.clone();
                self.controls.report = report;
            }
        }
        self.dirty = true;
    }

    fn cycle(&mut self, row: ERow, dir: i32) {
        let roles = [StickRole::Dpad, StickRole::Mouse, StickRole::Off];
        let step_role = |r: StickRole| {
            let i = roles.iter().position(|x| *x == r).unwrap_or(0) as i32;
            roles[(i + dir).rem_euclid(roles.len() as i32) as usize]
        };
        match row {
            ERow::LeftStick => self.controls.left_stick = step_role(self.controls.left_stick),
            ERow::RightStick => self.controls.right_stick = step_role(self.controls.right_stick),
            ERow::PointerSpeed => {
                self.controls.pointer_speed = (self.controls.pointer_speed + 100.0 * dir as f64).clamp(300.0, 4000.0);
            }
            ERow::Cursor => {
                let modes = [CursorMode::Auto, CursorMode::On, CursorMode::Off];
                let i = modes.iter().position(|x| *x == self.controls.cursor).unwrap_or(0) as i32;
                self.controls.cursor = modes[(i + dir).rem_euclid(3) as usize];
            }
            _ => return,
        }
        self.dirty = true;
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
            return true;
        }
        if nav & BTN_UP != 0 && self.focus > 0 {
            self.focus -= 1;
        }
        if nav & BTN_DOWN != 0 && self.focus + 1 < self.rows.len() {
            self.focus += 1;
        }
        let row = self.rows[self.focus];
        if nav & BTN_TRIANGLE != 0 && row != ERow::ResetAll {
            self.reset_row(row);
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
            ERow::ResetAll => {
                if nav & BTN_CROSS != 0 {
                    self.reset_row(ERow::ResetAll);
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
        match crate::fsutil::write_durably(&self.file, text.as_bytes()) {
            Ok(()) => {
                log(&format!("controls: saved {} ({} line(s) differ)", self.file, self.controls.diff_lines(&self.base).len()));
                Some("Controls saved".to_string())
            }
            Err(e) => {
                log(&format!("controls: cannot write {}: {e}", self.file));
                Some("Controls not saved (see ruffle.log)".to_string())
            }
        }
    }

    pub fn draw(&mut self, ui: &mut Ui, scene: &mut Scene, alpha: f32, scrim: bool) {
        scene.push_opacity(alpha);
        if scrim {
            scene.rect(Rect::new(0.0, 0.0, W, H), 0.0, NIGHT.with_alpha(0.78));
        } else {
            scene.gradient_h(Rect::new(0.0, 0.0, W, H), 0.0, NIGHT.with_alpha(0.86), NIGHT.with_alpha(0.55));
        }
        // the heading
        crate::widgets::ribbon_fold(scene, Rect::new(98.0, 142.0, 28.0, 6.0), ORANGE, 1.0);
        scene.text(ui, Face::SemiBold, 18.0, "CONTROLS", 136.0, 154.0, ORANGE, Align::Left, 4.0);
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
                other => {
                    let label = match other {
                        ERow::LeftStick => "Left stick",
                        ERow::RightStick => "Right stick",
                        ERow::PointerSpeed => "Pointer speed (sticks)",
                        _ => "Pointer",
                    };
                    scene.text(ui, Face::SemiBold, 24.0, label, r.x + 24.0, centred(cy, 24.0), INK, Align::Left, 0.0);
                }
            }
            if row != ERow::ResetAll {
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
                let (head, body) = match row {
                    ERow::Button(b) => (
                        b.title().to_string(),
                        "Cross: choose what it does. In the list, Square adds a second key (both are pressed together). Triangle puts it back.".to_string(),
                    ),
                    ERow::LeftStick | ERow::RightStick => (
                        "Sticks".to_string(),
                        "D-pad: the stick presses the arrow buttons (what Up/Down/Left/Right do). Mouse pointer: it moves the pointer. Left/Right to change.".to_string(),
                    ),
                    ERow::PointerSpeed => ("Pointer speed".to_string(), "How fast a stick moves the pointer at full tilt. Left/Right to change.".to_string()),
                    ERow::Cursor => ("Pointer".to_string(), "When the arrow is drawn. Games that draw their own pointer usually look best with it hidden.".to_string()),
                    ERow::ResetAll => ("Reset".to_string(), "Cross: every row back to what the game would do without this mapping. Leave with Circle to save.".to_string()),
                };
                scene.text(ui, Face::SemiBold, 28.0, &head, SIDE.x + 32.0, SIDE.y + 58.0, INK, Align::Left, 0.0);
                for (i, line) in ui.wrap(Face::Regular, 23.0, &body, SIDE.w - 64.0).iter().enumerate() {
                    scene.text(ui, Face::Regular, 23.0, line, SIDE.x + 32.0, SIDE.y + 104.0 + 34.0 * i as f32, BLUE_2, Align::Left, 0.0);
                }
                let base_name = match self.target {
                    Target::AllGames => "the built-in defaults",
                    Target::Game { .. } => "the controls for all games",
                };
                let note = format!("Orange values differ from {base_name}.");
                scene.text(ui, Face::Regular, 21.0, &note, SIDE.x + 32.0, SIDE.bottom() - 36.0, INK.with_alpha(0.6), Align::Left, 0.0);
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
                ERow::ResetAll => hints.push((Glyph::Cross, "Reset all")),
                _ => hints.push((Glyph::DPad(4), "Change")),
            }
            if row != ERow::ResetAll {
                hints.push((Glyph::Triangle, "Reset"));
            }
            hints.push((Glyph::Circle, if self.dirty { "Save and leave" } else { "Leave" }));
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
}
