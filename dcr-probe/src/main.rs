//! dcr-probe — play a Director movie in DirPlayer, natively, and record what happens.
//!
//!   dcr-probe <movie.dir|.dcr> <out folder> [seconds] [script]
//!
//! Every `--every` seconds (2 by default) the stage is saved as a PNG (`t0012.png` = 12 s in), and
//! once a second a status line is printed: time, score frame, playing. DirPlayer's own warnings and
//! Lingo errors go to stderr ("[WARN]" / "[ERROR]"); a panic is also written to `panic.txt`.
//!
//! The script is a list of timed inputs separated by `;`, each `SECONDS ACTION`:
//!   12 click 320 240     a mouse click on the stage
//!   14 move 100 50       move the mouse
//!   15 key Enter         a key press (down, one frame, up)
//!   16 down Right        hold a key ...      20 up Right   ... and let go
//!   18 snap name         save the stage as name.png
//!   19 eval EXPR         print a Lingo expression's value (dot syntax: `_movie.frameLabel`)
//!   20 lingo             write every loaded script's Lingo (decompiled) to <out>/lingo/
//! Keys: Enter Space Left Right Up Down Escape Shift Control Tab Backspace, a..z, 0..9.
//!
//! The movie runs as Ruffle5 runs it on the console (src/shockwave.rs): one score frame
//! (`run_native_frame`, DirPlayer's per-frame steps for a native host) when the tempo says it is due,
//! then the stage rendered, then the rest of a 60 Hz display refresh. The status line gives frames
//! and renders a second, the milliseconds a frame (lingo) and a render (draw) take — wall time and
//! this thread's CPU time — and what they allocate; a second line what the render's blits cost.
//!
//! Sound goes nowhere, but it is mixed as the console would (`native_audio::mix`, in step with the
//! clock), so a sound's end reaches the movie as it would there; the status line counts the voices.
//!
//! Text in a font the movie does not carry is drawn with the Liberation fonts, as Ruffle5 draws it:
//! from /usr/share/fonts/truetype/liberation (Debian/Ubuntu: fonts-liberation), or the folder in
//! DCR_PROBE_FONTS. The movie gets DirPlayer's system font as on the console (NativePlayer::new);
//! DCR_PROBE_NO_SYSTEM_FONT=1 leaves it out. The state at the end counts the strings drawn so.
//! Note: a scripted `click` moves the mouse and clicks at once, so a mouseEnter the move causes
//! reaches the movie after the click; put a `move` to the spot before it, as a player would.
//!
//! A game the Online tab fetched starts as Ruffle5 starts it: DCR_PROBE_LAUNCH its launch command
//! (online.txt `launch`), DCR_PROBE_ADDRESS the address it was published at (`url`),
//! DCR_PROBE_WEBROOT the folder its web addresses are read from (`webroot`: Flashpoint's content/).
//!
//! DCR_PROBE_BENCH=1 first prints the CPU / clock / memory benchmark Ruffle5 logs on the console.
//! DCR_PROBE_FAST=1: no waiting for the display refresh (frames still at most at the tempo).
//! DCR_PROBE_COMPARE=1: every stage rendered a second time without DirPlayer's upright blit fast
//! path and compared byte for byte (=2: with it, to check a render repeats exactly).
//! DCR_PROBE_PROF=1: a sampling profile of the renders, written to <out>/prof.txt.
//! With an LD_PRELOAD shim exporting `shim_clock_calls`, the clock reads a frame and a render make.
//!
//!   dcr-probe --blit-selftest [N]   N random blits (default 20000), each drawn with DirPlayer's
//!                                   upright fast path and with its pixel loop: they must match.
//!
//! At the end (and when the score stays on one frame for 10 s) the probe prints the movie's state:
//! its name and path, the frame and its label, the labels, every cast library with its file and
//! member count, and what each sprite channel shows.
// SPDX-License-Identifier: GPL-3.0-or-later

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering::Relaxed};
use std::time::Instant;

/// Counts allocations (calls and bytes; and those of 64 KiB or more) so the status line can say
/// what a score frame and a stage render allocate — memory the console's allocator has to serve.
struct Counting;
static ALLOCS: AtomicU64 = AtomicU64::new(0);
static ALLOC_BYTES: AtomicU64 = AtomicU64::new(0);
static BIG_ALLOCS: AtomicU64 = AtomicU64::new(0);
unsafe impl std::alloc::GlobalAlloc for Counting {
    unsafe fn alloc(&self, l: std::alloc::Layout) -> *mut u8 {
        ALLOCS.fetch_add(1, Relaxed);
        ALLOC_BYTES.fetch_add(l.size() as u64, Relaxed);
        if l.size() >= 65536 {
            BIG_ALLOCS.fetch_add(1, Relaxed);
        }
        unsafe { std::alloc::System.alloc(l) }
    }
    unsafe fn dealloc(&self, p: *mut u8, l: std::alloc::Layout) {
        unsafe { std::alloc::System.dealloc(p, l) }
    }
    unsafe fn realloc(&self, p: *mut u8, l: std::alloc::Layout, n: usize) -> *mut u8 {
        ALLOCS.fetch_add(1, Relaxed);
        ALLOC_BYTES.fetch_add(n as u64, Relaxed);
        if n >= 65536 {
            BIG_ALLOCS.fetch_add(1, Relaxed);
        }
        unsafe { std::alloc::System.realloc(p, l, n) }
    }
}
#[global_allocator]
static GLOBAL: Counting = Counting;
/// DCR_PROBE_PROF=1: a sampling profile of the stage render — SIGPROF every 250 us of CPU time
/// records where the render thread is (its instruction pointer) while `PROF_ON` is set; the
/// addresses are written to <out>/prof.txt at the end (symbolize with addr2line -f -i -C).
mod prof {
    use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
    pub static PROF_ON: AtomicBool = AtomicBool::new(false);
    const N: usize = 1 << 20;
    static mut PCS: [u64; N] = [0; N];
    static COUNT: AtomicUsize = AtomicUsize::new(0);
    #[repr(C)]
    struct Timeval {
        s: i64,
        us: i64,
    }
    #[repr(C)]
    struct Itimer {
        interval: Timeval,
        value: Timeval,
    }
    #[repr(C)]
    struct Sigaction {
        handler: usize,
        mask: [u64; 16],
        flags: i32,
        restorer: usize,
    }
    unsafe extern "C" {
        fn sigaction(sig: i32, act: *const Sigaction, old: *mut Sigaction) -> i32;
        fn setitimer(which: i32, new: *const Itimer, old: *mut Itimer) -> i32;
    }
    extern "C" fn on_prof(_sig: i32, _info: *mut u8, ctx: *mut u8) {
        if !PROF_ON.load(Ordering::Relaxed) || ctx.is_null() {
            return;
        }
        let rip = unsafe { *(ctx.add(168) as *const u64) };
        let i = COUNT.fetch_add(1, Ordering::Relaxed);
        if i < N {
            unsafe { PCS[i] = rip };
        }
    }
    pub fn start() {
        unsafe {
            let act = Sigaction { handler: on_prof as *const () as usize, mask: [0; 16], flags: 0x4 | 0x10000000, restorer: 0 };
            sigaction(27, &act, std::ptr::null_mut());
            let t = Itimer { interval: Timeval { s: 0, us: 250 }, value: Timeval { s: 0, us: 250 } };
            setitimer(2, &t, std::ptr::null_mut());
        }
    }
    pub fn dump(path: &std::path::Path) {
        let n = COUNT.load(Ordering::Relaxed).min(N);
        // the executable's load address (PIE), from /proc/self/maps
        let maps = std::fs::read_to_string("/proc/self/maps").unwrap_or_default();
        let exe = std::fs::read_link("/proc/self/exe").unwrap_or_default();
        let base = maps
            .lines()
            .find(|l| l.ends_with(exe.to_str().unwrap_or("?")))
            .and_then(|l| u64::from_str_radix(l.split('-').next().unwrap_or("0"), 16).ok())
            .unwrap_or(0);
        let mut text = format!("base {base:x} samples {n}\n");
        for i in 0..n {
            let pc = unsafe { PCS[i] };
            text += &format!("{:x}\n", pc.wrapping_sub(base));
        }
        let _ = std::fs::write(path, text);
    }
}

/// This thread's CPU time in ms (CLOCK_THREAD_CPUTIME_ID): a frame's work without waits.
fn thread_cpu_ms() -> f64 {
    #[repr(C)]
    struct Ts {
        s: i64,
        ns: i64,
    }
    unsafe extern "C" {
        fn clock_gettime(id: i32, ts: *mut Ts) -> i32;
    }
    let mut ts = Ts { s: 0, ns: 0 };
    unsafe { clock_gettime(3, &mut ts) };
    ts.s as f64 * 1000.0 + ts.ns as f64 / 1e6
}

/// clock_gettime calls so far, when an LD_PRELOAD shim exporting `shim_clock_calls` is loaded.
fn clock_calls() -> u64 {
    unsafe extern "C" {
        fn dlsym(handle: *mut std::ffi::c_void, name: *const std::ffi::c_char) -> *mut std::ffi::c_void;
    }
    static F: std::sync::OnceLock<usize> = std::sync::OnceLock::new();
    let f = *F.get_or_init(|| unsafe { dlsym(std::ptr::null_mut(), c"shim_clock_calls".as_ptr()) as usize });
    if f == 0 {
        return 0;
    }
    let f: extern "C" fn() -> u64 = unsafe { std::mem::transmute(f) };
    f()
}
fn alloc_counts() -> (u64, u64, u64) {
    (ALLOCS.load(Relaxed), ALLOC_BYTES.load(Relaxed), BIG_ALLOCS.load(Relaxed))
}

use vm_rust::player::testing::{run_test, StageSnapshot, TestPlayer};
use vm_rust::player::testing_shared::TestHarness;

#[derive(Debug, Clone)]
enum Act {
    Click(i32, i32),
    Move(i32, i32),
    Key(String),
    Down(String),
    Up(String),
    Snap(String),
    Eval(String),
    Lingo,
}

/// A key name as the browser front end would hand it to DirPlayer: (KeyboardEvent.key, keyCode).
fn js_key(name: &str) -> Option<(String, u16)> {
    let named = match name.to_ascii_lowercase().as_str() {
        "enter" | "return" => Some(("Enter", 13)),
        "space" => Some((" ", 32)),
        "left" => Some(("ArrowLeft", 37)),
        "up" => Some(("ArrowUp", 38)),
        "right" => Some(("ArrowRight", 39)),
        "down" => Some(("ArrowDown", 40)),
        "escape" | "esc" => Some(("Escape", 27)),
        "shift" => Some(("Shift", 16)),
        "control" | "ctrl" => Some(("Control", 17)),
        "tab" => Some(("Tab", 9)),
        "backspace" => Some(("Backspace", 8)),
        _ => None,
    };
    if let Some((k, c)) = named {
        return Some((k.to_string(), c));
    }
    let mut chars = name.chars();
    match (chars.next(), chars.next()) {
        (Some(c), None) if c.is_ascii_alphabetic() => Some((c.to_ascii_lowercase().to_string(), c.to_ascii_uppercase() as u16)),
        (Some(c), None) if c.is_ascii_digit() => Some((c.to_string(), c as u16)),
        _ => None,
    }
}

fn parse_script(text: &str) -> Vec<(f64, Act)> {
    let mut out = Vec::new();
    for item in text.split(';').map(str::trim).filter(|s| !s.is_empty()) {
        let w: Vec<&str> = item.split_whitespace().collect();
        let Some(at) = w.first().and_then(|s| s.parse::<f64>().ok()) else {
            eprintln!("script: no time in {item:?}, skipped");
            continue;
        };
        let num = |i: usize| w.get(i).and_then(|s| s.parse::<i32>().ok());
        let act = match (w.get(1).copied(), w.get(2).copied()) {
            (Some("click"), _) => num(2).zip(num(3)).map(|(x, y)| Act::Click(x, y)),
            (Some("move"), _) => num(2).zip(num(3)).map(|(x, y)| Act::Move(x, y)),
            (Some("key"), Some(k)) => Some(Act::Key(k.to_string())),
            (Some("down"), Some(k)) => Some(Act::Down(k.to_string())),
            (Some("up"), Some(k)) => Some(Act::Up(k.to_string())),
            (Some("snap"), Some(n)) => Some(Act::Snap(n.to_string())),
            (Some("lingo"), _) => Some(Act::Lingo),
            (Some("eval"), Some(_)) => {
                let rest = item.splitn(3, char::is_whitespace).nth(2).unwrap_or("").trim().to_string();
                Some(Act::Eval(rest))
            }
            _ => None,
        };
        match act {
            Some(a) => out.push((at, a)),
            None => eprintln!("script: cannot read {item:?}, skipped"),
        }
    }
    out.sort_by(|a, b| a.0.partial_cmp(&b.0).unwrap());
    out
}

fn save(player: &TestPlayer, out: &Path, name: &str) {
    let snap = StageSnapshot::from_output(player.snapshot_stage());
    let path = out.join(format!("{name}.png"));
    match std::fs::write(&path, snap.to_png()) {
        Ok(()) => println!("[probe] saved {} ({}x{})", path.display(), snap.width, snap.height),
        Err(e) => eprintln!("[probe] cannot write {}: {e}", path.display()),
    }
}

async fn show(player: &TestPlayer, expr: &str) {
    match player.eval_datum(expr).await {
        Ok(v) => println!("[eval] {expr} = {v:?}"),
        Err(e) => println!("[eval] {expr} -> error {e:?}"),
    }
}

/// The movie's state, for telling where a game waits.
async fn dump(player: &TestPlayer) {
    println!("[probe] ---- state ----");
    let (measured, drawn) = vm_rust::player::native_text::counts();
    println!("[probe] system-font text: {measured} strings measured, {drawn} drawn");
    for e in ["_movie.name", "_movie.path", "_movie.frame", "_movie.frameLabel", "_player.productVersion", "_system.environmentPropList"] {
        show(player, e).await;
    }
    let libs = match player.eval_datum("castLib(\"Lingo\").number").await {
        // the number of cast libraries: probe upwards until castLib(n) fails
        Ok(_) => {
            let mut n = 0;
            while n < 64 && matches!(player.eval_datum(&format!("castLib({}).name", n + 1)).await, Ok(vm_rust::director::static_datum::StaticDatum::String(_))) {
                n += 1;
            }
            Ok(vm_rust::director::static_datum::StaticDatum::Int(n))
        }
        e => e,
    };
    let libs = match libs {
        Ok(vm_rust::director::static_datum::StaticDatum::Int(n)) => n.clamp(0, 64),
        _ => 0,
    };
    for i in 1..=libs {
        show(player, &format!("castLib({i}).name")).await;
        show(player, &format!("castLib({i}).fileName")).await;
        show(player, &format!("castLib({i}).member.count")).await;
    }
    let last = match player.eval_datum("_movie.lastChannel").await {
        Ok(vm_rust::director::static_datum::StaticDatum::Int(n)) => n.clamp(0, 1000),
        _ => 150,
    };
    for ch in 1..=last {
        if let Ok(v) = player.eval_datum(&format!("sprite({ch}).member.name")).await {
            if !matches!(v, vm_rust::director::static_datum::StaticDatum::Void) && v != vm_rust::director::static_datum::StaticDatum::String(String::new()) {
                let loc = player.eval_datum(&format!("sprite({ch}).loc")).await.ok();
                let vis = player.eval_datum(&format!("sprite({ch}).visible")).await.ok();
                println!("[sprite] {ch}: {v:?} at {loc:?} visible {vis:?}");
            }
        }
    }
    println!("[probe] ---- end of state ----");
}

/// Every script the movie has loaded, decompiled by DirPlayer, one file per script.
fn dump_lingo(dir: &Path) {
    use vm_rust::player::mcp;
    let _ = std::fs::create_dir_all(dir);
    let list: serde_json::Value = vm_rust::player::reserve_player_ref(|p| {
        serde_json::from_str(&mcp::mcp_list_scripts(p, None, None, None)).unwrap_or(serde_json::Value::Null)
    });
    let scripts = list["scripts"].as_array().cloned().unwrap_or_default();
    let (mut files, mut handlers) = (0, 0);
    for sc in scripts {
        let lib = sc["cast_lib"].as_i64().unwrap_or(0) as i32;
        let mem = sc["cast_member"].as_i64().unwrap_or(0) as i32;
        let details: serde_json::Value = vm_rust::player::reserve_player_ref(|p| serde_json::from_str(&mcp::mcp_get_script(p, lib, mem)).unwrap_or(serde_json::Value::Null));
        let name = details["name"].as_str().unwrap_or("").to_string();
        let kind = details["script_type"].as_str().unwrap_or("").to_string();
        let mut text = format!("-- castLib {lib} member {mem}: {name} ({kind})\n");
        if let Some(props) = details["properties"].as_array() {
            if !props.is_empty() {
                let p: Vec<String> = props.iter().filter_map(|v| v.as_str().map(String::from)).collect();
                text += &format!("property {}\n", p.join(", "));
            }
        }
        for h in details["handlers"].as_array().cloned().unwrap_or_default() {
            let hn = h["name"].as_str().unwrap_or("").to_string();
            let out: serde_json::Value = vm_rust::player::reserve_player_ref(|p| serde_json::from_str(&mcp::mcp_decompile_handler(p, lib, mem, &hn)).unwrap_or(serde_json::Value::Null));
            let args: Vec<String> = out["arguments"].as_array().map(|a| a.iter().filter_map(|v| v.as_str().map(String::from)).collect()).unwrap_or_default();
            text += &format!("\non {hn} {}\n", args.join(", "));
            for line in out["source"].as_str().unwrap_or("-- (not decompiled)").lines() {
                text += &format!("  {line}\n");
            }
            text += "end\n";
            handlers += 1;
        }
        let safe: String = name.chars().map(|c| if c.is_ascii_alphanumeric() || c == '_' { c } else { '_' }).collect();
        let _ = std::fs::write(dir.join(format!("{lib:02}_{mem:04}_{safe}.ls")), text);
        files += 1;
    }
    println!("[probe] lingo: {files} scripts, {handlers} handlers -> {}", dir.display());
}

/// The same measurements Ruffle5 logs once on the console (ruffle-ps5-title/src/shockwave.rs
/// `bench`; keep the two identical), printed here with DCR_PROBE_BENCH=1 for comparison.
fn bench() -> String {
    use std::hint::black_box;
    let ns = |t: Instant, n: f64| t.elapsed().as_nanos() as f64 / n;
    // the clock (Instant: CLOCK_MONOTONIC; SystemTime: CLOCK_REALTIME, what chrono::Utc::now reads)
    let n = 20_000.0;
    let t = Instant::now();
    for _ in 0..20_000 {
        black_box(Instant::now());
    }
    let mono = ns(t, n);
    let t = Instant::now();
    for _ in 0..20_000 {
        black_box(std::time::SystemTime::now());
    }
    let wall = ns(t, n);
    // integer work (xorshift)
    let t = Instant::now();
    let mut x = 0x2545_f491_4f6c_dd1du64;
    for _ in 0..20_000_000 {
        x ^= x << 13;
        x ^= x >> 7;
        x ^= x << 17;
    }
    black_box(x);
    let int_ns = ns(t, 20e6);
    // small allocations (Lingo makes ~65 000 a frame), then 1 MiB ones written in full
    let t = Instant::now();
    for i in 0..100_000u32 {
        black_box(Box::new([i as u8; 48]));
    }
    let small = ns(t, 100_000.0);
    let t = Instant::now();
    for _ in 0..20 {
        black_box(vec![1u8; 1 << 20]);
    }
    let big_us = ns(t, 20.0) / 1000.0;
    // memory: read-modify-write passes over a buffer that fits L2, L3, and one that does not
    let mut bw = Vec::new();
    for kib in [64usize, 1024, 8192] {
        let mut buf = vec![1u32; kib * 256];
        let passes = (32 * 1024 / kib).max(4);
        let t = Instant::now();
        let mut sum = 0u64;
        for _ in 0..passes {
            for v in buf.iter_mut() {
                *v = v.wrapping_add(1);
                sum = sum.wrapping_add(*v as u64);
            }
        }
        black_box(sum);
        let mb = (kib * passes) as f64 / 1024.0;
        bw.push(format!("{kib} KiB {:.0} MB/s", mb / t.elapsed().as_secs_f64()));
    }
    // a blit like DirPlayer's: 600x400 RGBA, per-pixel f32 blend, 10 passes
    let src = vec![0x80u8; 600 * 400 * 4];
    let mut dst = vec![0x40u8; 600 * 400 * 4];
    let t = Instant::now();
    for pass in 0..10 {
        let a = 0.25 + pass as f32 * 0.05;
        for (d, s) in dst.chunks_exact_mut(4).zip(src.chunks_exact(4)) {
            for k in 0..3 {
                d[k] = (s[k] as f32 * a + d[k] as f32 * (1.0 - a)) as u8;
            }
        }
    }
    black_box(&dst);
    let blit = ns(t, 10.0 * 600.0 * 400.0);
    format!(
        "bench: clock {mono:.0} ns (monotonic), {wall:.0} ns (realtime); int {int_ns:.2} ns/op; alloc 48 B {small:.0} ns, 1 MiB {big_us:.0} us; memory {}; blend {blit:.1} ns/px",
        bw.join(", ")
    )
}

/// `dcr-probe --blit-selftest N`: N random upright blits onto a 32-bit bitmap — indexed and
/// RGBA sources, every ink the fast path takes and some it does not, masks, mattes, #blend, rects
/// partly off both bitmaps — each drawn with DirPlayer's upright fast path and with its pixel
/// loop; the two results must be the same bytes.
fn blit_selftest(n: u32) -> bool {
    use std::sync::atomic::Ordering::Relaxed;
    use vm_rust::player::bitmap::bitmap::{BuiltInPalette, Bitmap, PaletteRef};
    use vm_rust::player::bitmap::drawing::{draw_stats, CopyPixelsParams};
    use vm_rust::player::bitmap::mask::BitmapMask;
    use vm_rust::player::bitmap::palette_map::PaletteMap;
    use vm_rust::player::geometry::IntRect;
    use vm_rust::player::sprite::ColorRef;
    let mut seed = 0x9e37_79b9_7f4a_7c15u64;
    let mut rnd = move |m: u32| -> u32 {
        seed ^= seed << 13;
        seed ^= seed >> 7;
        seed ^= seed << 17;
        (seed % m.max(1) as u64) as u32
    };
    let palettes = PaletteMap::new();
    let pals = [BuiltInPalette::SystemMac, BuiltInPalette::SystemWin, BuiltInPalette::GrayScale, BuiltInPalette::Web216, BuiltInPalette::Rainbow];
    let inks = [0u32, 0, 0, 2, 7, 8, 8, 8, 32, 33, 35, 36, 36, 36, 37, 39, 40, 41, 5, 9];
    let (mut taken, mut failed) = (0u32, 0u32);
    for t in 0..n {
        let (dw, dh) = (1 + rnd(90) as u16, 1 + rnd(70) as u16);
        let mut dst = Bitmap::new(dw, dh, 32, 32, 0, PaletteRef::BuiltIn(BuiltInPalette::SystemMac));
        for b in dst.data.iter_mut() {
            *b = rnd(256) as u8;
        }
        dst.use_alpha = rnd(3) == 0;
        let indexed = rnd(3) != 0;
        let (sw, sh) = (1 + rnd(60) as u16, 1 + rnd(50) as u16);
        let pal = PaletteRef::BuiltIn(pals[rnd(pals.len() as u32) as usize]);
        let mut src = if indexed {
            let orig = [1u8, 2, 4, 8, 8, 8][rnd(6) as usize];
            let mut b = Bitmap::new(sw, sh, 8, orig, 0, pal);
            let few = rnd(2) == 0; // few distinct indices: colour-key and matte hits
            for v in b.data.iter_mut() {
                *v = if few { [0u8, 255, 3, 17][rnd(4) as usize] } else { rnd(256) as u8 };
            }
            b
        } else {
            let mut b = Bitmap::new(sw, sh, 32, 32, 0, pal);
            b.use_alpha = rnd(2) == 0;
            let few = rnd(2) == 0;
            for px in b.data.chunks_mut(4) {
                let c = if few { [(255u8, 255u8, 255u8), (0, 0, 0), (10, 200, 30)][rnd(3) as usize] } else { (rnd(256) as u8, rnd(256) as u8, rnd(256) as u8) };
                px[0] = c.0;
                px[1] = c.1;
                px[2] = c.2;
                px[3] = [0u8, 255, 255, 128, 1, 254][rnd(6) as usize];
            }
            b
        };
        src.trim_white_space = rnd(2) == 0;
        let mask = if rnd(3) == 0 {
            let (mw, mh) = (1 + rnd(70) as u16, 1 + rnd(60) as u16);
            let mut m = BitmapMask::new(mw, mh, true);
            for y in 0..mh {
                for x in 0..mw {
                    if rnd(3) == 0 {
                        m.set_bit(x, y, false);
                    }
                }
            }
            Some(m)
        } else {
            None
        };
        let (sl, st) = (rnd(sw as u32 + 6) as i32 - 3, rnd(sh as u32 + 6) as i32 - 3);
        let (rw, rh) = (1 + rnd(sw as u32 + 4) as i32, 1 + rnd(sh as u32 + 4) as i32);
        let src_rect = IntRect::from_tuple((sl, st, sl + rw, st + rh));
        let (dl, dt) = (rnd(dw as u32 + 40) as i32 - 20, rnd(dh as u32 + 40) as i32 - 20);
        let scaled = rnd(3) == 0;
        let (ow, oh) = if scaled { (1 + rnd(80) as i32, 1 + rnd(60) as i32) } else { (rw, rh) };
        let (mut l, mut r, mut tp, mut bt) = (dl, dl + ow, dt, dt + oh);
        if rnd(5) == 0 {
            std::mem::swap(&mut l, &mut r);
        }
        if rnd(5) == 0 {
            std::mem::swap(&mut tp, &mut bt);
        }
        let dst_rect = IntRect::from_tuple((l, tp, r, bt));
        let mut params = CopyPixelsParams::default(&src);
        params.ink = inks[rnd(inks.len() as u32) as usize];
        params.blend = [100, 100, 100, 50, 0, 85][rnd(6) as usize];
        params.bg_color = if rnd(2) == 0 { ColorRef::PaletteIndex([0u8, 255, 3, 17][rnd(4) as usize]) } else { [ColorRef::Rgb(255, 255, 255), ColorRef::Rgb(0, 0, 0), ColorRef::Rgb(10, 200, 30)][rnd(3) as usize].clone() };
        params.color = if rnd(2) == 0 { ColorRef::PaletteIndex(rnd(256) as u8) } else { ColorRef::Rgb(rnd(256) as u8, 0, 0) };
        params.bg_color_explicit = rnd(10) == 0;
        params.mask_image = mask.as_ref();
        params.mask_offset = (rnd(9) as i32 - 4, rnd(9) as i32 - 4);
        params.floor_rule = rnd(2) == 0;
        if rnd(6) == 0 {
            params.original_dst_rect = Some(IntRect::from_tuple((dl, dt, dl + 1 + rnd(80) as i32, dt + 1 + rnd(60) as i32)));
        }
        if rnd(8) == 0 {
            params.rotation = [90.0, 45.0, 180.0, 13.0][rnd(4) as usize];
        }
        let mut a = dst.clone();
        let before = draw_stats::FAST_BLITS.load(Relaxed);
        draw_stats::FAST_PATHS.store(true, Relaxed);
        a.copy_pixels_with_params(&palettes, &src, dst_rect.clone(), src_rect.clone(), &params);
        let took = draw_stats::FAST_BLITS.load(Relaxed) > before;
        let mut b = dst.clone();
        draw_stats::FAST_PATHS.store(false, Relaxed);
        b.copy_pixels_with_params(&palettes, &src, dst_rect, src_rect, &params);
        draw_stats::FAST_PATHS.store(true, Relaxed);
        taken += took as u32;
        if a.data != b.data {
            failed += 1;
            if failed <= 10 {
                let k = a.data.iter().zip(b.data.iter()).position(|(x, y)| x != y).unwrap_or(0) / 4;
                println!(
                    "[selftest] #{t} differs (fast path {took}): {} src {}/{} alpha {} trim {} ink {} blend {} mask {} src_rect {:?} dst_rect {:?} dst {}x{} alpha {} orig_rect {:?} rot {}: first at ({}, {}) fast {:?} loop {:?}",
                    if indexed { "indexed" } else { "rgba" }, src.bit_depth, src.original_bit_depth, src.use_alpha, src.trim_white_space,
                    params.ink, params.blend, mask.is_some(), (sl, st, rw, rh), (l, tp, r, bt), dw, dh, dst.use_alpha, params.original_dst_rect.as_ref().map(|r| (r.left, r.top, r.right, r.bottom)), params.rotation,
                    k % dw as usize, k / dw as usize, &a.data[k * 4..k * 4 + 4], &b.data[k * 4..k * 4 + 4]
                );
            }
        }
    }
    println!("[selftest] {n} blits, {taken} through the fast path, {failed} differ");
    failed == 0
}

fn main() {
    {
        let args: Vec<String> = std::env::args().collect();
        if args.get(1).map(String::as_str) == Some("--blit-selftest") {
            let n = args.get(2).and_then(|s| s.parse().ok()).unwrap_or(20_000);
            std::process::exit(if blit_selftest(n) { 0 } else { 1 });
        }
    }
    if std::env::var_os("DCR_PROBE_BENCH").is_some() {
        println!("[probe] {}", bench());
    }
    let args: Vec<String> = std::env::args().collect();
    let mut every = 2.0f64;
    let mut pos: Vec<String> = Vec::new();
    let mut i = 1;
    while i < args.len() {
        if args[i] == "--every" {
            every = args.get(i + 1).and_then(|s| s.parse().ok()).unwrap_or(every).max(0.2);
            i += 2;
            continue;
        }
        pos.push(args[i].clone());
        i += 1;
    }
    if pos.len() < 2 {
        eprintln!("usage: dcr-probe <movie.dir|.dcr> <out folder> [seconds] [script] [--every S]");
        std::process::exit(2);
    }
    let movie: PathBuf = std::fs::canonicalize(&pos[0]).unwrap_or_else(|e| {
        eprintln!("{}: {e}", pos[0]);
        std::process::exit(2)
    });
    let out = PathBuf::from(&pos[1]);
    let seconds: f64 = pos.get(2).and_then(|s| s.parse().ok()).unwrap_or(90.0);
    let script = parse_script(pos.get(3).map(String::as_str).unwrap_or(""));
    std::fs::create_dir_all(&out).expect("output folder");

    let panic_file = out.join("panic.txt");
    let default_hook = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        let _ = std::fs::write(&panic_file, format!("{info}\n"));
        default_hook(info);
    }));

    let profiling = std::env::var_os("DCR_PROBE_PROF").is_some();
    if profiling {
        prof::start();
    }
    let prof_out = out.join("prof.txt");
    println!("[probe] movie {} for {seconds} s, a picture every {every} s, {} scripted inputs", movie.display(), script.len());
    // text in system fonts: the Liberation fonts, as Ruffle5 gives them (DCR_PROBE_FONTS: their folder)
    let fonts_dir = std::env::var("DCR_PROBE_FONTS").unwrap_or_else(|_| "/usr/share/fonts/truetype/liberation".into());
    let fonts = vm_rust::player::native_text::add_liberation_fonts(&fonts_dir);
    println!("[probe] {fonts} fonts for system-font text from {fonts_dir}");
    run_test(async move {
        let mut player = TestPlayer::new();
        // the system font, as NativePlayer::new loads it (DCR_PROBE_NO_SYSTEM_FONT=1: without it)
        if std::env::var_os("DCR_PROBE_NO_SYSTEM_FONT").is_none() {
            if let Err(e) = vm_rust::player::reserve_player_mut(|p| vm_rust::player::font::native_load_system_font(p)) {
                println!("[probe] system font: {e}");
            }
        }
        // as Ruffle5 starts a game the Online tab fetched (src/shockwave.rs): its launch command,
        // the address it was published at, the folder its web addresses are read from
        if let Ok(cmd) = std::env::var("DCR_PROBE_LAUNCH") {
            let lc = vm_rust::player::native::parse_launch_command(&cmd);
            println!("[probe] launch command: {lc:?}");
            vm_rust::player::native::apply_launch_command(&lc);
        }
        if let Ok(address) = std::env::var("DCR_PROBE_ADDRESS") {
            println!("[probe] movie address {address}");
            vm_rust::player::native::set_movie_address(&address);
        }
        if let Ok(root) = std::env::var("DCR_PROBE_WEBROOT") {
            println!("[probe] web addresses read from {root}");
            vm_rust::player::native::set_web_root(&root);
        }
        vm_rust::player::native::run_launch_do_before().await;
        let t0 = Instant::now();
        player.load_movie(movie.to_str().expect("UTF-8 path")).await;
        println!("[probe] loaded in {:.2} s", t0.elapsed().as_secs_f64());
        player.init_movie().await;
        println!("[probe] init done at {:.2} s, frame {}", t0.elapsed().as_secs_f64(), player.current_frame());
        save(&player, &out, "t0000");

        let mut next_pic = every;
        let (mut last_frame, mut same_since, mut dumped_stall) = (0u32, 0.0f64, false);
        let (mut mixed_to, mut mix_buf, mut peak) = (t0.elapsed().as_secs_f64(), Vec::<f32>::new(), 0.0f32);
        let mut next_status = 1.0;
        let mut steps = 0u64;
        let fast = std::env::var_os("DCR_PROBE_FAST").is_some();
        let compare = std::env::var("DCR_PROBE_COMPARE").ok();
        let (mut cmp_frames, mut cmp_bad_frames, mut cmp_bad_px, mut shown_diffs) = (0u64, 0u64, 0usize, 0u32);
        let (mut step_ms, mut draw_ms, mut draws, mut window_steps) = (0.0f64, 0.0f64, 0u64, 0u64);
        let (mut step_cpu, mut draw_cpu) = (0.0f64, 0.0f64);
        let (mut next_frame_at, mut status_since) = (Instant::now(), 0.0f64);
        let (mut step_alloc, mut draw_alloc) = ((0u64, 0u64, 0u64), (0u64, 0u64, 0u64));
        let (mut step_clock, mut draw_clock) = (0u64, 0u64);
        let mut todo = script.into_iter().peekable();
        loop {
            let now = t0.elapsed().as_secs_f64();
            if now >= seconds {
                break;
            }
            while todo.peek().is_some_and(|(at, _)| *at <= now) {
                let (at, act) = todo.next().unwrap();
                println!("[probe] {at:>6.1} s: {act:?}");
                match act {
                    Act::Click(x, y) => player.click(x, y).await,
                    Act::Move(x, y) => player.mouse_move(x, y).await,
                    Act::Key(k) | Act::Down(k) | Act::Up(k) if js_key(&k).is_none() => {
                        eprintln!("[probe] unknown key {k}")
                    }
                    Act::Key(k) => {
                        let (key, code) = js_key(&k).unwrap();
                        player.key_down(&key, code).await;
                        vm_rust::player::native::run_native_frame().await;
                        steps += 1;
                        player.key_up(&key, code).await;
                    }
                    Act::Down(k) => {
                        let (key, code) = js_key(&k).unwrap();
                        player.key_down(&key, code).await;
                    }
                    Act::Up(k) => {
                        let (key, code) = js_key(&k).unwrap();
                        player.key_up(&key, code).await;
                    }
                    Act::Snap(name) => save(&player, &out, &name),
                    Act::Eval(expr) => show(&player, &expr).await,
                    Act::Lingo => dump_lingo(&out.join("lingo")),
                }
            }
            // ---- the next score frame once it is due, as the console's loop runs it
            // (src/shockwave.rs, Director's model): one frame, then the stage rendered; a frame
            // starts one tempo period after the last, or at once if that has passed (no burst to
            // catch up); then the rest of a 60 Hz display refresh, as the console's present waits.
            // DCR_PROBE_FAST=1: no waiting for the refresh.
            let loop_start = Instant::now();
            let tempo = match vm_rust::player::reserve_player_ref(|p| p.movie.get_effective_tempo()) {
                0 => 30,
                t => t.min(120),
            };
            let period = std::time::Duration::from_secs_f64(1.0 / tempo as f64);
            let mut n = 0;
            let mut playing = true;
            if steps == 0 || loop_start >= next_frame_at {
                n = 1;
                let t_step = Instant::now();
                let cpu0 = thread_cpu_ms();
                let a0 = alloc_counts();
                let c0 = clock_calls();
                playing = vm_rust::player::native::run_native_frame().await;
                step_clock += clock_calls() - c0;
                let a1 = alloc_counts();
                step_ms += t_step.elapsed().as_secs_f64() * 1000.0;
                step_cpu += thread_cpu_ms() - cpu0;
                step_alloc = (step_alloc.0 + a1.0 - a0.0, step_alloc.1 + a1.1 - a0.1, step_alloc.2 + a1.2 - a0.2);
                steps += 1;
                next_frame_at = (next_frame_at + period).max(Instant::now());
            }
            if n > 0 {
                let t_draw = Instant::now();
                let cpu0 = thread_cpu_ms();
                let a1 = alloc_counts();
                let c1 = clock_calls();
                prof::PROF_ON.store(true, std::sync::atomic::Ordering::Relaxed);
                let snap = player.snapshot_stage();
                prof::PROF_ON.store(false, std::sync::atomic::Ordering::Relaxed);
                // DCR_PROBE_COMPARE=1: the same stage again without DirPlayer's upright blit fast
                // path, compared byte for byte (=2: with it, to check the render is repeatable)
                if let Some(mode) = compare.as_deref() {
                    use std::sync::atomic::Ordering::Relaxed;
                    use vm_rust::player::bitmap::drawing::draw_stats::FAST_PATHS;
                    FAST_PATHS.store(mode == "2", Relaxed);
                    let again = player.snapshot_stage();
                    FAST_PATHS.store(true, Relaxed);
                    use vm_rust::player::testing_shared::SnapshotOutput;
                    if let (SnapshotOutput::Rgba { data: a, width, .. }, SnapshotOutput::Rgba { data: b, .. }) = (&snap, &again) {
                        cmp_frames += 1;
                        let mut bad = 0usize;
                        for (k, (x, y)) in a.chunks(4).zip(b.chunks(4)).enumerate() {
                            if x != y {
                                if bad == 0 && shown_diffs < 8 {
                                    shown_diffs += 1;
                                    println!("[compare] frame {}: first difference at ({}, {}): fast {:?}, loop {:?}", player.current_frame(), k % *width as usize, k / *width as usize, x, y);
                                }
                                bad += 1;
                            }
                        }
                        if bad > 0 || a.len() != b.len() {
                            cmp_bad_frames += 1;
                            cmp_bad_px += bad;
                        }
                    }
                }
                draw_clock += clock_calls() - c1;
                let a2 = alloc_counts();
                draw_ms += t_draw.elapsed().as_secs_f64() * 1000.0;
                draw_cpu += thread_cpu_ms() - cpu0;
                draw_alloc = (draw_alloc.0 + a2.0 - a1.0, draw_alloc.1 + a2.1 - a1.1, draw_alloc.2 + a2.2 - a1.2);
                draws += 1;
            }
            // the speaker the console would have: mix what has played since the last loop
            let now_s = t0.elapsed().as_secs_f64();
            let frames = ((now_s - mixed_to) * 48_000.0) as usize;
            if frames > 0 {
                mix_buf.resize(frames * 2, 0.0);
                mix_buf.iter_mut().for_each(|x| *x = 0.0);
                vm_rust::player::native_audio::mix(&mut mix_buf, 48_000);
                peak = mix_buf.iter().fold(peak, |m, x| m.max(x.abs()));
                mixed_to += frames as f64 / 48_000.0;
            }
            if !fast {
                let spent = loop_start.elapsed().as_secs_f64();
                if spent < 1.0 / 60.0 {
                    std::thread::sleep(std::time::Duration::from_secs_f64(1.0 / 60.0 - spent));
                }
            }
            let now = t0.elapsed().as_secs_f64();
            if now >= next_status {
                let window = now - status_since;
                let fr = (steps - window_steps).max(1) as f64;
                println!(
                    "[probe] {now:>6.1} s: frame {}, tempo {tempo}, playing {playing}, {:.1} frames/s, {:.1} draws/s, {} voices, peak {peak:.2}; ms per frame: lingo {:.1} (cpu {:.1}), draw {:.1} (cpu {:.1}); allocs per frame: lingo {} ({} KB, {} >=64K), draw {} ({} KB, {} >=64K)",
                    player.current_frame(),
                    (steps - window_steps) as f64 / window,
                    draws as f64 / window,
                    vm_rust::player::native_audio::playing(),
                    step_ms / fr,
                    step_cpu / fr,
                    draw_ms / draws.max(1) as f64,
                    draw_cpu / draws.max(1) as f64,
                    step_alloc.0 / fr as u64,
                    step_alloc.1 / 1024 / fr as u64,
                    step_alloc.2 / fr as u64,
                    draw_alloc.0 / draws.max(1),
                    draw_alloc.1 / 1024 / draws.max(1),
                    draw_alloc.2 / draws.max(1)
                );
                if clock_calls() > 0 {
                    println!(
                        "[probe]          clock reads per frame: lingo {}, draw {}",
                        step_clock / fr as u64,
                        draw_clock / draws.max(1)
                    );
                    (step_clock, draw_clock) = (0, 0);
                }
                let (copies, copy_ms, copy_mpx, mattes, matte_ms, matte_mpx) =
                    vm_rust::player::bitmap::drawing::draw_stats::take();
                let per = |v: f64| v / draws.max(1) as f64;
                let fast_blits = vm_rust::player::bitmap::drawing::draw_stats::FAST_BLITS.swap(0, std::sync::atomic::Ordering::Relaxed);
                println!(
                    "[probe]          draw per frame: {:.0} blits ({:.0} fast) ({:.2} Mpx) {:.1} ms, of which {:.0} matte masks ({:.2} Mpx) {:.1} ms",
                    per(copies as f64), per(fast_blits as f64), per(copy_mpx), per(copy_ms), per(mattes as f64), per(matte_mpx), per(matte_ms)
                );
                (step_ms, step_cpu, draw_ms, draw_cpu, draws, window_steps) = (0.0, 0.0, 0.0, 0.0, 0, steps);
                (step_alloc, draw_alloc) = ((0, 0, 0), (0, 0, 0));
                peak = 0.0;
                next_status += 1.0;
                status_since = now;
            }
            if now >= next_pic {
                save(&player, &out, &format!("t{:04}", now as u32));
                next_pic += every;
            }
            let frame = player.current_frame();
            if frame != last_frame {
                (last_frame, same_since, dumped_stall) = (frame, now, false);
            } else if now - same_since >= 10.0 && !dumped_stall {
                dumped_stall = true;
                println!("[probe] the score has stayed on frame {frame} for 10 s");
                dump(&player).await;
            }
            if !playing {
                println!("[probe] the movie stopped playing at {now:.1} s");
                save(&player, &out, "stopped");
                break;
            }
        }
        dump(&player).await;
        println!("[probe] done: {steps} frames in {:.1} s", t0.elapsed().as_secs_f64());
        if compare.is_some() {
            println!("[compare] {cmp_frames} stages compared, {cmp_bad_frames} differ ({cmp_bad_px} pixels)");
        }
        if profiling {
            prof::dump(&prof_out);
        }

    });
}
