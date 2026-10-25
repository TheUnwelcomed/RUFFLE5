//! The Internet Archive's Flash collection (archive.org, collection `softwarelibrary_flash`, the
//! movies archive.org itself plays with Ruffle), through its public APIs:
//!   * search: `advancedsearch.php` (Lucene query, `fl[]` fields, `sort[]`, `rows`, `page`,
//!     `output=json`) -> `response.numFound` and `response.docs[]`;
//!   * an item's files: `/metadata/{identifier}` -> `files[]` (name, source, format, size) and
//!     `metadata` (title, creator, `emulator_start`: the file archive.org's player starts);
//!   * a file: `/download/{identifier}/{name}` (redirects to the server holding it);
//!   * a picture: `/services/img/{identifier}` (the item's thumbnail).
//!
//! 1.000.028, from the answers archive.org gave the online check (2026-10-24): the collection
//! also holds its sub-collections (softwarelibrary_flash_games, _kidzone, _nick, ... — mediatype
//! "collection", no .swf of their own), and by downloads they are the whole first page of 48. The
//! search leaves out items whose mediatype is collection.
// SPDX-License-Identifier: GPL-3.0-or-later

use serde_json::Value;

use crate::http::{Client, Error};
use crate::source::{clip, html_text, search_words, Files, Item, Page, RemoteFile};
use crate::url::{encode_component, encode_path};

pub const COLLECTION: &str = "softwarelibrary_flash";
/// Results per page.
pub const ROWS: u32 = 48;
/// Files beside the movie that are fetched with it: what a movie may load by a relative name.
const EXTRA_EXT: [&str; 12] = ["swf", "xml", "json", "txt", "mp3", "flv", "png", "jpg", "jpeg", "gif", "dat", "bin"];
const EXTRA_MAX_FILES: usize = 64;
const EXTRA_MAX_BYTES: u64 = 64 << 20;

/// The Lucene query: the collection without its sub-collections, and every search word.
pub fn query(words: &str) -> String {
    let w = search_words(words);
    if w.is_empty() {
        format!("collection:{COLLECTION} AND NOT mediatype:collection")
    } else {
        format!("collection:{COLLECTION} AND NOT mediatype:collection AND ({})", w.join(" AND "))
    }
}

pub fn search_url(words: &str, page: u32) -> String {
    let fields = ["identifier", "title", "creator", "year", "date", "description", "downloads", "mediatype"];
    let fl: String = fields.iter().map(|f| format!("&fl%5B%5D={f}")).collect();
    format!(
        "https://archive.org/advancedsearch.php?q={}{fl}&sort%5B%5D={}&rows={ROWS}&page={}&output=json",
        encode_component(&query(words)),
        encode_component("downloads desc"),
        page.max(1)
    )
}

pub fn details_url(id: &str) -> String {
    format!("https://archive.org/details/{}", encode_component(id))
}

pub fn thumb_url(id: &str) -> String {
    format!("https://archive.org/services/img/{}", encode_component(id))
}

pub fn metadata_url(id: &str) -> String {
    format!("https://archive.org/metadata/{}", encode_component(id))
}

pub fn download_url(id: &str, name: &str) -> String {
    format!("https://archive.org/download/{}/{}", encode_component(id), encode_path(name))
}

/// A field that may be a string, a number or a list of them.
fn text(v: &Value) -> Option<String> {
    let s = match v {
        Value::String(s) => s.trim().to_string(),
        Value::Number(n) => n.to_string(),
        Value::Array(a) => a.iter().filter_map(text).collect::<Vec<_>>().join(", "),
        _ => String::new(),
    };
    if s.is_empty() {
        None
    } else {
        Some(s)
    }
}

fn number(v: &Value) -> Option<u64> {
    match v {
        Value::Number(n) => n.as_u64(),
        Value::String(s) => s.trim().parse().ok(),
        _ => None,
    }
}

pub fn parse_search(v: &Value, page: u32) -> Result<Page, Error> {
    if let Some(e) = v.get("error").and_then(text) {
        return Err(Error::Other(format!("archive.org search: {e}")));
    }
    let resp = v.get("response").ok_or_else(|| Error::Other("archive.org search: no \"response\" in the answer".into()))?;
    let docs = resp.get("docs").and_then(Value::as_array).ok_or_else(|| Error::Other("archive.org search: no \"docs\" in the answer".into()))?;
    let total = resp.get("numFound").and_then(number);
    let mut items = Vec::new();
    for d in docs {
        let Some(id) = d.get("identifier").and_then(text) else {
            continue;
        };
        // the query leaves collections out; one that comes anyway is not a game either
        if d.get("mediatype").and_then(text).is_some_and(|m| m.eq_ignore_ascii_case("collection")) {
            continue;
        }
        let year = d.get("year").and_then(text).or_else(|| d.get("date").and_then(text).map(|s| s.chars().take(4).collect()));
        items.push(Item {
            title: d.get("title").and_then(text).map(|t| html_text(&t)).unwrap_or_else(|| id.clone()),
            creator: d.get("creator").and_then(text).map(|t| clip(&html_text(&t), 80)),
            year,
            description: d.get("description").and_then(text).map(|t| clip(&html_text(&t), 600)).filter(|t| !t.is_empty()),
            kind: None,
            thumb: Some(thumb_url(&id)),
            page: details_url(&id),
            downloads: d.get("downloads").and_then(number),
            data: None,
            id,
        });
    }
    let shown = (page.max(1) as u64 - 1) * ROWS as u64 + docs.len() as u64;
    let more = match total {
        Some(t) => shown < t && !docs.is_empty(),
        None => docs.len() as u32 == ROWS,
    };
    Ok(Page { items, total, more })
}

pub fn search(client: &Client, words: &str, page: u32) -> Result<Page, Error> {
    let v = client.get_json(&search_url(words, page))?;
    parse_search(&v, page)
}

fn is_generated(name: &str) -> bool {
    let n = name.to_ascii_lowercase();
    ["_meta.xml", "_files.xml", "_meta.sqlite", "_reviews.xml", "_archive.torrent", ".torrent", "__ia_thumb.jpg", "_itemimage.jpg"].iter().any(|s| n.ends_with(s))
}

fn ext(name: &str) -> String {
    name.rsplit_once('.').map(|(_, e)| e.to_ascii_lowercase()).unwrap_or_default()
}

/// Which file is the movie, and what goes with it.
pub fn parse_files(id: &str, v: &Value) -> Result<Files, Error> {
    let Some(files) = v.get("files").and_then(Value::as_array) else {
        if v.as_object().is_some_and(|o| o.is_empty()) {
            return Err(Error::Other(format!("archive.org has no item {id}")));
        }
        return Err(Error::Other(format!("archive.org {id}: no file list in the answer")));
    };
    if v.get("is_dark").and_then(Value::as_bool) == Some(true) {
        return Err(Error::Other(format!("archive.org {id}: the item is not public")));
    }
    let meta = v.get("metadata").cloned().unwrap_or(Value::Null);
    if meta.get("mediatype").and_then(text).is_some_and(|m| m.eq_ignore_ascii_case("collection")) {
        return Err(Error::Other(format!("archive.org {id} is a collection of items, not a game")));
    }
    struct F {
        name: String,
        original: bool,
        size: Option<u64>,
    }
    let list: Vec<F> = files
        .iter()
        .filter_map(|f| {
            let name = f.get("name").and_then(Value::as_str)?.to_string();
            Some(F { original: f.get("source").and_then(Value::as_str) == Some("original"), size: f.get("size").and_then(number), name })
        })
        .collect();
    let swfs: Vec<&F> = list.iter().filter(|f| ext(&f.name) == "swf").collect();
    let start = meta.get("emulator_start").and_then(text);
    let (main, note) = match start.as_deref().and_then(|s| {
        swfs.iter().find(|f| f.name == s).or_else(|| swfs.iter().find(|f| f.name.eq_ignore_ascii_case(s))).or_else(|| swfs.iter().find(|f| f.name.rsplit('/').next() == s.rsplit('/').next()))
    }) {
        Some(f) => (*f, format!("emulator_start = {}", start.as_deref().unwrap_or(""))),
        None => {
            let best = swfs.iter().filter(|f| f.original).max_by_key(|f| (f.size.unwrap_or(0), std::cmp::Reverse(f.name.len()))).or_else(|| swfs.iter().max_by_key(|f| f.size.unwrap_or(0)));
            match best {
                Some(f) => (
                    *f,
                    match &start {
                        Some(s) => format!("emulator_start {s:?} is not one of its files; the largest .swf of {}", swfs.len()),
                        None => format!("the largest .swf of {}", swfs.len()),
                    },
                ),
                None => {
                    let kinds: Vec<String> = list.iter().filter(|f| f.original).map(|f| f.name.clone()).take(8).collect();
                    return Err(Error::Other(format!("archive.org {id}: no .swf among its files ({})", if kinds.is_empty() { "none original".into() } else { kinds.join(", ") })));
                }
            }
        }
    };
    let mut extra = Vec::new();
    let mut extra_bytes = 0u64;
    for f in &list {
        if f.name == main.name || !f.original || is_generated(&f.name) || !EXTRA_EXT.contains(&ext(&f.name).as_str()) {
            continue;
        }
        let size = f.size.unwrap_or(0);
        if extra.len() >= EXTRA_MAX_FILES || extra_bytes + size > EXTRA_MAX_BYTES {
            break;
        }
        extra_bytes += size;
        extra.push(RemoteFile { name: f.name.clone(), url: download_url(id, &f.name), size: f.size });
    }
    Ok(Files { main: RemoteFile { name: main.name.clone(), url: download_url(id, &main.name), size: main.size }, extra, referer: None, note, address: None, archive: None, launch: None, beside: None })
}

pub fn files(client: &Client, id: &str) -> Result<Files, Error> {
    let v = client.get_json(&metadata_url(id))?;
    parse_files(id, &v)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_search_asks_the_collection_by_downloads() {
        assert_eq!(query(""), "collection:softwarelibrary_flash AND NOT mediatype:collection");
        assert_eq!(query("Duck Life (4)"), "collection:softwarelibrary_flash AND NOT mediatype:collection AND (duck AND life AND 4)");
        let u = search_url("duck", 2);
        assert!(u.starts_with("https://archive.org/advancedsearch.php?q=collection%3Asoftwarelibrary_flash%20AND%20NOT%20mediatype%3Acollection%20AND%20%28duck%29&fl%5B%5D=identifier"), "{u}");
        assert!(u.contains("&fl%5B%5D=downloads&fl%5B%5D=mediatype&sort"), "{u}");
        assert!(u.ends_with("&sort%5B%5D=downloads%20desc&rows=48&page=2&output=json"), "{u}");
    }

    #[test]
    fn reads_a_search_answer() {
        let v: Value = serde_json::from_str(
            r#"{"responseHeader":{"status":0,"QTime":12},"response":{"numFound":50,"start":0,"docs":[
                {"identifier":"alien-hominid","title":"Alien Hominid","creator":["The Behemoth","Tom Fulp"],"year":"2002","downloads":12345,"description":"<p>Shoot &amp; run.</p>"},
                {"identifier":"no-title","date":"2005-01-01T00:00:00Z","downloads":"7"},
                {"title":"no identifier"}]}}"#,
        )
        .unwrap();
        let p = parse_search(&v, 1).unwrap();
        assert_eq!((p.total, p.more, p.items.len()), (Some(50), true, 2));
        let a = &p.items[0];
        assert_eq!((a.id.as_str(), a.title.as_str(), a.creator.as_deref(), a.year.as_deref()), ("alien-hominid", "Alien Hominid", Some("The Behemoth, Tom Fulp"), Some("2002")));
        assert_eq!((a.description.as_deref(), a.downloads), (Some("Shoot & run."), Some(12345)));
        assert_eq!(a.thumb.as_deref(), Some("https://archive.org/services/img/alien-hominid"));
        assert_eq!(a.page, "https://archive.org/details/alien-hominid");
        let b = &p.items[1];
        assert_eq!((b.title.as_str(), b.year.as_deref(), b.downloads), ("no-title", Some("2005"), Some(7)));
        // the last page
        let p = parse_search(&v, 2).unwrap();
        assert!(!p.more, "48 + 3 docs shown of 50: none left");
        let v: Value = serde_json::from_str(r#"{"response":{"numFound":3,"docs":[{"identifier":"a"},{"identifier":"b"},{"identifier":"c"}]}}"#).unwrap();
        assert!(!parse_search(&v, 1).unwrap().more);
        let v: Value = serde_json::from_str(r#"{"error":"bad query"}"#).unwrap();
        assert_eq!(parse_search(&v, 1).unwrap_err().to_string(), "archive.org search: bad query");
    }

    #[test]
    fn sub_collections_are_not_games() {
        // as archive.org answered on 2026-10-24 (the first page was all sub-collections)
        let v: Value = serde_json::from_str(
            r#"{"response":{"numFound":3,"docs":[
                {"identifier":"softwarelibrary_flash_games","title":"Software Library: Flash Games","downloads":10843942,"mediatype":"collection"},
                {"identifier":"duck-life","title":"Duck Life","downloads":3204,"mediatype":"software"},
                {"identifier":"no-mediatype","title":"x"}]}}"#,
        )
        .unwrap();
        let ids: Vec<String> = parse_search(&v, 1).unwrap().items.into_iter().map(|i| i.id).collect();
        assert_eq!(ids, ["duck-life", "no-mediatype"]);
        let v: Value = serde_json::from_str(
            r#"{"files":[{"name":"__ia_thumb.jpg","source":"original","format":"Item Tile","size":"9011"},{"name":"logo_header.jpg","source":"original","format":"JPEG","size":"17622"}],
               "metadata":{"identifier":"softwarelibrary_flash_games","mediatype":"collection"}}"#,
        )
        .unwrap();
        assert_eq!(parse_files("softwarelibrary_flash_games", &v).unwrap_err().to_string(), "archive.org softwarelibrary_flash_games is a collection of items, not a game");
    }

    #[test]
    fn picks_the_movie_archive_org_plays() {
        let v: Value = serde_json::from_str(
            r#"{"files":[
                {"name":"loader.swf","source":"original","size":"1000"},
                {"name":"Game Main.swf","source":"original","size":"500"},
                {"name":"levels/one.xml","source":"original","size":"20"},
                {"name":"cover.png","source":"original","size":"30"},
                {"name":"source.fla","source":"original","size":"99999"},
                {"name":"game_meta.xml","source":"original","size":"5"},
                {"name":"__ia_thumb.jpg","source":"original","size":"5"},
                {"name":"Game Main.png","source":"derivative","size":"5"}],
              "metadata":{"identifier":"g","emulator":"ruffle-swf","emulator_start":"game main.swf"}}"#,
        )
        .unwrap();
        let f = parse_files("g", &v).unwrap();
        assert_eq!(f.main.name, "Game Main.swf");
        assert_eq!(f.main.url, "https://archive.org/download/g/Game%20Main.swf");
        assert_eq!(f.main.size, Some(500));
        assert_eq!(f.note, "emulator_start = game main.swf");
        let extra: Vec<&str> = f.extra.iter().map(|e| e.name.as_str()).collect();
        assert_eq!(extra, ["loader.swf", "levels/one.xml", "cover.png"]);
        // without emulator_start: the largest original .swf
        let v: Value = serde_json::from_str(r#"{"files":[{"name":"a.swf","source":"original","size":"10"},{"name":"b.SWF","source":"original","size":"20"}],"metadata":{}}"#).unwrap();
        let f = parse_files("x", &v).unwrap();
        assert_eq!((f.main.name.as_str(), f.note.as_str()), ("b.SWF", "the largest .swf of 2"));
        // no movie at all
        let v: Value = serde_json::from_str(r#"{"files":[{"name":"game.zip","source":"original"}],"metadata":{}}"#).unwrap();
        assert_eq!(parse_files("z", &v).unwrap_err().to_string(), "archive.org z: no .swf among its files (game.zip)");
        let v: Value = serde_json::from_str("{}").unwrap();
        assert_eq!(parse_files("gone", &v).unwrap_err().to_string(), "archive.org has no item gone");
    }
}
