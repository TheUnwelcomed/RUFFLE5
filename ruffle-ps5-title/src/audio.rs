//! PS5_RUFFLE_AUDIO_V5 — Ruffle's mixer on the console's speaker.
//!
//! Ruffle mixes every sound into whatever buffer the platform asks for; this backend asks on a
//! thread that feeds AudioOut grains of 48 kHz s16 stereo (shell/pad.c: the port PS5_RetroArch's
//! audio_ps5.cpp opens the same way). `sceAudioOutOutput` blocks until the grain is queued, so the
//! thread paces itself on the hardware. Pause keeps the port fed with silence.
//!
//! V2: the grain is 1024 frames (21 ms) instead of 256 (5 ms), falling back to 512 and 256 if the
//! port refuses it: a grain is also the mixer's deadline, and a 5 ms one leaves no room for an
//! MP3 frame to decode on a thread competing with the renderer. And the thread keeps score —
//! every 10 s it logs how many grains it mixed, the longest mix, how many mixes overran the grain
//! and how many outputs the port refused — so the next ruffle.log says whether sound starved.
//!
//! V3: the mixer mixes in f32 and this thread converts to s16 with a clamp. Mixed straight into
//! i16 (V1/V2), Ruffle sums the playing sounds with dasp's `Sample::add_amp`, which for i16 is a
//! plain `+` — and in a release build that wraps on overflow: a loud effect over the music turned
//! into full-scale noise for as long as the sum exceeded 16 bits (dad 'n me, 2026-10-21: "music
//! sounds great, some sound effects sound like garbled noise"). Ruffle's desktop and web
//! frontends mix in f32 and let the device clip, which is what Flash did. The score line also
//! counts samples that had to be clamped.
//!
//! V5: the mixing thread pins itself to the core src/cpu.rs keeps for sound.
//!
//! V4: a backend ends with its player (one per game, src/lib.rs): dropping it stops the thread,
//! joins it (at most one grain) and closes the port, so the next game's `sceAudioOutOpen` finds
//! the main output free.
// SPDX-License-Identifier: GPL-3.0-or-later

use ruffle_core::backend::audio::{
    swf, AudioBackend, AudioMixer, DecodeError, RegisterError, SoundHandle, SoundInstanceHandle,
    SoundStreamInfo, SoundTransform,
};
use ruffle_core::impl_audio_mixer_backend;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

extern "C" {
    fn ps5_audio_open(frames: u32, detail: *mut i32) -> i32;
    fn ps5_audio_output(port: i32, frames: *const i16) -> i32;
    fn ps5_audio_close(port: i32) -> i32;
}

const RATE: u32 = 48_000;
/// Grain sizes to try, in frames (AudioOut wants 256..2048, a multiple of 64).
const GRAINS: [u32; 3] = [1024, 512, 256];
const REPORT_EVERY: Duration = Duration::from_secs(10);

pub struct Ps5AudioBackend {
    mixer: AudioMixer,
    paused: Arc<AtomicBool>,
    stop: Arc<AtomicBool>,
    thread: Option<std::thread::JoinHandle<()>>,
    pub grain: u32,
}

impl Ps5AudioBackend {
    /// `log` gets the thread's 10-second score lines.
    pub fn new(log: fn(&str)) -> Result<Self, String> {
        let mut detail = 0i32;
        let mut port = -1;
        let mut grain = 0u32;
        let mut tried = Vec::new();
        for g in GRAINS {
            port = unsafe { ps5_audio_open(g, &mut detail) };
            if port >= 0 {
                grain = g;
                break;
            }
            tried.push(format!("{g} frames: step {port}, 0x{:08x}", detail as u32));
        }
        if port < 0 {
            return Err(format!("AudioOut refused every grain ({})", tried.join("; ")));
        }
        for t in &tried {
            log(&format!("warn AudioOut refused {t}"));
        }
        let mixer = AudioMixer::new(2, RATE);
        let proxy = mixer.proxy();
        let paused = Arc::new(AtomicBool::new(false));
        let paused_t = Arc::clone(&paused);
        let stop = Arc::new(AtomicBool::new(false));
        let stop_t = Arc::clone(&stop);
        let frames = grain as usize;
        let budget = Duration::from_micros(frames as u64 * 1_000_000 / RATE as u64);
        let thread = std::thread::Builder::new()
            .name("audio".into())
            .spawn(move || {
                // V5: the sound on a CPU core of its own (src/cpu.rs), so a busy game cannot make it
                // miss its deadline.
                if let Some(warn) = crate::cpu::pin_audio_thread() {
                    log(&warn);
                }
                let mut mixed = vec![0f32; frames * 2];
                let mut buf = vec![0i16; frames * 2];
                let mut grains = 0u64;
                let mut overruns = 0u64;
                let mut refused = 0u64;
                let mut clipped = 0u64;
                let mut max_mix = Duration::ZERO;
                let mut since = Instant::now();
                while !stop_t.load(Ordering::Relaxed) {
                    let t0 = Instant::now();
                    if paused_t.load(Ordering::Relaxed) {
                        buf.fill(0);
                    } else {
                        proxy.mix::<f32>(&mut mixed);
                        for (out, &s) in buf.iter_mut().zip(mixed.iter()) {
                            let c = s.clamp(-1.0, 1.0);
                            if c != s {
                                clipped += 1;
                            }
                            *out = (c * 32767.0) as i16;
                        }
                    }
                    let took = t0.elapsed();
                    max_mix = max_mix.max(took);
                    if took > budget {
                        overruns += 1;
                    }
                    if unsafe { ps5_audio_output(port, buf.as_ptr()) } < 0 {
                        refused += 1;
                        std::thread::sleep(Duration::from_millis(5));
                    }
                    grains += 1;
                    if since.elapsed() >= REPORT_EVERY {
                        log(&format!(
                            "audio: {grains} grains of {frames} in {:.1} s; longest mix {:.2} ms (budget {:.2} ms), {overruns} over budget, {refused} refused, {clipped} samples clamped",
                            since.elapsed().as_secs_f64(),
                            max_mix.as_secs_f64() * 1000.0,
                            budget.as_secs_f64() * 1000.0
                        ));
                        grains = 0;
                        overruns = 0;
                        refused = 0;
                        clipped = 0;
                        max_mix = Duration::ZERO;
                        since = Instant::now();
                    }
                }
                let rc = unsafe { ps5_audio_close(port) };
                log(&format!("audio: port {port} closed ({rc:#x})"));
            })
            .map_err(|e| format!("audio thread: {e}"))?;
        Ok(Self {
            mixer,
            paused,
            stop,
            thread: Some(thread),
            grain,
        })
    }
}

impl Drop for Ps5AudioBackend {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
        if let Some(t) = self.thread.take() {
            let _ = t.join();
        }
    }
}

impl AudioBackend for Ps5AudioBackend {
    impl_audio_mixer_backend!(mixer);

    fn play(&mut self) {
        self.paused.store(false, Ordering::Relaxed);
    }

    fn pause(&mut self) {
        self.paused.store(true, Ordering::Relaxed);
    }
}
