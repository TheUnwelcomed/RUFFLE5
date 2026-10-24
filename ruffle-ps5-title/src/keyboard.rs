//! PS5_RUFFLE_KEYBOARD_V1 — the on-screen keyboard: for games that ask for keys the controller
//! does not have ("press K", "type your name") and for searching the menu.
//!
//! After Ruffle-Flash-PS5's keyboard (src/keys.rs, GPL-3.0, by ZiZc3), drawn with Ruffle5's own
//! renderer: a button bound to `keyboard` (L2 by default) opens it over the game, and a game's text
//! box opens it by itself (Settings > Keyboard for text boxes). The d-pad picks a key, Cross sends it
//! (a key press and, for characters, the typed character), Square deletes, Triangle types a space,
//! R1 moves the keyboard to the other end of the screen, Circle closes it. Shift is a caps toggle.
// SPDX-License-Identifier: GPL-3.0-or-later

use crate::controls::{key_by_name, Key};
use crate::input::{BTN_CIRCLE, BTN_CROSS, BTN_DOWN, BTN_LEFT, BTN_R1, BTN_RIGHT, BTN_SQUARE, BTN_TRIANGLE, BTN_UP};
use crate::ui::{Align, Face, Rect, Scene, Ui, H, W};
use crate::widgets::{centred, draw_hints, ease, Glyph, BLACK, BLUE_4, INK, NIGHT, ON_ORANGE, ORANGE, WHITE};
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

const KEY: f32 = 66.0;
const GAP: f32 = 8.0;
const BOARD_W: f32 = 10.0 * KEY + 9.0 * GAP;
const PANEL_W: f32 = BOARD_W + 72.0;
const PANEL_H: f32 = 6.0 * (KEY + GAP) + 92.0 + 58.0;
/// How long a tapped key stays down.
const TAP: f32 = 0.09;

pub struct Osk {
    open: bool,
    row: usize,
    col: usize,
    /// A key tapped, released when its time runs out.
    pending_up: Option<(Key, f32)>,
    /// The player closed it (text boxes then stop opening it for the rest of the game).
    pub user_closed: bool,
    caps: bool,
    at_top: bool,
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
            at_top: false,
            shown: 0.0,
        }
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
            self.at_top = !self.at_top;
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

    fn key_x(row: usize, col: usize) -> (f32, f32) {
        let mut x = 0.0;
        for (i, def) in ROWS[row].iter().enumerate() {
            let w = def.2 as f32 * KEY + (def.2 as f32 - 1.0) * GAP;
            if i == col {
                return (x, w);
            }
            x += w + GAP;
        }
        (x, KEY)
    }

    fn key_centre(&self, row: usize, col: usize) -> f32 {
        let (x, w) = Self::key_x(row, col);
        x + w * 0.5
    }

    fn nearest(&self, row: usize, x: f32) -> usize {
        (0..ROWS[row].len())
            .min_by(|&a, &b| (self.key_centre(row, a) - x).abs().total_cmp(&(self.key_centre(row, b) - x).abs()))
            .unwrap_or(0)
    }

    /// Where the panel is (for the menu's search box, which sits above it).
    pub fn panel(&self) -> Rect {
        let y = if self.at_top { 40.0 } else { H - 28.0 - PANEL_H };
        let slide = (1.0 - ease(self.shown)) * 60.0 * if self.at_top { -1.0 } else { 1.0 };
        Rect::new((W - PANEL_W) * 0.5, y + slide, PANEL_W, PANEL_H)
    }

    pub fn draw(&self, ui: &mut Ui, scene: &mut Scene, title: &str) {
        if self.shown <= 0.0 {
            return;
        }
        let p = self.panel();
        scene.push_opacity(ease(self.shown));
        scene.shadow(Rect::new(p.x, p.y + 14.0, p.w, p.h), 28.0, 44.0, BLACK.with_alpha(0.5));
        scene.rect(p, 28.0, NIGHT.with_alpha(0.95));
        scene.stroke(p, 28.0, 1.5, ORANGE.with_alpha(0.3));
        let x0 = p.x + 36.0;
        let y0 = p.y + 92.0;
        scene.text(ui, Face::SemiBold, 26.0, title, x0, p.y + 54.0, INK, Align::Left, 0.0);
        if self.caps {
            scene.text(ui, Face::SemiBold, 18.0, "CAPS", x0 + BOARD_W, p.y + 52.0, ORANGE, Align::Right, 2.0);
        }
        for (r, row) in ROWS.iter().enumerate() {
            let y = y0 + r as f32 * (KEY + GAP);
            for (c, def) in row.iter().enumerate() {
                let (kx, w) = Self::key_x(r, c);
                let k = Rect::new(x0 + kx, y, w, KEY);
                let selected = r == self.row && c == self.col;
                let lit = def.1 == "shift" && self.caps;
                if selected {
                    scene.shadow(k, 14.0, 20.0, ORANGE.with_alpha(0.45));
                    scene.rect(k, 14.0, ORANGE);
                } else {
                    scene.rect(k, 14.0, if lit { BLUE_4.with_alpha(0.6) } else { WHITE.with_alpha(0.09) });
                }
                let label = if def.2 == 1 && def.0.len() == 1 && !self.caps && def.0.chars().all(|c| c.is_ascii_alphabetic()) { def.0.to_ascii_lowercase() } else { def.0.to_string() };
                let size = if label.chars().count() > 1 { 20.0 } else { 28.0 };
                let ink = if selected { ON_ORANGE } else { INK };
                scene.text(ui, Face::SemiBold, size, &label, k.cx(), centred(k.cy(), size), ink, Align::Center, 0.0);
            }
        }
        let hints = [(Glyph::Cross, "Type"), (Glyph::Square, "Delete"), (Glyph::Triangle, "Space"), (Glyph::Chip("R1"), "Move"), (Glyph::Circle, "Close")];
        draw_hints(ui, scene, &hints, x0 + BOARD_W, y0 + 6.0 * (KEY + GAP) + 24.0);
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
}
