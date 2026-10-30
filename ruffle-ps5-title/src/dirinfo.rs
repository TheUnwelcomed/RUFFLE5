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
//!
//! PS5_RUFFLE_PROJECTOR_V1 (1.000.037): a Director projector for Windows (mallcop.exe) is the
//! player program with its movie appended: after the program's last PE section a `PJ` marker (the
//! bytes `10JP` = PJ01 stored little-endian) and, at the very end of the file, the movie as a
//! `XFIR` container — a movie codec (`FGDM`, `MV93`…) — ending 4 bytes before the end (Director
//! writes the marker's offset there as a trailer). The folder is started by that movie when there
//! is no Projector.ini naming one: Paul Blart: Mall Cop's (gameloader.dcr, paul_blart.dcr,
//! advert_loader.swf, mallcop.exe) is a 6 KB stub that mounts Miniclip's loader casts and goes to
//! paul_blart.dcr with the loader's intro counted as done — started on gameloader.dcr itself the
//! loader waited for an intro and a game address it never had (a white screen).
// SPDX-License-Identifier: GPL-3.0-or-later

/// File extensions DirPlayer plays (lower case).
pub const MOVIE_EXTS: [&str; 3] = ["dcr", "dir", "dxr"];

pub fn is_movie(name: &str) -> bool {
    name.rsplit_once('.').is_some_and(|(_, ext)| MOVIE_EXTS.iter().any(|e| ext.eq_ignore_ascii_case(e)))
}

/// "Director 8.0" (from an afterburned movie's Fver), "Director" for an unprotected movie, None if
/// the file is not a Director file. A projector: its movie's, with " projector" after it.
pub fn describe(path: &str) -> Option<String> {
    use std::io::{Read, Seek};
    let mut head = [0u8; 96];
    let mut f = std::fs::File::open(path).ok()?;
    if is_exe_name(path) {
        let p = projector(path)?;
        f.seek(std::io::SeekFrom::Start(p.offset)).ok()?;
        let n = f.read(&mut head).ok()?;
        return parse_head(&head[..n.min(p.len as usize)]).map(|d| format!("{d} projector"));
    }
    let n = f.read(&mut head).ok()?;
    parse_head(&head[..n])
}

fn is_exe_name(name: &str) -> bool {
    name.rsplit_once('.').is_some_and(|(_, ext)| ext.eq_ignore_ascii_case("exe"))
}

/// The movie a Director projector for Windows carries: where it is in the file and how long it is.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Projector {
    pub offset: u64,
    pub len: u64,
}

/// The movie in the projector at `path`, if the file is one (see the top of this file). Reads the
/// program's header and the end of the file: the movie is looked for backwards from the end, so a
/// big one costs reading it once.
pub fn projector(path: &str) -> Option<Projector> {
    use std::io::{Read, Seek, SeekFrom};
    let mut f = std::fs::File::open(path).ok()?;
    let len = f.metadata().ok()?.len();
    let mut head = vec![0u8; 0x400.min(len as usize)];
    f.read_exact(&mut head).ok()?;
    let pe_end = pe_end(&head)?;
    if pe_end + 8 > len {
        return None;
    }
    // the marker after the program: "PJ" + two digits, stored reversed
    let mut mark = [0u8; 4];
    f.seek(SeekFrom::Start(pe_end)).ok()?;
    f.read_exact(&mut mark).ok()?;
    if &mark[2..4] != b"JP" || !mark[0].is_ascii_digit() || !mark[1].is_ascii_digit() {
        return None;
    }
    // the movie: the last container whose end is at the end of the file (4 bytes before it)
    let mut window = 64 * 1024u64;
    loop {
        let start = len.saturating_sub(window).max(pe_end + 4);
        let n = (len - start) as usize;
        let mut tail = vec![0u8; n];
        f.seek(SeekFrom::Start(start)).ok()?;
        f.read_exact(&mut tail).ok()?;
        if let Some(p) = movie_at_end(&tail, start, len) {
            return Some(p);
        }
        if start <= pe_end + 4 {
            return None;
        }
        window *= 4;
    }
}

/// Where the sections of the Windows program in `head` (its first bytes) end: the data a projector
/// appends starts there. None if it is not a Windows program.
fn pe_end(head: &[u8]) -> Option<u64> {
    if head.get(..2)? != b"MZ" {
        return None;
    }
    let u16_at = |at: usize| -> Option<u16> { head.get(at..at + 2).map(|b| u16::from_le_bytes([b[0], b[1]])) };
    let u32_at = |at: usize| -> Option<u32> { head.get(at..at + 4).map(|b| u32::from_le_bytes([b[0], b[1], b[2], b[3]])) };
    let pe = u32_at(0x3c)? as usize;
    if head.get(pe..pe + 4)? != b"PE\0\0" {
        return None;
    }
    let sections = u16_at(pe + 6)? as usize;
    let optional = u16_at(pe + 20)? as usize;
    let table = pe + 24 + optional;
    let mut end = 0u64;
    for i in 0..sections {
        let s = table + i * 40;
        let raw_size = u32_at(s + 16)? as u64;
        let raw_offset = u32_at(s + 20)? as u64;
        end = end.max(raw_offset + raw_size);
    }
    (end > 0).then_some(end)
}

/// In `tail` (the file's bytes from `start` to its end `len`), the last `XFIR`/`RIFX` movie
/// container ending within 64 bytes of the end.
fn movie_at_end(tail: &[u8], start: u64, len: u64) -> Option<Projector> {
    let mut i = tail.len().checked_sub(12)?;
    loop {
        let le = match &tail[i..i + 4] {
            b"XFIR" => true,
            b"RIFX" => false,
            _ => {
                if i == 0 {
                    return None;
                }
                i -= 1;
                continue;
            }
        };
        let size = {
            let b = [tail[i + 4], tail[i + 5], tail[i + 6], tail[i + 7]];
            if le { u32::from_le_bytes(b) } else { u32::from_be_bytes(b) }
        } as u64;
        let codec = {
            let t = [tail[i + 8], tail[i + 9], tail[i + 10], tail[i + 11]];
            if le { [t[3], t[2], t[1], t[0]] } else { t }
        };
        let chunk_end = start + i as u64 + 8 + size;
        if matches!(&codec, b"FGDM" | b"MV93") && chunk_end <= len && len - chunk_end <= 64 {
            return Some(Projector { offset: start + i as u64, len: 8 + size });
        }
        if i == 0 {
            return None;
        }
        i -= 1;
    }
}

/// The movie a projector carries, read out of it.
pub fn projector_movie(path: &str) -> Option<Vec<u8>> {
    use std::io::{Read, Seek, SeekFrom};
    let p = projector(path)?;
    let mut f = std::fs::File::open(path).ok()?;
    f.seek(SeekFrom::Start(p.offset)).ok()?;
    let mut movie = vec![0u8; p.len as usize];
    f.read_exact(&mut movie).ok()?;
    Some(movie)
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
    // 1.000.037: a projector with its movie inside starts the game as it does on a PC
    if let Some(exe) = with("exe").into_iter().find(|e| projector(&format!("{dir}/{e}")).is_some()) {
        return Some(exe.clone());
    }
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

    /// A Windows program (one section) with `movie` appended as a projector does: the `PJ01`
    /// marker after the section, filler, the movie, the marker's offset as a trailer.
    fn projector_bytes(movie: &[u8], filler: usize) -> Vec<u8> {
        let mut b = vec![0u8; 0x200];
        b[0] = b'M';
        b[1] = b'Z';
        b[0x3c..0x40].copy_from_slice(&0x80u32.to_le_bytes());
        b[0x80..0x84].copy_from_slice(b"PE\0\0");
        b[0x86..0x88].copy_from_slice(&1u16.to_le_bytes()); // one section
        b[0x80 + 20..0x80 + 22].copy_from_slice(&0xe0u16.to_le_bytes()); // optional header size
        let s = 0x80 + 24 + 0xe0;
        b[s + 16..s + 20].copy_from_slice(&0x1000u32.to_le_bytes()); // raw size
        b[s + 20..s + 24].copy_from_slice(&0x200u32.to_le_bytes()); // raw offset
        b.resize(0x1200, 0);
        let marker = b.len() as u32;
        b.extend_from_slice(b"10JP");
        b.extend_from_slice(&[0x11; 12]);
        b.extend(std::iter::repeat(0x58u8).take(filler));
        b.extend_from_slice(movie);
        b.extend_from_slice(&marker.to_le_bytes());
        b
    }

    fn movie_bytes(len: usize) -> Vec<u8> {
        let mut m = Vec::from(&b"XFIR"[..]);
        m.extend_from_slice(&((len - 8) as u32).to_le_bytes());
        m.extend_from_slice(b"MDGFrevF");
        m.push(14);
        m.extend_from_slice(&[0x8a, 0x01, 1, 0x8c, 0x40, 7]);
        m.extend_from_slice(b"8.0#178");
        m.resize(len, 0xab);
        m
    }

    #[test]
    fn finds_the_movie_in_a_projector() {
        let dir = std::env::temp_dir().join(format!("ruffle5-projector-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let exe = dir.join("game.exe");
        let exe_s = exe.to_str().unwrap().to_string();
        // a small movie near the end, as a stub projector has
        let m = movie_bytes(6162);
        std::fs::write(&exe, projector_bytes(&m, 1000)).unwrap();
        let p = projector(&exe_s).unwrap();
        assert_eq!((p.offset, p.len), (0x1200 + 16 + 1000, 6162));
        assert_eq!(projector_movie(&exe_s).unwrap(), m);
        assert_eq!(describe(&exe_s).as_deref(), Some("Director 8.0 projector"));
        // a movie bigger than the first window looked at, behind a lot of program data
        let big = movie_bytes(300_000);
        std::fs::write(&exe, projector_bytes(&big, 200_000)).unwrap();
        let p = projector(&exe_s).unwrap();
        assert_eq!((p.offset, p.len), (0x1200 + 16 + 200_000, 300_000));
        assert_eq!(projector_movie(&exe_s).unwrap(), big);
        // the folder starts by the projector, not by its first .dcr
        let files: Vec<String> = ["advert_loader.swf", "game.exe", "gameloader.dcr", "paul_blart.dcr"].iter().map(|s| s.to_string()).collect();
        assert_eq!(launcher(dir.to_str().unwrap(), &files).as_deref(), Some("game.exe"));
        // a program without a movie (a stub run by Projector.ini, or any other .exe) is not one
        std::fs::write(&exe, projector_bytes(&[], 0)[..0x1200].to_vec()).unwrap();
        assert_eq!(projector(&exe_s), None);
        assert_eq!(describe(&exe_s), None);
        assert_eq!(launcher(dir.to_str().unwrap(), &files).as_deref(), Some("gameloader.dcr"));
        // the real one, where it is on this machine
        let real = "/tmp/claude-0/-home-claude/3a8874bf-1cef-5042-823a-205604af376f/scratchpad/s3d/blart/mallcop.exe";
        if std::path::Path::new(real).is_file() {
            let p = projector(real).unwrap();
            assert_eq!((p.offset, p.len), (9542338, 6162));
            assert_eq!(describe(real).as_deref(), Some("Director 11.5.9 projector"));
        }
        let _ = std::fs::remove_dir_all(&dir);
    }
}
