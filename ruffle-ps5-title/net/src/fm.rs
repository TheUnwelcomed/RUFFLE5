//! Flash Museum (flashmuseum.net, Nostalgia Labs LLC): its games and animations as its own pages
//! list them. A WordPress site, read as a browser reads it:
//!   * the list: `/games/`, `/games/page/N/`; a search: `/?s=words` (`/page/N/?s=words`) — cards
//!     whose links are `/game/<slug>/` or `/animation/<slug>/`, with the title and a picture;
//!   * an item's page holds the player's settings, `var FM = {...}` (script
//!     `fm-frame-host-js-extra`): gameUUID, playerURL, rootURL, fileName, launchCommand, gameType,
//!     title, image; and embeds the player, `<iframe src="https://player.flashmuseum.net/game/<slug>/">`;
//!   * the player page has the same settings as `window.FM_FRAME_PAGE = {..., "config": {...}}`.
//!
//! How the player plays a game — its own main.js (v3.5.0, flashmuseum.net/wp-content/themes/
//! FlashMuseum/inc/flashplayer/main.js, as fetched 2026-10-25 UTC): it downloads
//! `joinRootURL(fileName)` = rootURL without trailing '/', '/', fileName without leading '/'
//! (rootURL was https://games.flashmuseum.net for every game seen). For gameType "GameZip" that is
//! a .zip whose `content/<host>/<path>` entries are the movie (at launchCommand) and its files;
//! anything else is a plain .swf, which Ruffle is told to load from launchCommand (the movie's old
//! home, or http://localflash/<file>) with `base` its folder; requests the movie makes to that
//! host go to rootURL + their path. Ruffle5 fetches plain movies; GameZip ones are refused, by
//! name, until one has been seen.
// SPDX-License-Identifier: GPL-3.0-or-later

use serde_json::Value;

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
    Page { more: last > page && !items.is_empty(), total: None, items, elsewhere: None }
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

/// The player's settings for one item, as the site gives them to its player.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct PlayerConfig {
    pub root_url: String,
    pub file_name: String,
    /// Where the movie is played as if it were (its old home, or http://localflash/<file>).
    pub launch_command: String,
    /// "Legacy" (a plain .swf) or "GameZip" (a .zip of the movie and its files).
    pub game_type: String,
    pub title: String,
    pub image: String,
    /// The item page's settings only: the player's page.
    pub player_url: Option<String>,
}

impl PlayerConfig {
    /// main.js `joinRootURL(fileName)`.
    pub fn movie_url(&self) -> String {
        format!("{}/{}", self.root_url.trim_end_matches('/'), self.file_name.trim_start_matches('/'))
    }

    pub fn is_zip(&self) -> bool {
        self.game_type == "GameZip"
    }

    /// From the settings object: frame-runtime.js wants each value a string, or false for none.
    fn from_json(v: &Value) -> Option<PlayerConfig> {
        let s = |k: &str| match v.get(k) {
            Some(Value::String(s)) => s.clone(),
            _ => String::new(),
        };
        let c = PlayerConfig {
            root_url: s("rootURL"),
            file_name: s("fileName"),
            launch_command: s("launchCommand"),
            game_type: s("gameType"),
            title: s("title"),
            image: s("image"),
            player_url: Some(s("playerURL")).filter(|p| !p.is_empty()),
        };
        (!c.root_url.is_empty() && !c.file_name.is_empty()).then_some(c)
    }
}

/// Where `name = ` is assigned in a script (`var FM = {`, `window.FM_FRAME_PAGE = {`): just after
/// the `=`. `name` must stand alone (not the end of a longer name, not followed by more of one).
fn assignment(text: &str, name: &str) -> Option<usize> {
    let ident = |c: char| c.is_ascii_alphanumeric() || c == '_' || c == '$';
    let mut from = 0;
    while let Some(i) = text[from..].find(name) {
        let at = from + i;
        from = at + name.len();
        if text[..at].chars().next_back().is_some_and(ident) {
            continue;
        }
        let rest = &text[at + name.len()..];
        let trimmed = rest.trim_start();
        if trimmed.starts_with('=') && !trimmed.starts_with("==") {
            return Some(text.len() - trimmed.len() + 1);
        }
    }
    None
}

/// The JSON object assigned to `name` in a page's script.
fn json_assigned(html: &str, name: &str) -> Option<Value> {
    let at = assignment(html, name)?;
    let start = at + html[at..].find(|c: char| !c.is_whitespace())?;
    if !html[start..].starts_with('{') {
        return None;
    }
    let (mut depth, mut in_str, mut esc) = (0u32, false, false);
    for (i, b) in html.as_bytes()[start..].iter().enumerate() {
        if in_str {
            match b {
                _ if esc => esc = false,
                b'\\' => esc = true,
                b'"' => in_str = false,
                _ => {}
            }
            continue;
        }
        match b {
            b'"' => in_str = true,
            b'{' => depth += 1,
            b'}' => {
                depth -= 1;
                if depth == 0 {
                    return serde_json::from_str(&html[start..start + i + 1]).ok();
                }
            }
            _ => {}
        }
    }
    None
}

/// The player page's settings (`window.FM_FRAME_PAGE.config`).
pub fn parse_player_page(html: &str) -> Option<PlayerConfig> {
    let page = json_assigned(html, "FM_FRAME_PAGE")?;
    PlayerConfig::from_json(page.get("config")?)
}

/// What an item's page says: the player's settings, its player's address, its picture and title.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ItemPage {
    pub config: Option<PlayerConfig>,
    pub player: Option<String>,
    pub picture: Option<String>,
    pub title: Option<String>,
}

pub fn parse_item_page(html: &str) -> ItemPage {
    let lower = html.to_ascii_lowercase();
    let mut out = ItemPage { config: json_assigned(html, "FM").as_ref().and_then(PlayerConfig::from_json), ..ItemPage::default() };
    // the player: the settings' playerURL, else the first link to it or frame of it
    out.player = out.config.as_ref().and_then(|c| c.player_url.clone());
    let mut at = 0;
    while out.player.is_none() {
        let Some(i) = lower[at..].find("player.flashmuseum.net/") else {
            break;
        };
        let s = at + i;
        at = s + 1;
        let tag_start = lower[..s].rfind('<').unwrap_or(0);
        let tag_end = lower[s..].find('>').map(|e| s + e).unwrap_or(html.len());
        let tag = &html[tag_start..tag_end];
        out.player = attr(tag, "src").or_else(|| attr(tag, "href")).filter(|h| h.contains("player.flashmuseum.net/"));
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

/// The player page of an item: the one its page names, else the address every item seen had.
pub fn player_url(item: &Item, named: Option<&str>) -> String {
    let kind = if item.kind.as_deref() == Some("Animation") { "animation" } else { "game" };
    named.map(str::to_string).unwrap_or_else(|| format!("https://player.flashmuseum.net/{kind}/{}/", item.id))
}

/// `https://host/` of an address: what a browser sends as Referer from that page to another site
/// (its default policy, strict-origin-when-cross-origin; the player page sets no other).
fn origin(url: &str) -> Option<String> {
    let u = Url::parse(url).ok()?;
    Some(format!("{}://{}/", if u.https { "https" } else { "http" }, u.host_header()))
}

/// The player's settings for an item: from its page, else from its player page.
pub fn player_config(client: &Client, item: &Item) -> Result<(PlayerConfig, &'static str), Error> {
    let mut req = Request::new(&item.page);
    req.accept = "text/html";
    let page = client.get_ok(&req)?;
    let html = page.text();
    let info = parse_item_page(&html);
    if let Some(c) = info.config {
        return Ok((c, "its page"));
    }
    let player = player_url(item, info.player.as_deref());
    let mut req = Request::new(&player);
    req.accept = "text/html";
    req.referer = Some(&item.page);
    let pr = client.get_ok(&req)?;
    let text = pr.text();
    match parse_player_page(&text) {
        Some(mut c) => {
            c.player_url.get_or_insert(pr.url.clone());
            Ok((c, "its player page"))
        }
        None => {
            let hint: Vec<String> = text.lines().chain(html.lines()).filter(|l| l.contains("rootURL") || l.contains("FM_FRAME_PAGE") || l.contains("var FM")).take(2).map(|l| clip(l.trim(), 160)).collect();
            Err(Error::Other(format!(
                "Flash Museum {}: neither its page nor its player page {} has the player's settings{}",
                item.id,
                pr.url,
                if hint.is_empty() { String::new() } else { format!(" (lines: {})", hint.join(" | ")) }
            )))
        }
    }
}

/// What the player fetches for a game, from its settings: the movie, for a plain one.
pub fn files_for(item: &Item, c: &PlayerConfig, from: &str) -> Result<Files, Error> {
    if c.is_zip() {
        return Err(Error::Other(format!("Flash Museum {}: this game is a GameZip (a .zip of the movie and the files it loads); Ruffle5 does not fetch those yet", item.id)));
    }
    let url = c.movie_url();
    let name = crate::url::decode_percent(c.file_name.rsplit('/').next().unwrap_or(&c.file_name));
    let name = if name.is_empty() { format!("{}.swf", item.id) } else { name };
    Ok(Files {
        main: RemoteFile { name, url, size: None },
        extra: Vec::new(),
        referer: c.player_url.as_deref().and_then(origin),
        note: format!("{from}: rootURL + fileName, as the site's player fetches it (gameType {:?})", c.game_type),
        address: Some(c.launch_command.clone()).filter(|a| a.starts_with("http://") || a.starts_with("https://")),
        archive: None,
        launch: None,
        beside: None,
    })
}

pub fn files(client: &Client, item: &Item) -> Result<Files, Error> {
    let (c, from) = player_config(client, item)?;
    log::info!(target: "net", "flashmuseum {}: settings from {from}: gameType {:?}, rootURL {}, fileName {}, launchCommand {}", item.id, c.game_type, c.root_url, c.file_name, c.launch_command);
    files_for(item, &c, from)
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

    fn insectonator() -> Item {
        Item { id: "insectonator-mt5".into(), title: "Insectonator".into(), kind: Some("Game".into()), page: "https://flashmuseum.net/game/insectonator-mt5/".into(), ..Item::default() }
    }

    // The parts of the pages that matter, as the online check saved them on 2026-10-24.
    const ITEM_PAGE: &str = r#"<meta property="og:title" content="Insectonator - Play Online on Flash Museum &#x1F579;&#xFE0F;" /><meta property="og:image" content="https://flashmuseum.net/wp-content/uploads/images/game-logo/i/insectonator-logo-mt5.webp" />
<div class="gb-element-660c0b40 game-player"><iframe class="fm-player-frame" title="Game player" src="https://player.flashmuseum.net/game/insectonator-mt5/" allow="fullscreen; autoplay" allowfullscreen></iframe></div>
<script id="fm-frame-host-js-extra">
var FM = {"gameUUID":"f25e7cdc-923f-4a5a-a221-7c28e94f76af","playerURL":"https://player.flashmuseum.net/game/insectonator-mt5/","rootURL":"https://games.flashmuseum.net","fileName":"f25e7cdc-923f-4a5a-a221-7c28e94f76af.swf","launchCommand":"http://localflash/f25e7cdc-923f-4a5a-a221-7c28e94f76af.swf","gameType":"Legacy","title":"Insectonator","image":"https://flashmuseum.net/wp-content/uploads/images/game-logo/i/insectonator-logo-mt5.webp"};
//# sourceURL=fm-frame-host-js-extra
</script>"#;
    const PLAYER_PAGE: &str = r#"<script>
window.FM_FRAME_PAGE = {"identifier":"game\/square-face-icon-generator-g0x","siteOrigin":"https:\/\/flashmuseum.net","config":{"rootURL":"https:\/\/games.flashmuseum.net","fileName":"dc4e1306-b10f-4150-a830-e5f8fa8dc60e.swf","launchCommand":"http:\/\/www.icongenerators.net\/flash\/dc4e1306-b10f-4150-a830-e5f8fa8dc60e.swf","gameType":"Legacy","title":"Square Face Icon Generator","image":false},"assets":{"ruffle":"https:\/\/cdn.jsdelivr.net\/npm\/@ruffle-rs\/ruffle@0.4.1\/ruffle.js"},"earlyPlay":true};
window.RufflePlayer = { config: { openUrlMode: 'deny' } };
if (window === window.top) window.location.replace(window.FM_FRAME_PAGE.redirect);
</script>"#;

    #[test]
    fn reads_the_players_settings_from_an_item_page() {
        let p = parse_item_page(ITEM_PAGE);
        assert_eq!(p.title.as_deref(), Some("Insectonator"));
        assert_eq!(p.picture.as_deref(), Some("https://flashmuseum.net/wp-content/uploads/images/game-logo/i/insectonator-logo-mt5.webp"));
        assert_eq!(p.player.as_deref(), Some("https://player.flashmuseum.net/game/insectonator-mt5/"));
        let c = p.config.expect("the settings");
        assert_eq!((c.root_url.as_str(), c.file_name.as_str(), c.game_type.as_str()), ("https://games.flashmuseum.net", "f25e7cdc-923f-4a5a-a221-7c28e94f76af.swf", "Legacy"));
        assert_eq!(c.movie_url(), "https://games.flashmuseum.net/f25e7cdc-923f-4a5a-a221-7c28e94f76af.swf");
        let f = files_for(&insectonator(), &c, "its page").unwrap();
        assert_eq!(f.main.url, "https://games.flashmuseum.net/f25e7cdc-923f-4a5a-a221-7c28e94f76af.swf");
        assert_eq!(f.main.name, "f25e7cdc-923f-4a5a-a221-7c28e94f76af.swf");
        assert_eq!(f.referer.as_deref(), Some("https://player.flashmuseum.net/"));
        assert_eq!(f.address.as_deref(), Some("http://localflash/f25e7cdc-923f-4a5a-a221-7c28e94f76af.swf"));
        // without the settings: the frame's address
        let p = parse_item_page(&ITEM_PAGE.replace("var FM = ", "var FM2 = "));
        assert_eq!((p.config, p.player.as_deref()), (None, Some("https://player.flashmuseum.net/game/insectonator-mt5/")));
    }

    #[test]
    fn reads_the_players_settings_from_a_player_page() {
        let c = parse_player_page(PLAYER_PAGE).expect("the settings");
        assert_eq!(c.movie_url(), "https://games.flashmuseum.net/dc4e1306-b10f-4150-a830-e5f8fa8dc60e.swf");
        assert_eq!(c.launch_command, "http://www.icongenerators.net/flash/dc4e1306-b10f-4150-a830-e5f8fa8dc60e.swf");
        assert_eq!((c.image.as_str(), c.player_url.as_deref()), ("", None), "false is no value");
        assert_eq!(parse_player_page("<script>window.FM_FRAME_PAGE == {}</script>"), None);
    }

    #[test]
    fn join_root_url_and_game_zips() {
        let mut c = PlayerConfig { root_url: "https://games.example.net//".into(), file_name: "//a/b c.swf".into(), game_type: "Legacy".into(), ..PlayerConfig::default() };
        assert_eq!(c.movie_url(), "https://games.example.net/a/b c.swf");
        c.game_type = "GameZip".into();
        let e = files_for(&insectonator(), &c, "its page").unwrap_err().to_string();
        assert!(e.contains("GameZip"), "{e}");
        // braces and quotes inside strings do not end the object
        assert_eq!(json_assigned(r#"var FM = {"t":"a } \" {","n":{"x":1}};"#, "FM").unwrap()["n"]["x"], 1);
        assert_eq!(json_assigned(r#"var FMX = {"a":1}; var FM = {"a":2}"#, "FM").unwrap()["a"], 2);
    }

    #[test]
    fn attributes() {
        assert_eq!(attr(r#"<a class="x" href="/a?b=1&amp;c=2">"#, "href").as_deref(), Some("/a?b=1&c=2"));
        assert_eq!(attr(r#"<img data-src='x.webp' src=y.webp>"#, "src").as_deref(), Some("y.webp"));
        assert_eq!(attr(r#"<img data-src='x.webp'>"#, "src"), None);
        assert_eq!(attr(r#"<IMG SRC = "z.png">"#, "src").as_deref(), Some("z.png"));
    }
}
