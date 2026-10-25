//! A zip reader for Flashpoint's game data (1.000.028): the central directory, and each file
//! stored or deflated (methods 0 and 8) with its CRC-32 checked. No zip64 and no encryption — game
//! data is neither, and such an archive is refused with that reason. A name is taken as stored
//! (UTF-8; a name that is not is read as Latin-1).
// SPDX-License-Identifier: GPL-3.0-or-later

use std::fs::File;
use std::io::{Read, Seek, SeekFrom, Write};
use std::path::Path;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Entry {
    pub name: String,
    pub method: u16,
    pub flags: u16,
    pub crc: u32,
    pub compressed: u64,
    pub size: u64,
    /// Where its local header starts.
    pub offset: u64,
}

impl Entry {
    pub fn is_dir(&self) -> bool {
        self.name.ends_with('/')
    }
}

pub struct Zip {
    file: File,
    pub entries: Vec<Entry>,
}

fn u16_at(b: &[u8], at: usize) -> u16 {
    u16::from_le_bytes([b[at], b[at + 1]])
}

fn u32_at(b: &[u8], at: usize) -> u32 {
    u32::from_le_bytes([b[at], b[at + 1], b[at + 2], b[at + 3]])
}

fn name_of(raw: &[u8]) -> String {
    match std::str::from_utf8(raw) {
        Ok(s) => s.to_string(),
        Err(_) => raw.iter().map(|&b| b as char).collect(),
    }
}

impl Zip {
    pub fn open(path: &Path) -> Result<Zip, String> {
        let mut file = File::open(path).map_err(|e| format!("{}: {e}", path.display()))?;
        let len = file.metadata().map_err(|e| e.to_string())?.len();
        if len < 22 {
            return Err("not a zip (too short)".into());
        }
        // the end-of-central-directory record: in the last 22 bytes, or before a comment of up to 64 KiB
        let tail_len = len.min(22 + 0xFFFF);
        let mut tail = vec![0u8; tail_len as usize];
        file.seek(SeekFrom::Start(len - tail_len)).map_err(|e| e.to_string())?;
        file.read_exact(&mut tail).map_err(|e| e.to_string())?;
        let eocd = (0..=tail.len() - 22).rev().find(|&i| tail[i..i + 4] == [0x50, 0x4b, 0x05, 0x06]).ok_or("not a zip (no end of central directory)")?;
        let count = u16_at(&tail, eocd + 10) as usize;
        let cd_size = u32_at(&tail, eocd + 12) as u64;
        let cd_offset = u32_at(&tail, eocd + 16) as u64;
        if count == 0xFFFF || cd_offset == 0xFFFF_FFFF || cd_size == 0xFFFF_FFFF {
            return Err("a zip64 archive (Ruffle5 reads plain zips)".into());
        }
        if cd_offset + cd_size > len {
            return Err("a damaged zip (its directory is past its end)".into());
        }
        let mut cd = vec![0u8; cd_size as usize];
        file.seek(SeekFrom::Start(cd_offset)).map_err(|e| e.to_string())?;
        file.read_exact(&mut cd).map_err(|e| e.to_string())?;
        let mut entries = Vec::with_capacity(count);
        let mut at = 0usize;
        for _ in 0..count {
            if at + 46 > cd.len() || cd[at..at + 4] != [0x50, 0x4b, 0x01, 0x02] {
                return Err("a damaged zip (bad directory entry)".into());
            }
            let n = u16_at(&cd, at + 28) as usize;
            let x = u16_at(&cd, at + 30) as usize;
            let c = u16_at(&cd, at + 32) as usize;
            if at + 46 + n > cd.len() {
                return Err("a damaged zip (a name runs past the directory)".into());
            }
            entries.push(Entry {
                name: name_of(&cd[at + 46..at + 46 + n]),
                flags: u16_at(&cd, at + 8),
                method: u16_at(&cd, at + 10),
                crc: u32_at(&cd, at + 16),
                compressed: u32_at(&cd, at + 20) as u64,
                size: u32_at(&cd, at + 24) as u64,
                offset: u32_at(&cd, at + 42) as u64,
            });
            at += 46 + n + x + c;
        }
        Ok(Zip { file, entries })
    }

    /// Writes entry `i` to `out`, at most `limit` bytes; returns how many. Its CRC-32 is checked.
    pub fn extract(&mut self, i: usize, out: &mut dyn Write, limit: u64) -> Result<u64, String> {
        let e = self.entries.get(i).ok_or("no such entry")?.clone();
        if e.flags & 1 != 0 {
            return Err(format!("{}: encrypted", e.name));
        }
        if e.size > limit {
            return Err(format!("{}: {} bytes, more than {limit}", e.name, e.size));
        }
        let mut local = [0u8; 30];
        self.file.seek(SeekFrom::Start(e.offset)).map_err(|err| err.to_string())?;
        self.file.read_exact(&mut local).map_err(|err| err.to_string())?;
        if local[..4] != [0x50, 0x4b, 0x03, 0x04] {
            return Err(format!("{}: no local header", e.name));
        }
        let skip = u16_at(&local, 26) as i64 + u16_at(&local, 28) as i64;
        self.file.seek(SeekFrom::Current(skip)).map_err(|err| err.to_string())?;
        let raw = (&mut self.file).take(e.compressed);
        let mut reader: Box<dyn Read + '_> = match e.method {
            0 => Box::new(raw),
            8 => Box::new(flate2::read::DeflateDecoder::new(raw)),
            m => return Err(format!("{}: compression method {m} (Ruffle5 reads stored and deflated files)", e.name)),
        };
        let mut crc = flate2::Crc::new();
        let mut buf = vec![0u8; 1 << 16];
        let mut total = 0u64;
        loop {
            let n = reader.read(&mut buf).map_err(|err| format!("{}: {err}", e.name))?;
            if n == 0 {
                break;
            }
            total += n as u64;
            if total > limit {
                return Err(format!("{}: more than {limit} bytes", e.name));
            }
            crc.update(&buf[..n]);
            out.write_all(&buf[..n]).map_err(|err| format!("{}: {err}", e.name))?;
        }
        if total != e.size || crc.sum() != e.crc {
            return Err(format!("{}: damaged ({total} of {} bytes, CRC-32 {:08x} where the zip says {:08x})", e.name, e.size, crc.sum(), e.crc));
        }
        Ok(total)
    }

    /// The index of the entry named `name`, case and `\` / `/` aside.
    pub fn find(&self, name: &str) -> Option<usize> {
        let want = name.replace('\\', "/").to_lowercase();
        self.entries.iter().position(|e| e.name.replace('\\', "/").to_lowercase() == want)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A zip with one stored and one deflated file, built here (crc from flate2).
    fn sample() -> Vec<u8> {
        let files: [(&str, &[u8], bool); 2] = [("content.json", b"{\"a\":1}", false), ("content/h/x/game.dcr", b"XFIRXFIRXFIRXFIRXFIRXFIRXFIR", true)];
        let mut out = Vec::new();
        let mut cd = Vec::new();
        for (name, body, deflate) in files {
            let mut crc = flate2::Crc::new();
            crc.update(body);
            let data = if deflate {
                let mut e = flate2::write::DeflateEncoder::new(Vec::new(), flate2::Compression::default());
                e.write_all(body).unwrap();
                e.finish().unwrap()
            } else {
                body.to_vec()
            };
            let offset = out.len() as u32;
            let method: u16 = if deflate { 8 } else { 0 };
            out.extend_from_slice(&[0x50, 0x4b, 0x03, 0x04, 20, 0, 0, 0]);
            out.extend_from_slice(&method.to_le_bytes());
            out.extend_from_slice(&[0, 0, 0, 0]);
            out.extend_from_slice(&crc.sum().to_le_bytes());
            out.extend_from_slice(&(data.len() as u32).to_le_bytes());
            out.extend_from_slice(&(body.len() as u32).to_le_bytes());
            out.extend_from_slice(&(name.len() as u16).to_le_bytes());
            out.extend_from_slice(&0u16.to_le_bytes());
            out.extend_from_slice(name.as_bytes());
            out.extend_from_slice(&data);
            cd.extend_from_slice(&[0x50, 0x4b, 0x01, 0x02, 20, 0, 20, 0, 0, 0]);
            cd.extend_from_slice(&method.to_le_bytes());
            cd.extend_from_slice(&[0, 0, 0, 0]);
            cd.extend_from_slice(&crc.sum().to_le_bytes());
            cd.extend_from_slice(&(data.len() as u32).to_le_bytes());
            cd.extend_from_slice(&(body.len() as u32).to_le_bytes());
            cd.extend_from_slice(&(name.len() as u16).to_le_bytes());
            cd.extend_from_slice(&[0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0]);
            cd.extend_from_slice(&offset.to_le_bytes());
            cd.extend_from_slice(name.as_bytes());
        }
        let cd_offset = out.len() as u32;
        out.extend_from_slice(&cd);
        out.extend_from_slice(&[0x50, 0x4b, 0x05, 0x06, 0, 0, 0, 0, 2, 0, 2, 0]);
        out.extend_from_slice(&(cd.len() as u32).to_le_bytes());
        out.extend_from_slice(&cd_offset.to_le_bytes());
        out.extend_from_slice(&[0, 0]);
        out
    }

    #[test]
    fn reads_stored_and_deflated_files() {
        let dir = std::env::temp_dir().join(format!("r5net-zip-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("t.zip");
        std::fs::write(&path, sample()).unwrap();
        let mut z = Zip::open(&path).unwrap();
        assert_eq!(z.entries.iter().map(|e| e.name.as_str()).collect::<Vec<_>>(), ["content.json", "content/h/x/game.dcr"]);
        let i = z.find("CONTENT\\h\\x\\GAME.DCR").unwrap();
        let mut out = Vec::new();
        assert_eq!(z.extract(i, &mut out, 1 << 20).unwrap(), 28);
        assert!(out.starts_with(b"XFIR"));
        assert!(z.extract(i, &mut Vec::new(), 10).is_err());
        let mut bad = sample();
        let at = bad.windows(4).position(|w| w == b"{\"a\"").unwrap();
        bad[at + 1] = b'b';
        std::fs::write(&path, bad).unwrap();
        let mut z = Zip::open(&path).unwrap();
        assert!(z.extract(0, &mut Vec::new(), 1 << 20).unwrap_err().contains("CRC-32"));
        let _ = std::fs::remove_dir_all(&dir);
    }
}
