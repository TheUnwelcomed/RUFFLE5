//! PS5_RUFFLE_CPU_V1 — CPU cores: the sound gets a core of its own (its hyperthread sibling left
//! idle), every other thread the rest. On the PS5 a new thread inherits its creator's CPUs, so
//! without this the sound can share a core with a busy game and miss its deadline (crackle).
//!
//! As Ruffle-Flash-PS5 lays them out (src/cpu.rs, GPL-3.0, by ZiZc3, after XPSemu and PS5SX2):
//! at start-up keep the top whole core for the sound and move the process to the CPUs left over;
//! the sound thread pins itself to that core when it starts. If the console gives no CPU mask, the
//! layout is skipped and everything runs as before.
// SPDX-License-Identifier: GPL-3.0-or-later

use std::sync::atomic::{AtomicI32, Ordering};

extern "C" {
    /// 1 when the console left that import unbound (shell/hid.c, over build-title.sh's import table).
    fn ps5_import_missing(name: *const std::ffi::c_char) -> i32;
    fn pthread_self() -> usize;
    fn scePthreadGetaffinity(thread: usize, mask: *mut u64) -> i32;
    fn scePthreadSetaffinity(thread: usize, mask: u64) -> i32;
    fn cpuset_setaffinity(level: i32, which: i32, id: i64, size: usize, mask: *const u8) -> i32;
}

const CPU_LEVEL_WHICH: i32 = 3;
const CPU_WHICH_PID: i32 = 2;
const ERANGE: i32 = 34;

static AUDIO_CPU: AtomicI32 = AtomicI32::new(-1);
/// CPUs the title's work runs on (after the sound's core), 0 while unknown.
static WORK_CPUS: AtomicI32 = AtomicI32::new(0);

/// How many CPUs the game's work may use: the mask `init` left the process, or None when the
/// console gave none.
pub fn work_cpus() -> Option<usize> {
    match WORK_CPUS.load(Ordering::Relaxed) {
        n if n > 0 => Some(n as usize),
        _ => None,
    }
}

fn missing(name: &std::ffi::CStr) -> bool {
    unsafe { ps5_import_missing(name.as_ptr()) != 0 }
}

/// Once, at start-up. Returns a line for the log.
pub fn init() -> String {
    for name in [c"scePthreadGetaffinity", c"scePthreadSetaffinity", c"cpuset_setaffinity"] {
        if missing(name) {
            return format!("cpus: {} is not available, layout skipped", name.to_string_lossy());
        }
    }
    let mut all = 0u64;
    if unsafe { scePthreadGetaffinity(pthread_self(), &mut all) } != 0 || all == 0 {
        return "cpus: no CPU mask, layout skipped".into();
    }
    WORK_CPUS.store(all.count_ones() as i32, Ordering::Relaxed);
    let Some(k) = (0..32).rev().find(|k| all & (3u64 << (2 * k)) == 3u64 << (2 * k)) else {
        return format!("cpus {all:#x}: no whole core free, layout skipped");
    };
    let rest = all & !(3u64 << (2 * k));
    if rest == 0 {
        return format!("cpus {all:#x}: one core only, layout skipped");
    }
    // The console's kernel may want a smaller set than FreeBSD's (PS5SX2 found 8, 16 or 32 bytes).
    let mut set = [0u8; 32];
    set[..8].copy_from_slice(&rest.to_le_bytes());
    let mut rc = -1;
    for size in [8usize, 16, 32] {
        rc = if unsafe { cpuset_setaffinity(CPU_LEVEL_WHICH, CPU_WHICH_PID, -1, size, set.as_ptr()) } == 0 {
            0
        } else {
            std::io::Error::last_os_error().raw_os_error().unwrap_or(-1)
        };
        if rc != ERANGE {
            break;
        }
    }
    AUDIO_CPU.store(2 * k as i32, Ordering::Relaxed);
    if rc == 0 {
        WORK_CPUS.store(rest.count_ones() as i32, Ordering::Relaxed);
    }
    format!("cpus {all:#x}: sound on CPU {}, the rest on {rest:#x} (rc {rc})", 2 * k)
}

/// Called by a sound thread when it starts.
pub fn pin_audio_thread() -> Option<String> {
    let cpu = AUDIO_CPU.load(Ordering::Relaxed);
    if cpu < 0 {
        return None;
    }
    let rc = unsafe { scePthreadSetaffinity(pthread_self(), 1u64 << cpu) };
    if rc != 0 {
        Some(format!("warn sound thread to CPU {cpu}: 0x{:x}", rc as u32))
    } else {
        None
    }
}
