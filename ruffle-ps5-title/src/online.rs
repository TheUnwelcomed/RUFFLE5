//! PS5_RUFFLE_ONLINE_V1 — the menu's Online tab (1.000.027): Flash games and movies from the
//! Internet Archive's Flash collection and from Flash Museum, listed with their pictures; Cross
//! fetches the chosen one into the games folder and plays it, Square only fetches it.
//!
//! Network work runs on two threads of its own (r5net: HTTPS on the console's sockets, rustls with
//! pure-Rust cryptography), so the menu never waits: "online" asks for pages of results and fetches
//! games, one at a time; "pictures" fetches and decodes the tiles' pictures, a few at a time, the
//! ones nearest the focused tile first. Nothing touches the network until the tab is opened.
//!
//! A fetched game is an ordinary game folder in the games folder: `<title>/` with the movie (and
//! the files beside it the item has, for movies that load them), `cover.png` from its picture, and
//! `online.txt` — where it came from and which file is the movie, so the menu plays that one and
//! shows the title as the source spells it (src/menu.rs). Deleting the folder removes the game.
// SPDX-License-Identifier: GPL-3.0-or-later

use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, Receiver, Sender};
use std::sync::Arc;

use r5net::{Client, Files, Item, Request, Source, SOURCES};

use crate::input::{BTN_CROSS, BTN_DOWN, BTN_LEFT, BTN_R2, BTN_RIGHT, BTN_SQUARE, BTN_TRIANGLE, BTN_UP};
use crate::sounds::{self, Sfx};
use crate::ui::{Align, Color, Face, ImageId, Rect, Scene, Ui, W};
use crate::widgets::*;

/// Pictures kept on the GPU at once (the least recently shown give their slot to new ones).
const MAX_PICTURES: usize = 72;
/// Pictures being fetched at once.
const PICTURES_IN_FLIGHT: usize = 3;
/// A tile's picture, longest side.
const PICTURE_SIZE: u32 = 512;
/// The most a movie may weigh.
const MOVIE_LIMIT: u64 = 512 << 20;
/// Seconds after the last key typed before a search is sent.
const TYPE_PAUSE: f32 = 0.8;

/// "archive.org:alien-hominid".
pub fn key(source: Source, id: &str) -> String {
    format!("{}:{}", source.site(), id)
}

pub fn user_agent() -> String {
    format!("Ruffle5/{} (PlayStation 5 Flash player; +https://github.com/TheUnwelcomed/RUFFLE5)", crate::APP_VERSION)
}

// ---- the worker threads ---------------------------------------------------------------------------

enum Job {
    Search { source: Source, words: String, page: u32, gen: u64 },
    Get { source: Source, item: Item, games_dir: String, play: bool, cancel: Arc<AtomicBool> },
}

enum Event {
    Page { source: Source, gen: u64, page: u32, result: Result<r5net::Page, String> },
    Picture { key: String, result: Result<(u32, u32, Vec<u8>), String> },
    Progress { key: String, stage: &'static str, got: u64, total: Option<u64> },
    Got { key: String, play: bool, result: Result<String, String> },
}

struct Workers {
    jobs: Sender<Job>,
    pictures: Sender<(String, String)>,
    events: Receiver<Event>,
}

fn start_workers() -> Result<Workers, String> {
    let (ev_tx, events) = mpsc::channel();
    let (jobs, job_rx) = mpsc::channel::<Job>();
    let (pictures, pic_rx) = mpsc::channel::<(String, String)>();
    let client = Arc::new(Client::new(&user_agent()));
    {
        let ev = ev_tx.clone();
        let client = Arc::clone(&client);
        std::thread::Builder::new()
            .name("online".into())
            .spawn(move || {
                for job in job_rx {
                    match job {
                        Job::Search { source, words, page, gen } => {
                            let result = source.search(&client, &words, page).map_err(|e| e.to_string());
                            if ev.send(Event::Page { source, gen, page, result }).is_err() {
                                return;
                            }
                        }
                        Job::Get { source, item, games_dir, play, cancel } => {
                            let k = key(source, &item.id);
                            let mut progress = |stage: &'static str, got: u64, total: Option<u64>| {
                                let _ = ev.send(Event::Progress { key: k.clone(), stage, got, total });
                            };
                            let result = get_game(&client, source, &item, &games_dir, &cancel, &mut progress);
                            if ev.send(Event::Got { key: k.clone(), play, result }).is_err() {
                                return;
                            }
                        }
                    }
                }
            })
            .map_err(|e| format!("the online thread: {e}"))?;
    }
    {
        let ev = ev_tx;
        std::thread::Builder::new()
            .name("pictures".into())
            .spawn(move || {
                for (k, url) in pic_rx {
                    let result = fetch_picture(&client, &url, PICTURE_SIZE, false);
                    if ev.send(Event::Picture { key: k, result }).is_err() {
                        return;
                    }
                }
            })
            .map_err(|e| format!("the pictures thread: {e}"))?;
    }
    Ok(Workers { jobs, pictures, events })
}

/// A picture (PNG, JPEG, GIF or WebP), as RGBA no larger than `max` on its longest side.
fn fetch_picture(client: &Client, url: &str, max: u32, log: bool) -> Result<(u32, u32, Vec<u8>), String> {
    let mut req = Request::new(url);
    req.accept = "image/*";
    req.limit = 8 << 20;
    req.log = log;
    let r = client.get_ok(&req).map_err(|e| e.to_string())?;
    decode_picture(&r.body, max).map_err(|e| format!("{}: {e}", r.url))
}

pub fn decode_picture(bytes: &[u8], max: u32) -> Result<(u32, u32, Vec<u8>), String> {
    let img = image::load_from_memory(bytes).map_err(|e| format!("not a picture this can read ({e})"))?;
    let rgba = img.to_rgba8();
    let (w, h) = (rgba.width(), rgba.height());
    if w == 0 || h == 0 {
        return Err("an empty picture".into());
    }
    Ok(crate::ui::shrink_rgba(w, h, rgba.as_raw(), max))
}

// ---- fetching a game --------------------------------------------------------------------------------

/// A file or folder name from a title: no path separators or characters a file system refuses,
/// no leading/trailing dots or spaces, at most 80 characters.
pub fn safe_name(s: &str) -> String {
    let cleaned: String = s.chars().map(|c| if c.is_control() || "/\\:*?\"<>|".contains(c) { ' ' } else { c }).collect();
    let words: Vec<&str> = cleaned.split_whitespace().collect();
    let mut out: String = words.join(" ").chars().take(80).collect();
    out = out.trim_matches(|c: char| c == '.' || c == ' ').to_string();
    out
}

/// A path inside the game's folder from a name the source gave ("dir/My Game.swf"): each part a safe
/// name, no "." or "..", at most 4 levels.
pub fn safe_rel(name: &str) -> String {
    let parts: Vec<String> = name.split(['/', '\\']).map(safe_name).filter(|p| !p.is_empty() && p != "." && p != "..").collect();
    let n = parts.len();
    parts.into_iter().skip(n.saturating_sub(4)).collect::<Vec<_>>().join("/")
}

/// online.txt's fields.
pub fn read_online_txt(path: &str) -> HashMap<String, String> {
    std::fs::read_to_string(path)
        .unwrap_or_default()
        .lines()
        .filter(|l| !l.trim_start().starts_with('#'))
        .filter_map(|l| l.split_once('=').map(|(k, v)| (k.trim().to_string(), v.trim().to_string())))
        .collect()
}

fn online_txt(source: Source, item: &Item, movie: &str) -> String {
    let one_line = |s: &str| s.replace(['\r', '\n'], " ");
    let mut s = String::from("# Ruffle5: where this game came from (the Online tab). Delete the folder to remove the game.\n");
    s.push_str(&format!("site = {}\n", source.site()));
    s.push_str(&format!("id = {}\n", one_line(&item.id)));
    s.push_str(&format!("title = {}\n", one_line(&item.title)));
    s.push_str(&format!("movie = {}\n", one_line(movie)));
    s.push_str(&format!("page = {}\n", one_line(&item.page)));
    if let Some(c) = &item.creator {
        s.push_str(&format!("creator = {}\n", one_line(c)));
    }
    if let Some(y) = &item.year {
        s.push_str(&format!("year = {}\n", one_line(y)));
    }
    s
}

/// The folder a game goes in: its title, or its title and id when another game has the title;
/// the folder it already has when it was fetched before.
pub fn choose_folder(games_dir: &str, source: Source, item: &Item) -> String {
    let base = match safe_name(&item.title) {
        t if t.is_empty() => safe_name(&item.id),
        t => t,
    };
    let base = if base.is_empty() { "game".to_string() } else { base };
    let candidates = [base.clone(), format!("{base} ({})", safe_name(&item.id))];
    for c in &candidates {
        let dir = format!("{games_dir}/{c}");
        if !std::path::Path::new(&dir).exists() {
            return dir;
        }
        let o = read_online_txt(&format!("{dir}/online.txt"));
        if o.get("site").map(String::as_str) == Some(source.site()) && o.get("id") == Some(&item.id) {
            return dir;
        }
    }
    let mut n = 2;
    loop {
        let dir = format!("{games_dir}/{} {n}", candidates[1]);
        if !std::path::Path::new(&dir).exists() {
            return dir;
        }
        n += 1;
    }
}

/// Download `url` to `path` (through `path.part`, renamed when whole).
fn download_to(client: &Client, url: &str, referer: Option<&str>, path: &str, limit: u64, cancel: &AtomicBool, progress: &mut dyn FnMut(u64, Option<u64>)) -> Result<u64, String> {
    if let Some(dir) = std::path::Path::new(path).parent() {
        std::fs::create_dir_all(dir).map_err(|e| format!("making {}: {e}", dir.display()))?;
    }
    let part = format!("{path}.part");
    let mut f = std::fs::File::create(&part).map_err(|e| format!("writing {part}: {e}"))?;
    let mut req = Request::new(url);
    req.referer = referer;
    req.limit = limit;
    let result = client.download(&req, &mut f, &mut |got, total| {
        progress(got, total);
        !cancel.load(Ordering::Relaxed)
    });
    let _ = f.sync_all();
    drop(f);
    match result {
        Ok(d) => {
            std::fs::rename(&part, path).map_err(|e| format!("renaming {part}: {e}"))?;
            Ok(d.bytes)
        }
        Err(e) => {
            let _ = std::fs::remove_file(&part);
            Err(e.to_string())
        }
    }
}

/// Look the game up, then fetch it.
fn get_game(client: &Client, source: Source, item: &Item, games_dir: &str, cancel: &AtomicBool, progress: &mut dyn FnMut(&'static str, u64, Option<u64>)) -> Result<String, String> {
    progress("Looking it up", 0, None);
    let files = source.files(client, item).map_err(|e| e.to_string())?;
    crate::say(&format!("online: {} {}: the movie is {} ({}), {} file(s) beside it", source.site(), item.id, files.main.name, files.note, files.extra.len()));
    fetch_files(client, source, item, &files, games_dir, cancel, progress)
}

/// The movie, the files beside it, the picture and online.txt into the game's folder. Returns the
/// movie's path.
pub fn fetch_files(client: &Client, source: Source, item: &Item, files: &Files, games_dir: &str, cancel: &AtomicBool, progress: &mut dyn FnMut(&'static str, u64, Option<u64>)) -> Result<String, String> {
    let folder = choose_folder(games_dir, source, item);
    let created = !std::path::Path::new(&folder).exists();
    std::fs::create_dir_all(&folder).map_err(|e| format!("making {folder}: {e}"))?;
    let undo = |why: String| -> String {
        if created {
            let _ = std::fs::remove_dir_all(&folder);
        }
        why
    };
    let mut movie = safe_rel(&files.main.name);
    if movie.is_empty() {
        movie = format!("{}.swf", safe_name(&item.id));
    }
    if !movie.to_ascii_lowercase().ends_with(".swf") {
        movie.push_str(".swf");
    }
    let movie_path = format!("{folder}/{movie}");
    let t0 = std::time::Instant::now();
    let bytes = download_to(client, &files.main.url, files.referer.as_deref(), &movie_path, MOVIE_LIMIT, cancel, &mut |got, total| progress("Downloading", got, total)).map_err(&undo)?;
    let head: Vec<u8> = std::fs::read(&movie_path).map(|b| b.into_iter().take(8).collect()).unwrap_or_default();
    if !(head.starts_with(b"FWS") || head.starts_with(b"CWS") || head.starts_with(b"ZWS")) {
        let hex: Vec<String> = head.iter().map(|b| format!("{b:02x}")).collect();
        return Err(undo(format!("{} is not a Flash movie (it starts {})", files.main.url, hex.join(" "))));
    }
    crate::say(&format!("online: {movie_path}: {bytes} bytes in {:.1} s, SWF version {}", t0.elapsed().as_secs_f64(), head.get(3).copied().unwrap_or(0)));
    let mut extra_ok = 0;
    for (i, f) in files.extra.iter().enumerate() {
        if cancel.load(Ordering::Relaxed) {
            return Err(undo("cancelled".into()));
        }
        let rel = safe_rel(&f.name);
        if rel.is_empty() || rel == movie {
            continue;
        }
        progress("The files beside it", i as u64, Some(files.extra.len() as u64));
        match download_to(client, &f.url, files.referer.as_deref(), &format!("{folder}/{rel}"), 64 << 20, cancel, &mut |_, _| {}) {
            Ok(_) => extra_ok += 1,
            Err(e) => crate::say(&format!("online: {}: not fetched: {e}", f.name)),
        }
    }
    if !files.extra.is_empty() {
        crate::say(&format!("online: {extra_ok} of {} file(s) beside the movie fetched", files.extra.len()));
    }
    if let Some(url) = &item.thumb {
        progress("Its picture", 0, None);
        match fetch_picture(client, url, 1024, true).and_then(|(w, h, rgba)| encode_png(w, h, &rgba)) {
            Ok(png) => {
                if let Err(e) = crate::fsutil::write_durably(format!("{folder}/cover.png"), &png) {
                    crate::say(&format!("online: {folder}/cover.png: {e}"));
                }
            }
            Err(e) => crate::say(&format!("online: no cover for {}: {e}", item.id)),
        }
    }
    crate::fsutil::write_durably(format!("{folder}/online.txt"), online_txt(source, item, &movie)).map_err(|e| undo(format!("{folder}/online.txt: {e}")))?;
    Ok(movie_path)
}

fn encode_png(w: u32, h: u32, rgba: &[u8]) -> Result<Vec<u8>, String> {
    let mut out = Vec::new();
    {
        let mut enc = png::Encoder::new(&mut out, w, h);
        enc.set_color(png::ColorType::Rgba);
        enc.set_depth(png::BitDepth::Eight);
        let mut writer = enc.write_header().map_err(|e| e.to_string())?;
        writer.write_image_data(rgba).map_err(|e| e.to_string())?;
        writer.finish().map_err(|e| e.to_string())?;
    }
    Ok(out)
}

/// The games fetched before: "site:id" -> the movie's path, from every online.txt in the games folder.
pub fn installed(games_dir: &str) -> HashMap<String, String> {
    let mut out = HashMap::new();
    let Ok(entries) = std::fs::read_dir(games_dir) else {
        return out;
    };
    for e in entries.filter_map(|e| e.ok()) {
        let dir = e.path();
        let o = read_online_txt(&dir.join("online.txt").to_string_lossy());
        let (Some(site), Some(id), Some(movie)) = (o.get("site"), o.get("id"), o.get("movie")) else {
            continue;
        };
        let Some(source) = Source::from_site(site) else {
            continue;
        };
        let path = dir.join(movie);
        if path.is_file() {
            out.insert(key(source, id), path.to_string_lossy().into_owned());
        }
    }
    out
}

// ---- the tab ----------------------------------------------------------------------------------------

#[derive(Default)]
struct List {
    items: Vec<Item>,
    total: Option<u64>,
    pages: u32,
    more: bool,
    loading: bool,
    error: Option<String>,
    /// The search these results are for.
    words: String,
    /// Bumped by a new search: answers to an older one are dropped.
    gen: u64,
    focus: usize,
    asked: bool,
}

enum Picture {
    Asked,
    Ready { id: ImageId, size: (u32, u32), shown: f32 },
    Failed,
}

struct Getting {
    key: String,
    title: String,
    stage: &'static str,
    got: u64,
    total: Option<u64>,
    cancel: Arc<AtomicBool>,
}

/// What the tab hands the menu.
pub enum Outcome {
    /// Rescan, then play this movie.
    Play(String),
    /// Rescan: a game was added.
    Added(String),
}

pub struct Online {
    pub source: usize,
    lists: [List; 2],
    workers: Option<Workers>,
    start_error: Option<String>,
    pictures: HashMap<String, Picture>,
    /// GPU slots of evicted pictures, reused before new ones are made.
    spare: Vec<ImageId>,
    in_flight: usize,
    getting: Option<Getting>,
    /// The last fetch's failure, shown on its item.
    failed: Option<(String, String)>,
    installed: HashMap<String, String>,
    games_dir: String,
    outcome: Option<Outcome>,
    typed_for: f32,
    last_words: String,
    pub scroll: Smooth,
    lifts: HashMap<String, Smooth>,
    time: f32,
    /// Seconds since the focused item changed.
    stage_anim: f32,
    stage_key: Option<String>,
    shelf_anim: f32,
    shown_now: bool,
    toasts: Vec<String>,
}

impl Online {
    pub fn new(games_dir: &str) -> Online {
        Online {
            source: 0,
            lists: [List::default(), List::default()],
            workers: None,
            start_error: None,
            pictures: HashMap::new(),
            spare: Vec::new(),
            in_flight: 0,
            getting: None,
            failed: None,
            installed: HashMap::new(),
            games_dir: games_dir.to_string(),
            outcome: None,
            typed_for: 0.0,
            last_words: String::new(),
            scroll: Smooth::default(),
            lifts: HashMap::new(),
            time: 0.0,
            stage_anim: 0.0,
            stage_key: None,
            shelf_anim: 0.0,
            shown_now: false,
            toasts: Vec::new(),
        }
    }

    fn src(&self) -> Source {
        SOURCES[self.source.min(SOURCES.len() - 1)]
    }

    fn list(&self) -> &List {
        &self.lists[self.source.min(1)]
    }

    fn list_mut(&mut self) -> &mut List {
        &mut self.lists[self.source.min(1)]
    }

    /// The menu took what the tab handed over.
    pub fn take_outcome(&mut self) -> Option<Outcome> {
        self.outcome.take()
    }

    /// Messages for the menu's toast.
    pub fn take_toasts(&mut self) -> Vec<String> {
        std::mem::take(&mut self.toasts)
    }

    /// A download is running (Circle cancels it rather than leaving).
    pub fn busy(&self) -> bool {
        self.getting.is_some()
    }

    pub fn cancel(&mut self) {
        if let Some(g) = self.getting.as_mut() {
            g.cancel.store(true, Ordering::Relaxed);
            g.stage = "Cancelling";
        }
    }

    fn ensure_started(&mut self) {
        if self.workers.is_some() || self.start_error.is_some() {
            return;
        }
        extern "C" {
            fn ps5_net_init() -> i32;
        }
        let rc = unsafe { ps5_net_init() };
        crate::say(&format!("online: sceNetInit = 0x{:08x}", rc as u32));
        if rc == -1000 {
            self.start_error = Some("the network library is not bound on this console".into());
            return;
        }
        match start_workers() {
            Ok(w) => {
                crate::say(&format!("online: started ({})", user_agent()));
                self.workers = Some(w);
                self.installed = installed(&self.games_dir);
                crate::say(&format!("online: {} game(s) in {} came from the Online tab", self.installed.len(), self.games_dir));
            }
            Err(e) => {
                crate::say(&format!("online: could not start: {e}"));
                self.start_error = Some(e);
            }
        }
    }

    fn ask_page(&mut self, page: u32) {
        let source = self.src();
        let Some(w) = &self.workers else {
            return;
        };
        let l = &mut self.lists[self.source.min(1)];
        l.loading = true;
        l.asked = true;
        l.error = None;
        let _ = w.jobs.send(Job::Search { source, words: l.words.clone(), page, gen: l.gen });
    }

    fn new_search(&mut self, words: &str) {
        let l = self.list_mut();
        let gen = l.gen + 1;
        *l = List { words: words.to_string(), gen, ..List::default() };
        self.scroll.snap(0.0);
        self.shelf_anim = 0.0;
        self.ask_page(1);
    }

    /// The tab is shown this frame: input and the requests it leads to. `words` is the menu's
    /// search, `typing` true while the on-screen keyboard is open.
    pub fn update(&mut self, dt: f32, nav: u32, pressed: u32, words: &str, typing: bool) {
        self.time += dt;
        self.stage_anim += dt;
        self.shelf_anim += dt;
        self.shown_now = true;
        self.ensure_started();
        if self.workers.is_none() {
            return;
        }
        // a search: when the keyboard closes, or a moment after the last key on a USB keyboard
        if words != self.last_words {
            self.last_words = words.to_string();
            self.typed_for = 0.0;
        } else {
            self.typed_for += dt;
        }
        let words_now = words.trim().to_string();
        if !self.list().asked {
            let w = words_now.clone();
            self.list_mut().words = w;
            self.ask_page(1);
        } else if words_now != self.list().words && !typing && self.typed_for >= TYPE_PAUSE {
            self.new_search(&words_now);
        }
        if pressed & BTN_R2 != 0 {
            self.source = (self.source + 1) % SOURCES.len();
            sounds::play(Sfx::Tab);
            self.shelf_anim = 0.0;
            self.scroll.snap(0.0);
            if !self.list().asked || self.list().words != words_now {
                let w = words_now.clone();
                let l = self.list_mut();
                let gen = l.gen + 1;
                *l = List { words: w, gen, ..List::default() };
                self.ask_page(1);
            }
        }
        let n = self.list().items.len();
        if n > 0 {
            let before = self.list().focus;
            let mut f = before.min(n - 1);
            if nav & BTN_RIGHT != 0 && f + 1 < n {
                f += 1;
            }
            if nav & BTN_LEFT != 0 && f > 0 {
                f -= 1;
            }
            if nav & BTN_DOWN != 0 {
                f = (f + 4).min(n - 1);
            }
            if nav & BTN_UP != 0 {
                f = f.saturating_sub(4);
            }
            if f != before {
                sounds::play(Sfx::Move);
            }
            self.list_mut().focus = f;
            // the next page, before the end is reached
            let (more, loading, failed, pages) = {
                let l = self.list();
                (l.more, l.loading, l.error.is_some(), l.pages)
            };
            if f + 8 >= n && more && !loading && !failed {
                self.ask_page(pages + 1);
            }
            let item = self.list().items[f].clone();
            let k = key(self.src(), &item.id);
            if self.stage_key.as_ref() != Some(&k) {
                self.stage_key = Some(k.clone());
                self.stage_anim = 0.0;
            }
            if pressed & (BTN_CROSS | BTN_SQUARE) != 0 {
                let play = pressed & BTN_CROSS != 0;
                if let Some(path) = self.installed.get(&k).cloned() {
                    if play {
                        sounds::play(Sfx::Launch);
                        self.outcome = Some(Outcome::Play(path));
                    } else {
                        self.toasts.push("Already in your Games".into());
                    }
                } else if let Some(g) = &self.getting {
                    self.toasts.push(format!("Still getting {}", g.title));
                } else {
                    sounds::play(Sfx::Select);
                    self.start_get(item, play);
                }
            }
        }
        if pressed & BTN_TRIANGLE != 0 && self.list().error.is_some() {
            sounds::play(Sfx::Select);
            let next = self.list().pages + 1;
            self.ask_page(next);
        }
        // the shelf follows the focus
        let pitch = crate::menu::TILE_W + crate::menu::TILE_GAP;
        let visible = crate::menu::RIGHT - crate::menu::MARGIN;
        let tile_x = self.list().focus as f32 * pitch;
        self.scroll.target = self.scroll.target.clamp((tile_x + crate::menu::TILE_W - visible).max(0.0), tile_x.max(0.0));
        self.scroll.step(dt);
        let focus_key = self.stage_key.clone();
        for (k, lift) in self.lifts.iter_mut() {
            lift.target = if Some(k) == focus_key.as_ref() { 1.0 } else { 0.0 };
            lift.step(dt);
        }
        if let Some(k) = focus_key {
            self.lifts.entry(k).or_default();
        }
        self.ask_pictures();
    }

    fn start_get(&mut self, item: Item, play: bool) {
        let source = self.src();
        let Some(w) = &self.workers else {
            return;
        };
        let cancel = Arc::new(AtomicBool::new(false));
        let k = key(source, &item.id);
        crate::say(&format!("online: getting {} ({k}){}", item.title, if play { ", then playing it" } else { "" }));
        self.failed = None;
        self.getting = Some(Getting { key: k, title: item.title.clone(), stage: "Starting", got: 0, total: None, cancel: Arc::clone(&cancel) });
        let _ = w.jobs.send(Job::Get { source, item, games_dir: self.games_dir.clone(), play, cancel });
    }

    /// Ask for the pictures of the tiles nearest the focus.
    fn ask_pictures(&mut self) {
        if self.in_flight >= PICTURES_IN_FLIGHT {
            return;
        }
        let source = self.src();
        let l = &self.lists[self.source.min(1)];
        let n = l.items.len();
        if n == 0 {
            return;
        }
        let f = l.focus.min(n - 1);
        // the focused one, then outward: right first (the shelf scrolls that way)
        let mut order = vec![f];
        for d in 1..12 {
            if f + d < n {
                order.push(f + d);
            }
            if d <= 4 && f >= d {
                order.push(f - d);
            }
        }
        let wanted: Vec<(String, String)> = order
            .into_iter()
            .filter_map(|i| {
                let it = &l.items[i];
                let k = key(source, &it.id);
                match (&it.thumb, self.pictures.contains_key(&k)) {
                    (Some(url), false) => Some((k, url.clone())),
                    _ => None,
                }
            })
            .take(PICTURES_IN_FLIGHT - self.in_flight)
            .collect();
        let Some(w) = &self.workers else {
            return;
        };
        for (k, url) in wanted {
            if w.pictures.send((k.clone(), url)).is_ok() {
                self.pictures.insert(k, Picture::Asked);
                self.in_flight += 1;
            }
        }
    }

    /// Every menu frame (the tab shown or not): answers from the threads; pictures onto the GPU.
    pub fn poll(&mut self, ui: &mut Ui, device: &wgpu::Device, queue: &wgpu::Queue) {
        let shown = std::mem::replace(&mut self.shown_now, false);
        let Some(w) = &self.workers else {
            return;
        };
        let mut events = Vec::new();
        while let Ok(e) = w.events.try_recv() {
            events.push(e);
        }
        for e in events {
            match e {
                Event::Page { source, gen, page, result } => {
                    let li = SOURCES.iter().position(|s| *s == source).unwrap_or(0);
                    let l = &mut self.lists[li];
                    if gen != l.gen {
                        continue;
                    }
                    l.loading = false;
                    match result {
                        Ok(p) => {
                            let before = l.items.len();
                            for it in p.items {
                                if !l.items.iter().any(|x| x.id == it.id) {
                                    l.items.push(it);
                                }
                            }
                            l.total = p.total.or(l.total);
                            l.more = p.more;
                            l.pages = page;
                            crate::say(&format!(
                                "online: {} {}page {page}: {} new, {} listed{}",
                                source.site(),
                                if l.words.is_empty() { String::new() } else { format!("\"{}\" ", l.words) },
                                l.items.len() - before,
                                l.items.len(),
                                l.total.map(|t| format!(" of {t}")).unwrap_or_default()
                            ));
                        }
                        Err(e) => {
                            crate::say(&format!("online: {} page {page}: {e}", source.site()));
                            l.error = Some(e);
                        }
                    }
                }
                Event::Picture { key: k, result } => {
                    self.in_flight = self.in_flight.saturating_sub(1);
                    match result {
                        Ok((pw, ph, rgba)) => {
                            let id = self.picture_slot(ui, device, queue, pw, ph, &rgba);
                            self.pictures.insert(k, Picture::Ready { id, size: (pw, ph), shown: self.time });
                        }
                        Err(e) => {
                            crate::say(&format!("online: picture of {k}: {e}"));
                            self.pictures.insert(k, Picture::Failed);
                        }
                    }
                }
                Event::Progress { key: k, stage, got, total } => {
                    if let Some(g) = self.getting.as_mut().filter(|g| g.key == k) {
                        if g.stage != "Cancelling" {
                            g.stage = stage;
                        }
                        g.got = got;
                        g.total = total;
                    }
                }
                Event::Got { key: k, play, result } => {
                    let title = self.getting.take().map(|g| g.title).unwrap_or_default();
                    match result {
                        Ok(path) => {
                            crate::say(&format!("online: {title} is in {path}"));
                            self.installed = installed(&self.games_dir);
                            self.installed.insert(k, path.clone());
                            if play && shown {
                                self.outcome = Some(Outcome::Play(path));
                            } else {
                                self.toasts.push(format!("{title} is in your Games"));
                                self.outcome = Some(Outcome::Added(path));
                            }
                        }
                        Err(e) if e == "cancelled" => {
                            crate::say(&format!("online: {title}: cancelled"));
                            self.toasts.push("Cancelled".into());
                        }
                        Err(e) => {
                            crate::say(&format!("online: {title} not fetched: {e}"));
                            self.failed = Some((k, e));
                            self.toasts.push(format!("Could not get {title}"));
                        }
                    }
                }
            }
        }
    }

    /// A GPU picture: a spare slot, a new one, or the one shown longest ago.
    fn picture_slot(&mut self, ui: &mut Ui, device: &wgpu::Device, queue: &wgpu::Queue, w: u32, h: u32, rgba: &[u8]) -> ImageId {
        if let Some(id) = self.spare.pop() {
            ui.replace_image(device, queue, id, w, h, rgba);
            return id;
        }
        let ready = self.pictures.values().filter(|p| matches!(p, Picture::Ready { .. })).count();
        if ready >= MAX_PICTURES {
            let oldest = self
                .pictures
                .iter()
                .filter_map(|(k, p)| match p {
                    Picture::Ready { shown, id, .. } => Some((k.clone(), *shown, *id)),
                    _ => None,
                })
                .min_by(|a, b| a.1.total_cmp(&b.1));
            if let Some((k, _, id)) = oldest {
                self.pictures.remove(&k);
                ui.replace_image(device, queue, id, w, h, rgba);
                return id;
            }
        }
        ui.add_image(device, queue, w, h, rgba)
    }

    pub fn hints(&self) -> Vec<(Glyph, &'static str)> {
        let mut h = Vec::new();
        if let Some(it) = self.focused() {
            if self.installed.contains_key(&key(self.src(), &it.id)) {
                h.push((Glyph::Cross, "Play"));
            } else {
                h.push((Glyph::Cross, "Get & play"));
                h.push((Glyph::Square, "Get"));
            }
        }
        if self.list().error.is_some() {
            h.push((Glyph::Triangle, "Try again"));
        }
        h.push((Glyph::Chip("L2"), "Search"));
        h.push((Glyph::Chip("R2"), "Source"));
        h
    }

    fn focused(&self) -> Option<&Item> {
        let l = self.list();
        l.items.get(l.focus.min(l.items.len().saturating_sub(1)))
    }

    fn picture(&mut self, k: &str) -> Option<(ImageId, (u32, u32))> {
        let t = self.time;
        match self.pictures.get_mut(k) {
            Some(Picture::Ready { id, size, shown }) => {
                *shown = t;
                Some((*id, *size))
            }
            _ => None,
        }
    }

    pub fn draw(&mut self, ui: &mut Ui, scene: &mut Scene) {
        use crate::menu::{MARGIN, STAGE_BUTTONS_Y, STAGE_KICKER_Y, STAGE_META_Y, STAGE_TEXT_Y, STAGE_TITLE_Y};
        let source = self.src();
        if self.list().items.is_empty() {
            scene.gradient_h(Rect::new(0.0, 150.0, 1400.0, 700.0), 0.0, NIGHT.with_alpha(0.86), NIGHT.with_alpha(0.0));
            ribbon_fold(scene, Rect::new(MARGIN + 2.0, STAGE_KICKER_Y - 12.0, 28.0, 6.0), ORANGE, 1.0);
            let l = self.list();
            let (kicker, title, lines): (String, String, Vec<String>) = if let Some(e) = &self.start_error {
                ("OFFLINE".into(), "Not available".into(), vec![e.clone()])
            } else if let Some(e) = &l.error {
                (
                    source.name().to_uppercase(),
                    format!("{} did not answer", source.site()),
                    vec![e.clone(), "Triangle tries again; R2 switches to the other source. Is the console online?".into()],
                )
            } else if l.loading || !l.asked {
                (source.name().to_uppercase(), "Looking\u{2026}".into(), vec![if l.words.is_empty() { format!("Asking {} for its Flash games.", source.site()) } else { format!("Searching {} for \u{201c}{}\u{201d}.", source.site(), l.words) }])
            } else if !l.words.is_empty() {
                (source.name().to_uppercase(), "Nothing found".into(), vec![format!("{} has nothing for \u{201c}{}\u{201d}.", source.name(), l.words), "Circle clears the search; R2 tries the other source.".into()])
            } else {
                (source.name().to_uppercase(), "Nothing listed".into(), vec![format!("{} listed nothing.", source.site())])
            };
            scene.text(ui, Face::SemiBold, 18.0, &kicker, MARGIN + 40.0, STAGE_KICKER_Y, ORANGE, Align::Left, 4.0);
            let w = scene.text(ui, Face::Display, 88.0, &title, MARGIN - 4.0, STAGE_TITLE_Y, INK, Align::Left, 0.0);
            if l.loading {
                spinner(scene, MARGIN + w + 60.0, STAGE_TITLE_Y - 30.0, 22.0, self.time, ORANGE);
            }
            for (i, line) in lines.iter().enumerate() {
                for (j, part) in ui.wrap(Face::Regular, 25.0, line, 1500.0).iter().take(2).enumerate() {
                    scene.text(ui, Face::Regular, 25.0, part, MARGIN, STAGE_META_Y + 40.0 * (i * 2 + j) as f32, INK.with_alpha(0.86), Align::Left, 0.0);
                }
            }
            self.draw_source_switch(ui, scene);
            return;
        }
        let Some(item) = self.focused().cloned() else {
            return;
        };
        let k = key(source, &item.id);
        let accent = Color::hue(hash(&k) as f32 / u32::MAX as f32, 0.55, 0.78);
        let pic = self.picture(&k);

        // the stage: the picture on the right, the words on the left (as the Games tab)
        let come = ease(self.stage_anim / 0.35);
        let bob = 7.0 * (self.time * 0.9).sin();
        let cover = Rect::new(1220.0, 232.0 + bob, 420.0, 360.0).scaled(0.96 + 0.04 * come);
        scene.push_opacity(come);
        focus_halo(scene, cover, 56.0, 1.0, self.time, Color::mix(accent, ORANGE, 0.45));
        scene.shadow(Rect::new(cover.x, cover.y + 30.0, cover.w, cover.h), 40.0, 70.0, BLACK.with_alpha(0.45));
        match pic {
            Some((id, size)) => scene.image_cover(id, size, cover, 56.0, WHITE),
            None => {
                scene.gradient_v(cover, 56.0, Color::mix(accent, BLUE_6, 0.35), Color::mix(accent, NIGHT, 0.6));
                scene.text(ui, Face::Display, 150.0, &initials(&item.title), cover.cx(), cover.cy() + 54.0, INK.with_alpha(0.92), Align::Center, 2.0);
            }
        }
        focus_ring(scene, cover, 56.0, 0.85, self.time + 1.6);
        scene.pop_opacity();
        scene.gradient_h(Rect::new(0.0, 150.0, 1300.0, 560.0), 0.0, NIGHT.with_alpha(0.86), NIGHT.with_alpha(0.0));
        scene.gradient_v(Rect::new(0.0, 560.0, W, 140.0), 0.0, NIGHT.with_alpha(0.0), NIGHT.with_alpha(0.86));
        scene.rect(Rect::new(0.0, 700.0, W, crate::ui::H - 700.0), 0.0, NIGHT.with_alpha(0.86));

        scene.push_opacity(come);
        let mx = MARGIN - 28.0 * (1.0 - come);
        ribbon_fold(scene, Rect::new(mx + 2.0, STAGE_KICKER_Y - 12.0, 28.0 * come.max(0.2), 6.0), ORANGE, 1.0);
        let kind = item.kind.as_deref().unwrap_or("Flash").to_uppercase();
        let kicker = format!("{} \u{b7} {kind}", source.name().to_uppercase());
        scene.text(ui, Face::SemiBold, 18.0, &kicker, mx + 40.0, STAGE_KICKER_Y, ORANGE, Align::Left, 4.0);
        let headline = ui.fit(Face::Display, 88.0, &item.title, 1080.0);
        scene.text(ui, Face::Display, 88.0, &headline, mx - 4.0, STAGE_TITLE_Y, INK, Align::Left, 0.0);
        let mut meta = Vec::new();
        if let Some(c) = &item.creator {
            meta.push(c.clone());
        }
        if let Some(y) = &item.year {
            meta.push(y.clone());
        }
        if let Some(d) = item.downloads {
            meta.push(format!("{} downloads", thousands(d)));
        }
        meta.push(source.site().to_string());
        let meta = ui.fit(Face::Regular, 25.0, &meta.join("  \u{b7}  "), 1080.0);
        scene.text(ui, Face::Regular, 25.0, &meta, mx, STAGE_META_Y, BLUE_2, Align::Left, 0.0);

        // the line under it: a download's progress, its failure, or the item's description
        let installed = self.installed.contains_key(&k);
        let mut y = STAGE_TEXT_Y;
        if let Some(g) = self.getting.as_ref().filter(|g| g.key == k) {
            let words = match (g.stage, g.total) {
                ("Downloading", Some(t)) if t > 0 => format!("Downloading {} of {}", size_text(g.got), size_text(t)),
                ("Downloading", _) => format!("Downloading {}", size_text(g.got)),
                (s, _) => format!("{s}\u{2026}"),
            };
            scene.text(ui, Face::SemiBold, 25.0, &words, mx, y, ORANGE, Align::Left, 0.0);
            let track = Rect::new(mx, y + 18.0, 640.0, 8.0);
            scene.rect(track, 4.0, WHITE.with_alpha(0.14));
            let f = match g.total {
                Some(t) if t > 0 && g.stage == "Downloading" => (g.got as f32 / t as f32).clamp(0.0, 1.0),
                _ => 0.0,
            };
            if f > 0.0 {
                scene.rect(Rect::new(track.x, track.y, (track.w * f).max(8.0), track.h), 4.0, ORANGE);
            } else {
                // not known yet: a slider
                let u = (self.time * 0.8).fract();
                scene.rect(Rect::new(track.x + (track.w - 120.0) * u, track.y, 120.0, track.h), 4.0, ORANGE);
            }
        } else if let Some((_, e)) = self.failed.as_ref().filter(|(fk, _)| *fk == k) {
            for (i, line) in ui.wrap(Face::Regular, 23.0, &format!("Not fetched: {e}"), 1080.0).iter().take(2).enumerate() {
                scene.text(ui, Face::Regular, 23.0, line, mx, y + 34.0 * i as f32, Color::rgb(0xff8a80), Align::Left, 0.0);
            }
        } else if installed {
            scene.text(ui, Face::SemiBold, 25.0, "In your Games \u{2713}", mx, y, Color::rgb(0x9be38f), Align::Left, 0.0);
            y += 38.0;
            if let Some(d) = &item.description {
                if let Some(line) = ui.wrap(Face::Regular, 23.0, d, 1080.0).first() {
                    scene.text(ui, Face::Regular, 23.0, line, mx, y, INK.with_alpha(0.8), Align::Left, 0.0);
                }
            }
        } else if let Some(d) = &item.description {
            for (i, line) in ui.wrap(Face::Regular, 23.0, d, 1080.0).iter().take(2).enumerate() {
                scene.text(ui, Face::Regular, 23.0, line, mx, y + 34.0 * i as f32, INK.with_alpha(0.86), Align::Left, 0.0);
            }
        }
        let breath = breathe(self.time, 2.4);
        let mut x = mx;
        if installed {
            button(ui, scene, x, STAGE_BUTTONS_Y, 64.0, Glyph::Cross, "Play", true, 0.6 + 0.4 * breath);
        } else if self.getting.is_none() {
            x += button(ui, scene, x, STAGE_BUTTONS_Y, 64.0, Glyph::Cross, "Get & play", true, 0.6 + 0.4 * breath) + 16.0;
            button(ui, scene, x, STAGE_BUTTONS_Y, 64.0, Glyph::Square, "Get", false, 0.0);
        } else {
            button(ui, scene, x, STAGE_BUTTONS_Y, 64.0, Glyph::Circle, "Cancel", false, 0.0);
        }
        scene.pop_opacity();
        self.draw_shelf(ui, scene);
    }

    /// "Internet Archive | Flash Museum" with the current one lit, at the shelf's head.
    fn draw_source_switch(&self, ui: &mut Ui, scene: &mut Scene) {
        let mut x = crate::menu::RIGHT;
        for (i, s) in SOURCES.iter().enumerate().rev() {
            let on = i == self.source;
            let w = ui.measure(Face::SemiBold, 20.0, s.name(), 0.0) + 36.0;
            x -= w;
            let r = Rect::new(x, crate::menu::SHELF_HEAD_Y - 30.0, w, 40.0);
            scene.rect(r, 20.0, if on { ORANGE } else { NIGHT.with_alpha(0.8) });
            scene.text(ui, Face::SemiBold, 20.0, s.name(), r.cx(), centred(r.cy(), 20.0), if on { ON_ORANGE } else { INK.with_alpha(0.7) }, Align::Center, 0.0);
            x -= 10.0;
        }
        draw_glyph(ui, scene, Glyph::Chip("R2"), x - 52.0, crate::menu::SHELF_HEAD_Y - 10.0, 26.0);
    }

    fn draw_shelf(&mut self, ui: &mut Ui, scene: &mut Scene) {
        use crate::menu::{LIFT, MARGIN, SHELF_HEAD_Y, SHELF_TILES_Y, TILE_GAP, TILE_H, TILE_RADIUS, TILE_W};
        let source = self.src();
        let l = self.list();
        let head = if l.words.is_empty() { "Most played".to_string() } else { format!("\u{201c}{}\u{201d}", l.words) };
        let hw = scene.text(ui, Face::SemiBold, 30.0, &head, MARGIN, SHELF_HEAD_Y, INK, Align::Left, 0.0);
        let count = match (l.total, l.loading) {
            (Some(t), _) => format!("{} of {}", l.items.len(), thousands(t)),
            (None, true) => format!("{} \u{b7} more coming", l.items.len()),
            (None, false) => format!("{}{}", l.items.len(), if l.more { "+" } else { "" }),
        };
        scene.text(ui, Face::Regular, 18.0, &count, MARGIN + hw + 12.0, SHELF_HEAD_Y, BLUE_2, Align::Left, 0.0);
        self.draw_source_switch(ui, scene);
        let pitch = TILE_W + TILE_GAP;
        let scroll = self.scroll.value;
        let n = self.list().items.len();
        let focus = self.list().focus.min(n.saturating_sub(1));
        let order: Vec<usize> = (0..n).filter(|&i| i != focus).chain(std::iter::once(focus)).collect();
        for i in order {
            let screen_k = (i as f32 - scroll / pitch).max(0.0);
            let enter = ease((self.shelf_anim - 0.05 * screen_k) / 0.32);
            if enter <= 0.0 {
                continue;
            }
            let r = Rect::new(MARGIN + i as f32 * pitch - scroll, SHELF_TILES_Y + 36.0 * (1.0 - enter), TILE_W, TILE_H);
            if r.x > W || r.x + r.w < 0.0 {
                continue;
            }
            let item = self.list().items[i].clone();
            let k = key(source, &item.id);
            let lift = self.lifts.get(&k).map(|s| s.value).unwrap_or(0.0);
            let rs = r.scaled(1.0 + LIFT * lift);
            let rs = Rect::new(rs.x, rs.y - 8.0 * lift, rs.w, rs.h);
            let accent = Color::hue(hash(&k) as f32 / u32::MAX as f32, 0.55, 0.78);
            let light = 0.74 + 0.26 * lift;
            scene.push_opacity(enter);
            focus_halo(scene, rs, TILE_RADIUS, lift, self.time, Color::mix(accent, ORANGE, 0.5));
            scene.gradient_v(rs, TILE_RADIUS, Color::mix(accent, BLUE_6, 0.35).with_alpha(0.95), Color::mix(accent, NIGHT, 0.86));
            match self.picture(&k) {
                Some((id, size)) => {
                    scene.image_cover(id, size, rs, TILE_RADIUS, Color { r: light, g: light, b: light, a: 1.0 });
                    scene.gradient_v(Rect::new(rs.x, rs.y + rs.h * 0.45, rs.w, rs.h * 0.55), TILE_RADIUS, BLACK.with_alpha(0.0), NIGHT.with_alpha(0.85));
                }
                None => {
                    let s = rs.h * 0.48;
                    let icon = Rect::new(rs.cx() - s * 0.5, rs.y + rs.h * 0.4 - s * 0.5, s, s);
                    scene.gradient_v(icon, s * 0.22, Color::mix(accent, BLUE_6, 0.3), Color::mix(accent, NIGHT, 0.55));
                    scene.text(ui, Face::Display, s * 0.46, &initials(&item.title), icon.cx(), icon.cy() + s * 0.17, INK.with_alpha(0.92), Align::Center, 1.0);
                }
            }
            let name = ui.fit(Face::SemiBold, 24.0, &item.title, rs.w - 40.0);
            scene.text(ui, Face::SemiBold, 24.0, &name, rs.x + 20.0, rs.y + rs.h - 42.0, INK, Align::Left, 0.0);
            let mut sub = Vec::new();
            if let Some(y) = &item.year {
                sub.push(y.clone());
            }
            if let Some(kind) = &item.kind {
                sub.push(kind.clone());
            }
            if let Some(c) = &item.creator {
                sub.push(c.clone());
            }
            let sub = ui.fit(Face::Regular, 18.0, &sub.join("  \u{b7}  "), rs.w - 40.0);
            scene.text(ui, Face::Regular, 18.0, &sub, rs.x + 20.0, rs.y + rs.h - 16.0, BLUE_2, Align::Left, 0.0);
            if self.installed.contains_key(&k) {
                scene.text(ui, Face::SemiBold, 22.0, "\u{2713}", rs.right() - 22.0, rs.y + 36.0, Color::rgb(0x9be38f), Align::Right, 0.0);
            } else if self.getting.as_ref().is_some_and(|g| g.key == k) {
                spinner(scene, rs.right() - 34.0, rs.y + 32.0, 12.0, self.time, ORANGE);
            }
            scene.stroke(rs, TILE_RADIUS, 1.5, INK.with_alpha(0.1));
            focus_ring(scene, rs, TILE_RADIUS, lift, self.time);
            scene.pop_opacity();
        }
        if self.list().loading && n > 0 {
            let x = MARGIN + n as f32 * pitch - scroll + 40.0;
            if x < W {
                spinner(scene, x, SHELF_TILES_Y + TILE_H * 0.5, 22.0, self.time, ORANGE);
            }
        }
    }
}

/// 12345 -> "12,345".
fn thousands(n: u64) -> String {
    let s = n.to_string();
    let mut out = String::new();
    for (i, c) in s.chars().enumerate() {
        if i > 0 && (s.len() - i) % 3 == 0 {
            out.push(',');
        }
        out.push(c);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::{Read, Write};

    #[test]
    fn names_and_paths_are_file_safe() {
        assert_eq!(safe_name("Alien Hominid: The \"Game\"?"), "Alien Hominid The Game");
        assert_eq!(safe_name("  ..dots..  "), "dots");
        assert_eq!(safe_name("a/b\\c"), "a b c");
        assert_eq!(safe_name(&"x".repeat(200)).len(), 80);
        assert_eq!(safe_rel("../../etc/passwd"), "etc/passwd");
        assert_eq!(safe_rel("dir/My Game.swf"), "dir/My Game.swf");
        assert_eq!(safe_rel("a/b/c/d/e/f.swf"), "c/d/e/f.swf");
        assert_eq!(safe_rel("./x.swf"), "x.swf");
        assert_eq!(thousands(1234567), "1,234,567");
        assert_eq!(thousands(12), "12");
    }

    fn temp(name: &str) -> String {
        let d = std::env::temp_dir().join(format!("ruffle5-online-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        d.to_string_lossy().into_owned()
    }

    #[test]
    fn folders_follow_the_title_and_find_their_own_game_again() {
        let games = temp("folders");
        let item = Item { id: "alien-hominid".into(), title: "Alien Hominid".into(), ..Item::default() };
        let first = choose_folder(&games, Source::InternetArchive, &item);
        assert_eq!(first, format!("{games}/Alien Hominid"));
        std::fs::create_dir_all(&first).unwrap();
        std::fs::write(format!("{first}/online.txt"), online_txt(Source::InternetArchive, &item, "ah.swf")).unwrap();
        // the same game: the same folder
        assert_eq!(choose_folder(&games, Source::InternetArchive, &item), first);
        // another game with that title: title and id
        let other = Item { id: "alien-hominid-2".into(), title: "Alien Hominid".into(), ..Item::default() };
        assert_eq!(choose_folder(&games, Source::FlashMuseum, &other), format!("{games}/Alien Hominid (alien-hominid-2)"));
        // what online.txt says, read back
        let o = read_online_txt(&format!("{first}/online.txt"));
        assert_eq!((o["site"].as_str(), o["id"].as_str(), o["movie"].as_str(), o["title"].as_str()), ("archive.org", "alien-hominid", "ah.swf", "Alien Hominid"));
        std::fs::write(format!("{first}/ah.swf"), b"FWS\x08").unwrap();
        let inst = installed(&games);
        assert_eq!(inst.get("archive.org:alien-hominid").map(String::as_str), Some(format!("{first}/ah.swf").as_str()));
        let _ = std::fs::remove_dir_all(&games);
    }

    /// A tiny HTTP server: path -> (status, body).
    fn serve(routes: Vec<(&'static str, u16, Vec<u8>)>, n: usize) -> (u16, std::thread::JoinHandle<()>) {
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        let h = std::thread::spawn(move || {
            for _ in 0..n {
                let Ok((mut s, _)) = listener.accept() else { return };
                let mut req = Vec::new();
                let mut b = [0u8; 1];
                while !req.ends_with(b"\r\n\r\n") {
                    if s.read(&mut b).unwrap_or(0) == 0 {
                        break;
                    }
                    req.push(b[0]);
                }
                let line = String::from_utf8_lossy(&req).lines().next().unwrap_or("").to_string();
                let path = line.split_whitespace().nth(1).unwrap_or("/").to_string();
                let (status, body) = routes.iter().find(|(p, _, _)| *p == path).map(|(_, s, b)| (*s, b.clone())).unwrap_or((404, b"nope".to_vec()));
                let _ = s.write_all(format!("HTTP/1.1 {status} X\r\nContent-Length: {}\r\n\r\n", body.len()).as_bytes());
                let _ = s.write_all(&body);
            }
        });
        (port, h)
    }

    fn tiny_png() -> Vec<u8> {
        encode_png(2, 2, &[255, 0, 0, 255, 0, 255, 0, 255, 0, 0, 255, 255, 255, 255, 255, 255]).unwrap()
    }

    #[test]
    fn a_game_is_fetched_into_a_folder_of_its_own() {
        let games = temp("fetch");
        let mut swf = b"CWS\x0a".to_vec();
        swf.extend_from_slice(&[0u8; 300]);
        let (port, server) = serve(vec![("/m/Game.swf", 200, swf.clone()), ("/m/levels/one.xml", 200, b"<l/>".to_vec()), ("/m/missing.xml", 404, vec![]), ("/pic", 200, tiny_png())], 4);
        let base = format!("http://127.0.0.1:{port}");
        let item = Item { id: "game-1".into(), title: "A Game: Part 1".into(), thumb: Some(format!("{base}/pic")), page: format!("{base}/details/game-1"), ..Item::default() };
        let files = Files {
            main: r5net::RemoteFile { name: "Game.swf".into(), url: format!("{base}/m/Game.swf"), size: None },
            extra: vec![
                r5net::RemoteFile { name: "levels/one.xml".into(), url: format!("{base}/m/levels/one.xml"), size: None },
                r5net::RemoteFile { name: "missing.xml".into(), url: format!("{base}/m/missing.xml"), size: None },
            ],
            referer: None,
            note: "test".into(),
        };
        let client = Client::new("test");
        let cancel = AtomicBool::new(false);
        let mut stages = Vec::new();
        let movie = fetch_files(&client, Source::InternetArchive, &item, &files, &games, &cancel, &mut |s, _, _| stages.push(s)).unwrap();
        server.join().unwrap();
        let folder = format!("{games}/A Game Part 1");
        assert_eq!(movie, format!("{folder}/Game.swf"));
        assert_eq!(std::fs::read(&movie).unwrap(), swf);
        assert_eq!(std::fs::read_to_string(format!("{folder}/levels/one.xml")).unwrap(), "<l/>");
        assert!(!std::path::Path::new(&format!("{folder}/missing.xml")).exists());
        let cover = std::fs::read(format!("{folder}/cover.png")).unwrap();
        assert_eq!(crate::ui::decode_png(&cover).unwrap().0, 2);
        let o = read_online_txt(&format!("{folder}/online.txt"));
        assert_eq!((o["movie"].as_str(), o["title"].as_str()), ("Game.swf", "A Game: Part 1"));
        assert!(stages.contains(&"Downloading") && stages.contains(&"Its picture"), "{stages:?}");
        // no partial files left
        let leftovers: Vec<String> = walk(&folder).into_iter().filter(|p| p.ends_with(".part") || p.ends_with(".tmp")).collect();
        assert!(leftovers.is_empty(), "{leftovers:?}");
        let _ = std::fs::remove_dir_all(&games);
    }

    #[test]
    fn something_that_is_not_a_movie_is_refused_and_leaves_nothing() {
        let games = temp("notswf");
        let (port, server) = serve(vec![("/x.swf", 200, b"<html>blocked</html>".to_vec())], 1);
        let item = Item { id: "x".into(), title: "X".into(), ..Item::default() };
        let files = Files { main: r5net::RemoteFile { name: "x.swf".into(), url: format!("http://127.0.0.1:{port}/x.swf"), size: None }, extra: vec![], referer: None, note: String::new() };
        let e = fetch_files(&Client::new("t"), Source::FlashMuseum, &item, &files, &games, &AtomicBool::new(false), &mut |_, _, _| {}).unwrap_err();
        server.join().unwrap();
        assert!(e.contains("is not a Flash movie (it starts 3c 68 74 6d"), "{e}");
        assert!(!std::path::Path::new(&format!("{games}/X")).exists());
        let _ = std::fs::remove_dir_all(&games);
    }

    fn walk(dir: &str) -> Vec<String> {
        let mut out = Vec::new();
        for e in std::fs::read_dir(dir).unwrap().filter_map(|e| e.ok()) {
            let p = e.path().to_string_lossy().into_owned();
            if e.path().is_dir() {
                out.extend(walk(&p));
            } else {
                out.push(p);
            }
        }
        out
    }

    #[test]
    fn pictures_decode_from_png() {
        let (w, h, rgba) = decode_picture(&tiny_png(), 512).unwrap();
        assert_eq!((w, h, rgba.len()), (2, 2, 16));
        assert!(decode_picture(b"not a picture", 512).is_err());
    }
}
