//! PS5_RUFFLE_PATHS_V1 — where Ruffle5 keeps things on the console.
//!
//! The shell (shell/main.c) passes three folders:
//!   * `data`     /data/ruffle5 — the log, settings, play times, badges, saves, covers. Ruffle5's
//!                own: the other Flash player for the PS5 (Ruffle-Flash-PS5) uses /data/ruffle, and
//!                the two must not overwrite each other's log and settings.
//!   * `games`    /data/homebrew/PPSA99620/games — the SWFs the menu lists.
//!   * `mappings` /data/homebrew/PPSA99620/Custom Mappings — every controls file: the presets that
//!                ship with the title (dadnme.controls.txt), the ones the controls editor writes, and
//!                "_All Games.controls.txt" for every game.
//! `/app0` is the title's own (read-only) folder: the shipped controls.txt and presets.
// SPDX-License-Identifier: GPL-3.0-or-later

#[derive(Clone, Debug)]
pub struct Paths {
    pub data: String,
    pub games: String,
    pub mappings: String,
    pub app: String,
    /// Where Ruffle5 kept its files before 1.000.020's Custom Mappings: /data/ruffle.
    pub legacy: String,
}

impl Paths {
    pub fn new(data: &str, games: &str, mappings: &str) -> Paths {
        Paths {
            data: data.to_string(),
            games: games.to_string(),
            mappings: mappings.to_string(),
            app: "/app0".to_string(),
            legacy: "/data/ruffle".to_string(),
        }
    }

    pub fn file(&self, name: &str) -> String {
        format!("{}/{name}", self.data)
    }

    pub fn saves(&self, key: &str) -> String {
        format!("{}/saves/{key}", self.data)
    }

    pub fn cover(&self, key: &str) -> String {
        format!("{}/covers/{key}.png", self.data)
    }
}

/// A file-name-safe key for a game, from its file name ("Super Mario 63.swf" -> "Super_Mario_63").
pub fn game_key(file_name: &str) -> String {
    let stem = match file_name.rsplit_once('.') {
        Some((s, ext)) if ext.eq_ignore_ascii_case("swf") => s,
        _ => file_name,
    };
    let key: String = stem
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() || "._-".contains(c) { c } else { '_' })
        .collect();
    if key.is_empty() {
        "game".to_string()
    } else {
        key
    }
}

/// FNV-1a of a path, to tell apart two games whose keys collide.
pub fn path_hash(path: &str) -> u32 {
    let mut h: u32 = 2166136261;
    for b in path.bytes() {
        h ^= b as u32;
        h = h.wrapping_mul(16777619);
    }
    h
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keys_are_file_safe() {
        assert_eq!(game_key("Super Mario 63.swf"), "Super_Mario_63");
        assert_eq!(game_key("dadnme.SWF"), "dadnme");
        assert_eq!(game_key("a/b.swf"), "a_b");
        assert_eq!(game_key(".swf"), "game");
    }
}
