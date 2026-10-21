//! PS5_RUST_HELLO_V5 — milestone 1 of Ruffle-on-PS5: Rust's standard library on the console. The
//! checks live in this library so the same code runs two ways: as an elfldr payload (src/main.rs,
//! prints to the sender) and inside a native title (ps5-rust-title: a C shell calls
//! `ps5_rust_hello_main`, the report goes to /app0/ps5-rust-hello/report.txt, the tally to a toast).
//! v3: the fs and net checks also run the shim's probes, which name the exact C call that fails.
//! v4: the net probe also covers the fcntl calls and a non-blocking connect (v8: FIONBIO is EACCES).
//! v5: the net probe measures Sony's SO_NBIO socket option, raw and via sceNetSetsockopt, and kqueue
//!     (v9: fcntl(O_NONBLOCK) is EACCES in a title too — the sandbox, not the call).
//! No crates: every check is std only, so a failure points at the toolchain, not a dependency.
//!
//! What it proves, in order: printing (stdio over the elfldr socket), the C shim (a toast on
//! the TV), clocks, threads with a mutex and a condvar (FreeBSD's _umtx_op), the hashmap's
//! random seed (arc4random_buf), the filesystem with the FreeBSD-11 struct layouts
//! (metadata().len() and read_dir() compared with the console's own stat/readdir through the
//! shim), a 64 MB allocation, and one outbound TCP connection.
//!
//! Exit code = number of failed checks. The toast at the end says the same.
// SPDX-License-Identifier: GPL-3.0-or-later

use std::collections::HashMap;
use std::ffi::CString;
use std::io::{Read, Write};
use std::net::{TcpStream, ToSocketAddrs};
use std::os::raw::{c_char, c_int, c_long, c_longlong};
use std::sync::{Arc, Condvar, Mutex};
use std::thread;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

extern "C" {
    fn ps5_notify(msg: *const c_char) -> c_int;
    fn ps5_c_stat_size(path: *const c_char) -> c_longlong;
    fn ps5_c_dir_count(path: *const c_char) -> c_long;
    fn ps5_c_dir_probe(path: *const c_char, out: *mut c_char, size: usize) -> c_long;
    fn ps5_c_net_probe(ip: *const c_char, port: c_int, out: *mut c_char, size: usize) -> c_long;
    fn ps5_c_sizeof_stat() -> c_long;
    fn ps5_c_sizeof_dirent() -> c_long;
}

/// Runs one of the shim's probes and returns its text (each primitive call with its errno).
fn c_probe(run: impl FnOnce(*mut c_char, usize) -> c_long) -> (c_long, String) {
    let mut buf = vec![0u8; 2048];
    let r = run(buf.as_mut_ptr() as *mut c_char, buf.len());
    let end = buf.iter().position(|&b| b == 0).unwrap_or(buf.len());
    (r, String::from_utf8_lossy(&buf[..end]).into_owned())
}

use std::ffi::CStr;
use std::fmt::Write as _;

/// One outbound connection to prove sockets + getaddrinfo work. Cloudflare's resolver answers
/// on 443 from anywhere; change it to your own server if the console has no internet.
const NET_TARGET: &str = "1.1.1.1:443";

struct Report {
    passed: u32,
    failed: u32,
    text: String,
    /// The report file, written line by line and flushed each time: if the title dies in a check,
    /// the lines up to that point are already on disk.
    file: Option<std::fs::File>,
}

impl Report {
    fn line(&mut self, s: String) {
        println!("{s}");
        let _ = writeln!(self.text, "{s}");
        if let Some(f) = self.file.as_mut() {
            let _ = writeln!(f, "{s}");
            let _ = f.flush();
        }
        // the ".." markers also go to the TV: if the title dies, the last toast names the check
        if s.starts_with("..") {
            notify(s.trim_start_matches(['.', ' ']));
        }
    }
    fn check(&mut self, name: &str, ok: bool, detail: String) {
        if ok {
            self.passed += 1;
            self.line(format!("ok    {name}: {detail}"));
        } else {
            self.failed += 1;
            self.line(format!("FAIL  {name}: {detail}"));
        }
    }
}

fn notify(msg: &str) {
    if let Ok(c) = CString::new(msg) {
        unsafe {
            ps5_notify(c.as_ptr());
        }
    }
}

/// C entry for the title shell: `scratch_dir` is a writable folder (`/download0/...` in a title,
/// `/data` as a payload), `report_path` (may be NULL) gets the full text. Returns the failure count.
#[no_mangle]
pub extern "C" fn ps5_rust_hello_main(scratch_dir: *const c_char, report_path: *const c_char) -> c_int {
    let scratch = if scratch_dir.is_null() { "/data".to_string() } else { unsafe { CStr::from_ptr(scratch_dir) }.to_string_lossy().into_owned() };
    let report = if report_path.is_null() { None } else { Some(unsafe { CStr::from_ptr(report_path) }.to_string_lossy().into_owned()) };
    run(&scratch, report.as_deref()) as c_int
}

/// Runs the ten checks; returns the number of failures.
pub fn run(scratch_dir: &str, report_path: Option<&str>) -> u32 {
    let scratch_file = format!("{}/ps5-rust-hello.tmp", scratch_dir.trim_end_matches('/'));
    let file = report_path.and_then(|p| std::fs::File::create(p).ok());
    let mut r = Report { passed: 0, failed: 0, text: String::new(), file };
    let started = Instant::now();

    // 1. stdio and the basics
    r.line("Rust on the PS5 — ps5-rust-hello (PS5_RUST_HELLO_V5)".to_string());
    let args: Vec<String> = std::env::args().collect();
    r.check(
        "basics",
        true,
        format!(
            "os={} arch={} pid={} args={:?} parallelism={:?}",
            std::env::consts::OS,
            std::env::consts::ARCH,
            std::process::id(),
            args,
            thread::available_parallelism().map(|n| n.get()),
        ),
    );

    // 2. the C shim: a toast on the TV
    notify("Rust is running on this PS5");
    r.check("shim", true, "ps5_notify called (look for the toast on the TV)".into());

    // 3. clocks
    r.line("..    clocks: SystemTime::now, thread::sleep(100ms)".into());
    let unix = SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_secs());
    let t0 = Instant::now();
    thread::sleep(Duration::from_millis(100));
    let slept = t0.elapsed();
    r.check(
        "clocks",
        unix.is_ok() && slept >= Duration::from_millis(95) && slept < Duration::from_millis(500),
        format!("unix={:?} sleep(100ms) took {:?}", unix, slept),
    );

    // 4. threads, mutex, condvar (std uses _umtx_op on FreeBSD for all three)
    r.line("..    threads: spawn x4, Mutex, Condvar".into());
    let counter = Arc::new(Mutex::new(0u64));
    let gate = Arc::new((Mutex::new(false), Condvar::new()));
    let mut handles = Vec::new();
    for _ in 0..4 {
        let counter = Arc::clone(&counter);
        let gate = Arc::clone(&gate);
        handles.push(thread::spawn(move || {
            let (lock, cv) = &*gate;
            let mut open = lock.lock().unwrap();
            while !*open {
                open = cv.wait(open).unwrap();
            }
            drop(open);
            for _ in 0..10_000 {
                *counter.lock().unwrap() += 1;
            }
        }));
    }
    thread::sleep(Duration::from_millis(20));
    {
        let (lock, cv) = &*gate;
        *lock.lock().unwrap() = true;
        cv.notify_all();
    }
    let joined = handles.into_iter().all(|h| h.join().is_ok());
    let total = *counter.lock().unwrap();
    r.check("threads", joined && total == 40_000, format!("4 threads x 10000 = {total}, joined={joined}"));

    // 5. HashMap: its RandomState seed comes from arc4random_buf
    r.line("..    hashmap: RandomState (arc4random_buf)".into());
    let mut m = HashMap::new();
    for i in 0..1000u32 {
        m.insert(format!("k{i}"), i);
    }
    r.check("hashmap", m.len() == 1000 && m.get("k777") == Some(&777), format!("{} entries", m.len()));

    // 6. filesystem with the FreeBSD-11 layouts: Rust's metadata vs the console's own stat
    r.line("..    fs: write, metadata, read, remove, read_dir, current_dir, current_exe".into());
    let c_sizeof_stat = unsafe { ps5_c_sizeof_stat() };
    let c_sizeof_dirent = unsafe { ps5_c_sizeof_dirent() };
    r.line(format!("info  C side: sizeof(struct stat)={c_sizeof_stat} (FreeBSD 11: 120) sizeof(struct dirent)={c_sizeof_dirent}"));
    let payload = vec![b'x'; 1234];
    let fs_ok = match std::fs::write(&scratch_file, &payload) {
        Ok(()) => {
            let rust_len = std::fs::metadata(&scratch_file).map(|m| m.len()).unwrap_or(u64::MAX);
            let c_path = CString::new(scratch_file.as_str()).unwrap();
            let c_len = unsafe { ps5_c_stat_size(c_path.as_ptr()) };
            let mut back = Vec::new();
            let read_ok = std::fs::File::open(&scratch_file)
                .and_then(|mut f| f.read_to_end(&mut back))
                .is_ok()
                && back == payload;
            let _ = std::fs::remove_file(&scratch_file);
            let ok = rust_len == 1234 && c_len == 1234 && read_ok;
            r.check(
                "fs-stat",
                ok,
                format!("wrote 1234 bytes: Rust metadata().len()={rust_len} C stat st_size={c_len} read back ok={read_ok}"),
            );
            ok
        }
        Err(e) => {
            r.check("fs-stat", false, format!("cannot write {scratch_file}: {e}"));
            false
        }
    };
    if fs_ok {
        // read_dir skips "." and ".."; the C count includes them when the filesystem lists them.
        // v7 counted a single Err as "1 entry": now Ok names and Err texts are told apart.
        let mut names: Vec<String> = Vec::new();
        let mut errors: Vec<String> = Vec::new();
        match std::fs::read_dir(scratch_dir) {
            Ok(d) => {
                for e in d {
                    match e {
                        Ok(e) => names.push(e.file_name().to_string_lossy().into_owned()),
                        Err(e) => errors.push(e.to_string()),
                    }
                }
            }
            Err(e) => errors.push(format!("read_dir: {e}")),
        }
        let c_path = CString::new(scratch_dir).unwrap();
        let c_n = unsafe { ps5_c_dir_count(c_path.as_ptr()) };
        let (_, probe) = c_probe(|out, size| unsafe { ps5_c_dir_probe(c_path.as_ptr(), out, size) });
        r.line(format!("info  C side: {probe}"));
        // report.txt is in that folder while this runs, so Rust must see at least one name
        r.check(
            "fs-readdir",
            !names.is_empty() && errors.is_empty(),
            format!("{scratch_dir}: Rust read_dir() names={names:?} errors={errors:?}, C readdir={c_n} (incl. . and .. where listed; negative = -errno)"),
        );
        let cwd = std::env::current_dir().map(|p| p.display().to_string());
        let exe = std::env::current_exe().map(|p| p.display().to_string());
        r.check("fs-paths", true, format!("current_dir={cwd:?} current_exe={exe:?}"));
    }

    // 7. memory: 64 MB, touched
    r.line("..    memory: 64 MB".into());
    let mut big = vec![0u8; 64 << 20];
    for i in (0..big.len()).step_by(4096) {
        big[i] = (i >> 12) as u8;
    }
    let sum: u64 = big.iter().step_by(4096).map(|&b| b as u64).sum();
    r.check("memory", sum > 0, format!("64 MB allocated and touched (checksum {sum})"));
    drop(big);

    // 8. one outbound TCP connection (getaddrinfo + socket + connect + a TLS ClientHello-sized write)
    r.line(format!("..    net: connect {NET_TARGET}"));
    {
        let (ip, port) = NET_TARGET.split_once(':').unwrap_or((NET_TARGET, "443"));
        let c_ip = CString::new(ip).unwrap();
        let port: c_int = port.parse().unwrap_or(443);
        let (rc, probe) = c_probe(|out, size| unsafe { ps5_c_net_probe(c_ip.as_ptr(), port, out, size) });
        r.line(format!("info  C side: {probe} => {}", if rc == 0 { "connected" } else { "not connected" }));
    }
    let net = NET_TARGET
        .to_socket_addrs()
        .map_err(|e| format!("resolve: {e}"))
        .and_then(|mut addrs| addrs.next().ok_or_else(|| "no address".to_string()))
        .and_then(|addr| TcpStream::connect_timeout(&addr, Duration::from_secs(5)).map_err(|e| format!("connect {addr}: {e}")))
        .and_then(|mut s| {
            s.set_write_timeout(Some(Duration::from_secs(5))).ok();
            s.write_all(b"\x16\x03\x01\x00\x05hello").map(|_| s.peer_addr().ok()).map_err(|e| format!("write: {e}"))
        });
    match net {
        Ok(peer) => r.check("net", true, format!("connected to {NET_TARGET} (peer {peer:?})")),
        Err(e) => r.check("net", false, format!("{e} — fine if the console has no internet; the other checks still count")),
    }

    // done
    let msg = format!("Rust on PS5: {} ok, {} failed, {:.1} s", r.passed, r.failed, started.elapsed().as_secs_f32());
    r.line(msg.clone());
    if let Some(p) = report_path {
        println!("info  report at {p} ({} bytes)", r.text.len());
    }
    notify(&msg);
    std::io::stdout().flush().ok();
    r.failed
}
