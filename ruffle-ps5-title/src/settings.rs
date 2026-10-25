//! PS5_RUFFLE_SETTINGS_V1 — the title's settings, kept in /data/ruffle5/settings.txt as
//! `key=value` lines, and the rows the Settings section shows.
//!
//! The Display and Player options are Ruffle's own player options (as Ruffle on PC offers them),
//! the list and its explanations after Ruffle-Flash-PS5's settings (src/settings.rs, GPL-3.0, by
//! ZiZc3); Resolution (4K), the animated background and the controls rows are Ruffle5's.
// SPDX-License-Identifier: GPL-3.0-or-later

pub const SCALE_MODES: [&str; 4] = ["Unscaled (100%)", "Zoom to Fit", "Stretch to Fit", "Crop to Fit"];
pub const LETTERBOX: [&str; 3] = ["On", "Fullscreen Only", "Off"];
pub const ALIGNS: [&str; 9] = ["Center", "Top", "Bottom", "Left", "Right", "Top-Left", "Top-Right", "Bottom-Left", "Bottom-Right"];
pub const QUALITIES: [&str; 8] = ["Low", "Medium", "High", "Best", "High (8x8)", "High (8x8) Linear", "High (16x16)", "High (16x16) Linear"];
/// 0 = the game's own frame rate.
pub const FRAME_RATES: [u32; 8] = [0, 15, 24, 30, 40, 50, 60, 120];
/// 0 = Ruffle's default (32); then Flash Player versions.
pub const PLAYER_VERSIONS: [u8; 13] = [0, 6, 7, 8, 9, 10, 11, 15, 20, 25, 32, 40, 51];
pub const RUNTIMES: [&str; 2] = ["Flash Player", "Adobe AIR"];
pub const LOAD_BEHAVIORS: [&str; 3] = ["Streaming", "Delayed", "Blocking"];
/// Seconds a script may run before Ruffle stops it as stuck.
pub const MAX_EXECUTION: [u32; 6] = [5, 10, 15, 20, 30, 60];
pub const MOUSE_SPEEDS: [&str; 5] = ["Slowest", "Slow", "Normal", "Fast", "Fastest"];
/// Sites a sitelocked game may insist on, and where its file seems to come from there (the game's
/// own file name is appended).
pub const SPOOF_SITES: [(&str, &str); 7] = [
    ("Off", ""),
    ("Newgrounds", "https://uploads.ungrounded.net/alternate/1/"),
    ("Kongregate", "https://chat.kongregate.com/gamez/0001/0001/live/"),
    ("Armor Games", "https://cache.armorgames.com/files/games/"),
    ("Miniclip", "https://www.miniclip.com/games/en/"),
    ("Addicting Games", "https://www.addictinggames.com/games/"),
    ("Y8", "https://www.y8.com/games/"),
];

#[derive(Clone, PartialEq, Debug)]
pub struct Settings {
    /// Games render at the display's full 3840x2160 (else 1920x1080, scaled up).
    pub uhd: bool,
    pub quality: u8,
    pub scale_mode: u8,
    pub force_scale: bool,
    pub letterbox: u8,
    pub align: u8,
    pub force_align: bool,
    pub frame_rate: u8,
    pub player_version: u8,
    pub runtime: u8,
    pub load_behavior: u8,
    pub max_execution: u8,
    pub dummy_external_interface: bool,
    pub spoof_site: u8,
    /// 0..=10, tenths.
    pub volume: u8,
    pub mouse_speed: u8,
    pub auto_keyboard: bool,
    pub ui_sounds: bool,
    pub animated_background: bool,
    pub screensaver: bool,
    pub fps_counter: bool,
    pub auto_covers: bool,
    pub badges: bool,
    /// 1.000.028: a game fetched from the Online tab stays in the games folder after it is played
    /// (Off: it is deleted when the player leaves it; its saves stay).
    pub keep_online: bool,
}

impl Default for Settings {
    fn default() -> Self {
        Settings {
            uhd: false,
            quality: 2,
            scale_mode: 1,
            force_scale: false,
            letterbox: 0,
            align: 0,
            force_align: false,
            frame_rate: 0,
            player_version: 0,
            runtime: 0,
            load_behavior: 0,
            max_execution: 2,
            dummy_external_interface: false,
            spoof_site: 0,
            volume: 10,
            mouse_speed: 2,
            auto_keyboard: true,
            ui_sounds: true,
            animated_background: true,
            screensaver: true,
            fps_counter: false,
            auto_covers: true,
            badges: true,
            keep_online: true,
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Row {
    Resolution,
    Quality,
    ScaleMode,
    ForceScale,
    Letterbox,
    Align,
    ForceAlign,
    FrameRate,
    PlayerVersion,
    Runtime,
    LoadBehavior,
    MaxExecution,
    DummyExternalInterface,
    SpoofSite,
    AllControls,
    KeyboardLayout,
    /// 1.000.033: the menu's search keyboard, settings of its own.
    SearchKeyboard,
    AutoKeyboard,
    MouseSpeed,
    Volume,
    UiSounds,
    AnimatedBackground,
    Screensaver,
    FpsCounter,
    AutoCovers,
    Badges,
    Rescan,
    ClearRecent,
    KeepOnline,
    Version,
    CreatedBy,
    Credits,
}

/// The sections of Settings, each with its rows.
pub const CATEGORIES: [(&str, &[Row]); 6] = [
    ("Display", &[Row::Resolution, Row::Quality, Row::ScaleMode, Row::Letterbox, Row::Align, Row::ForceScale, Row::ForceAlign]),
    ("Player", &[Row::FrameRate, Row::PlayerVersion, Row::Runtime, Row::LoadBehavior, Row::MaxExecution, Row::DummyExternalInterface, Row::SpoofSite]),
    ("Controls & Sound", &[Row::AllControls, Row::KeyboardLayout, Row::SearchKeyboard, Row::AutoKeyboard, Row::MouseSpeed, Row::Volume, Row::UiSounds]),
    ("App", &[Row::AnimatedBackground, Row::Screensaver, Row::FpsCounter, Row::AutoCovers, Row::Badges]),
    ("Library", &[Row::Rescan, Row::ClearRecent, Row::KeepOnline]),
    ("About", &[Row::Version, Row::CreatedBy, Row::Credits]),
];

/// What a section is for (shown over its rows).
pub fn category_help(name: &str) -> &'static str {
    match name {
        "Display" => "How games are drawn: in 4K or 1080p, how smooth their edges are, how they fill the screen.",
        "Player" => "How Ruffle runs games: speed, Flash version, and helpers for games that will not start.",
        "Controls & Sound" => "The controller, a USB mouse and keyboard, the on-screen keyboard, and the volume.",
        "App" => "Ruffle5 itself: the animated background, the screensaver, covers, badges and the FPS counter.",
        "Library" => "Your games list: look again for new games, forget what you played recently, and keep or drop games from Flash Browser.",
        _ => "Ruffle5 plays Flash games on the PS5 with Ruffle, the open-source Flash Player emulator.",
    }
}

/// What a row does when pressed, for the action rows.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Act {
    EditAllControls,
    /// 1.000.031: the on-screen keyboard's settings for all games.
    EditKeyboard,
    /// 1.000.033: the menu's search keyboard's.
    EditSearchKeyboard,
    Rescan,
    ClearRecent,
}

impl Row {
    pub fn action(self) -> Option<Act> {
        match self {
            Row::AllControls => Some(Act::EditAllControls),
            Row::KeyboardLayout => Some(Act::EditKeyboard),
            Row::SearchKeyboard => Some(Act::EditSearchKeyboard),
            Row::Rescan => Some(Act::Rescan),
            Row::ClearRecent => Some(Act::ClearRecent),
            _ => None,
        }
    }

    /// Rows that only show something (About).
    pub fn is_info(self) -> bool {
        matches!(self, Row::Version | Row::CreatedBy | Row::Credits)
    }

    pub fn label(self) -> &'static str {
        match self {
            Row::Resolution => "Resolution",
            Row::Quality => "Quality",
            Row::ScaleMode => "Scale mode",
            Row::ForceScale => "Force scale mode",
            Row::Letterbox => "Letterbox",
            Row::Align => "Stage alignment",
            Row::ForceAlign => "Force alignment",
            Row::FrameRate => "Frame rate",
            Row::PlayerVersion => "Player version",
            Row::Runtime => "Player runtime",
            Row::LoadBehavior => "Load behavior",
            Row::MaxExecution => "Max script time",
            Row::DummyExternalInterface => "Dummy External Interface",
            Row::SpoofSite => "Pretend to be on",
            Row::AllControls => "Controls for all games",
            Row::KeyboardLayout => "On-screen keyboard",
            Row::SearchKeyboard => "Search keyboard",
            Row::AutoKeyboard => "Keyboard for text boxes",
            Row::MouseSpeed => "USB mouse speed",
            Row::Volume => "Game volume",
            Row::UiSounds => "Menu sounds",
            Row::AnimatedBackground => "Animated background",
            Row::Screensaver => "Screensaver",
            Row::FpsCounter => "FPS counter",
            Row::AutoCovers => "Automatic covers",
            Row::Badges => "Badges",
            Row::Rescan => "Rescan games",
            Row::ClearRecent => "Clear recently played",
            Row::KeepOnline => "Keep online games",
            Row::Version => "Version",
            Row::CreatedBy => "Created by",
            Row::Credits => "Built with",
        }
    }

    pub fn help(self, s: &Settings) -> String {
        let text = match self {
            Row::Resolution => {
                if s.uhd {
                    "4K: games are drawn at 3840x2160 — Flash is vector art, so this is real 4K, not an upscale. Takes effect when a game starts."
                } else {
                    "1080p: games are drawn at 1920x1080 and scaled to the display on the GPU. Choose 4K for the sharpest picture."
                }
            }
            Row::Quality => "Smoothing of edges. Low can help heavy games run faster; the 8x8 and 16x16 modes are the smoothest.",
            Row::ScaleMode => match s.scale_mode {
                0 => "Unscaled (100%): the game at its own size, without any zoom.",
                1 => "Zoom to Fit: as large as the screen allows without cropping, keeping its shape.",
                2 => "Stretch to Fit: fills the whole screen, ignoring the game's shape.",
                _ => "Crop to Fit: fills the screen keeping the game's shape, cropping its edges if needed.",
            },
            Row::ForceScale => "Stops the game from changing the scale mode, locking it to the one chosen above.",
            Row::Letterbox => "Black bars where the game does not fill the screen; they hide what is drawn outside its area. Fullscreen Only: only when the game asks for fullscreen.",
            Row::Align => "Where the game sits when it does not fill the screen (mostly with Unscaled).",
            Row::ForceAlign => "Stops the game from changing its alignment.",
            Row::FrameRate => "Runs games at a frame rate of your choice instead of their own.",
            Row::PlayerVersion => "The Flash Player version a game sees. Some old games only work with an older one.",
            Row::Runtime => "Pretend to be Flash Player in a browser, or Adobe AIR (for games made as AIR apps).",
            Row::LoadBehavior => "Streaming: the game starts while it loads. Delayed: once loaded. Blocking: loads everything before the first frame.",
            Row::MaxExecution => "How long a game's script may run before it is stopped as stuck.",
            Row::DummyExternalInterface => "Tells games the web page they expect is there, answering its calls with nothing. Some games wait for it and will not start without it.",
            Row::SpoofSite => "Makes a game believe it runs on this site. Many games only start on the site they were made for.",
            Row::AllControls => "What each button does in every game. A game's own mapping (Triangle on it in Games) starts from these. Saved in Custom Mappings.",
            Row::KeyboardLayout => "Its size, where it opens, and whether the game moves out of its way, for every game; ready-made settings for kinds of game. A game's own (quick menu: Keyboard for this game) start from these. Saved in Custom Mappings.",
            Row::SearchKeyboard => "The keyboard L2 opens to search Games and Flash Browser: its size, where it opens and how solid it is. It starts as the on-screen keyboard for all games; what you change here is its own. Saved in Custom Mappings.",
            Row::AutoKeyboard => "Opens the on-screen keyboard by itself when a game's text box wants typing (a name, a code).",
            Row::MouseSpeed => "How fast a USB mouse moves the pointer in games.",
            Row::Volume => "Volume of every game's sound and music.",
            Row::UiSounds => "Soft, quiet sounds as you move around the menus.",
            Row::AnimatedBackground => "The Ruffle logo animation from ruffle.rs behind the menu, drawn live at the display's resolution.",
            Row::Screensaver => "After two minutes without input the menu fades away and leaves the animation. Any button wakes it.",
            Row::FpsCounter => "Shows the game's frame rate in the corner while playing.",
            Row::AutoCovers => "Saves a picture of a game as its cover 8 seconds into its first run (games that have no picture of their own).",
            Row::Badges => "Little goals to unlock while you play. Off hides the Badges section and the popups (progress is kept).",
            Row::Rescan => "Looks again for games in the games folder and on USB drives.",
            Row::ClearRecent => "Empties Recent. Saves, play times and favorites are kept.",
            Row::KeepOnline => {
                if s.keep_online {
                    "On: a game you fetch from Flash Browser stays in your games folder after you play it. Delete one yourself with OPTIONS on its tile."
                } else {
                    "Off: a game fetched from Flash Browser is deleted when you leave it and come back to the menu. Its saves are kept."
                }
            }
            Row::Version | Row::CreatedBy => "",
            Row::Credits => "Ruffle (ruffle.rs) · DirPlayer by Igor Lira for Shockwave · PS5_Vulkan by Mihawk-99 · ideas from Ruffle-Flash-PS5 by ZiZc3 · menu after ProsperoStore · Inter, Montserrat and Liberation fonts.",
        };
        text.to_string()
    }

    pub fn value(self, s: &Settings) -> String {
        let on = |b: bool| if b { "On" } else { "Off" }.to_string();
        match self {
            Row::Resolution => if s.uhd { "4K (2160p)" } else { "1080p" }.to_string(),
            Row::Quality => QUALITIES[s.quality as usize].to_string(),
            Row::ScaleMode => SCALE_MODES[s.scale_mode as usize].to_string(),
            Row::ForceScale => on(s.force_scale),
            Row::Letterbox => LETTERBOX[s.letterbox as usize].to_string(),
            Row::Align => ALIGNS[s.align as usize].to_string(),
            Row::ForceAlign => on(s.force_align),
            Row::FrameRate => match FRAME_RATES[s.frame_rate as usize] {
                0 => "Game's own".to_string(),
                f => format!("{f} fps"),
            },
            Row::PlayerVersion => match PLAYER_VERSIONS[s.player_version as usize] {
                0 => "Default (32)".to_string(),
                v => v.to_string(),
            },
            Row::Runtime => RUNTIMES[s.runtime as usize].to_string(),
            Row::LoadBehavior => LOAD_BEHAVIORS[s.load_behavior as usize].to_string(),
            Row::MaxExecution => format!("{} s", MAX_EXECUTION[s.max_execution as usize]),
            Row::DummyExternalInterface => on(s.dummy_external_interface),
            Row::SpoofSite => SPOOF_SITES[s.spoof_site as usize].0.to_string(),
            Row::AllControls => "Edit".to_string(),
            Row::KeyboardLayout => "Edit".to_string(),
            Row::SearchKeyboard => "Edit".to_string(),
            Row::AutoKeyboard => on(s.auto_keyboard),
            Row::MouseSpeed => MOUSE_SPEEDS[s.mouse_speed as usize].to_string(),
            Row::Volume => format!("{}%", s.volume as u32 * 10),
            Row::UiSounds => on(s.ui_sounds),
            Row::AnimatedBackground => on(s.animated_background),
            Row::Screensaver => on(s.screensaver),
            Row::FpsCounter => on(s.fps_counter),
            Row::AutoCovers => on(s.auto_covers),
            Row::Badges => on(s.badges),
            Row::Rescan => "Look again".to_string(),
            Row::ClearRecent => "Clear".to_string(),
            Row::KeepOnline => on(s.keep_online),
            Row::Version => crate::APP_VERSION.to_string(),
            Row::CreatedBy => crate::AUTHOR.to_string(),
            Row::Credits => "Ruffle".to_string(),
        }
    }
}

impl Settings {
    pub fn load(path: &str) -> Settings {
        let mut s = Settings::default();
        let Ok(text) = std::fs::read_to_string(path) else {
            return s;
        };
        for line in text.lines() {
            let Some((k, v)) = line.split_once('=') else {
                continue;
            };
            let n: u8 = v.trim().parse().unwrap_or(0);
            let b = n != 0;
            let last = |len: usize| (len - 1) as u8;
            match k.trim() {
                "uhd" => s.uhd = b,
                "quality" => s.quality = n.min(last(QUALITIES.len())),
                "scale_mode" => s.scale_mode = n.min(last(SCALE_MODES.len())),
                "force_scale" => s.force_scale = b,
                "letterbox" => s.letterbox = n.min(last(LETTERBOX.len())),
                "align" => s.align = n.min(last(ALIGNS.len())),
                "force_align" => s.force_align = b,
                "frame_rate" => s.frame_rate = n.min(last(FRAME_RATES.len())),
                "player_version" => s.player_version = n.min(last(PLAYER_VERSIONS.len())),
                "runtime" => s.runtime = n.min(last(RUNTIMES.len())),
                "load_behavior" => s.load_behavior = n.min(last(LOAD_BEHAVIORS.len())),
                "max_execution" => s.max_execution = n.min(last(MAX_EXECUTION.len())),
                "dummy_external_interface" => s.dummy_external_interface = b,
                "spoof_site" => s.spoof_site = n.min(last(SPOOF_SITES.len())),
                "volume" => s.volume = n.min(10),
                "mouse_speed" => s.mouse_speed = n.min(last(MOUSE_SPEEDS.len())),
                "auto_keyboard" => s.auto_keyboard = b,
                "ui_sounds" => s.ui_sounds = b,
                "animated_background" => s.animated_background = b,
                "screensaver" => s.screensaver = b,
                "fps_counter" => s.fps_counter = b,
                "auto_covers" => s.auto_covers = b,
                "badges" => s.badges = b,
                "keep_online" => s.keep_online = b,
                _ => {}
            }
        }
        s
    }

    pub fn to_text(&self) -> String {
        let fields: [(&str, u8); 24] = [
            ("uhd", self.uhd as u8),
            ("quality", self.quality),
            ("scale_mode", self.scale_mode),
            ("force_scale", self.force_scale as u8),
            ("letterbox", self.letterbox),
            ("align", self.align),
            ("force_align", self.force_align as u8),
            ("frame_rate", self.frame_rate),
            ("player_version", self.player_version),
            ("runtime", self.runtime),
            ("load_behavior", self.load_behavior),
            ("max_execution", self.max_execution),
            ("dummy_external_interface", self.dummy_external_interface as u8),
            ("spoof_site", self.spoof_site),
            ("volume", self.volume),
            ("mouse_speed", self.mouse_speed),
            ("auto_keyboard", self.auto_keyboard as u8),
            ("ui_sounds", self.ui_sounds as u8),
            ("animated_background", self.animated_background as u8),
            ("screensaver", self.screensaver as u8),
            ("fps_counter", self.fps_counter as u8),
            ("auto_covers", self.auto_covers as u8),
            ("badges", self.badges as u8),
            ("keep_online", self.keep_online as u8),
        ];
        let mut body = String::from("# Ruffle5 settings (Settings in the menu writes this file)\n");
        for (k, v) in fields {
            body.push_str(&format!("{k}={v}\n"));
        }
        body
    }

    pub fn save(&self, path: &str) {
        crate::fsutil::write_in_background(path, self.to_text().into_bytes());
    }

    /// Steps a value left (-1) or right (+1); true when it changed.
    pub fn step(&mut self, row: Row, dir: i32) -> bool {
        fn step(v: &mut u8, count: usize, dir: i32) {
            *v = (*v as i32 + dir).clamp(0, count as i32 - 1) as u8;
        }
        fn flip(v: &mut bool, dir: i32) {
            *v = if dir == 0 { !*v } else { dir > 0 };
        }
        let before = self.clone();
        match row {
            Row::Resolution => flip(&mut self.uhd, dir),
            Row::Quality => step(&mut self.quality, QUALITIES.len(), dir),
            Row::ScaleMode => step(&mut self.scale_mode, SCALE_MODES.len(), dir),
            Row::ForceScale => flip(&mut self.force_scale, dir),
            Row::Letterbox => step(&mut self.letterbox, LETTERBOX.len(), dir),
            Row::Align => step(&mut self.align, ALIGNS.len(), dir),
            Row::ForceAlign => flip(&mut self.force_align, dir),
            Row::FrameRate => step(&mut self.frame_rate, FRAME_RATES.len(), dir),
            Row::PlayerVersion => step(&mut self.player_version, PLAYER_VERSIONS.len(), dir),
            Row::Runtime => step(&mut self.runtime, RUNTIMES.len(), dir),
            Row::LoadBehavior => step(&mut self.load_behavior, LOAD_BEHAVIORS.len(), dir),
            Row::MaxExecution => step(&mut self.max_execution, MAX_EXECUTION.len(), dir),
            Row::DummyExternalInterface => flip(&mut self.dummy_external_interface, dir),
            Row::SpoofSite => step(&mut self.spoof_site, SPOOF_SITES.len(), dir),
            Row::AutoKeyboard => flip(&mut self.auto_keyboard, dir),
            Row::MouseSpeed => step(&mut self.mouse_speed, MOUSE_SPEEDS.len(), dir),
            Row::Volume => step(&mut self.volume, 11, dir),
            Row::UiSounds => flip(&mut self.ui_sounds, dir),
            Row::AnimatedBackground => flip(&mut self.animated_background, dir),
            Row::Screensaver => flip(&mut self.screensaver, dir),
            Row::FpsCounter => flip(&mut self.fps_counter, dir),
            Row::AutoCovers => flip(&mut self.auto_covers, dir),
            Row::Badges => flip(&mut self.badges, dir),
            Row::KeepOnline => flip(&mut self.keep_online, dir),
            _ => return false,
        }
        *self != before
    }

    /// The URL a game should believe it was loaded from, when pretending.
    pub fn spoofed_url(&self, game_path: &str) -> Option<String> {
        let (_, base) = SPOOF_SITES[self.spoof_site as usize];
        if base.is_empty() {
            return None;
        }
        let file = game_path.rsplit('/').next().filter(|f| !f.is_empty()).unwrap_or("game.swf");
        Some(format!("{base}{file}"))
    }

    /// Game pixels per USB mouse count.
    pub fn mouse_multiplier(&self) -> f64 {
        [0.5, 0.8, 1.2, 1.8, 2.6][self.mouse_speed as usize]
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trips_and_clamps() {
        let mut s = Settings::default();
        s.uhd = true;
        s.quality = 6;
        s.spoof_site = 3;
        s.volume = 7;
        s.animated_background = false;
        s.keep_online = false;
        let dir = std::env::temp_dir().join(format!("ruffle5-settings-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("settings.txt");
        std::fs::write(&path, s.to_text()).unwrap();
        assert_eq!(Settings::load(path.to_str().unwrap()), s);
        std::fs::write(&path, "quality=99\nvolume=50\nspoof_site=200\nbogus\nuhd=1\n").unwrap();
        let t = Settings::load(path.to_str().unwrap());
        assert_eq!((t.quality, t.volume, t.spoof_site, t.uhd), (7, 10, 6, true));
        // a settings file from before 1.000.028 keeps online games
        assert!(t.keep_online);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn stepping_stays_in_range() {
        let mut s = Settings::default();
        assert!(!s.step(Row::Volume, 1)); // already at 100%
        assert!(s.step(Row::Volume, -1));
        assert_eq!(s.volume, 9);
        assert!(s.step(Row::Resolution, 0));
        assert!(s.uhd);
        assert!(!s.step(Row::Version, 1));
        assert_eq!(s.spoofed_url("/data/games/x.swf"), None);
        s.spoof_site = 1;
        assert_eq!(s.spoofed_url("/data/games/x.swf").unwrap(), "https://uploads.ungrounded.net/alternate/1/x.swf");
    }

    #[test]
    fn every_row_has_words() {
        let s = Settings::default();
        for (name, rows) in CATEGORIES {
            assert!(!category_help(name).is_empty());
            for r in rows {
                assert!(!r.label().is_empty());
                let _ = r.value(&s);
                if !r.is_info() || *r == Row::Credits {
                    let _ = r.help(&s);
                }
            }
        }
    }
}
