//! PS5_RUFFLE_LIBRARY_V1 — what Ruffle5 remembers about each game: favourite or not, when it was
//! last played, how long it has been played in all, how many times started. One line per game in
//! /data/ruffle5/library.txt (`key<TAB>fav<TAB>last<TAB>secs<TAB>launches`), keyed by the game's
//! file-safe key (src/paths.rs `game_key`).
//!
//! A running game's play time is also written every 30 s to session.txt, so a game cut off by a
//! shutdown or a crash is still counted the next time Ruffle5 starts (as Ruffle-Flash-PS5 does).
// SPDX-License-Identifier: GPL-3.0-or-later

use std::collections::HashMap;

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Record {
    pub favorite: bool,
    pub last_played: u64,
    pub play_secs: u64,
    pub launches: u32,
}

pub struct Library {
    path: String,
    session_path: String,
    pub records: HashMap<String, Record>,
}

pub fn now_secs() -> u64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0)
}

impl Library {
    pub fn load(path: &str, session_path: &str) -> Library {
        let mut records = HashMap::new();
        if let Ok(text) = std::fs::read_to_string(path) {
            for line in text.lines() {
                let f: Vec<&str> = line.split('\t').collect();
                if f.len() < 5 || f[0].is_empty() || f[0].starts_with('#') {
                    continue;
                }
                records.insert(
                    f[0].to_string(),
                    Record {
                        favorite: f[1] == "1",
                        last_played: f[2].parse().unwrap_or(0),
                        play_secs: f[3].parse().unwrap_or(0),
                        launches: f[4].parse().unwrap_or(0),
                    },
                );
            }
        }
        Library {
            path: path.to_string(),
            session_path: session_path.to_string(),
            records,
        }
    }

    pub fn get(&self, key: &str) -> Record {
        self.records.get(key).copied().unwrap_or_default()
    }

    fn text(&self) -> String {
        let mut keys: Vec<&String> = self.records.keys().collect();
        keys.sort();
        let mut out = String::from("# Ruffle5 library: key, favorite, last played (unix), seconds played, launches\n");
        for k in keys {
            let r = self.records[k];
            out.push_str(&format!("{k}\t{}\t{}\t{}\t{}\n", r.favorite as u8, r.last_played, r.play_secs, r.launches));
        }
        out
    }

    pub fn save(&self) {
        crate::fsutil::write_in_background(self.path.as_str(), self.text().into_bytes());
    }

    pub fn toggle_favorite(&mut self, key: &str) -> bool {
        let r = self.records.entry(key.to_string()).or_default();
        r.favorite = !r.favorite;
        let fav = r.favorite;
        self.save();
        fav
    }

    pub fn mark_started(&mut self, key: &str) {
        let r = self.records.entry(key.to_string()).or_default();
        r.last_played = now_secs();
        r.launches += 1;
        self.save();
    }

    pub fn add_play_time(&mut self, key: &str, secs: u64) {
        if secs == 0 {
            return;
        }
        self.records.entry(key.to_string()).or_default().play_secs += secs;
        self.save();
    }

    pub fn clear_recent(&mut self) {
        for r in self.records.values_mut() {
            r.last_played = 0;
        }
        self.save();
    }

    pub fn favorites(&self) -> usize {
        self.records.values().filter(|r| r.favorite).count()
    }

    pub fn played(&self) -> usize {
        self.records.values().filter(|r| r.launches > 0).count()
    }

    /// The running game's progress so far (written in the background).
    pub fn write_session(&self, key: &str, secs: u64) {
        crate::fsutil::write_in_background(self.session_path.as_str(), format!("{key}\n{secs}\n").into_bytes());
    }

    pub fn clear_session(&self) {
        crate::fsutil::remove_in_background(self.session_path.as_str());
    }

    /// A session a crash or shutdown cut off: counted now. Returns (key, seconds).
    pub fn recover_session(&mut self) -> Option<(String, u64)> {
        let text = std::fs::read_to_string(&self.session_path).ok()?;
        let _ = std::fs::remove_file(&self.session_path);
        let mut lines = text.lines();
        let key = lines.next()?.trim().to_string();
        let secs: u64 = lines.next()?.trim().parse().ok()?;
        if key.is_empty() {
            return None;
        }
        self.add_play_time(&key, secs);
        Some((key, secs))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn records_round_trip() {
        let dir = std::env::temp_dir().join(format!("ruffle5-library-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("library.txt");
        let session = dir.join("session.txt");
        let mut lib = Library::load(path.to_str().unwrap(), session.to_str().unwrap());
        lib.toggle_favorite("dadnme");
        lib.mark_started("dadnme");
        lib.add_play_time("dadnme", 125);
        std::fs::write(&path, lib.text()).unwrap();
        let back = Library::load(path.to_str().unwrap(), session.to_str().unwrap());
        let r = back.get("dadnme");
        assert!(r.favorite);
        assert_eq!((r.play_secs, r.launches), (125, 1));
        assert!(r.last_played > 0);
        assert_eq!(back.favorites(), 1);
        std::fs::write(&session, "dadnme\n60\n").unwrap();
        let mut back = back;
        assert_eq!(back.recover_session(), Some(("dadnme".to_string(), 60)));
        assert_eq!(back.get("dadnme").play_secs, 185);
        assert!(!session.exists());
        let _ = std::fs::remove_dir_all(&dir);
    }
}
