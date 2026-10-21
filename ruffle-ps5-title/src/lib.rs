//! PS5_RUFFLE_V1 — Ruffle as a PlayStation 5 title (milestone 3).
//!
//! The C shell (shell/main.c) calls `sceAgcInit(8)` from the executable, then `ruffle_ps5_main`.
//! Two phases, so the novel GPU seam is proven before Ruffle rides on it:
//!   * 3a (the default build): bring up wgpu on the static RADV and clear the display to an
//!     animated colour for a few seconds. If a triangle-equivalent shows, the static-RADV +
//!     VK_KHR_display + wgpu path works in a title — the thing the payload route could never get.
//!   * 3b (feature `ruffle`): load a SWF from /data/ruffle and play it through ruffle_render_wgpu.
//!
//! Progress is toasted on the TV and written to /data/ruffle/ruffle.log by the shell's `ps5_log`.
// SPDX-License-Identifier: GPL-3.0-or-later

use std::ffi::{c_char, c_int, CStr};
use std::time::{Duration, Instant};

mod gpu;

extern "C" {
    fn ps5_notify(msg: *const c_char);
    fn ps5_log(msg: *const c_char);
}

fn say(s: &str) {
    if let Ok(c) = std::ffi::CString::new(s) {
        unsafe { ps5_log(c.as_ptr()) };
    }
}
fn toast(s: &str) {
    if let Ok(c) = std::ffi::CString::new(s) {
        unsafe { ps5_notify(c.as_ptr()) };
    }
}

/// Entry from the C shell. `data_dir` is where SWFs and the log live (/data/ruffle). Returns the
/// number of failures (0 = ok), which the shell toasts.
#[no_mangle]
pub extern "C" fn ruffle_ps5_main(data_dir: *const c_char) -> c_int {
    let data_dir = unsafe { CStr::from_ptr(data_dir) }
        .to_string_lossy()
        .into_owned();
    say(&format!("ruffle-ps5 (PS5_RUFFLE_V1) starting; data dir {data_dir}"));
    toast("ruffle-ps5: bringing up RADV");

    let gpu = match gpu::create() {
        Ok(g) => {
            say(&format!(
                "ok   wgpu on RADV: {}x{}, adapter {:?}",
                g.width,
                g.height,
                g.adapter.get_info()
            ));
            toast(&format!("ruffle-ps5: RADV up, {}x{}", g.width, g.height));
            g
        }
        Err(e) => {
            say(&format!("FAIL wgpu on RADV: {e}"));
            toast("ruffle-ps5: RADV bring-up failed (see log)");
            return 1;
        }
    };

    #[cfg(not(feature = "ruffle"))]
    {
        // 3a: animate a clear colour for 6 seconds, proving present works end to end.
        let t0 = Instant::now();
        let mut frames = 0u64;
        while t0.elapsed() < Duration::from_secs(6) {
            let t = t0.elapsed().as_secs_f64();
            let r = 0.5 + 0.5 * (t * 1.1).sin();
            let g = 0.1;
            let b = 0.5 + 0.5 * (t * 0.7).cos();
            if let Err(e) = gpu::present_clear(&gpu, r, g, b) {
                say(&format!("FAIL present at frame {frames}: {e}"));
                toast("ruffle-ps5: present failed (see log)");
                return 1;
            }
            frames += 1;
        }
        let fps = frames as f64 / t0.elapsed().as_secs_f64();
        say(&format!("ok   {frames} frames presented, {fps:.1} fps"));
        toast(&format!("ruffle-ps5: RADV present OK, {fps:.0} fps"));
        0
    }

    #[cfg(feature = "ruffle")]
    {
        match run_ruffle(&gpu, &data_dir) {
            Ok(()) => 0,
            Err(e) => {
                say(&format!("FAIL ruffle: {e}"));
                toast("ruffle-ps5: playback failed (see log)");
                1
            }
        }
    }
}

#[cfg(feature = "ruffle")]
fn run_ruffle(gpu: &gpu::Gpu, data_dir: &str) -> Result<(), String> {
    use ruffle_core::backend::audio::NullAudioBackend;
    use ruffle_core::backend::log::NullLogBackend;
    use ruffle_core::backend::navigator::{NullExecutor, NullNavigatorBackend};
    use ruffle_core::backend::storage::MemoryStorageBackend;
    use ruffle_core::backend::ui::NullUiBackend;
    use ruffle_core::tag_utils::SwfMovie;
    use ruffle_core::{PlayerBuilder, ViewportDimensions};
    use ruffle_render_wgpu::backend::WgpuRenderBackend;
    use ruffle_render_wgpu::target::SwapChainTarget;

    let swf_path = format!("{data_dir}/movie.swf");
    say(&format!("loading {swf_path}"));
    let data = std::fs::read(&swf_path).map_err(|e| format!("read {swf_path}: {e}"))?;
    let movie = SwfMovie::from_data(&data, format!("file://{swf_path}"), None)
        .map_err(|e| format!("parse swf: {e}"))?;

    // Ruffle's wgpu backend over our RADV surface. SwapChainTarget presents through the surface
    // we already configured; Descriptors wrap the shared device/queue.
    let descriptors = gpu::descriptors(gpu);
    let target = SwapChainTarget::new(
        // SERVER: match SwapChainTarget::new's exact argument order for this Ruffle rev.
        unsafe { std::mem::transmute_copy(&gpu.surface) },
        &gpu.adapter,
        (gpu.width, gpu.height),
        &gpu.device,
    );
    let renderer = WgpuRenderBackend::new(descriptors, target).map_err(|e| format!("renderer: {e}"))?;

    let mut executor = NullExecutor::new();
    let navigator = NullNavigatorBackend::new();
    let player = PlayerBuilder::new()
        .with_renderer(renderer)
        .with_audio(NullAudioBackend::new())
        .with_navigator(navigator)
        .with_storage(Box::new(MemoryStorageBackend::new()))
        .with_ui(NullUiBackend::new())
        .with_log(NullLogBackend::new())
        .with_movie(movie)
        .with_viewport_dimensions(gpu.width, gpu.height, 1.0)
        .with_autoplay(true)
        .build();

    say("player built; entering frame loop");
    toast("ruffle-ps5: playing");
    let mut last = Instant::now();
    loop {
        let now = Instant::now();
        let dt = now.duration_since(last);
        last = now;
        player.lock().unwrap().tick(dt.as_millis() as f64);
        if player.lock().unwrap().needs_render() {
            player.lock().unwrap().render();
        }
        executor.poll_all().ok();
        // crude 60 fps cap; replace with the present clock once pacing matters.
        let frame = Duration::from_micros(16_666);
        if dt < frame {
            std::thread::sleep(frame - dt);
        }
    }
}
