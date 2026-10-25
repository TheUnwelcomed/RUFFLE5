//! R5NET_V1 — Ruffle5's network (1.000.026): HTTPS on the console's sockets and resolver, with
//! rustls and pure-Rust cryptography, and the online sources the menu's Online tab lists — the
//! Internet Archive's Flash collection (src/ia.rs), Flash Museum (src/fm.rs) and, 1.000.028,
//! Flashpoint Archive's Shockwave games (src/fp.rs, with its game data's zips: src/zip.rs).
//!
//! A crate of its own, so the same code runs on a PC: `cargo run --example online-check` asks
//! both sources for real (tools/online-check.sh), before a console build.
// SPDX-License-Identifier: GPL-3.0-or-later

pub mod fm;
pub mod fp;
pub mod http;
pub mod ia;
pub mod source;
mod tls;
pub mod url;
pub mod zip;

pub use http::{Client, Download, Error, Head, Request, Response};
pub use source::{Archive, Elsewhere, Files, Item, Other, Page, RemoteFile, Source, SOURCES};

/// A file's SHA-256, lowercase hex (Flashpoint's game data is checked against it).
pub fn sha256_file(path: &std::path::Path) -> std::io::Result<String> {
    use sha2::{Digest, Sha256};
    use std::io::Read;
    let mut f = std::fs::File::open(path)?;
    let mut h = Sha256::new();
    let mut buf = vec![0u8; 1 << 16];
    loop {
        let n = f.read(&mut buf)?;
        if n == 0 {
            break;
        }
        h.update(&buf[..n]);
    }
    Ok(h.finalize().iter().map(|b| format!("{b:02x}")).collect())
}
