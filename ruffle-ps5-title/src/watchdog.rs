//! PS5_RUFFLE_WATCHDOG_V1 — says in ruffle.log which step a game's loop is in when one takes long.
//!
//! A game runs on the title's main thread, one step after another: the movie's own frame (its
//! scripts), the player's background jobs, drawing, showing the frame. When a step does not come
//! back, the screen keeps its last picture and the quick menu cannot open, since the quick menu is
//! that same loop; until 1.000.032 the log then simply stopped. Now the loop names its step as it
//! enters it (`Watchdog::step`), and:
//!   * a second thread looks twice a second and writes a line when a step has been running for
//!     5 s, then at 15, 30 and 60 s, then every minute ("still ... after 30 s");
//!   * the loop itself writes how long a step took when one that took 5 s or more ends.
//! Nothing else is done: the watchdog cannot stop a step, it only says which one it is.
// SPDX-License-Identifier: GPL-3.0-or-later

use std::sync::atomic::{AtomicBool, AtomicU64, AtomicU8, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

/// The steps of a game's loop.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u8)]
pub enum Step {
    /// Between steps: input, the menus over the game, the overlay.
    Loop = 0,
    /// Reading and parsing the movie (the loading screen).
    Loading = 1,
    /// Making the player and starting the movie.
    Starting = 2,
    /// The movie's own frame: its scripts and timeline (Ruffle's tick, a Director score frame).
    Frame = 3,
    /// The player's background jobs (files the movie asked for).
    Jobs = 4,
    /// Drawing the frame.
    Drawing = 5,
    /// Showing it.
    Showing = 6,
    /// Writing the game's saves.
    Saving = 7,
}

impl Step {
    const ALL: [Step; 8] = [Step::Loop, Step::Loading, Step::Starting, Step::Frame, Step::Jobs, Step::Drawing, Step::Showing, Step::Saving];

    fn from_u8(v: u8) -> Step {
        Step::ALL.get(v as usize).copied().unwrap_or(Step::Loop)
    }

    pub fn words(self) -> &'static str {
        match self {
            Step::Loop => "the loop between steps (input, menus)",
            Step::Loading => "reading the movie",
            Step::Starting => "starting the player",
            Step::Frame => "the movie's frame (its scripts and timeline)",
            Step::Jobs => "the player's background jobs (files the movie asked for)",
            Step::Drawing => "drawing the frame",
            Step::Showing => "showing the frame",
            Step::Saving => "writing the game's saves",
        }
    }
}

/// A step that has run this long is written to the log; then at the later marks, then every minute.
const MARKS: [u64; 4] = [5, 15, 30, 60];

/// The next time (seconds into a step) to say it is still going, after `said` such lines.
fn mark(said: usize) -> u64 {
    MARKS.get(said).copied().unwrap_or_else(|| 60 * (said as u64 + 2 - MARKS.len() as u64))
}

struct Shared {
    step: AtomicU8,
    /// When the step began, ms after `t0`.
    since: AtomicU64,
    /// Counts step changes (a step entered again counts from then).
    seq: AtomicU64,
    stop: AtomicBool,
}

pub struct Watchdog {
    shared: Arc<Shared>,
    t0: Instant,
    game: String,
    thread: Option<std::thread::JoinHandle<()>>,
}

impl Watchdog {
    /// Watches the loop of `game` (its name, for the log) from now, in the `Loop` step.
    pub fn start(game: &str) -> Watchdog {
        let shared = Arc::new(Shared { step: AtomicU8::new(Step::Loop as u8), since: AtomicU64::new(0), seq: AtomicU64::new(0), stop: AtomicBool::new(false) });
        let t0 = Instant::now();
        let (s, name) = (Arc::clone(&shared), game.to_string());
        let thread = std::thread::Builder::new().name("watchdog".into()).spawn(move || watch(&s, t0, &name));
        let thread = match thread {
            Ok(t) => Some(t),
            Err(e) => {
                crate::say(&format!("warn watchdog: no thread ({e}); long steps are only logged when they end"));
                None
            }
        };
        Watchdog { shared, t0, game: game.to_string(), thread }
    }

    /// The loop enters `step`. When the step it leaves took 5 s or more, that goes in the log.
    pub fn step(&self, step: Step) {
        let now = self.t0.elapsed().as_millis() as u64;
        let began = self.shared.since.swap(now, Ordering::Relaxed);
        let left = Step::from_u8(self.shared.step.swap(step as u8, Ordering::Relaxed));
        self.shared.seq.fetch_add(1, Ordering::Release);
        let took = now.saturating_sub(began);
        if took >= MARKS[0] * 1000 {
            crate::say(&format!("watchdog: {}: {} took {:.1} s", self.game, left.words(), took as f64 / 1000.0));
        }
    }
}

impl Drop for Watchdog {
    fn drop(&mut self) {
        self.shared.stop.store(true, Ordering::Relaxed);
        if let Some(t) = self.thread.take() {
            t.thread().unpark();
            let _ = t.join();
        }
    }
}

fn watch(s: &Shared, t0: Instant, game: &str) {
    // the step being watched (its seq) and how many "still" lines were written about it
    let mut seen = u64::MAX;
    let mut said = 0usize;
    while !s.stop.load(Ordering::Relaxed) {
        std::thread::park_timeout(Duration::from_millis(500));
        let seq = s.seq.load(Ordering::Acquire);
        if seq != seen {
            seen = seq;
            said = 0;
        }
        let step = Step::from_u8(s.step.load(Ordering::Relaxed));
        let since = s.since.load(Ordering::Relaxed);
        // the step may have changed between the loads: then the next look sees the new one
        if s.seq.load(Ordering::Acquire) != seq {
            continue;
        }
        let secs = (t0.elapsed().as_millis() as u64).saturating_sub(since) / 1000;
        if secs >= mark(said) {
            crate::say(&format!("watchdog: {game}: still in {} after {secs} s", step.words()));
            said += 1;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn marks_then_every_minute() {
        assert_eq!((0..7).map(mark).collect::<Vec<_>>(), vec![5, 15, 30, 60, 120, 180, 240]);
        for s in Step::ALL {
            assert_eq!(Step::from_u8(s as u8), s);
            assert!(!s.words().is_empty());
        }
    }

    #[test]
    fn steps_are_tracked_and_the_thread_stops() {
        let w = Watchdog::start("Test");
        w.step(Step::Frame);
        assert_eq!(Step::from_u8(w.shared.step.load(Ordering::Relaxed)), Step::Frame);
        let seq = w.shared.seq.load(Ordering::Relaxed);
        w.step(Step::Drawing);
        assert_eq!(w.shared.seq.load(Ordering::Relaxed), seq + 1);
        drop(w); // joins the thread
    }
}
