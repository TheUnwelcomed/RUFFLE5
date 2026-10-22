//! PS5_RUFFLE_UI_V1 — a small 2D renderer for the title's own screens (the menu, the overlays).
//!
//! Everything is one instanced quad pipeline: an instance is a rectangle with rounded corners
//! (a signed-distance field in the fragment shader, so edges are anti-aliased), drawn as a solid,
//! a vertical or horizontal gradient, a stroke, a soft shadow/glow, an image, or a glyph from the
//! font atlas. Text comes from three bundled faces (Inter Regular, Inter SemiBold, Montserrat
//! Medium — the ones ProsperoStore's screens use, SIL OFL 1.1, `fonts/`), rasterised by ab_glyph
//! into an R8 atlas on demand. The canvas is the display (1920x1080), y down, in pixels.
//!
//! The menu builds a `Scene` (instances in draw order) through `Ui`, which owns the fonts and the
//! atlas; `Ui::render` draws a scene into a texture view in one render pass.
// SPDX-License-Identifier: GPL-3.0-or-later

use ab_glyph::{point, Font, FontRef, GlyphId, PxScale, ScaleFont};
use std::collections::HashMap;

pub const W: f32 = 1920.0;
pub const H: f32 = 1080.0;

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Color {
    pub r: f32,
    pub g: f32,
    pub b: f32,
    pub a: f32,
}

impl Color {
    pub const fn rgb(hex: u32) -> Color {
        Color {
            r: ((hex >> 16) & 0xff) as f32 / 255.0,
            g: ((hex >> 8) & 0xff) as f32 / 255.0,
            b: (hex & 0xff) as f32 / 255.0,
            a: 1.0,
        }
    }
    pub const fn rgba(hex: u32, a: f32) -> Color {
        let mut c = Color::rgb(hex);
        c.a = a;
        c
    }
    pub const fn with_alpha(self, a: f32) -> Color {
        Color { a, ..self }
    }
    pub fn mix(a: Color, b: Color, t: f32) -> Color {
        let t = t.clamp(0.0, 1.0);
        Color {
            r: a.r + (b.r - a.r) * t,
            g: a.g + (b.g - a.g) * t,
            b: a.b + (b.b - a.b) * t,
            a: a.a + (b.a - a.a) * t,
        }
    }
    /// A colour from a hue (0..1), for a tile that has no picture of its own.
    pub fn hue(h: f32, s: f32, v: f32) -> Color {
        let h6 = (h.rem_euclid(1.0)) * 6.0;
        let i = h6.floor();
        let f = h6 - i;
        let p = v * (1.0 - s);
        let q = v * (1.0 - s * f);
        let t = v * (1.0 - s * (1.0 - f));
        let (r, g, b) = match i as i32 {
            0 => (v, t, p),
            1 => (q, v, p),
            2 => (p, v, t),
            3 => (p, q, v),
            4 => (t, p, v),
            _ => (v, p, q),
        };
        Color { r, g, b, a: 1.0 }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Rect {
    pub x: f32,
    pub y: f32,
    pub w: f32,
    pub h: f32,
}

impl Rect {
    pub const fn new(x: f32, y: f32, w: f32, h: f32) -> Rect {
        Rect { x, y, w, h }
    }
    pub fn cx(&self) -> f32 {
        self.x + self.w * 0.5
    }
    pub fn cy(&self) -> f32 {
        self.y + self.h * 0.5
    }
    pub fn inset(&self, d: f32) -> Rect {
        Rect::new(self.x + d, self.y + d, self.w - 2.0 * d, self.h - 2.0 * d)
    }
    /// Scaled about its centre.
    pub fn scaled(&self, s: f32) -> Rect {
        let w = self.w * s;
        let h = self.h * s;
        Rect::new(self.cx() - w * 0.5, self.cy() - h * 0.5, w, h)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Face {
    Regular = 0,
    SemiBold = 1,
    Display = 2,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Align {
    Left,
    Center,
    Right,
}

pub type ImageId = usize;

// Instance modes (params.y in the shader).
const MODE_SOLID: f32 = 0.0; // colour, alpha from the texture's red (the white pixel, or a glyph)
const MODE_IMAGE: f32 = 1.0; // the texture's RGBA times the colour
const MODE_STROKE: f32 = 2.0; // a ring of params.z pixels inside the edge
const MODE_SHADOW: f32 = 3.0; // soft: fades out over params.z pixels beyond the edge
const MODE_GRADIENT_V: f32 = 4.0; // colour at the top, color2 at the bottom
const MODE_GRADIENT_H: f32 = 5.0; // colour at the left, color2 at the right

const FLOATS_PER_INSTANCE: usize = 20;
const ATLAS_SIZE: u32 = 1024;
/// The atlas's top-left 4x4 is solid white: every untextured instance samples its middle.
const WHITE_UV: [f32; 4] = [2.0 / ATLAS_SIZE as f32, 2.0 / ATLAS_SIZE as f32, 2.0 / ATLAS_SIZE as f32, 2.0 / ATLAS_SIZE as f32];

const SHADER: &str = r#"
struct Globals {
    viewport: vec2<f32>,
    _pad: vec2<f32>,
};
@group(0) @binding(0) var<uniform> globals: Globals;
@group(1) @binding(0) var tex: texture_2d<f32>;
@group(1) @binding(1) var samp: sampler;

struct Inst {
    @location(0) rect: vec4<f32>,
    @location(1) uv: vec4<f32>,
    @location(2) color: vec4<f32>,
    @location(3) params: vec4<f32>,
    @location(4) color2: vec4<f32>,
};

struct VsOut {
    @builtin(position) pos: vec4<f32>,
    @location(0) local: vec2<f32>,
    @location(1) uv: vec2<f32>,
    @location(2) @interpolate(flat) half: vec2<f32>,
    @location(3) @interpolate(flat) color: vec4<f32>,
    @location(4) @interpolate(flat) params: vec4<f32>,
    @location(5) @interpolate(flat) color2: vec4<f32>,
};

@vertex
fn vs_main(@builtin(vertex_index) vi: u32, inst: Inst) -> VsOut {
    var corners = array<vec2<f32>, 6>(
        vec2<f32>(0.0, 0.0), vec2<f32>(1.0, 0.0), vec2<f32>(0.0, 1.0),
        vec2<f32>(1.0, 0.0), vec2<f32>(1.0, 1.0), vec2<f32>(0.0, 1.0),
    );
    let c = corners[vi];
    var expand = 0.0;
    if (inst.params.y == 3.0) {
        expand = inst.params.z;
    }
    let half = inst.rect.zw * 0.5 + vec2<f32>(expand, expand);
    let centre = inst.rect.xy + inst.rect.zw * 0.5;
    let local = (c - vec2<f32>(0.5, 0.5)) * 2.0 * half;
    let cs = cos(inst.params.w);
    let sn = sin(inst.params.w);
    let world = centre + vec2<f32>(local.x * cs - local.y * sn, local.x * sn + local.y * cs);
    var out: VsOut;
    out.pos = vec4<f32>(world.x / globals.viewport.x * 2.0 - 1.0, 1.0 - world.y / globals.viewport.y * 2.0, 0.0, 1.0);
    out.local = local;
    out.uv = inst.uv.xy + (inst.uv.zw - inst.uv.xy) * c;
    out.half = inst.rect.zw * 0.5;
    out.color = inst.color;
    out.params = inst.params;
    out.color2 = inst.color2;
    return out;
}

@fragment
fn fs_main(in: VsOut) -> @location(0) vec4<f32> {
    let r = min(in.params.x, min(in.half.x, in.half.y));
    let q = abs(in.local) - in.half + vec2<f32>(r, r);
    let d = length(max(q, vec2<f32>(0.0, 0.0))) + min(max(q.x, q.y), 0.0) - r;
    let mode = in.params.y;
    var color = in.color;
    if (mode == 4.0) {
        let t = clamp((in.local.y + in.half.y) / max(2.0 * in.half.y, 0.001), 0.0, 1.0);
        color = mix(in.color, in.color2, t);
    }
    if (mode == 5.0) {
        let t = clamp((in.local.x + in.half.x) / max(2.0 * in.half.x, 0.001), 0.0, 1.0);
        color = mix(in.color, in.color2, t);
    }
    var coverage = 1.0 - smoothstep(-0.75, 0.75, d);
    if (mode == 2.0) {
        let w = in.params.z;
        coverage = coverage * smoothstep(-w - 0.75, -w + 0.75, d);
    }
    if (mode == 3.0) {
        let b = max(in.params.z, 0.5);
        coverage = 1.0 - smoothstep(-b * 0.35, b, d);
    }
    let t = textureSample(tex, samp, in.uv);
    if (mode == 1.0) {
        color = color * t;
    } else {
        color.a = color.a * t.r;
    }
    return vec4<f32>(color.rgb, color.a * coverage);
}
"#;

#[derive(Clone, Copy)]
struct GlyphEntry {
    uv: [f32; 4],
    w: f32,
    h: f32,
    left: f32,
    top: f32,
}

struct Run {
    image: Option<ImageId>,
    start: u32,
    count: u32,
}

/// One frame's drawing, in order.
#[derive(Default)]
pub struct Scene {
    data: Vec<f32>,
    runs: Vec<Run>,
    opacity: Vec<f32>,
}

impl Default for Run {
    fn default() -> Self {
        Run {
            image: None,
            start: 0,
            count: 0,
        }
    }
}

impl Scene {
    pub fn new() -> Scene {
        Scene::default()
    }

    pub fn clear(&mut self) {
        self.data.clear();
        self.runs.clear();
        self.opacity.clear();
    }

    pub fn is_empty(&self) -> bool {
        self.runs.is_empty()
    }

    /// Everything pushed until `pop_opacity` is drawn `a` times as opaque.
    pub fn push_opacity(&mut self, a: f32) {
        let current = self.opacity.last().copied().unwrap_or(1.0);
        self.opacity.push(current * a);
    }

    pub fn pop_opacity(&mut self) {
        self.opacity.pop();
    }

    fn alpha(&self) -> f32 {
        self.opacity.last().copied().unwrap_or(1.0)
    }

    #[allow(clippy::too_many_arguments)]
    fn push(&mut self, image: Option<ImageId>, rect: Rect, uv: [f32; 4], color: Color, params: [f32; 4], color2: Color) {
        let a = self.alpha();
        self.data.extend_from_slice(&[
            rect.x, rect.y, rect.w, rect.h, uv[0], uv[1], uv[2], uv[3], color.r, color.g, color.b, color.a * a, params[0], params[1],
            params[2], params[3], color2.r, color2.g, color2.b, color2.a * a,
        ]);
        let index = (self.data.len() / FLOATS_PER_INSTANCE - 1) as u32;
        match self.runs.last_mut() {
            Some(run) if run.image == image => run.count += 1,
            _ => self.runs.push(Run {
                image,
                start: index,
                count: 1,
            }),
        }
    }

    pub fn rect(&mut self, r: Rect, radius: f32, color: Color) {
        self.push(None, r, WHITE_UV, color, [radius, MODE_SOLID, 0.0, 0.0], color);
    }

    /// A rectangle turned `angle` radians about its centre.
    pub fn rect_rotated(&mut self, r: Rect, radius: f32, angle: f32, color: Color) {
        self.push(None, r, WHITE_UV, color, [radius, MODE_SOLID, 0.0, angle], color);
    }

    /// A `width`-pixel line just inside the edge.
    pub fn stroke(&mut self, r: Rect, radius: f32, width: f32, color: Color) {
        self.push(None, r, WHITE_UV, color, [radius, MODE_STROKE, width, 0.0], color);
    }

    /// A soft shape: solid inside, fading out over `blur` pixels beyond the edge (a drop shadow
    /// when dark and offset, a glow when coloured).
    pub fn shadow(&mut self, r: Rect, radius: f32, blur: f32, color: Color) {
        self.push(None, r, WHITE_UV, color, [radius, MODE_SHADOW, blur, 0.0], color);
    }

    pub fn gradient_v(&mut self, r: Rect, radius: f32, top: Color, bottom: Color) {
        self.push(None, r, WHITE_UV, top, [radius, MODE_GRADIENT_V, 0.0, 0.0], bottom);
    }

    pub fn gradient_h(&mut self, r: Rect, radius: f32, left: Color, right: Color) {
        self.push(None, r, WHITE_UV, left, [radius, MODE_GRADIENT_H, 0.0, 0.0], right);
    }

    pub fn circle(&mut self, cx: f32, cy: f32, radius: f32, color: Color) {
        self.rect(Rect::new(cx - radius, cy - radius, 2.0 * radius, 2.0 * radius), radius, color);
    }

    pub fn ring(&mut self, cx: f32, cy: f32, radius: f32, width: f32, color: Color) {
        self.stroke(Rect::new(cx - radius, cy - radius, 2.0 * radius, 2.0 * radius), radius, width, color);
    }

    /// An image, stretched into `r`, its corners rounded, tinted (white = as is).
    pub fn image(&mut self, id: ImageId, r: Rect, radius: f32, tint: Color) {
        self.push(Some(id), r, [0.0, 0.0, 1.0, 1.0], tint, [radius, MODE_IMAGE, 0.0, 0.0], tint);
    }

    /// One line of text with its baseline at `baseline`; `x` is the left edge, centre or right
    /// edge per `align`. `tracking` is extra space between glyphs. Returns the width.
    #[allow(clippy::too_many_arguments)]
    pub fn text(&mut self, ui: &mut Ui, face: Face, size: f32, text: &str, x: f32, baseline: f32, color: Color, align: Align, tracking: f32) -> f32 {
        let width = ui.measure(face, size, text, tracking);
        let mut pen = match align {
            Align::Left => x,
            Align::Center => x - width * 0.5,
            Align::Right => x - width,
        }
        .round();
        let baseline = baseline.round();
        let mut prev: Option<GlyphId> = None;
        for ch in text.chars() {
            let (id, advance, kern) = ui.advance(face, size, ch, prev);
            pen += kern;
            if let Some(g) = ui.glyph(face, size, ch) {
                if g.w > 0.0 {
                    let r = Rect::new((pen + g.left).round(), baseline + g.top, g.w, g.h);
                    self.push(None, r, g.uv, color, [0.0, MODE_SOLID, 0.0, 0.0], color);
                }
            }
            pen += advance + tracking;
            prev = Some(id);
        }
        width
    }
}

struct Atlas {
    texture: wgpu::Texture,
    next_x: u32,
    next_y: u32,
    row_h: u32,
    /// Bitmaps placed since the last render, to upload: (x, y, w, h, pixels).
    pending: Vec<(u32, u32, u32, u32, Vec<u8>)>,
}

impl Atlas {
    /// Reserve a w x h cell; None when the atlas is full (text then simply stops appearing).
    fn place(&mut self, w: u32, h: u32) -> Option<(u32, u32)> {
        if self.next_x + w > ATLAS_SIZE {
            self.next_x = 0;
            self.next_y += self.row_h + 1;
            self.row_h = 0;
        }
        if self.next_y + h > ATLAS_SIZE || w > ATLAS_SIZE {
            return None;
        }
        let at = (self.next_x, self.next_y);
        self.next_x += w + 1;
        self.row_h = self.row_h.max(h);
        Some(at)
    }
}

pub struct Ui {
    pipeline: wgpu::RenderPipeline,
    texture_layout: wgpu::BindGroupLayout,
    sampler: wgpu::Sampler,
    globals_group: wgpu::BindGroup,
    atlas: Atlas,
    atlas_group: wgpu::BindGroup,
    images: Vec<wgpu::BindGroup>,
    instances: Option<wgpu::Buffer>,
    instance_capacity: usize,
    fonts: [FontRef<'static>; 3],
    glyphs: HashMap<(Face, char, u32), Option<GlyphEntry>>,
}

impl Ui {
    pub fn new(device: &wgpu::Device, queue: &wgpu::Queue, format: wgpu::TextureFormat, width: u32, height: u32) -> Result<Ui, String> {
        let fonts = [
            FontRef::try_from_slice(include_bytes!("../fonts/Inter-Regular.ttf")).map_err(|e| format!("Inter Regular: {e}"))?,
            FontRef::try_from_slice(include_bytes!("../fonts/Inter-SemiBold.ttf")).map_err(|e| format!("Inter SemiBold: {e}"))?,
            FontRef::try_from_slice(include_bytes!("../fonts/Montserrat-Medium.ttf")).map_err(|e| format!("Montserrat Medium: {e}"))?,
        ];
        let module = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("ui"),
            source: wgpu::ShaderSource::Wgsl(SHADER.into()),
        });
        let globals_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("ui globals"),
            entries: &[wgpu::BindGroupLayoutEntry {
                binding: 0,
                visibility: wgpu::ShaderStages::VERTEX,
                ty: wgpu::BindingType::Buffer {
                    ty: wgpu::BufferBindingType::Uniform,
                    has_dynamic_offset: false,
                    min_binding_size: wgpu::BufferSize::new(16),
                },
                count: None,
            }],
        });
        let texture_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("ui texture"),
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
        let globals = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("ui globals"),
            size: 16,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let g: [f32; 4] = [width as f32, height as f32, 0.0, 0.0];
        let bytes: Vec<u8> = g.iter().flat_map(|f| f.to_ne_bytes()).collect();
        queue.write_buffer(&globals, 0, &bytes);
        let globals_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("ui globals"),
            layout: &globals_layout,
            entries: &[wgpu::BindGroupEntry {
                binding: 0,
                resource: globals.as_entire_binding(),
            }],
        });
        let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("ui"),
            address_mode_u: wgpu::AddressMode::ClampToEdge,
            address_mode_v: wgpu::AddressMode::ClampToEdge,
            address_mode_w: wgpu::AddressMode::ClampToEdge,
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            mipmap_filter: wgpu::MipmapFilterMode::Nearest,
            ..Default::default()
        });
        let atlas_texture = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("ui atlas"),
            size: wgpu::Extent3d {
                width: ATLAS_SIZE,
                height: ATLAS_SIZE,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::R8Unorm,
            usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
            view_formats: &[],
        });
        let atlas_group = Self::texture_group(device, &texture_layout, &sampler, &atlas_texture, "ui atlas");
        let mut atlas = Atlas {
            texture: atlas_texture,
            next_x: 0,
            next_y: 0,
            row_h: 0,
            pending: Vec::new(),
        };
        // The white block every plain shape samples.
        let _ = atlas.place(4, 4);
        atlas.pending.push((0, 0, 4, 4, vec![255u8; 16]));

        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("ui"),
            bind_group_layouts: &[Some(&globals_layout), Some(&texture_layout)],
            immediate_size: 0,
        });
        let attributes = [
            wgpu::VertexAttribute {
                format: wgpu::VertexFormat::Float32x4,
                offset: 0,
                shader_location: 0,
            },
            wgpu::VertexAttribute {
                format: wgpu::VertexFormat::Float32x4,
                offset: 16,
                shader_location: 1,
            },
            wgpu::VertexAttribute {
                format: wgpu::VertexFormat::Float32x4,
                offset: 32,
                shader_location: 2,
            },
            wgpu::VertexAttribute {
                format: wgpu::VertexFormat::Float32x4,
                offset: 48,
                shader_location: 3,
            },
            wgpu::VertexAttribute {
                format: wgpu::VertexFormat::Float32x4,
                offset: 64,
                shader_location: 4,
            },
        ];
        let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("ui"),
            layout: Some(&pipeline_layout),
            vertex: wgpu::VertexState {
                module: &module,
                entry_point: Some("vs_main"),
                buffers: &[Some(wgpu::VertexBufferLayout {
                    array_stride: (FLOATS_PER_INSTANCE * 4) as u64,
                    step_mode: wgpu::VertexStepMode::Instance,
                    attributes: &attributes,
                })],
                compilation_options: Default::default(),
            },
            fragment: Some(wgpu::FragmentState {
                module: &module,
                entry_point: Some("fs_main"),
                targets: &[Some(wgpu::ColorTargetState {
                    format,
                    blend: Some(wgpu::BlendState::ALPHA_BLENDING),
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
        Ok(Ui {
            pipeline,
            texture_layout,
            sampler,
            globals_group,
            atlas,
            atlas_group,
            images: Vec::new(),
            instances: None,
            instance_capacity: 0,
            fonts,
            glyphs: HashMap::new(),
        })
    }

    fn texture_group(device: &wgpu::Device, layout: &wgpu::BindGroupLayout, sampler: &wgpu::Sampler, texture: &wgpu::Texture, label: &str) -> wgpu::BindGroup {
        let view = texture.create_view(&wgpu::TextureViewDescriptor::default());
        device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some(label),
            layout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: wgpu::BindingResource::TextureView(&view),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: wgpu::BindingResource::Sampler(sampler),
                },
            ],
        })
    }

    /// An RGBA8 image the scenes can draw by id.
    pub fn add_image(&mut self, device: &wgpu::Device, queue: &wgpu::Queue, width: u32, height: u32, rgba: &[u8]) -> ImageId {
        let texture = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("ui image"),
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
        queue.write_texture(
            wgpu::TexelCopyTextureInfo {
                texture: &texture,
                mip_level: 0,
                origin: wgpu::Origin3d::ZERO,
                aspect: wgpu::TextureAspect::All,
            },
            rgba,
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
        let group = Self::texture_group(device, &self.texture_layout, &self.sampler, &texture, "ui image");
        self.images.push(group);
        self.images.len() - 1
    }

    fn font(&self, face: Face) -> &FontRef<'static> {
        &self.fonts[face as usize]
    }

    /// The glyph id, its advance and the kerning after `prev`.
    fn advance(&self, face: Face, size: f32, ch: char, prev: Option<GlyphId>) -> (GlyphId, f32, f32) {
        let font = self.font(face);
        let scaled = font.as_scaled(PxScale::from(size));
        let id = font.glyph_id(ch);
        let kern = prev.map(|p| scaled.kern(p, id)).unwrap_or(0.0);
        (id, scaled.h_advance(id), kern)
    }

    /// The glyph's cell in the atlas, rasterised on first use. None for a glyph with no outline
    /// (a space) or when the atlas is full.
    fn glyph(&mut self, face: Face, size: f32, ch: char) -> Option<GlyphEntry> {
        let key = (face, ch, (size * 4.0).round() as u32);
        if let Some(entry) = self.glyphs.get(&key) {
            return *entry;
        }
        let font = &self.fonts[face as usize];
        let glyph = font.glyph_id(ch).with_scale_and_position(PxScale::from(size), point(0.0, 0.0));
        let entry = font.outline_glyph(glyph).and_then(|outlined| {
            let bounds = outlined.px_bounds();
            let w = (bounds.max.x - bounds.min.x).ceil().max(0.0) as u32;
            let h = (bounds.max.y - bounds.min.y).ceil().max(0.0) as u32;
            if w == 0 || h == 0 {
                return None;
            }
            // One transparent pixel of padding around the bitmap, so the quad's anti-aliased edge
            // never cuts into the glyph.
            let (cw, chh) = (w + 2, h + 2);
            let (ax, ay) = self.atlas.place(cw, chh)?;
            let mut pixels = vec![0u8; (cw * chh) as usize];
            outlined.draw(|x, y, c| {
                if x < w && y < h {
                    pixels[((y + 1) * cw + x + 1) as usize] = (c.clamp(0.0, 1.0) * 255.0) as u8;
                }
            });
            self.atlas.pending.push((ax, ay, cw, chh, pixels));
            let s = ATLAS_SIZE as f32;
            Some(GlyphEntry {
                uv: [ax as f32 / s, ay as f32 / s, (ax + cw) as f32 / s, (ay + chh) as f32 / s],
                w: cw as f32,
                h: chh as f32,
                left: bounds.min.x - 1.0,
                top: bounds.min.y - 1.0,
            })
        });
        self.glyphs.insert(key, entry);
        entry
    }

    /// The width of one line.
    pub fn measure(&self, face: Face, size: f32, text: &str, tracking: f32) -> f32 {
        let mut width = 0.0;
        let mut prev = None;
        let mut n = 0;
        for ch in text.chars() {
            let (id, advance, kern) = self.advance(face, size, ch, prev);
            width += kern + advance + tracking;
            prev = Some(id);
            n += 1;
        }
        if n > 0 {
            width -= tracking;
        }
        width
    }

    /// `text` cut to fit `width`, ending in an ellipsis when it had to be.
    pub fn fit(&self, face: Face, size: f32, text: &str, width: f32) -> String {
        if self.measure(face, size, text, 0.0) <= width {
            return text.to_string();
        }
        let chars: Vec<char> = text.chars().collect();
        let mut n = chars.len();
        while n > 0 {
            n -= 1;
            let candidate: String = chars[..n].iter().collect::<String>().trim_end().to_string() + "\u{2026}";
            if self.measure(face, size, &candidate, 0.0) <= width {
                return candidate;
            }
        }
        "\u{2026}".to_string()
    }

    /// The face's ascent at `size` (baseline = top + ascent).
    pub fn ascent(&self, face: Face, size: f32) -> f32 {
        self.font(face).as_scaled(PxScale::from(size)).ascent()
    }

    /// Draw `scene` into `view`: cleared to `clear` first when given, over what is there otherwise.
    pub fn render(&mut self, device: &wgpu::Device, queue: &wgpu::Queue, view: &wgpu::TextureView, clear: Option<Color>, scene: &Scene) -> wgpu::CommandBuffer {
        for (x, y, w, h, pixels) in self.atlas.pending.drain(..) {
            queue.write_texture(
                wgpu::TexelCopyTextureInfo {
                    texture: &self.atlas.texture,
                    mip_level: 0,
                    origin: wgpu::Origin3d { x, y, z: 0 },
                    aspect: wgpu::TextureAspect::All,
                },
                &pixels,
                wgpu::TexelCopyBufferLayout {
                    offset: 0,
                    bytes_per_row: Some(w),
                    rows_per_image: Some(h),
                },
                wgpu::Extent3d {
                    width: w,
                    height: h,
                    depth_or_array_layers: 1,
                },
            );
        }
        let count = scene.data.len() / FLOATS_PER_INSTANCE;
        if count > 0 {
            if count > self.instance_capacity {
                let capacity = (count * 2).max(1024);
                self.instances = Some(device.create_buffer(&wgpu::BufferDescriptor {
                    label: Some("ui instances"),
                    size: (capacity * FLOATS_PER_INSTANCE * 4) as u64,
                    usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
                    mapped_at_creation: false,
                }));
                self.instance_capacity = capacity;
            }
            let bytes: Vec<u8> = scene.data.iter().flat_map(|f| f.to_ne_bytes()).collect();
            queue.write_buffer(self.instances.as_ref().expect("instance buffer"), 0, &bytes);
        }
        let mut enc = device.create_command_encoder(&wgpu::CommandEncoderDescriptor { label: Some("ui") });
        {
            let load = match clear {
                Some(c) => wgpu::LoadOp::Clear(wgpu::Color {
                    r: c.r as f64,
                    g: c.g as f64,
                    b: c.b as f64,
                    a: 1.0,
                }),
                None => wgpu::LoadOp::Load,
            };
            let mut pass = enc.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("ui"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view,
                    depth_slice: None,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load,
                        store: wgpu::StoreOp::Store,
                    },
                })],
                depth_stencil_attachment: None,
                timestamp_writes: None,
                occlusion_query_set: None,
                multiview_mask: None,
            });
            if count > 0 {
                pass.set_pipeline(&self.pipeline);
                pass.set_bind_group(0, &self.globals_group, &[]);
                pass.set_vertex_buffer(0, self.instances.as_ref().expect("instance buffer").slice(..));
                for run in &scene.runs {
                    let group = match run.image {
                        Some(id) => self.images.get(id).unwrap_or(&self.atlas_group),
                        None => &self.atlas_group,
                    };
                    pass.set_bind_group(1, group, &[]);
                    pass.draw(0..6, run.start..run.start + run.count);
                }
            }
        }
        enc.finish()
    }
}

/// A PNG's pixels as RGBA8 (any colour type), for `Ui::add_image`.
pub fn decode_png(bytes: &[u8]) -> Result<(u32, u32, Vec<u8>), String> {
    use png::{ColorType, Transformations};
    let mut decoder = png::Decoder::new(std::io::Cursor::new(bytes));
    decoder.set_transformations(Transformations::normalize_to_color8());
    let mut reader = decoder.read_info().map_err(|e| e.to_string())?;
    let size = reader.output_buffer_size().ok_or_else(|| "too large".to_string())?;
    let mut data = vec![0u8; size];
    let info = reader.next_frame(&mut data).map_err(|e| e.to_string())?;
    let data = &data[..info.buffer_size()];
    let rgba: Vec<u8> = match info.color_type {
        ColorType::Rgba => data.to_vec(),
        ColorType::Rgb => data.chunks_exact(3).flat_map(|p| [p[0], p[1], p[2], 255]).collect(),
        ColorType::Grayscale => data.iter().flat_map(|&v| [v, v, v, 255]).collect(),
        ColorType::GrayscaleAlpha => data.chunks_exact(2).flat_map(|p| [p[0], p[0], p[0], p[1]]).collect(),
        other => return Err(format!("unsupported colour type {other:?}")),
    };
    Ok((info.width, info.height, rgba))
}
