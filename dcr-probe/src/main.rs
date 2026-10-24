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
//! Keys: Enter Space Left Right Up Down Escape Shift Control Tab Backspace, a..z, 0..9.
// SPDX-License-Identifier: GPL-3.0-or-later

use std::path::{Path, PathBuf};
use std::time::Instant;

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

fn main() {
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
        let mut next_status = 1.0;
        let mut steps = 0u64;
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
                }
            }
            let playing = player.step_frame().await;
            steps += 1;
            let now = t0.elapsed().as_secs_f64();
            if now >= next_status {
                println!("[probe] {now:>6.1} s: frame {}, playing {playing}, {steps} steps", player.current_frame());
                next_status += 1.0;
            }
            if now >= next_pic {
                save(&player, &out, &format!("t{:04}", now as u32));
                next_pic += every;
            }
            if !playing {
                println!("[probe] the movie stopped playing at {now:.1} s");
                save(&player, &out, "stopped");
                break;
            }
        }
        println!("[probe] done: {steps} frames in {:.1} s", t0.elapsed().as_secs_f64());
    });
}
