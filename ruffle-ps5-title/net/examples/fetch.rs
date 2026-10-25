//! fetch — one GET through r5net, as the console makes it: status, final address, headers, size,
//! the first bytes. For trying a site from a PC before the console does.
//!
//!   cargo run --example fetch -- URL [OUT_FILE]
//!
//! HTTPS_PROXY and R5NET_EXTRA_CA as for online-check.
// SPDX-License-Identifier: GPL-3.0-or-later

fn main() {
    let mut args = std::env::args().skip(1);
    let Some(url) = args.next() else {
        eprintln!("usage: fetch URL [OUT_FILE]");
        std::process::exit(2);
    };
    let out = args.next();
    let mut client = r5net::Client::new(&format!("Ruffle5/{} (PlayStation 5 Flash player; fetch; +https://github.com/TheUnwelcomed/RUFFLE5)", env!("CARGO_PKG_VERSION")));
    if let Some(p) = std::env::var("HTTPS_PROXY").ok().or_else(|| std::env::var("https_proxy").ok()).filter(|p| !p.is_empty()) {
        if let Err(e) = client.set_proxy(&p) {
            eprintln!("proxy {p}: {e}");
        }
    }
    if let Ok(ca) = std::env::var("R5NET_EXTRA_CA") {
        match std::fs::read(&ca).map_err(|e| e.to_string()).and_then(|pem| client.add_roots_pem(&pem)) {
            Ok(n) => eprintln!("{n} more root(s) from {ca}"),
            Err(e) => eprintln!("R5NET_EXTRA_CA {ca}: {e}"),
        }
    }
    struct L;
    impl log::Log for L {
        fn enabled(&self, m: &log::Metadata) -> bool {
            m.level() <= log::Level::Info
        }
        fn log(&self, r: &log::Record) {
            eprintln!("[{} {}] {}", r.level(), r.target(), r.args());
        }
        fn flush(&self) {}
    }
    static LOGGER: L = L;
    let _ = log::set_logger(&LOGGER);
    log::set_max_level(log::LevelFilter::Info);
    let mut req = r5net::Request::new(&url);
    req.limit = 64 << 20;
    match client.get(&req) {
        Ok(r) => {
            println!("status {} from {}", r.status, r.url);
            for (n, v) in &r.head.headers {
                println!("  {n}: {v}");
            }
            let head: String = r.text().chars().take(300).collect();
            println!("{} bytes; starts: {head:?}", r.body.len());
            if let Some(o) = out {
                std::fs::write(&o, &r.body).expect("writing the body");
                println!("saved {o}");
            }
        }
        Err(e) => {
            println!("FAIL {e}");
            std::process::exit(1);
        }
    }
}
