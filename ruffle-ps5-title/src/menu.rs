//! PS5_RUFFLE_MENU_V1 — the screen the title opens on: pick a game, or look at Settings.
//!
//! The layout and design follow ProsperoStore's home screen (darkxex/BlackBearReloaded,
//! GPL-3.0-or-later: `src/app/store.cpp` there — its constants are kept by name below): a top
//! bar with the mark, the wordmark and the sections as tabs under a gold line (L1/R1), a stage
//! that shows the focused game big — kicker, headline in Montserrat, a meta line, the call to
//! action — with its picture floating on the right, a shelf of 16:9 tiles, the focused one lifted
//! inside a white ring, and a hint row. Holding Circle fills a bar and closes the title.
//!
//! Games come from one folder (`GAMES_DIR`, set by the shell): every `.swf` in it, and the first
//! `.swf` in each sub-folder. A `<name>.png` beside the file (or `cover.png` / `icon.png` in the
//! folder) is the picture; without one, a tile gets a colour from its name and its initials.
// SPDX-License-Identifier: GPL-3.0-or-later

use crate::input::{BTN_CIRCLE, BTN_CROSS, BTN_DOWN, BTN_L1, BTN_LEFT, BTN_R1, BTN_RIGHT, BTN_UP};
use crate::swfinfo::{self, SwfInfo};
use crate::ui::{Align, Color, Face, ImageId, Rect, Scene, Ui, H, W};

pub const VERSION: &str = crate::APP_VERSION;
pub const AUTHOR: &str = "@TheUnwelcomed";

// ---- the palette (ProsperoStore's "Farlight" look) ----
const INK: Color = Color::rgb(0xf4efe6); // warm white: all text
const DEEP: Color = Color::rgb(0x0e0f24); // the dark,
const MID: Color = Color::rgb(0x42358f); // ... its mid tone
const ACCENT: Color = Color::rgb(0xffd166); // ... and its light: calls to action
const ON_ACCENT: Color = Color::rgb(0x241a05);
const COAL: Color = Color::rgb(0x0e0f12); // the page
const BLACK: Color = Color::rgb(0x000000);
const WHITE: Color = Color::rgb(0xffffff);
const CROSS_BLUE: Color = Color::rgb(0x4a86f7);
const CIRCLE_RED: Color = Color::rgb(0xff5d7c);
const CHIP_BODY: Color = Color::rgba(0x11131f, 0.92);
const CHIP_EDGE: Color = Color::rgba(0x6c7196, 0.9);
const CHIP_INK: Color = Color::rgb(0xe4e3f5);
const HINT_INK: Color = Color::rgba(0xf5f3ff, 0.86);

fn panel() -> Color {
    Color::mix(Color::rgb(0x202228), DEEP, 0.5)
}

// ---- the layout (store.cpp's constants, 1920x1080) ----
const MARGIN: f32 = 96.0;
const RIGHT: f32 = W - MARGIN;
const TOP_Y: f32 = 64.0; // the top bar's centre line
const TAB_H: f32 = 44.0;
const TABS_START: f32 = 664.0;
const TAB_PAD: f32 = 36.0;
const TAB_GAP: f32 = 4.0;
const STAGE_KICKER_Y: f32 = 240.0;
const STAGE_TITLE_Y: f32 = 338.0;
const STAGE_META_Y: f32 = 392.0;
const STAGE_TEXT_Y: f32 = 446.0;
const STAGE_BUTTON: Rect = Rect::new(96.0, 520.0, 200.0, 64.0);
const SHELF_HEAD_Y: f32 = 694.0;
const SHELF_TILES_Y: f32 = 718.0;
const TILE_W: f32 = 368.0;
const TILE_H: f32 = 207.0;
const TILE_GAP: f32 = 24.0;
const TILE_RADIUS: f32 = 18.0;
const LIFT: f32 = 0.06; // how much the focused tile grows
const HEADER_Y: f32 = 206.0;
const HINTS_Y: f32 = 1010.0; // the hint row's centre line
const HOLD_BAR_Y: f32 = 842.0;
const CLOSE_DELAY: f32 = 0.3; // the bar appears after this,
const CLOSE_HOLD: f32 = 1.2; // ... and fills over this
const OMEGA: f32 = 16.0; // how fast things move (rad/s)

const SECTIONS: [&str; 2] = ["Games", "Settings"];

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
        let k = 1.0 - (-OMEGA * dt).exp();
        self.value += (self.target - self.value) * k;
        if (self.target - self.value).abs() < 0.0005 {
            self.value = self.target;
        }
    }
}

pub struct Game {
    pub path: String,
    pub file: String,
    pub name: String,
    pub size: u64,
    pub info: Option<SwfInfo>,
    pub thumb_png: Option<Vec<u8>>,
    pub thumb: Option<ImageId>,
    pub preset: bool,
    pub accent: Color,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Action {
    Play(usize),
    Quit,
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Glyph {
    Cross,
    Circle,
    Chip(&'static str),
}

pub struct Menu {
    pub games: Vec<Game>,
    pub games_dir: String,
    pub report: Vec<String>,
    tab: usize,
    focus: usize,
    scroll: Smooth,
    lifts: Vec<Smooth>,
    line_x: Smooth,
    line_w: Smooth,
    tabs: [Rect; 2],
    time: f32,
    hold: f32,
    placed: bool,
}

impl Menu {
    /// Scan the games folder. Nothing is uploaded to the GPU yet (`load_pictures`).
    pub fn new(games_dir: &str, preset_dir: &str) -> Menu {
        let mut report = Vec::new();
        let mut games = Vec::new();
        match std::fs::read_dir(games_dir) {
            Ok(entries) => {
                let mut names: Vec<String> = entries.filter_map(|e| e.ok()).map(|e| e.file_name().to_string_lossy().into_owned()).collect();
                names.sort_by_key(|n| n.to_ascii_lowercase());
                for n in names {
                    let full = format!("{games_dir}/{n}");
                    let lower = n.to_ascii_lowercase();
                    if lower.ends_with(".swf") {
                        let stem = &n[..n.len() - 4];
                        let thumb = [format!("{games_dir}/{stem}.png"), format!("{games_dir}/{stem}.PNG")].into_iter().find(|p| std::path::Path::new(p).is_file());
                        games.push(Self::game(&full, &n, thumb, preset_dir, &mut report));
                    } else if std::path::Path::new(&full).is_dir() {
                        // A game in its own folder: its first .swf, its cover.png / icon.png.
                        if let Ok(inner) = std::fs::read_dir(&full) {
                            let mut files: Vec<String> = inner.filter_map(|e| e.ok()).map(|e| e.file_name().to_string_lossy().into_owned()).collect();
                            files.sort_by_key(|n| n.to_ascii_lowercase());
                            if let Some(swf) = files.iter().find(|f| f.to_ascii_lowercase().ends_with(".swf")) {
                                let stem = &swf[..swf.len() - 4];
                                let thumb = [format!("{full}/{stem}.png"), format!("{full}/cover.png"), format!("{full}/icon.png")].into_iter().find(|p| std::path::Path::new(p).is_file());
                                let mut g = Self::game(&format!("{full}/{swf}"), swf, thumb, preset_dir, &mut report);
                                g.name = pretty_name(&n);
                                games.push(g);
                            }
                        }
                    }
                }
                report.push(format!("menu: {} game(s) in {games_dir}", games.len()));
            }
            Err(e) => report.push(format!("menu: cannot list {games_dir}: {e}")),
        }
        let lifts = vec![Smooth::default(); games.len()];
        Menu {
            games,
            games_dir: games_dir.to_string(),
            report,
            tab: 0,
            focus: 0,
            scroll: Smooth::default(),
            lifts,
            line_x: Smooth::default(),
            line_w: Smooth::default(),
            tabs: [Rect::default(); 2],
            time: 0.0,
            hold: 0.0,
            placed: false,
        }
    }

    fn game(path: &str, file: &str, thumb: Option<String>, preset_dir: &str, report: &mut Vec<String>) -> Game {
        let stem = &file[..file.len() - 4];
        let size = std::fs::metadata(path).map(|m| m.len()).unwrap_or(0);
        let info = match swfinfo::read(path) {
            Ok(i) => Some(i),
            Err(e) => {
                report.push(format!("menu: {file}: header not read: {e}"));
                None
            }
        };
        let thumb_png = thumb.and_then(|p| std::fs::read(&p).ok());
        let preset = std::path::Path::new(&format!("{preset_dir}/{stem}.controls.txt")).is_file();
        let accent = Color::hue(hash(stem) as f32 / u32::MAX as f32, 0.55, 0.78);
        Game {
            path: path.to_string(),
            file: file.to_string(),
            name: pretty_name(stem),
            size,
            info,
            thumb_png,
            thumb: None,
            preset,
            accent,
        }
    }

    /// Decode and upload the pictures found by the scan.
    pub fn load_pictures(&mut self, ui: &mut Ui, device: &wgpu::Device, queue: &wgpu::Queue) {
        for g in &mut self.games {
            if let Some(png) = g.thumb_png.take() {
                match crate::ui::decode_png(&png) {
                    Ok((w, h, rgba)) if w > 0 && h > 0 && w <= 2048 && h <= 2048 => {
                        g.thumb = Some(ui.add_image(device, queue, w, h, &rgba));
                    }
                    Ok((w, h, _)) => self.report.push(format!("menu: {}: picture {w}x{h} not used (limit 2048)", g.file)),
                    Err(e) => self.report.push(format!("menu: {}: picture not decoded: {e}", g.file)),
                }
            }
        }
    }

    pub fn focused(&self) -> Option<&Game> {
        self.games.get(self.focus)
    }

    /// Advance the animations and act on the pad: `pressed` are this poll's new button bits,
    /// `down` the held ones (src/input.rs's BTN_ bits).
    pub fn update(&mut self, dt: f32, pressed: u32, down: u32) -> Option<Action> {
        self.time += dt;
        let mut action = None;
        if pressed & BTN_R1 != 0 {
            self.tab = (self.tab + 1) % SECTIONS.len();
        }
        if pressed & BTN_L1 != 0 {
            self.tab = (self.tab + SECTIONS.len() - 1) % SECTIONS.len();
        }
        if self.tab == 0 && !self.games.is_empty() {
            if pressed & BTN_RIGHT != 0 && self.focus + 1 < self.games.len() {
                self.focus += 1;
            }
            if pressed & BTN_LEFT != 0 && self.focus > 0 {
                self.focus -= 1;
            }
            if pressed & BTN_DOWN != 0 {
                self.focus = (self.focus + 4).min(self.games.len() - 1);
            }
            if pressed & BTN_UP != 0 {
                self.focus = self.focus.saturating_sub(4);
            }
            if pressed & BTN_CROSS != 0 {
                action = Some(Action::Play(self.focus));
            }
        }
        // Holding Circle closes the title: the bar appears after a moment, fills, then it is done.
        if down & BTN_CIRCLE != 0 {
            self.hold += dt;
            if self.hold >= CLOSE_DELAY + CLOSE_HOLD {
                action = Some(Action::Quit);
            }
        } else {
            self.hold = 0.0;
        }
        // Motion.
        for (k, lift) in self.lifts.iter_mut().enumerate() {
            lift.target = if k == self.focus { 1.0 } else { 0.0 };
            lift.step(dt);
        }
        let pitch = TILE_W + TILE_GAP;
        let visible = RIGHT - MARGIN;
        let tile_x = self.focus as f32 * pitch;
        self.scroll.target = self.scroll.target.clamp((tile_x + TILE_W - visible).max(0.0), tile_x.max(0.0));
        self.scroll.step(dt);
        action
    }

    /// Where the tabs sit, from their words (once the fonts can measure them).
    fn place_tabs(&mut self, ui: &Ui) {
        let mut x = TABS_START;
        for (i, name) in SECTIONS.iter().enumerate() {
            let w = ui.measure(Face::SemiBold, 22.0, name, 0.0) + TAB_PAD;
            self.tabs[i] = Rect::new(x, TOP_Y - TAB_H * 0.5, w, TAB_H);
            x += w + TAB_GAP;
        }
        let t = self.tabs[self.tab];
        self.line_x.target = t.x;
        self.line_w.target = t.w;
        if !self.placed {
            self.line_x.snap(t.x);
            self.line_w.snap(t.w);
            self.placed = true;
        }
    }

    pub fn draw(&mut self, ui: &mut Ui, scene: &mut Scene, dt: f32) {
        self.place_tabs(ui);
        self.line_x.step(dt);
        self.line_w.step(dt);
        self.draw_backdrop(scene);
        if self.tab == 0 {
            if self.games.is_empty() {
                self.draw_empty(ui, scene);
            } else {
                self.draw_stage(ui, scene);
                self.draw_shelf(ui, scene);
            }
        } else {
            self.draw_settings(ui, scene);
        }
        self.draw_top_bar(ui, scene);
        self.draw_hint_row(ui, scene);
        if self.hold > CLOSE_DELAY {
            draw_hold_bar(ui, scene, self.hold, "Keep holding to close Ruffle5", Glyph::Circle);
        }
    }

    fn accent(&self) -> Color {
        self.focused().map(|g| g.accent).unwrap_or(MID)
    }

    /// Farlight's clouds: two soft lights leaning toward the focused game's colour.
    fn draw_backdrop(&self, scene: &mut Scene) {
        let tint = self.accent();
        let drift = (self.time * 0.25).sin() * 40.0;
        scene.shadow(Rect::new(1040.0 + drift, -220.0, 1000.0, 720.0), 360.0, 320.0, Color::mix(MID, tint, 0.55).with_alpha(0.42));
        scene.shadow(Rect::new(-260.0 - drift, 420.0, 820.0, 640.0), 320.0, 300.0, Color::mix(MID, tint, 0.3).with_alpha(0.28));
        scene.shadow(Rect::new(500.0, 820.0, 1200.0, 500.0), 250.0, 260.0, Color::mix(DEEP, tint, 0.25).with_alpha(0.3));
    }

    fn draw_top_bar(&mut self, ui: &mut Ui, scene: &mut Scene) {
        // A soft band of the dark, so the bar reads over anything.
        scene.gradient_v(Rect::new(0.0, 0.0, W, 200.0), 0.0, DEEP.with_alpha(0.7), DEEP.with_alpha(0.0));
        // The mark: a gold diamond with a dark heart.
        scene.rect_rotated(Rect::new(MARGIN + 2.0, TOP_Y - 11.0, 22.0, 22.0), 5.0, 0.7854, ACCENT);
        scene.rect_rotated(Rect::new(MARGIN + 8.0, TOP_Y - 5.0, 10.0, 10.0), 2.0, 0.7854, DEEP);
        let brand = scene.text(ui, Face::SemiBold, 22.0, "RUFFLE5", MARGIN + 42.0, centred(TOP_Y, 22.0), INK, Align::Left, 5.0);
        let x = MARGIN + 42.0 + brand + 20.0;
        scene.rect(Rect::new(x, TOP_Y - 11.0, 1.5, 22.0), 0.0, INK.with_alpha(0.25));
        scene.text(ui, Face::SemiBold, 20.0, "Flash on PS5", x + 20.0, centred(TOP_Y, 20.0), ACCENT.with_alpha(0.92), Align::Left, 0.0);

        // The sections: words, the active one lit, one gold line gliding under it.
        let l1_w = glyph_width(ui, Glyph::Chip("L1"), 26.0);
        draw_glyph(ui, scene, Glyph::Chip("L1"), self.tabs[0].x - l1_w - 8.0, TOP_Y, 26.0, false);
        let under = Rect::new(self.line_x.value, 0.0, self.line_w.value, 0.0);
        for (i, name) in SECTIONS.iter().enumerate() {
            let tab = self.tabs[i];
            let overlap = (under.x + under.w).min(tab.x + tab.w) - under.x.max(tab.x);
            let on = (overlap / tab.w).clamp(0.0, 1.0);
            scene.text(ui, Face::SemiBold, 22.0, name, tab.cx(), centred(TOP_Y, 22.0), INK.with_alpha(0.52 + 0.48 * on), Align::Center, 0.0);
        }
        scene.rect(Rect::new(under.x + 18.0, TOP_Y + 21.0, (under.w - 36.0).max(8.0), 3.0), 1.5, ACCENT);
        let last = self.tabs[SECTIONS.len() - 1];
        draw_glyph(ui, scene, Glyph::Chip("R1"), last.x + last.w + 8.0, TOP_Y, 26.0, false);

        // The right end: how many games the folder holds.
        let count = match self.games.len() {
            0 => "no games".to_string(),
            1 => "1 game".to_string(),
            n => format!("{n} games"),
        };
        scene.text(ui, Face::SemiBold, 20.0, &count, RIGHT, centred(TOP_Y, 20.0), INK.with_alpha(0.8), Align::Right, 0.0);
    }

    /// The stage: the focused game, big, its picture floating on the right.
    fn draw_stage(&mut self, ui: &mut Ui, scene: &mut Scene) {
        let Some(game) = self.games.get(self.focus) else {
            return;
        };
        let (name, file, size, info, thumb, preset, accent) = (game.name.clone(), game.file.clone(), game.size, game.info.clone(), game.thumb, game.preset, game.accent);
        let initials = initials(&name);

        // The picture: large, floating a little over its own colours.
        let bob = 7.0 * (self.time * 0.9).sin();
        let cover = Rect::new(1252.0, 240.0 + bob, 352.0, 352.0);
        scene.shadow(cover.inset(-10.0), 90.0, 230.0, Color::mix(accent, INK, 0.18).with_alpha(0.4));
        scene.shadow(Rect::new(cover.x + 60.0, 606.0, cover.w - 120.0, 18.0), 9.0, 30.0 - bob, BLACK.with_alpha(0.55));
        scene.shadow(Rect::new(cover.x, cover.y + 30.0, cover.w, cover.h), 40.0, 70.0, BLACK.with_alpha(0.5));
        match thumb {
            Some(id) => scene.image(id, cover, 78.0, WHITE),
            None => {
                scene.gradient_v(cover, 78.0, Color::mix(accent, INK, 0.2), Color::mix(accent, DEEP, 0.55));
                scene.text(ui, Face::Display, 150.0, &initials, cover.cx(), cover.cy() + 54.0, INK.with_alpha(0.92), Align::Center, 2.0);
            }
        }
        scene.gradient_v(Rect::new(cover.x, cover.y, cover.w, cover.h * 0.46), 78.0, WHITE.with_alpha(0.1), WHITE.with_alpha(0.0));
        scene.stroke(cover, 78.0, 1.5, INK.with_alpha(0.2));

        // The words sit on the dark: from the left, and under the shelf.
        scene.gradient_h(Rect::new(0.0, 0.0, 1480.0, H), 0.0, DEEP.with_alpha(0.93), DEEP.with_alpha(0.0));
        scene.gradient_v(Rect::new(0.0, 380.0, W, 390.0), 0.0, DEEP.with_alpha(0.0), DEEP.with_alpha(0.95));
        scene.rect(Rect::new(0.0, 770.0, W, H - 770.0), 0.0, DEEP.with_alpha(0.95));

        scene.rect(Rect::new(MARGIN, STAGE_KICKER_Y - 9.0, 28.0, 3.0), 1.5, ACCENT);
        scene.text(ui, Face::SemiBold, 18.0, "FLASH GAME", MARGIN + 40.0, STAGE_KICKER_Y, ACCENT, Align::Left, 4.0);
        let headline = ui.fit(Face::Display, 92.0, &name, 1100.0);
        scene.text(ui, Face::Display, 92.0, &headline, MARGIN - 4.0, STAGE_TITLE_Y, INK, Align::Left, 0.0);
        let mut meta = vec![file.clone(), size_text(size)];
        if let Some(i) = &info {
            meta.push(format!("SWF {}", i.version));
            if i.width > 0 && i.height > 0 {
                meta.push(format!("{} \u{d7} {}", i.width, i.height));
            }
            if i.frame_rate > 0.0 {
                meta.push(format!("{} fps", trim_float(i.frame_rate)));
            }
            if i.frames > 0 {
                meta.push(format!("{} frames", i.frames));
            }
        }
        let meta = ui.fit(Face::Regular, 26.0, &meta.join("  \u{b7}  "), 1100.0);
        scene.text(ui, Face::Regular, 26.0, &meta, MARGIN, STAGE_META_Y, INK.with_alpha(0.8), Align::Left, 0.0);
        let line1 = if preset {
            "Controls: a preset for this game ships with the title.".to_string()
        } else {
            format!("Controls: the defaults. Add {}.controls.txt beside the game to change them.", file.trim_end_matches(".swf").trim_end_matches(".SWF"))
        };
        let line1 = ui.fit(Face::Regular, 26.0, &line1, 1100.0);
        scene.text(ui, Face::Regular, 26.0, &line1, MARGIN, STAGE_TEXT_Y, INK.with_alpha(0.84), Align::Left, 0.0);
        scene.text(ui, Face::Regular, 26.0, "In the game, hold OPTIONS to come back here.", MARGIN, STAGE_TEXT_Y + 38.0, INK.with_alpha(0.84), Align::Left, 0.0);

        // The call to action, lit.
        let b = STAGE_BUTTON;
        let breath = 0.5 + 0.5 * (self.time * 2.0).sin();
        scene.shadow(b, b.h * 0.5, 26.0, ACCENT.with_alpha(0.22 + 0.08 * breath));
        scene.rect(b, b.h * 0.5, ACCENT);
        scene.stroke(b, b.h * 0.5, 1.5, ACCENT);
        draw_glyph(ui, scene, Glyph::Cross, b.x + 22.0, b.cy(), 32.0, true);
        scene.text(ui, Face::SemiBold, 24.0, "Play", b.x + 70.0, centred(b.cy(), 24.0), ON_ACCENT, Align::Left, 0.0);
    }

    fn draw_shelf(&mut self, ui: &mut Ui, scene: &mut Scene) {
        let head = scene.text(ui, Face::SemiBold, 30.0, "Games", MARGIN, SHELF_HEAD_Y, INK, Align::Left, 0.0);
        scene.text(ui, Face::Regular, 18.0, &self.games.len().to_string(), MARGIN + head + 12.0, SHELF_HEAD_Y, INK.with_alpha(0.62), Align::Left, 0.0);
        let pitch = TILE_W + TILE_GAP;
        let scroll = self.scroll.value;
        // Resting tiles first, the focused one last, so its lift overlaps its neighbours.
        let order: Vec<usize> = (0..self.games.len()).filter(|&k| k != self.focus).chain(std::iter::once(self.focus)).collect();
        for k in order {
            let r = Rect::new(MARGIN + k as f32 * pitch - scroll, SHELF_TILES_Y, TILE_W, TILE_H);
            if r.x > W || r.x + r.w < 0.0 {
                continue;
            }
            let lift = self.lifts.get(k).map(|l| l.value).unwrap_or(0.0);
            let rs = r.scaled(1.0 + LIFT * lift);
            let game = &self.games[k];
            let light = 0.74 + 0.26 * lift;
            scene.gradient_v(rs, TILE_RADIUS, Color::mix(game.accent, MID, 0.25).with_alpha(0.9), Color::mix(game.accent, DEEP, 0.86));
            match game.thumb {
                Some(id) => {
                    scene.image(id, rs, TILE_RADIUS, Color { r: light, g: light, b: light, a: 1.0 });
                    scene.gradient_v(Rect::new(rs.x, rs.y + rs.h * 0.45, rs.w, rs.h * 0.55), TILE_RADIUS, BLACK.with_alpha(0.0), BLACK.with_alpha(0.72));
                }
                None => {
                    let s = rs.h * 0.48;
                    let icon = Rect::new(rs.cx() - s * 0.5, rs.y + rs.h * 0.4 - s * 0.5, s, s);
                    scene.shadow(icon, s * 0.3, 60.0, Color::mix(game.accent, INK, 0.2).with_alpha(0.35));
                    scene.gradient_v(icon, s * 0.22, Color::mix(game.accent, INK, 0.2), Color::mix(game.accent, DEEP, 0.55));
                    scene.stroke(icon, s * 0.22, 1.0, INK.with_alpha(0.2));
                    scene.text(ui, Face::Display, s * 0.46, &initials(&game.name), icon.cx(), icon.cy() + s * 0.17, INK.with_alpha(0.92), Align::Center, 1.0);
                }
            }
            let name = ui.fit(Face::SemiBold, 24.0, &game.name, rs.w - 40.0);
            scene.text(ui, Face::SemiBold, 24.0, &name, rs.x + 20.0, rs.y + rs.h - 42.0, INK, Align::Left, 0.0);
            let mut sub = size_text(game.size);
            if let Some(i) = &game.info {
                sub = format!("{sub}  \u{b7}  SWF {}", i.version);
            }
            scene.text(ui, Face::Regular, 18.0, &sub, rs.x + 20.0, rs.y + rs.h - 16.0, INK.with_alpha(0.62), Align::Left, 0.0);
            scene.stroke(rs, TILE_RADIUS, 1.5, INK.with_alpha(0.1));
            if lift > 0.01 {
                scene.stroke(rs.inset(-7.0), TILE_RADIUS + 7.0, 3.0, INK.with_alpha(lift));
            }
        }
    }

    fn draw_empty(&mut self, ui: &mut Ui, scene: &mut Scene) {
        scene.rect(Rect::new(MARGIN, STAGE_KICKER_Y - 9.0, 28.0, 3.0), 1.5, ACCENT);
        scene.text(ui, Face::SemiBold, 18.0, "NOTHING TO PLAY YET", MARGIN + 40.0, STAGE_KICKER_Y, ACCENT, Align::Left, 4.0);
        scene.text(ui, Face::Display, 92.0, "No games", MARGIN - 4.0, STAGE_TITLE_Y, INK, Align::Left, 0.0);
        let line = ui.fit(Face::Regular, 26.0, &format!("Put .swf files in {} (FTP) and open Ruffle5 again.", self.games_dir), 1500.0);
        scene.text(ui, Face::Regular, 26.0, &line, MARGIN, STAGE_META_Y, INK.with_alpha(0.8), Align::Left, 0.0);
        scene.text(ui, Face::Regular, 26.0, "A game can also sit in its own folder, with a cover.png for its picture.", MARGIN, STAGE_TEXT_Y, INK.with_alpha(0.84), Align::Left, 0.0);
        for (i, line) in self.report.iter().take(3).enumerate() {
            scene.text(ui, Face::Regular, 20.0, line, MARGIN, STAGE_TEXT_Y + 60.0 + 30.0 * i as f32, INK.with_alpha(0.5), Align::Left, 0.0);
        }
    }

    fn draw_settings(&mut self, ui: &mut Ui, scene: &mut Scene) {
        scene.text(ui, Face::Display, 56.0, "Settings", MARGIN, HEADER_Y, INK, Align::Left, 0.0);
        let rows: [(&str, String); 2] = [("Version", VERSION.to_string()), ("Created by", AUTHOR.to_string())];
        let row_h = 72.0;
        let p = Rect::new(MARGIN, 248.0, 900.0, rows.len() as f32 * row_h + 24.0);
        scene.shadow(Rect::new(p.x, p.y + 10.0, p.w, p.h), 20.0, 30.0, BLACK.with_alpha(0.35));
        scene.rect(p, 20.0, panel());
        scene.stroke(p, 20.0, 1.5, INK.with_alpha(0.1));
        for (i, (label, value)) in rows.iter().enumerate() {
            let cy = p.y + 12.0 + row_h * (i as f32 + 0.5);
            scene.text(ui, Face::SemiBold, 24.0, label, p.x + 32.0, centred(cy, 24.0), INK, Align::Left, 0.0);
            scene.text(ui, Face::Regular, 24.0, value, p.x + p.w - 32.0, centred(cy, 24.0), INK.with_alpha(0.62), Align::Right, 0.0);
            if i + 1 < rows.len() {
                scene.rect(Rect::new(p.x + 32.0, p.y + 12.0 + row_h * (i as f32 + 1.0), p.w - 64.0, 1.0), 0.0, INK.with_alpha(0.08));
            }
        }
    }

    fn draw_hint_row(&mut self, ui: &mut Ui, scene: &mut Scene) {
        let mut hints: Vec<(Glyph, &str)> = Vec::new();
        if self.tab == 0 && !self.games.is_empty() {
            hints.push((Glyph::Cross, "Play"));
        }
        hints.push((Glyph::Chip("L1"), ""));
        hints.push((Glyph::Chip("R1"), "Sections"));
        hints.push((Glyph::Circle, "Close (hold)"));
        draw_hints(ui, scene, &hints, RIGHT, HINTS_Y);
    }
}

/// The baseline that centres text of `size` on `cy`.
pub fn centred(cy: f32, size: f32) -> f32 {
    cy + size * 0.36
}

fn hash(s: &str) -> u32 {
    let mut h: u32 = 2166136261;
    for b in s.bytes() {
        h ^= b as u32;
        h = h.wrapping_mul(16777619);
    }
    h
}

/// "super_mario_63" -> "Super Mario 63".
fn pretty_name(stem: &str) -> String {
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

fn initials(name: &str) -> String {
    let mut s: String = name.split_whitespace().filter_map(|w| w.chars().next()).take(2).collect();
    if s.is_empty() {
        s.push('?');
    }
    s.to_uppercase()
}

fn size_text(bytes: u64) -> String {
    if bytes >= 1 << 20 {
        format!("{:.1} MB", bytes as f64 / (1u64 << 20) as f64)
    } else if bytes >= 1 << 10 {
        format!("{} KB", bytes >> 10)
    } else {
        format!("{bytes} B")
    }
}

fn trim_float(v: f32) -> String {
    if (v - v.round()).abs() < 0.01 {
        format!("{}", v.round() as i32)
    } else {
        format!("{v:.2}")
    }
}

/// Width of a glyph drawn `h` tall.
pub fn glyph_width(ui: &Ui, glyph: Glyph, h: f32) -> f32 {
    match glyph {
        Glyph::Cross | Glyph::Circle => h,
        Glyph::Chip(label) => ui.measure(Face::SemiBold, h * 0.58, label, 0.0) + h * 0.9,
    }
}

/// A controller glyph with its left edge at `x`, vertically centred on `cy`. `light` colours it
/// for a light surface (the gold button).
pub fn draw_glyph(ui: &mut Ui, scene: &mut Scene, glyph: Glyph, x: f32, cy: f32, h: f32, light: bool) {
    match glyph {
        Glyph::Cross => {
            let c = if light { Color::rgb(0x1b1d2b) } else { CROSS_BLUE };
            let cx = x + h * 0.5;
            let len = h * 0.78;
            let thick = (h * 0.14).max(2.0);
            scene.rect_rotated(Rect::new(cx - len * 0.5, cy - thick * 0.5, len, thick), thick * 0.5, 0.7854, c);
            scene.rect_rotated(Rect::new(cx - len * 0.5, cy - thick * 0.5, len, thick), thick * 0.5, -0.7854, c);
        }
        Glyph::Circle => {
            let c = if light { Color::rgb(0x1b1d2b) } else { CIRCLE_RED };
            scene.ring(x + h * 0.5, cy, h * 0.38, (h * 0.13).max(2.0), c);
        }
        Glyph::Chip(label) => {
            let w = glyph_width(ui, glyph, h);
            let r = Rect::new(x, cy - h * 0.5, w, h);
            scene.rect(r, h * 0.28, CHIP_BODY);
            scene.stroke(r, h * 0.28, 1.5, CHIP_EDGE);
            scene.text(ui, Face::SemiBold, h * 0.58, label, r.cx(), centred(cy, h * 0.58), CHIP_INK, Align::Center, 0.0);
        }
    }
}

/// A row of [glyph label] hints ending at `right`, centred on `cy`.
pub fn draw_hints(ui: &mut Ui, scene: &mut Scene, hints: &[(Glyph, &str)], right: f32, cy: f32) {
    let h = 30.0;
    let size = 22.0;
    let gap = 12.0;
    let between = 44.0;
    // Measure from the right.
    let mut widths = Vec::new();
    let mut total = 0.0;
    for (i, (glyph, label)) in hints.iter().enumerate() {
        let lw = if label.is_empty() { 0.0 } else { ui.measure(Face::SemiBold, size, label, 0.0) + gap };
        let w = glyph_width(ui, *glyph, h) + lw;
        widths.push(w);
        total += w;
        if i + 1 < hints.len() {
            total += if hints[i + 1].1.is_empty() || label.is_empty() { 8.0 } else { between };
        }
    }
    let mut x = right - total;
    for (i, (glyph, label)) in hints.iter().enumerate() {
        draw_glyph(ui, scene, *glyph, x, cy, h, false);
        let gw = glyph_width(ui, *glyph, h);
        if !label.is_empty() {
            scene.text(ui, Face::SemiBold, size, label, x + gw + gap, centred(cy, size), HINT_INK, Align::Left, 0.0);
        }
        x += widths[i];
        if i + 1 < hints.len() {
            x += if hints[i + 1].1.is_empty() || label.is_empty() { 8.0 } else { between };
        }
    }
}

/// The bar that fills while a button is held; full, the thing happens. `held` is the seconds so
/// far (the bar shows after `CLOSE_DELAY`, fills over `CLOSE_HOLD`).
pub fn draw_hold_bar(ui: &mut Ui, scene: &mut Scene, held: f32, words: &str, glyph: Glyph) {
    let filled = ((held - CLOSE_DELAY) / CLOSE_HOLD).clamp(0.0, 1.0);
    let shown = ((held - CLOSE_DELAY) / 0.12).clamp(0.0, 1.0);
    let gw = glyph_width(ui, glyph, 30.0);
    let width = ui.measure(Face::SemiBold, 24.0, words, 0.0) + gw + 102.0;
    let pill = Rect::new((W - width) * 0.5, HOLD_BAR_Y, width, 76.0);
    scene.push_opacity(shown);
    scene.shadow(Rect::new(pill.x, pill.y + 12.0, pill.w, pill.h), 38.0, 40.0, BLACK.with_alpha(0.5));
    scene.rect(pill, 38.0, Color::mix(COAL, DEEP, 0.5).with_alpha(0.96));
    scene.stroke(pill, 38.0, 1.5, INK.with_alpha(0.22));
    draw_glyph(ui, scene, glyph, pill.x + 30.0, pill.y + 32.0, 30.0, false);
    scene.text(ui, Face::SemiBold, 24.0, words, pill.x + 30.0 + gw + 18.0, centred(pill.y + 32.0, 24.0), INK, Align::Left, 0.0);
    let track = Rect::new(pill.x + 34.0, pill.y + 58.0, pill.w - 68.0, 6.0);
    scene.rect(track, 3.0, INK.with_alpha(0.16));
    scene.rect(Rect::new(track.x, track.y, (track.w * filled).max(6.0), track.h), 3.0, ACCENT);
    scene.pop_opacity();
}

/// Hold timing shared with the in-game "return to the menu" bar.
pub const HOLD_TOTAL: f32 = CLOSE_DELAY + CLOSE_HOLD;
pub const HOLD_SHOW_AFTER: f32 = CLOSE_DELAY;
