//! PS5_RUFFLE_NAVIGATOR_V1 — where a Flash game's files come from.
//!
//! Until 1.000.033 a Flash game ran with Ruffle's `NullNavigatorBackend` on the movie's folder: a
//! file the movie loads by a relative name is read from beside it, and nothing else is. A game from
//! Flashpoint (1.000.033: its Flash games in Flash Browser) is unpacked as its game data keeps it,
//! `content/<host>/<path>` — the files at the addresses the game was published at — and plays as if
//! from its address (online.txt `url`). Flashpoint's own launcher serves those files to Ruffle
//! through its proxy; here a load of `http(s)://host/path` reads `<web root>/host/path` (online.txt
//! `webroot`, `content` in the game's folder): the path's parts decoded (`%20` is a space), as the
//! game data keeps its file names and as Flashpoint's server looks them up (the name as written
//! when only that is there), the query left off. Relative names and file addresses are read as
//! before, by Ruffle's own `fetch_path`. Nothing goes to the network.
//!
//! 1.000.038: a file under a game's `db/` folder that is not there, in a folder that has the level
//! server's answers as files (Happy Wheels' full version: `db/get_featured_levels.php`), is
//! answered by the stand-in server (src/hwserver.rs) — from bytes, not a file.
// SPDX-License-Identifier: GPL-3.0-or-later

use std::path::{Path, PathBuf};
use std::time::Duration;

use async_channel::{Receiver, Sender};
use ruffle_core::backend::navigator::{
    fetch_path, resolve_url_with_relative_base_path, ErrorResponse, NavigationMethod, NavigatorBackend, NullExecutor, NullSpawner,
    OwnedFuture, Request, SuccessResponse,
};
use ruffle_core::loader::Error;
use ruffle_core::socket::{ConnectionState, SocketAction, SocketHandle};
use url::{ParseError, Url};

/// The variables a `getURL` sends, as Ruffle's trait takes them (Ruffle re-exports `indexmap`).
type Vars = Option<(NavigationMethod, ruffle_core::indexmap::IndexMap<String, String>)>;

pub struct Ps5Navigator {
    spawner: NullSpawner,
    /// The movie's folder: relative names are read from it.
    base: PathBuf,
    /// The folder web addresses are read from (`<web root>/<host>/<path>`), when the game has one.
    web_root: Option<PathBuf>,
    /// 1.000.034: a Flashpoint game's files not on the console, from Flashpoint's server.
    fetch: Option<std::sync::Arc<crate::fpfetch::FpFetch>>,
}

impl Ps5Navigator {
    /// For the movie in `base`, with `web_root` (a game data's `content`) when there is one. The
    /// error, when the movie's folder cannot be resolved: relative loads then do not work (as
    /// Ruffle's own navigator without a base path).
    pub fn new(base: &Path, web_root: Option<&Path>, executor: &NullExecutor) -> (Self, Option<std::io::Error>) {
        let (base, err) = match base.canonicalize() {
            Ok(b) => (b, None),
            Err(e) => (PathBuf::new(), Some(e)),
        };
        (Self { spawner: executor.spawner(), base, web_root: web_root.and_then(|w| w.canonicalize().ok()), fetch: None }, err)
    }

    /// Files the game asks for that are not on the console are asked of Flashpoint's server.
    pub fn set_fetch(&mut self, f: crate::fpfetch::FpFetch) {
        self.fetch = Some(std::sync::Arc::new(f));
    }

    /// The file a request is read from on the console, when it is a file (a web address under
    /// the web root, or a file address).
    fn local_file(&self, url: &Url) -> Option<PathBuf> {
        match url.scheme() {
            "http" | "https" => web_file(self.web_root.as_ref()?, url),
            "file" => url.to_file_path().ok(),
            _ => None,
        }
    }

    /// 1.000.034: a file not on the console, from Flashpoint's server into its place (the game
    /// waits for it). True when it is there now.
    fn fetched(&self, url: &Url) -> bool {
        let (Some(f), Some(local)) = (&self.fetch, self.local_file(url)) else {
            return false;
        };
        if local.is_file() {
            return true;
        }
        f.fetch(url, Some(&local)).is_some() && local.is_file()
    }

    pub fn web_root(&self) -> Option<&Path> {
        self.web_root.as_deref()
    }

    /// 1.000.038: `<game>/db/<name>` asked for and not there, in a folder with the level server's
    /// answers: what the stand-in server says (src/hwserver.rs).
    fn stand_in_answer(&self, url: &Url, request: &Request) -> Option<Vec<u8>> {
        if url.scheme() != "file" {
            return None;
        }
        let mut plain = url.clone();
        plain.set_query(None);
        let path = plain.to_file_path().ok()?;
        if path.is_file() {
            return None;
        }
        let db = path.parent()?;
        if db.file_name()?.to_str()? != "db" {
            return None;
        }
        let game = db.parent()?;
        if !crate::hwserver::serves(game) {
            return None;
        }
        let name = path.file_name()?.to_str()?;
        let body = request.body().as_ref().map(|(b, _)| b.as_slice());
        let answer = crate::hwserver::answer(game, name, url.query().unwrap_or(""), body);
        crate::say(&format!(
            "     level server: db/{name}{}{} -> {}",
            url.query().map(|q| format!("?{q}")).unwrap_or_default(),
            body.map(|b| format!(" (form {} bytes)", b.len())).unwrap_or_default(),
            answer.as_ref().map(|a| format!("answered, {} bytes", a.len())).unwrap_or_else(|| "not one of its files".to_string())
        ));
        answer
    }
}

/// An answer made here, not read from a file.
struct BytesResponse {
    url: String,
    bytes: Option<Vec<u8>>,
}

impl SuccessResponse for BytesResponse {
    fn url(&self) -> std::borrow::Cow<'_, str> {
        std::borrow::Cow::Borrowed(&self.url)
    }
    fn set_url(&mut self, url: String) {
        self.url = url;
    }
    fn body(self: Box<Self>) -> OwnedFuture<Vec<u8>, ruffle_core::loader::Error> {
        let bytes = self.bytes.unwrap_or_default();
        Box::pin(async move { Ok(bytes) })
    }
    fn text_encoding(&self) -> Option<&'static encoding_rs::Encoding> {
        None
    }
    fn status(&self) -> u16 {
        200
    }
    fn redirected(&self) -> bool {
        false
    }
    fn next_chunk(&mut self) -> OwnedFuture<Option<Vec<u8>>, ruffle_core::loader::Error> {
        let chunk = self.bytes.take();
        Box::pin(async move { Ok(chunk) })
    }
    fn expected_length(&self) -> Result<Option<u64>, ruffle_core::loader::Error> {
        Ok(self.bytes.as_ref().map(|b| b.len() as u64))
    }
}

impl NavigatorBackend for Ps5Navigator {
    fn navigate_to_url(&self, url: &str, _target: &str, _vars_method: Vars) {
        // a game asking to open a web page: there is no browser here
        crate::say(&format!("     game: asked to open {url} (no web browser here)"));
    }

    fn fetch(&self, request: Request) -> OwnedFuture<Box<dyn SuccessResponse>, ErrorResponse> {
        if let Ok(url) = Url::parse(request.url()).or_else(|_| self.resolve_url(request.url())) {
            // not on the console: from Flashpoint's server first, when the game came from there
            if self.fetch.is_some() {
                self.fetched(&url);
            }
            // 1.000.038: the level server's answers a Happy Wheels folder does not have
            if let Some(bytes) = self.stand_in_answer(&url, &request) {
                return Box::pin(async move { Ok(Box::new(BytesResponse { url: url.to_string(), bytes: Some(bytes) }) as Box<dyn SuccessResponse>) });
            }
            if let (Some(root), true) = (&self.web_root, matches!(url.scheme(), "http" | "https")) {
                if let Some(file) = web_file(root, &url).and_then(|p| Url::from_file_path(p).ok()) {
                    return fetch_path(self, "Ruffle5", file.as_str(), None);
                }
            }
        }
        fetch_path(self, "Ruffle5", request.url(), None)
    }

    fn resolve_url(&self, url: &str) -> Result<Url, ParseError> {
        resolve_url_with_relative_base_path(self, self.base.clone(), url)
    }

    fn spawn_future(&mut self, future: OwnedFuture<(), Error>) {
        self.spawner.spawn_local(future);
    }

    fn pre_process_url(&self, url: Url) -> Url {
        url
    }

    fn connect_socket(&mut self, _host: String, _port: u16, _timeout: Duration, handle: SocketHandle, _receiver: Receiver<Vec<u8>>, sender: Sender<SocketAction>) {
        let _ = sender.try_send(SocketAction::Connect(handle, ConnectionState::Failed));
    }
}

/// The file under `root` an address names: `<root>/<host>/<path>`, the path's parts decoded (or,
/// when only that file is there, as written). None for an address with no host or a `..` part.
fn web_file(root: &Path, url: &Url) -> Option<PathBuf> {
    let host = url.host_str().filter(|h| !h.is_empty())?;
    let raw: Vec<&str> = url.path().split('/').filter(|p| !p.is_empty()).collect();
    let decoded: Vec<String> = raw.iter().map(|p| r5net::url::decode_percent(p)).collect();
    let bad = |p: &str| p == ".." || p.contains('/') || p.contains('\\');
    if decoded.iter().any(|p| bad(p)) || raw.iter().any(|p| bad(p)) {
        return None;
    }
    let join = |parts: &[String]| parts.iter().fold(root.join(host), |p, part| p.join(part));
    let plain = join(&decoded);
    if plain.is_file() {
        return Some(plain);
    }
    let written = join(&raw.iter().map(|p| p.to_string()).collect::<Vec<_>>());
    Some(if written.is_file() { written } else { plain })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::future::Future;
    use std::task::{Context, Poll, Waker};

    /// The futures here are ready at once (a local file); one poll runs them.
    fn now<T>(f: impl Future<Output = T>) -> T {
        let mut f = std::pin::pin!(f);
        match f.as_mut().poll(&mut Context::from_waker(Waker::noop())) {
            Poll::Ready(v) => v,
            Poll::Pending => panic!("not ready"),
        }
    }

    #[test]
    fn web_addresses_are_read_from_the_web_root() {
        let root = std::env::temp_dir().join(format!("ruffle5-nav-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        let game = root.join("content/www.example.com/games/x");
        std::fs::create_dir_all(&game).unwrap();
        std::fs::create_dir_all(root.join("content/cdn.example.com/a")).unwrap();
        std::fs::write(game.join("level.swf"), b"FWS-level").unwrap();
        std::fs::write(root.join("content/cdn.example.com/a/b%20c.xml"), b"<x/>").unwrap();
        std::fs::write(root.join("content/cdn.example.com/a/b c.xml"), b"<y/>").unwrap();
        std::fs::write(root.join("content/cdn.example.com/a/only%2Bencoded.txt"), b"enc").unwrap();
        let executor = NullExecutor::new();
        let (nav, err) = Ps5Navigator::new(&game, Some(&root.join("content")), &executor);
        assert!(err.is_none());
        let read = |url: &str| -> Result<Vec<u8>, String> {
            let response = now(nav.fetch(Request::get(url.to_string()))).map_err(|e| e.error.to_string())?;
            now(response.body()).map_err(|e| e.to_string())
        };
        // beside the movie, by a relative name (as before)
        assert_eq!(read("level.swf").unwrap(), b"FWS-level");
        // by its web address, from the web root
        assert_eq!(read("http://www.example.com/games/x/level.swf").unwrap(), b"FWS-level");
        assert_eq!(read("https://cdn.example.com/a/b%20c.xml").unwrap(), b"<y/>", "the path is read decoded, as the zip keeps it");
        assert_eq!(read("https://cdn.example.com/a/b%20c.xml?v=3").unwrap(), b"<y/>", "the query is left off");
        assert_eq!(read("http://cdn.example.com/a/only%2Bencoded.txt").unwrap(), b"enc", "as written when only that is there");
        assert!(read("http://cdn.example.com/a/..%2F..%2Fescape").is_err());
        // not there: an error, never the network
        assert!(read("http://www.example.com/missing.swf").is_err());
        // without a web root, a web address is not read
        let (plain, _) = Ps5Navigator::new(&game, None, &executor);
        assert!(now(plain.fetch(Request::get("http://www.example.com/games/x/level.swf".to_string()))).is_err());
        let _ = std::fs::remove_dir_all(&root);
    }

    /// 1.000.038: Happy Wheels' level browser answered by the stand-in server.
    #[test]
    fn a_level_servers_missing_answers_are_made_up_from_the_featured_list() {
        let root = std::env::temp_dir().join(format!("ruffle5-nav-hw-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        let game = root.join("HappyWheels Full");
        std::fs::create_dir_all(game.join("db")).unwrap();
        std::fs::write(game.join("Happy-Wheels.swf"), b"FWS-main").unwrap();
        // the site's own form of the featured list: one level
        std::fs::write(
            game.join("db/get_featured_levels.php"),
            b"ToFlash=a%3A1%3A%7Bi%3A0%3Ba%3A3%3A%7Bs%3A4%3A%22id_x%22%3Bs%3A1%3A%227%22%3Bs%3A4%3A%22name%22%3Bs%3A10%3A%22Crime+City%22%3Bs%3A6%3A%22public%22%3Bs%3A1%3A%221%22%3B%7D%7D",
        )
        .unwrap();
        std::fs::write(game.join("db/get_login.php"), b"ToFlash=x").unwrap();
        let executor = NullExecutor::new();
        let (nav, err) = Ps5Navigator::new(&game, None, &executor);
        assert!(err.is_none());
        let read = |url: &str| -> Result<Vec<u8>, String> {
            let response = now(nav.fetch(Request::get(url.to_string()))).map_err(|e| e.error.to_string())?;
            now(response.body()).map_err(|e| e.to_string())
        };
        // a file that is there is read as before
        assert_eq!(read("db/get_login.php").unwrap(), b"ToFlash=x");
        // the browser's list, not a file: made up from the featured list
        let list = read("./db/get_levels_wr2.php?offset=0&character=0&uploaded=week&sortby=newest&public=1").unwrap();
        let text = String::from_utf8(list).unwrap();
        assert!(text.starts_with("ToFlash=a%3A1%3A") && text.contains("Crime%20City"), "{text}");
        // not one of the server's files: an error, as before
        assert!(read("db/vote_level.php").is_err());
        // a folder without the featured list is untouched
        let other = root.join("Other");
        std::fs::create_dir_all(other.join("db")).unwrap();
        let (plain, _) = Ps5Navigator::new(&other, None, &executor);
        let Ok(response) = now(plain.fetch(Request::get("db/get_levels_wr2.php".to_string()))) else { panic!("a file response") };
        assert!(now(response.body()).is_err(), "nothing made up there");
        let _ = std::fs::remove_dir_all(&root);
    }

    /// 1.000.034: a Flashpoint game's files that are not on the console.
    #[test]
    fn a_flashpoint_games_files_not_here_come_from_its_server_and_stay() {
        let root = std::env::temp_dir().join(format!("ruffle5-nav-fp-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        let game = root.join("Some Game");
        let content = game.join("content");
        std::fs::create_dir_all(&content).unwrap();
        std::fs::write(game.join("game.swf"), b"FWS-main").unwrap();
        let (port, server) = crate::fpfetch::serve(
            &[("/htdocs/www.example.com/games/x/level2.swf", b"FWS-level2"), ("/htdocs/cdn.example.com/a/b.xml", b"<b/>")],
            3,
        );
        let mut f = crate::fpfetch::FpFetch::new(&game.join("game.swf"), Some("http://www.example.com/games/x/game.swf"), Some(&content));
        f.set_server(format!("http://127.0.0.1:{port}/htdocs/"));
        let executor = NullExecutor::new();
        let (mut nav, err) = Ps5Navigator::new(&game, Some(&content), &executor);
        assert!(err.is_none());
        nav.set_fetch(f);
        let read = |url: &str| -> Result<Vec<u8>, String> {
            let response = now(nav.fetch(Request::get(url.to_string()))).map_err(|e| e.error.to_string())?;
            now(response.body()).map_err(|e| e.to_string())
        };
        // a name beside the movie: the same name beside its address, kept beside the movie
        assert_eq!(read("level2.swf").unwrap(), b"FWS-level2");
        assert_eq!(std::fs::read(game.join("level2.swf")).unwrap(), b"FWS-level2");
        // another site's address: kept under the web root, the query left off
        assert_eq!(read("http://cdn.example.com/a/b.xml?v=2").unwrap(), b"<b/>");
        assert_eq!(std::fs::read(content.join("cdn.example.com/a/b.xml")).unwrap(), b"<b/>");
        // what the server does not have: an error, asked for once
        assert!(read("missing.swf").is_err());
        assert!(read("missing.swf").is_err());
        assert!(!game.join("missing.swf").exists());
        // what is here is read from the console (the stand-in answers three requests only)
        assert_eq!(read("level2.swf").unwrap(), b"FWS-level2");
        assert_eq!(read("game.swf").unwrap(), b"FWS-main");
        let asked = server.join().unwrap();
        assert_eq!(asked, ["/htdocs/www.example.com/games/x/level2.swf", "/htdocs/cdn.example.com/a/b.xml", "/htdocs/www.example.com/games/x/missing.swf"]);
        let _ = std::fs::remove_dir_all(&root);
    }
}
