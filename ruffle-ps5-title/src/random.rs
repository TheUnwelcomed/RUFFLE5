//! PS5_RUFFLE_RANDOM_V1 — the `getrandom` crate's custom backend for the PS5.
//!
//! Ruffle's `rand` (0.10, through getrandom 0.4) and `ahash` (through getrandom 0.3) ask the OS for
//! entropy. On `target_os = "freebsd"` the crate calls `getrandom(2)`, a FreeBSD 12 call: the
//! console is FreeBSD 11 underneath and the SDK's libc has no `getrandom`. Both crate versions
//! accept a custom backend instead (`--cfg getrandom_backend="custom"` in .cargo/config.toml): an
//! `extern "Rust"` function named `__getrandom_v03_custom` — the same name and shape in 0.3 and 0.4,
//! their `Error` being a newtype over `NonZeroU32` in both — which this file provides over
//! `arc4random_buf`, bound to the platform layer's `ps5_arc4random_buf` at link time (radv-link.sh;
//! the console's own libc lacks it). `arc4random_buf` cannot fail, so the result is always `Ok`.
// SPDX-License-Identifier: GPL-3.0-or-later

use std::ffi::c_void;
use std::num::NonZeroU32;

extern "C" {
    fn arc4random_buf(buf: *mut c_void, nbytes: usize);
}

/// Layout-identical to `getrandom::Error` (0.3 and 0.4): a `NonZeroU32` newtype, so that
/// `Result<(), Error>` is one `u32` with 0 meaning `Ok(())`. Never constructed here.
#[repr(transparent)]
#[allow(dead_code)]
pub struct GetrandomError(NonZeroU32);

#[no_mangle]
pub unsafe extern "Rust" fn __getrandom_v03_custom(
    dest: *mut u8,
    len: usize,
) -> Result<(), GetrandomError> {
    if len != 0 {
        arc4random_buf(dest.cast(), len);
    }
    Ok(())
}
