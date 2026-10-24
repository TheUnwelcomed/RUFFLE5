//! PS5_RUFFLE_BADGES_V1 — badges: little goals for playing (first game, an hour of Flash, night
//! owl...), unlocked with a popup and a chime. Counters in /data/ruffle5/stats.txt, unlocked badges
//! in /data/ruffle5/badges.txt.
//!
//! After Ruffle-Flash-PS5's badges (src/badges.rs, GPL-3.0, by ZiZc3); the goals that belong to
//! that app's themes and accent colours are replaced by Ruffle5's own (4K, the controls editor, the
//! on-screen keyboard).
// SPDX-License-Identifier: GPL-3.0-or-later

use crate::ui::{Align, Face, Rect, Scene, Ui, W};
use crate::widgets::{centred, ease, medal, BLACK, BLUE_2, INK, NIGHT, ORANGE};
use std::collections::VecDeque;

const POPUP_TIME: f32 = 4.2;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Metric {
    Launches,
    PlayMinutes,
    LongestMinutes,
    GamesPlayed,
    LibrarySize,
    Favorites,
    Covers,
    QuickMenus,
    Restarts,
    Keyboard,
    Mouse,
    Searches,
    Settings,
    Screensaver,
    Controls,
    Night,
    Morning,
    Uhd,
    Osk,
    Badges,
}

pub struct Badge {
    pub id: &'static str,
    pub name: &'static str,
    pub about: &'static str,
    pub metric: Metric,
    pub target: u64,
}

const fn b(id: &'static str, name: &'static str, about: &'static str, metric: Metric, target: u64) -> Badge {
    Badge { id, name, about, metric, target }
}

use Metric as M;

pub const BADGES: [Badge; 34] = [
    b("first-flight", "First Flight", "Play your first game.", M::Launches, 1),
    b("regular", "Regular", "Start 10 games.", M::Launches, 10),
    b("arcade-rat", "Arcade Rat", "Start 50 games.", M::Launches, 50),
    b("flash-legend", "Flash Legend", "Start 200 games.", M::Launches, 200),
    b("warm-up", "Warm Up", "Play for 10 minutes in total.", M::PlayMinutes, 10),
    b("hour", "Hour of Flash", "Play for an hour in total.", M::PlayMinutes, 60),
    b("marathon", "Marathon", "Play for 10 hours in total.", M::PlayMinutes, 600),
    b("living-2007", "Living in 2007", "Play for 50 hours in total.", M::PlayMinutes, 3000),
    b("sit-tight", "Sit Tight", "Play one game for 30 minutes straight.", M::LongestMinutes, 30),
    b("deep-dive", "Deep Dive", "Play one game for 2 hours straight.", M::LongestMinutes, 120),
    b("explorer", "Explorer", "Play 5 different games.", M::GamesPlayed, 5),
    b("collector", "Collector", "Play 15 different games.", M::GamesPlayed, 15),
    b("archivist", "Archivist", "Play 40 different games.", M::GamesPlayed, 40),
    b("full-shelf", "Full Shelf", "Have 25 games in your library.", M::LibrarySize, 25),
    b("hoarder", "Hoarder", "Have 100 games in your library.", M::LibrarySize, 100),
    b("first-love", "First Love", "Add a game to your favorites.", M::Favorites, 1),
    b("curator", "Curator", "Have 10 favorite games.", M::Favorites, 10),
    b("photographer", "Photographer", "Take a game's cover yourself.", M::Covers, 1),
    b("gallery", "Gallery", "Take 10 covers yourself.", M::Covers, 10),
    b("breather", "Take a Breather", "Open the quick menu in a game.", M::QuickMenus, 1),
    b("pause-master", "Pause Master", "Open the quick menu 25 times.", M::QuickMenus, 25),
    b("again", "Again!", "Restart a game from the quick menu.", M::Restarts, 1),
    b("never-give-up", "Never Give Up", "Restart games 10 times.", M::Restarts, 10),
    b("typist", "Typist", "Play with a USB keyboard.", M::Keyboard, 1),
    b("point-click", "Point and Click", "Play with a USB mouse.", M::Mouse, 1),
    b("seeker", "Seeker", "Search your library.", M::Searches, 1),
    b("night-owl", "Night Owl", "Start a game between midnight and 5 am.", M::Night, 1),
    b("early-bird", "Early Bird", "Start a game between 5 and 8 in the morning.", M::Morning, 1),
    b("ultra-hd", "Ultra HD", "Play a game in 4K.", M::Uhd, 1),
    b("on-screen", "Hunt and Peck", "Type with the on-screen keyboard.", M::Osk, 1),
    b("tinkerer", "Tinkerer", "Change 10 settings.", M::Settings, 10),
    b("daydreamer", "Daydreamer", "Let the screensaver start.", M::Screensaver, 1),
    b("remapper", "Remapper", "Save controls for a game.", M::Controls, 1),
    b("completionist", "Completionist", "Unlock 30 other badges.", M::Badges, 30),
];

const STAT_NAMES: [&str; 16] = [
    "launches", "play_secs", "longest_secs", "covers", "quick_menus", "restarts", "keyboard", "mouse", "searches", "settings", "screensaver", "controls", "night",
    "morning", "uhd", "osk",
];

/// Counters kept across runs.
#[derive(Default, Clone, Debug, PartialEq)]
pub struct Stats {
    pub launches: u64,
    pub play_secs: u64,
    pub longest_secs: u64,
    pub covers: u64,
    pub quick_menus: u64,
    pub restarts: u64,
    pub keyboard: u64,
    pub mouse: u64,
    pub searches: u64,
    pub settings: u64,
    pub screensaver: u64,
    pub controls: u64,
    pub night: u64,
    pub morning: u64,
    pub uhd: u64,
    pub osk: u64,
}

impl Stats {
    fn slot(&mut self, name: &str) -> Option<&mut u64> {
        Some(match name {
            "launches" => &mut self.launches,
            "play_secs" => &mut self.play_secs,
            "longest_secs" => &mut self.longest_secs,
            "covers" => &mut self.covers,
            "quick_menus" => &mut self.quick_menus,
            "restarts" => &mut self.restarts,
            "keyboard" => &mut self.keyboard,
            "mouse" => &mut self.mouse,
            "searches" => &mut self.searches,
            "settings" => &mut self.settings,
            "screensaver" => &mut self.screensaver,
            "controls" => &mut self.controls,
            "night" => &mut self.night,
            "morning" => &mut self.morning,
            "uhd" => &mut self.uhd,
            "osk" => &mut self.osk,
            _ => return None,
        })
    }
}

/// Counts the library knows right now.
#[derive(Clone, Copy, Default)]
pub struct Live {
    pub games_played: u64,
    pub library_size: u64,
    pub favorites: u64,
}

pub struct Badges {
    pub stats: Stats,
    /// Unlock time per badge (0 = locked).
    unlocked: [u64; BADGES.len()],
    queue: VecDeque<usize>,
    popup: Option<(usize, f32)>,
    /// Settings > Badges: off drops the popups (progress is still kept).
    pub enabled: bool,
    stats_path: String,
    badges_path: String,
}

impl Badges {
    pub fn load(stats_path: &str, badges_path: &str) -> Badges {
        let mut stats = Stats::default();
        if let Ok(text) = std::fs::read_to_string(stats_path) {
            for line in text.lines() {
                if let Some((k, v)) = line.split_once('=') {
                    if let Some(slot) = stats.slot(k.trim()) {
                        *slot = v.trim().parse().unwrap_or(0);
                    }
                }
            }
        }
        let mut unlocked = [0u64; BADGES.len()];
        if let Ok(text) = std::fs::read_to_string(badges_path) {
            for line in text.lines() {
                let Some((id, t)) = line.split_once('\t') else {
                    continue;
                };
                if let Some(i) = BADGES.iter().position(|b| b.id == id) {
                    unlocked[i] = t.trim().parse().unwrap_or(1).max(1);
                }
            }
        }
        Badges {
            stats,
            unlocked,
            queue: VecDeque::new(),
            popup: None,
            enabled: true,
            stats_path: stats_path.to_string(),
            badges_path: badges_path.to_string(),
        }
    }

    pub fn save(&self) {
        let mut s = self.stats.clone();
        let mut body = String::new();
        for name in STAT_NAMES {
            if let Some(v) = s.slot(name) {
                body.push_str(&format!("{name}={v}\n"));
            }
        }
        crate::fsutil::write_in_background(self.stats_path.as_str(), body.into_bytes());
        let body: String = BADGES.iter().zip(self.unlocked.iter()).filter(|(_, t)| **t > 0).map(|(b, t)| format!("{}\t{}\n", b.id, t)).collect();
        crate::fsutil::write_in_background(self.badges_path.as_str(), body.into_bytes());
    }

    pub fn count(&self) -> usize {
        self.unlocked.iter().filter(|t| **t > 0).count()
    }

    pub fn unlocked_at(&self, i: usize) -> Option<u64> {
        Some(self.unlocked[i]).filter(|t| *t > 0)
    }

    fn value(&self, metric: Metric, live: &Live) -> u64 {
        let s = &self.stats;
        match metric {
            M::Launches => s.launches,
            M::PlayMinutes => s.play_secs / 60,
            M::LongestMinutes => s.longest_secs / 60,
            M::GamesPlayed => live.games_played,
            M::LibrarySize => live.library_size,
            M::Favorites => live.favorites,
            M::Covers => s.covers,
            M::QuickMenus => s.quick_menus,
            M::Restarts => s.restarts,
            M::Keyboard => s.keyboard,
            M::Mouse => s.mouse,
            M::Searches => s.searches,
            M::Settings => s.settings,
            M::Screensaver => s.screensaver,
            M::Controls => s.controls,
            M::Night => s.night,
            M::Morning => s.morning,
            M::Uhd => s.uhd,
            M::Osk => s.osk,
            M::Badges => self.count() as u64 - self.unlocked_at(BADGES.len() - 1).map_or(0, |_| 1),
        }
    }

    /// (now, goal) for badge `i`.
    pub fn progress(&self, i: usize, live: &Live) -> (u64, u64) {
        let b = &BADGES[i];
        (self.value(b.metric, live).min(b.target), b.target)
    }

    /// Unlocks every badge whose goal is met (queueing their popups) and saves the counters.
    pub fn check(&mut self, live: &Live) {
        let now = crate::library::now_secs().max(1);
        loop {
            let mut more = false;
            for i in 0..BADGES.len() {
                if self.unlocked[i] == 0 && self.value(BADGES[i].metric, live) >= BADGES[i].target {
                    self.unlocked[i] = now;
                    self.queue.push_back(i);
                    crate::say(&format!("badges: unlocked {}", BADGES[i].name));
                    more = true;
                }
            }
            // Completionist counts the others.
            if !more {
                break;
            }
        }
        self.save();
    }

    /// The unlock popup at the top of the screen, one badge at a time. True while one shows (the
    /// caller keeps drawing).
    pub fn draw_popup(&mut self, ui: &mut Ui, scene: &mut Scene, dt: f32) -> bool {
        if !self.enabled {
            self.queue.clear();
            self.popup = None;
            return false;
        }
        if let Some((_, t)) = self.popup.as_mut() {
            *t += dt;
        }
        if self.popup.map(|(_, t)| t > POPUP_TIME).unwrap_or(false) {
            self.popup = None;
        }
        if self.popup.is_none() {
            if let Some(i) = self.queue.pop_front() {
                self.popup = Some((i, 0.0));
                crate::sounds::play(crate::sounds::Sfx::Badge);
            }
        }
        let Some((i, t)) = self.popup else {
            return false;
        };
        let a = ease((t / 0.35).min((POPUP_TIME - t) / 0.4));
        let badge = &BADGES[i];
        let (w, h) = (640.0, 116.0);
        let r = Rect::new((W - w) * 0.5, -h + (40.0 + h) * a, w, h);
        scene.shadow(Rect::new(r.x, r.y + 10.0, r.w, r.h), 24.0, 34.0, BLACK.with_alpha(0.5 * a));
        scene.rect(r, 24.0, NIGHT.with_alpha(0.96 * a));
        scene.stroke(r, 24.0, 2.0, ORANGE.with_alpha(0.6 * a));
        scene.push_opacity(a);
        medal(ui, scene, r.x + 64.0, r.cy(), 38.0, true, &symbol(i));
        crate::widgets::ribbon_fold(scene, Rect::new(r.x + 126.0, r.y + 18.0, 24.0, 5.0), ORANGE, 1.0);
        scene.text(ui, Face::SemiBold, 18.0, "BADGE UNLOCKED", r.x + 160.0, r.y + 36.0, ORANGE, Align::Left, 3.0);
        crate::widgets::sparks(scene, r.x + 64.0, r.cy(), t - 0.2, i as u32);
        let name = ui.fit(Face::Display, 32.0, badge.name, w - 150.0);
        scene.text(ui, Face::Display, 32.0, &name, r.x + 124.0, r.y + 74.0, INK, Align::Left, 0.0);
        let about = ui.fit(Face::Regular, 20.0, badge.about, w - 150.0);
        scene.text(ui, Face::Regular, 20.0, &about, r.x + 124.0, centred(r.y + 96.0, 20.0), BLUE_2, Align::Left, 0.0);
        scene.pop_opacity();
        true
    }
}

/// What a medal shows: the badge's number (1-34), as the badges are a set to collect.
pub fn symbol(i: usize) -> String {
    format!("{}", i + 1)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ids_are_unique_and_goals_unlock() {
        let mut ids: Vec<&str> = BADGES.iter().map(|b| b.id).collect();
        ids.sort();
        ids.dedup();
        assert_eq!(ids.len(), BADGES.len());
        let dir = std::env::temp_dir().join(format!("ruffle5-badges-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let (s, b) = (dir.join("stats.txt"), dir.join("badges.txt"));
        let mut badges = Badges::load(s.to_str().unwrap(), b.to_str().unwrap());
        badges.stats.launches = 1;
        badges.stats.uhd = 1;
        badges.check(&Live::default());
        assert!(badges.unlocked_at(0).is_some());
        assert!(badges.unlocked_at(BADGES.iter().position(|x| x.id == "ultra-hd").unwrap()).is_some());
        assert_eq!(badges.count(), 2);
        let _ = std::fs::remove_dir_all(&dir);
    }
}
