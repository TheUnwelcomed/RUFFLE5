//! What every online source hands the menu: pages of items, and for one item the files to fetch.
// SPDX-License-Identifier: GPL-3.0-or-later

use crate::http::{Client, Error};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Source {
    InternetArchive,
    FlashMuseum,
    /// 1.000.028: Flashpoint Archive's Shockwave games (src/fp.rs).
    Flashpoint,
}

/// The sources Flash Browser lists, in its order. 1.000.032: Flash Museum is no longer one of them.
/// Its file server refused the movies on the console ("403 Forbidden" for games.flashmuseum.net,
/// 2026-10-25), and Ruffle5 does not try to get around a site refusing it. Its code (src/fm.rs)
/// stays; games fetched from it before still play (their online.txt names the movie).
pub const SOURCES: [Source; 2] = [Source::InternetArchive, Source::Flashpoint];

impl Source {
    pub fn name(self) -> &'static str {
        match self {
            Source::InternetArchive => "Internet Archive",
            Source::FlashMuseum => "Flash Museum",
            Source::Flashpoint => "Flashpoint",
        }
    }

    /// The site, as online.txt records it.
    pub fn site(self) -> &'static str {
        match self {
            Source::InternetArchive => "archive.org",
            Source::FlashMuseum => "flashmuseum.net",
            Source::Flashpoint => "flashpointarchive.org",
        }
    }

    /// What it lists: Flash movies, or (Flashpoint, as Ruffle5 asks it) Shockwave ones.
    pub fn plays(self) -> &'static str {
        match self {
            Source::InternetArchive | Source::FlashMuseum => "Flash",
            Source::Flashpoint => "Shockwave",
        }
    }

    /// What its list without search words is: archive.org's sorted by downloads, Flash Museum's
    /// games page as the site orders it, Flashpoint's sorted by title.
    pub fn listing(self) -> &'static str {
        match self {
            Source::InternetArchive => "Most played",
            Source::FlashMuseum => "Games",
            Source::Flashpoint => "A to Z",
        }
    }

    pub fn from_site(s: &str) -> Option<Source> {
        [Source::InternetArchive, Source::FlashMuseum, Source::Flashpoint].into_iter().find(|x| x.site() == s.trim())
    }

    /// A page of results: `words` empty lists everything, as [`Source::listing`] says.
    pub fn search(self, client: &Client, words: &str, page: u32) -> Result<Page, Error> {
        match self {
            Source::InternetArchive => crate::ia::search(client, words, page),
            Source::FlashMuseum => crate::fm::search(client, words, page),
            Source::Flashpoint => crate::fp::search(client, words, page),
        }
    }

    /// The files that make up `item`'s game.
    pub fn files(self, client: &Client, item: &Item) -> Result<Files, Error> {
        match self {
            Source::InternetArchive => crate::ia::files(client, &item.id),
            Source::FlashMuseum => crate::fm::files(client, item),
            Source::Flashpoint => crate::fp::files(item),
        }
    }
}

/// One game or movie a source lists.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Item {
    /// The source's own name for it (an archive.org identifier, a Flash Museum slug).
    pub id: String,
    pub title: String,
    pub creator: Option<String>,
    pub year: Option<String>,
    pub description: Option<String>,
    /// "Game", "Animation".
    pub kind: Option<String>,
    pub thumb: Option<String>,
    /// Its page on the web.
    pub page: String,
    pub downloads: Option<u64>,
    /// 1.000.028: what the source needs later to name the item's files, kept from the listing
    /// (Flashpoint: the entry's game data and launch command, as JSON).
    pub data: Option<String>,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Page {
    pub items: Vec<Item>,
    /// How many there are in all, when the source says.
    pub total: Option<u64>,
    /// A next page exists.
    pub more: bool,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RemoteFile {
    /// Where it goes inside the game's folder ("game.swf", "data/level1.xml").
    pub name: String,
    pub url: String,
    pub size: Option<u64>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Files {
    /// The movie to play.
    pub main: RemoteFile,
    /// Files beside it that it may load.
    pub extra: Vec<RemoteFile>,
    /// Sent as Referer with the downloads (the page that embeds the movie).
    pub referer: Option<String>,
    /// Why this file is the movie, for the log.
    pub note: String,
    /// The address the movie is played as if it were loaded from, when its source plays it so
    /// (Flash Museum: the player's `launchCommand`, the movie's old home or `http://localflash/`).
    /// online.txt keeps it as `url`; Ruffle5 then gives it to the movie as its address.
    pub address: Option<String>,
    /// 1.000.028: `main` is a zip holding the game's files (Flashpoint's game data).
    pub archive: Option<Archive>,
    /// 1.000.028: the Shockwave Projector command the source starts the movie with (Flashpoint):
    /// its address and flags (`--do`, `--forceTheExitLock 0`...), which Ruffle5 hands DirPlayer.
    /// online.txt keeps it as `launch`.
    pub launch: Option<String>,
    /// 1.000.028: the folder (an address ending in `/`) that serves the files the movie loads by
    /// name, when the source does not list them (Flashpoint's entries without game data): Ruffle5
    /// fetches a Director movie's external casts from there.
    pub beside: Option<String>,
}

/// A zip of a game's files, and where the movie is in it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Archive {
    /// Its SHA-256 (lowercase hex), as the source records it.
    pub sha256: String,
    /// Other addresses of the same file, tried in turn when `main.url` fails.
    pub mirrors: Vec<String>,
    /// The movie's path inside the zip, as the source's address maps onto it
    /// ("content/www.cartoonnetwork.com/games/x/game.dcr"); the movie is looked up by it after
    /// the zip is opened (`fp::pick_movie`).
    pub movie: String,
}

/// Text from HTML: tags dropped (a <br> or </p> is a space), entities decoded, spaces collapsed.
pub fn html_text(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut in_tag = false;
    for c in s.chars() {
        match c {
            '<' => {
                in_tag = true;
                out.push(' ');
            }
            '>' if in_tag => in_tag = false,
            _ if !in_tag => out.push(c),
            _ => {}
        }
    }
    let decoded = decode_entities(&out);
    decoded.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// &amp; &lt; &gt; &quot; &#39; &#x27; &nbsp; and the other numeric ones.
pub fn decode_entities(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut rest = s;
    while let Some(i) = rest.find('&') {
        out.push_str(&rest[..i]);
        rest = &rest[i..];
        let end = rest[1..].find(';').map(|e| e + 1).filter(|&e| e <= 10);
        let Some(end) = end else {
            out.push('&');
            rest = &rest[1..];
            continue;
        };
        let name = &rest[1..end];
        let ch = match name {
            "amp" => Some('&'),
            "lt" => Some('<'),
            "gt" => Some('>'),
            "quot" => Some('"'),
            "apos" => Some('\''),
            "nbsp" => Some(' '),
            "hellip" => Some('\u{2026}'),
            "ndash" => Some('\u{2013}'),
            "mdash" => Some('\u{2014}'),
            "rsquo" => Some('\u{2019}'),
            "lsquo" => Some('\u{2018}'),
            "rdquo" => Some('\u{201d}'),
            "ldquo" => Some('\u{201c}'),
            n if n.starts_with("#x") || n.starts_with("#X") => u32::from_str_radix(&n[2..], 16).ok().and_then(char::from_u32),
            n if n.starts_with('#') => n[1..].parse::<u32>().ok().and_then(char::from_u32),
            _ => None,
        };
        match ch {
            Some(c) => {
                out.push(c);
                rest = &rest[end + 1..];
            }
            None => {
                out.push('&');
                rest = &rest[1..];
            }
        }
    }
    out.push_str(rest);
    out
}

/// At most `n` characters, cut at a word with an ellipsis.
pub fn clip(s: &str, n: usize) -> String {
    if s.chars().count() <= n {
        return s.to_string();
    }
    let cut: String = s.chars().take(n).collect();
    let cut = match cut.rfind(' ') {
        Some(i) if i > n / 2 => cut[..i].to_string(),
        _ => cut,
    };
    format!("{}\u{2026}", cut.trim_end_matches([',', '.', ';', ':', ' ']))
}

/// The words of a search, safe for any source's query language: letters and digits only.
pub fn search_words(text: &str) -> Vec<String> {
    text.split(|c: char| !c.is_alphanumeric() && c != '\'')
        .map(|w| w.trim_matches('\'').to_lowercase())
        .filter(|w| !w.is_empty())
        .take(8)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn text_from_html() {
        assert_eq!(html_text("<p>A <b>fun</b> game&nbsp;&amp; more<br/>by &#8220;Me&#x201D;</p>"), "A fun game & more by \u{201c}Me\u{201d}");
        assert_eq!(decode_entities("a & b &unknown; &#65;"), "a & b &unknown; A");
        assert_eq!(clip("one two three four", 9), "one two\u{2026}");
        assert_eq!(clip("short", 9), "short");
        assert_eq!(search_words("Duck   Life: 4! (AND) o'clock"), vec!["duck", "life", "4", "and", "o'clock"]);
    }
}
