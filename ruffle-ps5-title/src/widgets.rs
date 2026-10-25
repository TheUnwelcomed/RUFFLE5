//! PS5_RUFFLE_WIDGETS_V1 — Ruffle5's look, shared by every screen: the palette, the controller
//! glyphs, hint rows, hold bars, cards, buttons, toasts, the spinner and the badge medals.
//!
//! The palette is Ruffle's own, from ruffle.rs (src/theme.ts: "this is ruffle orange" #FFAD33,
//! "this is ruffle blue" #37528C, and their Mantine shades): the page is Ruffle blue, headings and
//! calls to action are Ruffle orange with the site's dark maroon (#3C1518) on orange buttons,
//! secondary surfaces the deeper blues. Text is the palest blue so long lines stay easy to read.
// SPDX-License-Identifier: GPL-3.0-or-later

use crate::ui::{Align, Color, Face, Rect, Scene, Ui, H, W};

// ---- Ruffle's palette ----
pub const ORANGE: Color = Color::rgb(0xffad33); // ruffle orange
pub const ORANGE_3: Color = Color::rgb(0xffc163);
pub const ORANGE_0: Color = Color::rgb(0xfff6e0);
pub const ORANGE_7: Color = Color::rgb(0xe48800);
pub const FOLD: Color = Color::rgb(0x966214); // the mark's fold
pub const ON_ORANGE: Color = Color::rgb(0x3c1518); // text on an orange button (ruffle.rs)
pub const BLUE: Color = Color::rgb(0x37528c); // ruffle blue: the page
pub const BLUE_9: Color = Color::rgb(0x2c4980);
pub const BLUE_7: Color = Color::rgb(0x425fa1);
pub const BLUE_6: Color = Color::rgb(0x5271b6);
pub const BLUE_4: Color = Color::rgb(0x728ac1);
pub const BLUE_2: Color = Color::rgb(0xb9c5e0);
pub const BLUE_0: Color = Color::rgb(0xedf3ff);
/// The deepest blue: scrims behind text over the moving background, cards.
pub const NIGHT: Color = Color::rgb(0x1a2b52);
pub const INK: Color = BLUE_0;
pub const WHITE: Color = Color::rgb(0xffffff);
pub const BLACK: Color = Color::rgb(0x000000);
// The PlayStation symbols' own colours (on a dark disc they read on any background).
const CROSS_BLUE: Color = Color::rgb(0x7fa8ff);
const CIRCLE_RED: Color = Color::rgb(0xff6f8a);
const SQUARE_PINK: Color = Color::rgb(0xf49ad1);
const TRIANGLE_GREEN: Color = Color::rgb(0x4fd8b1);

pub fn ease(t: f32) -> f32 {
    let t = t.clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

/// ProsperoStore's idle "breath" (ui/motion.hpp): 0..1..0 over `period` seconds.
pub fn breathe(time: f32, period: f32) -> f32 {
    0.5 - 0.5 * (time * std::f32::consts::TAU / period).cos()
}

/// Overshoots once and settles (a pop): 0..1.
pub fn back_out(t: f32) -> f32 {
    let t = t.clamp(0.0, 1.0) - 1.0;
    let c = 1.70158;
    1.0 + t * t * ((c + 1.0) * t + c)
}

/// The point `s` pixels along the outline of `r` (corners rounded by `radius`), clockwise from the
/// top edge's left end.
pub fn perimeter_point(r: Rect, radius: f32, s: f32) -> (f32, f32) {
    let rr = radius.min(r.w * 0.5).min(r.h * 0.5).max(0.0);
    let (sw, sh) = (r.w - 2.0 * rr, r.h - 2.0 * rr);
    let arc = std::f32::consts::FRAC_PI_2 * rr;
    let total = 2.0 * (sw + sh) + 4.0 * arc;
    let mut s = s.rem_euclid(total.max(1.0));
    // top edge, top-right corner, right edge, bottom-right, bottom edge, bottom-left, left edge, top-left
    let corners = [(r.right() - rr, r.y + rr, -std::f32::consts::FRAC_PI_2), (r.right() - rr, r.bottom() - rr, 0.0), (r.x + rr, r.bottom() - rr, std::f32::consts::FRAC_PI_2), (r.x + rr, r.y + rr, std::f32::consts::PI)];
    let edges = [(r.x + rr, r.y, 1.0, 0.0, sw), (r.right(), r.y + rr, 0.0, 1.0, sh), (r.right() - rr, r.bottom(), -1.0, 0.0, sw), (r.x, r.bottom() - rr, 0.0, -1.0, sh)];
    for i in 0..4 {
        let (x0, y0, dx, dy, len) = edges[i];
        if s <= len {
            return (x0 + dx * s, y0 + dy * s);
        }
        s -= len;
        if s <= arc {
            let (cx, cy, a0) = corners[i];
            let a = a0 + if rr > 0.0 { s / rr } else { 0.0 };
            return (cx + rr * a.cos(), cy + rr * a.sin());
        }
        s -= arc;
    }
    (r.x, r.y)
}

/// A light running around a border: a bright head and a fading tail of dots along the outline.
pub fn border_comet(scene: &mut Scene, r: Rect, radius: f32, time: f32, width: f32, colour: Color, alpha: f32) {
    let rr = radius.min(r.w * 0.5).min(r.h * 0.5).max(0.0);
    let total = 2.0 * (r.w - 2.0 * rr + r.h - 2.0 * rr) + 2.0 * std::f32::consts::PI * rr;
    // one lap every 3.2 s, whatever the size
    let head = (time / 3.2).fract() * total;
    let tail = (total * 0.22).min(260.0);
    let n = 30;
    for i in 0..n {
        let k = i as f32 / n as f32;
        let (x, y) = perimeter_point(r, radius, head - k * tail);
        let fade = (1.0 - k) * (1.0 - k);
        let size = width * (0.55 + 0.45 * (1.0 - k));
        scene.circle(x, y, size, colour.with_alpha(alpha * fade));
    }
    let (x, y) = perimeter_point(r, radius, head);
    scene.shadow(Rect::new(x - width, y - width, 2.0 * width, 2.0 * width), width, width * 5.0, colour.with_alpha(0.5 * alpha));
}

/// ProsperoStore's focused card (ui/components/card.cpp), behind it: a halo in the card's colour
/// that breathes (0.8..1 over 2.4 s). `amount` is the focus (0..1), `time` the screen's clock.
pub fn focus_halo(scene: &mut Scene, r: Rect, radius: f32, amount: f32, time: f32, glow: Color) {
    if amount <= 0.01 {
        return;
    }
    let idle = 0.8 + 0.2 * breathe(time, 2.4);
    scene.shadow(r, radius, 34.0, glow.with_alpha(0.6 * amount * idle));
}

/// ... and over it: a ring just outside it, breathing with the halo, and a light running around
/// the ring (one lap every 3.2 s).
pub fn focus_ring(scene: &mut Scene, r: Rect, radius: f32, amount: f32, time: f32) {
    if amount <= 0.01 {
        return;
    }
    let idle = 0.8 + 0.2 * breathe(time, 2.4);
    let ring = r.inset(-7.0);
    scene.stroke(ring, radius + 7.0, 3.0, ORANGE.with_alpha(amount * (0.7 + 0.3 * idle)));
    border_comet(scene, ring, radius + 7.0, time, 3.2, ORANGE_0, amount);
}

// ---- Ruffle's own flourishes: the "r" is a folded ribbon, leaning, orange with a brown fold ----

/// How far Ruffle's mark leans: its stems go 6.70 units left over 47.64 down.
pub const LEAN: f32 = 6.699219 / 47.636719;

/// A ribbon at the mark's lean with the mark's fold at its left end: the band, and under its start
/// a short darker piece, as the r's stem tucks under its arch.
pub fn ribbon_fold(scene: &mut Scene, r: Rect, colour: Color, alpha: f32) {
    let fold_w = (r.h * 1.4).min(r.w * 0.4);
    scene.ribbon(Rect::new(r.x - r.h * 0.2, r.y + r.h * 0.55, fold_w, r.h * 0.9), r.h * 0.2, LEAN, FOLD.with_alpha(alpha));
    scene.ribbon(r, r.h * 0.25, LEAN, colour.with_alpha(alpha));
}

/// The ribbon wipe between the menu and a game: three leaning bands (orange, the fold's brown, the
/// deep blue) sweep across and cover the screen at `t` = 1; drawn with `t` running back to 0 they
/// uncover it.
pub fn ribbon_wipe(scene: &mut Scene, t: f32) {
    if t <= 0.0 {
        return;
    }
    let bands = [(ORANGE, 0.0f32), (FOLD, 0.12), (NIGHT, 0.24)];
    for (colour, delay) in bands {
        let u = ease(((t - delay) / (1.0 - 0.24)).clamp(0.0, 1.0));
        if u <= 0.0 {
            continue;
        }
        // each band grows from the left edge across the whole screen, leaning
        let w = (W + H * LEAN + 200.0) * u;
        scene.ribbon(Rect::new(-H * LEAN * 0.5 - 100.0, -20.0, w, H + 40.0), 0.0, LEAN, colour);
    }
}

/// A burst of small leaning ribbon sparks, `age` seconds after it started at (x, y).
pub fn sparks(scene: &mut Scene, x: f32, y: f32, age: f32, seed: u32) {
    let life = 0.7;
    if !(0.0..life).contains(&age) {
        return;
    }
    let u = age / life;
    for i in 0..12u32 {
        let h = hash(&format!("{seed}-{i}"));
        let angle = (i as f32 / 12.0) * std::f32::consts::TAU + (h % 100) as f32 * 0.01;
        let speed = 160.0 + (h % 140) as f32;
        let d = speed * age * (1.0 - 0.45 * u);
        let (px, py) = (x + angle.cos() * d, y + angle.sin() * d + 120.0 * age * age);
        let len = 14.0 * (1.0 - u) + 4.0;
        let colour = if i % 3 == 0 { ORANGE_0 } else if i % 3 == 1 { ORANGE } else { ORANGE_3 };
        scene.ribbon(Rect::new(px - len * 0.5, py - 2.5, len, 5.0), 2.5, LEAN, colour.with_alpha(1.0 - u * u));
    }
}

/// The "ruffle" a cover gives when it is chosen, as the logo's letters do when a pointer passes:
/// a quick springy wobble of its scale that dies away (`age` seconds since).
pub fn ruffle(age: f32) -> f32 {
    if age < 0.0 {
        return 0.0;
    }
    0.035 * (age * 22.0).sin() * (-age * 7.0).exp()
}

/// A value that eases toward its target.
#[derive(Clone, Copy, Debug, Default)]
pub struct Smooth {
    pub value: f32,
    pub target: f32,
}

impl Smooth {
    pub fn snap(&mut self, v: f32) {
        self.value = v;
        self.target = v;
    }
    pub fn step(&mut self, dt: f32) {
        let k = 1.0 - (-16.0 * dt).exp();
        self.value += (self.target - self.value) * k;
        if (self.target - self.value).abs() < 0.0005 {
            self.value = self.target;
        }
    }
}

/// The baseline that centres text of `size` on `cy`.
pub fn centred(cy: f32, size: f32) -> f32 {
    cy + size * 0.36
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Glyph {
    Cross,
    Circle,
    Square,
    Triangle,
    /// The d-pad, with one direction lit (0 up, 1 right, 2 down, 3 left), or none (4).
    DPad(u8),
    Chip(&'static str),
}

/// Width of a glyph drawn `h` tall.
pub fn glyph_width(ui: &Ui, glyph: Glyph, h: f32) -> f32 {
    match glyph {
        Glyph::Chip(label) => ui.measure(Face::SemiBold, h * 0.5, label, 0.0) + h * 0.8,
        _ => h,
    }
}

/// A controller glyph with its left edge at `x`, vertically centred on `cy`.
pub fn draw_glyph(ui: &mut Ui, scene: &mut Scene, glyph: Glyph, x: f32, cy: f32, h: f32) {
    let cx = x + h * 0.5;
    match glyph {
        Glyph::Cross | Glyph::Circle | Glyph::Square | Glyph::Triangle => {
            scene.circle(cx, cy, h * 0.5, NIGHT.with_alpha(0.92));
            scene.ring(cx, cy, h * 0.5, 1.2, BLUE_4.with_alpha(0.5));
            let thick = (h * 0.09).max(1.5);
            match glyph {
                Glyph::Cross => {
                    let len = h * 0.46;
                    for a in [0.7854f32, -0.7854] {
                        scene.rect_rotated(Rect::new(cx - len * 0.5, cy - thick * 0.5, len, thick), thick * 0.5, a, CROSS_BLUE);
                    }
                }
                Glyph::Circle => scene.ring(cx, cy, h * 0.2, thick, CIRCLE_RED),
                Glyph::Square => {
                    let s = h * 0.36;
                    scene.stroke(Rect::new(cx - s * 0.5, cy - s * 0.5, s, s), 1.0, thick, SQUARE_PINK);
                }
                _ => {
                    // a triangle from three bars
                    let r = h * 0.22;
                    let pts: Vec<(f32, f32)> = (0..3)
                        .map(|i| {
                            let a = -std::f32::consts::FRAC_PI_2 + i as f32 * std::f32::consts::TAU / 3.0;
                            (cx + r * a.cos(), cy + r * 0.12 + r * a.sin())
                        })
                        .collect();
                    for i in 0..3 {
                        let (x0, y0) = pts[i];
                        let (x1, y1) = pts[(i + 1) % 3];
                        let len = ((x1 - x0).powi(2) + (y1 - y0).powi(2)).sqrt() + thick * 0.6;
                        let ang = (y1 - y0).atan2(x1 - x0);
                        let (mx, my) = ((x0 + x1) * 0.5, (y0 + y1) * 0.5);
                        scene.rect_rotated(Rect::new(mx - len * 0.5, my - thick * 0.5, len, thick), thick * 0.5, ang, TRIANGLE_GREEN);
                    }
                }
            }
        }
        Glyph::DPad(dir) => {
            scene.circle(cx, cy, h * 0.5, NIGHT.with_alpha(0.92));
            scene.ring(cx, cy, h * 0.5, 1.2, BLUE_4.with_alpha(0.5));
            let arm = h * 0.2;
            let len = h * 0.3;
            let spots = [(0.0, -1.0), (1.0, 0.0), (0.0, 1.0), (-1.0, 0.0)];
            for (i, (dx, dy)) in spots.iter().enumerate() {
                let lit = dir as usize == i;
                let (rx, ry) = (cx + dx * len * 0.62, cy + dy * len * 0.62);
                let r = if *dx == 0.0 { Rect::new(rx - arm * 0.5, ry - len * 0.5, arm, len) } else { Rect::new(rx - len * 0.5, ry - arm * 0.5, len, arm) };
                scene.rect(r, arm * 0.25, if lit { ORANGE } else { BLUE_2.with_alpha(0.45) });
            }
        }
        Glyph::Chip(label) => {
            let w = glyph_width(ui, glyph, h);
            let r = Rect::new(x, cy - h * 0.5, w, h);
            scene.rect(r, h * 0.28, NIGHT.with_alpha(0.92));
            scene.stroke(r, h * 0.28, 1.2, BLUE_4.with_alpha(0.6));
            scene.text(ui, Face::SemiBold, h * 0.5, label, r.cx(), centred(cy, h * 0.5), BLUE_0, Align::Center, 0.0);
        }
    }
}

/// A row of [glyph label] hints ending at `right`, centred on `cy`.
pub fn draw_hints(ui: &mut Ui, scene: &mut Scene, hints: &[(Glyph, &str)], right: f32, cy: f32) {
    draw_hints_scaled(ui, scene, hints, right, cy, 1.0);
}

/// Each hint's width and the row's, for `draw_hints_scaled` at `scale`.
fn hint_widths(ui: &mut Ui, hints: &[(Glyph, &str)], scale: f32) -> (Vec<f32>, f32) {
    let h = 30.0 * scale;
    let size = 22.0 * scale;
    let gap = 10.0 * scale;
    let between = 36.0 * scale;
    let mut widths = Vec::new();
    let mut total = 0.0;
    for (i, (glyph, label)) in hints.iter().enumerate() {
        let lw = if label.is_empty() { 0.0 } else { ui.measure(Face::SemiBold, size, label, 0.0) + gap };
        let w = glyph_width(ui, *glyph, h) + lw;
        widths.push(w);
        total += w;
        if i + 1 < hints.len() {
            total += if hints[i + 1].1.is_empty() || label.is_empty() { 6.0 * scale } else { between };
        }
    }
    (widths, total)
}

/// How wide `draw_hints_scaled` draws `hints`.
pub fn hints_width(ui: &mut Ui, hints: &[(Glyph, &str)], scale: f32) -> f32 {
    hint_widths(ui, hints, scale).1
}

/// `draw_hints` at `scale` times its size (the smaller on-screen keyboards).
pub fn draw_hints_scaled(ui: &mut Ui, scene: &mut Scene, hints: &[(Glyph, &str)], right: f32, cy: f32, scale: f32) {
    let h = 30.0 * scale;
    let size = 22.0 * scale;
    let gap = 10.0 * scale;
    let between = 36.0 * scale;
    let (widths, total) = hint_widths(ui, hints, scale);
    let mut x = right - total;
    for (i, (glyph, label)) in hints.iter().enumerate() {
        draw_glyph(ui, scene, *glyph, x, cy, h);
        let gw = glyph_width(ui, *glyph, h);
        if !label.is_empty() {
            scene.text(ui, Face::SemiBold, size, label, x + gw + gap, centred(cy, size), INK.with_alpha(0.88), Align::Left, 0.0);
        }
        x += widths[i];
        if i + 1 < hints.len() {
            x += if hints[i + 1].1.is_empty() || label.is_empty() { 6.0 * scale } else { between };
        }
    }
}

/// The bar that fills while a button is held; `held` seconds so far, shown after `delay`, full at
/// `delay + fill`.
#[allow(clippy::too_many_arguments)]
pub fn draw_hold_bar(ui: &mut Ui, scene: &mut Scene, held: f32, delay: f32, fill: f32, words: &str, glyph: Glyph, y: f32) {
    let filled = ((held - delay) / fill).clamp(0.0, 1.0);
    let shown = ((held - delay) / 0.12).clamp(0.0, 1.0);
    let gw = glyph_width(ui, glyph, 30.0);
    let width = ui.measure(Face::SemiBold, 24.0, words, 0.0) + gw + 102.0;
    let pill = Rect::new((W - width) * 0.5, y, width, 76.0);
    scene.push_opacity(shown);
    scene.shadow(Rect::new(pill.x, pill.y + 12.0, pill.w, pill.h), 38.0, 40.0, BLACK.with_alpha(0.45));
    scene.rect(pill, 38.0, NIGHT.with_alpha(0.96));
    scene.stroke(pill, 38.0, 1.5, ORANGE.with_alpha(0.35));
    draw_glyph(ui, scene, glyph, pill.x + 30.0, pill.y + 32.0, 30.0);
    scene.text(ui, Face::SemiBold, 24.0, words, pill.x + 30.0 + gw + 18.0, centred(pill.y + 32.0, 24.0), INK, Align::Left, 0.0);
    let track = Rect::new(pill.x + 34.0, pill.y + 58.0, pill.w - 68.0, 6.0);
    scene.rect(track, 3.0, INK.with_alpha(0.16));
    scene.rect(Rect::new(track.x, track.y, (track.w * filled).max(6.0), track.h), 3.0, ORANGE);
    scene.pop_opacity();
}

/// A card: the deep blue, a hairline edge, a soft shadow.
pub fn card(scene: &mut Scene, r: Rect, radius: f32, alpha: f32) {
    scene.shadow(Rect::new(r.x, r.y + 14.0, r.w, r.h), radius, 42.0, BLACK.with_alpha(0.35 * alpha));
    scene.rect(r, radius, NIGHT.with_alpha(0.9 * alpha));
    scene.stroke(r, radius, 1.5, BLUE_4.with_alpha(0.28 * alpha));
}

/// A button: Ruffle orange with the maroon text (primary), or the mid blue with white (secondary),
/// a glyph at its left. `lit` breathes the glow of the focused one. Returns its width.
#[allow(clippy::too_many_arguments)]
pub fn button(ui: &mut Ui, scene: &mut Scene, x: f32, y: f32, h: f32, glyph: Glyph, label: &str, primary: bool, lit: f32) -> f32 {
    let size = h * 0.38;
    let gh = h * 0.5;
    let w = h * 0.36 + glyph_width(ui, glyph, gh) + 14.0 + ui.measure(Face::SemiBold, size, label, 0.0) + h * 0.5;
    let r = Rect::new(x, y, w, h);
    let (body, ink) = if primary { (ORANGE, ON_ORANGE) } else { (BLUE_7, WHITE) };
    if lit > 0.0 {
        scene.shadow(r, h * 0.5, 26.0, if primary { ORANGE } else { BLUE_4 }.with_alpha(0.3 * lit));
    }
    scene.rect(r, h * 0.5, body);
    scene.gradient_v(Rect::new(r.x, r.y, r.w, r.h * 0.5), h * 0.5, WHITE.with_alpha(0.14), WHITE.with_alpha(0.0));
    draw_glyph(ui, scene, glyph, x + h * 0.36, r.cy(), gh);
    scene.text(ui, Face::SemiBold, size, label, x + h * 0.36 + glyph_width(ui, glyph, gh) + 14.0, centred(r.cy(), size), ink, Align::Left, 0.0);
    w
}

/// A short message at the top right, fading in and out over `life` seconds; `age` since shown.
pub fn toast(ui: &mut Ui, scene: &mut Scene, msg: &str, age: f32, life: f32) {
    if age > life {
        return;
    }
    let a = ease((age / 0.2).min((life - age) / 0.35));
    let w = ui.measure(Face::SemiBold, 24.0, msg, 0.0) + 64.0;
    let r = Rect::new(W - 96.0 - w, 56.0 - (1.0 - a) * 16.0, w, 56.0);
    scene.push_opacity(a);
    scene.shadow(Rect::new(r.x, r.y + 8.0, r.w, r.h), 28.0, 30.0, BLACK.with_alpha(0.4));
    scene.rect(r, 28.0, NIGHT.with_alpha(0.94));
    scene.stroke(r, 28.0, 1.2, ORANGE.with_alpha(0.4));
    scene.circle(r.x + 28.0, r.cy(), 6.0, ORANGE);
    scene.text(ui, Face::SemiBold, 24.0, msg, r.x + 46.0, centred(r.cy(), 24.0), INK, Align::Left, 0.0);
    scene.pop_opacity();
}

/// A spinner: twelve dashes around (cx, cy), one bright, the rest fading behind it.
pub fn spinner(scene: &mut Scene, cx: f32, cy: f32, radius: f32, t: f32, colour: Color) {
    let n = 12;
    let head = (t * 12.0) as i32;
    for i in 0..n {
        let a = i as f32 / n as f32 * std::f32::consts::TAU;
        let age = (head - i).rem_euclid(n) as f32 / n as f32;
        let (x, y) = (cx + radius * a.cos(), cy + radius * a.sin());
        let len = radius * 0.42;
        scene.rect_rotated(Rect::new(x - len * 0.5, y - 3.5, len, 7.0), 3.5, a, colour.with_alpha(1.0 - age * 0.85));
    }
}

/// A badge's medal at (cx, cy): a disc with a ribbon ring, the badge's number of points as a star
/// row inside; dim when locked.
pub fn medal(ui: &mut Ui, scene: &mut Scene, cx: f32, cy: f32, r: f32, unlocked: bool, symbol: &str) {
    if unlocked {
        scene.shadow(Rect::new(cx - r, cy - r, 2.0 * r, 2.0 * r), r, r * 0.6, ORANGE.with_alpha(0.35));
        scene.circle(cx, cy, r, ORANGE);
        scene.ring(cx, cy, r * 0.82, r * 0.1, FOLD.with_alpha(0.55));
        scene.gradient_v(Rect::new(cx - r, cy - r, 2.0 * r, r), r, WHITE.with_alpha(0.3), WHITE.with_alpha(0.0));
        scene.text(ui, Face::SemiBold, r * 0.9, symbol, cx, centred(cy, r * 0.9), ON_ORANGE, Align::Center, 0.0);
    } else {
        scene.circle(cx, cy, r, NIGHT.with_alpha(0.9));
        scene.ring(cx, cy, r, 2.0, BLUE_4.with_alpha(0.5));
        scene.ring(cx, cy, r * 0.82, r * 0.08, BLUE_4.with_alpha(0.25));
        scene.text(ui, Face::SemiBold, r * 0.9, symbol, cx, centred(cy, r * 0.9), BLUE_4.with_alpha(0.7), Align::Center, 0.0);
    }
}

pub fn hash(s: &str) -> u32 {
    let mut h: u32 = 2166136261;
    for b in s.bytes() {
        h ^= b as u32;
        h = h.wrapping_mul(16777619);
    }
    h
}

/// "super_mario_63" -> "Super Mario 63".
pub fn pretty_name(stem: &str) -> String {
    let mut out = String::new();
    let mut new_word = true;
    for ch in stem.chars() {
        let c = if ch == '_' || ch == '-' || ch == '.' { ' ' } else { ch };
        if c == ' ' {
            if !out.ends_with(' ') && !out.is_empty() {
                out.push(' ');
            }
            new_word = true;
            continue;
        }
        if new_word {
            out.extend(c.to_uppercase());
            new_word = false;
        } else {
            out.push(c);
        }
    }
    let out = out.trim().to_string();
    if out.is_empty() {
        stem.to_string()
    } else {
        out
    }
}

pub fn initials(name: &str) -> String {
    let mut s: String = name.split_whitespace().filter_map(|w| w.chars().next()).take(2).collect();
    if s.is_empty() {
        s.push('?');
    }
    s.to_uppercase()
}

pub fn size_text(bytes: u64) -> String {
    if bytes >= 1 << 20 {
        format!("{:.1} MB", bytes as f64 / (1u64 << 20) as f64)
    } else if bytes >= 1 << 10 {
        format!("{} KB", bytes >> 10)
    } else {
        format!("{bytes} B")
    }
}

pub fn trim_float(v: f32) -> String {
    if (v - v.round()).abs() < 0.01 {
        format!("{}", v.round() as i32)
    } else {
        format!("{v:.2}")
    }
}

/// "2 h 13 min", "4 min", "less than a minute".
pub fn duration_text(secs: u64) -> String {
    let (h, m) = (secs / 3600, (secs % 3600) / 60);
    match (h, m) {
        (0, 0) => "less than a minute".to_string(),
        (0, m) => format!("{m} min"),
        (h, 0) => format!("{h} h"),
        (h, m) => format!("{h} h {m} min"),
    }
}

/// "today", "yesterday", "3 days ago", from two Unix times.
pub fn ago_text(then: u64, now: u64) -> String {
    if then == 0 {
        return "never".to_string();
    }
    let days = now.saturating_sub(then) / 86_400;
    match days {
        0 => "today".to_string(),
        1 => "yesterday".to_string(),
        d if d < 60 => format!("{d} days ago"),
        d => format!("{} months ago", d / 30),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names_and_times_read_well() {
        assert_eq!(pretty_name("super_mario_63"), "Super Mario 63");
        assert_eq!(initials("Dad n Me"), "DN");
        assert_eq!(duration_text(0), "less than a minute");
        assert_eq!(duration_text(7980), "2 h 13 min");
        assert_eq!(duration_text(3600), "1 h");
        assert_eq!(ago_text(0, 100), "never");
        assert_eq!(ago_text(1000, 1000 + 86_400 * 3), "3 days ago");
        assert_eq!(size_text(2048), "2 KB");
    }
}
