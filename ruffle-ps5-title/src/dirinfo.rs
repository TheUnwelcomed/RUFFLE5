//! PS5_RUFFLE_DIRINFO_V1 — what the menu shows about a Director / Shockwave game, and which file in
//! its folder starts it.
//!
//! A Shockwave movie (.dcr, and .cct casts) is "afterburned": `XFIR`/`RIFX`, the codec `FGDM` or
//! `FGDC`, then an `Fver` record whose last field is the Director version that saved it ("8.0#178").
//! A plain .dir/.dxr (codec `MV93`) has no such record; the menu then just says Director.
//!
//! A game in a folder of its own (movie, casts, Xtras, a projector) is started by the movie its
//! projector would open — `[Movies] Movie01=` in Projector.ini — or, without one, its only .dcr, its
//! first .dir/.dxr, its first .dcr.
// SPDX-License-Identifier: GPL-3.0-or-later

/// File extensions DirPlayer plays (lower case).
pub const MOVIE_EXTS: [&str; 3] = ["dcr", "dir", "dxr"];

pub fn is_movie(name: &str) -> bool {
    name.rsplit_once('.').is_some_and(|(_, ext)| MOVIE_EXTS.iter().any(|e| ext.eq_ignore_ascii_case(e)))
}

/// "Director 8.0" (from an afterburned movie's Fver), "Director" for an unprotected movie, None if
/// the file is not a Director file.
pub fn describe(path: &str) -> Option<String> {
    use std::io::Read;
    let mut head = [0u8; 96];
    let mut f = std::fs::File::open(path).ok()?;
    let n = f.read(&mut head).ok()?;
    parse_head(&head[..n])
}

fn parse_head(b: &[u8]) -> Option<String> {
    if b.len() < 12 {
        return None;
    }
    let le = match &b[0..4] {
        b"XFIR" => true,
        b"RIFX" => false,
        _ => return None,
    };
    let tag = |at: usize| -> Option<[u8; 4]> {
        let t: [u8; 4] = b.get(at..at + 4)?.try_into().ok()?;
        Some(if le { [t[3], t[2], t[1], t[0]] } else { t })
    };
    match &tag(8)? {
        b"FGDM" | b"FGDC" => {}
        b"MV93" | b"MC95" => return Some("Director".to_string()),
        _ => return None,
    }
    if &tag(12)? != b"Fver" {
        return Some("Shockwave".to_string());
    }
    let mut p = 16;
    let mut var = || -> Option<u32> {
        let mut v = 0u32;
        loop {
            let x = *b.get(p)?;
            p += 1;
            v = (v << 7) | (x & 0x7f) as u32;
            if x & 0x80 == 0 {
                return Some(v);
            }
        }
    };
    let _len = var()?;
    let version = var()?;
    if version < 0x501 {
        return Some("Shockwave".to_string());
    }
    let _imap = var()?;
    let _director = var()?;
    let l = *b.get(p)? as usize;
    let s = b.get(p + 1..p + 1 + l)?;
    let text = String::from_utf8_lossy(s);
    // "8.0#178": the version, without the build number
    let shown = text.split('#').next().unwrap_or("").trim();
    Some(if shown.is_empty() { "Shockwave".to_string() } else { format!("Director {shown}") })
}

/// The movie that starts the game in folder `dir` (whose file names are `files`), if any.
pub fn launcher(dir: &str, files: &[String]) -> Option<String> {
    let ini = files.iter().find(|f| f.eq_ignore_ascii_case("projector.ini"));
    if let Some(ini) = ini {
        if let Ok(text) = std::fs::read(format!("{dir}/{ini}")) {
            let text = String::from_utf8_lossy(&text);
            let mut in_movies = false;
            for line in text.lines() {
                let line = line.trim();
                if line.starts_with(';') || line.is_empty() {
                    continue;
                }
                if line.starts_with('[') {
                    in_movies = line.eq_ignore_ascii_case("[movies]");
                    continue;
                }
                if in_movies {
                    if let Some((k, v)) = line.split_once('=') {
                        if k.trim().eq_ignore_ascii_case("movie01") {
                            let v = v.trim().replace('\\', "/");
                            let name = v.rsplit('/').next().unwrap_or(&v).to_string();
                            if let Some(found) = files.iter().find(|f| f.eq_ignore_ascii_case(&name)) {
                                return Some(found.clone());
                            }
                        }
                    }
                }
            }
        }
    }
    let with = |ext: &str| -> Vec<&String> {
        files.iter().filter(|f| f.rsplit_once('.').is_some_and(|(_, e)| e.eq_ignore_ascii_case(ext))).collect()
    };
    let dcr = with("dcr");
    if dcr.len() == 1 {
        return Some(dcr[0].clone());
    }
    with("dir").first().or(with("dxr").first()).or(dcr.first()).map(|f| (*f).clone())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_an_afterburned_version() {
        // XFIR, size, MDGF (FGDM reversed), revF (Fver reversed), len, version 0x501.., imap 1,
        // director 0x640, "8.0#178"
        let mut b = Vec::new();
        b.extend_from_slice(b"XFIR");
        b.extend_from_slice(&[0, 0, 0, 0]);
        b.extend_from_slice(b"MDGF");
        b.extend_from_slice(b"revF");
        b.push(14);
        b.extend_from_slice(&[0x8a, 0x01]); // 0x501
        b.push(1);
        b.extend_from_slice(&[0x8c, 0x40]); // 0x640
        b.push(7);
        b.extend_from_slice(b"8.0#178");
        assert_eq!(parse_head(&b).as_deref(), Some("Director 8.0"));
        assert_eq!(parse_head(b"RIFX\0\0\0\0MV93....").as_deref(), Some("Director"));
        assert_eq!(parse_head(b"FWS\x06aaaaaaaaaaaa"), None);
    }

    #[test]
    fn picks_the_projector_movie() {
        let dir = std::env::temp_dir().join(format!("ruffle5-dirinfo-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("Projector.ini"), "[Movies]\r\nMovie01=gamewrapper.dir\r\n").unwrap();
        let files: Vec<String> = ["Projector.ini", "game.dcr", "gamewrapper.dir", "char_Robin.cct"].iter().map(|s| s.to_string()).collect();
        assert_eq!(launcher(dir.to_str().unwrap(), &files).as_deref(), Some("gamewrapper.dir"));
        let files: Vec<String> = ["game.dcr", "x.cct"].iter().map(|s| s.to_string()).collect();
        assert_eq!(launcher("/nonexistent", &files).as_deref(), Some("game.dcr"));
        assert!(is_movie("A.DCR") && is_movie("b.dir") && !is_movie("c.cct"));
        let _ = std::fs::remove_dir_all(&dir);
    }
}
