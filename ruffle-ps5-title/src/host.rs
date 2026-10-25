//! Stand-ins for the C shell's functions (shell/*.c) and the console's own, so the crate's tests
//! build and run on a PC (`cargo test --features ruffle --target x86_64-unknown-linux-gnu`, with the
//! wgpu checkout build-title.sh makes). Only in test builds, never in a title.
// SPDX-License-Identifier: GPL-3.0-or-later
#![allow(clippy::missing_safety_doc)]

use std::ffi::{c_char, c_int, c_void, CStr};

#[no_mangle]
pub unsafe extern "C" fn ps5_log(msg: *const c_char) {
    if std::env::var_os("RUFFLE5_TEST_LOG").is_some() {
        eprintln!("{}", CStr::from_ptr(msg).to_string_lossy());
    }
}

#[no_mangle]
pub unsafe extern "C" fn ps5_notify(_msg: *const c_char) {}

#[no_mangle]
pub extern "C" fn ps5_local_hour() -> c_int {
    12
}

#[no_mangle]
pub extern "C" fn ps5_vk_get_instance_proc_addr() -> *const c_void {
    std::ptr::null()
}

#[no_mangle]
pub unsafe extern "C" fn ps5_pad_init(detail: *mut i32) -> i32 {
    *detail = 1;
    0
}

#[no_mangle]
pub unsafe extern "C" fn ps5_pad_open(_detail: *mut i32) -> i32 {
    -4
}

#[no_mangle]
pub unsafe extern "C" fn ps5_pad_read(_out: *mut c_void) -> i32 {
    0
}

#[no_mangle]
pub unsafe extern "C" fn ps5_audio_open(_frames: u32, detail: *mut i32) -> i32 {
    *detail = -1;
    -1
}

#[no_mangle]
pub unsafe extern "C" fn ps5_audio_output(_port: i32, _frames: *const i16) -> i32 {
    0
}

#[no_mangle]
pub extern "C" fn ps5_audio_close(_port: i32) -> i32 {
    0
}

#[no_mangle]
pub unsafe extern "C" fn ps5_hid_load(_which: i32, detail: *mut i32) -> i32 {
    *detail = 0;
    -1
}

#[no_mangle]
pub extern "C" fn ps5_hid_sysmodule_rc(_which: i32) -> i32 {
    1
}

#[no_mangle]
pub extern "C" fn ps5_hid_source(_which: i32) -> i32 {
    0
}

#[no_mangle]
pub extern "C" fn ps5_hid_init(_which: i32) -> i32 {
    -1
}

#[no_mangle]
pub extern "C" fn ps5_hid_open(_which: i32, _user: i32, _index: i32) -> i32 {
    -1
}

#[no_mangle]
pub unsafe extern "C" fn ps5_hid_read(_which: i32, _handle: i32, _data: *mut u8, _count: i32) -> i32 {
    0
}

#[no_mangle]
pub unsafe extern "C" fn ps5_hid_read_state(_handle: i32, _data: *mut u8) -> i32 {
    -1000
}

#[no_mangle]
pub extern "C" fn ps5_hid_close(_which: i32, _handle: i32) -> i32 {
    -1
}

#[no_mangle]
pub unsafe extern "C" fn scePthreadGetaffinity(_thread: usize, mask: *mut u64) -> i32 {
    *mask = 0;
    -1
}

#[no_mangle]
pub extern "C" fn scePthreadSetaffinity(_thread: usize, _mask: u64) -> i32 {
    -1
}

#[no_mangle]
pub unsafe extern "C" fn cpuset_setaffinity(_level: i32, _which: i32, _id: i64, _size: usize, _mask: *const u8) -> i32 {
    -1
}

#[no_mangle]
pub unsafe extern "C" fn ps5_import_missing(_name: *const c_char) -> i32 {
    0
}
