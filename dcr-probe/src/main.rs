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
//! Sound goes nowhere, but it is mixed as the console would (`native_audio::mix`, in step with the
//! clock), so a sound's end reaches the movie as it would there; the status line counts the voices.
//!
//! DCR_PROBE_BENCH=1 first prints the CPU / clock / memory benchmark Ruffle5 logs on the console.
//!
//! The status line also gives the milliseconds a score frame takes (lingo) and, with
//! DCR_PROBE_RENDER=1 set, what rendering the stage after every frame takes (draw), as on the console.
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
        std::alloc::System.alloc(l)
    }
    unsafe fn dealloc(&self, p: *mut u8, l: std::alloc::Layout) {
        std::alloc::System.dealloc(p, l)
    }
    unsafe fn realloc(&self, p: *mut u8, l: std::alloc::Layout, n: usize) -> *mut u8 {
        ALLOCS.fetch_add(1, Relaxed);
        ALLOC_BYTES.fetch_add(n as u64, Relaxed);
        if n >= 65536 {
            BIG_ALLOCS.fetch_add(1, Relaxed);
        }
        std::alloc::System.realloc(p, l, n)
    }
}
#[global_allocator]
static GLOBAL: Counting = Counting;
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

fn main() {
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

    println!("[probe] movie {} for {seconds} s, a picture every {every} s, {} scripted inputs", movie.display(), script.len());
    run_test(async move {
        let mut player = TestPlayer::new();
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
        let render_each = std::env::var_os("DCR_PROBE_RENDER").is_some();
        let (mut step_ms, mut draw_ms, mut draws, mut window_steps) = (0.0f64, 0.0f64, 0u64, 0u64);
        let (mut step_alloc, mut draw_alloc) = ((0u64, 0u64, 0u64), (0u64, 0u64, 0u64));
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
                        player.step_frame().await;
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
            let t_step = Instant::now();
            let a0 = alloc_counts();
            let playing = player.step_frame().await;
            let a1 = alloc_counts();
            step_ms += t_step.elapsed().as_secs_f64() * 1000.0;
            step_alloc = (step_alloc.0 + a1.0 - a0.0, step_alloc.1 + a1.1 - a0.1, step_alloc.2 + a1.2 - a0.2);
            steps += 1;
            // DCR_PROBE_RENDER=1: render the stage after every frame, as the console does, and time it
            if render_each {
                let t_draw = Instant::now();
                let _ = player.snapshot_stage();
                let a2 = alloc_counts();
                draw_ms += t_draw.elapsed().as_secs_f64() * 1000.0;
                draw_alloc = (draw_alloc.0 + a2.0 - a1.0, draw_alloc.1 + a2.1 - a1.1, draw_alloc.2 + a2.2 - a1.2);
                draws += 1;
            }
            // the speaker the console would have: mix what has played since the last frame
            let now_s = t0.elapsed().as_secs_f64();
            let frames = ((now_s - mixed_to) * 48_000.0) as usize;
            if frames > 0 {
                mix_buf.resize(frames * 2, 0.0);
                mix_buf.iter_mut().for_each(|x| *x = 0.0);
                vm_rust::player::native_audio::mix(&mut mix_buf, 48_000);
                peak = mix_buf.iter().fold(peak, |m, x| m.max(x.abs()));
                mixed_to += frames as f64 / 48_000.0;
            }
            let now = t0.elapsed().as_secs_f64();
            if now >= next_status {
                println!(
                    "[probe] {now:>6.1} s: frame {}, playing {playing}, {steps} steps, {} voices, peak {peak:.2}; ms per frame: lingo {:.1}, draw {:.1}; allocs per frame: lingo {} ({} KB, {} >=64K), draw {} ({} KB, {} >=64K)",
                    player.current_frame(),
                    vm_rust::player::native_audio::playing(),
                    step_ms / (steps - window_steps).max(1) as f64,
                    draw_ms / draws.max(1) as f64,
                    step_alloc.0 / (steps - window_steps).max(1),
                    step_alloc.1 / 1024 / (steps - window_steps).max(1),
                    step_alloc.2 / (steps - window_steps).max(1),
                    draw_alloc.0 / draws.max(1),
                    draw_alloc.1 / 1024 / draws.max(1),
                    draw_alloc.2 / draws.max(1)
                );
                let (copies, copy_ms, copy_mpx, mattes, matte_ms, matte_mpx) =
                    vm_rust::player::bitmap::drawing::draw_stats::take();
                let per = |v: f64| v / draws.max(1) as f64;
                println!(
                    "[probe]          draw per frame: {:.0} blits ({:.2} Mpx) {:.1} ms, of which {:.0} matte masks ({:.2} Mpx) {:.1} ms",
                    per(copies as f64), per(copy_mpx), per(copy_ms), per(mattes as f64), per(matte_mpx), per(matte_ms)
                );
                (step_ms, draw_ms, draws, window_steps) = (0.0, 0.0, 0, steps);
                (step_alloc, draw_alloc) = ((0, 0, 0), (0, 0, 0));
                peak = 0.0;
                next_status += 1.0;
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
    });
}
