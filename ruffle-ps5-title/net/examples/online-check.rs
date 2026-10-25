//! online-check — from a PC, ask both online sources what the console's Online tab asks them: a
//! list, a search, an item's files, a movie, a picture; over the same HTTPS code (rustls with
//! RustCrypto). tools/online-check.sh runs it on the build server.
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

use r5net::{fm, ia, Client, Request, Source};

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

fn show_items(items: &[r5net::Item], n: usize) {
    for it in items.iter().take(n) {
        out(&format!(
            "     {} | {} | {} | {} | {} | thumb {}",
            it.id,
            it.title,
            it.kind.as_deref().unwrap_or("-"),
            it.year.as_deref().unwrap_or("-"),
            it.creator.as_deref().unwrap_or("-"),
            it.thumb.as_deref().unwrap_or("-")
        ));
    }
}

fn movie(client: &Client, dir: &Path, files: &r5net::Files) -> Result<(), String> {
    let movies = dir.join("movies");
    std::fs::create_dir_all(&movies).map_err(|e| e.to_string())?;
    let safe: String = files.main.name.rsplit('/').next().unwrap_or("movie.swf").chars().map(|c| if c.is_alphanumeric() || ".-_".contains(c) { c } else { '_' }).collect();
    let path: PathBuf = movies.join(&safe);
    let mut f = std::fs::File::create(&path).map_err(|e| e.to_string())?;
    let mut req = Request::new(&files.main.url);
    req.limit = 32 << 20;
    req.referer = files.referer.as_deref();
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
    if !head.starts_with(b"FWS") && !head.starts_with(b"CWS") && !head.starts_with(b"ZWS") {
        return Err(format!("not a SWF: {}", magic(&head)));
    }
    Ok(())
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
    t.step("flashmuseum.net: the games list, page 2", || {
        let url = fm::list_url("", 2);
        let r = client.get_ok(&Request::new(&url)).map_err(|e| e.to_string())?;
        save(&dir, "fm-list-2.html", &r.body);
        let p = fm::parse_list(&r.text(), 2);
        out(&format!("     {} item(s), more: {}", p.items.len(), p.more));
        show_items(&p.items, 4);
        Ok(())
    });
    t.step("flashmuseum.net: search \"duck life\"", || {
        let url = fm::list_url("duck life", 1);
        out(&format!("     {url}"));
        let r = client.get(&Request::new(&url)).map_err(|e| e.to_string())?;
        out(&format!("     status {}", r.status));
        save(&dir, "fm-search-duck-life.html", &r.body);
        let p = fm::parse_list(&r.text(), 1);
        out(&format!("     {} item(s), more: {}", p.items.len(), p.more));
        show_items(&p.items, 8);
        Ok(())
    });
    for probe in ["/wp-json/", "/wp-json/wp/v2/types", "/wp-json/wp/v2/game?per_page=3", "/wp-json/wp/v2/search?search=duck%20life&per_page=5", "/wp-sitemap.xml", "/sitemap_index.xml", "/robots.txt"] {
        t.step(&format!("flashmuseum.net{probe} (what else the site offers)"), || {
            let url = format!("{}{probe}", fm::SITE);
            let mut req = Request::new(&url);
            req.limit = 4 << 20;
            let r = client.get(&req).map_err(|e| e.to_string())?;
            out(&format!("     status {}, Content-Type {:?}, {} bytes", r.status, r.head.content_type(), r.body.len()));
            let name: String = probe.trim_matches('/').chars().map(|c| if c.is_alphanumeric() || c == '.' { c } else { '_' }).collect();
            save(&dir, &format!("fm-probe-{name}.txt"), &r.body);
            Ok(())
        });
    }
    let first = list.as_ref().and_then(|p| p.items.first().cloned()).unwrap_or_else(|| r5net::Item {
        id: "insectonator-mt5".into(),
        title: "Insectonator".into(),
        kind: Some("Game".into()),
        page: "https://flashmuseum.net/game/insectonator-mt5/".into(),
        ..Default::default()
    });
    let mut fm_items = vec![first];
    if fm_items[0].id != "insectonator-mt5" {
        fm_items.push(r5net::Item { id: "insectonator-mt5".into(), title: "Insectonator".into(), kind: Some("Game".into()), page: "https://flashmuseum.net/game/insectonator-mt5/".into(), ..Default::default() });
    }
    for it in &fm_items {
        t.step(&format!("flashmuseum.net: the pages of {}", it.id), || {
            let r = client.get_ok(&Request::new(&it.page)).map_err(|e| e.to_string())?;
            save(&dir, &format!("fm-item-{}.html", it.id), &r.body);
            let info = fm::parse_item_page(&r.text());
            out(&format!("     player {:?}, picture {:?}, title {:?}", info.player, info.picture, info.title));
            let player = info.player.unwrap_or_else(|| format!("https://player.flashmuseum.net/game/{}/", it.id));
            let mut req = Request::new(&player);
            req.referer = Some(&it.page);
            let pr = client.get(&req).map_err(|e| e.to_string())?;
            out(&format!("     player page: status {}, {} bytes, Content-Type {:?}", pr.status, pr.body.len(), pr.head.content_type()));
            save(&dir, &format!("fm-player-{}.html", it.id), &pr.body);
            let base = r5net::url::Url::parse(&pr.url).map_err(|e| e.to_string())?;
            let swfs = fm::swf_addresses(&pr.text(), &base);
            out(&format!("     .swf addresses in it: {swfs:?}"));
            Ok(())
        });
        let files = t.step(&format!("flashmuseum.net: the files of {}", it.id), || Source::FlashMuseum.files(&client, it).map_err(|e| e.to_string()));
        if let Some(files) = files {
            out(&format!("     the movie: {} at {} ({})", files.main.name, files.main.url, files.note));
            t.step(&format!("flashmuseum.net: download {}", it.id), || movie(&client, &dir, &files));
        }
        if let Some(thumb) = &it.thumb {
            t.step(&format!("flashmuseum.net: the picture of {}", it.id), || picture(&client, &dir, thumb, &format!("fm-thumb-{}", it.id)));
        }
    }

    out(&format!("online-check: {} ok, {} failed", t.ok.len(), t.failed.len()));
    for f in &t.failed {
        out(&format!("     failed: {f}"));
    }
}
