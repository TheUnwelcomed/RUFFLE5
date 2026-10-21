//! The elfldr-payload form of ps5-rust-hello: the checks from src/lib.rs with /data as the
//! scratch folder and no report file (the sender sees the output). Exit code = failures.
// SPDX-License-Identifier: GPL-3.0-or-later

fn main() {
    let failed = ps5_rust_hello::run("/data", None);
    std::process::exit(failed as i32);
}
