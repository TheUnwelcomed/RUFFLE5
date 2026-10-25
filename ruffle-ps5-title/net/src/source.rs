//! What every online source hands the menu: pages of items, and for one item the files to fetch.
// SPDX-License-Identifier: GPL-3.0-or-later

use crate::http::{Client, Error};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Source {
    InternetArchive,
    FlashMuseum,
}

pub const SOURCES: [Source; 2] = [Source::InternetArchive, Source::FlashMuseum];

impl Source {
    pub fn name(self) -> &'static str {
        match self {
            Source::InternetArchive => "Internet Archive",
            Source::FlashMuseum => "Flash Museum",
        }
    }

    /// The site, as online.txt records it.
    pub fn site(self) -> &'static str {
        match self {
            Source::InternetArchive => "archive.org",
            Source::FlashMuseum => "flashmuseum.net",
        }
    }

    pub fn from_site(s: &str) -> Option<Source> {
        SOURCES.into_iter().find(|x| x.site() == s.trim())
    }

    /// A page of results: `words` empty lists the most played.
    pub fn search(self, client: &Client, words: &str, page: u32) -> Result<Page, Error> {
        match self {
            Source::InternetArchive => crate::ia::search(client, words, page),
            Source::FlashMuseum => crate::fm::search(client, words, page),
        }
    }

    /// The files that make up `item`'s game.
    pub fn files(self, client: &Client, item: &Item) -> Result<Files, Error> {
        match self {
            Source::InternetArchive => crate::ia::files(client, &item.id),
            Source::FlashMuseum => crate::fm::files(client, item),
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
