//! PS5_RUFFLE_QUICKMENU_V1 — the in-game quick menu: holding OPTIONS (or Esc on a USB keyboard)
//! pauses the game behind a card with Resume, Restart, Volume, Controls, Keyboard (1.000.031: the
//! on-screen keyboard's settings for this game), Take cover picture, FPS counter and Back to games.
//!
//! After Ruffle-Flash-PS5's quick menu (src/quickmenu.rs, GPL-3.0, by ZiZc3), drawn on the GPU over
//! the paused game's last frame (Ruffle keeps drawing it), not on a CPU copy of it.
// SPDX-License-Identifier: GPL-3.0-or-later

use crate::input::{BTN_CIRCLE, BTN_CROSS, BTN_DOWN, BTN_LEFT, BTN_OPTIONS, BTN_RIGHT, BTN_UP};
use crate::sounds::{self, Sfx};
use crate::ui::{Align, Face, Rect, Scene, Ui, H, W};
use crate::widgets::{centred, draw_hints, ease, Glyph, Smooth, BLACK, BLUE_2, INK, NIGHT, ORANGE, WHITE};

const CARD_W: f32 = 760.0;
const ROW_H: f32 = 70.0;
const OPEN_TIME: f32 = 0.22;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Item {
    Resume,
    Restart,
    Volume,
    Controls,
    Keyboard,
    Cover,
    Fps,
    Library,
}

const ITEMS: [(Item, &str); 8] = [
    (Item::Resume, "Resume"),
    (Item::Restart, "Restart game"),
    (Item::Volume, "Volume"),
    (Item::Controls, "Controls for this game"),
    (Item::Keyboard, "Keyboard for this game"),
    (Item::Cover, "Take cover picture"),
    (Item::Fps, "FPS counter"),
    (Item::Library, "Back to games"),
];

/// What the player chose.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Action {
    None,
    Resume,
    Restart,
    /// The new game volume, 0..=10.
    Volume(u8),
    Controls,
    /// The on-screen keyboard's settings for this game.
    Keyboard,
    Cover,
    Fps(bool),
    Library,
}

pub struct QuickMenu {
    time: f32,
    sel: usize,
    volume: u8,
    fps: bool,
    name: String,
    /// Closing: the card leaves before the game resumes.
    closing: Option<(Action, f32)>,
    /// The highlight, gliding between rows.
    hl: Smooth,
}

impl QuickMenu {
    pub fn new(name: &str, volume: u8, fps: bool) -> QuickMenu {
        QuickMenu {
            time: 0.0,
            sel: 0,
            volume: volume.min(10),
            fps,
            name: name.to_string(),
            closing: None,
            hl: Smooth::default(),
        }
    }

    /// `nav`: new presses with the d-pad's repeats.
    pub fn update(&mut self, dt: f32, nav: u32) -> Action {
        self.time += dt;
        self.hl.target = self.sel as f32;
        self.hl.step(dt);
        if let Some((action, t)) = self.closing.as_mut() {
            *t += dt;
            if *t >= 0.12 {
                return *action;
            }
            return Action::None;
        }
        // Ignore the press that opened it.
        if self.time < 0.15 {
            return Action::None;
        }
        if nav & (BTN_CIRCLE | BTN_OPTIONS) != 0 {
            sounds::play(Sfx::Back);
            self.closing = Some((Action::Resume, 0.0));
            return Action::None;
        }
        let before = self.sel;
        if nav & BTN_DOWN != 0 {
            self.sel = (self.sel + 1) % ITEMS.len();
        }
        if nav & BTN_UP != 0 {
            self.sel = (self.sel + ITEMS.len() - 1) % ITEMS.len();
        }
        if self.sel != before {
            sounds::play(Sfx::Move);
        }
        let item = ITEMS[self.sel].0;
        match item {
            Item::Volume => {
                let step = if nav & (BTN_RIGHT | BTN_CROSS) != 0 {
                    1
                } else if nav & BTN_LEFT != 0 {
                    -1
                } else {
                    0
                };
                if step != 0 {
                    let v = (self.volume as i32 + step).clamp(0, 10) as u8;
                    if v != self.volume {
                        self.volume = v;
                        sounds::play(Sfx::Toggle);
                        return Action::Volume(v);
                    }
                }
                Action::None
            }
            Item::Fps => {
                if nav & (BTN_LEFT | BTN_RIGHT | BTN_CROSS) != 0 {
                    self.fps = !self.fps;
                    sounds::play(Sfx::Toggle);
                    return Action::Fps(self.fps);
                }
                Action::None
            }
            _ => {
                if nav & BTN_CROSS != 0 {
                    sounds::play(Sfx::Select);
                    let action = match item {
                        Item::Resume => Action::Resume,
                        Item::Restart => Action::Restart,
                        Item::Controls => Action::Controls,
                        Item::Keyboard => Action::Keyboard,
                        Item::Cover => Action::Cover,
                        _ => Action::Library,
                    };
                    // Resume and the cover leave the card first; the rest act at once.
                    if matches!(action, Action::Resume | Action::Cover) {
                        self.closing = Some((action, 0.0));
                        return Action::None;
                    }
                    return action;
                }
                Action::None
            }
        }
    }

    pub fn draw(&self, ui: &mut Ui, scene: &mut Scene) {
        let open = ease(self.time / OPEN_TIME);
        let t = match self.closing {
            Some((_, c)) => open * (1.0 - ease(c / 0.12)),
            None => open,
        };
        scene.rect(Rect::new(0.0, 0.0, W, H), 0.0, NIGHT.with_alpha(0.62 * t));
        let card_h = 200.0 + ITEMS.len() as f32 * ROW_H + 70.0;
        let x = (W - CARD_W) * 0.5;
        let y = (H - card_h) * 0.5 + (1.0 - t) * 30.0;
        let r = Rect::new(x, y, CARD_W, card_h);
        scene.push_opacity(t);
        scene.shadow(Rect::new(r.x, r.y + 14.0, r.w, r.h), 28.0, 44.0, BLACK.with_alpha(0.55));
        scene.rect(r, 28.0, NIGHT.with_alpha(0.94));
        scene.stroke(r, 28.0, 1.5, ORANGE.with_alpha(0.3));
        let ix = x + 44.0;
        let iw = CARD_W - 88.0;
        scene.text(ui, Face::SemiBold, 18.0, "PAUSED", ix, y + 56.0, ORANGE, Align::Left, 4.0);
        let title = ui.fit(Face::Display, 44.0, &self.name, iw);
        scene.text(ui, Face::Display, 44.0, &title, ix, y + 112.0, INK, Align::Left, 0.0);
        crate::widgets::ribbon_fold(scene, Rect::new(ix + 2.0, y + 138.0, 64.0, 6.0), ORANGE, 1.0);
        let mut ry = y + 168.0;
        let hy = ry + ROW_H * self.hl.value;
        scene.rect(Rect::new(x + 18.0, hy + 4.0, CARD_W - 36.0, ROW_H - 8.0), 16.0, WHITE.with_alpha(0.1));
        scene.rect(Rect::new(x + 18.0, hy + 20.0, 5.0, ROW_H - 40.0), 2.5, ORANGE);
        for (i, (item, label)) in ITEMS.iter().enumerate() {
            let selected = i == self.sel;
            if !selected && i > 0 && i != self.sel + 1 {
                scene.rect(Rect::new(ix, ry, iw, 1.0), 0.0, WHITE.with_alpha(0.06));
            }
            let a = if selected { 1.0 } else { 0.78 };
            let cy = ry + ROW_H * 0.5;
            scene.text(ui, Face::SemiBold, 28.0, label, ix, centred(cy, 28.0), INK.with_alpha(a), Align::Left, 0.0);
            let right = x + CARD_W - 44.0;
            match item {
                Item::Volume => {
                    let bar_w = 200.0;
                    let value = format!("{}%", self.volume as u32 * 10);
                    scene.text(ui, Face::SemiBold, 24.0, &value, right, centred(cy, 24.0), INK.with_alpha(a), Align::Right, 0.0);
                    let vw = ui.measure(Face::SemiBold, 24.0, "100%", 0.0);
                    let bx = right - vw - 24.0 - bar_w;
                    scene.rect(Rect::new(bx, cy - 4.0, bar_w, 8.0), 4.0, WHITE.with_alpha(0.18));
                    let fill = bar_w * self.volume as f32 / 10.0;
                    if fill > 0.0 {
                        scene.rect(Rect::new(bx, cy - 4.0, fill, 8.0), 4.0, ORANGE);
                    }
                    if selected {
                        scene.text(ui, Face::SemiBold, 22.0, "\u{25c0}", bx - 30.0, centred(cy, 22.0), ORANGE, Align::Left, 0.0);
                    }
                }
                Item::Fps => {
                    let on = if self.fps { "On" } else { "Off" };
                    scene.text(ui, Face::SemiBold, 24.0, on, right, centred(cy, 24.0), if self.fps { ORANGE } else { BLUE_2 }, Align::Right, 0.0);
                }
                _ if selected => {
                    scene.text(ui, Face::SemiBold, 24.0, "\u{203a}", right, centred(cy, 24.0), INK, Align::Right, 0.0);
                }
                _ => {}
            }
            ry += ROW_H;
        }
        let hints = [(Glyph::Cross, "Select"), (Glyph::DPad(4), "Volume"), (Glyph::Circle, "Resume")];
        draw_hints(ui, scene, &hints, x + CARD_W - 44.0, ry + 36.0);
        scene.pop_opacity();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn volume_and_resume() {
        let mut q = QuickMenu::new("Game", 5, false);
        assert_eq!(q.update(0.05, BTN_CROSS), Action::None, "the opening press is ignored");
        q.update(0.2, 0);
        q.update(0.016, BTN_DOWN);
        q.update(0.016, BTN_DOWN);
        assert_eq!(q.update(0.016, BTN_RIGHT), Action::Volume(6));
        assert_eq!(q.update(0.016, BTN_CIRCLE), Action::None);
        assert_eq!(q.update(0.2, 0), Action::Resume);
    }
}
