//! Flashpoint Archive (flashpointarchive.org), 1.000.028: its Shockwave games, through the
//! archive's public API, as the archive answered the probe of 2026-10-24 (fp-probe.py, run from the
//! build server) and the download check after it:
//!   * search: `/api/search?query=…&limit=…&page=…&sort=title` -> `{total, pages, page, games[]}`;
//!     the query is the Flashpoint launcher's language: `platform:Shockwave` lists every Shockwave
//!     entry (7789 then), `!Shockwave words` the Shockwave entries matching the words;
//!   * an entry's game data (`gameData[]`: `dateAdded`, `size`, `sha256`, `launchCommand`) is a zip
//!     at `download.flashpointarchive.org/gib-roms/Games/<id>-<ms>.zip` (the same file at
//!     download.unstable.life), `<ms>` being `dateAdded` in milliseconds since 1970 as JavaScript's
//!     Date reads it — digits below the millisecond dropped: `2025-03-08T02:02:29.043736Z` ->
//!     1741399349043 (rounding to …044 is a 404). Its SHA-256 is the entry's. It holds
//!     `content.json` and `content/<host>/<path>`: the files at the addresses the game was
//!     published at (Trick or Treat Beat!: `content/www.cartoonnetwork.com/games/cc/
//!     trickortreatbeat/game.dcr`, its `sound.cct` beside it, and two other versions);
//!   * an entry without game data ("legacy"): its files on the legacy server,
//!     `infinity.flashpointarchive.org/Flashpoint/Legacy/htdocs/<host>/<path>`;
//!   * its picture: `infinity.flashpointarchive.org/images/<logoPath>` (PNG);
//!   * how it starts: a Shockwave Projector command — the movie's address, then flags
//!     (`--forceTheExitLock 0`, `--do "…"`) that Ruffle5 hands DirPlayer (online.txt `launch`).
// SPDX-License-Identifier: GPL-3.0-or-later

use serde_json::Value;

use crate::http::{Client, Error};
use crate::source::{clip, html_text, search_words, Archive, Files, Item, Page, RemoteFile};
use crate::url::{decode_percent, encode_component, encode_path};

/// Results per page (the API takes up to 100).
pub const ROWS: u32 = 48;
pub const API: &str = "https://flashpointarchive.org/api/search";
/// Where game data zips are, in the order tried.
pub const GAME_DATA: [&str; 2] = ["https://download.flashpointarchive.org/gib-roms/Games/", "https://download.unstable.life/gib-roms/Games/"];
pub const IMAGES: &str = "https://infinity.flashpointarchive.org/images/";
pub const LEGACY: &str = "https://infinity.flashpointarchive.org/Flashpoint/Legacy/htdocs/";

/// The search: every Shockwave entry, or those matching the words.
pub fn query(words: &str) -> String {
    let w = search_words(words);
    if w.is_empty() {
        "platform:Shockwave".to_string()
    } else {
        format!("!Shockwave {}", w.join(" "))
    }
}

pub fn search_url(words: &str, page: u32) -> String {
    format!("{API}?query={}&limit={ROWS}&page={}&sort=title", encode_component(&query(words)), page.max(1))
}

/// The API's answer for one entry (online.txt `page`).
pub fn entry_url(id: &str) -> String {
    format!("{API}?query={}", encode_component(&format!("id:{id}")))
}

fn text(v: &Value, key: &str) -> Option<String> {
    v.get(key).and_then(Value::as_str).map(str::trim).filter(|s| !s.is_empty()).map(str::to_string)
}

pub fn parse_search(v: &Value, page: u32) -> Result<Page, Error> {
    if let Some(e) = v.get("error").and_then(Value::as_str) {
        return Err(Error::Other(format!("Flashpoint search: {e}")));
    }
    let games = v.get("games").and_then(Value::as_array).ok_or_else(|| Error::Other("Flashpoint search: no \"games\" in the answer".into()))?;
    let total = v.get("total").and_then(Value::as_u64);
    let pages = v.get("pages").and_then(Value::as_u64);
    let mut items = Vec::new();
    for g in games {
        let Some(id) = text(g, "id") else {
            continue;
        };
        let year = text(g, "releaseDate").map(|d| d.chars().take(4).collect::<String>()).filter(|y| y.len() == 4 && y.chars().all(|c| c.is_ascii_digit()));
        let kind = match text(g, "library").as_deref() {
            Some("arcade") => Some("Game".to_string()),
            Some("theatre") => Some("Animation".to_string()),
            _ => None,
        };
        // what files() needs: the game data and the launch command
        let data = serde_json::json!({
            "gameData": g.get("gameData").cloned().unwrap_or(Value::Array(Vec::new())),
            "legacyLaunchCommand": g.get("legacyLaunchCommand").cloned().unwrap_or(Value::Null),
        });
        items.push(Item {
            title: text(g, "title").map(|t| html_text(&t)).unwrap_or_else(|| id.clone()),
            creator: text(g, "developer").or_else(|| text(g, "publisher")).map(|t| clip(&html_text(&t), 80)),
            year,
            description: text(g, "originalDescription").map(|t| clip(&html_text(&t), 600)).filter(|t| !t.is_empty()),
            kind,
            thumb: text(g, "logoPath").map(|p| format!("{IMAGES}{}", encode_path(&p))),
            page: entry_url(&id),
            downloads: None,
            data: Some(data.to_string()),
            id,
        });
    }
    let more = match pages {
        Some(p) => (page.max(1) as u64) < p && !games.is_empty(),
        None => games.len() as u32 == ROWS,
    };
    Ok(Page { items, total, more })
}

pub fn search(client: &Client, words: &str, page: u32) -> Result<Page, Error> {
    let v = client.get_json(&search_url(words, page))?;
    parse_search(&v, page)
}

/// Milliseconds since 1970 of an ISO date and time as JavaScript's Date reads it: digits below the
/// millisecond dropped, `Z` or `±hh:mm` for the zone (none: UTC, as every date the API gave was).
pub fn js_date_ms(s: &str) -> Option<i64> {
    let s = s.trim();
    let (date, time) = s.split_once(['T', ' '])?;
    let mut d = date.split('-');
    let (y, mo, da): (i64, i64, i64) = (d.next()?.parse().ok()?, d.next()?.parse().ok()?, d.next()?.parse().ok()?);
    let (clock, offset_min) = if let Some(c) = time.strip_suffix(['Z', 'z']) {
        (c, 0)
    } else if let Some(i) = time.rfind(['+', '-']).filter(|&i| i >= 8) {
        let (c, z) = time.split_at(i);
        let sign = if z.starts_with('-') { -1 } else { 1 };
        let (hh, mm) = z[1..].split_once(':').unwrap_or((&z[1..3.min(z.len())], &z[3.min(z.len())..]));
        (c, sign * (hh.parse::<i64>().ok()? * 60 + mm.parse::<i64>().unwrap_or(0)))
    } else {
        (time, 0)
    };
    let (hms, frac) = clock.split_once('.').unwrap_or((clock, ""));
    let mut t = hms.split(':');
    let (h, mi, se): (i64, i64, i64) = (t.next()?.parse().ok()?, t.next()?.parse().ok()?, t.next().unwrap_or("0").parse().ok()?);
    let ms: i64 = if frac.is_empty() {
        0
    } else {
        if !frac.chars().all(|c| c.is_ascii_digit()) {
            return None;
        }
        format!("{:0<3}", &frac[..frac.len().min(3)]).parse().ok()?
    };
    // days from 1970-01-01 (H. Hinnant's days_from_civil)
    let yy = if mo <= 2 { y - 1 } else { y };
    let era = if yy >= 0 { yy } else { yy - 399 } / 400;
    let yoe = yy - era * 400;
    let doy = (153 * (if mo > 2 { mo - 3 } else { mo + 9 }) + 2) / 5 + da - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    let days = era * 146097 + doe - 719468;
    Some((((days * 24 + h) * 60 + mi) * 60 + se) * 1000 + ms - offset_min * 60_000)
}

/// The movie a Shockwave Projector command starts: its first argument that is not a flag or a
/// flag's value, read as DirPlayer's launch-command parser reads it (an `…SPR.exe` in front is
/// dropped; `"…"` quotes a token, `\"` a quote in it).
pub fn launch_movie(cmd: &str) -> Option<String> {
    let mut rest = cmd.trim_start();
    if let Some(first) = rest.split_whitespace().next() {
        let lower = first.to_ascii_lowercase();
        if let Some(stem) = lower.strip_suffix(".exe") {
            let spr = stem.rfind("spr").is_some_and(|i| stem[i + 3..].chars().all(|c| c.is_ascii_alphabetic()));
            if spr && rest.len() > first.len() {
                rest = rest[first.len()..].trim_start();
            }
        }
    }
    let tokens = tokenize(rest);
    let mut i = 0;
    while i < tokens.len() {
        let t = &tokens[i];
        let Some(flag) = t.strip_prefix("--") else {
            return Some(t.clone());
        };
        let f = flag.to_ascii_lowercase();
        let takes = match f.as_str() {
            "setexternalparam" => 2,
            "do" | "dobefore" | "go" | "preload" | "newscriptname" | "newscripttext" | "newscripttype" | "trace" | "traceload" | "tracelogfile" => 1,
            "setthemoviepath" | "setthemoviename" | "settheenvironment_shockmachineversion" | "settheplatform" | "settherunmode"
            | "settheenvironment_productbuildversion" | "settheproductversion" | "settheenvironment_osversion" => 1,
            "settheenvironment_shockmachine" | "setthemachinetype" | "forcetheexitlock" | "forcethesafeplayer" => 1,
            "disablegotonetmovie" | "disablegotonetpage" | "bugfixshockwave3dbaddriverlist" | "versionstrict" | "nodirectx7" => 0,
            // an unknown flag takes the next token unless that is a flag
            _ => usize::from(tokens.get(i + 1).is_some_and(|n| !n.starts_with("--"))),
        };
        i += 1 + takes;
    }
    None
}

fn tokenize(cmd: &str) -> Vec<String> {
    let chars: Vec<char> = cmd.chars().collect();
    let mut out = Vec::new();
    let mut i = 0;
    while i < chars.len() {
        while i < chars.len() && chars[i].is_whitespace() {
            i += 1;
        }
        if i >= chars.len() {
            break;
        }
        let mut tok = String::new();
        if chars[i] == '"' {
            i += 1;
            while i < chars.len() {
                if chars[i] == '\\' && chars.get(i + 1) == Some(&'"') {
                    tok.push('"');
                    i += 2;
                    continue;
                }
                if chars[i] == '"' {
                    i += 1;
                    break;
                }
                tok.push(chars[i]);
                i += 1;
            }
        } else {
            while i < chars.len() && !chars[i].is_whitespace() {
                tok.push(chars[i]);
                i += 1;
            }
        }
        out.push(tok);
    }
    out
}

/// `http://host/a/b.dcr?x` -> ("host", "/a/b.dcr"), the path as written (still percent-encoded).
fn host_and_path(url: &str) -> Option<(String, String)> {
    let rest = url.strip_prefix("http://").or_else(|| url.strip_prefix("https://"))?;
    let rest = rest.split(['?', '#']).next().unwrap_or(rest);
    let (host, path) = match rest.find('/') {
        Some(i) => (&rest[..i], &rest[i..]),
        None => (rest, "/"),
    };
    (!host.is_empty()).then(|| (host.to_string(), path.to_string()))
}

/// Where a game data zip keeps the file published at `url`: `content/<host>/<path>`.
pub fn zip_path(url: &str) -> Option<String> {
    let (host, path) = host_and_path(url)?;
    Some(format!("content/{host}{}", decode_percent(&path)))
}

pub fn is_director_name(name: &str) -> bool {
    let n = name.to_ascii_lowercase();
    [".dcr", ".dir", ".dxr"].iter().any(|e| n.ends_with(e))
}

/// The movie in a game data zip: the file at the launch address (`wanted`), else — the address
/// being a web page or not in the zip — the zip's one Director movie. Returns its index.
pub fn pick_movie(names: &[String], wanted: &str) -> Result<usize, String> {
    let norm = |s: &str| s.replace('\\', "/").to_lowercase();
    let want = norm(wanted);
    if let Some(i) = names.iter().position(|n| norm(n) == want) {
        return Ok(i);
    }
    let movies: Vec<usize> = (0..names.len()).filter(|&i| is_director_name(&names[i]) && norm(&names[i]).starts_with("content/")).collect();
    match movies.len() {
        1 => Ok(movies[0]),
        0 => Err(format!("its game data has no Director movie (the entry starts at {wanted})")),
        n => {
            let shown: Vec<&str> = movies.iter().take(4).map(|&i| names[i].as_str()).collect();
            Err(format!("{wanted} is not in its game data, which has {n} movies ({}{})", shown.join(", "), if n > 4 { ", ..." } else { "" }))
        }
    }
}

/// The files of a listed entry: its newest game data's zip, else (no game data) the movie on the
/// legacy server, with the folder there for the casts it loads.
pub fn files(item: &Item) -> Result<Files, Error> {
    let data: Value = item
        .data
        .as_deref()
        .and_then(|d| serde_json::from_str(d).ok())
        .ok_or_else(|| Error::Other(format!("Flashpoint {}: listed without its game data; search again", item.id)))?;
    let legacy_cmd = data.get("legacyLaunchCommand").and_then(Value::as_str).map(str::trim).unwrap_or("").to_string();
    let newest = data
        .get("gameData")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(|gd| Some((js_date_ms(gd.get("dateAdded")?.as_str()?)?, gd)))
        .max_by_key(|(ms, _)| *ms);
    if let Some((ms, gd)) = newest {
        let cmd = gd.get("launchCommand").and_then(Value::as_str).map(str::trim).filter(|c| !c.is_empty()).map(str::to_string).unwrap_or(legacy_cmd);
        let movie_url = launch_movie(&cmd).ok_or_else(|| Error::Other(format!("Flashpoint {}: no movie in its launch command {cmd:?}", item.id)))?;
        let movie = zip_path(&movie_url).ok_or_else(|| Error::Other(format!("Flashpoint {}: its movie {movie_url:?} is not a web address", item.id)))?;
        let sha256 = gd.get("sha256").and_then(Value::as_str).unwrap_or("").to_ascii_lowercase();
        if sha256.len() != 64 {
            return Err(Error::Other(format!("Flashpoint {}: its game data has no SHA-256", item.id)));
        }
        let name = format!("{}-{ms}.zip", item.id);
        let size = gd.get("size").and_then(Value::as_u64);
        let date = gd.get("dateAdded").and_then(Value::as_str).unwrap_or("");
        return Ok(Files {
            main: RemoteFile { name: name.clone(), url: format!("{}{name}", GAME_DATA[0]), size },
            extra: Vec::new(),
            referer: None,
            note: format!("game data of {date} ({} bytes), {movie}", size.unwrap_or(0)),
            address: Some(movie_url),
            archive: Some(Archive { sha256, mirrors: GAME_DATA[1..].iter().map(|b| format!("{b}{name}")).collect(), movie }),
            launch: Some(cmd),
            beside: None,
        });
    }
    let cmd = legacy_cmd;
    let movie_url = launch_movie(&cmd).ok_or_else(|| Error::Other(format!("Flashpoint {}: no game data and no launch command", item.id)))?;
    let (host, path) = host_and_path(&movie_url).ok_or_else(|| Error::Other(format!("Flashpoint {}: its movie {movie_url:?} is not a web address", item.id)))?;
    let decoded = decode_percent(&path);
    let file = decoded.rsplit('/').next().unwrap_or("").to_string();
    if !is_director_name(&file) {
        return Err(Error::Other(format!("Flashpoint {}: it starts at {movie_url}, not a Director movie, and has no game data", item.id)));
    }
    let dir = &decoded[..decoded.len() - file.len()];
    Ok(Files {
        main: RemoteFile { name: file, url: format!("{LEGACY}{host}{}", encode_path(&decoded)), size: None },
        extra: Vec::new(),
        referer: None,
        note: "no game data: the movie from Flashpoint's legacy server".to_string(),
        address: Some(movie_url.clone()),
        archive: None,
        launch: Some(cmd),
        beside: Some(format!("{LEGACY}{host}{}", encode_path(dir))),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// flashpointarchive.org's answer to `query=trick or treat beat&limit=20`, 2026-10-24.
    const TOTB: &str = r#"{"total":1,"pages":1,"page":1,"games":[{"id":"29207f98-538a-4597-8522-3777341a9376","title":"Trick or Treat Beat!","alternateTitles":"Doce ou Travessura","series":"","developer":"This Is Pop","publisher":"Cartoon Network","primaryPlatform":"Shockwave","platforms":["Shockwave"],"library":"arcade","tags":["Puzzle","Pixel","Halloween","Johnny Bravo","Ed, Edd n Eddy","Dexter's Laboratory","Sheep in the Big City","Officially Licensed"],"playMode":"Single Player","status":"Playable","notes":"","source":"CartoonNetwork.com","releaseDate":"2002","version":"","originalDescription":"Go trick-or-treating with Dexter and other cartoon stars!","language":"en; es; pt","ruffleSupport":"","dateAdded":"2018-08-04T04:32:29.552Z","dateModified":"2025-09-12T11:14:04.104037Z","legacyApplicationPath":"FPSoftware\\Shockwave\\PJ851\\SPR.exe","legacyLaunchCommand":"http://www.cartoonnetwork.com/games/cc/trickortreatbeat/game.dcr","logoPath":"Logos/29/20/29207f98-538a-4597-8522-3777341a9376.png","screenshotPath":"Screenshots/29/20/29207f98-538a-4597-8522-3777341a9376.png","gameData":[{"dateAdded":"2025-03-08T02:02:29.043736Z","size":2718096,"sha256":"5016dba0bf2a3141489fdeeacd82d63329ecc662bfcc0617dcc186642abeda5c","crc32":0,"applicationPath":"FPSoftware\\Shockwave\\PJ851\\SPR.exe","launchCommand":"http://www.cartoonnetwork.com/games/cc/trickortreatbeat/game.dcr"}]}]}"#;

    #[test]
    fn a_listing_and_its_game_data() {
        let v: Value = serde_json::from_str(TOTB).unwrap();
        let page = parse_search(&v, 1).unwrap();
        assert_eq!((page.total, page.more, page.items.len()), (Some(1), false, 1));
        let it = &page.items[0];
        assert_eq!(it.title, "Trick or Treat Beat!");
        assert_eq!(it.creator.as_deref(), Some("This Is Pop"));
        assert_eq!(it.year.as_deref(), Some("2002"));
        assert_eq!(it.kind.as_deref(), Some("Game"));
        assert_eq!(it.thumb.as_deref(), Some("https://infinity.flashpointarchive.org/images/Logos/29/20/29207f98-538a-4597-8522-3777341a9376.png"));
        let f = files(it).unwrap();
        // the name the download check found (…043; …044 was a 404)
        assert_eq!(f.main.url, "https://download.flashpointarchive.org/gib-roms/Games/29207f98-538a-4597-8522-3777341a9376-1741399349043.zip");
        assert_eq!(f.main.size, Some(2718096));
        let a = f.archive.unwrap();
        assert_eq!(a.sha256, "5016dba0bf2a3141489fdeeacd82d63329ecc662bfcc0617dcc186642abeda5c");
        assert_eq!(a.mirrors, ["https://download.unstable.life/gib-roms/Games/29207f98-538a-4597-8522-3777341a9376-1741399349043.zip"]);
        assert_eq!(a.movie, "content/www.cartoonnetwork.com/games/cc/trickortreatbeat/game.dcr");
        assert_eq!(f.address.as_deref(), Some("http://www.cartoonnetwork.com/games/cc/trickortreatbeat/game.dcr"));
        // the zip's listing, as the download check printed it
        let names: Vec<String> = [
            "content.json",
            "content/www.cartoonnetwork.com.br/play/treaten/game.dcr",
            "content/www.cartoonnetwork.com.br/play/treaten/sound.cct",
            "content/www.cartoonnetwork.com/games/cc/trickortreatbeat/game.dcr",
            "content/www.cartoonnetwork.com/games/cc/trickortreatbeat/sound.cct",
            "content/www.cartoonnetwork.com/games/cc/trickortreatbeat/sp/game.dcr",
        ]
        .map(String::from)
        .to_vec();
        assert_eq!(pick_movie(&names, &a.movie), Ok(3));
        assert!(pick_movie(&names, "content/awesomehouseparty.com/index.html").unwrap_err().contains("3 movies"));
        assert_eq!(pick_movie(&names[..3], "content/x/index.html"), Ok(1));
    }

    #[test]
    fn dates_as_javascript_reads_them() {
        assert_eq!(js_date_ms("2025-03-08T02:02:29.043736Z"), Some(1741399349043));
        assert_eq!(js_date_ms("2021-04-18T03:28:06.41Z"), Some(1618716486410));
        assert_eq!(js_date_ms("1970-01-01T00:00:00Z"), Some(0));
        assert_eq!(js_date_ms("1970-01-01T01:00:00+01:00"), Some(0));
        assert_eq!(js_date_ms("1969-12-31T23:59:59.999Z"), Some(-1));
        assert_eq!(js_date_ms("nonsense"), None);
    }

    #[test]
    fn launch_commands() {
        assert_eq!(launch_movie("http://www.acclaim.com/movies/galaxies1.dcr\t").as_deref(), Some("http://www.acclaim.com/movies/galaxies1.dcr"));
        assert_eq!(launch_movie("http://ht.cdn.turner.com/toon/games/ben10/battleready/game.dcr --setTheRunMode Plugin --forceTheExitLock 0").as_deref(), Some("http://ht.cdn.turner.com/toon/games/ben10/battleready/game.dcr"));
        assert_eq!(launch_movie(r#"FPSoftware\Shockwave\PJ851\SPR.exe "http://a.com/b c.dcr" --do "x""#).as_deref(), Some("http://a.com/b c.dcr"));
        assert_eq!(launch_movie(r#"--do "member('u').text = 'x'" --go 3 http://a.com/w.dcr"#).as_deref(), Some("http://a.com/w.dcr"));
        assert_eq!(launch_movie("--disableGoToNetPage"), None);
        assert_eq!(zip_path("http://localsw/iceboxCN/ScratchGame/Crossover%20-%20Aka%20Scratch%20Game.dcr").as_deref(), Some("content/localsw/iceboxCN/ScratchGame/Crossover - Aka Scratch Game.dcr"));
        assert_eq!(zip_path("https://a.b/c.dcr?v=2").as_deref(), Some("content/a.b/c.dcr"));
    }

    #[test]
    fn an_entry_without_game_data_comes_from_the_legacy_server() {
        let it = Item { id: "c586f6ca".into(), data: Some(r#"{"gameData":[],"legacyLaunchCommand":"http://www.acclaim.com/movies/galaxies1.dcr\t"}"#.into()), ..Item::default() };
        let f = files(&it).unwrap();
        assert_eq!(f.main.url, "https://infinity.flashpointarchive.org/Flashpoint/Legacy/htdocs/www.acclaim.com/movies/galaxies1.dcr");
        assert_eq!(f.main.name, "galaxies1.dcr");
        assert_eq!(f.beside.as_deref(), Some("https://infinity.flashpointarchive.org/Flashpoint/Legacy/htdocs/www.acclaim.com/movies/"));
        assert!(f.archive.is_none());
        let web = Item { id: "x".into(), data: Some(r#"{"gameData":[],"legacyLaunchCommand":"http://awesomehouseparty.com/index.html"}"#.into()), ..Item::default() };
        assert!(files(&web).is_err());
        assert_eq!(query("Duck  Life!"), "!Shockwave duck life");
        assert_eq!(query(""), "platform:Shockwave");
        assert_eq!(search_url("", 2), "https://flashpointarchive.org/api/search?query=platform%3AShockwave&limit=48&page=2&sort=title");
    }
}
