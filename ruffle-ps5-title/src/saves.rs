//! PS5_RUFFLE_SAVES_V1 — Flash saves (SharedObjects, the ".sol" files) on disk, one folder per game:
//! /data/ruffle5/saves/<game>/. Ruffle's own default (`MemoryStorageBackend`, which Ruffle5 used
//! until now) keeps them in memory only, so every game forgot its progress when it closed.
//!
//! Ported from Ruffle-Flash-PS5 (src/saves.rs, GPL-3.0, by ZiZc3). Many games never call
//! SharedObject.flush() and count on the player writing their saves when it closes; the game loop
//! flushes every few seconds as well, so a console shut down mid-game keeps the progress. A save is
//! written only when it changed, on the background writer, so the game never waits for the disk.
// SPDX-License-Identifier: GPL-3.0-or-later

use ruffle_core::backend::storage::StorageBackend;
use std::collections::HashMap;
use std::path::PathBuf;

pub struct DiskStorage {
    dir: PathBuf,
    /// Each save as last read or written (newer than the disk while the writer catches up).
    known: HashMap<String, Vec<u8>>,
}

impl DiskStorage {
    pub fn new(dir: &str) -> Self {
        let dir = PathBuf::from(dir);
        if let Err(e) = std::fs::create_dir_all(&dir) {
            crate::say(&format!("warn saves: cannot create {}: {e}", dir.display()));
        }
        DiskStorage { dir, known: HashMap::new() }
    }

    /// Ruffle names a save like "localhost/path/to/movie.swf/name"; one file each.
    fn file(&self, name: &str) -> PathBuf {
        let safe: String = name.chars().map(|c| if c.is_ascii_alphanumeric() || "._-".contains(c) { c } else { '_' }).collect();
        self.dir.join(format!("{safe}.sol"))
    }
}

impl StorageBackend for DiskStorage {
    fn get(&self, name: &str) -> Option<Vec<u8>> {
        match self.known.get(name) {
            Some(v) => Some(v.clone()),
            None => std::fs::read(self.file(name)).ok(),
        }
    }

    fn put(&mut self, name: &str, value: &[u8]) -> bool {
        let path = self.file(name);
        let same = match self.known.get(name) {
            Some(v) => v.as_slice() == value,
            None => std::fs::read(&path).map(|v| v == value).unwrap_or(false),
        };
        if !same {
            crate::fsutil::write_in_background(path, value.to_vec());
        }
        self.known.insert(name.to_string(), value.to_vec());
        true
    }

    fn remove_key(&mut self, name: &str) {
        self.known.remove(name);
        crate::fsutil::remove_in_background(self.file(name));
    }
}
