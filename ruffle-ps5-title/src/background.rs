//! PS5_RUFFLE_BACKGROUND_V1 — the menu's background: the animation from ruffle.rs's front page,
//! played live by Ruffle itself at the display's resolution (3840x2160 on the PS5: Flash is vector
//! art, so it is drawn in 4K, not scaled up).
//!
//! The movie is ruffle.rs's `logo-anim.swf` (assets/, from ruffle-rs/ruffle-rs.github.io): the
//! "ruffle" letters drop in on Ruffle blue, then sit as springy clips that ruffle when a pointer
//! passes over them. On the site that pointer is your mouse; here an invisible one sweeps across the
//! letters every few seconds, and again whenever you move in the menu, so the background keeps
//! playing. It has no sound here (a null audio backend). The menu's screens are drawn over each
//! frame as the overlay (src/shell.rs), so the menu costs one Ruffle frame and one overlay pass.
//! The player is dropped while a game runs (its 4K buffers are freed) and made anew after.
// SPDX-License-Identifier: GPL-3.0-or-later

use crate::shell::Display;
use crate::ui::Scene;
use ruffle_core::backend::navigator::NullNavigatorBackend;
use ruffle_core::config::Letterbox;
use ruffle_core::events::PlayerEvent;
use ruffle_core::tag_utils::SwfMovieData;
use ruffle_core::{Player, PlayerBuilder, StageScaleMode};
use ruffle_render::quality::StageQuality;
use ruffle_render_wgpu::backend::WgpuRenderBackend;
use ruffle_render_wgpu::descriptors::Descriptors;
use std::sync::{Arc, Mutex};

/// ruffle.rs's front-page animation (400x220, 30 fps, ActionScript 2).
pub static LOGO_SWF: &[u8] = include_bytes!("../assets/logo-anim.swf");

/// Seconds between the idle sweeps, and how long one takes.
const SWEEP_EVERY: f64 = 7.0;
const SWEEP_TIME: f64 = 1.9;

pub struct Background {
    player: Arc<Mutex<Player>>,
    width: f64,
    height: f64,
    /// Since the last sweep began.
    since: f64,
    pointer: (f64, f64),
    pub frames: u64,
}

impl Background {
    pub fn new(display: &Arc<Display>, descriptors: &Arc<Descriptors>) -> Result<Background, String> {
        let movie = SwfMovieData::from_data(LOGO_SWF, "file:///app0/logo-anim.swf".to_string(), None, None).map_err(|e| format!("logo-anim.swf: {e}"))?;
        let (w, h) = (display.width, display.height);
        let renderer = WgpuRenderBackend::new(Arc::clone(descriptors), display.target(w, h)).map_err(|e| format!("background renderer: {e}"))?;
        let player = PlayerBuilder::new()
            .with_renderer(renderer)
            .with_navigator(NullNavigatorBackend::new())
            .with_movie(movie)
            .with_viewport_dimensions(w, h, w as f64 / crate::ui::W as f64)
            .with_letterbox(Letterbox::Off)
            .with_scale_mode(StageScaleMode::ShowAll, true)
            .with_quality(StageQuality::High)
            .with_autoplay(true)
            .build();
        Ok(Background {
            player,
            width: w as f64,
            height: h as f64,
            since: SWEEP_EVERY - 2.5,
            pointer: (-10.0, h as f64 * 0.9),
            frames: 0,
        })
    }

    /// Sweep across the letters now (the menu moved).
    pub fn nudge(&mut self) {
        if self.since > SWEEP_TIME * 0.7 {
            self.since = 0.0;
        }
    }

    /// Where the invisible pointer is `t` seconds into a sweep: in from the left, across the
    /// letters with a slow wave, out at the right; parked below the letters in between.
    fn sweep_point(&self, t: f64) -> (f64, f64) {
        if t >= SWEEP_TIME {
            return (self.width * 1.02, self.height * 0.92);
        }
        let u = t / SWEEP_TIME;
        let x = self.width * (-0.02 + 1.04 * u);
        let y = self.height * (0.52 + 0.1 * (u * std::f64::consts::TAU * 1.5).sin());
        (x, y)
    }

    /// One frame: the menu's scene over the animation, presented.
    pub fn frame(&mut self, display: &Display, dt: f64, overlay: Scene) {
        self.since += dt;
        if self.since > SWEEP_EVERY {
            self.since = 0.0;
        }
        let p = self.sweep_point(self.since);
        display.set_overlay(overlay);
        if let Ok(mut player) = self.player.lock() {
            if p != self.pointer {
                self.pointer = p;
                player.handle_event(PlayerEvent::MouseMove { x: p.0, y: p.1 });
            }
            player.tick(ruffle_core::FloatDuration::from_millis(dt * 1000.0));
            // Every frame, not only when the movie changed: the menu over it animates.
            player.render();
        }
        self.frames += 1;
    }
}
