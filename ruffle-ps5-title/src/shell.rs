//! PS5_RUFFLE_SHELL_V3 — one display, shared by the menu and every game's player.
//!
//! Ruffle's `WgpuRenderBackend` takes ownership of its render target, and a `Player` owns its
//! renderer; to play one game after another each gets a fresh `Player`. So the swapchain lives here,
//! in `Display`, behind an `Arc`, and `ShellTarget` is a handle to it that implements Ruffle's
//! `RenderTarget`: a player is built with a new handle, dropped with it, and the display stays.
//!
//! V2 (4K): the display runs at the largest size VideoOut offers (3840x2160 on the PS5) and a game
//! renders at one of two sizes:
//!   * the display's own — Ruffle draws its vectors straight into the swapchain image at 3840x2160:
//!     real 4K, not an upscale, and nothing is copied (Settings > Display > Resolution: 4K);
//!   * 1920x1080 — Ruffle draws into a texture of that size, and one GPU pass scales it onto the
//!     swapchain image (bilinear). No CPU copy, no wait: the pass is queued with Ruffle's own work.
//! The menu's screens are drawn at the display's size by src/ui.rs. Over a game's frame, at submit
//! time, the shell paints the pointer (src/cursor.rs) and the overlay the main loop left in
//! `Display::overlay` (quick menu, keyboard, toasts, FPS, fades).
//!
//! A cover picture is one frame of the game read back: the next frame renders through the texture
//! path even at the display's size, is copied to a buffer, and the buffer is mapped without waiting
//! (`poll_capture` collects it a frame or two later). Only then, once.
//!
//! The time spent acquiring, painting overlays and presenting is counted (`FrameStats`) for the
//! frame-time log.
//!
//! V3 (Shockwave): a Director stage is rendered on the CPU (DirPlayer, src/shockwave.rs) and shown
//! as a picture: `present_picture` uploads its pixels into a texture of the stage's size and the
//! same scaling pass draws it into a rectangle of the display (`picture_rect`: whole multiples,
//! fitted, or stretched), sharp (nearest) or smooth (bilinear), black around it, the pointer and
//! the overlay over it.
// SPDX-License-Identifier: GPL-3.0-or-later

use crate::cursor::{Cursor, CursorPainter, SharedCursor};
use crate::ui::{Color, Scene, Ui};
use ruffle_render_wgpu::target::{RenderTarget, RenderTargetFrame};
use std::sync::atomic::{AtomicU64, AtomicU8, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Instant;

/// Time spent in the display's parts of a frame, in microseconds, since the last `take`.
#[derive(Default)]
pub struct FrameStats {
    acquire_us: AtomicU64,
    overlay_us: AtomicU64,
    present_us: AtomicU64,
    presents: AtomicU64,
    readback_us: AtomicU64,
}

/// One `FrameStats::take`, in milliseconds.
#[derive(Clone, Copy, Default, Debug)]
pub struct StatsTaken {
    pub acquire_ms: f64,
    pub overlay_ms: f64,
    pub present_ms: f64,
    pub presents: u64,
    pub readback_ms: f64,
}

impl FrameStats {
    fn add(slot: &AtomicU64, since: Instant) {
        slot.fetch_add(since.elapsed().as_micros() as u64, Ordering::Relaxed);
    }

    pub fn take(&self) -> StatsTaken {
        StatsTaken {
            acquire_ms: self.acquire_us.swap(0, Ordering::Relaxed) as f64 / 1000.0,
            overlay_ms: self.overlay_us.swap(0, Ordering::Relaxed) as f64 / 1000.0,
            present_ms: self.present_us.swap(0, Ordering::Relaxed) as f64 / 1000.0,
            presents: self.presents.swap(0, Ordering::Relaxed),
            readback_ms: self.readback_us.swap(0, Ordering::Relaxed) as f64 / 1000.0,
        }
    }
}

/// A picture read back from a frame: RGBA8, `width` x `height`.
pub struct Captured {
    pub width: u32,
    pub height: u32,
    pub rgba: Vec<u8>,
}

enum Capture {
    Idle,
    Wanted,
    /// The copy is recorded and submitted; the buffer maps when the GPU is done. `ready`:
    /// 0 waiting, 1 mapped, 2 failed.
    Pending {
        buffer: wgpu::Buffer,
        width: u32,
        height: u32,
        row: u32,
        ready: Arc<AtomicU8>,
        mapping: bool,
    },
}

struct Offscreen {
    texture: wgpu::Texture,
    view: wgpu::TextureView,
    bind: wgpu::BindGroup,
    width: u32,
    height: u32,
}

const BLIT_SHADER: &str = r#"
@group(0) @binding(0) var src: texture_2d<f32>;
@group(0) @binding(1) var samp: sampler;

var<private> CORNERS: array<vec2<f32>, 3> = array<vec2<f32>, 3>(
    vec2<f32>(-1.0, -1.0), vec2<f32>(3.0, -1.0), vec2<f32>(-1.0, 3.0),
);

struct VsOut {
    @builtin(position) pos: vec4<f32>,
    @location(0) uv: vec2<f32>,
};

@vertex
fn vs_main(@builtin(vertex_index) vi: u32) -> VsOut {
    let xy = CORNERS[vi];
    var out: VsOut;
    out.pos = vec4<f32>(xy, 0.0, 1.0);
    out.uv = vec2<f32>((xy.x + 1.0) * 0.5, 1.0 - (xy.y + 1.0) * 0.5);
    return out;
}

@fragment
fn fs_main(in: VsOut) -> @location(0) vec4<f32> {
    return vec4<f32>(textureSample(src, samp, in.uv).rgb, 1.0);
}
"#;

/// The pass that scales a game's texture onto the swapchain image.
struct Blit {
    pipeline: wgpu::RenderPipeline,
    layout: wgpu::BindGroupLayout,
    sampler: wgpu::Sampler,
    nearest: wgpu::Sampler,
}

/// How a picture (a Director stage) fills the display.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Fit {
    /// The largest whole multiple of its size that fits: every pixel the same size.
    Whole,
    /// As large as fits, its shape kept.
    Fit,
    /// The whole display, its shape not kept.
    Stretch,
}

/// The picture's texture (a Director stage, uploaded each frame it changes).
struct Picture {
    texture: wgpu::Texture,
    smooth: wgpu::BindGroup,
    sharp: wgpu::BindGroup,
    width: u32,
    height: u32,
}

/// Where a `width` x `height` picture goes on a `dw` x `dh` display: [x, y, w, h] in display pixels.
pub fn picture_rect(width: u32, height: u32, dw: u32, dh: u32, fit: Fit) -> [f32; 4] {
    let (w, h, dw, dh) = (width.max(1) as f32, height.max(1) as f32, dw as f32, dh as f32);
    let (rw, rh) = match fit {
        Fit::Stretch => (dw, dh),
        Fit::Fit => {
            let k = (dw / w).min(dh / h);
            (w * k, h * k)
        }
        Fit::Whole => {
            let k = (dw / w).min(dh / h);
            let k = if k >= 1.0 { k.floor() } else { k };
            (w * k, h * k)
        }
    };
    let (rw, rh) = (rw.round().min(dw), rh.round().min(dh));
    [((dw - rw) / 2.0).floor(), ((dh - rh) / 2.0).floor(), rw, rh]
}

impl Blit {
    fn new(device: &wgpu::Device, format: wgpu::TextureFormat) -> Blit {
        let module = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("blit"),
            source: wgpu::ShaderSource::Wgsl(BLIT_SHADER.into()),
        });
        let layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("blit"),
            entries: &[
                wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Texture {
                        sample_type: wgpu::TextureSampleType::Float { filterable: true },
                        view_dimension: wgpu::TextureViewDimension::D2,
                        multisampled: false,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 1,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                    count: None,
                },
            ],
        });
        let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("blit"),
            address_mode_u: wgpu::AddressMode::ClampToEdge,
            address_mode_v: wgpu::AddressMode::ClampToEdge,
            address_mode_w: wgpu::AddressMode::ClampToEdge,
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            mipmap_filter: wgpu::MipmapFilterMode::Nearest,
            ..Default::default()
        });
        let nearest = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("blit nearest"),
            address_mode_u: wgpu::AddressMode::ClampToEdge,
            address_mode_v: wgpu::AddressMode::ClampToEdge,
            address_mode_w: wgpu::AddressMode::ClampToEdge,
            mag_filter: wgpu::FilterMode::Nearest,
            min_filter: wgpu::FilterMode::Linear,
            mipmap_filter: wgpu::MipmapFilterMode::Nearest,
            ..Default::default()
        });
        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("blit"),
            bind_group_layouts: &[Some(&layout)],
            immediate_size: 0,
        });
        let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("blit"),
            layout: Some(&pipeline_layout),
            vertex: wgpu::VertexState {
                module: &module,
                entry_point: Some("vs_main"),
                buffers: &[],
                compilation_options: Default::default(),
            },
            fragment: Some(wgpu::FragmentState {
                module: &module,
                entry_point: Some("fs_main"),
                targets: &[Some(wgpu::ColorTargetState {
                    format,
                    blend: Some(wgpu::BlendState::REPLACE),
                    write_mask: wgpu::ColorWrites::ALL,
                })],
                compilation_options: Default::default(),
            }),
            primitive: wgpu::PrimitiveState {
                topology: wgpu::PrimitiveTopology::TriangleList,
                strip_index_format: None,
                front_face: wgpu::FrontFace::Ccw,
                cull_mode: None,
                polygon_mode: wgpu::PolygonMode::Fill,
                unclipped_depth: false,
                conservative: false,
            },
            depth_stencil: None,
            multisample: wgpu::MultisampleState {
                count: 1,
                mask: !0,
                alpha_to_coverage_enabled: false,
            },
            multiview_mask: None,
            cache: None,
        });
        Blit { pipeline, layout, sampler, nearest }
    }

    fn bind(&self, device: &wgpu::Device, view: &wgpu::TextureView, sampler: &wgpu::Sampler) -> wgpu::BindGroup {
        device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("blit"),
            layout: &self.layout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: wgpu::BindingResource::TextureView(view),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: wgpu::BindingResource::Sampler(sampler),
                },
            ],
        })
    }

    fn encode(&self, device: &wgpu::Device, bind: &wgpu::BindGroup, target: &wgpu::TextureView) -> wgpu::CommandBuffer {
        self.encode_into(device, bind, target, None)
    }

    /// The scaling pass, into `rect` ([x, y, w, h] in the target's pixels) or all of it; the rest
    /// of the target is cleared to black.
    fn encode_into(&self, device: &wgpu::Device, bind: &wgpu::BindGroup, target: &wgpu::TextureView, rect: Option<[f32; 4]>) -> wgpu::CommandBuffer {
        let mut enc = device.create_command_encoder(&wgpu::CommandEncoderDescriptor { label: Some("blit") });
        {
            let mut pass = enc.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("blit"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: target,
                    depth_slice: None,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(wgpu::Color::BLACK),
                        store: wgpu::StoreOp::Store,
                    },
                })],
                depth_stencil_attachment: None,
                timestamp_writes: None,
                occlusion_query_set: None,
                multiview_mask: None,
            });
            pass.set_pipeline(&self.pipeline);
            if let Some([x, y, w, h]) = rect {
                if w >= 1.0 && h >= 1.0 {
                    pass.set_viewport(x, y, w, h, 0.0, 1.0);
                }
            }
            pass.set_bind_group(0, bind, &[]);
            pass.draw(0..3, 0..1);
        }
        enc.finish()
    }
}

pub struct Display {
    surface: wgpu::Surface<'static>,
    config: wgpu::SurfaceConfiguration,
    pub device: wgpu::Device,
    pub queue: wgpu::Queue,
    pub width: u32,
    pub height: u32,
    pub format: wgpu::TextureFormat,
    cursor: SharedCursor,
    painter: CursorPainter,
    pub ui: Mutex<Ui>,
    overlay: Mutex<Scene>,
    blit: Blit,
    offscreen: Mutex<Option<Offscreen>>,
    picture: Mutex<Option<Picture>>,
    capture: Mutex<Capture>,
    pub stats: FrameStats,
    lost_logged: AtomicU8,
}

impl Display {
    /// `surface` is configured with `config` already (src/gpu.rs).
    pub fn new(
        surface: wgpu::Surface<'static>,
        config: wgpu::SurfaceConfiguration,
        device: wgpu::Device,
        queue: wgpu::Queue,
        cursor: SharedCursor,
    ) -> Result<Arc<Display>, String> {
        let (width, height, format) = (config.width, config.height, config.format);
        let painter = CursorPainter::new(&device, format, width, height);
        let ui = Ui::new(&device, &queue, format, width, height)?;
        let blit = Blit::new(&device, format);
        Ok(Arc::new(Display {
            surface,
            config,
            device,
            queue,
            width,
            height,
            format,
            cursor,
            painter,
            ui: Mutex::new(ui),
            overlay: Mutex::new(Scene::new()),
            blit,
            offscreen: Mutex::new(None),
            picture: Mutex::new(None),
            capture: Mutex::new(Capture::Idle),
            stats: FrameStats::default(),
            lost_logged: AtomicU8::new(0),
        }))
    }

    /// A render target for a new player, rendering at `width` x `height` (the display's size, or
    /// smaller: then scaled up when shown).
    pub fn target(self: &Arc<Self>, width: u32, height: u32) -> ShellTarget {
        ShellTarget {
            display: Arc::clone(self),
            width: width.max(1),
            height: height.max(1),
        }
    }

    /// Replace the overlay drawn over a game's frames (an empty scene draws nothing).
    pub fn set_overlay(&self, scene: Scene) {
        if let Ok(mut o) = self.overlay.lock() {
            *o = scene;
        }
    }

    /// Ask for the next game frame to be read back (a cover picture).
    pub fn request_capture(&self) {
        if let Ok(mut c) = self.capture.lock() {
            if matches!(*c, Capture::Idle) {
                *c = Capture::Wanted;
            }
        }
    }

    fn capture_wanted(&self) -> bool {
        self.capture.lock().map(|c| matches!(*c, Capture::Wanted)).unwrap_or(false)
    }

    /// The frame asked for with `request_capture`, once the GPU has delivered it (call every frame
    /// while one is pending; it never waits).
    pub fn poll_capture(&self) -> Option<Captured> {
        let mut c = self.capture.lock().ok()?;
        let state = match &*c {
            Capture::Pending { ready, .. } => ready.load(Ordering::Acquire),
            _ => return None,
        };
        if state == 0 {
            let _ = self.device.poll(wgpu::PollType::Poll);
            return None;
        }
        let t = Instant::now();
        let taken = std::mem::replace(&mut *c, Capture::Idle);
        let Capture::Pending { buffer, width, height, row, .. } = taken else {
            return None;
        };
        if state != 1 {
            crate::say("warn cover capture: the frame could not be read back");
            return None;
        }
        let mut rgba = vec![0u8; (width * height * 4) as usize];
        {
            let view = match buffer.get_mapped_range(..) {
                Ok(v) => v,
                Err(e) => {
                    crate::say(&format!("warn cover capture: {e:?}"));
                    return None;
                }
            };
            let bgra = matches!(self.format, wgpu::TextureFormat::Bgra8Unorm | wgpu::TextureFormat::Bgra8UnormSrgb);
            for y in 0..height as usize {
                let src = &view[y * row as usize..y * row as usize + width as usize * 4];
                let dst = &mut rgba[y * width as usize * 4..(y + 1) * width as usize * 4];
                if bgra {
                    for (d, s) in dst.chunks_exact_mut(4).zip(src.chunks_exact(4)) {
                        d[0] = s[2];
                        d[1] = s[1];
                        d[2] = s[0];
                        d[3] = 255;
                    }
                } else {
                    dst.copy_from_slice(src);
                }
            }
        }
        buffer.unmap();
        FrameStats::add(&self.stats.readback_us, t);
        Some(Captured { width, height, rgba })
    }

    /// Free the scaling texture (a game ended).
    pub fn release_offscreen(&self) {
        if let Ok(mut o) = self.offscreen.lock() {
            *o = None;
        }
        if let Ok(mut p) = self.picture.lock() {
            *p = None;
        }
        if let Ok(mut c) = self.capture.lock() {
            *c = Capture::Idle;
        }
    }

    /// The next swapchain image, or None (the frame is skipped).
    fn acquire(&self) -> Option<wgpu::SurfaceTexture> {
        let t = Instant::now();
        let got = match self.surface.get_current_texture() {
            wgpu::CurrentSurfaceTexture::Success(st) | wgpu::CurrentSurfaceTexture::Suboptimal(st) => Some(st),
            wgpu::CurrentSurfaceTexture::Outdated | wgpu::CurrentSurfaceTexture::Lost => {
                self.surface.configure(&self.device, &self.config);
                if self.lost_logged.fetch_add(1, Ordering::Relaxed) < 5 {
                    crate::say("warn swapchain outdated or lost: reconfigured");
                }
                None
            }
            other => {
                if self.lost_logged.fetch_add(1, Ordering::Relaxed) < 5 {
                    crate::say(&format!("warn no swapchain image: {other:?}"));
                }
                None
            }
        };
        FrameStats::add(&self.stats.acquire_us, t);
        got
    }

    /// The texture a smaller game renders into, made (or remade) at its size.
    fn offscreen_view(&self, width: u32, height: u32) -> Option<wgpu::TextureView> {
        let mut slot = self.offscreen.lock().ok()?;
        let fits = slot.as_ref().map(|o| o.width == width && o.height == height).unwrap_or(false);
        if !fits {
            let texture = self.device.create_texture(&wgpu::TextureDescriptor {
                label: Some("game frame"),
                size: wgpu::Extent3d {
                    width,
                    height,
                    depth_or_array_layers: 1,
                },
                mip_level_count: 1,
                sample_count: 1,
                dimension: wgpu::TextureDimension::D2,
                format: self.format,
                usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_SRC,
                view_formats: &[],
            });
            let view = texture.create_view(&wgpu::TextureViewDescriptor::default());
            let bind = self.device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some("blit"),
                layout: &self.blit.layout,
                entries: &[
                    wgpu::BindGroupEntry {
                        binding: 0,
                        resource: wgpu::BindingResource::TextureView(&view),
                    },
                    wgpu::BindGroupEntry {
                        binding: 1,
                        resource: wgpu::BindingResource::Sampler(&self.blit.sampler),
                    },
                ],
            });
            *slot = Some(Offscreen {
                texture,
                view,
                bind,
                width,
                height,
            });
        }
        slot.as_ref().map(|o| o.view.clone())
    }

    /// When a capture is wanted, the copy of the game's texture into a mappable buffer.
    fn record_capture(&self, device: &wgpu::Device) -> Option<wgpu::CommandBuffer> {
        let mut c = self.capture.lock().ok()?;
        if !matches!(*c, Capture::Wanted) {
            return None;
        }
        let slot = self.offscreen.lock().ok()?;
        let off = slot.as_ref()?;
        let (width, height) = (off.width, off.height);
        let row = (width * 4).div_ceil(wgpu::COPY_BYTES_PER_ROW_ALIGNMENT) * wgpu::COPY_BYTES_PER_ROW_ALIGNMENT;
        let buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("cover capture"),
            size: row as u64 * height as u64,
            usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
            mapped_at_creation: false,
        });
        let mut enc = device.create_command_encoder(&wgpu::CommandEncoderDescriptor { label: Some("cover capture") });
        enc.copy_texture_to_buffer(
            wgpu::TexelCopyTextureInfo {
                texture: &off.texture,
                mip_level: 0,
                origin: wgpu::Origin3d::ZERO,
                aspect: wgpu::TextureAspect::All,
            },
            wgpu::TexelCopyBufferInfo {
                buffer: &buffer,
                layout: wgpu::TexelCopyBufferLayout {
                    offset: 0,
                    bytes_per_row: Some(row),
                    rows_per_image: Some(height),
                },
            },
            wgpu::Extent3d {
                width,
                height,
                depth_or_array_layers: 1,
            },
        );
        *c = Capture::Pending {
            buffer,
            width,
            height,
            row,
            ready: Arc::new(AtomicU8::new(0)),
            mapping: false,
        };
        Some(enc.finish())
    }

    /// After the copy is submitted: ask for the buffer to be mapped (it happens when the GPU is done).
    fn map_capture(&self) {
        if let Ok(mut c) = self.capture.lock() {
            if let Capture::Pending { buffer, ready, mapping, .. } = &mut *c {
                if !*mapping {
                    *mapping = true;
                    let flag = Arc::clone(ready);
                    buffer.map_async(wgpu::MapMode::Read, .., move |result| {
                        flag.store(if result.is_ok() { 1 } else { 2 }, Ordering::Release);
                    });
                }
            }
        }
    }

    /// The pointer and the overlay over `view` (the swapchain image), as command buffers.
    fn paint_over(&self, device: &wgpu::Device, queue: &wgpu::Queue, view: &wgpu::TextureView, buffers: &mut Vec<wgpu::CommandBuffer>) {
        let cursor: Option<Cursor> = self.cursor.lock().ok().map(|c| *c);
        if let Some(c) = cursor {
            if let Some(cmd) = self.painter.paint(device, queue, view, c) {
                buffers.push(cmd);
            }
        }
        if let (Ok(overlay), Ok(mut ui)) = (self.overlay.lock(), self.ui.lock()) {
            if !overlay.is_empty() {
                buffers.push(ui.render(device, queue, view, None, &overlay));
            }
        }
    }

    /// Show a picture (a Director stage): `upload` is its RGBA8 pixels when they changed (None
    /// shows the last ones again), scaled into `picture_rect(.., fit)`, sharp or smooth, with black
    /// around it, then the pointer and the overlay over it; presented.
    pub fn present_picture(&self, upload: Option<(&[u8], u32, u32)>, fit: Fit, smooth: bool) -> Result<(), String> {
        let t = Instant::now();
        let mut slot = self.picture.lock().map_err(|_| "picture lock poisoned")?;
        if let Some((rgba, width, height)) = upload {
            let (width, height) = (width.max(1), height.max(1));
            if rgba.len() < (width * height * 4) as usize {
                return Err(format!("picture: {} bytes for {width}x{height}", rgba.len()));
            }
            let fits = slot.as_ref().is_some_and(|p| p.width == width && p.height == height);
            if !fits {
                let texture = self.device.create_texture(&wgpu::TextureDescriptor {
                    label: Some("stage"),
                    size: wgpu::Extent3d {
                        width,
                        height,
                        depth_or_array_layers: 1,
                    },
                    mip_level_count: 1,
                    sample_count: 1,
                    dimension: wgpu::TextureDimension::D2,
                    format: wgpu::TextureFormat::Rgba8Unorm,
                    usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
                    view_formats: &[],
                });
                let view = texture.create_view(&wgpu::TextureViewDescriptor::default());
                let smooth = self.blit.bind(&self.device, &view, &self.blit.sampler);
                let sharp = self.blit.bind(&self.device, &view, &self.blit.nearest);
                *slot = Some(Picture { texture, smooth, sharp, width, height });
            }
            if let Some(p) = slot.as_ref() {
                self.queue.write_texture(
                    wgpu::TexelCopyTextureInfo {
                        texture: &p.texture,
                        mip_level: 0,
                        origin: wgpu::Origin3d::ZERO,
                        aspect: wgpu::TextureAspect::All,
                    },
                    &rgba[..(width * height * 4) as usize],
                    wgpu::TexelCopyBufferLayout {
                        offset: 0,
                        bytes_per_row: Some(width * 4),
                        rows_per_image: Some(height),
                    },
                    wgpu::Extent3d {
                        width,
                        height,
                        depth_or_array_layers: 1,
                    },
                );
            }
        }
        FrameStats::add(&self.stats.overlay_us, t);
        let Some(st) = self.acquire() else {
            return Ok(());
        };
        let t = Instant::now();
        let view = st.texture.create_view(&wgpu::TextureViewDescriptor::default());
        let mut buffers = Vec::new();
        match slot.as_ref() {
            Some(p) => {
                let rect = picture_rect(p.width, p.height, self.width, self.height, fit);
                buffers.push(self.blit.encode_into(&self.device, if smooth { &p.smooth } else { &p.sharp }, &view, Some(rect)));
            }
            None => {
                let mut ui = self.ui.lock().map_err(|_| "ui lock poisoned")?;
                buffers.push(ui.render(&self.device, &self.queue, &view, Some(Color::rgb(0)), &Scene::new()));
            }
        }
        drop(slot);
        self.paint_over(&self.device, &self.queue, &view, &mut buffers);
        FrameStats::add(&self.stats.overlay_us, t);
        let t = Instant::now();
        self.queue.submit(buffers);
        self.queue.present(st);
        FrameStats::add(&self.stats.present_us, t);
        self.stats.presents.fetch_add(1, Ordering::Relaxed);
        Ok(())
    }

    /// Draw `scene` on a frame cleared to `clear` and present it (the menu without a background
    /// movie, the loading screen).
    pub fn present_scene(&self, scene: &Scene, clear: Color) -> Result<(), String> {
        let Some(st) = self.acquire() else {
            return Ok(());
        };
        let t = Instant::now();
        let view = st.texture.create_view(&wgpu::TextureViewDescriptor::default());
        let cmd = {
            let mut ui = self.ui.lock().map_err(|_| "ui lock poisoned")?;
            ui.render(&self.device, &self.queue, &view, Some(clear), scene)
        };
        FrameStats::add(&self.stats.overlay_us, t);
        let t = Instant::now();
        self.queue.submit([cmd]);
        self.queue.present(st);
        FrameStats::add(&self.stats.present_us, t);
        self.stats.presents.fetch_add(1, Ordering::Relaxed);
        Ok(())
    }
}

/// A frame Ruffle draws into: the swapchain image itself, or the game's own texture.
#[derive(Debug)]
pub struct ShellFrame {
    surface: Option<wgpu::SurfaceTexture>,
    view: wgpu::TextureView,
}

impl RenderTargetFrame for ShellFrame {
    fn into_view(self) -> wgpu::TextureView {
        self.view
    }

    fn view(&self) -> &wgpu::TextureView {
        &self.view
    }
}

pub struct ShellTarget {
    display: Arc<Display>,
    width: u32,
    height: u32,
}

impl std::fmt::Debug for ShellTarget {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "ShellTarget({}x{} on {}x{} {:?})", self.width, self.height, self.display.width, self.display.height, self.display.format)
    }
}

impl RenderTarget for ShellTarget {
    type Frame = ShellFrame;

    /// The player's viewport size: the size it renders at (the display stays as it is).
    fn resize(&mut self, _device: &wgpu::Device, width: u32, height: u32) {
        self.width = width.max(1);
        self.height = height.max(1);
    }

    fn format(&self) -> wgpu::TextureFormat {
        self.display.format
    }

    fn width(&self) -> u32 {
        self.width
    }

    fn height(&self) -> u32 {
        self.height
    }

    fn get_next_texture(&mut self) -> Option<Self::Frame> {
        let d = &self.display;
        let direct = self.width == d.width && self.height == d.height && !d.capture_wanted();
        if direct {
            let st = d.acquire()?;
            let view = st.texture.create_view(&wgpu::TextureViewDescriptor::default());
            Some(ShellFrame { surface: Some(st), view })
        } else {
            let view = d.offscreen_view(self.width, self.height)?;
            Some(ShellFrame { surface: None, view })
        }
    }

    fn submit<I: IntoIterator<Item = wgpu::CommandBuffer>>(&self, device: &wgpu::Device, queue: &wgpu::Queue, command_buffers: I, frame: Self::Frame) -> wgpu::SubmissionIndex {
        let d = &self.display;
        let mut buffers: Vec<wgpu::CommandBuffer> = command_buffers.into_iter().collect();
        let ShellFrame { surface, view } = frame;
        let (shown, view) = match surface {
            Some(st) => (Some(st), view),
            None => {
                // The game's texture: maybe read back, then scaled onto the swapchain image.
                if let Some(cmd) = d.record_capture(device) {
                    buffers.push(cmd);
                }
                match d.acquire() {
                    Some(st) => {
                        let t = Instant::now();
                        let swap_view = st.texture.create_view(&wgpu::TextureViewDescriptor::default());
                        if let Ok(slot) = d.offscreen.lock() {
                            if let Some(off) = slot.as_ref() {
                                buffers.push(d.blit.encode(device, &off.bind, &swap_view));
                            }
                        }
                        FrameStats::add(&d.stats.overlay_us, t);
                        (Some(st), swap_view)
                    }
                    None => (None, view),
                }
            }
        };
        let t = Instant::now();
        if shown.is_some() {
            d.paint_over(device, queue, &view, &mut buffers);
        }
        FrameStats::add(&d.stats.overlay_us, t);
        let t = Instant::now();
        let index = queue.submit(buffers);
        d.map_capture();
        if let Some(st) = shown {
            queue.present(st);
            d.stats.presents.fetch_add(1, Ordering::Relaxed);
        }
        FrameStats::add(&d.stats.present_us, t);
        index
    }
}
