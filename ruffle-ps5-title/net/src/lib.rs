//! R5NET_V1 — Ruffle5's network (1.000.026): HTTPS on the console's sockets and resolver, with
//! rustls and pure-Rust cryptography, and the online sources the menu's Online tab lists — the
//! Internet Archive's Flash collection (src/ia.rs) and Flash Museum (src/fm.rs).
//!
//! A crate of its own, so the same code runs on a PC: `cargo run --example online-check` asks
//! both sources for real (tools/online-check.sh), before a console build.
// SPDX-License-Identifier: GPL-3.0-or-later

pub mod fm;
pub mod http;
pub mod ia;
pub mod source;
mod tls;
pub mod url;

pub use http::{Client, Download, Error, Head, Request, Response};
pub use source::{Files, Item, Page, RemoteFile, Source, SOURCES};
