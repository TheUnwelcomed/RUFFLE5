//! PS5_RUFFLE_DIR3D_V1 — does a Director game use Shockwave 3D? The menu's categories (1.000.033:
//! All, Flash, Shockwave, Shockwave 3D; R2 on the shelves).
//!
//! A Shockwave 3D cast member is an Xtra member whose record (`CASt`) holds the Xtra's type as a
//! 4-byte big-endian length and "shockwave3d" — DirPlayer's own test (`is_shockwave3d_ole`,
//! vm-rust/src/player/cast_member.rs). Found that way in Supersonic RC's game.dcr (5 members) and
//! Paul Blart: Mall Cop's paul_blart.dcr (7), in none of Trick or Treat Beat's two movies, Teen
//! Titans Battle Blitz's, Paul Blart's gameloader.dcr or its projector stub. The movie's Xtra list
//! (`XTRl`) is not enough: gameloader.dcr and the projector stub list "Shockwave 3D Asset.x32"
//! and have no 3D member.
//!
//! An afterburned file (.dcr, .cct; codec `FGDM`/`FGDC`): `Fver`, `Fcdr`, `ABMP` (the map of its
//! resources, zlib), `FGEI` then the initial load segment (zlib); a `CASt` is in that segment, or
//! at its offset after the segment's start, zlib-compressed (DirPlayer's `read_after_burner_map`
//! and `get_chunk_data`). A plain one (.dir, .dxr, .cst, .cxt; `MV93`): `imap` -> `mmap`, each
//! entry's tag, length and offset.
//!
//! A game in a folder of its own is 3D when any Director file beside its movie has a 3D member
//! (Paul Blart's folder starts gameloader.dcr, which loads paul_blart.dcr). The answers are kept in
//! `<data>/shockwave3d.txt` by file size and time, so a file is read once.
// SPDX-License-Identifier: GPL-3.0-or-later

use std::collections::HashMap;
use std::io::Read;
use std::path::Path;

/// What DirPlayer looks for: the Xtra type's length (11, big-endian) and the type.
const MARK: &[u8] = b"\x00\x00\x00\x0bshockwave3d";

/// Director files: movies and casts.
pub fn is_director_file(name: &str) -> bool {
    name.rsplit_once('.').is_some_and(|(_, e)| ["dcr", "dir", "dxr", "cct", "cst", "cxt"].iter().any(|x| e.eq_ignore_ascii_case(x)))
}

/// Has the Director file `bytes` a Shockwave 3D cast member?
pub fn has_3d(bytes: &[u8]) -> Result<bool, String> {
    let mut found = false;
    resources(bytes, &|t| t == b"CASt", &mut |_, d| {
        found = contains(d, MARK);
        found
    })?;
    Ok(found)
}

/// 1.000.035: each resource of the Director file `bytes` whose tag `want` takes, as the file keeps
/// it (a plain file) or decompressed (an afterburned one), in the order of its ids, until `visit`
/// says to stop (true). Pretend to be on, Automatic (src/pretend.rs), reads a movie's text so.
pub(crate) fn resources(bytes: &[u8], want: &dyn Fn(&[u8; 4]) -> bool, visit: &mut dyn FnMut(&[u8; 4], &[u8]) -> bool) -> Result<(), String> {
    if bytes.len() < 12 {
        return Err("too short for a Director file".into());
    }
    let le = match &bytes[0..4] {
        b"XFIR" => true,
        b"RIFX" => false,
        _ => return Err("not a Director file".into()),
    };
    let r = Reader { b: bytes, p: 8, le };
    match &r.tag_at(8)? {
        b"FGDM" | b"FGDC" => afterburned(bytes, le, want, visit),
        _ => plain(bytes, le, want, visit),
    }
}

struct Reader<'a> {
    b: &'a [u8],
    p: usize,
    le: bool,
}

impl Reader<'_> {
    fn tag_at(&self, at: usize) -> Result<[u8; 4], String> {
        let t: [u8; 4] = self.b.get(at..at + 4).ok_or("cut short")?.try_into().map_err(|_| "cut short")?;
        Ok(if self.le { [t[3], t[2], t[1], t[0]] } else { t })
    }
    fn tag(&mut self) -> Result<[u8; 4], String> {
        let t = self.tag_at(self.p)?;
        self.p += 4;
        Ok(t)
    }
    fn expect(&mut self, want: &[u8; 4]) -> Result<(), String> {
        let t = self.tag()?;
        if &t == want {
            Ok(())
        } else {
            Err(format!("{} expected, {} found", String::from_utf8_lossy(want), String::from_utf8_lossy(&t)))
        }
    }
    /// Director's variable-length number: 7 bits a byte, the high bit set on all but the last.
    fn var(&mut self) -> Result<usize, String> {
        let mut v: usize = 0;
        for _ in 0..5 {
            let x = *self.b.get(self.p).ok_or("cut short")?;
            self.p += 1;
            v = (v << 7) | (x & 0x7f) as usize;
            if x & 0x80 == 0 {
                return Ok(v);
            }
        }
        Err("a number too long".into())
    }
    fn u32_at(&self, at: usize) -> Result<u32, String> {
        let x: [u8; 4] = self.b.get(at..at + 4).ok_or("cut short")?.try_into().map_err(|_| "cut short")?;
        Ok(if self.le { u32::from_le_bytes(x) } else { u32::from_be_bytes(x) })
    }
    fn u16_at(&self, at: usize) -> Result<u16, String> {
        let x: [u8; 2] = self.b.get(at..at + 2).ok_or("cut short")?.try_into().map_err(|_| "cut short")?;
        Ok(if self.le { u16::from_le_bytes(x) } else { u16::from_be_bytes(x) })
    }
}

fn inflate(data: &[u8], limit: usize) -> Result<Vec<u8>, String> {
    let mut out = Vec::new();
    flate2::read::ZlibDecoder::new(data).take(limit as u64).read_to_end(&mut out).map_err(|e| format!("zlib: {e}"))?;
    Ok(out)
}

fn contains(hay: &[u8], needle: &[u8]) -> bool {
    hay.windows(needle.len()).any(|w| w == needle)
}

fn afterburned(b: &[u8], le: bool, want: &dyn Fn(&[u8; 4]) -> bool, visit: &mut dyn FnMut(&[u8; 4], &[u8]) -> bool) -> Result<(), String> {
    let mut r = Reader { b, p: 12, le };
    r.expect(b"Fver")?;
    let n = r.var()?;
    r.p += n;
    r.expect(b"Fcdr")?;
    let n = r.var()?;
    r.p += n;
    r.expect(b"ABMP")?;
    let n = r.var()?;
    let end = r.p + n;
    r.var()?; // compression type
    let map_len = r.var()?;
    let map = inflate(b.get(r.p..end).ok_or("ABMP cut short")?, map_len.max(1) * 2)?;
    r.p = end;
    let mut m = Reader { b: &map, p: 0, le };
    m.var()?;
    m.var()?;
    let count = m.var()?;
    // id -> (tag, offset after the initial load segment's start, size stored, size)
    let mut res: HashMap<usize, ([u8; 4], usize, usize, usize)> = HashMap::new();
    for _ in 0..count {
        let id = m.var()?;
        let offset = m.var()?;
        let stored = m.var()?;
        let size = m.var()?;
        m.var()?; // compression
        let tag = m.tag()?;
        res.insert(id, (tag, offset, stored, size));
    }
    r.expect(b"FGEI")?;
    r.var()?;
    let body = r.p;
    let &(_, _, ils_stored, ils_size) = res.get(&2).ok_or("no initial load segment")?;
    let ils = inflate(b.get(body..(body + ils_stored).min(b.len())).ok_or("the initial load segment cut short")?, ils_size.max(1) * 2)?;
    // the resources in the initial load segment
    let mut loaded: HashMap<usize, (usize, usize)> = HashMap::new();
    let mut s = Reader { b: &ils, p: 0, le };
    while s.p < ils.len() {
        let id = s.var()?;
        let Some(&(_, _, stored, _)) = res.get(&id) else {
            break;
        };
        loaded.insert(id, (s.p, stored));
        s.p += stored;
    }
    let mut ids: Vec<usize> = res.keys().copied().filter(|&id| id != 2).collect();
    ids.sort_unstable();
    for id in ids {
        let (tag, offset, stored, size) = res[&id];
        if !want(&tag) {
            continue;
        }
        let stop = match loaded.get(&id) {
            Some(&(at, len)) => match ils.get(at..(at + len).min(ils.len())) {
                Some(d) => visit(&tag, d),
                None => false,
            },
            None => {
                let Some(raw) = b.get(body + offset..(body + offset + stored).min(b.len())) else {
                    continue;
                };
                // zlib, or stored as it is (its size is no sign: a record can compress to its own size)
                match inflate(raw, size.max(1) * 2) {
                    Ok(d) => visit(&tag, &d),
                    Err(_) => visit(&tag, raw),
                }
            }
        };
        if stop {
            return Ok(());
        }
    }
    Ok(())
}

fn plain(b: &[u8], le: bool, want: &dyn Fn(&[u8; 4]) -> bool, visit: &mut dyn FnMut(&[u8; 4], &[u8]) -> bool) -> Result<(), String> {
    let r = Reader { b, p: 12, le };
    if &r.tag_at(12)? != b"imap" {
        return Err("imap expected".into());
    }
    let mmap = r.u32_at(24)? as usize;
    if &r.tag_at(mmap)? != b"mmap" {
        return Err("mmap expected".into());
    }
    let header = r.u16_at(mmap + 8)? as usize;
    let entry = r.u16_at(mmap + 10)? as usize;
    let used = r.u32_at(mmap + 16)? as usize;
    if entry < 12 {
        return Err("mmap entries too small".into());
    }
    for i in 0..used {
        let at = mmap + 8 + header + i * entry;
        let tag = r.tag_at(at)?;
        if !want(&tag) {
            continue;
        }
        let len = r.u32_at(at + 4)? as usize;
        let off = r.u32_at(at + 8)? as usize;
        if let Some(d) = b.get(off + 8..(off + 8 + len).min(b.len())) {
            if visit(&tag, d) {
                return Ok(());
            }
        }
    }
    Ok(())
}

/// The answers so far, by file: (size, modified, 3D).
pub struct Cache {
    path: String,
    files: HashMap<String, (u64, u64, bool)>,
    changed: bool,
}

fn stamp(path: &Path) -> Option<(u64, u64)> {
    let m = std::fs::metadata(path).ok()?;
    let t = m.modified().ok().and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok()).map(|d| d.as_secs()).unwrap_or(0);
    Some((m.len(), t))
}

impl Cache {
    pub fn load(path: &str) -> Cache {
        let mut files = HashMap::new();
        if let Ok(text) = std::fs::read_to_string(path) {
            for line in text.lines().filter(|l| !l.starts_with('#')) {
                let f: Vec<&str> = line.splitn(4, '\t').collect();
                if let [size, time, d3, file] = f[..] {
                    if let (Ok(size), Ok(time)) = (size.parse(), time.parse()) {
                        files.insert(file.to_string(), (size, time, d3 == "1"));
                    }
                }
            }
        }
        Cache { path: path.to_string(), files, changed: false }
    }

    /// Has the file a 3D member (read again when its size or time changed)? None: not readable.
    pub fn file(&mut self, path: &Path, report: &mut Vec<String>) -> Option<bool> {
        let (size, time) = stamp(path)?;
        let key = path.to_string_lossy().into_owned();
        if let Some(&(s, t, d3)) = self.files.get(&key) {
            if s == size && t == time {
                return Some(d3);
            }
        }
        let t0 = std::time::Instant::now();
        let d3 = match std::fs::read(path).map_err(|e| e.to_string()).and_then(|b| has_3d(&b)) {
            Ok(d3) => d3,
            Err(e) => {
                report.push(format!("menu: {key}: not looked into for Shockwave 3D: {e}"));
                false
            }
        };
        if d3 {
            report.push(format!("menu: {key}: Shockwave 3D (read in {} ms)", t0.elapsed().as_millis()));
        }
        self.files.insert(key, (size, time, d3));
        self.changed = true;
        Some(d3)
    }

    /// A game: its movie, and when it is in a folder of its own every Director file beside it.
    pub fn game(&mut self, movie: &str, own_folder: bool, report: &mut Vec<String>) -> bool {
        let movie = Path::new(movie);
        // (1.000.037: a projector, mallcop.exe, is not read: the movies beside it are)
        let is_file = is_director_file(&movie.file_name().unwrap_or_default().to_string_lossy());
        if is_file && self.file(movie, report) == Some(true) {
            return true;
        }
        if !own_folder {
            return false;
        }
        let Some(dir) = movie.parent() else {
            return false;
        };
        let Ok(it) = std::fs::read_dir(dir) else {
            return false;
        };
        let mut others: Vec<std::path::PathBuf> =
            it.filter_map(|e| e.ok()).map(|e| e.path()).filter(|p| p != movie && p.is_file() && is_director_file(&p.file_name().unwrap_or_default().to_string_lossy())).collect();
        others.sort();
        others.into_iter().any(|p| self.file(&p, report) == Some(true))
    }

    /// Write the answers when there are new ones (the files gone are left out).
    pub fn save(&mut self) {
        if !self.changed {
            return;
        }
        self.changed = false;
        let mut keys: Vec<&String> = self.files.keys().filter(|k| Path::new(k).exists()).collect();
        keys.sort();
        let mut out = String::from("# Ruffle5: Director files with a Shockwave 3D cast member (size, modified, 1 = 3D, file)\n");
        for k in keys {
            let (s, t, d3) = self.files[k];
            out.push_str(&format!("{s}\t{t}\t{}\t{k}\n", d3 as u8));
        }
        crate::fsutil::write_in_background(self.path.as_str(), out.into_bytes());
    }
}

/// Small Director files made here (the tests of this file and the menu's).
#[cfg(test)]
pub(crate) mod sample {
    use super::MARK;
    use flate2::write::ZlibEncoder;
    use std::io::Write;

    fn zlib(d: &[u8]) -> Vec<u8> {
        let mut e = ZlibEncoder::new(Vec::new(), flate2::Compression::default());
        e.write_all(d).unwrap();
        e.finish().unwrap()
    }

    fn var(v: usize) -> Vec<u8> {
        let mut out = vec![(v & 0x7f) as u8];
        let mut v = v >> 7;
        while v > 0 {
            out.insert(0, 0x80 | (v & 0x7f) as u8);
            v >>= 7;
        }
        out
    }

    /// A little-endian afterburned file (XFIR) with these CASt records: those marked `true` in the
    /// initial load segment, the others after it, zlib-compressed.
    pub(crate) fn afterburned_file(casts: &[(&[u8], bool)]) -> Vec<u8> {
        let rev = |t: &[u8; 4]| [t[3], t[2], t[1], t[0]];
        // the initial load segment: resource 2 (itself) is not in it; CASt ids from 10
        let mut ils = Vec::new();
        let mut outside = Vec::new();
        let mut entries: Vec<(usize, usize, usize, usize, [u8; 4])> = Vec::new();
        for (i, (data, in_ils)) in casts.iter().enumerate() {
            let id = 10 + i;
            if *in_ils {
                ils.extend(var(id));
                ils.extend_from_slice(data);
                entries.push((id, 0, data.len(), data.len(), *b"CASt"));
            } else {
                let z = zlib(data);
                entries.push((id, 0, z.len(), data.len(), *b"CASt"));
                outside.push((entries.len() - 1, z));
            }
        }
        let ils_z = zlib(&ils);
        // the others follow the segment, at offsets from its start
        let mut after = Vec::new();
        for (k, z) in &outside {
            entries[*k].1 = ils_z.len() + after.len();
            after.extend_from_slice(z);
        }
        entries.insert(0, (2, 0, ils_z.len(), ils.len(), *b"ILS "));
        let mut map = Vec::new();
        map.extend(var(0));
        map.extend(var(0));
        map.extend(var(entries.len()));
        for (id, off, stored, size, tag) in &entries {
            map.extend(var(*id));
            map.extend(var(*off));
            map.extend(var(*stored));
            map.extend(var(*size));
            map.extend(var(0));
            map.extend_from_slice(&rev(tag));
        }
        let map_z = zlib(&map);
        let mut f = Vec::new();
        f.extend_from_slice(b"XFIR");
        f.extend_from_slice(&[0, 0, 0, 0]);
        f.extend_from_slice(&rev(b"FGDM"));
        f.extend_from_slice(&rev(b"Fver"));
        f.extend(var(1));
        f.push(0);
        let fcdr = zlib(&[0, 0]);
        f.extend_from_slice(&rev(b"Fcdr"));
        f.extend(var(fcdr.len()));
        f.extend_from_slice(&fcdr);
        let mut abmp = var(0);
        abmp.extend(var(map.len()));
        abmp.extend_from_slice(&map_z);
        f.extend_from_slice(&rev(b"ABMP"));
        f.extend(var(abmp.len()));
        f.extend_from_slice(&abmp);
        f.extend_from_slice(&rev(b"FGEI"));
        f.extend(var(0));
        f.extend_from_slice(&ils_z);
        f.extend_from_slice(&after);
        f
    }

    pub(crate) fn cast_3d() -> Vec<u8> {
        let mut c = vec![0, 0, 0, 15, 0, 0, 0, 0, 0, 0, 0, 30];
        c.extend_from_slice(MARK);
        c.extend_from_slice(b"3DPR....");
        c
    }

    /// A plain big-endian file (RIFX, MV93): imap -> mmap, these records as `tag` resources.
    pub(crate) fn plain_file(records: &[(&[u8; 4], &[u8])]) -> Vec<u8> {
        let mut f = Vec::new();
        f.extend_from_slice(b"RIFX\0\0\0\0MV93");
        f.extend_from_slice(b"imap");
        f.extend_from_slice(&24u32.to_be_bytes());
        f.extend_from_slice(&1u32.to_be_bytes());
        let mmap_at = 12 + 8 + 24;
        f.extend_from_slice(&(mmap_at as u32).to_be_bytes());
        f.resize(mmap_at, 0);
        let header = 24usize;
        let entry = 20usize;
        let n = records.len();
        let data_at = mmap_at + 8 + header + n * entry;
        f.extend_from_slice(b"mmap");
        f.extend_from_slice(&((header + n * entry) as u32).to_be_bytes());
        f.extend_from_slice(&(header as u16).to_be_bytes());
        f.extend_from_slice(&(entry as u16).to_be_bytes());
        f.extend_from_slice(&(n as u32).to_be_bytes());
        f.extend_from_slice(&(n as u32).to_be_bytes());
        f.extend_from_slice(&[0xff; 8]);
        f.extend_from_slice(&0u32.to_be_bytes());
        let mut at = data_at;
        for (tag, r) in records {
            f.extend_from_slice(*tag);
            f.extend_from_slice(&(r.len() as u32).to_be_bytes());
            f.extend_from_slice(&(at as u32).to_be_bytes());
            f.extend_from_slice(&[0; 8]);
            at += 8 + r.len();
        }
        for (tag, r) in records {
            f.extend_from_slice(*tag);
            f.extend_from_slice(&(r.len() as u32).to_be_bytes());
            f.extend_from_slice(r);
        }
        f
    }

    /// An afterburned movie with a bitmap's record, and a Shockwave 3D member when `three_d`.
    pub(crate) fn movie(three_d: bool) -> Vec<u8> {
        let d3 = cast_3d();
        let mut casts: Vec<(&[u8], bool)> = vec![(b"\x00\x00\x00\x01bitmap", true)];
        if three_d {
            casts.push((&d3, false));
        }
        afterburned_file(&casts)
    }
}

#[cfg(test)]
mod tests {
    use super::sample::{afterburned_file, cast_3d};
    use super::*;

    #[test]
    fn a_3d_member_is_found_where_director_keeps_it() {
        let bitmap: &[u8] = b"\x00\x00\x00\x01 a bitmap's record";
        let flash_xtra: &[u8] = b"\x00\x00\x00\x0f\x00\x00\x00\x00\x00\x00\x00\x09\x00\x00\x00\x05flash";
        let d3 = cast_3d();
        assert_eq!(has_3d(&afterburned_file(&[(bitmap, true), (flash_xtra, false)])), Ok(false));
        assert_eq!(has_3d(&afterburned_file(&[(bitmap, true), (&d3, true)])), Ok(true), "in the initial load segment");
        assert_eq!(has_3d(&afterburned_file(&[(bitmap, true), (&d3, false)])), Ok(true), "after it, compressed");
        // the Xtra's name somewhere else in a record (a member called "shockwave3d") is not a 3D member
        assert_eq!(has_3d(&afterburned_file(&[(b"\x00\x00\x00\x03shockwave3d", true)])), Ok(false));
        assert!(has_3d(b"FWS\x08 not director").is_err());
        assert!(has_3d(&afterburned_file(&[(&d3, true)])[..40]).is_err(), "cut short: an error, not a guess");
    }

    #[test]
    fn a_plain_movie_is_read_through_its_memory_map() {
        // RIFX (big-endian): imap -> mmap with two entries (a bitmap's record, a 3D member's)
        let build = |second: &[u8]| -> Vec<u8> { super::sample::plain_file(&[(b"CASt", b"\x00\x00\x00\x01bitmap"), (b"CASt", second)]) };
        assert_eq!(has_3d(&build(&cast_3d())), Ok(true));
        assert_eq!(has_3d(&build(b"\x00\x00\x00\x03text")), Ok(false));
    }

    #[test]
    fn answers_are_kept_by_size_and_time_and_a_folder_counts_its_casts() {
        let dir = std::env::temp_dir().join(format!("ruffle5-dir3d-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join("Blart")).unwrap();
        let plain2d = afterburned_file(&[(b"\x00\x00\x00\x01bitmap", true)]);
        std::fs::write(dir.join("Blart/gameloader.dcr"), &plain2d).unwrap();
        std::fs::write(dir.join("Blart/paul_blart.dcr"), afterburned_file(&[(&cast_3d(), false)])).unwrap();
        std::fs::write(dir.join("lone.dcr"), &plain2d).unwrap();
        let list = dir.join("shockwave3d.txt");
        let mut c = Cache::load(&list.to_string_lossy());
        let mut report = Vec::new();
        let loader = dir.join("Blart/gameloader.dcr").to_string_lossy().into_owned();
        assert!(!c.game(&loader, false, &mut report), "the loader alone has no 3D member");
        assert!(c.game(&loader, true, &mut report), "its folder has");
        assert!(!c.game(&dir.join("lone.dcr").to_string_lossy(), false, &mut report));
        assert!(report.iter().any(|l| l.contains("paul_blart.dcr: Shockwave 3D")), "{report:?}");
        // written, read back, used without reading the files again
        c.save();
        // written on the writer thread
        for _ in 0..200 {
            if std::fs::read_to_string(&list).is_ok_and(|t| t.lines().count() == 4) {
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(10));
        }
        let mut again = Cache::load(&list.to_string_lossy());
        assert_eq!(again.files.len(), 3);
        std::fs::write(dir.join("Blart/paul_blart.dcr"), b"RIFX broken now").unwrap();
        // same size and time? (written within the same second: the size differs here, so it is read)
        let mut report = Vec::new();
        assert!(!again.game(&loader, true, &mut report));
        assert!(report.iter().any(|l| l.contains("not looked into")), "{report:?}");
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// The real movies, when given: `R5_DIR3D=path=1;path=0` (host only).
    #[test]
    fn real_movies() {
        let Ok(list) = std::env::var("R5_DIR3D") else {
            return;
        };
        for item in list.split(';').filter(|s| !s.is_empty()) {
            let (path, want) = item.rsplit_once('=').unwrap();
            let bytes = std::fs::read(path).unwrap();
            let t0 = std::time::Instant::now();
            let got = has_3d(&bytes);
            eprintln!("{path}: {got:?} in {:.1} ms ({} bytes)", t0.elapsed().as_secs_f64() * 1000.0, bytes.len());
            assert_eq!(got, Ok(want == "1"), "{path}");
        }
    }
}
