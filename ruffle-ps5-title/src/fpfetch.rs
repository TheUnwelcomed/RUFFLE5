//! PS5_RUFFLE_FPFETCH_V1 (1.000.034) — the files a Flashpoint game asks for that are not on the
//! console, fetched when the game asks for them, as Flashpoint's own game server fetches them.
//!
//! FlashpointGameServer (da3b7da: main.go, legacyServer.go `ServeLegacy`): a request is answered
//! from the game's data zip; failing that from `Legacy/htdocs/<host>/<path>`; failing that, with
//! its Infinity server on, by GET `https://infinity.flashpointarchive.org/Flashpoint/Legacy/htdocs/
//! <host>/<path>` (no query; server scripts never), and what comes is saved where it was looked
//! for. Ruffle5 fetches a Flashpoint entry without game data as its movie alone from that server
//! (1.000.028), so what the movie loads afterwards was never fetched: 1.000.032's log, Golfish and
//! Golf Ace — miniclip.com's Silent Bay wrapper loads `gameloader.dcr` from beside it: "failed to
//! parse movie data", then "Script not found MiniclipServices script".
//!
//! Here, when DirPlayer (or Ruffle, for a Flash game) cannot find a file a Flashpoint game asks
//! for, the web address it stands for is asked of that server — each address once per game — and
//! what comes is kept where it was looked for, so the next time it is on the console. The game
//! waits while it comes; each fetch is a line in ruffle.log with the seconds it took.
// SPDX-License-Identifier: GPL-3.0-or-later

use std::collections::HashSet;
use std::path::{Component, Path, PathBuf};
use std::sync::Mutex;
use std::time::Instant;

use url::Url;

/// Flashpoint's server for files the game data does not hold (FlashpointGameServer's
/// `infinityServerURL`).
pub const SERVER: &str = r5net::fp::LEGACY;

/// The site online.txt names for a Flashpoint game.
pub const SITE: &str = "flashpointarchive.org";

/// What FlashpointGameServer runs instead of serving (`extScriptTypes`): never asked for here.
const SCRIPTS: [&str; 4] = ["php", "php5", "phtml", "pl"];

pub struct FpFetch {
    client: r5net::Client,
    /// The server asked (`SERVER`; a local one in the tests).
    server: String,
    /// The movie's folder, as given and as the file system resolves it (Ruffle's navigator reads
    /// relative names from the resolved one), and the web address of that folder: a file in the
    /// folder stands for the same name at that address (DirPlayer reads the movie's own address
    /// from its folder).
    movie_dirs: Vec<PathBuf>,
    address_dir: Option<Url>,
    /// Where a web address with no place of its own is kept: `<root>/<host>/<path>`.
    web_root: Option<PathBuf>,
    asked: Mutex<HashSet<String>>,
}

impl FpFetch {
    /// For the movie at `movie_path`, published at `address` (online.txt `url`).
    pub fn new(movie_path: &Path, address: Option<&str>, web_root: Option<&Path>) -> FpFetch {
        let movie_dir = movie_path.parent().map(Path::to_path_buf).unwrap_or_default();
        let mut movie_dirs = vec![movie_dir.clone()];
        if let Ok(resolved) = movie_dir.canonicalize() {
            if resolved != movie_dir {
                movie_dirs.push(resolved);
            }
        }
        let address_dir = address.and_then(|a| Url::parse(a).ok()).and_then(|u| u.join("./").ok());
        // The game waits while a file comes: a server that does not answer is given up sooner
        // than by Flash Browser's downloads (10 s to connect, 20 s for a read).
        let mut client = r5net::Client::new(&crate::online::user_agent());
        client.connect_timeout = std::time::Duration::from_secs(5);
        client.io_timeout = std::time::Duration::from_secs(10);
        FpFetch {
            client,
            server: SERVER.to_string(),
            movie_dirs,
            address_dir,
            web_root: web_root.map(Path::to_path_buf),
            asked: Mutex::new(HashSet::new()),
        }
    }

    /// The web address a request stands for: a web address as it is (its query and fragment left
    /// off); a file in the movie's folder (or below it) at the movie's address.
    pub fn web_address(&self, url: &Url) -> Option<Url> {
        match url.scheme() {
            "http" | "https" => {
                let mut u = url.clone();
                u.set_query(None);
                u.set_fragment(None);
                Some(u)
            }
            "file" => {
                let path = url.to_file_path().ok()?;
                let rel = self.movie_dirs.iter().find_map(|d| path.strip_prefix(d).ok())?;
                let mut u = self.address_dir.clone()?;
                {
                    let mut segs = u.path_segments_mut().ok()?;
                    segs.pop_if_empty();
                    for c in rel.components() {
                        match c {
                            Component::Normal(s) => {
                                segs.push(&s.to_string_lossy());
                            }
                            _ => return None,
                        }
                    }
                }
                Some(u)
            }
            _ => None,
        }
    }

    /// The server's address for a web address: `<SERVER><host><path>`, the path as the address
    /// writes it. None for a server script or an address with no host.
    pub fn server_url(&self, web: &Url) -> Option<String> {
        let host = web.host_str().filter(|h| !h.is_empty())?;
        let path = web.path();
        let ext = path.rsplit('/').next().and_then(|n| n.rsplit_once('.')).map(|(_, e)| e.to_ascii_lowercase()).unwrap_or_default();
        if SCRIPTS.contains(&ext.as_str()) {
            return None;
        }
        Some(format!("{}{host}{path}", self.server))
    }

    /// Where what comes for `web` is kept: the place it was looked for, else under the web root.
    fn keep_at(&self, web: &Url, local: Option<&Path>) -> Option<PathBuf> {
        if let Some(l) = local {
            return Some(l.to_path_buf());
        }
        let mut p = self.web_root.as_ref()?.join(web.host_str()?);
        for seg in web.path_segments()? {
            let part = r5net::url::decode_percent(seg);
            if part.is_empty() {
                continue;
            }
            if part == "." || part == ".." || part.contains(['/', '\\', '\0']) {
                return None;
            }
            p.push(&part);
        }
        Some(p)
    }

    /// True the first time `web` is asked for in this game.
    fn first_ask(&self, web: &Url) -> bool {
        self.asked.lock().map(|mut a| a.insert(web.as_str().to_string())).unwrap_or(false)
    }

    /// The bytes of `url` (looked for at `local`, not there), from Flashpoint's server, kept on
    /// the console; None when the server does not have it or it was asked for before.
    pub fn fetch(&self, url: &Url, local: Option<&Path>) -> Option<Vec<u8>> {
        let web = self.web_address(url)?;
        if !self.first_ask(&web) {
            return None;
        }
        let Some(server) = self.server_url(&web) else {
            crate::say(&format!("     net: {web}: a server script, not asked for"));
            return None;
        };
        net_up();
        let t0 = Instant::now();
        let mut req = r5net::Request::new(&server);
        req.limit = 64 << 20;
        req.log = false;
        match self.client.get(&req) {
            Ok(r) if r.status == 200 => {
                let kept = match self.keep_at(&web, local) {
                    Some(at) => match crate::fsutil::write_durably(&at, &r.body) {
                        Ok(()) => format!(", kept as {}", at.display()),
                        Err(e) => format!(", not kept ({}: {e})", at.display()),
                    },
                    None => ", not kept (no place for it)".to_string(),
                };
                crate::say(&format!("     net: {web}: fetched from Flashpoint's server, {} bytes in {:.2} s{kept}", r.body.len(), t0.elapsed().as_secs_f64()));
                Some(r.body)
            }
            Ok(r) => {
                crate::say(&format!("     net: {web}: Flashpoint's server answered {} ({:.2} s)", r.status, t0.elapsed().as_secs_f64()));
                None
            }
            Err(e) => {
                crate::say(&format!("     net: {web}: not fetched from Flashpoint's server: {e}"));
                None
            }
        }
    }
}

/// The console's network library, started once (Flash Browser starts it too).
fn net_up() {
    extern "C" {
        fn ps5_net_init() -> i32;
    }
    static DONE: std::sync::Once = std::sync::Once::new();
    DONE.call_once(|| {
        let rc = unsafe { ps5_net_init() };
        crate::say(&format!("     net: sceNetInit = 0x{:08x}", rc as u32));
    });
}

/// Tests: ask a stand-in server instead of Flashpoint's.
#[cfg(test)]
impl FpFetch {
    pub(crate) fn set_server(&mut self, server: String) {
        self.server = server;
    }
}

/// A server standing in for Flashpoint's: answers `n` requests, 200 with the body of a path in
/// `files`, 404 for anything else; the paths asked come back through the handle.
#[cfg(test)]
pub(crate) fn serve(files: &'static [(&'static str, &'static [u8])], n: usize) -> (u16, std::thread::JoinHandle<Vec<String>>) {
    use std::io::{BufRead, BufReader, Write};
    let l = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let port = l.local_addr().unwrap().port();
    let h = std::thread::spawn(move || {
        let mut asked = Vec::new();
        for _ in 0..n {
            let (mut s, _) = l.accept().unwrap();
            let mut r = BufReader::new(s.try_clone().unwrap());
            let mut line = String::new();
            r.read_line(&mut line).unwrap();
            let p = line.split_whitespace().nth(1).unwrap_or("").to_string();
            loop {
                let mut h = String::new();
                if r.read_line(&mut h).unwrap() <= 2 {
                    break;
                }
            }
            let (status, b): (&str, &[u8]) = match files.iter().find(|(f, _)| *f == p) {
                Some((_, body)) => ("200 OK", body),
                None => ("404 Not Found", b"no"),
            };
            write!(s, "HTTP/1.1 {status}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n", b.len()).unwrap();
            s.write_all(b).unwrap();
            asked.push(p);
        }
        asked
    });
    (port, h)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn requests_map_to_flashpoints_server_and_are_kept_where_looked_for() {
        let dir = std::env::temp_dir().join(format!("ruffle5-fpfetch-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let movie = dir.join("Golfish/wrapper_silentbaystudios.dcr");
        let f = FpFetch::new(&movie, Some("http://www.miniclip.com/games/golfish/en/wrapper_silentbaystudios.dcr"), Some(&dir.join("Golfish/content")));
        // a file beside the movie stands for the same name beside its address
        let beside = Url::from_file_path(dir.join("Golfish/gameloader.dcr")).unwrap();
        let web = f.web_address(&beside).unwrap();
        assert_eq!(web.as_str(), "http://www.miniclip.com/games/golfish/en/gameloader.dcr");
        assert_eq!(f.server_url(&web).unwrap(), "https://infinity.flashpointarchive.org/Flashpoint/Legacy/htdocs/www.miniclip.com/games/golfish/en/gameloader.dcr");
        // below it too, with a space
        let deeper = Url::from_file_path(dir.join("Golfish/data/level 1.txt")).unwrap();
        assert_eq!(f.web_address(&deeper).unwrap().as_str(), "http://www.miniclip.com/games/golfish/en/data/level%201.txt");
        // a web address keeps its host and path, not its query
        let other = Url::parse("http://cdn.example.com/a/b%20c.cct?x=1#y").unwrap();
        assert_eq!(f.web_address(&other).unwrap().as_str(), "http://cdn.example.com/a/b%20c.cct");
        // kept under the web root when it has no place of its own; never outside it
        assert_eq!(f.keep_at(&f.web_address(&other).unwrap(), None).unwrap(), dir.join("Golfish/content/cdn.example.com/a/b c.cct"));
        assert_eq!(f.keep_at(&web, Some(&dir.join("x"))).unwrap(), dir.join("x"));
        assert!(f.keep_at(&Url::parse("http://h.example/a/..%2F..%2Fx").unwrap(), None).is_none(), "an encoded slash");
        // (dot segments are resolved by the address itself: /a/%2E%2E/x is /x, under the root)
        assert_eq!(f.keep_at(&Url::parse("http://h.example/a/%2E%2E/x").unwrap(), None).unwrap(), dir.join("Golfish/content/h.example/x"));
        // outside the movie's folder, or a server script: nothing
        assert!(f.web_address(&Url::from_file_path("/elsewhere/x.dcr").unwrap()).is_none());
        assert!(f.server_url(&Url::parse("http://www.miniclip.com/scores/submit.php").unwrap()).is_none());
        // each address is asked for once a game
        assert!(f.first_ask(&web));
        assert!(!f.first_ask(&web));
        assert!(f.fetch(&beside, None).is_none(), "asked before: not sent again");
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// The navigator reads relative names from the movie's folder as the file system resolves it.
    #[test]
    fn a_file_in_the_resolved_folder_stands_for_the_same_address() {
        let dir = std::env::temp_dir().join(format!("ruffle5-fpfetch-link-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join("real/Game")).unwrap();
        std::os::unix::fs::symlink(dir.join("real"), dir.join("link")).unwrap();
        let f = FpFetch::new(&dir.join("link/Game/game.dcr"), Some("http://h.example/g/game.dcr"), None);
        let resolved = dir.join("real/Game").canonicalize().unwrap().join("x.cct");
        assert_eq!(f.web_address(&Url::from_file_path(resolved).unwrap()).unwrap().as_str(), "http://h.example/g/x.cct");
        let given = dir.join("link/Game/x.cct");
        assert_eq!(f.web_address(&Url::from_file_path(given).unwrap()).unwrap().as_str(), "http://h.example/g/x.cct");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_missing_file_comes_from_the_server_and_stays() {
        let dir = std::env::temp_dir().join(format!("ruffle5-fpfetch-e2e-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join("Golfish")).unwrap();
        let movie = dir.join("Golfish/wrapper_silentbaystudios.dcr");
        let (port, server) = serve(&[("/htdocs/www.miniclip.com/games/golfish/en/gameloader.dcr", b"XFIR-loader")], 2);
        let mut f = FpFetch::new(&movie, Some("http://www.miniclip.com/games/golfish/en/wrapper_silentbaystudios.dcr"), Some(&dir.join("Golfish/content")));
        f.set_server(format!("http://127.0.0.1:{port}/htdocs/"));
        // DirPlayer looked for it beside the movie: it comes, and is kept there
        let local = dir.join("Golfish/gameloader.dcr");
        let got = f.fetch(&Url::from_file_path(&local).unwrap(), Some(&local));
        assert_eq!(got.as_deref(), Some(&b"XFIR-loader"[..]));
        assert_eq!(std::fs::read(&local).unwrap(), b"XFIR-loader");
        // one the server does not have: nothing, and nothing kept
        let other = dir.join("Golfish/missing.cct");
        assert!(f.fetch(&Url::from_file_path(&other).unwrap(), Some(&other)).is_none());
        assert!(!other.exists());
        let asked = server.join().unwrap();
        assert_eq!(asked, ["/htdocs/www.miniclip.com/games/golfish/en/gameloader.dcr", "/htdocs/www.miniclip.com/games/golfish/en/missing.cct"]);
        let _ = std::fs::remove_dir_all(&dir);
    }
}
