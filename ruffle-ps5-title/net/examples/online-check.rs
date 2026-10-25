//! online-check — from a PC, ask the online sources what the console's Online tab asks them: a
//! list, a search, an item's files, a movie, a picture; over the same HTTPS code (rustls with
//! RustCrypto). tools/online-check.sh runs it on the build server. 1.000.028: Flashpoint Archive
//! too — its Shockwave list, a search, an entry's game data zip (checked against its SHA-256 and
//! listed, the movie in it read and checked) and an entry without game data (its movie from the
//! legacy server).
//!
//!   cargo run --release --example online-check -- [OUT_DIR]
//!
//! OUT_DIR (default ./online-check) gets every answer as it came (JSON, HTML), pictures, and
//! check.log; the movies go to OUT_DIR/movies and are only measured (they are not anyone's to pass
//! around). HTTPS_PROXY (an HTTP CONNECT proxy) is used when set; R5NET_EXTRA_CA names a PEM file of
//! more roots to trust (a proxy that re-signs TLS).
// SPDX-License-Identifier: GPL-3.0-or-later

use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use r5net::{fm, fp, ia, Client, Request};

struct Logger {
    file: Mutex<std::fs::File>,
}

impl log::Log for Logger {
    fn enabled(&self, m: &log::Metadata) -> bool {
        m.level() <= log::Level::Info
    }
    fn log(&self, r: &log::Record) {
        if self.enabled(r.metadata()) {
            say(&self.file, &format!("[{} {}] {}", r.level(), r.target(), r.args()));
        }
    }
    fn flush(&self) {}
}

static LOG: std::sync::OnceLock<Logger> = std::sync::OnceLock::new();

fn say(file: &Mutex<std::fs::File>, line: &str) {
    println!("{line}");
    if let Ok(mut f) = file.lock() {
        let _ = writeln!(f, "{line}");
    }
}

fn out(line: &str) {
    if let Some(l) = LOG.get() {
        say(&l.file, line);
    }
}

fn save(dir: &Path, name: &str, body: &[u8]) {
    let p = dir.join(name);
    match std::fs::write(&p, body) {
        Ok(()) => out(&format!("     saved {} ({} bytes)", p.display(), body.len())),
        Err(e) => out(&format!("     could not save {}: {e}", p.display())),
    }
}

/// What a file's first bytes say it is.
fn magic(b: &[u8]) -> String {
    let kind = match b {
        [b'F', b'W', b'S', ..] => "SWF (uncompressed)",
        [b'C', b'W', b'S', ..] => "SWF (zlib)",
        [b'Z', b'W', b'S', ..] => "SWF (LZMA)",
        [0xff, 0xd8, 0xff, ..] => "JPEG",
        [0x89, b'P', b'N', b'G', ..] => "PNG",
        [b'G', b'I', b'F', b'8', ..] => "GIF",
        [b'R', b'I', b'F', b'F', _, _, _, _, b'W', b'E', b'B', b'P', ..] => "WebP",
        [b'X', b'F', b'I', b'R', ..] => "Director (XFIR)",
        [b'R', b'I', b'F', b'X', ..] => "Director (RIFX)",
        [b'P', b'K', 3, 4, ..] => "zip",
        [b'<', ..] => "HTML/XML text",
        _ => "unknown",
    };
    let version = if kind.starts_with("SWF") { b.get(3).map(|v| format!(", SWF version {v}")).unwrap_or_default() } else { String::new() };
    let hex: Vec<String> = b.iter().take(12).map(|x| format!("{x:02x}")).collect();
    format!("{kind}{version} [{}]", hex.join(" "))
}

fn ext_for(content_type: &str) -> &'static str {
    let c = content_type.to_ascii_lowercase();
    if c.contains("jpeg") {
        "jpg"
    } else if c.contains("png") {
        "png"
    } else if c.contains("gif") {
        "gif"
    } else if c.contains("webp") {
        "webp"
    } else {
        "bin"
    }
}

struct Tally {
    ok: Vec<String>,
    failed: Vec<String>,
}

impl Tally {
    fn step<T>(&mut self, name: &str, f: impl FnOnce() -> Result<T, String>) -> Option<T> {
        out(&format!("== {name}"));
        let t0 = std::time::Instant::now();
        match f() {
            Ok(v) => {
                out(&format!("ok   {name} ({:.2} s)", t0.elapsed().as_secs_f64()));
                self.ok.push(name.to_string());
                Some(v)
            }
            Err(e) => {
                out(&format!("FAIL {name}: {e}"));
                self.failed.push(format!("{name}: {e}"));
                None
            }
        }
    }
}

/// A Flash movie starts FWS, CWS or ZWS; a Director one RIFX or XFIR.
fn is_movie_bytes(name: &str, head: &[u8]) -> bool {
    if fp::is_flash_name(name) {
        head.starts_with(b"FWS") || head.starts_with(b"CWS") || head.starts_with(b"ZWS")
    } else {
        head.starts_with(b"XFIR") || head.starts_with(b"RIFX")
    }
}

fn show_items(items: &[r5net::Item], n: usize) {
    for it in items.iter().take(n) {
        out(&format!(
            "     {} | {} | {} | {} | {} | ruffle {} | thumb {}",
            it.id,
            it.title,
            it.kind.as_deref().unwrap_or("-"),
            it.year.as_deref().unwrap_or("-"),
            it.creator.as_deref().unwrap_or("-"),
            it.ruffle.as_deref().unwrap_or("-"),
            it.thumb.as_deref().unwrap_or("-")
        ));
    }
}

/// Downloads `url` into OUT_DIR/movies/`name` (not part of the pack); says what came.
fn fetch_to(client: &Client, dir: &Path, url: &str, referer: Option<&str>, name: &str, limit: u64) -> Result<PathBuf, String> {
    let movies = dir.join("movies");
    std::fs::create_dir_all(&movies).map_err(|e| e.to_string())?;
    let safe: String = name.rsplit('/').next().unwrap_or("movie.swf").chars().map(|c| if c.is_alphanumeric() || ".-_".contains(c) { c } else { '_' }).collect();
    let path: PathBuf = movies.join(&safe);
    let mut f = std::fs::File::create(&path).map_err(|e| e.to_string())?;
    let mut req = Request::new(url);
    req.limit = limit;
    req.referer = referer;
    let mut last = 0u64;
    let d = client
        .download(&req, &mut f, &mut |got, total| {
            if got - last >= 1 << 20 {
                last = got;
                out(&format!("     ... {got} of {}", total.map(|t| t.to_string()).unwrap_or_else(|| "?".into())));
            }
            true
        })
        .map_err(|e| e.to_string())?;
    drop(f);
    let head = std::fs::read(&path).map(|b| b.into_iter().take(16).collect::<Vec<u8>>()).unwrap_or_default();
    out(&format!("     {} bytes from {} ({}), Content-Type {:?}: {}", d.bytes, d.url, path.display(), d.head.content_type(), magic(&head)));
    Ok(path)
}

fn movie(client: &Client, dir: &Path, files: &r5net::Files) -> Result<(), String> {
    let path = fetch_to(client, dir, &files.main.url, files.referer.as_deref(), &files.main.name, 32 << 20)?;
    let head = std::fs::read(&path).map(|b| b.into_iter().take(16).collect::<Vec<u8>>()).unwrap_or_default();
    if !head.starts_with(b"FWS") && !head.starts_with(b"CWS") && !head.starts_with(b"ZWS") {
        return Err(format!("not a SWF: {}", magic(&head)));
    }
    Ok(())
}

/// A .zip's central directory: (name, packed size, size, method) of each entry. Nothing unpacked.
fn zip_listing(path: &Path) -> Result<Vec<(String, u64, u64, u16)>, String> {
    let b = std::fs::read(path).map_err(|e| e.to_string())?;
    let u16at = |i: usize| b.get(i..i + 2).map(|s| u16::from_le_bytes([s[0], s[1]]) as u64);
    let u32at = |i: usize| b.get(i..i + 4).map(|s| u32::from_le_bytes([s[0], s[1], s[2], s[3]]) as u64);
    // the end of central directory record: its signature, searched from the end (it may have a comment)
    let eocd = (0..b.len().saturating_sub(21)).rev().take(65_557).find(|&i| b[i..].starts_with(b"PK\x05\x06")).ok_or("not a .zip (no end of central directory)")?;
    let count = u16at(eocd + 10).ok_or("short .zip")?;
    let mut at = u32at(eocd + 16).ok_or("short .zip")? as usize;
    let mut out = Vec::new();
    for _ in 0..count {
        if !b.get(at..).is_some_and(|s| s.starts_with(b"PK\x01\x02")) {
            return Err(format!("bad central directory entry at {at}"));
        }
        let method = u16at(at + 10).ok_or("short entry")? as u16;
        let packed = u32at(at + 20).ok_or("short entry")?;
        let size = u32at(at + 24).ok_or("short entry")?;
        let (n, x, c) = (u16at(at + 28).ok_or("short entry")? as usize, u16at(at + 30).ok_or("short entry")? as usize, u16at(at + 32).ok_or("short entry")? as usize);
        let name = b.get(at + 46..at + 46 + n).map(|s| String::from_utf8_lossy(s).into_owned()).ok_or("short name")?;
        out.push((name, packed, size, method));
        at += 46 + n + x + c;
    }
    Ok(out)
}

fn picture(client: &Client, dir: &Path, url: &str, name: &str) -> Result<(), String> {
    let mut req = Request::new(url);
    req.limit = 8 << 20;
    req.accept = "image/*";
    let r = client.get_ok(&req).map_err(|e| e.to_string())?;
    out(&format!("     {} bytes from {}, Content-Type {:?}: {}", r.body.len(), r.url, r.head.content_type(), magic(&r.body)));
    save(dir, &format!("{name}.{}", ext_for(r.head.content_type())), &r.body);
    Ok(())
}

fn main() {
    let dir = PathBuf::from(std::env::args().nth(1).unwrap_or_else(|| "online-check".into()));
    std::fs::create_dir_all(&dir).expect("the output folder");
    let file = std::fs::File::create(dir.join("check.log")).expect("check.log");
    let _ = LOG.set(Logger { file: Mutex::new(file) });
    let _ = log::set_logger(LOG.get().unwrap());
    log::set_max_level(log::LevelFilter::Info);

    let ua = format!("Ruffle5/{} (PlayStation 5 Flash player; online-check; +https://github.com/TheUnwelcomed/RUFFLE5)", env!("CARGO_PKG_VERSION"));
    let mut client = Client::new(&ua);
    if let Some(p) = std::env::var("HTTPS_PROXY").ok().or_else(|| std::env::var("https_proxy").ok()).filter(|p| !p.is_empty()) {
        match client.set_proxy(&p) {
            Ok(()) => out(&format!("through the proxy {p}")),
            Err(e) => out(&format!("proxy {p} not used: {e}")),
        }
    }
    if let Ok(ca) = std::env::var("R5NET_EXTRA_CA") {
        match std::fs::read(&ca).map_err(|e| e.to_string()).and_then(|pem| client.add_roots_pem(&pem)) {
            Ok(n) => out(&format!("{n} more root certificate(s) from {ca}")),
            Err(e) => out(&format!("R5NET_EXTRA_CA {ca}: {e}")),
        }
    }
    out(&format!("online-check: {ua}; answers into {}", dir.display()));
    let mut t = Tally { ok: Vec::new(), failed: Vec::new() };

    // ---- the Internet Archive ----
    let browse = t.step("archive.org: the Flash collection, most downloaded first", || {
        let url = ia::search_url("", 1);
        out(&format!("     {url}"));
        let r = client.get_ok(&Request::new(&url)).map_err(|e| e.to_string())?;
        save(&dir, "ia-search-browse.json", &r.body);
        let v: serde_json::Value = serde_json::from_slice(&r.body).map_err(|e| e.to_string())?;
        let p = ia::parse_search(&v, 1).map_err(|e| e.to_string())?;
        out(&format!("     numFound {:?}, {} on page 1, more: {}", p.total, p.items.len(), p.more));
        show_items(&p.items, 12);
        if p.items.is_empty() {
            return Err("no items".into());
        }
        Ok(p)
    });
    t.step("archive.org: search \"duck life\"", || {
        let url = ia::search_url("duck life", 1);
        out(&format!("     {url}"));
        let r = client.get_ok(&Request::new(&url)).map_err(|e| e.to_string())?;
        save(&dir, "ia-search-duck-life.json", &r.body);
        let v: serde_json::Value = serde_json::from_slice(&r.body).map_err(|e| e.to_string())?;
        let p = ia::parse_search(&v, 1).map_err(|e| e.to_string())?;
        out(&format!("     numFound {:?}, {} on page 1", p.total, p.items.len()));
        show_items(&p.items, 8);
        Ok(())
    });
    if let Some(p) = &browse {
        let mut first_files = None;
        for it in p.items.iter().take(3) {
            let files = t.step(&format!("archive.org: the files of {}", it.id), || {
                let url = ia::metadata_url(&it.id);
                let r = client.get_ok(&Request::new(&url)).map_err(|e| e.to_string())?;
                save(&dir, &format!("ia-metadata-{}.json", it.id), &r.body);
                let v: serde_json::Value = serde_json::from_slice(&r.body).map_err(|e| e.to_string())?;
                let m = &v["metadata"];
                out(&format!("     emulator {:?}, emulator_start {:?}, mediatype {:?}", m["emulator"], m["emulator_start"], m["mediatype"]));
                if let Some(list) = v["files"].as_array() {
                    for f in list.iter().take(20) {
                        out(&format!("     file {:?} source {:?} format {:?} size {:?}", f["name"], f["source"], f["format"], f["size"]));
                    }
                }
                let files = ia::parse_files(&it.id, &v).map_err(|e| e.to_string())?;
                out(&format!("     the movie: {} ({}), {:?} bytes; {} file(s) beside it", files.main.name, files.note, files.main.size, files.extra.len()));
                Ok(files)
            });
            if first_files.is_none() {
                first_files = files;
            }
        }
        if let Some(files) = first_files {
            t.step("archive.org: download the first movie", || movie(&client, &dir, &files));
        }
        if let Some(it) = p.items.first() {
            t.step("archive.org: the first item's picture", || picture(&client, &dir, it.thumb.as_deref().unwrap_or(""), &format!("ia-thumb-{}", it.id)));
        }
    }

    // ---- Flash Museum ----
    let list = t.step("flashmuseum.net: the games list, page 1", || {
        let url = fm::list_url("", 1);
        let r = client.get_ok(&Request::new(&url)).map_err(|e| e.to_string())?;
        save(&dir, "fm-list-1.html", &r.body);
        let p = fm::parse_list(&r.text(), 1);
        out(&format!("     {} item(s), more: {}", p.items.len(), p.more));
        show_items(&p.items, 12);
        Ok(p)
    });
    let list2 = t.step("flashmuseum.net: the games list, page 2", || {
        let url = fm::list_url("", 2);
        let r = client.get_ok(&Request::new(&url)).map_err(|e| e.to_string())?;
        let p = fm::parse_list(&r.text(), 2);
        out(&format!("     {} item(s), more: {}", p.items.len(), p.more));
        show_items(&p.items, 4);
        Ok(p)
    });
    t.step("flashmuseum.net: search \"duck life\"", || {
        let url = fm::list_url("duck life", 1);
        out(&format!("     {url}"));
        let r = client.get(&Request::new(&url)).map_err(|e| e.to_string())?;
        out(&format!("     status {}", r.status));
        let p = fm::parse_list(&r.text(), 1);
        out(&format!("     {} item(s), more: {}", p.items.len(), p.more));
        show_items(&p.items, 8);
        Ok(())
    });
    let mut listed: Vec<r5net::Item> = Vec::new();
    for p in [&list, &list2].into_iter().flatten() {
        for it in &p.items {
            if !listed.iter().any(|l| l.id == it.id) {
                listed.push(it.clone());
            }
        }
    }
    // What kind of game each listed one is: its player page's settings (about 5 KB each).
    let mut a_zip: Option<(r5net::Item, fm::PlayerConfig)> = None;
    t.step(&format!("flashmuseum.net: the kind of each of the {} listed games (their player pages)", listed.len()), || {
        let mut kinds: std::collections::BTreeMap<String, Vec<String>> = Default::default();
        for it in &listed {
            let url = fm::player_url(it, None);
            let mut req = Request::new(&url);
            req.accept = "text/html";
            req.referer = Some(&it.page);
            let kind = match client.get(&req) {
                Ok(r) if r.status == 200 => match fm::parse_player_page(&r.text()) {
                    Some(c) => {
                        if c.is_zip() && a_zip.is_none() {
                            save(&dir, &format!("fm-player-{}.html", it.id), &r.body);
                            a_zip = Some((it.clone(), c.clone()));
                        }
                        format!("gameType {:?}", c.game_type)
                    }
                    None => {
                        save(&dir, &format!("fm-player-{}.html", it.id), &r.body);
                        "no settings in the page".to_string()
                    }
                },
                Ok(r) => format!("HTTP {}", r.status),
                Err(e) => format!("error: {e}"),
            };
            kinds.entry(kind).or_default().push(it.id.clone());
        }
        for (k, ids) in &kinds {
            out(&format!("     {k}: {} — {}", ids.len(), ids.join(", ")));
        }
        if listed.is_empty() {
            return Err("no games listed".into());
        }
        Ok(())
    });
    // Two games (the first listed, and Insectonator): the player's settings, the movie, the picture.
    let insectonator = r5net::Item { id: "insectonator-mt5".into(), title: "Insectonator".into(), kind: Some("Game".into()), page: "https://flashmuseum.net/game/insectonator-mt5/".into(), ..Default::default() };
    let mut fm_items: Vec<r5net::Item> = listed.iter().take(1).cloned().collect();
    if !fm_items.iter().any(|i| i.id == insectonator.id) {
        fm_items.push(insectonator);
    }
    for it in &fm_items {
        let files = t.step(&format!("flashmuseum.net: the player's settings for {}", it.id), || {
            let (c, from) = fm::player_config(&client, it).map_err(|e| e.to_string())?;
            out(&format!("     from {from}: gameType {:?}, rootURL {}, fileName {}, launchCommand {}, player {:?}", c.game_type, c.root_url, c.file_name, c.launch_command, c.player_url));
            let files = fm::files_for(it, &c, from).map_err(|e| e.to_string())?;
            out(&format!("     the movie: {} at {} (Referer {:?}; plays as {:?})", files.main.name, files.main.url, files.referer, files.address));
            Ok(files)
        });
        if let Some(files) = files {
            t.step(&format!("flashmuseum.net: download {}", it.id), || movie(&client, &dir, &files));
        }
        if let Some(thumb) = &it.thumb {
            t.step(&format!("flashmuseum.net: the picture of {}", it.id), || picture(&client, &dir, thumb, &format!("fm-thumb-{}", it.id)));
        }
    }
    // A GameZip, when one is listed: what is in its archive (names and sizes only).
    if let Some((it, c)) = &a_zip {
        t.step(&format!("flashmuseum.net: the archive of {} (a GameZip)", it.id), || {
            out(&format!("     rootURL {}, fileName {}, launchCommand {}", c.root_url, c.file_name, c.launch_command));
            let referer = c.player_url.clone().unwrap_or_else(|| fm::player_url(it, None));
            let path = fetch_to(&client, &dir, &c.movie_url(), Some(&referer), "gamezip.zip", 256 << 20)?;
            let entries = zip_listing(&path)?;
            out(&format!("     {} entries:", entries.len()));
            for (name, packed, size, method) in entries.iter().take(80) {
                out(&format!("       {name}  ({size} bytes, {packed} packed, method {method})"));
            }
            let _ = std::fs::remove_file(&path);
            Ok(())
        });
    } else {
        out("     (no GameZip among the listed games)");
    }

    // ---- Flashpoint Archive (1.000.028) ----
    let fp_page = |platform: &str, words: &str, file: &str| -> Result<r5net::Page, String> {
        let url = fp::search_url(platform, words, 1);
        out(&format!("     {url}"));
        let r = client.get_ok(&Request::new(&url)).map_err(|e| e.to_string())?;
        save(&dir, file, &r.body);
        let v: serde_json::Value = serde_json::from_slice(&r.body).map_err(|e| e.to_string())?;
        let p = fp::parse_search(&v, 1).map_err(|e| e.to_string())?;
        out(&format!("     total {:?}, {} on page 1, more: {}", p.total, p.items.len(), p.more));
        show_items(&p.items, 12);
        Ok(p)
    };
    let fp_all = t.step("flashpointarchive.org: every Shockwave entry, A to Z", || fp_page(fp::SHOCKWAVE, "", "fp-search-all.json"));
    let fp_totb = t.step("flashpointarchive.org: search \"trick or treat beat\"", || fp_page(fp::SHOCKWAVE, "trick or treat beat", "fp-search-totb.json"));
    // 1.000.033: the Flash entries, and how many of a page Flashpoint plays with Ruffle
    let fp_flash = t.step("flashpointarchive.org: every Flash entry, A to Z", || fp_page(fp::FLASH, "", "fp-search-flash.json"));
    let fp_flash_words = t.step("flashpointarchive.org: Flash entries matching \"bloxorz\"", || fp_page(fp::FLASH, "bloxorz", "fp-search-flash-bloxorz.json"));
    for (what, p) in [("A to Z", &fp_flash), ("bloxorz", &fp_flash_words)] {
        if let Some(p) = p {
            let marked = p.items.iter().filter(|it| it.ruffle.is_some()).count();
            out(&format!("     Flash, {what}: {marked} of {} on page 1 marked for Ruffle (ruffleSupport)", p.items.len()));
        }
    }
    // an entry with game data (Trick or Treat Beat!), and the first listed one without
    let mut fp_items: Vec<r5net::Item> = fp_totb.iter().flat_map(|p| p.items.iter().take(1).cloned()).collect();
    let no_game_data = |it: &r5net::Item| it.data.as_deref().and_then(|d| serde_json::from_str::<serde_json::Value>(d).ok()).is_some_and(|v| v["gameData"].as_array().map_or(true, |a| a.is_empty()));
    if let Some(it) = fp_all.iter().flat_map(|p| p.items.iter()).find(|it| no_game_data(it)) {
        fp_items.push(it.clone());
    }
    // Flash: the first entry marked for Ruffle with game data, and the first without game data
    let flash_items: Vec<&r5net::Item> = fp_flash.iter().chain(fp_flash_words.iter()).flat_map(|p| p.items.iter()).collect();
    if let Some(it) = flash_items.iter().find(|it| it.ruffle.is_some() && !no_game_data(it)) {
        fp_items.push((*it).clone());
    }
    if let Some(it) = flash_items.iter().find(|it| no_game_data(it)) {
        fp_items.push((*it).clone());
    }
    for it in &fp_items {
        let files = t.step(&format!("flashpointarchive.org: the files of {} ({})", it.title, it.id), || {
            let f = fp::files(it).map_err(|e| e.to_string())?;
            out(&format!("     {}: {} at {}", f.note, f.main.name, f.main.url));
            out(&format!("     address {:?}; launch {:?}; beside {:?}", f.address, f.launch, f.beside));
            if let Some(a) = &f.archive {
                out(&format!("     SHA-256 {}, mirrors {:?}, the movie {}", a.sha256, a.mirrors, a.movie));
            }
            Ok(f)
        });
        match files {
            Some(files) if files.archive.is_some() => {
                let a = files.archive.clone().unwrap();
                t.step(&format!("flashpointarchive.org: the game data of {} (download, SHA-256, listing, the movie)", it.id), || {
                    let mut got = Err(String::from("no address"));
                    for url in std::iter::once(&files.main.url).chain(a.mirrors.iter()) {
                        got = fetch_to(&client, &dir, url, None, &files.main.name, 1 << 30);
                        match &got {
                            Ok(_) => break,
                            Err(e) => out(&format!("     {url}: {e}")),
                        }
                    }
                    let path = got?;
                    let sum = r5net::sha256_file(&path).map_err(|e| e.to_string())?;
                    out(&format!("     SHA-256 {sum}: {}", if sum == a.sha256 { "as listed" } else { "NOT as listed" }));
                    let mut z = r5net::zip::Zip::open(&path)?;
                    out(&format!("     {} entries:", z.entries.len()));
                    for e in z.entries.iter().take(80) {
                        out(&format!("       {}  ({} bytes, {} packed, method {})", e.name, e.size, e.compressed, e.method));
                    }
                    let names: Vec<String> = z.entries.iter().map(|e| e.name.clone()).collect();
                    let i = fp::pick_movie(&names, &a.movie)?;
                    let mut movie = Vec::new();
                    z.extract(i, &mut movie, 1 << 30)?;
                    out(&format!("     the movie: {} ({} bytes, CRC-32 as the zip says): {}", names[i], movie.len(), magic(&movie[..movie.len().min(16)])));
                    let _ = std::fs::remove_file(&path);
                    if sum != a.sha256 {
                        return Err(format!("SHA-256 {sum}, listed {}", a.sha256));
                    }
                    if !is_movie_bytes(&names[i], &movie) {
                        return Err(format!("{} is not the movie its name says: {}", names[i], magic(&movie[..movie.len().min(16)])));
                    }
                    Ok(())
                });
            }
            Some(files) => {
                t.step(&format!("flashpointarchive.org: the movie of {} from the legacy server", it.id), || {
                    let path = fetch_to(&client, &dir, &files.main.url, None, &files.main.name, 64 << 20)?;
                    let head = std::fs::read(&path).map(|b| b.into_iter().take(16).collect::<Vec<u8>>()).unwrap_or_default();
                    if !is_movie_bytes(&files.main.name, &head) {
                        return Err(format!("not the movie its name says: {}", magic(&head)));
                    }
                    Ok(())
                });
            }
            None => {}
        }
        if let Some(thumb) = &it.thumb {
            t.step(&format!("flashpointarchive.org: the picture of {}", it.id), || picture(&client, &dir, thumb, &format!("fp-logo-{}", it.id)));
        }
    }

    out(&format!("online-check: {} ok, {} failed", t.ok.len(), t.failed.len()));
    for f in &t.failed {
        out(&format!("     failed: {f}"));
    }
}
