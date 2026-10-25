//! Flash Museum (flashmuseum.net, Nostalgia Labs LLC): its games and animations as its own pages
//! list them. A WordPress site, read as a browser reads it:
//!   * the list: `/games/`, `/games/page/N/`; a search: `/?s=words` (`/page/N/?s=words`) — cards
//!     whose links are `/game/<slug>/` or `/animation/<slug>/`, with the title and a picture;
//!   * an item's page links its player, `https://player.flashmuseum.net/game/<slug>/`;
//!   * the player page names the movie it plays (a .swf address), fetched with the player page as
//!     Referer, as the browser would.
//! Facts so far (2026-10-24, from the pages as fetched): the list's pagination, the item and player
//! links, pictures as .webp under /wp-content/uploads/images/. How the player page names its movie
//! has not been seen yet; any .swf address in it is taken, and the log says what was found.
// SPDX-License-Identifier: GPL-3.0-or-later

use crate::http::{Client, Error, Request};
use crate::source::{clip, decode_entities, html_text, search_words, Files, Item, Page, RemoteFile};
use crate::url::{encode_component, Url};

pub const SITE: &str = "https://flashmuseum.net";

pub fn list_url(words: &str, page: u32) -> String {
    let w = search_words(words);
    let page = page.max(1);
    match (w.is_empty(), page) {
        (true, 1) => format!("{SITE}/games/"),
        (true, n) => format!("{SITE}/games/page/{n}/"),
        (false, 1) => format!("{SITE}/?s={}", encode_component(&w.join(" "))),
        (false, n) => format!("{SITE}/page/{n}/?s={}", encode_component(&w.join(" "))),
    }
}

/// The value of `name="..."` in a tag's text.
fn attr(tag: &str, name: &str) -> Option<String> {
    let lower = tag.to_ascii_lowercase();
    let mut from = 0;
    while let Some(i) = lower[from..].find(name) {
        let at = from + i;
        from = at + name.len();
        // a whole attribute name: preceded by a space, followed by =
        if at == 0 || !lower.as_bytes()[at - 1].is_ascii_whitespace() {
            continue;
        }
        let rest = lower[at + name.len()..].trim_start();
        if !rest.starts_with('=') {
            continue;
        }
        let vstart = tag.len() - rest.len() + 1;
        let v = tag[vstart..].trim_start();
        let off = tag.len() - v.len();
        let (q, body) = match v.chars().next() {
            Some(c @ ('"' | '\'')) => (Some(c), &tag[off + 1..]),
            _ => (None, &tag[off..]),
        };
        let end = match q {
            Some(c) => body.find(c).unwrap_or(body.len()),
            None => body.find(|c: char| c.is_ascii_whitespace() || c == '>').unwrap_or(body.len()),
        };
        return Some(decode_entities(&body[..end]));
    }
    None
}

/// ("game" | "animation", slug) for an item link, absolute or from the site's root.
fn item_link(href: &str) -> Option<(&'static str, String)> {
    let path = href.strip_prefix(SITE).or_else(|| href.strip_prefix("//flashmuseum.net")).or_else(|| href.strip_prefix("http://flashmuseum.net")).unwrap_or(href);
    for kind in ["game", "animation"] {
        if let Some(rest) = path.strip_prefix(&format!("/{kind}/")) {
            let slug = rest.trim_end_matches('/');
            if !slug.is_empty() && !slug.contains(['/', '?', '#', '"', '<']) {
                return Some((kind, slug.to_string()));
            }
        }
    }
    None
}

/// A picture's address in an <img> tag (lazy-loading attributes first; no inline data).
fn img_src(tag: &str) -> Option<String> {
    for a in ["data-lazy-src", "data-src", "src", "data-srcset", "srcset"] {
        if let Some(v) = attr(tag, a) {
            let first = v.split([',', ' ']).find(|s| !s.is_empty()).unwrap_or("").to_string();
            if !first.is_empty() && !first.starts_with("data:") {
                return Some(first);
            }
        }
    }
    None
}

fn kind_name(kind: &str) -> String {
    if kind == "animation" {
        "Animation".into()
    } else {
        "Game".into()
    }
}

/// The cards of a list or search page.
pub fn parse_list(html: &str, page: u32) -> Page {
    let mut items: Vec<Item> = Vec::new();
    let lower = html.to_ascii_lowercase();
    let mut from = 0;
    while let Some(i) = lower[from..].find("<a ") {
        let start = from + i;
        let Some(tag_end) = lower[start..].find('>').map(|e| start + e) else {
            break;
        };
        let tag = &html[start..tag_end];
        let close = lower[tag_end..].find("</a>").map(|e| tag_end + e).unwrap_or(html.len());
        from = tag_end;
        let Some(href) = attr(tag, "href") else {
            continue;
        };
        let Some((kind, slug)) = item_link(&href) else {
            continue;
        };
        let inner = &html[tag_end + 1..close.max(tag_end + 1)];
        let inner_lower = &lower[tag_end + 1..close.max(tag_end + 1)];
        let mut title = html_text(inner);
        let mut thumb = None;
        if let Some(j) = inner_lower.find("<img") {
            let img_end = inner_lower[j..].find('>').map(|e| j + e).unwrap_or(inner.len());
            let img = &inner[j..img_end];
            thumb = img_src(img);
            if title.is_empty() {
                title = attr(img, "alt").unwrap_or_default();
            }
        }
        if title.is_empty() {
            title = attr(tag, "title").unwrap_or_default();
        }
        let title = title.trim().trim_end_matches(" logo").trim().to_string();
        let page_url = format!("{SITE}/{kind}/{slug}/");
        match items.iter_mut().find(|it| it.id == slug) {
            Some(it) => {
                if it.title.is_empty() {
                    it.title = title;
                }
                if it.thumb.is_none() {
                    it.thumb = thumb;
                }
            }
            None => items.push(Item { id: slug.clone(), title, kind: Some(kind_name(kind)), thumb, page: page_url, ..Item::default() }),
        }
    }
    for it in &mut items {
        if it.title.is_empty() {
            it.title = it.id.clone();
        }
        if let Some(t) = &it.thumb {
            it.thumb = Url::parse(SITE).and_then(|base| base.join(t)).ok().map(|u| u.to_string());
        }
    }
    // the highest page any pagination link names
    let mut last = page;
    let mut at = 0;
    while let Some(i) = lower[at..].find("/page/") {
        at += i + 6;
        let digits: String = lower[at..].chars().take_while(|c| c.is_ascii_digit()).collect();
        if let Ok(n) = digits.parse::<u32>() {
            last = last.max(n);
        }
    }
    Page { more: last > page && !items.is_empty(), total: None, items }
}

pub fn search(client: &Client, words: &str, page: u32) -> Result<Page, Error> {
    let url = list_url(words, page);
    let mut req = Request::new(&url);
    req.accept = "text/html";
    req.limit = 8 << 20;
    let r = client.get(&req)?;
    if r.status == 404 && page > 1 {
        // past the last page
        return Ok(Page::default());
    }
    if r.status != 200 {
        return Err(Error::Status { status: r.status, url: r.url });
    }
    Ok(parse_list(&r.text(), page))
}

/// What an item's page says: its player's address, its picture, who made it.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ItemPage {
    pub player: Option<String>,
    pub picture: Option<String>,
    pub title: Option<String>,
}

pub fn parse_item_page(html: &str) -> ItemPage {
    let lower = html.to_ascii_lowercase();
    let mut out = ItemPage::default();
    // the first link to the player
    let mut at = 0;
    while let Some(i) = lower[at..].find("href=") {
        let s = at + i;
        at = s + 5;
        let tag_start = lower[..s].rfind('<').unwrap_or(0);
        let tag_end = lower[s..].find('>').map(|e| s + e).unwrap_or(html.len());
        if let Some(h) = attr(&html[tag_start..tag_end], "href") {
            if h.contains("player.flashmuseum.net/") {
                out.player = Some(h);
                break;
            }
        }
    }
    // og:image, og:title
    let mut at = 0;
    while let Some(i) = lower[at..].find("<meta ") {
        let s = at + i;
        let e = lower[s..].find('>').map(|e| s + e).unwrap_or(html.len());
        at = e;
        let tag = &html[s..e];
        match attr(tag, "property").as_deref() {
            Some("og:image") if out.picture.is_none() => out.picture = attr(tag, "content"),
            Some("og:title") if out.title.is_none() => out.title = attr(tag, "content").map(|t| t.split(" - Play Online").next().unwrap_or(&t).trim().to_string()),
            _ => {}
        }
    }
    out
}

/// Every .swf address in a page (script text included: JSON's "\/" is read as "/").
pub fn swf_addresses(html: &str, base: &Url) -> Vec<String> {
    let text = html.replace("\\/", "/").replace("&amp;", "&");
    let lower = text.to_ascii_lowercase();
    let stop = |c: char| c.is_ascii_whitespace() || "\"'`()<>,;[]{}".contains(c);
    let mut found: Vec<String> = Vec::new();
    let mut at = 0;
    while let Some(i) = lower[at..].find(".swf") {
        let dot = at + i;
        at = dot + 4;
        // the address ends at the .swf, or after its query
        let mut end = dot + 4;
        if let Some(c) = text[end..].chars().next() {
            if c.is_alphanumeric() {
                continue; // ".swfobject", ".swft"...
            }
            if c == '?' {
                end += text[end..].find(stop).unwrap_or(text.len() - end);
            }
        }
        let begin = text[..dot].rfind(stop).map(|b| b + 1).unwrap_or(0);
        let mut raw = &text[begin..end];
        // an unquoted attribute (src=movie.swf): its name is not part of the address
        if let Some(eq) = raw.find('=') {
            if raw[..eq].bytes().all(|b| b.is_ascii_alphabetic() || b == b'-') {
                raw = &raw[eq + 1..];
            }
        }
        if raw.len() <= 4 || raw.contains("swfobject") {
            continue;
        }
        if let Ok(u) = base.join(raw) {
            let s = u.to_string();
            if !found.contains(&s) {
                found.push(s);
            }
        }
    }
    found
}

pub fn files(client: &Client, item: &Item) -> Result<Files, Error> {
    let mut req = Request::new(&item.page);
    req.accept = "text/html";
    let page = client.get_ok(&req)?;
    let info = parse_item_page(&page.text());
    let kind = if item.kind.as_deref() == Some("Animation") { "animation" } else { "game" };
    let (player, note) = match info.player {
        Some(p) => (p, "the player its page links"),
        None => (format!("https://player.flashmuseum.net/{kind}/{}/", item.id), "its page links no player: the player's usual address"),
    };
    let mut req = Request::new(&player);
    req.accept = "text/html";
    req.referer = Some(&item.page);
    let pr = client.get_ok(&req)?;
    let base = Url::parse(&pr.url).map_err(Error::Other)?;
    let html = pr.text();
    let swfs = swf_addresses(&html, &base);
    log::info!(target: "net", "flashmuseum {}: {note} {}: {} .swf address(es){}", item.id, pr.url, swfs.len(), if swfs.is_empty() { String::new() } else { format!(": {}", swfs.join(" ")) });
    let Some(main) = swfs.first().cloned() else {
        // what the page has about movies, for the log
        let hint: Vec<String> = html.lines().filter(|l| l.to_ascii_lowercase().contains("swf") || l.contains("ruffle")).take(3).map(|l| clip(l.trim(), 160)).collect();
        return Err(Error::Other(format!("Flash Museum {}: no .swf address in its player page {}{}", item.id, pr.url, if hint.is_empty() { String::new() } else { format!(" (lines: {})", hint.join(" | ")) })));
    };
    let name = main.split('?').next().unwrap_or(&main).rsplit('/').next().map(crate::url::decode_percent).filter(|n| !n.is_empty()).unwrap_or_else(|| format!("{}.swf", item.id));
    Ok(Files { main: RemoteFile { name, url: main, size: None }, extra: Vec::new(), referer: Some(pr.url.clone()), note: format!("the first of {} .swf address(es) in its player page", swfs.len()) })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn list_and_search_addresses() {
        assert_eq!(list_url("", 1), "https://flashmuseum.net/games/");
        assert_eq!(list_url("", 3), "https://flashmuseum.net/games/page/3/");
        assert_eq!(list_url("Duck Life", 1), "https://flashmuseum.net/?s=duck%20life");
        assert_eq!(list_url("duck", 2), "https://flashmuseum.net/page/2/?s=duck");
    }

    #[test]
    fn reads_cards() {
        let html = r#"<nav><a href="https://flashmuseum.net/games/">Flash games</a><a href="https://flashmuseum.net/genre/action/">Action</a></nav>
            <article><a href="https://flashmuseum.net/game/duck-life-wh8/" class="thumb"><img src="data:image/gif;base64,R0" data-src="https://flashmuseum.net/wp-content/uploads/images/game-logo/d/duck-life-logo-wh8.webp" alt="Duck Life logo"></a>
            <h2><a href="https://flashmuseum.net/game/duck-life-wh8/">Duck Life</a></h2></article>
            <article><a href='/animation/asdfmovie-d5y/' title="asdfmovie"><img src='/wp-content/uploads/images/animation-logo/a/asdfmovie.webp' alt=''></a></article>
            <a href="https://flashmuseum.net/games/page/2/">2</a><a href="https://flashmuseum.net/games/page/9110/">9,110</a>"#;
        let p = parse_list(html, 1);
        assert_eq!(p.items.len(), 2, "{:?}", p.items);
        let d = &p.items[0];
        assert_eq!((d.id.as_str(), d.title.as_str(), d.kind.as_deref()), ("duck-life-wh8", "Duck Life", Some("Game")));
        assert_eq!(d.thumb.as_deref(), Some("https://flashmuseum.net/wp-content/uploads/images/game-logo/d/duck-life-logo-wh8.webp"));
        assert_eq!(d.page, "https://flashmuseum.net/game/duck-life-wh8/");
        let a = &p.items[1];
        assert_eq!((a.id.as_str(), a.title.as_str(), a.kind.as_deref()), ("asdfmovie-d5y", "asdfmovie", Some("Animation")));
        assert_eq!(a.thumb.as_deref(), Some("https://flashmuseum.net/wp-content/uploads/images/animation-logo/a/asdfmovie.webp"));
        assert!(p.more);
        assert!(!parse_list(html, 9110).more);
    }

    #[test]
    fn reads_an_item_page_and_a_player_page() {
        let html = r#"<meta property="og:title" content="Insectonator - Play Online on Flash Museum &#x1F579;&#xFE0F;" /><meta property="og:image" content="https://flashmuseum.net/wp-content/uploads/images/game-logo/i/insectonator-logo-mt5.webp" />
            <a class="play" href="https://player.flashmuseum.net/game/insectonator-mt5/">Play</a>"#;
        let p = parse_item_page(html);
        assert_eq!(p.player.as_deref(), Some("https://player.flashmuseum.net/game/insectonator-mt5/"));
        assert_eq!(p.title.as_deref(), Some("Insectonator"));
        assert_eq!(p.picture.as_deref(), Some("https://flashmuseum.net/wp-content/uploads/images/game-logo/i/insectonator-logo-mt5.webp"));
        let base = Url::parse("https://player.flashmuseum.net/game/insectonator-mt5/").unwrap();
        let player = r#"<script src="https://unpkg.com/@ruffle-rs/ruffle"></script><script src="/js/swfobject.js"></script>
            <script>var cfg = {"url":"https:\/\/files.example.net\/games\/insectonator.swf?v=2","w":550};
            player.load("/files/other.SWF"); var dup = 'https://files.example.net/games/insectonator.swf?v=2';</script>
            <embed src="local.swf">"#;
        assert_eq!(
            swf_addresses(player, &base),
            vec![
                "https://files.example.net/games/insectonator.swf?v=2".to_string(),
                "https://player.flashmuseum.net/files/other.SWF".to_string(),
                "https://player.flashmuseum.net/game/insectonator-mt5/local.swf".to_string()
            ]
        );
    }

    #[test]
    fn attributes() {
        assert_eq!(attr(r#"<a class="x" href="/a?b=1&amp;c=2">"#, "href").as_deref(), Some("/a?b=1&c=2"));
        assert_eq!(attr(r#"<img data-src='x.webp' src=y.webp>"#, "src").as_deref(), Some("y.webp"));
        assert_eq!(attr(r#"<img data-src='x.webp'>"#, "src"), None);
        assert_eq!(attr(r#"<IMG SRC = "z.png">"#, "src").as_deref(), Some("z.png"));
    }
}
