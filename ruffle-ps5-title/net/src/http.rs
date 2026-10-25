//! HTTP/1.1 GET over the console's own sockets and resolver (std::net; getaddrinfo is the title's
//! shim over sceNetResolver). One request per connection (`Connection: close`), so no pool and no
//! keep-alive state; redirects followed (8 at most); a body framed by Content-Length, chunked
//! transfer coding or the connection's end; `Accept-Encoding: identity`, so nothing compressed
//! comes back. https goes through rustls (src/tls.rs).
// SPDX-License-Identifier: GPL-3.0-or-later

use std::collections::HashSet;
use std::fmt;
use std::io::{self, BufRead, BufReader, Read, Write};
use std::net::{SocketAddr, TcpStream, ToSocketAddrs};
use std::sync::{Arc, Mutex, OnceLock};
use std::time::{Duration, Instant};

use crate::url::Url;

/// A connection, plain or TLS.
pub trait Conn: Read + Write + Send {}
impl<T: Read + Write + Send> Conn for T {}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Error {
    /// The caller's progress callback said stop.
    Cancelled,
    /// The server answered, after any redirects, with something other than 200.
    Status { status: u16, url: String },
    /// Anything else, in words for the log and the screen.
    Other(String),
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Error::Cancelled => write!(f, "cancelled"),
            Error::Status { status, url } => write!(f, "{url} answered {status} {}", reason(*status)),
            Error::Other(s) => write!(f, "{s}"),
        }
    }
}

impl std::error::Error for Error {}

impl From<String> for Error {
    fn from(s: String) -> Error {
        Error::Other(s)
    }
}

fn reason(status: u16) -> &'static str {
    match status {
        301 => "Moved Permanently",
        302 => "Found",
        303 => "See Other",
        307 => "Temporary Redirect",
        308 => "Permanent Redirect",
        400 => "Bad Request",
        401 => "Unauthorized",
        403 => "Forbidden",
        404 => "Not Found",
        410 => "Gone",
        429 => "Too Many Requests",
        500 => "Internal Server Error",
        502 => "Bad Gateway",
        503 => "Service Unavailable",
        504 => "Gateway Timeout",
        _ => "",
    }
}

/// A response's status line and headers.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Head {
    pub status: u16,
    pub headers: Vec<(String, String)>,
}

impl Head {
    pub fn header(&self, name: &str) -> Option<&str> {
        self.headers.iter().find(|(n, _)| n.eq_ignore_ascii_case(name)).map(|(_, v)| v.as_str())
    }

    pub fn content_length(&self) -> Option<u64> {
        self.header("content-length").and_then(|v| v.trim().parse().ok())
    }

    /// Transfer-Encoding ends in chunked.
    pub fn chunked(&self) -> bool {
        self.header("transfer-encoding").is_some_and(|v| v.rsplit(',').next().is_some_and(|c| c.trim().eq_ignore_ascii_case("chunked")))
    }

    pub fn content_type(&self) -> &str {
        self.header("content-type").unwrap_or("")
    }

    fn is_redirect(&self) -> bool {
        matches!(self.status, 301 | 302 | 303 | 307 | 308) && self.header("location").is_some()
    }
}

/// A whole response.
#[derive(Clone, Debug)]
pub struct Response {
    pub status: u16,
    /// Where it came from, after redirects.
    pub url: String,
    pub head: Head,
    pub body: Vec<u8>,
}

impl Response {
    pub fn text(&self) -> String {
        String::from_utf8_lossy(&self.body).into_owned()
    }
}

/// What a download wrote.
#[derive(Clone, Debug)]
pub struct Download {
    pub url: String,
    pub head: Head,
    pub bytes: u64,
}

/// One GET.
#[derive(Clone, Debug)]
pub struct Request<'a> {
    pub url: &'a str,
    pub referer: Option<&'a str>,
    pub accept: &'a str,
    /// The most body bytes taken; more is an error.
    pub limit: u64,
    /// A line in the log when it is done.
    pub log: bool,
}

impl<'a> Request<'a> {
    pub fn new(url: &'a str) -> Request<'a> {
        Request { url, referer: None, accept: "*/*", limit: 16 << 20, log: true }
    }
}

pub struct Client {
    user_agent: String,
    pub connect_timeout: Duration,
    /// Each read and write on the socket.
    pub io_timeout: Duration,
    pub max_redirects: u32,
    /// An HTTP CONNECT proxy (host tests behind one); the console has none.
    proxy: Option<(String, u16)>,
    extra_roots: Vec<rustls::pki_types::CertificateDer<'static>>,
    tls: OnceLock<Result<Arc<rustls::ClientConfig>, String>>,
    /// Hosts whose TLS session was described in the log already (a few, not every server).
    told: Mutex<HashSet<String>>,
}

impl Client {
    pub fn new(user_agent: &str) -> Client {
        Client {
            user_agent: user_agent.to_string(),
            connect_timeout: Duration::from_secs(10),
            io_timeout: Duration::from_secs(20),
            max_redirects: 8,
            proxy: None,
            extra_roots: Vec::new(),
            tls: OnceLock::new(),
            told: Mutex::new(HashSet::new()),
        }
    }

    /// Go through an HTTP CONNECT proxy ("host:port" or "http://host:port").
    pub fn set_proxy(&mut self, proxy: &str) -> Result<(), String> {
        let p = proxy.trim().trim_start_matches("http://").trim_end_matches('/');
        let (h, port) = p.rsplit_once(':').ok_or_else(|| format!("proxy without a port: {proxy}"))?;
        self.proxy = Some((h.to_string(), port.parse().map_err(|_| format!("bad proxy port: {proxy}"))?));
        Ok(())
    }

    /// Trust these certificates too (PEM), besides Mozilla's roots.
    pub fn add_roots_pem(&mut self, pem: &[u8]) -> Result<usize, String> {
        use rustls::pki_types::pem::PemObject;
        let before = self.extra_roots.len();
        for c in rustls::pki_types::CertificateDer::pem_slice_iter(pem) {
            self.extra_roots.push(c.map_err(|e| format!("a certificate in the PEM: {e:?}"))?);
        }
        Ok(self.extra_roots.len() - before)
    }

    fn tls_config(&self) -> Result<Arc<rustls::ClientConfig>, Error> {
        self.tls.get_or_init(|| crate::tls::config(&self.extra_roots)).clone().map_err(Error::Other)
    }

    /// GET, following redirects; the body whatever the final status (up to `req.limit` bytes).
    pub fn get(&self, req: &Request) -> Result<Response, Error> {
        let t0 = Instant::now();
        let mut body = Vec::new();
        let limit = req.limit;
        let (url, head) = self.fetch(req, &mut |_, data| {
            if body.len() as u64 + data.len() as u64 > limit {
                return Err(Error::Other(format!("{}: more than {} bytes", req.url, limit)));
            }
            body.extend_from_slice(data);
            Ok(())
        })?;
        if req.log {
            log::info!(target: "net", "GET {} -> {} {} in {:.2} s", url, head.status, size(body.len() as u64), t0.elapsed().as_secs_f64());
        }
        Ok(Response { status: head.status, url, head, body })
    }

    /// GET that must end in 200.
    pub fn get_ok(&self, req: &Request) -> Result<Response, Error> {
        let r = self.get(req)?;
        if r.status != 200 {
            return Err(Error::Status { status: r.status, url: r.url });
        }
        Ok(r)
    }

    /// GET a JSON document (200, at most 8 MB).
    pub fn get_json(&self, url: &str) -> Result<serde_json::Value, Error> {
        let mut req = Request::new(url);
        req.accept = "application/json";
        req.limit = 8 << 20;
        let r = self.get_ok(&req)?;
        serde_json::from_slice(&r.body).map_err(|e| {
            let start: String = r.text().chars().take(120).collect();
            Error::Other(format!("{}: not JSON ({e}); it starts {start:?}", r.url))
        })
    }

    /// GET into `out` (200 only). `progress(bytes so far, the whole if known)` is called as data
    /// comes; false stops it (Error::Cancelled).
    pub fn download(&self, req: &Request, out: &mut dyn Write, progress: &mut dyn FnMut(u64, Option<u64>) -> bool) -> Result<Download, Error> {
        let t0 = Instant::now();
        let mut got = 0u64;
        let limit = req.limit;
        let (url, head) = self.fetch(req, &mut |head, data| {
            if head.status != 200 {
                // an error page: not written, the status says it
                return Ok(());
            }
            got += data.len() as u64;
            if got > limit {
                return Err(Error::Other(format!("{}: more than {} bytes", req.url, limit)));
            }
            out.write_all(data).map_err(|e| Error::Other(format!("writing {}: {e}", req.url)))?;
            if !progress(got, head.content_length()) {
                return Err(Error::Cancelled);
            }
            Ok(())
        })?;
        if head.status != 200 {
            return Err(Error::Status { status: head.status, url });
        }
        if let Some(n) = head.content_length() {
            if n != got {
                return Err(Error::Other(format!("{url}: {got} of {n} bytes came")));
            }
        }
        progress(got, Some(got));
        if req.log {
            let secs = t0.elapsed().as_secs_f64();
            log::info!(target: "net", "GET {} -> 200 {} in {:.2} s ({}/s)", url, size(got), secs, size((got as f64 / secs.max(0.001)) as u64));
        }
        Ok(Download { url, head, bytes: got })
    }

    /// The request and its redirects; `body(head, bytes)` gets the final response's body.
    fn fetch(&self, req: &Request, body: &mut dyn FnMut(&Head, &[u8]) -> Result<(), Error>) -> Result<(String, Head), Error> {
        let mut url = Url::parse(req.url).map_err(Error::Other)?;
        let mut hops = 0;
        loop {
            let head = self.exchange(&url, req, body)?;
            if head.is_redirect() {
                hops += 1;
                if hops > self.max_redirects {
                    return Err(Error::Other(format!("{}: more than {} redirects", req.url, self.max_redirects)));
                }
                let location = head.header("location").unwrap_or_default().to_string();
                url = url.join(&location).map_err(|e| Error::Other(format!("{url}: redirect to {location:?}: {e}")))?;
                continue;
            }
            return Ok((url.to_string(), head));
        }
    }

    /// One request on one connection. A redirect's body is not read.
    fn exchange(&self, url: &Url, req: &Request, body: &mut dyn FnMut(&Head, &[u8]) -> Result<(), Error>) -> Result<Head, Error> {
        let mut conn = self.connect(url)?;
        let mut text = format!(
            "GET {} HTTP/1.1\r\nHost: {}\r\nUser-Agent: {}\r\nAccept: {}\r\nAccept-Encoding: identity\r\nConnection: close\r\n",
            url.target,
            url.host_header(),
            self.user_agent,
            req.accept
        );
        if let Some(r) = req.referer {
            text.push_str(&format!("Referer: {r}\r\n"));
        }
        text.push_str("\r\n");
        conn.write_all(text.as_bytes()).and_then(|_| conn.flush()).map_err(|e| Error::Other(format!("{url}: sending the request: {}", describe(&e))))?;
        let mut r = BufReader::with_capacity(64 * 1024, conn);
        let head = read_head(&mut r).map_err(|e| Error::Other(format!("{url}: {e}")))?;
        if head.is_redirect() || head.status == 204 || head.status == 304 {
            return Ok(head);
        }
        read_body(&mut r, &head, &mut |d| body(&head, d)).map_err(|e| match e {
            Error::Other(s) => Error::Other(format!("{url}: {s}")),
            e => e,
        })?;
        Ok(head)
    }

    fn connect(&self, url: &Url) -> Result<Box<dyn Conn>, Error> {
        let (host, port) = match &self.proxy {
            Some((h, p)) => (h.as_str(), *p),
            None => (url.host.as_str(), url.port),
        };
        let addrs: Vec<SocketAddr> = (host, port).to_socket_addrs().map_err(|e| Error::Other(format!("{host}: the name did not resolve ({e})")))?.collect();
        if addrs.is_empty() {
            return Err(Error::Other(format!("{host}: the name has no address")));
        }
        let mut tcp = None;
        let mut failures = Vec::new();
        for a in addrs.iter().take(4) {
            match connect_tcp(a, self.connect_timeout) {
                Ok(s) => {
                    tcp = Some(s);
                    break;
                }
                Err(e) => failures.push(format!("{a}: {e}")),
            }
        }
        let mut tcp = tcp.ok_or_else(|| Error::Other(format!("{host}: no connection ({})", failures.join("; "))))?;
        let _ = tcp.set_read_timeout(Some(self.io_timeout));
        let _ = tcp.set_write_timeout(Some(self.io_timeout));
        let _ = tcp.set_nodelay(true);
        if self.proxy.is_some() {
            proxy_connect(&mut tcp, url)?;
        }
        if !url.https {
            return Ok(Box::new(tcp));
        }
        let cfg = self.tls_config()?;
        let stream = crate::tls::connect(cfg, &url.host, tcp).map_err(Error::Other)?;
        let mut told = self.told.lock().unwrap_or_else(|e| e.into_inner());
        if told.len() < 6 && told.insert(url.host.clone()) {
            log::info!(target: "net", "TLS with {}: {}", url.host, crate::tls::describe_session(&stream.conn));
        }
        Ok(Box::new(stream))
    }
}

/// Use the plain socket() path for every connection (std's was refused once).
static PLAIN_SOCKETS: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

/// A TCP connection within `timeout`. std's socket() asks for SOCK_CLOEXEC (and on FreeBSD sets
/// SO_NOSIGPIPE); the console's kernel is FreeBSD 11 underneath, and should it refuse either, the
/// socket is made without them instead (writes still pass MSG_NOSIGNAL: std's send does).
fn connect_tcp(addr: &SocketAddr, timeout: Duration) -> io::Result<TcpStream> {
    #[cfg(unix)]
    {
        use std::sync::atomic::Ordering;
        if PLAIN_SOCKETS.load(Ordering::Relaxed) {
            return plain::connect(addr, timeout);
        }
        match TcpStream::connect_timeout(addr, timeout) {
            Err(e) if plain::refused_socket(&e) => match plain::connect(addr, timeout) {
                Ok(s) => {
                    PLAIN_SOCKETS.store(true, Ordering::Relaxed);
                    log::info!(target: "net", "std's socket() failed ({e}); plain socket() works: used from now on");
                    Ok(s)
                }
                Err(e2) => Err(io::Error::new(e2.kind(), format!("{e2} (std's socket() said: {e})"))),
            },
            r => r,
        }
    }
    #[cfg(not(unix))]
    {
        TcpStream::connect_timeout(addr, timeout)
    }
}

#[cfg(unix)]
mod plain {
    use std::io;
    use std::net::{SocketAddr, TcpStream};
    use std::os::fd::FromRawFd;
    use std::time::Duration;

    /// The errors a socket() the kernel does not like gives (not those of a failed connection).
    pub fn refused_socket(e: &io::Error) -> bool {
        matches!(e.raw_os_error(), Some(libc::EINVAL | libc::EPROTOTYPE | libc::ENOPROTOOPT | libc::EPROTONOSUPPORT | libc::EOPNOTSUPP))
    }

    /// socket(AF_INET, SOCK_STREAM, 0), connected without blocking, then blocking again.
    pub fn connect(addr: &SocketAddr, timeout: Duration) -> io::Result<TcpStream> {
        let SocketAddr::V4(v4) = addr else {
            return Err(io::Error::new(io::ErrorKind::Unsupported, "an IPv6 address"));
        };
        unsafe {
            let fd = libc::socket(libc::AF_INET, libc::SOCK_STREAM, 0);
            if fd < 0 {
                return Err(io::Error::last_os_error());
            }
            // owns the descriptor from here: an early return closes it
            let stream = TcpStream::from_raw_fd(fd);
            let mut sa: libc::sockaddr_in = std::mem::zeroed();
            #[cfg(any(target_os = "freebsd", target_os = "dragonfly", target_os = "netbsd", target_os = "openbsd", target_os = "macos", target_os = "ios"))]
            {
                sa.sin_len = std::mem::size_of::<libc::sockaddr_in>() as u8;
            }
            sa.sin_family = libc::AF_INET as libc::sa_family_t;
            sa.sin_port = v4.port().to_be();
            sa.sin_addr = libc::in_addr { s_addr: u32::from_ne_bytes(v4.ip().octets()) };
            stream.set_nonblocking(true)?;
            let r = libc::connect(fd, &sa as *const libc::sockaddr_in as *const libc::sockaddr, std::mem::size_of::<libc::sockaddr_in>() as libc::socklen_t);
            if r < 0 {
                let e = io::Error::last_os_error();
                if e.raw_os_error() != Some(libc::EINPROGRESS) {
                    return Err(e);
                }
                let mut pfd = libc::pollfd { fd, events: libc::POLLOUT, revents: 0 };
                let ms = timeout.as_millis().clamp(1, i32::MAX as u128) as libc::c_int;
                let n = libc::poll(&mut pfd, 1, ms);
                if n < 0 {
                    return Err(io::Error::last_os_error());
                }
                if n == 0 {
                    return Err(io::Error::new(io::ErrorKind::TimedOut, "connection timed out"));
                }
                let mut err: libc::c_int = 0;
                let mut len = std::mem::size_of::<libc::c_int>() as libc::socklen_t;
                if libc::getsockopt(fd, libc::SOL_SOCKET, libc::SO_ERROR, &mut err as *mut libc::c_int as *mut libc::c_void, &mut len) < 0 {
                    return Err(io::Error::last_os_error());
                }
                if err != 0 {
                    return Err(io::Error::from_raw_os_error(err));
                }
            }
            stream.set_nonblocking(false)?;
            Ok(stream)
        }
    }
}

/// An io error, with the TLS reason when it is one.
pub(crate) fn describe(e: &io::Error) -> String {
    if let Some(inner) = e.get_ref().and_then(|i| i.downcast_ref::<rustls::Error>()) {
        return format!("TLS: {inner}");
    }
    e.to_string()
}

fn proxy_connect(tcp: &mut TcpStream, url: &Url) -> Result<(), Error> {
    let req = format!("CONNECT {h}:{p} HTTP/1.1\r\nHost: {h}:{p}\r\n\r\n", h = url.host, p = url.port);
    tcp.write_all(req.as_bytes()).map_err(|e| Error::Other(format!("proxy: {e}")))?;
    // byte by byte: nothing past the head may be taken from the socket
    let mut head = Vec::new();
    let mut b = [0u8; 1];
    while !head.ends_with(b"\r\n\r\n") {
        if head.len() > 16 * 1024 || tcp.read(&mut b).map_err(|e| Error::Other(format!("proxy: {e}")))? == 0 {
            return Err(Error::Other("proxy: no answer to CONNECT".into()));
        }
        head.push(b[0]);
    }
    let line = String::from_utf8_lossy(&head);
    let status = line.split_whitespace().nth(1).unwrap_or("");
    if status != "200" {
        return Err(Error::Other(format!("proxy refused {}:{}: {}", url.host, url.port, line.lines().next().unwrap_or(""))));
    }
    Ok(())
}

/// The status line and headers (1xx interim answers skipped).
pub(crate) fn read_head(r: &mut dyn BufRead) -> Result<Head, String> {
    loop {
        let mut head = Head::default();
        let mut total = 0usize;
        let mut first = true;
        loop {
            let mut line = Vec::new();
            let n = r.read_until(b'\n', &mut line).map_err(|e| format!("reading the answer: {}", describe(&e)))?;
            if n == 0 {
                return Err(if first { "the connection closed without an answer".into() } else { "the connection closed inside the headers".into() });
            }
            total += n;
            if total > 64 * 1024 {
                return Err("the headers are larger than 64 KB".into());
            }
            while line.last().is_some_and(|b| *b == b'\n' || *b == b'\r') {
                line.pop();
            }
            let line = String::from_utf8_lossy(&line).into_owned();
            if first {
                let mut parts = line.splitn(3, ' ');
                let version = parts.next().unwrap_or("");
                if !version.starts_with("HTTP/") {
                    let shown: String = line.chars().take(60).collect();
                    return Err(format!("not an HTTP answer: {shown:?}"));
                }
                head.status = parts.next().and_then(|s| s.parse().ok()).ok_or_else(|| format!("no status in {line:?}"))?;
                first = false;
                continue;
            }
            if line.is_empty() {
                break;
            }
            if let Some((n, v)) = line.split_once(':') {
                head.headers.push((n.trim().to_string(), v.trim().to_string()));
            }
        }
        if (100..200).contains(&head.status) && head.status != 101 {
            continue;
        }
        return Ok(head);
    }
}

/// The body as `head` frames it.
pub(crate) fn read_body(r: &mut dyn BufRead, head: &Head, data: &mut dyn FnMut(&[u8]) -> Result<(), Error>) -> Result<(), Error> {
    let mut buf = vec![0u8; 64 * 1024];
    if head.chunked() {
        loop {
            let mut line = Vec::new();
            if r.read_until(b'\n', &mut line).map_err(|e| Error::Other(format!("a chunk's size: {}", describe(&e))))? == 0 {
                return Err(Error::Other("the connection closed between chunks".into()));
            }
            let text = String::from_utf8_lossy(&line);
            let hex = text.split(';').next().unwrap_or("").trim();
            let mut left = u64::from_str_radix(hex, 16).map_err(|_| Error::Other(format!("a bad chunk size {hex:?}")))?;
            if left == 0 {
                // trailers, up to the empty line (or the end)
                loop {
                    let mut t = Vec::new();
                    let n = r.read_until(b'\n', &mut t).unwrap_or(0);
                    if n == 0 || t == b"\r\n" || t == b"\n" {
                        return Ok(());
                    }
                }
            }
            while left > 0 {
                let want = left.min(buf.len() as u64) as usize;
                let n = r.read(&mut buf[..want]).map_err(|e| Error::Other(format!("reading a chunk: {}", describe(&e))))?;
                if n == 0 {
                    return Err(Error::Other("the connection closed inside a chunk".into()));
                }
                data(&buf[..n])?;
                left -= n as u64;
            }
            let mut crlf = Vec::new();
            r.read_until(b'\n', &mut crlf).map_err(|e| Error::Other(format!("after a chunk: {}", describe(&e))))?;
        }
    }
    if let Some(len) = head.content_length() {
        let mut left = len;
        while left > 0 {
            let want = left.min(buf.len() as u64) as usize;
            let n = match r.read(&mut buf[..want]) {
                Ok(n) => n,
                Err(e) if e.kind() == io::ErrorKind::Interrupted => continue,
                Err(e) => return Err(Error::Other(format!("after {} of {len} bytes: {}", len - left, describe(&e)))),
            };
            if n == 0 {
                return Err(Error::Other(format!("the connection closed after {} of {len} bytes", len - left)));
            }
            data(&buf[..n])?;
            left -= n as u64;
        }
        return Ok(());
    }
    // to the connection's end (a TLS peer that closes without close_notify ends it too)
    loop {
        match r.read(&mut buf) {
            Ok(0) => return Ok(()),
            Ok(n) => data(&buf[..n])?,
            Err(e) if e.kind() == io::ErrorKind::Interrupted => continue,
            Err(e) if e.kind() == io::ErrorKind::UnexpectedEof => return Ok(()),
            Err(e) => return Err(Error::Other(format!("reading the body: {}", describe(&e)))),
        }
    }
}

/// "12.3 KB".
pub fn size(bytes: u64) -> String {
    if bytes < 1024 {
        format!("{bytes} B")
    } else if bytes < 1024 * 1024 {
        format!("{:.1} KB", bytes as f64 / 1024.0)
    } else {
        format!("{:.1} MB", bytes as f64 / (1024.0 * 1024.0))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse(raw: &[u8]) -> Result<(Head, Vec<u8>), Error> {
        let mut r = BufReader::with_capacity(7, raw); // a small buffer: reads cross its edge
        let head = read_head(&mut r).map_err(Error::Other)?;
        let mut body = Vec::new();
        read_body(&mut r, &head, &mut |d| {
            body.extend_from_slice(d);
            Ok(())
        })?;
        Ok((head, body))
    }

    #[test]
    fn reads_length_chunked_and_close_framed_bodies() {
        let (h, b) = parse(b"HTTP/1.1 200 OK\r\nContent-Length: 5\r\nContent-Type: text/plain\r\n\r\nhelloEXTRA").unwrap();
        assert_eq!((h.status, b.as_slice(), h.content_type()), (200, &b"hello"[..], "text/plain"));
        let (h, b) = parse(b"HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\n\r\n4;ext=1\r\nWiki\r\n5\r\npedia\r\nE\r\n in\r\n\r\nchunks.\r\n0\r\nX-Trailer: y\r\n\r\n").unwrap();
        assert!(h.chunked());
        assert_eq!(String::from_utf8(b).unwrap(), "Wikipedia in\r\n\r\nchunks.");
        let (_, b) = parse(b"HTTP/1.0 200 OK\nServer: x\n\nto the end").unwrap();
        assert_eq!(b, b"to the end");
        // interim answers are skipped
        let (h, b) = parse(b"HTTP/1.1 103 Early Hints\r\nLink: </a>\r\n\r\nHTTP/1.1 200 OK\r\nContent-Length: 2\r\n\r\nok").unwrap();
        assert_eq!((h.status, b.as_slice()), (200, &b"ok"[..]));
    }

    #[test]
    fn says_what_went_wrong() {
        let e = parse(b"HTTP/1.1 200 OK\r\nContent-Length: 10\r\n\r\nshort").unwrap_err();
        assert_eq!(e, Error::Other("the connection closed after 5 of 10 bytes".into()));
        let e = parse(b"").unwrap_err();
        assert_eq!(e, Error::Other("the connection closed without an answer".into()));
        let e = parse(b"<html>hi</html>").unwrap_err();
        assert!(e.to_string().starts_with("not an HTTP answer"), "{e}");
        let e = parse(b"HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\n\r\nzz\r\n").unwrap_err();
        assert!(e.to_string().contains("bad chunk size"), "{e}");
        assert_eq!(Error::Status { status: 404, url: "https://h/x".into() }.to_string(), "https://h/x answered 404 Not Found");
    }

    #[test]
    fn plain_http_round_trip_with_a_redirect() {
        // a server on loopback: /a redirects to /b, /b answers chunked
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        let server = std::thread::spawn(move || {
            let mut seen = Vec::new();
            for _ in 0..2 {
                let (mut s, _) = listener.accept().unwrap();
                let mut req = Vec::new();
                let mut b = [0u8; 1];
                while !req.ends_with(b"\r\n\r\n") {
                    s.read_exact(&mut b).unwrap();
                    req.push(b[0]);
                }
                let req = String::from_utf8(req).unwrap();
                seen.push(req.clone());
                if req.starts_with("GET /a ") {
                    s.write_all(b"HTTP/1.1 302 Found\r\nLocation: /b?x=1\r\nContent-Length: 3\r\n\r\nabc").unwrap();
                } else {
                    s.write_all(b"HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\n\r\n3\r\nfoo\r\n0\r\n\r\n").unwrap();
                }
            }
            seen
        });
        let client = Client::new("Ruffle5-test/1");
        let url = format!("http://127.0.0.1:{port}/a");
        let mut req = Request::new(&url);
        req.referer = Some("https://example.org/page");
        let r = client.get_ok(&req).unwrap();
        assert_eq!((r.status, r.body.as_slice()), (200, &b"foo"[..]));
        assert_eq!(r.url, format!("http://127.0.0.1:{port}/b?x=1"));
        let seen = server.join().unwrap();
        assert!(seen[0].contains("User-Agent: Ruffle5-test/1\r\n") && seen[0].contains("Connection: close\r\n"), "{}", seen[0]);
        assert!(seen[0].contains(&format!("Host: 127.0.0.1:{port}\r\n")) && seen[0].contains("Referer: https://example.org/page\r\n"), "{}", seen[0]);
        assert!(seen[1].starts_with("GET /b?x=1 HTTP/1.1\r\n"), "{}", seen[1]);
    }

    #[cfg(unix)]
    #[test]
    fn the_plain_socket_path_connects_and_reports_refusal() {
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap();
        let server = std::thread::spawn(move || {
            let (mut s, _) = listener.accept().unwrap();
            s.write_all(b"hi").unwrap();
        });
        let mut s = plain::connect(&addr, Duration::from_secs(2)).unwrap();
        let mut got = Vec::new();
        s.read_to_end(&mut got).unwrap();
        assert_eq!(got, b"hi");
        server.join().unwrap();
        // nothing listens on the port any more
        let e = plain::connect(&addr, Duration::from_secs(2)).unwrap_err();
        assert_eq!(e.kind(), io::ErrorKind::ConnectionRefused, "{e}");
        assert!(plain::refused_socket(&io::Error::from_raw_os_error(libc::EPROTONOSUPPORT)));
        assert!(!plain::refused_socket(&io::Error::from_raw_os_error(libc::ECONNREFUSED)));
    }

    #[test]
    fn a_download_reports_progress_and_can_be_stopped() {
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        let server = std::thread::spawn(move || {
            for _ in 0..2 {
                let (mut s, _) = listener.accept().unwrap();
                let mut req = [0u8; 1024];
                let _ = s.read(&mut req);
                let body = vec![7u8; 300_000];
                let _ = s.write_all(format!("HTTP/1.1 200 OK\r\nContent-Length: {}\r\n\r\n", body.len()).as_bytes());
                let _ = s.write_all(&body);
            }
        });
        let client = Client::new("t");
        let url = format!("http://127.0.0.1:{port}/f.swf");
        let mut out = Vec::new();
        let mut calls = Vec::new();
        let d = client
            .download(&Request::new(&url), &mut out, &mut |got, total| {
                calls.push((got, total));
                true
            })
            .unwrap();
        assert_eq!((d.bytes, out.len()), (300_000, 300_000));
        assert_eq!(calls.last(), Some(&(300_000, Some(300_000))));
        let mut out = Vec::new();
        let e = client.download(&Request::new(&url), &mut out, &mut |got, _| got < 100_000).unwrap_err();
        assert_eq!(e, Error::Cancelled);
        server.join().unwrap();
    }
}
