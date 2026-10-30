//! PS5_RUFFLE_HWSERVER_V1 (1.000.038) — a stand-in for the level server of Happy Wheels' full
//! version (totaljerkface.com), played from a folder that carries the server's answers as files.
//!
//! The full version of Happy Wheels (v1.70) asks its site for everything beyond the game itself:
//! `db/get_featured_levels.php` (the featured list), `db/get_leveldata_e.php+id=<id>` (a level),
//! `db/get_login.php`, and the user level browser's `db/get_levels_wr2.php` with a query
//! (`offset`, `character`, `sortby`, `uploaded`, `levelname`, `author`, `featured`, `importable`,
//! `favorites`, `public`), `db/get_userid.php` (an author's name to an id). A folder made to play
//! it offline has the first three as files; the browser's answers it cannot have, and the game
//! then says "Sorry, there was a problem". This answers them from the featured list, the one list
//! the folder has: the same records (`id_x`, `name`, `author_id`, `user_name`, `rating`, `votes`,
//! `plays`, `created`, `comments`, `playable_character`, `force_character`, `importable`,
//! `featured`, `public`), filtered and sorted as asked — `uploaded` (today / week / month) is not
//! applied, the levels being from 2012 — and paged by `offset`; `get_userid.php` finds an author
//! by name among them. The answers are what the site sends: `ToFlash=` and a PHP-serialized array,
//! URL-encoded (spaces as `%20`).
//!
//! Only asked when the game's folder has `db/get_featured_levels.php` and the file asked for is
//! not there (src/navigator.rs); any other game is untouched.
// SPDX-License-Identifier: GPL-3.0-or-later

use std::path::Path;

/// A value of PHP's serialize format, as far as the level lists need it.
#[derive(Clone, Debug, PartialEq)]
pub enum Php {
    Null,
    Bool(bool),
    Int(i64),
    Double(f64),
    Str(Vec<u8>),
    Arr(Vec<(Php, Php)>),
}

impl Php {
    /// The entry of a string-keyed array.
    fn get(&self, key: &str) -> Option<&Php> {
        match self {
            Php::Arr(items) => items.iter().find(|(k, _)| matches!(k, Php::Str(s) if s == key.as_bytes())).map(|(_, v)| v),
            _ => None,
        }
    }

    fn text(&self) -> String {
        match self {
            Php::Str(s) => String::from_utf8_lossy(s).into_owned(),
            Php::Int(i) => i.to_string(),
            Php::Double(d) => d.to_string(),
            Php::Bool(b) => if *b { "1".into() } else { "".into() },
            _ => String::new(),
        }
    }

    fn number(&self) -> f64 {
        match self {
            Php::Int(i) => *i as f64,
            Php::Double(d) => *d,
            Php::Str(s) => String::from_utf8_lossy(s).trim().parse().unwrap_or(0.0),
            Php::Bool(b) => if *b { 1.0 } else { 0.0 },
            _ => 0.0,
        }
    }
}

/// Reads one serialized value from the start of `b`; the value and what follows it.
fn parse(b: &[u8]) -> Option<(Php, &[u8])> {
    let kind = *b.first()?;
    let rest = &b[1..];
    match kind {
        b'N' => Some((Php::Null, rest.strip_prefix(b";")?)),
        b'b' => {
            let rest = rest.strip_prefix(b":")?;
            let v = *rest.first()? == b'1';
            Some((Php::Bool(v), rest[1..].strip_prefix(b";")?))
        }
        b'i' | b'd' => {
            let rest = rest.strip_prefix(b":")?;
            let end = rest.iter().position(|&c| c == b';')?;
            let text = std::str::from_utf8(&rest[..end]).ok()?;
            let v = if kind == b'i' { Php::Int(text.parse().ok()?) } else { Php::Double(text.parse().ok()?) };
            Some((v, &rest[end + 1..]))
        }
        b's' => {
            let rest = rest.strip_prefix(b":")?;
            let colon = rest.iter().position(|&c| c == b':')?;
            let len: usize = std::str::from_utf8(&rest[..colon]).ok()?.parse().ok()?;
            let rest = rest[colon + 1..].strip_prefix(b"\"")?;
            let s = rest.get(..len)?.to_vec();
            Some((Php::Str(s), rest[len..].strip_prefix(b"\";")?))
        }
        b'a' => {
            let rest = rest.strip_prefix(b":")?;
            let colon = rest.iter().position(|&c| c == b':')?;
            let n: usize = std::str::from_utf8(&rest[..colon]).ok()?.parse().ok()?;
            let mut rest = rest[colon + 1..].strip_prefix(b"{")?;
            let mut items = Vec::with_capacity(n);
            for _ in 0..n {
                let (k, r) = parse(rest)?;
                let (v, r) = parse(r)?;
                items.push((k, v));
                rest = r;
            }
            Some((Php::Arr(items), rest.strip_prefix(b"}")?))
        }
        _ => None,
    }
}

fn serialize(v: &Php, out: &mut Vec<u8>) {
    match v {
        Php::Null => out.extend_from_slice(b"N;"),
        Php::Bool(b) => out.extend_from_slice(if *b { b"b:1;" } else { b"b:0;" }),
        Php::Int(i) => out.extend_from_slice(format!("i:{i};").as_bytes()),
        Php::Double(d) => out.extend_from_slice(format!("d:{d};").as_bytes()),
        Php::Str(s) => {
            out.extend_from_slice(format!("s:{}:\"", s.len()).as_bytes());
            out.extend_from_slice(s);
            out.extend_from_slice(b"\";");
        }
        Php::Arr(items) => {
            out.extend_from_slice(format!("a:{}:{{", items.len()).as_bytes());
            for (k, v) in items {
                serialize(k, out);
                serialize(v, out);
            }
            out.extend_from_slice(b"}");
        }
    }
}

/// The answer file's text: `ToFlash=` and the URL-encoded serialized value.
fn decode_answer(file: &[u8]) -> Option<Php> {
    let body = file.strip_prefix(b"ToFlash=")?;
    let raw = percent_decode(body);
    parse(&raw).map(|(v, _)| v)
}

fn encode_answer(v: &Php) -> Vec<u8> {
    let mut raw = Vec::new();
    serialize(v, &mut raw);
    let mut out = Vec::from(&b"ToFlash="[..]);
    for &c in &raw {
        if c.is_ascii_alphanumeric() || matches!(c, b'-' | b'_' | b'.' | b'~') {
            out.push(c);
        } else {
            out.extend_from_slice(format!("%{c:02X}").as_bytes());
        }
    }
    out
}

/// URL decoding: `%XX` and `+` as a space.
fn percent_decode(b: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(b.len());
    let mut i = 0;
    while i < b.len() {
        match b[i] {
            b'+' => out.push(b' '),
            b'%' if i + 2 < b.len() => {
                let hex = |c: u8| (c as char).to_digit(16);
                match (hex(b[i + 1]), hex(b[i + 2])) {
                    (Some(h), Some(l)) => {
                        out.push((h * 16 + l) as u8);
                        i += 2;
                    }
                    _ => out.push(b'%'),
                }
            }
            c => out.push(c),
        }
        i += 1;
    }
    out
}

/// The variables of a query string or a form body.
fn variables(text: &[u8]) -> Vec<(String, String)> {
    text.split(|&c| c == b'&')
        .filter(|p| !p.is_empty())
        .map(|p| {
            let (k, v) = match p.iter().position(|&c| c == b'=') {
                Some(i) => (&p[..i], &p[i + 1..]),
                None => (p, &b""[..]),
            };
            (String::from_utf8_lossy(&percent_decode(k)).into_owned(), String::from_utf8_lossy(&percent_decode(v)).into_owned())
        })
        .collect()
}

/// The level records of the featured list in `dir/db`.
fn levels(dir: &Path) -> Option<Vec<Php>> {
    let file = std::fs::read(dir.join("db").join("get_featured_levels.php")).ok()?;
    match decode_answer(&file)? {
        Php::Arr(items) => Some(items.into_iter().map(|(_, v)| v).collect()),
        _ => None,
    }
}

/// Is the folder one this answers for?
pub fn serves(dir: &Path) -> bool {
    dir.join("db").join("get_featured_levels.php").is_file()
}

/// The answer to `db/<name>` asked with `query` (the address's query string) and `body` (a POST's
/// form), from the folder `dir`; None when it is not one of the server's files this stands in for.
pub fn answer(dir: &Path, name: &str, query: &str, body: Option<&[u8]>) -> Option<Vec<u8>> {
    let mut vars = variables(query.as_bytes());
    if let Some(b) = body {
        vars.extend(variables(b));
    }
    let var = |k: &str| vars.iter().rev().find(|(n, _)| n == k).map(|(_, v)| v.as_str());
    match name {
        "get_levels_wr2.php" => {
            let mut list = levels(dir)?;
            if let Some(c) = var("character").and_then(|c| c.parse::<i64>().ok()).filter(|&c| c != 0) {
                list.retain(|l| l.get("playable_character").map(|v| v.number() as i64) == Some(c));
            }
            if let Some(a) = var("author").filter(|a| !a.is_empty() && *a != "0") {
                list.retain(|l| l.get("author_id").map(|v| v.text()).as_deref() == Some(a));
            }
            if let Some(n) = var("levelname").map(|n| n.trim().to_lowercase()).filter(|n| !n.is_empty()) {
                list.retain(|l| l.get("name").map(|v| v.text().to_lowercase().contains(&n)).unwrap_or(false));
            }
            for (flag, key) in [("featured", "featured"), ("importable", "importable"), ("public", "public")] {
                if var(flag) == Some("1") {
                    list.retain(|l| l.get(key).map(|v| v.number() != 0.0).unwrap_or(key == "public"));
                }
            }
            if var("favorites") == Some("1") {
                // no account here: nothing is a favorite
                list.clear();
            }
            match var("sortby").unwrap_or("newest") {
                "oldest" => list.sort_by(|a, b| a.get("created").map(|v| v.text()).cmp(&b.get("created").map(|v| v.text()))),
                "plays" => list.sort_by(|a, b| b.get("plays").map(|v| v.number()).unwrap_or(0.0).total_cmp(&a.get("plays").map(|v| v.number()).unwrap_or(0.0))),
                "rating" => list.sort_by(|a, b| b.get("rating").map(|v| v.number()).unwrap_or(0.0).total_cmp(&a.get("rating").map(|v| v.number()).unwrap_or(0.0))),
                _ => list.sort_by(|a, b| b.get("created").map(|v| v.text()).cmp(&a.get("created").map(|v| v.text()))),
            }
            let offset = var("offset").and_then(|o| o.parse::<usize>().ok()).unwrap_or(0).min(list.len());
            let page: Vec<(Php, Php)> = list.into_iter().skip(offset).enumerate().map(|(i, l)| (Php::Int(i as i64), l)).collect();
            Some(encode_answer(&Php::Arr(page)))
        }
        "get_userid.php" => {
            let wanted = var("name").or_else(|| var("username")).or_else(|| var("user_name"))?.trim().to_lowercase();
            let list = levels(dir)?;
            let id = list.iter().find(|l| l.get("user_name").map(|v| v.text().to_lowercase()) == Some(wanted.clone())).and_then(|l| l.get("author_id")).map(|v| v.text());
            // the site answers the id, or nothing for a name it does not know
            Some(match id {
                Some(id) => format!("ToFlash={id}").into_bytes(),
                None => b"ToFlash=".to_vec(),
            })
        }
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn level(id: &str, name: &str, author: &str, user: &str, rating: &str, plays: &str, created: &str, character: &str) -> Php {
        let s = |v: &str| Php::Str(v.as_bytes().to_vec());
        Php::Arr(vec![
            (s("id_x"), s(id)),
            (s("name"), s(name)),
            (s("author_id"), s(author)),
            (s("rating"), s(rating)),
            (s("votes"), s("10")),
            (s("plays"), s(plays)),
            (s("created"), s(created)),
            (s("comments"), s("a level")),
            (s("playable_character"), s(character)),
            (s("featured"), s("1")),
            (s("public"), s("1")),
            (s("user_name"), s(user)),
        ])
    }

    fn folder(levels: Vec<Php>) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!("ruffle5-hw-{}-{}", std::process::id(), levels.len()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join("db")).unwrap();
        let list = Php::Arr(levels.into_iter().enumerate().map(|(i, l)| (Php::Int(i as i64), l)).collect());
        std::fs::write(dir.join("db/get_featured_levels.php"), encode_answer(&list)).unwrap();
        dir
    }

    #[test]
    fn php_serialize_round_trips_and_decodes_the_sites_form() {
        let text = b"a:2:{i:0;a:3:{s:4:\"id_x\";s:7:\"4073431\";s:4:\"name\";s:19:\"Prehistoric Pilgrim\";s:6:\"rating\";s:6:\"4.6396\";}i:1;a:2:{s:4:\"name\";s:6:\"Cr\xc3\xa9me\";s:1:\"n\";i:-3;}}";
        let (v, rest) = parse(text).unwrap();
        assert!(rest.is_empty());
        let mut again = Vec::new();
        serialize(&v, &mut again);
        assert_eq!(again, text);
        let Php::Arr(items) = &v else { panic!() };
        assert_eq!(items[0].1.get("name").unwrap().text(), "Prehistoric Pilgrim");
        assert_eq!(items[1].1.get("name").unwrap().text(), "Créme");
        assert_eq!(items[1].1.get("n"), Some(&Php::Int(-3)));
        // the site's form: ToFlash= and URL encoding (PHP's: spaces as +)
        let site = b"ToFlash=a%3A1%3A%7Bi%3A0%3Ba%3A1%3A%7Bs%3A4%3A%22name%22%3Bs%3A10%3A%22Crime+City%22%3B%7D%7D";
        let v = decode_answer(site).unwrap();
        let Php::Arr(items) = &v else { panic!() };
        assert_eq!(items[0].1.get("name").unwrap().text(), "Crime City");
        // encoded again: the same value, spaces as %20
        let enc = encode_answer(&v);
        assert!(std::str::from_utf8(&enc).unwrap().contains("Crime%20City"));
        assert_eq!(decode_answer(&enc).unwrap(), v);
    }

    #[test]
    fn the_browser_is_answered_from_the_featured_list() {
        let dir = folder(vec![
            level("1", "Crime City", "100", "Alice", "4.5", "900", "2012-07-03 06:07:19", "1"),
            level("2", "Prehistoric Pilgrim", "200", "mgmartinec", "4.6", "500", "2012-06-29 17:50:22", "2"),
            level("3", "Rope Swing", "100", "Alice", "3.9", "100", "2012-08-01 00:00:00", "2"),
        ]);
        let names = |answer: Vec<u8>| -> Vec<String> {
            let Php::Arr(items) = decode_answer(&answer).unwrap() else { panic!() };
            items.iter().map(|(_, l)| l.get("name").unwrap().text()).collect()
        };
        // newest first, as the site's default
        assert_eq!(names(answer(&dir, "get_levels_wr2.php", "offset=0&character=0&uploaded=week&sortby=newest&public=1", None).unwrap()), ["Rope Swing", "Crime City", "Prehistoric Pilgrim"]);
        assert_eq!(names(answer(&dir, "get_levels_wr2.php", "sortby=oldest", None).unwrap()), ["Prehistoric Pilgrim", "Crime City", "Rope Swing"]);
        assert_eq!(names(answer(&dir, "get_levels_wr2.php", "sortby=plays", None).unwrap()), ["Crime City", "Prehistoric Pilgrim", "Rope Swing"]);
        assert_eq!(names(answer(&dir, "get_levels_wr2.php", "sortby=rating", None).unwrap()), ["Prehistoric Pilgrim", "Crime City", "Rope Swing"]);
        // a character, an author, a name (the search's '+' is a space), a page
        assert_eq!(names(answer(&dir, "get_levels_wr2.php", "character=2&sortby=newest", None).unwrap()), ["Rope Swing", "Prehistoric Pilgrim"]);
        assert_eq!(names(answer(&dir, "get_levels_wr2.php", "author=100&sortby=oldest", None).unwrap()), ["Crime City", "Rope Swing"]);
        assert_eq!(names(answer(&dir, "get_levels_wr2.php", "levelname=crime+c&uploaded=anytime", None).unwrap()), ["Crime City"]);
        assert_eq!(names(answer(&dir, "get_levels_wr2.php", "offset=2&sortby=newest", None).unwrap()), ["Prehistoric Pilgrim"]);
        assert_eq!(names(answer(&dir, "get_levels_wr2.php", "favorites=1", None).unwrap()), Vec::<String>::new());
        // a POST's form counts as the query does
        assert_eq!(names(answer(&dir, "get_levels_wr2.php", "", Some(b"sortby=plays&character=1")).unwrap()), ["Crime City"]);
        // an author's id by name
        assert_eq!(answer(&dir, "get_userid.php", "name=alice", None).unwrap(), b"ToFlash=100");
        assert_eq!(answer(&dir, "get_userid.php", "name=nobody", None).unwrap(), b"ToFlash=");
        // not the server's files, or not such a folder
        assert!(answer(&dir, "get_login.php", "", None).is_none());
        assert!(answer(&std::env::temp_dir(), "get_levels_wr2.php", "", None).is_none());
        assert!(serves(&dir) && !serves(&std::env::temp_dir()));
        let _ = std::fs::remove_dir_all(&dir);
    }
}
