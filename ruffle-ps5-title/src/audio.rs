//! PS5_RUFFLE_AUDIO_V1 — Ruffle's mixer on the console's speaker.
//!
//! Ruffle mixes every sound into whatever buffer the platform asks for; this backend asks on a
//! thread that feeds AudioOut 256-frame grains of 48 kHz s16 stereo (shell/pad.c: the port
//! PS5_RetroArch's audio_ps5.cpp opens the same way). `sceAudioOutOutput` blocks until the grain is
//! queued, so the thread paces itself on the hardware. Pause keeps the port fed with silence.
// SPDX-License-Identifier: GPL-3.0-or-later

use ruffle_core::backend::audio::{
    swf, AudioBackend, AudioMixer, DecodeError, RegisterError, SoundHandle, SoundInstanceHandle,
    SoundStreamInfo, SoundTransform,
};
use ruffle_core::impl_audio_mixer_backend;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

extern "C" {
    fn ps5_audio_open(detail: *mut i32) -> i32;
    fn ps5_audio_output(port: i32, frames: *const i16) -> i32;
}

const RATE: u32 = 48_000;
const GRAIN: usize = 256;

pub struct Ps5AudioBackend {
    mixer: AudioMixer,
    paused: Arc<AtomicBool>,
}

impl Ps5AudioBackend {
    pub fn new() -> Result<Self, String> {
        let mut detail = 0i32;
        let port = unsafe { ps5_audio_open(&mut detail) };
        if port < 0 {
            return Err(format!(
                "AudioOut: step {port} failed with 0x{:08x}",
                detail as u32
            ));
        }
        let mixer = AudioMixer::new(2, RATE);
        let proxy = mixer.proxy();
        let paused = Arc::new(AtomicBool::new(false));
        let paused_t = Arc::clone(&paused);
        std::thread::Builder::new()
            .name("audio".into())
            .spawn(move || {
                let mut buf = [0i16; GRAIN * 2];
                loop {
                    if paused_t.load(Ordering::Relaxed) {
                        buf.fill(0);
                    } else {
                        proxy.mix::<i16>(&mut buf);
                    }
                    if unsafe { ps5_audio_output(port, buf.as_ptr()) } < 0 {
                        std::thread::sleep(std::time::Duration::from_millis(5));
                    }
                }
            })
            .map_err(|e| format!("audio thread: {e}"))?;
        Ok(Self { mixer, paused })
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
