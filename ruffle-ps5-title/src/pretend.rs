//! PS5_RUFFLE_PRETEND_V1 (1.000.035) — Settings › Player › Pretend to be on, for Flash and
//! Shockwave games, and its Automatic choice: the site the game's own file names.
//!
//! A sitelocked game compares the address it was loaded from (Flash: `_url`, `loaderInfo.url`;
//! Director: `the moviePath`) with the sites it was made for, which are written into it as text.
//! Paul Blart: Mall Cop's paul_blart.dcr: `validDomains = ["shockwave.com",
//! "developertestdomain.com", "silentbaystudios.com"]` and `the moviePath contains sDomain`;
//! Miniclip's Shockwave game loader builds its addresses from the domain of `the moviePath` when
//! it `contains "http:"`. Automatic reads the movie — a Flash movie decompressed (ruffle_core's
//! `swf::decompress_swf`), a Director movie's resources (an afterburned one's decompressed:
//! src/dir3d.rs) — and counts where each site's domains are written as a whole name ("y8.com" in
//! "www.y8.com", not in "heavy8.com"; any case). The site named most times is the one the game is
//! told it runs on, the list's order breaking a tie; a movie that names none is told nothing. A
//! movie that hides its sites (built from character codes, encrypted) is not found this way, and
//! a site named for another reason (a "more games" link) is taken as well — the log says what was
//! counted.
//!
//! Which address wins: the game's own (online.txt `url`: a Flashpoint game), then this setting.
// SPDX-License-Identifier: GPL-3.0-or-later

use std::time::Instant;

use crate::settings::{self, Settings, SPOOF_AUTOMATIC, SPOOF_SITES};

/// The domains each site of `settings::SPOOF_SITES` is known by, at the same index (Off and
/// Automatic have none). Each ends in ".com" or ".net".
pub const DOMAINS: [&[&str]; 9] = [
    &[],
    &["newgrounds.com", "ungrounded.net"],
    &["kongregate.com", "konggames.com"],
    &["armorgames.com"],
    &["miniclip.com"],
    &["addictinggames.com"],
    &["y8.com"],
    &["shockwave.com"],
    &[],
];

/// The most of a movie's text read (a big game's resources decompressed).
const LIMIT: usize = 256 << 20;

/// A letter, a digit or a hyphen: part of a host name.
fn name_char(c: u8) -> bool {
    c.is_ascii_alphanumeric() || c == b'-'
}

/// How many times each site's domains are written in `text` as whole names, the sites named most
/// first (the list's order next): (index in SPOOF_SITES, times). Sites not named are left out.
pub fn named_sites(text: &[u8]) -> Vec<(usize, usize)> {
    let mut counts = [0usize; DOMAINS.len()];
    let mut i = 0;
    // every domain here ends in a dot and three letters: look where a dot is
    while let Some(p) = text[i..].iter().position(|&b| b == b'.') {
        let dot = i + p;
        i = dot + 1;
        let end = dot + 4;
        if end > text.len() {
            break;
        }
        if text.get(end).is_some_and(|&c| name_char(c)) {
            continue;
        }
        for (site, domains) in DOMAINS.iter().enumerate() {
            for d in domains.iter().map(|d| d.as_bytes()) {
                if end < d.len() {
                    continue;
                }
                let start = end - d.len();
                if text[start..end].eq_ignore_ascii_case(d) && (start == 0 || !name_char(text[start - 1])) {
                    counts[site] += 1;
                }
            }
        }
    }
    let mut out: Vec<(usize, usize)> = counts.iter().copied().enumerate().filter(|&(_, n)| n > 0).collect();
    out.sort_by(|a, b| b.1.cmp(&a.1).then(a.0.cmp(&b.0)));
    out
}

/// The text of a movie to look in: a Flash movie's tags decompressed; a Director movie's resources
/// (decompressed when afterburned), up to LIMIT bytes.
pub fn movie_text(bytes: &[u8]) -> Result<Vec<u8>, String> {
    match bytes.get(..3) {
        Some(b"FWS") | Some(b"CWS") | Some(b"ZWS") => {
            let swf = ruffle_core::swf::decompress_swf(bytes).map_err(|e| format!("not read as a Flash movie: {e}"))?;
            Ok(swf.data)
        }
        _ if bytes.starts_with(b"XFIR") || bytes.starts_with(b"RIFX") => {
            let mut text = Vec::new();
            crate::dir3d::resources(bytes, &|_| true, &mut |_, d| {
                text.extend_from_slice(d);
                text.push(0);
                text.len() >= LIMIT
            })?;
            Ok(text)
        }
        _ => Err("neither a Flash nor a Director movie".into()),
    }
}

/// Where the game at `path` is told it came from when it has no address of its own, as the
/// setting says: a site's address, or (Automatic) that of the site its movie names. The address
/// (None: no pretending) and a line for ruffle.log (None when the setting is Off).
pub fn address(s: &Settings, path: &str, director: bool) -> (Option<String>, Option<String>) {
    let site = s.spoof_site as usize;
    if site == 0 || site >= SPOOF_SITES.len() {
        return (None, None);
    }
    if s.spoof_site != SPOOF_AUTOMATIC {
        let a = settings::site_address(site, path, director);
        let line = a.as_ref().map(|a| format!("     pretend to be on: {} ({a})", SPOOF_SITES[site].0));
        return (a, line);
    }
    let t0 = Instant::now();
    let named = std::fs::read(path).map_err(|e| e.to_string()).and_then(|b| movie_text(&b)).map(|t| named_sites(&t));
    let ms = t0.elapsed().as_millis();
    match named {
        Ok(sites) if !sites.is_empty() => {
            let site = sites[0].0;
            let a = settings::site_address(site, path, director);
            let counted = sites.iter().map(|&(i, n)| format!("{} {n}", SPOOF_SITES[i].0)).collect::<Vec<_>>().join(", ");
            let line = format!(
                "     pretend to be on: automatic: {} (sites the movie names, times: {counted}; read in {ms} ms){}",
                SPOOF_SITES[site].0,
                a.as_deref().map(|a| format!(" -> {a}")).unwrap_or_default()
            );
            (a, Some(line))
        }
        Ok(_) => (None, Some(format!("     pretend to be on: automatic: the movie names none of the sites (read in {ms} ms); not pretending"))),
        Err(e) => (None, Some(format!("     pretend to be on: automatic: the movie was not read ({e}); not pretending"))),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sites_are_counted_where_their_domains_are_whole_names() {
        let t: &[u8] = b"http://www.y8.com/games/ heavy8.com y8.community my-y8.com WWW.KONGREGATE.COM/x \"kongregate.com\" ungrounded.net\0newgrounds.com.au y8.com";
        // y8.com twice (www.y8.com and the last), not in heavy8.com, y8.community or my-y8.com;
        // Kongregate in any case; Newgrounds by either of its domains
        assert_eq!(named_sites(t), vec![(1, 2), (2, 2), (6, 2)], "a tie: the list's order");
        assert_eq!(named_sites(b"miniclip.com"), vec![(4, 1)], "at the very start and the very end");
        assert_eq!(named_sites(b"shockwave.com shockwave.com miniclip.com"), vec![(7, 2), (4, 1)], "the most named first");
        assert!(named_sites(b"nothing here. a.com .net .co").is_empty());
    }

    /// A Flash movie with these tags' bytes (a Metadata tag holding `text`), compressed or not.
    fn swf(text: &[u8], compressed: bool) -> Vec<u8> {
        let mut body = vec![0x00, 0x00, 0x18, 0x01, 0x00]; // an empty RECT, 24 fps, one frame
        body.extend_from_slice(&((77u16 << 6) | 0x3f).to_le_bytes());
        body.extend_from_slice(&(text.len() as u32).to_le_bytes());
        body.extend_from_slice(text);
        body.extend_from_slice(&[0, 0]);
        let mut f = Vec::from(if compressed { &b"CWS"[..] } else { &b"FWS"[..] });
        f.push(10);
        f.extend_from_slice(&((body.len() + 8) as u32).to_le_bytes());
        if compressed {
            use std::io::Write;
            let mut e = flate2::write::ZlibEncoder::new(Vec::new(), flate2::Compression::default());
            e.write_all(&body).unwrap();
            f.extend(e.finish().unwrap());
        } else {
            f.extend(body);
        }
        f
    }

    #[test]
    fn a_movies_text_is_read_decompressed() {
        let lock: &[u8] = b"if (_url.indexOf(\"www.armorgames.com\") == -1) gotoAndStop(\"locked\");";
        for compressed in [false, true] {
            let t = movie_text(&swf(lock, compressed)).unwrap();
            assert_eq!(named_sites(&t), vec![(3, 1)], "Flash, compressed {compressed}");
        }
        // Director: afterburned (a record in the initial load segment, one after it compressed)
        // and plain
        let lingo: &[u8] = b"validDomains = [\"shockwave.com\", \"developertestdomain.com\"]";
        let ab = crate::dir3d::sample::afterburned_file(&[(b"\x00\x00\x00\x01 a bitmap", true), (lingo, false)]);
        assert_eq!(named_sites(&movie_text(&ab).unwrap()), vec![(7, 1)]);
        let ab_ils = crate::dir3d::sample::afterburned_file(&[(lingo, true)]);
        assert_eq!(named_sites(&movie_text(&ab_ils).unwrap()), vec![(7, 1)]);
        let plain = crate::dir3d::sample::plain_file(&[(b"Lscr", lingo), (b"STXT", b"http://www.miniclip.com/games/x/en/")]);
        assert_eq!(named_sites(&movie_text(&plain).unwrap()), vec![(4, 1), (7, 1)]);
        assert!(movie_text(b"PNG not a movie").is_err());
    }

    #[test]
    fn the_setting_or_the_site_the_movie_names() {
        let dir = std::env::temp_dir().join(format!("ruffle5-pretend-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let locked = dir.join("locked.swf");
        std::fs::write(&locked, swf(b"miniclip.com miniclip.com kongregate.com", true)).unwrap();
        let open = dir.join("open.swf");
        std::fs::write(&open, swf(b"no sites here", false)).unwrap();
        let dcr = dir.join("game.dcr");
        std::fs::write(&dcr, crate::dir3d::sample::afterburned_file(&[(b"the moviePath contains \"shockwave.com\"", false)])).unwrap();
        let p = |f: &std::path::Path| f.to_string_lossy().into_owned();
        let mut s = Settings::default();
        // Off: nothing, and nothing said
        assert_eq!(address(&s, &p(&locked), false), (None, None));
        // a site chosen: that site, whatever the movie names
        s.spoof_site = 2;
        let (a, line) = address(&s, &p(&locked), false);
        assert_eq!(a.as_deref(), Some("https://chat.kongregate.com/gamez/0001/0001/live/locked.swf"));
        assert!(line.unwrap().contains("Kongregate"));
        // Automatic: the site named most
        s.spoof_site = SPOOF_AUTOMATIC;
        let (a, line) = address(&s, &p(&locked), false);
        assert_eq!(a.as_deref(), Some("https://www.miniclip.com/games/en/locked.swf"));
        let line = line.unwrap();
        assert!(line.contains("automatic: Miniclip") && line.contains("Miniclip 2, Kongregate 1"), "{line}");
        // a Shockwave movie: the http:// address
        assert_eq!(address(&s, &p(&dcr), true).0.as_deref(), Some("http://www.shockwave.com/content/game.dcr"));
        // naming none, or not a movie: no pretending, and the log says so
        let (a, line) = address(&s, &p(&open), false);
        assert!(a.is_none() && line.unwrap().contains("names none"));
        let (a, line) = address(&s, &p(&dir.join("missing.swf")), false);
        assert!(a.is_none() && line.unwrap().contains("not read"));
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// The real movies, when given: `R5_PRETEND=path=Site;path=-` (host only; `-`: none).
    #[test]
    fn real_movies() {
        let Ok(list) = std::env::var("R5_PRETEND") else {
            return;
        };
        for item in list.split(';').filter(|s| !s.is_empty()) {
            let (path, want) = item.rsplit_once('=').unwrap();
            let t0 = Instant::now();
            let bytes = std::fs::read(path).unwrap();
            let named = named_sites(&movie_text(&bytes).unwrap());
            let got = named.first().map(|&(i, _)| SPOOF_SITES[i].0).unwrap_or("-");
            let counts: Vec<String> = named.iter().map(|&(i, n)| format!("{} {n}", SPOOF_SITES[i].0)).collect();
            eprintln!("{path}: {got} ({}) in {:.1} ms, {} bytes", counts.join(", "), t0.elapsed().as_secs_f64() * 1000.0, bytes.len());
            assert_eq!(got, want, "{path}");
        }
    }
}
