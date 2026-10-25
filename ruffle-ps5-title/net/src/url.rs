//! The URLs the sources use: http or https, a host, a port, and the request target (path and
//! query). No user info, no IPv6 literals (the console's resolver is IPv4 only), no fragments.
// SPDX-License-Identifier: GPL-3.0-or-later

use std::fmt;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Url {
    pub https: bool,
    /// Lower case.
    pub host: String,
    pub port: u16,
    /// The request target: starts with '/', may carry a query, never a fragment; percent-encoded.
    pub target: String,
}

fn strip_prefix_ci<'a>(s: &'a str, prefix: &str) -> Option<&'a str> {
    if s.len() >= prefix.len() && s.is_char_boundary(prefix.len()) && s[..prefix.len()].eq_ignore_ascii_case(prefix) {
        Some(&s[prefix.len()..])
    } else {
        None
    }
}

impl Url {
    pub fn parse(s: &str) -> Result<Url, String> {
        let s = s.trim();
        let (https, rest) = if let Some(r) = strip_prefix_ci(s, "https://") {
            (true, r)
        } else if let Some(r) = strip_prefix_ci(s, "http://") {
            (false, r)
        } else {
            return Err(format!("not an http(s) address: {s}"));
        };
        let end = rest.find(['/', '?', '#']).unwrap_or(rest.len());
        let authority = &rest[..end];
        let authority = authority.rsplit('@').next().unwrap_or(authority);
        let mut target = rest[end..].to_string();
        if let Some(i) = target.find('#') {
            target.truncate(i);
        }
        if !target.starts_with('/') {
            target.insert(0, '/');
        }
        let default_port = if https { 443 } else { 80 };
        let (host, port) = match authority.rsplit_once(':') {
            Some((h, p)) if !p.is_empty() && p.bytes().all(|b| b.is_ascii_digit()) => (h, p.parse::<u16>().map_err(|_| format!("bad port in {s}"))?),
            Some((h, "")) => (h, default_port),
            _ => (authority, default_port),
        };
        if host.is_empty() || host.contains(['[', ']', ' ']) {
            return Err(format!("no usable host in {s}"));
        }
        Ok(Url { https, host: host.to_ascii_lowercase(), port, target: escape_target(&target) })
    }

    /// The Host header: the host, and the port when it is not the scheme's own.
    pub fn host_header(&self) -> String {
        if self.port == if self.https { 443 } else { 80 } {
            self.host.clone()
        } else {
            format!("{}:{}", self.host, self.port)
        }
    }

    /// The target without its query.
    pub fn path(&self) -> &str {
        self.target.split('?').next().unwrap_or("/")
    }

    /// `location` (a Location header, a link in a page) read against this URL.
    pub fn join(&self, location: &str) -> Result<Url, String> {
        let loc = location.trim();
        if strip_prefix_ci(loc, "http://").is_some() || strip_prefix_ci(loc, "https://").is_some() {
            return Url::parse(loc);
        }
        if let Some(rest) = loc.strip_prefix("//") {
            return Url::parse(&format!("{}://{rest}", if self.https { "https" } else { "http" }));
        }
        let loc = loc.split('#').next().unwrap_or("");
        let target = if loc.is_empty() {
            self.target.clone()
        } else if loc.starts_with('/') {
            loc.to_string()
        } else if loc.starts_with('?') {
            format!("{}{loc}", self.path())
        } else {
            let path = self.path();
            let dir = &path[..path.rfind('/').map(|i| i + 1).unwrap_or(0)];
            format!("{dir}{loc}")
        };
        let (path, query) = match target.split_once('?') {
            Some((p, q)) => (p.to_string(), Some(q.to_string())),
            None => (target, None),
        };
        let mut target = remove_dot_segments(&path);
        if let Some(q) = query {
            target.push('?');
            target.push_str(&q);
        }
        Ok(Url { target: escape_target(&target), ..self.clone() })
    }
}

impl fmt::Display for Url {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}://{}{}", if self.https { "https" } else { "http" }, self.host_header(), self.target)
    }
}

/// RFC 3986 5.2.4, for the path of a relative reference.
fn remove_dot_segments(path: &str) -> String {
    let mut out: Vec<&str> = Vec::new();
    let segments: Vec<&str> = path.split('/').collect();
    for (i, seg) in segments.iter().enumerate() {
        let last = i + 1 == segments.len();
        match *seg {
            "." => {
                if last {
                    out.push("");
                }
            }
            ".." => {
                if out.len() > 1 {
                    out.pop();
                }
                if last {
                    out.push("");
                }
            }
            s => out.push(s),
        }
    }
    let joined = out.join("/");
    if joined.starts_with('/') {
        joined
    } else {
        format!("/{joined}")
    }
}

/// Percent-encode what may not stand in a request target (spaces, non-ASCII, quotes, angle
/// brackets...), keeping what is already encoded.
pub fn escape_target(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for b in s.bytes() {
        let keep = b.is_ascii_alphanumeric() || b"-._~!$&'()*+,;=:@/?%[]".contains(&b);
        if keep {
            out.push(b as char);
        } else {
            out.push_str(&format!("%{b:02X}"));
        }
    }
    out
}

/// Percent-encode a query value or a single path segment: everything but the unreserved set.
pub fn encode_component(s: &str) -> String {
    let mut out = String::with_capacity(s.len() * 3);
    for b in s.bytes() {
        if b.is_ascii_alphanumeric() || b"-._~".contains(&b) {
            out.push(b as char);
        } else {
            out.push_str(&format!("%{b:02X}"));
        }
    }
    out
}

/// Percent-encode a file's path inside an item ("dir/My Game.swf"), keeping its slashes.
pub fn encode_path(s: &str) -> String {
    s.split('/').map(encode_component).collect::<Vec<_>>().join("/")
}

/// Decode %XX escapes (invalid ones are kept as they are); '+' is left alone.
pub fn decode_percent(s: &str) -> String {
    let b = s.as_bytes();
    let mut out = Vec::with_capacity(b.len());
    let mut i = 0;
    while i < b.len() {
        if b[i] == b'%' && i + 2 < b.len() {
            if let (Some(h), Some(l)) = ((b[i + 1] as char).to_digit(16), (b[i + 2] as char).to_digit(16)) {
                out.push((h * 16 + l) as u8);
                i += 3;
                continue;
            }
        }
        out.push(b[i]);
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_what_the_sources_hand_out() {
        let u = Url::parse("https://archive.org/advancedsearch.php?q=a%20b&rows=5").unwrap();
        assert!(u.https);
        assert_eq!((u.host.as_str(), u.port, u.target.as_str()), ("archive.org", 443, "/advancedsearch.php?q=a%20b&rows=5"));
        let u = Url::parse("HTTP://Example.COM:8080").unwrap();
        assert_eq!((u.https, u.host.as_str(), u.port, u.target.as_str()), (false, "example.com", 8080, "/"));
        assert_eq!(u.to_string(), "http://example.com:8080/");
        let u = Url::parse("https://example.com?x=1#frag").unwrap();
        assert_eq!(u.target, "/?x=1");
        assert!(Url::parse("ftp://x/y").is_err());
        assert!(Url::parse("https:///path").is_err());
        // a raw space or non-ASCII byte in a link is encoded; an escape already there is kept
        let u = Url::parse("https://h/My Game.swf?a=%41").unwrap();
        assert_eq!(u.target, "/My%20Game.swf?a=%41");
        let u = Url::parse("https://h/ä").unwrap();
        assert_eq!(u.target, "/%C3%A4");
    }

    #[test]
    fn joins_locations_and_links() {
        let base = Url::parse("https://archive.org/download/item/file.swf").unwrap();
        assert_eq!(base.join("https://ia801.us.archive.org/1/items/item/file.swf").unwrap().to_string(), "https://ia801.us.archive.org/1/items/item/file.swf");
        assert_eq!(base.join("//cdn.example.org/x").unwrap().to_string(), "https://cdn.example.org/x");
        assert_eq!(base.join("/other").unwrap().to_string(), "https://archive.org/other");
        assert_eq!(base.join("sibling.swf").unwrap().to_string(), "https://archive.org/download/item/sibling.swf");
        assert_eq!(base.join("../up.swf").unwrap().to_string(), "https://archive.org/download/up.swf");
        assert_eq!(base.join("./a/../b.swf?x=1#y").unwrap().to_string(), "https://archive.org/download/item/b.swf?x=1");
        assert_eq!(base.join("?page=2").unwrap().to_string(), "https://archive.org/download/item/file.swf?page=2");
        let root = Url::parse("https://h").unwrap();
        assert_eq!(root.join("a/b").unwrap().to_string(), "https://h/a/b");
        assert_eq!(root.join("../../a").unwrap().to_string(), "https://h/a");
    }

    #[test]
    fn encodes_and_decodes() {
        assert_eq!(encode_component("collection:x AND (a b)"), "collection%3Ax%20AND%20%28a%20b%29");
        assert_eq!(encode_path("dir/My Game.swf"), "dir/My%20Game.swf");
        assert_eq!(decode_percent("a%20b%zz%4"), "a b%zz%4");
        assert_eq!(decode_percent("%e3%81%82"), "\u{3042}");
    }
}
