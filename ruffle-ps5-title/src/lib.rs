//! PS5_RUFFLE_V2 — Ruffle as a PlayStation 5 title (milestone 3).
//!
//! The C shell (shell/main.c) calls `sceAgcInit(8)` from the executable, then `ruffle_ps5_main`.
//! Two phases, so the novel GPU seam is proven before Ruffle rides on it:
//!   * 3a (the default build): bring up wgpu on the static RADV and clear the display to an
//!     animated colour for a few seconds. Colour on the TV means the static-RADV + VK_KHR_display +
//!     wgpu path works in a title — the thing the payload route could never get.
//!   * 3b (feature `ruffle`): load a SWF from /data/ruffle and play it through ruffle_render_wgpu.
//!
//! Progress is toasted on the TV and written to /data/ruffle/ruffle.log by the shell's `ps5_log`.
// SPDX-License-Identifier: GPL-3.0-or-later

use std::ffi::{c_char, c_int, CStr};
use std::time::{Duration, Instant};

mod gpu;
mod random;

extern "C" {
    fn ps5_notify(msg: *const c_char);
    fn ps5_log(msg: *const c_char);
}

fn say(s: &str) {
    if let Ok(c) = std::ffi::CString::new(s) {
        unsafe { ps5_log(c.as_ptr()) };
    }
}

/// wgpu-hal says why it declines an adapter, which extensions it lacks, what the swapchain does —
/// all through `log`, at debug level. Into ruffle.log, so the console tells us instead of us guessing.
struct Ps5Logger;
impl log::Log for Ps5Logger {
    fn enabled(&self, m: &log::Metadata) -> bool {
        if m.target().starts_with("wgpu_hal") {
            m.level() <= log::Level::Debug
        } else {
            m.level() <= log::Level::Info
        }
    }
    fn log(&self, r: &log::Record) {
        if self.enabled(r.metadata()) {
            say(&format!("[{} {}] {}", r.level(), r.target(), r.args()));
        }
    }
    fn flush(&self) {}
}
static LOGGER: Ps5Logger = Ps5Logger;
fn toast(s: &str) {
    if let Ok(c) = std::ffi::CString::new(s) {
        unsafe { ps5_notify(c.as_ptr()) };
    }
}

/// Entry from the C shell. `data_dir` is where SWFs and the log live (/data/ruffle). Returns the
/// number of failures (0 = ok), which the shell logs.
#[no_mangle]
pub extern "C" fn ruffle_ps5_main(data_dir: *const c_char) -> c_int {
    let data_dir = unsafe { CStr::from_ptr(data_dir) }
        .to_string_lossy()
        .into_owned();
    say(&format!(
        "ruffle-ps5 (PS5_RUFFLE_V2) starting; data dir {data_dir}"
    ));
    if log::set_logger(&LOGGER).is_ok() {
        log::set_max_level(log::LevelFilter::Debug);
    }
    toast("ruffle-ps5: bringing up RADV");

    let t_start = Instant::now();
    let gpu = match gpu::create() {
        Ok(g) => {
            say(&format!(
                "ok   wgpu on RADV in {:.2} s: {}x{} {:?}; {}",
                t_start.elapsed().as_secs_f64(),
                g.width,
                g.height,
                g.format,
                g.report
            ));
            say(&format!("     adapter: {:?}", g.adapter.get_info()));
            say(&format!("     features: {:?}", g.device.features()));
            toast(&format!("ruffle-ps5: RADV up, {}x{}", g.width, g.height));
            g
        }
        Err(e) => {
            say(&format!("FAIL wgpu on RADV: {e}"));
            toast("ruffle-ps5: RADV bring-up failed (see ruffle.log)");
            return 1;
        }
    };

    #[cfg(not(feature = "ruffle"))]
    let result = run_clear(&gpu);
    #[cfg(feature = "ruffle")]
    let result = run_ruffle(gpu, &data_dir);
    match result {
        Ok(()) => 0,
        Err(e) => {
            say(&format!("FAIL {e}"));
            toast("ruffle-ps5: failed (see ruffle.log)");
            1
        }
    }
}

/// 3a: animate a clear colour for 8 seconds, proving present works end to end. The first frame
/// is logged on its own: it is the one that creates the swapchain.
#[cfg(not(feature = "ruffle"))]
fn run_clear(gpu: &gpu::Gpu) -> Result<(), String> {
    let t0 = Instant::now();
    let mut frames = 0u64;
    while t0.elapsed() < Duration::from_secs(8) {
        let t = t0.elapsed().as_secs_f64();
        let r = 0.5 + 0.5 * (t * 1.1).sin();
        let g = 0.15;
        let b = 0.5 + 0.5 * (t * 0.7).cos();
        gpu::present_clear(gpu, r, g, b).map_err(|e| format!("present at frame {frames}: {e}"))?;
        if frames == 0 {
            say(&format!(
                "ok   first frame presented after {:.3} s",
                t0.elapsed().as_secs_f64()
            ));
        }
        frames += 1;
    }
    let fps = frames as f64 / t0.elapsed().as_secs_f64();
    say(&format!(
        "ok   {frames} frames presented in {:.1} s, {fps:.1} fps",
        t0.elapsed().as_secs_f64()
    ));
    toast(&format!("ruffle-ps5: RADV present OK, {fps:.0} fps"));
    Ok(())
}

/// 3b: play /data/ruffle/movie.swf. Written against the pinned Ruffle revision's API
/// (core/src/player.rs, render/wgpu/src/target.rs); compiled only with --features ruffle.
#[cfg(feature = "ruffle")]
fn run_ruffle(gpu: gpu::Gpu, data_dir: &str) -> Result<(), String> {
    use ruffle_core::backend::audio::NullAudioBackend;
    use ruffle_core::backend::log::NullLogBackend;
    use ruffle_core::backend::navigator::{NullExecutor, NullNavigatorBackend};
    use ruffle_core::backend::storage::MemoryStorageBackend;
    use ruffle_core::backend::ui::NullUiBackend;
    use ruffle_core::tag_utils::SwfMovieData;
    use ruffle_core::PlayerBuilder;
    use ruffle_render_wgpu::backend::WgpuRenderBackend;
    use ruffle_render_wgpu::target::SwapChainTarget;

    let swf_path = format!("{data_dir}/movie.swf");
    say(&format!("loading {swf_path}"));
    let data = std::fs::read(&swf_path).map_err(|e| format!("read {swf_path}: {e}"))?;
    let movie = SwfMovieData::from_data(&data, format!("file://{swf_path}"), None, None)
        .map_err(|e| format!("parse swf: {e}"))?;

    // Ruffle's wgpu backend over our display surface: SwapChainTarget owns the surface and
    // configures it itself; Descriptors wrap the shared instance/adapter/device/queue.
    let descriptors = gpu::descriptors(&gpu);
    let gpu::Gpu {
        surface,
        adapter,
        device,
        width,
        height,
        ..
    } = gpu;
    let target = SwapChainTarget::new(surface, &adapter, (width, height), &device);
    let renderer =
        WgpuRenderBackend::new(descriptors, target).map_err(|e| format!("renderer: {e}"))?;

    let mut executor = NullExecutor::new();
    let player = PlayerBuilder::new()
        .with_renderer(renderer)
        .with_audio(NullAudioBackend::new())
        .with_navigator(
            NullNavigatorBackend::with_base_path(std::path::Path::new(data_dir), &executor)
                .map_err(|e| format!("navigator base path: {e}"))?,
        )
        .with_storage(Box::new(MemoryStorageBackend::new()))
        .with_ui(NullUiBackend::new())
        .with_log(NullLogBackend::new())
        .with_movie(movie)
        .with_viewport_dimensions(width, height, 1.0)
        .with_autoplay(true)
        .build();

    say("player built; entering frame loop");
    toast("ruffle-ps5: playing");
    let mut last = Instant::now();
    loop {
        let now = Instant::now();
        let dt = now.duration_since(last);
        last = now;
        {
            let mut p = player.lock().unwrap();
            p.tick(ruffle_core::FloatDuration::from_millis(
                dt.as_secs_f64() * 1000.0,
            ));
            if p.needs_render() {
                p.render();
            }
        }
        executor.run();
        // The surface is FIFO: present paces the loop at the display's rate. A floor for a
        // movie that needs no render this frame.
        let frame = Duration::from_micros(16_666);
        if dt < frame {
            std::thread::sleep(frame - dt);
        }
    }
}
