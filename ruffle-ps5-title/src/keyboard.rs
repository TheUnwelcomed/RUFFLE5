//! PS5_RUFFLE_KEYBOARD_V2 — the on-screen keyboard: for games that ask for keys the controller
//! does not have ("press K", "type your name") and for searching the menu.
//!
//! After Ruffle-Flash-PS5's keyboard (src/keys.rs, GPL-3.0, by ZiZc3), drawn with Ruffle5's own
//! renderer: a button bound to `keyboard` (L2 by default) opens it over the game, and a game's text
//! box opens it by itself (Settings > Keyboard for text boxes). The d-pad picks a key, Cross sends it
//! (a key press and, for characters, the typed character), Square deletes, Triangle types a space,
//! Circle closes it. Shift is a caps toggle.
//!
//! V2 (1.000.031): its settings (`KeyboardPrefs`, per game or for all games in the mapping files,
//! src/controls.rs): four sizes (Large as before; Medium, Small; Mini, a strip without the title or
//! the button hints), eight places (the bottom as before, the top, either side, the corners), the
//! game drawn under it or shown smaller in the part of the screen it leaves free (`game_area`), and
//! how solid its background is. R1 sends it across the screen (bottom and top swap, as before), L1
//! moves it on to the next place clockwise (bottom, bottom left, left, ...). `PRESETS` are ready-made
//! settings for kinds of game, offered by the editor (src/editor.rs).
// SPDX-License-Identifier: GPL-3.0-or-later

use crate::controls::{key_by_name, KbGame, KbPos, KbSize, Key, KeyboardPrefs};
use crate::input::{BTN_CIRCLE, BTN_CROSS, BTN_DOWN, BTN_L1, BTN_LEFT, BTN_R1, BTN_RIGHT, BTN_SQUARE, BTN_TRIANGLE, BTN_UP};
use crate::ui::{Align, Face, Rect, Scene, Ui, H, W};
use crate::widgets::{centred, draw_hints_scaled, ease, Glyph, BLACK, BLUE_4, INK, NIGHT, ON_ORANGE, ORANGE, WHITE};
use ruffle_core::events::PlayerEvent;

/// (label, key name for src/controls.rs, width in key units).
type KeyDef = (&'static str, &'static str, u32);

const ROWS: [&[KeyDef]; 6] = [
    &[("1", "1", 1), ("2", "2", 1), ("3", "3", 1), ("4", "4", 1), ("5", "5", 1), ("6", "6", 1), ("7", "7", 1), ("8", "8", 1), ("9", "9", 1), ("0", "0", 1)],
    &[("Q", "q", 1), ("W", "w", 1), ("E", "e", 1), ("R", "r", 1), ("T", "t", 1), ("Y", "y", 1), ("U", "u", 1), ("I", "i", 1), ("O", "o", 1), ("P", "p", 1)],
    &[("A", "a", 1), ("S", "s", 1), ("D", "d", 1), ("F", "f", 1), ("G", "g", 1), ("H", "h", 1), ("J", "j", 1), ("K", "k", 1), ("L", "l", 1), ("Enter", "enter", 1)],
    &[("Shift", "shift", 1), ("Z", "z", 1), ("X", "x", 1), ("C", "c", 1), ("V", "v", 1), ("B", "b", 1), ("N", "n", 1), ("M", "m", 1), ("Delete", "backspace", 2)],
    &[(".", ".", 1), (",", ",", 1), ("-", "-", 1), ("'", "'", 1), ("/", "/", 1), ("\u{2190}", "left", 1), ("\u{2191}", "up", 1), ("\u{2193}", "down", 1), ("\u{2192}", "right", 1), ("Tab", "tab", 1)],
    &[("Esc", "escape", 2), ("Ctrl", "control", 2), ("Space", "space", 4), ("Alt", "alt", 2)],
];

/// How long a tapped key stays down.
const TAP: f32 = 0.09;
/// Margins from the screen's edges (the top one as before, clear of the TV's overscan).
const EDGE_SIDE: f32 = 28.0;
const EDGE_TOP: f32 = 40.0;
const EDGE_BOTTOM: f32 = 28.0;
/// Between the keyboard and a game shown beside it.
const GAME_GAP: f32 = 16.0;
/// How far the panel slides in from.
const SLIDE: f32 = 60.0;

/// Ready-made settings for kinds of game: (name, settings, what they are for). The first is the
/// keyboard as it was before it had settings.
pub const PRESETS: [(&str, KeyboardPrefs, &str); 4] = [
    (
        "Typing a name or a code",
        KeyboardPrefs { size: KbSize::Large, position: KbPos::Bottom, game: KbGame::Under, opacity: 100 },
        "Large keys at the bottom, over the game: the keyboard as it always was. For a name on a high-score table, a password, a level code.",
    ),
    (
        "RPG and turn-based",
        KeyboardPrefs { size: KbSize::Small, position: KbPos::BottomRight, game: KbGame::Beside, opacity: 100 },
        "Small keys in the bottom-right corner and the game shown smaller beside them, so the map, the menus and the question the game asks stay in view while you pick a move or type a name.",
    ),
    (
        "Typing games",
        KeyboardPrefs { size: KbSize::Medium, position: KbPos::Bottom, game: KbGame::Beside, opacity: 100 },
        "Medium keys at the bottom and the game shrunk into the space above them: the words to type stay on screen, none of them under the keys.",
    ),
    (
        "A key now and then",
        KeyboardPrefs { size: KbSize::Mini, position: KbPos::TopRight, game: KbGame::Under, opacity: 60 },
        "A see-through Mini strip in the top-right corner, over the game, for games that want one key from time to time (M for the map, P for pause, a number for an item): the game keeps the whole screen.",
    ),
];

/// Which preset `prefs` are, if any.
pub fn preset_of(prefs: KeyboardPrefs) -> Option<usize> {
    PRESETS.iter().position(|p| p.1 == prefs)
}

/// A size's measurements, in UI units (1920x1080).
#[derive(Clone, Copy, Debug)]
struct Metrics {
    key: f32,
    gap: f32,
    /// Left and right of the keys (and below them on Mini, which has no hint row).
    pad: f32,
    /// Above the keys: the title's band (Mini: just a margin).
    head: f32,
    /// Below the keys: the hints' band.
    foot: f32,
    key_radius: f32,
    panel_radius: f32,
    label: f32,
    label_long: f32,
    /// 0: no title.
    title: f32,
    /// 0: no hints.
    hints: f32,
}

fn metrics(size: KbSize) -> Metrics {
    match size {
        // as the keyboard was before it had sizes
        KbSize::Large => Metrics { key: 66.0, gap: 8.0, pad: 36.0, head: 92.0, foot: 58.0, key_radius: 14.0, panel_radius: 28.0, label: 28.0, label_long: 20.0, title: 26.0, hints: 1.0 },
        KbSize::Medium => Metrics { key: 54.0, gap: 7.0, pad: 28.0, head: 78.0, foot: 52.0, key_radius: 12.0, panel_radius: 24.0, label: 24.0, label_long: 17.0, title: 22.0, hints: 0.85 },
        KbSize::Small => Metrics { key: 42.0, gap: 6.0, pad: 22.0, head: 62.0, foot: 46.0, key_radius: 10.0, panel_radius: 20.0, label: 19.0, label_long: 14.0, title: 18.0, hints: 0.7 },
        KbSize::Mini => Metrics { key: 32.0, gap: 4.0, pad: 14.0, head: 14.0, foot: 10.0, key_radius: 8.0, panel_radius: 16.0, label: 15.0, label_long: 11.0, title: 0.0, hints: 0.0 },
    }
}

/// The keyboard's size at `size`, in UI units (1920x1080).
pub fn panel_size(size: KbSize) -> (f32, f32) {
    let m = metrics(size);
    (m.panel_w(), m.panel_h())
}

impl Metrics {
    fn board_w(&self) -> f32 {
        10.0 * self.key + 9.0 * self.gap
    }
    fn panel_w(&self) -> f32 {
        self.board_w() + 2.0 * self.pad
    }
    fn panel_h(&self) -> f32 {
        self.head + 6.0 * (self.key + self.gap) + self.foot
    }
}

/// Where a `w` x `h` panel sits at `pos`, shown (UI units).
fn place(pos: KbPos, w: f32, h: f32) -> Rect {
    let (left, right, cx) = (EDGE_SIDE, W - EDGE_SIDE - w, (W - w) * 0.5);
    let (top, bottom, cy) = (EDGE_TOP, H - EDGE_BOTTOM - h, (H - h) * 0.5);
    let (x, y) = match pos {
        KbPos::Bottom => (cx, bottom),
        KbPos::Top => (cx, top),
        KbPos::Left => (left, cy),
        KbPos::Right => (right, cy),
        KbPos::TopLeft => (left, top),
        KbPos::TopRight => (right, top),
        KbPos::BottomLeft => (left, bottom),
        KbPos::BottomRight => (right, bottom),
    };
    Rect::new(x, y, w, h)
}

/// Which way the panel slides in from (its offset when hidden).
fn slide_dir(pos: KbPos) -> (f32, f32) {
    match pos {
        KbPos::Bottom | KbPos::BottomLeft | KbPos::BottomRight => (0.0, 1.0),
        KbPos::Top | KbPos::TopLeft | KbPos::TopRight => (0.0, -1.0),
        KbPos::Left => (-1.0, 0.0),
        KbPos::Right => (1.0, 0.0),
    }
}

/// The largest picture of shape `aspect` (width / height) the screen holds beside `panel`: above,
/// below, left or right of it, whichever lets it be biggest; centred there. UI units.
pub fn free_area(panel: Rect, aspect: f32) -> Rect {
    let aspect = if aspect.is_finite() && aspect > 0.0 { aspect } else { W / H };
    let candidates = [
        Rect::new(0.0, 0.0, W, panel.y - GAME_GAP),
        Rect::new(0.0, panel.bottom() + GAME_GAP, W, H - panel.bottom() - GAME_GAP),
        Rect::new(0.0, 0.0, panel.x - GAME_GAP, H),
        Rect::new(panel.right() + GAME_GAP, 0.0, W - panel.right() - GAME_GAP, H),
    ];
    let fitted = |r: &Rect| -> Rect {
        if r.w < 1.0 || r.h < 1.0 {
            return Rect::new(r.x, r.y, 0.0, 0.0);
        }
        let (w, h) = if r.w / r.h > aspect { (r.h * aspect, r.h) } else { (r.w, r.w / aspect) };
        Rect::new(r.x + (r.w - w) * 0.5, r.y + (r.h - h) * 0.5, w, h)
    };
    candidates
        .iter()
        .map(fitted)
        .max_by(|a, b| (a.w * a.h).total_cmp(&(b.w * b.h)))
        .filter(|r| r.w >= 1.0)
        .unwrap_or(Rect::new(0.0, 0.0, W, H))
}

fn lerp_rect(a: Rect, b: Rect, t: f32) -> Rect {
    Rect::new(a.x + (b.x - a.x) * t, a.y + (b.y - a.y) * t, a.w + (b.w - a.w) * t, a.h + (b.h - a.h) * t)
}

pub struct Osk {
    open: bool,
    row: usize,
    col: usize,
    /// A key tapped, released when its time runs out.
    pending_up: Option<(Key, f32)>,
    /// The player closed it (text boxes then stop opening it for the rest of the game).
    pub user_closed: bool,
    caps: bool,
    prefs: KeyboardPrefs,
    /// Where it is now (it starts at the setting's place; R1 / L1 move it for the session).
    pos: KbPos,
    shown: f32,
}

impl Default for Osk {
    fn default() -> Self {
        Osk::new()
    }
}

impl Osk {
    pub fn new() -> Osk {
        Osk {
            open: false,
            row: 2,
            col: 4,
            pending_up: None,
            user_closed: false,
            caps: false,
            prefs: KeyboardPrefs::default(),
            pos: KbPos::Bottom,
            shown: 0.0,
        }
    }

    /// A keyboard as `prefs` draw it, fully shown: the settings' preview.
    pub fn preview(prefs: KeyboardPrefs) -> Osk {
        let mut k = Osk::new();
        k.set_prefs(prefs);
        k.open = true;
        k.shown = 1.0;
        k
    }

    /// The game's settings for it (its size and place start over from them).
    pub fn set_prefs(&mut self, prefs: KeyboardPrefs) {
        self.prefs = prefs;
        self.pos = prefs.position;
    }

    pub fn prefs(&self) -> KeyboardPrefs {
        self.prefs
    }

    pub fn is_open(&self) -> bool {
        self.open
    }

    /// Still on screen (opening or closing).
    pub fn visible(&self) -> bool {
        self.open || self.shown > 0.0
    }

    pub fn set_open(&mut self, open: bool) {
        self.open = open;
    }

    /// The player toggled it (a button): closing this way counts as "the player closed it".
    pub fn toggle(&mut self) {
        self.open = !self.open;
        if !self.open {
            self.user_closed = true;
        }
    }

    fn row_len(&self) -> usize {
        ROWS[self.row].len()
    }

    fn tap(&mut self, def: KeyDef, events: &mut Vec<PlayerEvent>) {
        if def.1 == "shift" {
            self.caps = !self.caps;
            return;
        }
        let Some(key) = key_by_name(def.1) else {
            return;
        };
        if let Some((prev, _)) = self.pending_up.take() {
            events.push(PlayerEvent::KeyUp { key: prev.descriptor() });
        }
        events.push(PlayerEvent::KeyDown { key: key.descriptor() });
        let typed = match def.1 {
            "space" => Some(' '),
            n if n.chars().count() == 1 => n.chars().next().map(|c| if self.caps { c.to_ascii_uppercase() } else { c }),
            _ => None,
        };
        if let Some(c) = typed {
            events.push(PlayerEvent::TextInput { codepoint: c });
        }
        self.pending_up = Some((key, TAP));
    }

    /// Takes the pad while open: the key events for the game (or the search), and whether the
    /// keyboard used the pad this frame (so nothing else should). `pressed` are new button bits
    /// with repeats (input's `nav`).
    pub fn update(&mut self, dt: f32, pressed: u32, events: &mut Vec<PlayerEvent>) -> bool {
        let target = if self.open { 1.0 } else { 0.0 };
        self.shown += (target - self.shown) * (1.0 - (-18.0 * dt).exp());
        if (target - self.shown).abs() < 0.01 {
            self.shown = target;
        }
        if let Some((key, left)) = self.pending_up.as_mut() {
            *left -= dt;
            if *left <= 0.0 {
                events.push(PlayerEvent::KeyUp { key: key.descriptor() });
                self.pending_up = None;
            }
        }
        if !self.open {
            return false;
        }
        if pressed & BTN_CIRCLE != 0 {
            self.open = false;
            self.user_closed = true;
            return true;
        }
        if pressed & BTN_UP != 0 {
            let x = self.key_centre(self.row, self.col);
            self.row = (self.row + ROWS.len() - 1) % ROWS.len();
            self.col = self.nearest(self.row, x);
        }
        if pressed & BTN_DOWN != 0 {
            let x = self.key_centre(self.row, self.col);
            self.row = (self.row + 1) % ROWS.len();
            self.col = self.nearest(self.row, x);
        }
        if pressed & BTN_LEFT != 0 {
            self.col = (self.col + self.row_len() - 1) % self.row_len();
        }
        if pressed & BTN_RIGHT != 0 {
            self.col = (self.col + 1) % self.row_len();
        }
        if pressed & BTN_R1 != 0 {
            self.pos = self.pos.opposite();
        }
        if pressed & BTN_L1 != 0 {
            self.pos = self.pos.next();
        }
        if pressed & BTN_CROSS != 0 {
            let def = ROWS[self.row][self.col];
            self.tap(def, events);
        }
        if pressed & BTN_SQUARE != 0 {
            self.tap(("Delete", "backspace", 2), events);
        }
        if pressed & BTN_TRIANGLE != 0 {
            self.tap(("Space", "space", 4), events);
        }
        true
    }

    /// Lets go of a key still down (leaving a game).
    pub fn release_all(&mut self) -> Vec<PlayerEvent> {
        let mut events = Vec::new();
        if let Some((key, _)) = self.pending_up.take() {
            events.push(PlayerEvent::KeyUp { key: key.descriptor() });
        }
        self.open = false;
        events
    }

    fn key_x(m: &Metrics, row: usize, col: usize) -> (f32, f32) {
        let mut x = 0.0;
        for (i, def) in ROWS[row].iter().enumerate() {
            let w = def.2 as f32 * m.key + (def.2 as f32 - 1.0) * m.gap;
            if i == col {
                return (x, w);
            }
            x += w + m.gap;
        }
        (x, m.key)
    }

    fn key_centre(&self, row: usize, col: usize) -> f32 {
        // in key units, so moving between rows picks the same key at any size
        let m = metrics(KbSize::Large);
        let (x, w) = Self::key_x(&m, row, col);
        x + w * 0.5
    }

    fn nearest(&self, row: usize, x: f32) -> usize {
        (0..ROWS[row].len())
            .min_by(|&a, &b| (self.key_centre(row, a) - x).abs().total_cmp(&(self.key_centre(row, b) - x).abs()))
            .unwrap_or(0)
    }

    /// Where the panel is when fully shown.
    fn rest_panel(&self) -> Rect {
        let m = metrics(self.prefs.size);
        place(self.pos, m.panel_w(), m.panel_h())
    }

    /// Where the panel is now (for the menu's search box, which sits above it).
    pub fn panel(&self) -> Rect {
        let r = self.rest_panel();
        let (dx, dy) = slide_dir(self.pos);
        let off = (1.0 - ease(self.shown)) * SLIDE;
        Rect::new(r.x + dx * off, r.y + dy * off, r.w, r.h)
    }

    /// On screen with the game beside it (the game's loop then works out where the game goes).
    pub fn beside(&self) -> bool {
        self.prefs.game == KbGame::Beside && self.shown > 0.0
    }

    /// Where the game goes while the keyboard is open beside it (its picture `aspect` wide for
    /// each unit of height), UI units, eased with the keyboard's slide; None when the keyboard is
    /// drawn over the game, or is not on screen.
    pub fn game_area(&self, aspect: f32) -> Option<Rect> {
        if self.prefs.game != KbGame::Beside || self.shown <= 0.0 {
            return None;
        }
        let free = free_area(self.rest_panel(), aspect);
        Some(lerp_rect(Rect::new(0.0, 0.0, W, H), free, ease(self.shown)))
    }

    /// The panel and each key's rectangle, fully shown at its place (UI units; keys row by row as
    /// in `ROWS`): the settings' map of the screen draws the keyboard from these.
    pub fn layout(&self) -> (Rect, Vec<Rect>) {
        let m = metrics(self.prefs.size);
        let p = self.rest_panel();
        let mut keys = Vec::new();
        for (r, row) in ROWS.iter().enumerate() {
            for c in 0..row.len() {
                let (kx, w) = Self::key_x(&m, r, c);
                keys.push(Rect::new(p.x + m.pad + kx, p.y + m.head + r as f32 * (m.key + m.gap), w, m.key));
            }
        }
        (p, keys)
    }

    pub fn draw(&self, ui: &mut Ui, scene: &mut Scene, title: &str) {
        self.draw_with(ui, scene, title, None);
    }

    /// `draw` with other button hints than the keyboard's own (the settings' full-size preview).
    pub fn draw_with(&self, ui: &mut Ui, scene: &mut Scene, title: &str, hints: Option<&[(Glyph, &str)]>) {
        if self.shown <= 0.0 {
            return;
        }
        let m = metrics(self.prefs.size);
        let solid = (self.prefs.opacity.clamp(10, 100) as f32) / 100.0;
        let p = self.panel();
        scene.push_opacity(ease(self.shown));
        scene.shadow(Rect::new(p.x, p.y + 14.0 * m.key / 66.0, p.w, p.h), m.panel_radius, 44.0 * m.key / 66.0, BLACK.with_alpha(0.5 * solid));
        scene.rect(p, m.panel_radius, NIGHT.with_alpha(0.95 * solid));
        scene.stroke(p, m.panel_radius, 1.5, ORANGE.with_alpha(0.3));
        let x0 = p.x + m.pad;
        let y0 = p.y + m.head;
        if m.title > 0.0 {
            let base = p.y + m.head - 38.0 * m.title / 26.0;
            scene.text(ui, Face::SemiBold, m.title, title, x0, base, INK, Align::Left, 0.0);
            if self.caps {
                let caps = m.title * 18.0 / 26.0;
                scene.text(ui, Face::SemiBold, caps, "CAPS", x0 + m.board_w(), base - 2.0, ORANGE, Align::Right, 2.0);
            }
        }
        for (r, row) in ROWS.iter().enumerate() {
            let y = y0 + r as f32 * (m.key + m.gap);
            for (c, def) in row.iter().enumerate() {
                let (kx, w) = Self::key_x(&m, r, c);
                let k = Rect::new(x0 + kx, y, w, m.key);
                let selected = r == self.row && c == self.col;
                let lit = def.1 == "shift" && self.caps;
                if selected {
                    scene.shadow(k, m.key_radius, 20.0 * m.key / 66.0, ORANGE.with_alpha(0.45));
                    scene.rect(k, m.key_radius, ORANGE);
                } else if lit {
                    scene.rect(k, m.key_radius, BLUE_4.with_alpha(0.6));
                } else {
                    if solid < 1.0 {
                        // the keys keep some background of their own however see-through the panel is
                        scene.rect(k, m.key_radius, NIGHT.with_alpha(0.25 * (1.0 - solid) + 0.09));
                    }
                    scene.rect(k, m.key_radius, WHITE.with_alpha(0.09));
                }
                let label = if def.2 == 1 && def.0.len() == 1 && !self.caps && def.0.chars().all(|c| c.is_ascii_alphabetic()) { def.0.to_ascii_lowercase() } else { def.0.to_string() };
                let mut size = if label.chars().count() > 1 { m.label_long } else { m.label };
                // a word too wide for its key at the small sizes is set smaller, not cut
                let lw = ui.measure(Face::SemiBold, size, &label, 0.0);
                if lw > w - 6.0 && lw > 0.0 {
                    size *= (w - 6.0) / lw;
                }
                let ink = if selected { ON_ORANGE } else { INK };
                scene.text(ui, Face::SemiBold, size, &label, k.cx(), centred(k.cy(), size), ink, Align::Center, 0.0);
            }
        }
        if m.hints > 0.0 {
            let full = [(Glyph::Cross, "Type"), (Glyph::Square, "Delete"), (Glyph::Triangle, "Space"), (Glyph::Chip("L1 R1"), "Move"), (Glyph::Circle, "Close")];
            let short = [(Glyph::Cross, "Type"), (Glyph::Chip("L1 R1"), "Move"), (Glyph::Circle, "Close")];
            let hints: &[(Glyph, &str)] = match hints {
                Some(h) => h,
                None if self.prefs.size == KbSize::Small => &short,
                None => &full,
            };
            let cy = y0 + 6.0 * (m.key + m.gap) + 24.0 * m.hints;
            draw_hints_scaled(ui, scene, hints, x0 + m.board_w(), cy, m.hints);
        }
        scene.pop_opacity();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_key_is_a_known_key() {
        for row in ROWS {
            let units: u32 = row.iter().map(|d| d.2).sum();
            assert_eq!(units, 10, "a row is ten keys wide");
            for d in row {
                assert!(d.1 == "shift" || key_by_name(d.1).is_some(), "{}", d.1);
            }
        }
    }

    #[test]
    fn typing_sends_a_press_a_character_and_a_release() {
        let mut k = Osk::new();
        k.set_open(true);
        let mut ev = Vec::new();
        k.row = 1;
        k.col = 0; // Q
        assert!(k.update(0.016, BTN_CROSS, &mut ev));
        assert!(matches!(ev[0], PlayerEvent::KeyDown { .. }));
        assert!(matches!(ev[1], PlayerEvent::TextInput { codepoint: 'q' }));
        ev.clear();
        k.update(0.2, 0, &mut ev);
        assert!(matches!(ev[0], PlayerEvent::KeyUp { .. }));
        ev.clear();
        k.update(0.016, BTN_CIRCLE, &mut ev);
        assert!(!k.is_open() && k.user_closed);
    }

    #[test]
    fn the_large_keyboard_at_the_bottom_is_where_it_always_was() {
        // 1.000.030's constants: KEY 66, GAP 8, panel 804 x 594, centred, 28 above the bottom
        let k = Osk::preview(KeyboardPrefs::default());
        let p = k.panel();
        assert_eq!((p.w, p.h), (804.0, 594.0));
        assert_eq!((p.x, p.y), ((W - 804.0) * 0.5, H - 28.0 - 594.0));
        let mut top = KeyboardPrefs::default();
        top.position = KbPos::Top;
        assert_eq!(Osk::preview(top).panel().y, 40.0);
    }

    #[test]
    fn every_size_and_place_stays_on_screen() {
        for size in KbSize::ALL {
            for pos in KbPos::ALL {
                let k = Osk::preview(KeyboardPrefs { size, position: pos, ..KeyboardPrefs::default() });
                let p = k.panel();
                assert!(p.x >= 0.0 && p.y >= 0.0 && p.right() <= W && p.bottom() <= H, "{size:?} {pos:?}: {p:?}");
            }
        }
        // each size is smaller than the one before
        let areas: Vec<f32> = KbSize::ALL.iter().map(|s| {
            let m = metrics(*s);
            m.panel_w() * m.panel_h()
        }).collect();
        assert!(areas.windows(2).all(|w| w[1] < w[0]), "{areas:?}");
    }

    #[test]
    fn presets_and_layout() {
        assert_eq!(PRESETS[0].1, KeyboardPrefs::default(), "the first preset is the keyboard as it was");
        for (i, p) in PRESETS.iter().enumerate() {
            assert_eq!(preset_of(p.1), Some(i), "{} is told apart from the others", p.0);
            assert!(!p.2.is_empty());
        }
        assert_eq!(preset_of(KeyboardPrefs { opacity: 80, ..KeyboardPrefs::default() }), None);
        assert_eq!(panel_size(KbSize::Large), (804.0, 594.0));
        for size in KbSize::ALL {
            for pos in KbPos::ALL {
                let k = Osk::preview(KeyboardPrefs { size, position: pos, ..KeyboardPrefs::default() });
                let (p, keys) = k.layout();
                assert_eq!(p, k.panel());
                assert_eq!(keys.len(), ROWS.iter().map(|r| r.len()).sum::<usize>());
                for key in keys {
                    assert!(key.x >= p.x && key.y >= p.y && key.right() <= p.right() + 0.01 && key.bottom() <= p.bottom() + 0.01, "{size:?} {pos:?}");
                }
            }
        }
    }

    #[test]
    fn r1_swaps_sides_and_l1_goes_round() {
        let mut k = Osk::new();
        k.set_open(true);
        let mut ev = Vec::new();
        k.update(0.016, BTN_R1, &mut ev);
        assert_eq!(k.pos, KbPos::Top);
        k.update(0.016, BTN_R1, &mut ev);
        assert_eq!(k.pos, KbPos::Bottom);
        k.update(0.016, BTN_L1, &mut ev);
        assert_eq!(k.pos, KbPos::BottomLeft, "clockwise: from the bottom towards the left");
        // a game's settings put it back where they say
        k.set_prefs(KeyboardPrefs { position: KbPos::Left, ..KeyboardPrefs::default() });
        assert_eq!(k.pos, KbPos::Left);
    }

    #[test]
    fn a_game_beside_the_keyboard_is_never_under_it() {
        for size in KbSize::ALL {
            for pos in KbPos::ALL {
                let prefs = KeyboardPrefs { size, position: pos, game: KbGame::Beside, opacity: 100 };
                let k = Osk::preview(prefs);
                for aspect in [16.0 / 9.0, 4.0 / 3.0, 550.0 / 400.0] {
                    let a = k.game_area(aspect).expect("an area when beside");
                    assert!(!a.intersects(&k.panel()), "{size:?} {pos:?} {aspect}: {a:?} under {:?}", k.panel());
                    assert!(a.w > 100.0 && a.h > 100.0, "{size:?} {pos:?}: {a:?}");
                    assert!((a.w / a.h - aspect).abs() < 0.01, "the picture keeps its shape");
                }
            }
        }
        // under the keyboard: no area, the game keeps the whole screen
        assert!(Osk::preview(KeyboardPrefs::default()).game_area(16.0 / 9.0).is_none());
        // a Small keyboard in a corner leaves a 16:9 game most of the screen
        let k = Osk::preview(KeyboardPrefs { size: KbSize::Small, position: KbPos::BottomRight, game: KbGame::Beside, opacity: 100 });
        let a = k.game_area(16.0 / 9.0).unwrap();
        assert!(a.w > 1300.0, "{a:?}");
    }
}
