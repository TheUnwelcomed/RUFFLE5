//! PS5_RUFFLE_MENU_V5 — the screen the title opens on: pick a game, set its controls, change
//! settings, look at badges; find more online.
//!
//! The layout follows ProsperoStore's home screen (darkxex/BlackBearReloaded, GPL-3.0-or-later:
//! `src/app/store.cpp`): a top bar with the mark, the wordmark and the sections as tabs under a line
//! (L1/R1), a stage that shows the focused game big — kicker, headline in Montserrat, a meta line,
//! the calls to action — with its picture floating on the right, a shelf of 16:9 tiles, the focused
//! one lifted inside a ring, and a hint row. Holding Circle fills a bar and closes the title.
//!
//! V2: Ruffle's branding — Ruffle blue and orange (src/widgets.rs), Ruffle's "r" next to RUFFLE5,
//! ruffle.rs's logo animation playing behind it all (src/background.rs). Sections: Games,
//! Favorites, Recent, Badges, Settings. On a game: Cross plays, Triangle opens its controls editor,
//! Square makes it a favourite; L2 searches (the on-screen keyboard, or type on a USB keyboard).
//! Each game shows how long it has been played and when. After two quiet minutes the menu fades and
//! leaves the animation (the screensaver); any button brings it back.
//!
//! Games come from the games folder (`GAMES_DIR`, set by the shell) and from a USB drive's root or
//! its `ruffle5` folder: every `.swf`, and the first `.swf` in each sub-folder. A `<name>.png` beside
//! the file (or `cover.png` / `icon.png` in the folder) is the picture, else a cover Ruffle5 took
//! (/data/ruffle5/covers); without one, a tile gets a colour from its name and its initials.
//!
//! V3: Director / Shockwave games too (src/shockwave.rs): a `.dcr`, `.dir` or `.dxr` file, or a
//! folder holding one (with its casts and Xtras) — started by the movie its Projector.ini names, or
//! its only .dcr (src/dirinfo.rs). A folder game's controls are named after the folder.
//!
//! V5 (1.000.027): the Online tab (src/online.rs) — Flash games and movies from the Internet
//! Archive and Flash Museum; a fetched game lands in the games folder in a folder of its own, whose
//! online.txt names the movie to play and the title to show. A rescan reuses the GPU pictures of
//! the games before it (each rescan used to add a new set).
// SPDX-License-Identifier: GPL-3.0-or-later

use crate::badges::{self, Badges, Live, BADGES};
use crate::input::{BTN_CIRCLE, BTN_CROSS, BTN_DOWN, BTN_L1, BTN_L2, BTN_LEFT, BTN_OPTIONS, BTN_R1, BTN_RIGHT, BTN_SQUARE, BTN_TRIANGLE, BTN_UP};
use crate::keyboard::Osk;
use crate::library::{now_secs, Library};
use crate::paths::{self, Paths};
use crate::settings::{self, Act, Row, Settings, CATEGORIES};
use crate::sounds::{self, Sfx};
use crate::swfinfo::{self, SwfInfo};
use crate::ui::{Align, Color, Face, ImageId, Rect, Scene, Ui, H, W};
use crate::widgets::*;
use ruffle_core::events::{LogicalKey, NamedKey, PlayerEvent};

// ---- the layout (store.cpp's constants, 1920x1080) ----
pub(crate) const MARGIN: f32 = 96.0;
pub(crate) const RIGHT: f32 = W - MARGIN;
const TOP_Y: f32 = 64.0; // the top bar's centre line
const TAB_H: f32 = 44.0;
const TABS_START: f32 = 560.0;
const TAB_PAD: f32 = 34.0;
const TAB_GAP: f32 = 2.0;
pub(crate) const STAGE_KICKER_Y: f32 = 236.0;
pub(crate) const STAGE_TITLE_Y: f32 = 330.0;
pub(crate) const STAGE_META_Y: f32 = 384.0;
pub(crate) const STAGE_TEXT_Y: f32 = 436.0;
pub(crate) const STAGE_BUTTONS_Y: f32 = 548.0;
pub(crate) const SHELF_HEAD_Y: f32 = 694.0;
pub(crate) const SHELF_TILES_Y: f32 = 718.0;
pub(crate) const TILE_W: f32 = 368.0;
pub(crate) const TILE_H: f32 = 207.0;
pub(crate) const TILE_GAP: f32 = 24.0;
pub(crate) const TILE_RADIUS: f32 = 18.0;
pub(crate) const LIFT: f32 = 0.06;
const HINTS_Y: f32 = 1012.0;
const HOLD_BAR_Y: f32 = 842.0;
const CLOSE_DELAY: f32 = 0.3;
const CLOSE_HOLD: f32 = 1.2;
/// The screensaver starts after this many seconds without input.
const SAVER_AFTER: f32 = 120.0;

static MARK_PNG: &[u8] = include_bytes!("../assets/ruffle-mark.png");

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Tab {
    Games,
    Favorites,
    Recent,
    /// V5: games and movies from the Internet Archive and Flash Museum (src/online.rs); 1.000.028
    /// Flashpoint too. Shown as "Flash Browser" since 1.000.029 (it was "Online").
    Online,
    Badges,
    Settings,
}

impl Tab {
    fn name(self) -> &'static str {
        match self {
            Tab::Games => "Games",
            Tab::Favorites => "Favorites",
            Tab::Recent => "Recent",
            Tab::Online => "Flash Browser",
            Tab::Badges => "Badges",
            Tab::Settings => "Settings",
        }
    }
    fn shelf(self) -> Option<usize> {
        match self {
            Tab::Games => Some(0),
            Tab::Favorites => Some(1),
            Tab::Recent => Some(2),
            _ => None,
        }
    }
}

/// What plays a game.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    /// A .swf: Ruffle (src/game.rs).
    Flash,
    /// A Director movie: DirPlayer (src/shockwave.rs).
    Shockwave,
}

pub struct Game {
    pub kind: Kind,
    /// The movie to open (for a Shockwave folder: the one its projector starts).
    pub path: String,
    /// The path the controls files are looked up for: `path`, or for a Shockwave folder game
    /// `<folder>/<folder name>.dcr` (no such file: its stem names the mapping, and the folder is
    /// where a `.controls.txt` beside it would be).
    pub controls_path: String,
    /// "Director 8.0" for a Shockwave game.
    pub director: Option<String>,
    pub file: String,
    /// The file's stem: what its controls files are named after.
    pub stem: String,
    /// File-safe and unique: its saves, cover and library record.
    pub key: String,
    pub name: String,
    pub size: u64,
    pub info: Option<SwfInfo>,
    /// A picture of the player's own beside the game (wins over a captured cover).
    pub own_picture: bool,
    /// It has a mapping of its own in Custom Mappings.
    pub mapping: bool,
    /// 1.000.028: the folder the game is in, in the games folder or a USB drive's (a game in a
    /// folder of its own), which Delete removes; None for a movie on its own in one of those (Delete
    /// removes the file and the picture beside it).
    pub folder: Option<String>,
    /// 1.000.028: fetched by the Online tab (its folder's online.txt names the movie).
    pub online: bool,
    thumb_png: Option<Vec<u8>>,
    pub thumb: Option<ImageId>,
    pub accent: Color,
}

#[derive(Clone, Debug, PartialEq)]
pub enum Action {
    Play(usize),
    /// The Online tab fetched a game, or it was there already: look again, then play this movie.
    PlayFile(String),
    /// The Online tab fetched a game: look again.
    Added(String),
    EditControls(usize),
    EditAllControls,
    /// 1.000.031: Settings > On-screen keyboard (for all games).
    EditKeyboard,
    Quit,
    Rescan,
    ClearRecent,
    /// A setting changed (already applied to the Settings passed in).
    Setting(Row),
    /// 1.000.028: the player confirmed deleting this game (Menu::delete_game).
    Delete(usize),
}

/// What the menu needs from the pad this frame.
pub struct PadIn<'a> {
    pub nav: u32,
    pub pressed: u32,
    pub down: u32,
    pub typed: &'a [char],
    pub backspace: bool,
}

struct SettingsPage {
    cat: usize,
    row: usize,
    in_rows: bool,
}

pub struct Menu {
    pub games: Vec<Game>,
    pub games_dir: String,
    mappings_dir: String,
    pub report: Vec<String>,
    tab: usize,
    focus: [usize; 3],
    scroll: Smooth,
    lifts: Vec<Smooth>,
    line_x: Smooth,
    line_w: Smooth,
    tab_rects: Vec<Rect>,
    time: f32,
    hold: f32,
    hold_armed: bool,
    search: String,
    osk: Osk,
    settings_page: SettingsPage,
    badge_focus: usize,
    badge_scroll: Smooth,
    idle: f32,
    saver: f32,
    toast: Option<(String, f32)>,
    /// How far the menu has faded in (0..1), after the logo's drop-in when animated.
    intro: f32,
    intro_delay: f32,
    mark: Option<(ImageId, (u32, u32))>,
    placed: bool,
    /// The shelf's games for the current tab (indices into `games`).
    visible: Vec<usize>,
    /// A cover being written for a game: (game, file, seconds waited).
    pending_cover: Option<(usize, String, f32)>,
    /// Seconds since the stage's game changed (its words and picture come in).
    stage_anim: f32,
    stage_game: Option<usize>,
    /// Seconds since the shelf was shown (its tiles rise in one after another).
    shelf_anim: f32,
    /// Seconds since a favourite was toggled (the star pops).
    fav_pop: f32,
    /// The settings' highlight, gliding between rows.
    settings_hl: Smooth,
    /// A game chosen: the ribbon wipe runs, then the menu hands it over (game, seconds).
    launch: Option<(usize, f32)>,
    /// Seconds since the menu came back: the ribbons sweep off it.
    uncover: f32,
    /// The Online tab (src/online.rs).
    pub online: crate::online::Online,
    /// GPU pictures of the games before a rescan, reused for the games after it.
    spare_images: Vec<ImageId>,
    /// 1.000.028: Delete asked for this game; seconds the question has been shown.
    confirm_delete: Option<(usize, f32)>,
}

/// How long the ribbon wipe into a game takes.
pub const LAUNCH_TIME: f32 = 0.5;

/// A path as the player knows it: "games/…" inside the games folder, "USB: …" on a drive.
fn short_path(p: &str, games_dir: &str) -> String {
    if let Some(rest) = p.strip_prefix(games_dir).and_then(|r| r.strip_prefix('/')) {
        return format!("games/{rest}");
    }
    for usb in ["/mnt/usb0/", "/mnt/usb1/"] {
        if let Some(rest) = p.strip_prefix(usb) {
            return format!("USB: {rest}");
        }
    }
    p.to_string()
}

/// The folders the menu lists games in: the games folder, then each USB drive's ruffle5 folder
/// and its root.
pub fn scan_roots(games_dir: &str) -> Vec<String> {
    let mut dirs = vec![games_dir.to_string()];
    for usb in ["/mnt/usb0", "/mnt/usb1"] {
        dirs.push(format!("{usb}/ruffle5"));
        dirs.push(usb.to_string());
    }
    dirs
}

impl Menu {
    /// Scan the folders. Nothing is uploaded to the GPU yet (`load_pictures`).
    pub fn new(paths: &Paths) -> Menu {
        let mut m = Menu {
            games: Vec::new(),
            games_dir: paths.games.clone(),
            mappings_dir: paths.mappings.clone(),
            report: Vec::new(),
            tab: 0,
            focus: [0; 3],
            scroll: Smooth::default(),
            lifts: Vec::new(),
            line_x: Smooth::default(),
            line_w: Smooth::default(),
            tab_rects: Vec::new(),
            time: 0.0,
            hold: 0.0,
            hold_armed: true,
            search: String::new(),
            osk: Osk::new(),
            settings_page: SettingsPage { cat: 0, row: 0, in_rows: false },
            badge_focus: 0,
            badge_scroll: Smooth::default(),
            idle: 0.0,
            saver: 0.0,
            toast: None,
            intro: 0.0,
            intro_delay: 0.0,
            mark: None,
            placed: false,
            visible: Vec::new(),
            pending_cover: None,
            stage_anim: 0.0,
            stage_game: None,
            shelf_anim: 0.0,
            fav_pop: 10.0,
            settings_hl: Smooth::default(),
            launch: None,
            uncover: 0.0,
            online: crate::online::Online::new(&paths.games),
            spare_images: Vec::new(),
            confirm_delete: None,
        };
        m.scan(paths);
        m
    }

    /// Look (again) for games in the folders.
    pub fn scan(&mut self, paths: &Paths) {
        self.report.clear();
        let mut games = Vec::new();
        let dirs = scan_roots(&paths.games);
        for dir in &dirs {
            let Ok(entries) = std::fs::read_dir(dir) else {
                if dir == &paths.games {
                    self.report.push(format!("menu: cannot list {dir}"));
                }
                continue;
            };
            let mut names: Vec<String> = entries.filter_map(|e| e.ok()).map(|e| e.file_name().to_string_lossy().into_owned()).collect();
            names.sort_by_key(|n| n.to_ascii_lowercase());
            for n in names {
                let full = format!("{dir}/{n}");
                let lower = n.to_ascii_lowercase();
                if lower.ends_with(".swf") {
                    let stem = &n[..n.len() - 4];
                    let thumb = [format!("{dir}/{stem}.png"), format!("{dir}/{stem}.PNG")].into_iter().find(|p| std::path::Path::new(p).is_file());
                    games.push(Self::game(paths, &full, &n, None, thumb, &mut self.report));
                } else if crate::dirinfo::is_movie(&n) && std::path::Path::new(&full).is_file() {
                    let stem = n.rsplit_once('.').map(|(s, _)| s).unwrap_or(&n);
                    let thumb = [format!("{dir}/{stem}.png"), format!("{dir}/{stem}.PNG")].into_iter().find(|p| std::path::Path::new(p).is_file());
                    games.push(Self::director_game(paths, &full, stem, &full, None, thumb));
                } else if !n.starts_with('.') && std::path::Path::new(&full).is_dir() {
                    // A game in its own folder (V4: or in the one folder inside it, as unzipping
                    // into a folder of the same name leaves it). Skipped folders are logged for
                    // the games folder and a USB drive's ruffle5 folder, not a drive's root.
                    let logged = dir == &paths.games || dir.ends_with("/ruffle5");
                    if Self::folder_game(paths, &full, &n, &mut games, &mut self.report) {
                        if let Some(g) = games.last_mut() {
                            g.folder = Some(full.clone());
                        }
                        continue;
                    }
                    let subdirs = Self::list(&full).into_iter().filter(|f| !f.starts_with('.') && std::path::Path::new(&format!("{full}/{f}")).is_dir()).collect::<Vec<_>>();
                    if subdirs.len() == 1 && Self::folder_game(paths, &format!("{full}/{}", subdirs[0]), &n, &mut games, &mut self.report) {
                        if let Some(g) = games.last_mut() {
                            g.folder = Some(full.clone());
                        }
                        if logged {
                            self.report.push(format!("menu: {n}/ is one folder down, in {n}/{}/ (listed)", subdirs[0]));
                        }
                    } else if logged {
                        let files = Self::list(&full);
                        let shown: Vec<&str> = files.iter().take(8).map(|f| f.as_str()).collect();
                        self.report.push(format!(
                            "menu: {dir}/{n}/ not listed: no .swf and no Director movie (.dcr/.dir/.dxr) in it; {} entries: {}{}",
                            files.len(),
                            shown.join(", "),
                            if files.len() > shown.len() { ", ..." } else { "" }
                        ));
                    }
                }
            }
        }
        // The same file seen twice (a USB drive's root and its ruffle5 folder): keep one.
        games.dedup_by(|a, b| a.path == b.path);
        // Two games with one key would share saves, a cover and play time: those get their path's
        // hash added; every other game keeps the key it always had.
        let keys: Vec<String> = games.iter().map(|g| g.key.clone()).collect();
        for g in &mut games {
            if keys.iter().filter(|k| **k == g.key).count() > 1 {
                g.key = format!("{}-{:08x}", g.key, paths::path_hash(&g.path));
            }
        }
        games.sort_by_key(|g| g.name.to_lowercase());
        self.report.push(format!("menu: {} game(s) in {} and USB drives", games.len(), paths.games));
        self.lifts = vec![Smooth::default(); games.len()];
        self.spare_images.extend(self.games.iter().filter_map(|g| g.thumb));
        self.games = games;
        self.focus = [0; 3];
        self.scroll.snap(0.0);
        // indices into the old list mean nothing in the new one
        self.pending_cover = None;
        self.confirm_delete = None;
        self.stage_game = None;
        self.online.refresh_installed();
    }

    /// Where the shelves' focus is (to put it back after a rescan).
    pub fn focus(&self) -> [usize; 3] {
        self.focus
    }

    /// Put the shelves' focus back (each is kept inside its shelf as the menu updates).
    pub fn set_focus(&mut self, focus: [usize; 3]) {
        self.focus = focus;
    }

    /// 1.000.028: delete game `gi` — its own folder (a folder game), else its movie and the picture
    /// beside it. Its saves, cover and play time stay (they are kept by its key, so a game fetched
    /// again finds them). Never a folder the menu looks in itself. Returns what went, for the log,
    /// and whether it went to the trash folder rather than off the disk.
    ///
    /// 1.000.032: what the console refuses to remove (every removal in the 1.000.031 log) is moved
    /// into the games folder's `.ruffle5-trash` (fsutil::remove_or_trash), which the menu does not
    /// list; Ruffle5 tries to empty it at each start.
    pub fn delete_game(&self, gi: usize) -> Result<(String, bool), String> {
        use crate::fsutil::{remove_or_trash, Removed};
        let g = self.games.get(gi).ok_or_else(|| "no such game".to_string())?;
        let roots = scan_roots(&self.games_dir);
        let root_of = |p: &str| std::path::Path::new(p).parent().map(|d| d.to_string_lossy().into_owned()).filter(|d| roots.contains(d));
        let said = |what: &str, r: Removed| match r {
            Removed::Gone => (what.to_string(), false),
            Removed::Trashed(to, why) => (format!("{what} moved to {to} (the console refused removing it: {why})"), true),
        };
        match &g.folder {
            Some(folder) => {
                let name = std::path::Path::new(folder).file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
                let Some(root) = root_of(folder).filter(|_| !name.is_empty() && !name.starts_with('.') && !roots.contains(folder)) else {
                    return Err(format!("{folder} is not a game's own folder"));
                };
                let r = remove_or_trash(folder, &root).map_err(|e| format!("{folder}: {e}"))?;
                Ok(said(&format!("{folder}/"), r))
            }
            None => {
                let Some(root) = root_of(&g.path) else {
                    return Err(format!("{} is not in a games folder", g.path));
                };
                let (mut text, trashed) = said(&g.path, remove_or_trash(&g.path, &root).map_err(|e| format!("{}: {e}", g.path))?);
                if let Some((dir, file)) = g.path.rsplit_once('/') {
                    let stem = file.rsplit_once('.').map(|(s, _)| s).unwrap_or(file);
                    for ext in ["png", "PNG"] {
                        let picture = format!("{dir}/{stem}.{ext}");
                        if std::path::Path::new(&picture).is_file() {
                            if let Ok(r) = remove_or_trash(&picture, &root) {
                                text.push_str(&format!(", {}", said(&picture, r).0));
                            }
                        }
                    }
                }
                Ok((text, trashed))
            }
        }
    }

    /// A folder's entry names, sorted case-insensitively (empty if it cannot be read).
    fn list(dir: &str) -> Vec<String> {
        let mut files: Vec<String> = std::fs::read_dir(dir)
            .map(|it| it.filter_map(|e| e.ok()).map(|e| e.file_name().to_string_lossy().into_owned()).collect())
            .unwrap_or_default();
        files.sort_by_key(|n| n.to_ascii_lowercase());
        files
    }

    /// The game in folder `full` (named after `name`, the folder in the games folder): the Director
    /// movie its projector starts, else its first .swf. True if one was added.
    ///
    /// 1.000.032: Director first. A Director game's folder can hold .swf files of its own (Flash cast
    /// members, a web site's advert loader): Paul Blart: Mall Cop's (gameloader.dcr, paul_blart.dcr,
    /// advert_loader.swf) started its 100x100 advert loader, a white screen.
    fn folder_game(paths: &Paths, full: &str, name: &str, games: &mut Vec<Game>, report: &mut Vec<String>) -> bool {
        let files = Self::list(full);
        // V5: a game the Online tab fetched names its movie (the item may hold several .swf files)
        // and its title in online.txt. 1.000.028: the movie may be a Director one (Flashpoint),
        // named, keyed and mapped after the folder as a Director game in its own folder is.
        let online = crate::online::read_online_txt(&format!("{full}/online.txt"));
        if let Some(movie) = online.get("movie").filter(|m| std::path::Path::new(&format!("{full}/{m}")).is_file()) {
            let thumb = [format!("{full}/cover.png"), format!("{full}/icon.png")].into_iter().find(|p| std::path::Path::new(p).is_file());
            let file = movie.rsplit('/').next().unwrap_or(movie).to_string();
            let path = format!("{full}/{movie}");
            let mut g = if crate::dirinfo::is_movie(&file) {
                Self::director_game(paths, &path, name, &format!("{full}/{name}.dcr"), Some(name), thumb)
            } else {
                Self::game(paths, &path, &file, Some(name), thumb, report)
            };
            if let Some(t) = online.get("title").filter(|t| !t.is_empty()) {
                g.name = t.clone();
            }
            g.online = true;
            games.push(g);
            return true;
        }
        if let Some(movie) = crate::dirinfo::launcher(full, &files) {
            // A Director game in its own folder: named and mapped after the folder.
            let thumb = [format!("{full}/{name}.png"), format!("{full}/cover.png"), format!("{full}/icon.png")].into_iter().find(|p| std::path::Path::new(p).is_file());
            let controls_path = format!("{full}/{name}.dcr");
            if let Some(swf) = files.iter().find(|f| f.to_ascii_lowercase().ends_with(".swf")) {
                report.push(format!("menu: {full}/: Director movie {movie} chosen over {swf}"));
            }
            games.push(Self::director_game(paths, &format!("{full}/{movie}"), name, &controls_path, Some(name), thumb));
            return true;
        }
        if let Some(swf) = files.iter().find(|f| f.to_ascii_lowercase().ends_with(".swf")) {
            let stem = &swf[..swf.len() - 4];
            let thumb = [format!("{full}/{stem}.png"), format!("{full}/cover.png"), format!("{full}/icon.png")].into_iter().find(|p| std::path::Path::new(p).is_file());
            games.push(Self::game(paths, &format!("{full}/{swf}"), swf, Some(name), thumb, report));
            return true;
        }
        false
    }

    fn game(paths: &Paths, path: &str, file: &str, folder: Option<&str>, thumb: Option<String>, report: &mut Vec<String>) -> Game {
        let stem = file[..file.len() - 4].to_string();
        let size = std::fs::metadata(path).map(|m| m.len()).unwrap_or(0);
        let info = match swfinfo::read(path) {
            Ok(i) => Some(i),
            Err(e) => {
                report.push(format!("menu: {file}: header not read: {e}"));
                None
            }
        };
        let key = paths::game_key(file);
        let own_picture = thumb.is_some();
        let picture = thumb.or_else(|| Some(paths.cover(&key)).filter(|p| std::path::Path::new(p).is_file()));
        let thumb_png = picture.and_then(|p| std::fs::read(&p).ok());
        let accent = Color::hue(hash(&stem) as f32 / u32::MAX as f32, 0.55, 0.78);
        let mapping = std::path::Path::new(&crate::controls::mapping_file(paths, Some(&stem))).is_file();
        Game {
            kind: Kind::Flash,
            path: path.to_string(),
            controls_path: path.to_string(),
            director: None,
            file: file.to_string(),
            name: pretty_name(folder.unwrap_or(&stem)),
            stem,
            key,
            size,
            info,
            own_picture,
            mapping,
            folder: None,
            online: false,
            thumb_png,
            thumb: None,
            accent,
        }
    }

    /// A Director game: the movie at `path`, named (and its controls) after `stem` — the file's
    /// stem, or its folder's name.
    fn director_game(paths: &Paths, path: &str, stem: &str, controls_path: &str, folder: Option<&str>, thumb: Option<String>) -> Game {
        let file = path.rsplit('/').next().unwrap_or(path).to_string();
        // A folder game's size is the folder's movies and casts together.
        let size = match folder {
            Some(_) => std::path::Path::new(path)
                .parent()
                .and_then(|d| std::fs::read_dir(d).ok())
                .map(|it| {
                    it.filter_map(|e| e.ok())
                        .filter(|e| {
                            let n = e.file_name().to_string_lossy().to_ascii_lowercase();
                            crate::dirinfo::is_movie(&n) || n.ends_with(".cct") || n.ends_with(".cst") || n.ends_with(".cxt")
                        })
                        .filter_map(|e| e.metadata().ok())
                        .map(|m| m.len())
                        .sum()
                })
                .unwrap_or(0),
            None => std::fs::metadata(path).map(|m| m.len()).unwrap_or(0),
        };
        let key = paths::game_key(stem);
        let own_picture = thumb.is_some();
        let picture = thumb.or_else(|| Some(paths.cover(&key)).filter(|p| std::path::Path::new(p).is_file()));
        let thumb_png = picture.and_then(|p| std::fs::read(&p).ok());
        let accent = Color::hue(hash(stem) as f32 / u32::MAX as f32, 0.55, 0.78);
        let mapping = std::path::Path::new(&crate::controls::mapping_file(paths, Some(stem))).is_file();
        Game {
            kind: Kind::Shockwave,
            path: path.to_string(),
            controls_path: controls_path.to_string(),
            director: crate::dirinfo::describe(path),
            file,
            name: pretty_name(folder.unwrap_or(stem)),
            stem: stem.to_string(),
            key,
            size,
            info: None,
            own_picture,
            mapping,
            folder: None,
            online: false,
            thumb_png,
            thumb: None,
            accent,
        }
    }

    /// Decode and upload the pictures found by the scan (and the mark).
    pub fn load_pictures(&mut self, ui: &mut Ui, device: &wgpu::Device, queue: &wgpu::Queue) {
        if self.mark.is_none() {
            match crate::ui::decode_png(MARK_PNG) {
                Ok((w, h, rgba)) => self.mark = Some((ui.add_image(device, queue, w, h, &rgba), (w, h))),
                Err(e) => self.report.push(format!("menu: the mark: {e}")),
            }
        }
        for g in &mut self.games {
            if let Some(png) = g.thumb_png.take() {
                match crate::ui::decode_png(&png) {
                    Ok((w, h, rgba)) if w > 0 && h > 0 => {
                        let (w, h, rgba) = crate::ui::shrink_rgba(w, h, &rgba, 1024);
                        // a slot of a picture from before the rescan, else a new one
                        g.thumb = Some(match self.spare_images.pop() {
                            Some(id) => {
                                ui.replace_image(device, queue, id, w, h, &rgba);
                                id
                            }
                            None => ui.add_image(device, queue, w, h, &rgba),
                        });
                    }
                    Ok(_) => {}
                    Err(e) => self.report.push(format!("menu: {}: picture not decoded: {e}", g.file)),
                }
            }
        }
    }

    /// A game's cover was just taken: show it (unless it has a picture of its own).
    pub fn refresh_cover(&mut self, index: usize, path: &str, ui: &mut Ui, device: &wgpu::Device, queue: &wgpu::Queue) {
        let Some(g) = self.games.get_mut(index) else {
            return;
        };
        if g.own_picture {
            return;
        }
        let Ok(png) = std::fs::read(path) else {
            return;
        };
        if let Ok((w, h, rgba)) = crate::ui::decode_png(&png) {
            match g.thumb {
                Some(id) => ui.replace_image(device, queue, id, w, h, &rgba),
                None => g.thumb = Some(ui.add_image(device, queue, w, h, &rgba)),
            }
        }
    }

    /// After the controls editor saved: which games have a mapping of their own now.
    pub fn refresh_mappings(&mut self) {
        for g in &mut self.games {
            g.mapping = std::path::Path::new(&format!("{}/{}.controls.txt", self.mappings_dir, g.stem)).is_file();
        }
    }

    /// A cover for game `index` is being written to `path`: show it once it is there.
    pub fn expect_cover(&mut self, index: usize, path: String) {
        self.pending_cover = Some((index, path, 0.0));
    }

    /// Call every menu frame: uploads an expected cover once its file is complete.
    pub fn poll_cover(&mut self, dt: f32, ui: &mut Ui, device: &wgpu::Device, queue: &wgpu::Queue) {
        let Some((index, path, waited)) = self.pending_cover.take() else {
            return;
        };
        // A PNG is complete when it ends with its IEND chunk (the writer renames a whole file in).
        let done = std::fs::read(&path).map(|b| b.len() > 12 && &b[b.len() - 8..b.len() - 4] == b"IEND").unwrap_or(false);
        if done {
            self.refresh_cover(index, &path, ui, device, queue);
        } else if waited + dt < 5.0 {
            self.pending_cover = Some((index, path, waited + dt));
        }
    }

    pub fn toast(&mut self, msg: &str) {
        self.toast = Some((msg.to_string(), 0.0));
    }

    /// Back from a game or the editor: wake up, and fade in again (after the logo's drop-in when
    /// the background animates).
    pub fn enter(&mut self, animated: bool) {
        self.intro = 0.0;
        self.intro_delay = if animated { 1.0 } else { 0.0 };
        self.idle = 0.0;
        self.saver = 0.0;
        self.hold = 0.0;
        self.hold_armed = false;
        self.shelf_anim = -self.intro_delay;
        self.stage_anim = -self.intro_delay;
        self.launch = None;
        self.uncover = 0.0;
    }

    fn tabs(&self, settings: &Settings) -> Vec<Tab> {
        // 1.000.029: Flash Browser last before Settings (it came after Recent).
        let mut t = vec![Tab::Games, Tab::Favorites, Tab::Recent];
        if settings.badges {
            t.push(Tab::Badges);
        }
        t.push(Tab::Online);
        t.push(Tab::Settings);
        t
    }

    fn compute_visible(&mut self, tab: Tab, library: &Library) {
        let q = self.search.to_lowercase();
        let matches = |g: &Game| q.is_empty() || g.name.to_lowercase().contains(&q) || g.file.to_lowercase().contains(&q);
        self.visible = match tab {
            Tab::Games => (0..self.games.len()).filter(|&i| matches(&self.games[i])).collect(),
            Tab::Favorites => (0..self.games.len()).filter(|&i| library.get(&self.games[i].key).favorite && matches(&self.games[i])).collect(),
            Tab::Recent => {
                let mut v: Vec<usize> = (0..self.games.len()).filter(|&i| library.get(&self.games[i].key).last_played > 0 && matches(&self.games[i])).collect();
                v.sort_by_key(|&i| std::cmp::Reverse(library.get(&self.games[i].key).last_played));
                v.truncate(24);
                v
            }
            _ => Vec::new(),
        };
    }

    /// The focused game (on a shelf tab), as an index into `games`.
    pub fn focused(&self) -> Option<usize> {
        let shelf = self.shelf_index()?;
        self.visible.get(self.focus[shelf]).copied()
    }

    fn shelf_index(&self) -> Option<usize> {
        // The tab's shelf, from the tab's position (Games, Favorites, Recent are always first).
        if self.tab < 3 {
            Some(self.tab)
        } else {
            None
        }
    }

    /// After a rescan: the wipe into the game whose movie is `path` (false if it is not listed).
    pub fn launch_path(&mut self, path: &str) -> bool {
        match self.games.iter().position(|g| g.path == path) {
            Some(gi) => {
                sounds::play(Sfx::Launch);
                self.launch = Some((gi, 0.0));
                true
            }
            None => false,
        }
    }

    /// Every menu frame: the Online tab's answers, its pictures onto the GPU.
    pub fn poll_online(&mut self, ui: &mut Ui, device: &wgpu::Device, queue: &wgpu::Queue) {
        self.online.poll(ui, device, queue);
    }

    pub fn live(&self, library: &Library) -> Live {
        Live { games_played: library.played() as u64, library_size: self.games.len() as u64, favorites: library.favorites() as u64 }
    }

    /// Advance the animations and act on the pad.
    pub fn update(&mut self, dt: f32, pad: &PadIn, settings: &mut Settings, library: &mut Library, badges: &mut Badges) -> Option<Action> {
        self.time += dt;
        self.uncover += dt;
        self.stage_anim += dt;
        self.shelf_anim += dt;
        self.fav_pop += dt;
        if let Some((gi, t)) = self.launch.as_mut() {
            *t += dt;
            if *t >= LAUNCH_TIME {
                let gi = *gi;
                self.launch = None;
                return Some(Action::Play(gi));
            }
            return None;
        }
        // a toast waits for the menu to have faded in (one made as a game ended)
        if let Some((_, t)) = self.toast.as_mut() {
            if self.intro >= 1.0 {
                *t += dt;
            }
        }
        for msg in self.online.take_toasts() {
            self.toast(&msg);
        }
        match self.online.take_outcome() {
            Some(crate::online::Outcome::Play(path)) => return Some(Action::PlayFile(path)),
            Some(crate::online::Outcome::Added(path)) => return Some(Action::Added(path)),
            None => {}
        }
        if self.intro_delay > 0.0 {
            self.intro_delay -= dt;
        } else {
            self.intro = (self.intro + dt / 0.6).min(1.0);
        }
        // The screensaver: any button wakes it, and that press does nothing else.
        let any = pad.pressed != 0 || pad.down != 0 || !pad.typed.is_empty();
        if any {
            if self.saver > 0.05 {
                sounds::play(Sfx::Wake);
                self.saver = 0.0;
                self.idle = 0.0;
                self.hold_armed = false;
                return None;
            }
            self.idle = 0.0;
        } else {
            self.idle += dt;
        }
        let sleeping = settings.screensaver && self.idle > SAVER_AFTER;
        if sleeping && self.saver == 0.0 {
            sounds::play(Sfx::Sleep);
            badges.stats.screensaver += 1;
            badges.check(&self.live(library));
        }
        self.saver = if sleeping { (self.saver + dt / 1.5).min(1.0) } else { 0.0 };

        let tabs = self.tabs(settings);
        if self.tab >= tabs.len() {
            self.tab = tabs.len() - 1;
        }
        let tab = tabs[self.tab];

        // The search keyboard has the pad while open.
        let mut nav = pad.nav;
        let mut pressed = pad.pressed;
        if self.osk.visible() {
            let mut events = Vec::new();
            let used = self.osk.update(dt, if self.osk.is_open() { nav } else { 0 }, &mut events);
            for e in events {
                match e {
                    PlayerEvent::TextInput { codepoint } => self.type_char(codepoint, badges),
                    PlayerEvent::KeyDown { key } => match key.logical_key {
                        LogicalKey::Named(NamedKey::Backspace) => {
                            self.search.pop();
                        }
                        LogicalKey::Named(NamedKey::Enter) => self.osk.set_open(false),
                        LogicalKey::Named(NamedKey::Escape) => {
                            self.search.clear();
                            self.osk.set_open(false);
                        }
                        _ => {}
                    },
                    _ => {}
                }
            }
            if used {
                nav = 0;
                pressed = 0;
                self.hold_armed = false;
            }
        }
        // 1.000.028: a delete waiting for its answer has the pad: Cross deletes, Circle keeps.
        if let Some((gi, t)) = self.confirm_delete.as_mut() {
            *t += dt;
            let gi = *gi;
            self.hold_armed = false;
            if pressed & BTN_CROSS != 0 {
                self.confirm_delete = None;
                sounds::play(Sfx::Select);
                return Some(Action::Delete(gi));
            }
            if pressed & (BTN_CIRCLE | BTN_OPTIONS) != 0 {
                self.confirm_delete = None;
                sounds::play(Sfx::Back);
            }
            return None;
        }
        // Typing on a USB keyboard searches the shelves (and the Online tab).
        if tab.shelf().is_some() || tab == Tab::Online {
            for &c in pad.typed {
                self.type_char(c, badges);
            }
            if pad.backspace {
                self.search.pop();
            }
        }

        let mut action = None;
        if nav & (BTN_R1 | BTN_L1) != 0 {
            let n = tabs.len();
            self.tab = if nav & BTN_R1 != 0 { (self.tab + 1) % n } else { (self.tab + n - 1) % n };
            sounds::play(Sfx::Tab);
            self.shelf_anim = 0.0;
            self.stage_game = None;
        }
        let tab = tabs[self.tab];
        self.compute_visible(tab, library);
        match tab {
            Tab::Games | Tab::Favorites | Tab::Recent => {
                let shelf = tab.shelf().unwrap_or(0);
                let n = self.visible.len();
                if n > 0 {
                    let f = &mut self.focus[shelf];
                    *f = (*f).min(n - 1);
                    let before = *f;
                    if nav & BTN_RIGHT != 0 && *f + 1 < n {
                        *f += 1;
                    }
                    if nav & BTN_LEFT != 0 && *f > 0 {
                        *f -= 1;
                    }
                    if nav & BTN_DOWN != 0 {
                        *f = (*f + 4).min(n - 1);
                    }
                    if nav & BTN_UP != 0 {
                        *f = f.saturating_sub(4);
                    }
                    if *f != before {
                        sounds::play(Sfx::Move);
                    }
                    let gi = self.visible[*f];
                    if pressed & BTN_CROSS != 0 {
                        sounds::play(Sfx::Launch);
                        self.launch = Some((gi, 0.0));
                    } else if pressed & BTN_TRIANGLE != 0 {
                        sounds::play(Sfx::Select);
                        action = Some(Action::EditControls(gi));
                    } else if pressed & BTN_OPTIONS != 0 {
                        // 1.000.028: delete it (asked first)
                        sounds::play(Sfx::Select);
                        self.confirm_delete = Some((gi, 0.0));
                    } else if pressed & BTN_SQUARE != 0 {
                        let key = self.games[gi].key.clone();
                        let fav = library.toggle_favorite(&key);
                        sounds::play(Sfx::Toggle);
                        self.fav_pop = 0.0;
                        self.toast(if fav { "Added to Favorites" } else { "Removed from Favorites" });
                        badges.check(&self.live(library));
                    }
                }
                if pressed & BTN_L2 != 0 {
                    self.osk.set_open(true);
                    sounds::play(Sfx::Select);
                }
            }
            Tab::Badges => {
                let n = BADGES.len();
                let cols = 6;
                if nav & BTN_RIGHT != 0 && self.badge_focus + 1 < n {
                    self.badge_focus += 1;
                }
                if nav & BTN_LEFT != 0 && self.badge_focus > 0 {
                    self.badge_focus -= 1;
                }
                if nav & BTN_DOWN != 0 {
                    self.badge_focus = (self.badge_focus + cols).min(n - 1);
                }
                if nav & BTN_UP != 0 {
                    self.badge_focus = self.badge_focus.saturating_sub(cols);
                }
            }
            Tab::Settings => {
                action = self.update_settings(nav, pressed, settings, library);
            }
            Tab::Online => {
                self.online.update(dt, nav, pressed, &self.search, self.osk.is_open());
                if pressed & BTN_L2 != 0 {
                    self.osk.set_open(true);
                    sounds::play(Sfx::Select);
                }
            }
        }

        // Circle on the Online tab while a game is being fetched: stops that.
        if tab == Tab::Online && pressed & BTN_CIRCLE != 0 && self.online.busy() {
            self.online.cancel();
            sounds::play(Sfx::Back);
            self.hold_armed = false;
            pressed &= !BTN_CIRCLE;
        }
        // Circle: clears a search first; held, it closes the title.
        if pressed & BTN_CIRCLE != 0 && !self.search.is_empty() && (tab.shelf().is_some() || tab == Tab::Online) {
            self.search.clear();
            sounds::play(Sfx::Back);
            self.hold_armed = false;
        }
        let circle_free = !(tab == Tab::Settings && self.settings_page.in_rows);
        if pad.down & BTN_CIRCLE != 0 && circle_free {
            if self.hold_armed {
                self.hold += dt;
                if self.hold >= CLOSE_DELAY + CLOSE_HOLD {
                    action = Some(Action::Quit);
                }
            }
        } else {
            self.hold = 0.0;
            self.hold_armed = true;
        }

        // Motion.
        let focus_game = self.focused();
        if focus_game != self.stage_game {
            self.stage_game = focus_game;
            // the words and picture come in again (unless the menu's own entrance is still due)
            if self.stage_anim > 0.0 {
                self.stage_anim = 0.0;
            }
        }
        for (k, lift) in self.lifts.iter_mut().enumerate() {
            lift.target = if Some(k) == focus_game { 1.0 } else { 0.0 };
            lift.step(dt);
        }
        if let Some(shelf) = tab.shelf() {
            let pitch = TILE_W + TILE_GAP;
            let visible = RIGHT - MARGIN;
            let tile_x = self.focus[shelf].min(self.visible.len().saturating_sub(1)) as f32 * pitch;
            self.scroll.target = self.scroll.target.clamp((tile_x + TILE_W - visible).max(0.0), tile_x.max(0.0));
        }
        self.scroll.step(dt);
        self.badge_scroll.step(dt);
        // the settings' highlight glides to its row (and jumps when the section changes)
        if !self.settings_page.in_rows {
            self.settings_hl.snap(0.0);
        }
        self.settings_hl.target = self.settings_page.row as f32;
        self.settings_hl.step(dt);
        action
    }

    fn type_char(&mut self, c: char, badges: &mut Badges) {
        if self.search.is_empty() && !c.is_alphanumeric() {
            return;
        }
        if self.search.is_empty() {
            badges.stats.searches += 1;
        }
        if self.search.chars().count() < 32 {
            self.search.push(c);
        }
    }

    fn update_settings(&mut self, nav: u32, pressed: u32, settings: &mut Settings, library: &mut Library) -> Option<Action> {
        let page = &mut self.settings_page;
        if !page.in_rows {
            if nav & BTN_DOWN != 0 && page.cat + 1 < CATEGORIES.len() {
                page.cat += 1;
                sounds::play(Sfx::Move);
            }
            if nav & BTN_UP != 0 && page.cat > 0 {
                page.cat -= 1;
                sounds::play(Sfx::Move);
            }
            if pressed & (BTN_CROSS | BTN_RIGHT) != 0 {
                page.in_rows = true;
                page.row = 0;
                sounds::play(Sfx::Select);
            }
            return None;
        }
        let rows = CATEGORIES[page.cat].1;
        if pressed & BTN_CIRCLE != 0 {
            page.in_rows = false;
            sounds::play(Sfx::Back);
            self.hold_armed = false;
            return None;
        }
        if nav & BTN_DOWN != 0 && page.row + 1 < rows.len() {
            page.row += 1;
            sounds::play(Sfx::Move);
        }
        if nav & BTN_UP != 0 && page.row > 0 {
            page.row -= 1;
            sounds::play(Sfx::Move);
        }
        let row = rows[page.row.min(rows.len() - 1)];
        if let Some(act) = row.action() {
            if pressed & BTN_CROSS != 0 {
                sounds::play(Sfx::Select);
                return Some(match act {
                    Act::EditAllControls => Action::EditAllControls,
                    Act::EditKeyboard => Action::EditKeyboard,
                    Act::Rescan => Action::Rescan,
                    Act::ClearRecent => {
                        library.clear_recent();
                        self.toast("Recently played cleared");
                        Action::ClearRecent
                    }
                });
            }
            return None;
        }
        if row.is_info() {
            return None;
        }
        let dir = if nav & BTN_RIGHT != 0 {
            1
        } else if nav & BTN_LEFT != 0 {
            -1
        } else if pressed & BTN_CROSS != 0 {
            0
        } else {
            return None;
        };
        if dir == 0 && !matches!(row.value(settings).as_str(), "On" | "Off") && row != Row::Resolution {
            // Cross steps a list forward, wrapping.
            let before = settings.clone();
            if !settings.step(row, 1) {
                // at the end: back to the first value
                while settings.step(row, -1) {}
            }
            if *settings != before {
                sounds::play(Sfx::Toggle);
                return Some(Action::Setting(row));
            }
            return None;
        }
        if settings.step(row, dir) {
            sounds::play(Sfx::Toggle);
            return Some(Action::Setting(row));
        }
        None
    }

    /// Where the tabs sit, from their words.
    fn place_tabs(&mut self, ui: &Ui, tabs: &[Tab]) {
        self.tab_rects.clear();
        let mut x = TABS_START;
        for t in tabs {
            let w = ui.measure(Face::SemiBold, 22.0, t.name(), 0.0) + TAB_PAD;
            self.tab_rects.push(Rect::new(x, TOP_Y - TAB_H * 0.5, w, TAB_H));
            x += w + TAB_GAP;
        }
        let r = self.tab_rects[self.tab.min(self.tab_rects.len() - 1)];
        self.line_x.target = r.x;
        self.line_w.target = r.w;
        if !self.placed {
            self.line_x.snap(r.x);
            self.line_w.snap(r.w);
            self.placed = true;
        }
    }

    /// The menu over the background. `animated`: the logo animation is behind (else this draws a
    /// still backdrop of its own).
    #[allow(clippy::too_many_arguments)]
    pub fn draw(&mut self, ui: &mut Ui, scene: &mut Scene, dt: f32, settings: &Settings, library: &Library, badges: &mut Badges, animated: bool) {
        let tabs = self.tabs(settings);
        self.place_tabs(ui, &tabs);
        self.line_x.step(dt);
        self.line_w.step(dt);
        let tab = tabs[self.tab.min(tabs.len() - 1)];
        if !animated {
            self.draw_still_backdrop(scene);
        }
        let ui_alpha = ease(self.intro) * (1.0 - ease(self.saver));
        scene.push_opacity(ui_alpha);
        match tab {
            Tab::Games | Tab::Favorites | Tab::Recent => {
                if self.visible.is_empty() {
                    self.draw_empty(ui, scene, tab);
                } else {
                    self.draw_stage(ui, scene, library);
                    self.draw_shelf(ui, scene, tab, library);
                }
            }
            Tab::Badges => self.draw_badges(ui, scene, library, badges),
            Tab::Settings => self.draw_settings(ui, scene, settings),
            Tab::Online => self.online.draw(ui, scene),
        }
        self.draw_top_bar(ui, scene, &tabs);
        self.draw_hint_row(ui, scene, tab);
        if self.hold > CLOSE_DELAY {
            draw_hold_bar(ui, scene, self.hold, CLOSE_DELAY, CLOSE_HOLD, "Keep holding to close Ruffle5", Glyph::Circle, HOLD_BAR_Y);
        }
        self.osk.draw(ui, scene, "Search");
        if let Some((gi, t)) = self.confirm_delete {
            self.draw_confirm_delete(ui, scene, gi, t);
        }
        if let Some((msg, t)) = self.toast.clone() {
            toast(ui, scene, &msg, t, 2.4);
        }
        scene.pop_opacity();
        badges.draw_popup(ui, scene, dt);
        if let Some((_, t)) = self.launch {
            ribbon_wipe(scene, t / LAUNCH_TIME);
        } else if self.uncover < 0.45 {
            ribbon_wipe(scene, 1.0 - self.uncover / 0.45);
        }
    }

    /// 1.000.028: "Delete <game>?" over the menu, with what goes and what stays.
    fn draw_confirm_delete(&self, ui: &mut Ui, scene: &mut Scene, gi: usize, t: f32) {
        let Some(g) = self.games.get(gi) else {
            return;
        };
        let a = ease((t / 0.18).min(1.0));
        scene.push_opacity(a);
        scene.rect(Rect::new(0.0, 0.0, W, H), 0.0, NIGHT.with_alpha(0.62));
        let r = Rect::new((W - 1000.0) * 0.5, 330.0 - (1.0 - a) * 24.0, 1000.0, 380.0);
        card(scene, r, 28.0, 1.0);
        let inner = r.w - 112.0;
        let title = ui.fit(Face::Display, 40.0, &format!("Delete {}?", g.name), inner);
        scene.text(ui, Face::Display, 40.0, &title, r.x + 56.0, centred(r.y + 76.0, 40.0), INK, Align::Left, 0.0);
        let what = match &g.folder {
            Some(folder) => format!("Its folder goes: {}", short_path(folder, &self.games_dir)),
            None => format!("Its file goes: {}", short_path(&g.path, &self.games_dir)),
        };
        let what = ui.fit(Face::SemiBold, 24.0, &what, inner);
        scene.text(ui, Face::SemiBold, 24.0, &what, r.x + 56.0, centred(r.y + 150.0, 24.0), INK.with_alpha(0.9), Align::Left, 0.0);
        let kept = if g.online { "Its saves, cover and play time stay: fetch it again from Flash Browser and they are back." } else { "Its saves, cover and play time stay." };
        let kept = ui.fit(Face::Regular, 22.0, kept, inner);
        scene.text(ui, Face::Regular, 22.0, &kept, r.x + 56.0, centred(r.y + 196.0, 22.0), INK.with_alpha(0.7), Align::Left, 0.0);
        let mut x = r.x + 56.0;
        x += button(ui, scene, x, r.y + r.h - 108.0, 64.0, Glyph::Cross, "Delete", true, 0.6) + 16.0;
        button(ui, scene, x, r.y + r.h - 108.0, 64.0, Glyph::Circle, "Keep", false, 0.0);
        scene.pop_opacity();
    }

    /// Without the animation: Ruffle blue, lit from the top right, the mark large and faint.
    fn draw_still_backdrop(&self, scene: &mut Scene) {
        scene.gradient_v(Rect::new(0.0, 0.0, W, H), 0.0, BLUE_6, BLUE_9);
        let drift = (self.time * 0.25).sin() * 40.0;
        scene.shadow(Rect::new(1040.0 + drift, -220.0, 1000.0, 720.0), 360.0, 320.0, BLUE_4.with_alpha(0.35));
        scene.shadow(Rect::new(-260.0 - drift, 420.0, 820.0, 640.0), 320.0, 300.0, NIGHT.with_alpha(0.35));
        for (i, (x0, w, a)) in [(260.0f32, 180.0f32, 0.05f32), (900.0, 90.0, 0.04), (1500.0, 240.0, 0.035)].iter().enumerate() {
            let drift = ((self.time * 0.05 + i as f32 * 0.37).fract()) * 400.0 - 200.0;
            scene.ribbon(Rect::new(x0 + drift, -40.0, *w, H + 80.0), 0.0, LEAN, ORANGE.with_alpha(*a));
        }
        if let Some((id, (w, h))) = self.mark {
            let mh = 760.0;
            let mw = mh * w as f32 / h as f32;
            scene.image(id, Rect::new(W - mw - 40.0, 120.0, mw, mh), 0.0, WHITE.with_alpha(0.07));
        }
    }

    fn draw_top_bar(&mut self, ui: &mut Ui, scene: &mut Scene, tabs: &[Tab]) {
        // A band of the deep blue, so the bar reads over the animation.
        scene.gradient_v(Rect::new(0.0, 0.0, W, 190.0), 0.0, NIGHT.with_alpha(0.82), NIGHT.with_alpha(0.0));
        // Ruffle's mark, then the wordmark.
        let mut x = MARGIN;
        if let Some((id, (w, h))) = self.mark {
            let mh = 46.0;
            let mw = mh * w as f32 / h as f32;
            scene.image(id, Rect::new(x, TOP_Y - mh * 0.5, mw, mh), 0.0, WHITE);
            x += mw + 14.0;
        }
        let brand = scene.text(ui, Face::Display, 26.0, "RUFFLE5", x, centred(TOP_Y, 26.0), ORANGE, Align::Left, 4.0);
        x += brand + 22.0;
        scene.rect(Rect::new(x, TOP_Y - 11.0, 1.5, 22.0), 0.0, INK.with_alpha(0.25));
        scene.text(ui, Face::SemiBold, 20.0, "Flash on PS5", x + 20.0, centred(TOP_Y, 20.0), BLUE_2, Align::Left, 0.0);

        // The sections: words, the active one lit, one orange line gliding under it.
        let first = self.tab_rects[0];
        let l1_w = glyph_width(ui, Glyph::Chip("L1"), 26.0);
        draw_glyph(ui, scene, Glyph::Chip("L1"), first.x - l1_w - 8.0, TOP_Y, 26.0);
        let under = Rect::new(self.line_x.value, 0.0, self.line_w.value, 0.0);
        for (i, t) in tabs.iter().enumerate() {
            let r = self.tab_rects[i];
            let overlap = (under.x + under.w).min(r.x + r.w) - under.x.max(r.x);
            let on = (overlap / r.w).clamp(0.0, 1.0);
            scene.text(ui, Face::SemiBold, 22.0, t.name(), r.cx(), centred(TOP_Y, 22.0), Color::mix(INK.with_alpha(0.6), ORANGE, on), Align::Center, 0.0);
        }
        // Ruffle's ribbon, folded at its start, glides under the active section.
        ribbon_fold(scene, Rect::new(under.x + 18.0, TOP_Y + 19.0, (under.w - 36.0).max(10.0), 5.0), ORANGE, 1.0);
        let last = self.tab_rects[tabs.len() - 1];
        draw_glyph(ui, scene, Glyph::Chip("R1"), last.x + last.w + 8.0, TOP_Y, 26.0);

        // The right end: the search, or how many games there are.
        if !self.search.is_empty() || self.osk.is_open() {
            let q = if self.search.is_empty() { "type to search".to_string() } else { self.search.clone() };
            let label = ui.fit(Face::SemiBold, 22.0, &q, 280.0);
            let w = ui.measure(Face::SemiBold, 22.0, &label, 0.0) + 78.0;
            let r = Rect::new(RIGHT - w, TOP_Y - 22.0, w, 44.0);
            scene.rect(r, 22.0, NIGHT.with_alpha(0.9));
            scene.stroke(r, 22.0, 1.5, ORANGE.with_alpha(0.6));
            scene.ring(r.x + 26.0, TOP_Y - 2.0, 8.0, 2.5, ORANGE);
            scene.rect_rotated(Rect::new(r.x + 30.0, TOP_Y + 6.0, 9.0, 3.0), 1.5, 0.785, ORANGE);
            let ink = if self.search.is_empty() { INK.with_alpha(0.5) } else { INK };
            scene.text(ui, Face::SemiBold, 22.0, &label, r.x + 48.0, centred(TOP_Y, 22.0), ink, Align::Left, 0.0);
        } else {
            let count = match self.games.len() {
                0 => "no games".to_string(),
                1 => "1 game".to_string(),
                n => format!("{n} games"),
            };
            scene.text(ui, Face::SemiBold, 20.0, &count, RIGHT, centred(TOP_Y, 20.0), BLUE_2, Align::Right, 0.0);
        }
    }

    /// The stage: the focused game, big, its picture floating on the right.
    fn draw_stage(&mut self, ui: &mut Ui, scene: &mut Scene, library: &Library) {
        let Some(gi) = self.focused() else {
            return;
        };
        let game = &self.games[gi];
        let (name, file, size, info, thumb, accent, key, stem, mapping) =
            (game.name.clone(), game.file.clone(), game.size, game.info.clone(), game.thumb, game.accent, game.key.clone(), game.stem.clone(), game.mapping);
        let (kind, director) = (game.kind, game.director.clone());
        let rec = library.get(&key);

        // The picture: large, floating a little over its own colours; it settles in when the
        // focused game changes (fades up and grows from 96%).
        let come = ease(self.stage_anim / 0.35);
        let bob = 7.0 * (self.time * 0.9).sin();
        let cover = Rect::new(1220.0, 232.0 + bob, 420.0, 360.0).scaled(0.96 + 0.04 * come);
        scene.push_opacity(come);
        focus_halo(scene, cover, 56.0, 1.0, self.time, Color::mix(accent, ORANGE, 0.45));
        scene.shadow(Rect::new(cover.x + 60.0, 610.0, cover.w - 120.0, 18.0), 9.0, 30.0 - bob, BLACK.with_alpha(0.5));
        scene.shadow(Rect::new(cover.x, cover.y + 30.0, cover.w, cover.h), 40.0, 70.0, BLACK.with_alpha(0.45));
        match thumb {
            Some(id) => {
                let size = ui.image_size(id);
                scene.image_cover(id, size, cover, 56.0, WHITE);
            }
            None => {
                scene.gradient_v(cover, 56.0, Color::mix(accent, BLUE_6, 0.35), Color::mix(accent, NIGHT, 0.6));
                scene.text(ui, Face::Display, 150.0, &initials(&name), cover.cx(), cover.cy() + 54.0, INK.with_alpha(0.92), Align::Center, 2.0);
            }
        }
        scene.gradient_v(Rect::new(cover.x, cover.y, cover.w, cover.h * 0.46), 56.0, WHITE.with_alpha(0.1), WHITE.with_alpha(0.0));
        // A slow sheen across the picture now and then.
        let sweep = (self.time / 6.0).fract();
        if sweep < 0.25 {
            let u = sweep / 0.25;
            let band = Rect::new(cover.x - 160.0 + (cover.w + 320.0) * u, cover.y, 120.0, cover.h);
            scene.push_clip(cover);
            scene.rect_rotated(band, 0.0, 0.35, WHITE.with_alpha(0.07 * (1.0 - (2.0 * u - 1.0).abs())));
            scene.pop_clip();
        }
        focus_ring(scene, cover, 56.0, 0.85, self.time + 1.6);
        if rec.favorite {
            let pop = back_out(self.fav_pop / 0.4);
            let star = Rect::new(cover.right() - 74.0, cover.y + 18.0, 56.0, 56.0);
            scene.circle(star.cx(), star.cy(), 28.0 * pop, NIGHT.with_alpha(0.85));
            scene.text(ui, Face::Regular, 30.0 * pop.max(0.01), "\u{2605}", star.cx(), centred(star.cy(), 30.0 * pop), ORANGE, Align::Center, 0.0);
            sparks(scene, star.cx(), star.cy(), self.fav_pop, hash(&key));
        }
        scene.pop_opacity();

        // The words sit on the deep blue: from the left, and under the shelf.
        scene.gradient_h(Rect::new(0.0, 150.0, 1300.0, 560.0), 0.0, NIGHT.with_alpha(0.86), NIGHT.with_alpha(0.0));
        scene.gradient_v(Rect::new(0.0, 560.0, W, 140.0), 0.0, NIGHT.with_alpha(0.0), NIGHT.with_alpha(0.86));
        scene.rect(Rect::new(0.0, 700.0, W, H - 700.0), 0.0, NIGHT.with_alpha(0.86));

        // The words follow the picture in, sliding from the left.
        scene.push_opacity(come);
        let mx = MARGIN - 28.0 * (1.0 - come);
        ribbon_fold(scene, Rect::new(mx + 2.0, STAGE_KICKER_Y - 12.0, 28.0 * come.max(0.2), 6.0), ORANGE, 1.0);
        let kicker = match (kind, rec.favorite) {
            (Kind::Flash, false) => "FLASH GAME",
            (Kind::Flash, true) => "FAVORITE FLASH GAME",
            (Kind::Shockwave, false) => "SHOCKWAVE GAME",
            (Kind::Shockwave, true) => "FAVORITE SHOCKWAVE GAME",
        };
        scene.text(ui, Face::SemiBold, 18.0, kicker, mx + 40.0, STAGE_KICKER_Y, ORANGE, Align::Left, 4.0);
        let headline = ui.fit(Face::Display, 88.0, &name, 1080.0);
        scene.text(ui, Face::Display, 88.0, &headline, mx - 4.0, STAGE_TITLE_Y, INK, Align::Left, 0.0);
        let mut meta = vec![file.clone(), size_text(size)];
        if let Some(d) = &director {
            meta.push(d.clone());
        }
        if let Some(i) = &info {
            meta.push(format!("SWF {}", i.version));
            if i.width > 0 && i.height > 0 {
                meta.push(format!("{} \u{d7} {}", i.width, i.height));
            }
            if i.frame_rate > 0.0 {
                meta.push(format!("{} fps", trim_float(i.frame_rate)));
            }
        }
        let meta = ui.fit(Face::Regular, 25.0, &meta.join("  \u{b7}  "), 1080.0);
        scene.text(ui, Face::Regular, 25.0, &meta, mx, STAGE_META_Y, BLUE_2, Align::Left, 0.0);
        let played = if rec.launches == 0 {
            "Not played yet.".to_string()
        } else {
            format!("Played {} \u{b7} last {}", duration_text(rec.play_secs), ago_text(rec.last_played, now_secs()))
        };
        scene.text(ui, Face::Regular, 25.0, &played, mx, STAGE_TEXT_Y, INK.with_alpha(0.86), Align::Left, 0.0);
        let mapping_note = if mapping {
            format!("Controls: its own, Custom Mappings/{stem}.controls.txt")
        } else {
            "Controls: the ones for all games (Triangle to give it its own).".to_string()
        };
        let mapping_note = ui.fit(Face::Regular, 25.0, &mapping_note, 1080.0);
        scene.text(ui, Face::Regular, 25.0, &mapping_note, mx, STAGE_TEXT_Y + 38.0, INK.with_alpha(0.86), Align::Left, 0.0);

        // The calls to action.
        let breath = breathe(self.time, 2.4);
        let mut x = mx;
        x += button(ui, scene, x, STAGE_BUTTONS_Y, 64.0, Glyph::Cross, "Play", true, 0.6 + 0.4 * breath) + 16.0;
        x += button(ui, scene, x, STAGE_BUTTONS_Y, 64.0, Glyph::Triangle, "Controls", false, 0.0) + 16.0;
        button(ui, scene, x, STAGE_BUTTONS_Y, 64.0, Glyph::Square, if rec.favorite { "Unfavorite" } else { "Favorite" }, false, 0.0);
        scene.pop_opacity();
    }

    fn draw_shelf(&mut self, ui: &mut Ui, scene: &mut Scene, tab: Tab, library: &Library) {
        let head = scene.text(ui, Face::SemiBold, 30.0, tab.name(), MARGIN, SHELF_HEAD_Y, INK, Align::Left, 0.0);
        let count = if self.search.is_empty() { self.visible.len().to_string() } else { format!("{} matching \u{201c}{}\u{201d}", self.visible.len(), self.search) };
        scene.text(ui, Face::Regular, 18.0, &count, MARGIN + head + 12.0, SHELF_HEAD_Y, BLUE_2, Align::Left, 0.0);
        let pitch = TILE_W + TILE_GAP;
        let scroll = self.scroll.value;
        let shelf = tab.shelf().unwrap_or(0);
        let focus = self.focus[shelf].min(self.visible.len().saturating_sub(1));
        // Resting tiles first, the focused one last, so its lift overlaps its neighbours.
        let order: Vec<usize> = (0..self.visible.len()).filter(|&k| k != focus).chain(std::iter::once(focus)).collect();
        for k in order {
            // Tiles rise into place one after another when the shelf appears.
            let screen_k = (k as f32 - scroll / pitch).max(0.0);
            let enter = ease((self.shelf_anim - 0.05 * screen_k) / 0.32);
            if enter <= 0.0 {
                continue;
            }
            let r = Rect::new(MARGIN + k as f32 * pitch - scroll, SHELF_TILES_Y + 36.0 * (1.0 - enter), TILE_W, TILE_H);
            if r.x > W || r.x + r.w < 0.0 {
                continue;
            }
            let gi = self.visible[k];
            let lift = self.lifts.get(gi).map(|l| l.value).unwrap_or(0.0);
            // ProsperoStore's focused card: larger, and risen a little; and, Ruffle's own, it ruffles
            // as it is chosen (the logo's letters do that when a pointer passes them).
            let wobble = if Some(gi) == self.stage_game { ruffle(self.stage_anim) } else { 0.0 };
            let rs = r.scaled(1.0 + LIFT * lift + wobble);
            let rs = Rect::new(rs.x, rs.y - 8.0 * lift, rs.w, rs.h);
            let game = &self.games[gi];
            let light = 0.74 + 0.26 * lift;
            scene.push_opacity(enter);
            focus_halo(scene, rs, TILE_RADIUS, lift, self.time, Color::mix(game.accent, ORANGE, 0.5));
            scene.gradient_v(rs, TILE_RADIUS, Color::mix(game.accent, BLUE_6, 0.35).with_alpha(0.95), Color::mix(game.accent, NIGHT, 0.86));
            match game.thumb {
                Some(id) => {
                    let size = ui.image_size(id);
                    scene.image_cover(id, size, rs, TILE_RADIUS, Color { r: light, g: light, b: light, a: 1.0 });
                    scene.gradient_v(Rect::new(rs.x, rs.y + rs.h * 0.45, rs.w, rs.h * 0.55), TILE_RADIUS, BLACK.with_alpha(0.0), NIGHT.with_alpha(0.85));
                }
                None => {
                    let s = rs.h * 0.48;
                    let icon = Rect::new(rs.cx() - s * 0.5, rs.y + rs.h * 0.4 - s * 0.5, s, s);
                    scene.shadow(icon, s * 0.3, 60.0, Color::mix(game.accent, ORANGE, 0.3).with_alpha(0.3));
                    scene.gradient_v(icon, s * 0.22, Color::mix(game.accent, BLUE_6, 0.3), Color::mix(game.accent, NIGHT, 0.55));
                    scene.stroke(icon, s * 0.22, 1.0, INK.with_alpha(0.2));
                    scene.text(ui, Face::Display, s * 0.46, &initials(&game.name), icon.cx(), icon.cy() + s * 0.17, INK.with_alpha(0.92), Align::Center, 1.0);
                }
            }
            let name = ui.fit(Face::SemiBold, 24.0, &game.name, rs.w - 40.0);
            scene.text(ui, Face::SemiBold, 24.0, &name, rs.x + 20.0, rs.y + rs.h - 42.0, INK, Align::Left, 0.0);
            if lift > 0.01 {
                let sweep = ease(self.stage_anim / 0.4) * lift;
                let nw = ui.measure(Face::SemiBold, 24.0, &name, 0.0).min(rs.w - 40.0);
                ribbon_fold(scene, Rect::new(rs.x + 22.0, rs.y + rs.h - 34.0, (nw * sweep).max(1.0), 4.0), ORANGE, lift);
            }
            let rec = library.get(&game.key);
            let mut sub = size_text(game.size);
            if let Some(i) = &game.info {
                sub = format!("{sub}  \u{b7}  SWF {}", i.version);
            }
            if game.kind == Kind::Shockwave {
                sub = format!("{sub}  \u{b7}  Shockwave");
            }
            if rec.play_secs > 0 {
                sub = format!("{sub}  \u{b7}  {}", duration_text(rec.play_secs));
            }
            let sub = ui.fit(Face::Regular, 18.0, &sub, rs.w - 40.0);
            scene.text(ui, Face::Regular, 18.0, &sub, rs.x + 20.0, rs.y + rs.h - 16.0, BLUE_2, Align::Left, 0.0);
            if rec.favorite {
                scene.text(ui, Face::Regular, 24.0, "\u{2605}", rs.right() - 22.0, rs.y + 36.0, ORANGE, Align::Right, 0.0);
            }
            scene.stroke(rs, TILE_RADIUS, 1.5, INK.with_alpha(0.1));
            focus_ring(scene, rs, TILE_RADIUS, lift, self.time);
            scene.pop_opacity();
        }
    }

    fn draw_empty(&mut self, ui: &mut Ui, scene: &mut Scene, tab: Tab) {
        scene.gradient_h(Rect::new(0.0, 150.0, 1400.0, 700.0), 0.0, NIGHT.with_alpha(0.86), NIGHT.with_alpha(0.0));
        ribbon_fold(scene, Rect::new(MARGIN + 2.0, STAGE_KICKER_Y - 12.0, 28.0, 6.0), ORANGE, 1.0);
        let (kicker, title, lines): (&str, &str, Vec<String>) = if !self.search.is_empty() {
            ("NO MATCHES", "Nothing found", vec![format!("No game is called \u{201c}{}\u{201d}.", self.search), "Circle clears the search.".to_string()])
        } else {
            match tab {
                Tab::Favorites => ("NO FAVORITES YET", "Favorites", vec!["Square on a game in Games makes it a favorite.".to_string()]),
                Tab::Recent => ("NOTHING PLAYED YET", "Recent", vec!["The games you play show up here, the latest first.".to_string()]),
                _ => (
                    "NOTHING TO PLAY YET",
                    "No games",
                    vec![
                        format!("Put .swf, .dcr or game folders in {} (FTP) and open Ruffle5 again,", self.games_dir),
                        "or in a ruffle5 folder on a USB drive. A game can sit in its own folder, with a cover.png.".to_string(),
                    ],
                ),
            }
        };
        scene.text(ui, Face::SemiBold, 18.0, kicker, MARGIN + 40.0, STAGE_KICKER_Y, ORANGE, Align::Left, 4.0);
        scene.text(ui, Face::Display, 88.0, title, MARGIN - 4.0, STAGE_TITLE_Y, INK, Align::Left, 0.0);
        for (i, line) in lines.iter().enumerate() {
            let line = ui.fit(Face::Regular, 25.0, line, 1500.0);
            scene.text(ui, Face::Regular, 25.0, &line, MARGIN, STAGE_META_Y + 40.0 * i as f32, INK.with_alpha(0.86), Align::Left, 0.0);
        }
        if tab == Tab::Games && self.search.is_empty() {
            for (i, line) in self.report.iter().take(3).enumerate() {
                let line = ui.fit(Face::Regular, 20.0, line, 1500.0);
                scene.text(ui, Face::Regular, 20.0, &line, MARGIN, STAGE_TEXT_Y + 60.0 + 30.0 * i as f32, BLUE_2.with_alpha(0.7), Align::Left, 0.0);
            }
        }
    }

    fn draw_settings(&mut self, ui: &mut Ui, scene: &mut Scene, settings: &Settings) {
        scene.rect(Rect::new(0.0, 0.0, W, H), 0.0, NIGHT.with_alpha(0.55));
        scene.text(ui, Face::Display, 56.0, "Settings", MARGIN, 200.0, INK, Align::Left, 0.0);
        let page = &self.settings_page;
        // the sections
        let cats = Rect::new(MARGIN, 248.0, 420.0, 700.0);
        card(scene, cats, 22.0, 1.0);
        for (i, (name, _)) in CATEGORIES.iter().enumerate() {
            let r = Rect::new(cats.x + 12.0, cats.y + 12.0 + 64.0 * i as f32, cats.w - 24.0, 58.0);
            let on = i == page.cat;
            if on {
                scene.rect(r, 14.0, if page.in_rows { WHITE.with_alpha(0.06) } else { WHITE.with_alpha(0.12) });
                scene.rect(Rect::new(r.x + 4.0, r.y + 14.0, 5.0, r.h - 28.0), 2.5, ORANGE);
            }
            scene.text(ui, Face::SemiBold, 25.0, name, r.x + 26.0, centred(r.cy(), 25.0), if on { ORANGE } else { INK.with_alpha(0.8) }, Align::Left, 0.0);
            if on && !page.in_rows {
                scene.text(ui, Face::SemiBold, 24.0, "\u{203a}", r.right() - 20.0, centred(r.cy(), 24.0), ORANGE, Align::Right, 0.0);
            }
        }
        // the rows
        let (cat_name, rows) = CATEGORIES[page.cat];
        let panel = Rect::new(548.0, 248.0, W - MARGIN - 548.0, 560.0);
        card(scene, panel, 22.0, 1.0);
        let row_h = 64.0;
        if page.in_rows {
            let hl = Rect::new(panel.x + 12.0, panel.y + 12.0 + row_h * self.settings_hl.value, panel.w - 24.0, row_h - 6.0);
            scene.rect(hl, 14.0, WHITE.with_alpha(0.1));
            scene.rect(Rect::new(hl.x + 4.0, hl.y + 14.0, 5.0, hl.h - 28.0), 2.5, ORANGE);
        }
        for (i, row) in rows.iter().enumerate() {
            let r = Rect::new(panel.x + 12.0, panel.y + 12.0 + row_h * i as f32, panel.w - 24.0, row_h - 6.0);
            let focused = page.in_rows && i == page.row;
            // (the focused row's highlight glides, drawn above; no divider touches it)
            if !focused && i > 0 && !(page.in_rows && i == page.row + 1) {
                scene.rect(Rect::new(r.x + 20.0, r.y - 3.0, r.w - 40.0, 1.0), 0.0, INK.with_alpha(0.07));
            }
            scene.text(ui, Face::SemiBold, 24.0, row.label(), r.x + 26.0, centred(r.cy(), 24.0), INK, Align::Left, 0.0);
            let value = row.value(settings);
            let vx = r.right() - 26.0;
            if row.action().is_some() {
                let w = ui.measure(Face::SemiBold, 20.0, &value, 0.0) + 36.0;
                let pill = Rect::new(vx - w, r.cy() - 19.0, w, 38.0);
                scene.rect(pill, 19.0, if focused { ORANGE } else { BLUE_7 });
                scene.text(ui, Face::SemiBold, 20.0, &value, pill.cx(), centred(pill.cy(), 20.0), if focused { ON_ORANGE } else { WHITE }, Align::Center, 0.0);
            } else {
                let ink = if row.is_info() { BLUE_2 } else if focused { ORANGE } else { INK.with_alpha(0.75) };
                let value = ui.fit(Face::Regular, 24.0, &value, 560.0);
                let w = scene.text(ui, Face::Regular, 24.0, &value, vx - if focused && !row.is_info() { 34.0 } else { 0.0 }, centred(r.cy(), 24.0), ink, Align::Right, 0.0);
                if focused && !row.is_info() {
                    scene.text(ui, Face::Regular, 18.0, "\u{25b6}", vx, centred(r.cy(), 18.0), ORANGE, Align::Right, 0.0);
                    scene.text(ui, Face::Regular, 18.0, "\u{25c0}", vx - 34.0 - w - 14.0, centred(r.cy(), 18.0), ORANGE, Align::Right, 0.0);
                }
            }
        }
        // the help
        let help = Rect::new(panel.x, panel.bottom() + 20.0, panel.w, 120.0);
        card(scene, help, 22.0, 1.0);
        let text = if page.in_rows { rows[page.row.min(rows.len() - 1)].help(settings) } else { settings::category_help(cat_name).to_string() };
        for (i, line) in ui.wrap(Face::Regular, 22.0, &text, help.w - 64.0).iter().take(3).enumerate() {
            scene.text(ui, Face::Regular, 22.0, line, help.x + 32.0, help.y + 44.0 + 30.0 * i as f32, BLUE_2, Align::Left, 0.0);
        }
    }

    fn draw_badges(&mut self, ui: &mut Ui, scene: &mut Scene, library: &Library, badges: &Badges) {
        scene.rect(Rect::new(0.0, 0.0, W, H), 0.0, NIGHT.with_alpha(0.55));
        scene.text(ui, Face::Display, 56.0, "Badges", MARGIN, 200.0, INK, Align::Left, 0.0);
        let count = format!("{} of {} unlocked", badges.count(), BADGES.len());
        scene.text(ui, Face::SemiBold, 22.0, &count, RIGHT, 200.0, ORANGE, Align::Right, 0.0);
        let live = self.live(library);
        let cols = 6;
        let (cw, ch) = (275.0, 196.0);
        let area = Rect::new(MARGIN - 8.0, 236.0, W - 2.0 * MARGIN + 16.0, 600.0);
        let row = (self.badge_focus / cols) as f32;
        let visible_rows = (area.h / ch).floor();
        if row < self.badge_scroll.target {
            self.badge_scroll.target = row;
        } else if row > self.badge_scroll.target + visible_rows - 1.0 {
            self.badge_scroll.target = row - visible_rows + 1.0;
        }
        scene.push_clip(area);
        for (i, b) in BADGES.iter().enumerate() {
            let (c, rr) = ((i % cols) as f32, (i / cols) as f32);
            let r = Rect::new(MARGIN + c * (cw + 12.0), area.y + 8.0 + (rr - self.badge_scroll.value) * ch, cw, ch - 16.0);
            if r.bottom() < area.y || r.y > area.bottom() {
                continue;
            }
            let focused = i == self.badge_focus;
            let unlocked = badges.unlocked_at(i).is_some();
            if focused {
                focus_halo(scene, r, 18.0, 1.0, self.time, ORANGE);
            }
            scene.rect(r, 18.0, if focused { Color::mix(NIGHT, BLUE_7, 0.6) } else { NIGHT.with_alpha(0.7) });
            // an unlocked medal turns slowly toward you now and then
            let spin = if unlocked { 1.0 - 0.18 * breathe(self.time + i as f32 * 0.7, 3.0) } else { 1.0 };
            medal(ui, scene, r.cx(), r.y + 58.0, 38.0 * spin.max(0.5), unlocked, &badges::symbol(i));
            if focused {
                focus_ring(scene, r, 18.0, 1.0, self.time);
            }
            let name = ui.fit(Face::SemiBold, 20.0, b.name, r.w - 24.0);
            scene.text(ui, Face::SemiBold, 20.0, &name, r.cx(), r.y + 128.0, if unlocked { INK } else { INK.with_alpha(0.55) }, Align::Center, 0.0);
            let (now, goal) = badges.progress(i, &live);
            let track = Rect::new(r.x + 30.0, r.y + 148.0, r.w - 60.0, 6.0);
            scene.rect(track, 3.0, WHITE.with_alpha(0.12));
            let f = if goal == 0 { 1.0 } else { now as f32 / goal as f32 };
            scene.rect(Rect::new(track.x, track.y, (track.w * f).max(6.0), track.h), 3.0, if unlocked { ORANGE } else { BLUE_4 });
        }
        scene.pop_clip();
        // the focused badge, told in full
        let b = &BADGES[self.badge_focus];
        let (now, goal) = badges.progress(self.badge_focus, &live);
        let info = Rect::new(MARGIN, 852.0, W - 2.0 * MARGIN, 108.0);
        card(scene, info, 22.0, 1.0);
        scene.text(ui, Face::SemiBold, 28.0, b.name, info.x + 32.0, info.y + 46.0, ORANGE, Align::Left, 0.0);
        scene.text(ui, Face::Regular, 22.0, b.about, info.x + 32.0, info.y + 84.0, INK, Align::Left, 0.0);
        let status = match badges.unlocked_at(self.badge_focus) {
            Some(_) => "Unlocked".to_string(),
            None => format!("{now} / {goal}"),
        };
        scene.text(ui, Face::SemiBold, 24.0, &status, info.right() - 32.0, centred(info.cy(), 24.0), BLUE_2, Align::Right, 0.0);
    }

    fn draw_hint_row(&mut self, ui: &mut Ui, scene: &mut Scene, tab: Tab) {
        let mut hints: Vec<(Glyph, &str)> = Vec::new();
        match tab {
            Tab::Games | Tab::Favorites | Tab::Recent => {
                if !self.visible.is_empty() {
                    hints.push((Glyph::Cross, "Play"));
                    hints.push((Glyph::Triangle, "Controls"));
                    hints.push((Glyph::Square, "Favorite"));
                    hints.push((Glyph::Chip("OPTIONS"), "Delete"));
                }
                hints.push((Glyph::Chip("L2"), "Search"));
            }
            Tab::Settings => {
                if self.settings_page.in_rows {
                    hints.push((Glyph::DPad(4), "Change"));
                    hints.push((Glyph::Circle, "Back"));
                } else {
                    hints.push((Glyph::Cross, "Open"));
                }
            }
            Tab::Badges => hints.push((Glyph::DPad(4), "Browse")),
            Tab::Online => hints.extend(self.online.hints()),
        }
        hints.push((Glyph::Chip("L1"), ""));
        hints.push((Glyph::Chip("R1"), "Sections"));
        if tab == Tab::Online && self.online.busy() {
            hints.push((Glyph::Circle, "Cancel"));
        } else if !(tab == Tab::Settings && self.settings_page.in_rows) {
            hints.push((Glyph::Circle, if self.search.is_empty() { "Close (hold)" } else { "Clear search" }));
        }
        draw_hints(ui, scene, &hints, RIGHT, HINTS_Y);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn director_folders_are_found_one_level_down_and_skips_are_logged() {
        let root = std::env::temp_dir().join(format!("ruffle5-menu-scan-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        let games = root.join("games");
        let mk = |rel: &str, body: &[u8]| {
            let p = games.join(rel);
            std::fs::create_dir_all(p.parent().unwrap()).unwrap();
            std::fs::write(p, body).unwrap();
        };
        // unzipped into a folder of the same name: the game is one level down
        mk("ttbattleblitz/ttbattleblitz/gamewrapper.dir", b"XFIR\0\0\0\039VM");
        mk("ttbattleblitz/ttbattleblitz/game.dcr", b"XFIR\0\0\0\0MDGF");
        mk("ttbattleblitz/ttbattleblitz/Projector.ini", b"[Movies]\r\nMovie01=gamewrapper.dir\r\n");
        // the usual layout
        mk("flat/game.dcr", b"XFIR\0\0\0\0MDGF");
        // nothing playable
        mk("notes/readme.txt", b"hi");
        // 1.000.032: a Director game with a Flash file of its own (Paul Blart: Mall Cop's layout)
        mk("Paul Blart Mall Cop/advert_loader.swf", b"FWS\x08\x15\0\0\0\x30\0\x01\x00\x0c\x01\x00");
        mk("Paul Blart Mall Cop/gameloader.dcr", b"XFIR\0\0\0\0MDGF");
        mk("Paul Blart Mall Cop/paul_blart.dcr", b"XFIR\0\0\0\0MDGF");
        mk("Paul Blart Mall Cop/mallcop.exe", b"MZ");
        let p = |s: &str| {
            let d = root.join(s);
            std::fs::create_dir_all(&d).unwrap();
            d.to_string_lossy().into_owned()
        };
        let paths = Paths::new(&p("data"), &p("games"), &p("maps"));
        let menu = Menu::new(&paths);
        let names: Vec<&str> = menu.games.iter().map(|g| g.name.as_str()).collect();
        assert_eq!(names, ["Flat", "Paul Blart Mall Cop", "Ttbattleblitz"], "{:?}", menu.report);
        let pb = menu.games.iter().find(|g| g.name == "Paul Blart Mall Cop").unwrap();
        assert!(pb.path.ends_with("Paul Blart Mall Cop/gameloader.dcr"), "the Director movie, not the advert loader: {}", pb.path);
        assert!(menu.report.iter().any(|l| l.contains("gameloader.dcr chosen over advert_loader.swf")), "{:?}", menu.report);
        let tt = menu.games.iter().find(|g| g.name == "Ttbattleblitz").unwrap();
        assert!(tt.path.ends_with("ttbattleblitz/ttbattleblitz/gamewrapper.dir"), "{}", tt.path);
        assert_eq!(crate::controls::movie_stem(&tt.controls_path), "ttbattleblitz");
        assert!(menu.report.iter().any(|l| l.contains("one folder down")), "{:?}", menu.report);
        assert!(menu.report.iter().any(|l| l.contains("notes/ not listed") && l.contains("readme.txt")), "{:?}", menu.report);
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn delete_takes_the_game_and_leaves_the_rest() {
        let root = std::env::temp_dir().join(format!("ruffle5-menu-delete-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        let games = root.join("games");
        let mk = |rel: &str, body: &[u8]| {
            let p = games.join(rel);
            std::fs::create_dir_all(p.parent().unwrap()).unwrap();
            std::fs::write(p, body).unwrap();
        };
        let swf = b"FWS\x08\x15\0\0\0\x30\0\x01\x00\x0c\x01\x00";
        mk("Loose.swf", swf);
        mk("Loose.png", b"png");
        mk("Other.swf", swf);
        mk("Fetched/Fetched Game.swf", swf);
        mk("Fetched/online.txt", b"site = archive.org\nid = fetched\ntitle = Fetched Game\nmovie = Fetched Game.swf\n");
        mk("Unzipped/Unzipped/game.dcr", b"XFIR\0\0\0\0MDGF");
        let p = |s: &str| {
            let d = root.join(s);
            std::fs::create_dir_all(&d).unwrap();
            d.to_string_lossy().into_owned()
        };
        let paths = Paths::new(&p("data"), &p("games"), &p("maps"));
        let saves = paths.saves("Fetched_Game");
        std::fs::create_dir_all(&saves).unwrap();
        let mut menu = Menu::new(&paths);
        let find = |m: &Menu, name: &str| m.games.iter().position(|g| g.name == name).unwrap_or_else(|| panic!("{name} not in {:?}", m.games.iter().map(|g| &g.name).collect::<Vec<_>>()));

        let f = find(&menu, "Fetched Game");
        assert!(menu.games[f].online);
        assert_eq!(menu.games[f].folder.as_deref(), Some(games.join("Fetched").to_str().unwrap()));
        menu.delete_game(f).unwrap();
        assert!(!games.join("Fetched").exists());
        assert!(std::path::Path::new(&saves).is_dir(), "its saves stay");

        menu.scan(&paths);
        let u = find(&menu, "Unzipped");
        assert!(!menu.games[u].online);
        // the folder in the games folder, not the one inside it
        assert_eq!(menu.games[u].folder.as_deref(), Some(games.join("Unzipped").to_str().unwrap()));
        menu.delete_game(u).unwrap();
        assert!(!games.join("Unzipped").exists());

        menu.scan(&paths);
        let l = find(&menu, "Loose");
        assert_eq!(menu.games[l].folder, None);
        let (gone, trashed) = menu.delete_game(l).unwrap();
        assert!(gone.contains("Loose.swf") && gone.contains("Loose.png") && !trashed, "{gone}");
        assert!(games.join("Other.swf").is_file() && games.is_dir());

        menu.scan(&paths);
        assert_eq!(menu.games.iter().map(|g| g.name.as_str()).collect::<Vec<_>>(), ["Other"]);
        // what waits in the trash folder (a removal the console refused) is not listed
        std::fs::create_dir_all(games.join(crate::fsutil::TRASH).join("Fetched")).unwrap();
        std::fs::write(games.join(crate::fsutil::TRASH).join("Fetched/game.swf"), b"FWS").unwrap();
        menu.scan(&paths);
        assert_eq!(menu.games.iter().map(|g| g.name.as_str()).collect::<Vec<_>>(), ["Other"]);
        assert!(!menu.report.iter().any(|l| l.contains(crate::fsutil::TRASH)), "{:?}", menu.report);
        let _ = std::fs::remove_dir_all(games.join(crate::fsutil::TRASH));
        // never the games folder itself
        menu.games[0].folder = Some(games.to_string_lossy().into_owned());
        assert!(menu.delete_game(0).is_err());
        assert!(games.join("Other.swf").is_file());
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn paths_are_shown_short() {
        assert_eq!(short_path("/data/homebrew/PPSA99620/games/Ab/c.swf", "/data/homebrew/PPSA99620/games"), "games/Ab/c.swf");
        assert_eq!(short_path("/mnt/usb0/ruffle5/x.dcr", "/data/g"), "USB: ruffle5/x.dcr");
    }
}
