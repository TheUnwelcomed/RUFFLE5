//! PS5_RUFFLE_CURSOR_V2 — a mouse pointer drawn over Ruffle's frame.
//!
//! Ruffle draws no cursor (a desktop's window system does that), so on the TV there was nothing to
//! show where the touchpad had put the mouse. `CursorPainter` paints an arrow at the pointer in one
//! render pass that loads what Ruffle drew; src/shell.rs runs it at submit time, before the frame
//! is presented. The arrow is 15 vertices in the shader (no vertex buffer), drawn five times by
//! instancing: four black copies offset by two pixels make the outline, the fifth, white, is the
//! pointer. `Cursor` is the shared state the input side updates (position, visible).
// SPDX-License-Identifier: GPL-3.0-or-later

use std::sync::{Arc, Mutex};

#[derive(Debug, Clone, Copy)]
pub struct Cursor {
    pub x: f32,
    pub y: f32,
    pub visible: bool,
    pub scale: f32,
}

pub type SharedCursor = Arc<Mutex<Cursor>>;

pub fn shared(x: f32, y: f32, scale: f32) -> SharedCursor {
    Arc::new(Mutex::new(Cursor {
        x,
        y,
        visible: false,
        scale,
    }))
}

const SHADER: &str = r#"
struct Params {
    pos: vec2<f32>,
    viewport: vec2<f32>,
    scale: f32,
    _pad0: f32,
    _pad1: vec2<f32>,
};
@group(0) @binding(0) var<uniform> params: Params;

// The arrow: polygon (0,0) (0,17) (4,13) (7,20) (10,19) (7,12) (12,12), hotspot at (0,0),
// as five triangles.
var<private> SHAPE: array<vec2<f32>, 15> = array<vec2<f32>, 15>(
    vec2<f32>(0.0, 0.0), vec2<f32>(0.0, 17.0), vec2<f32>(4.0, 13.0),
    vec2<f32>(0.0, 0.0), vec2<f32>(4.0, 13.0), vec2<f32>(7.0, 12.0),
    vec2<f32>(0.0, 0.0), vec2<f32>(7.0, 12.0), vec2<f32>(12.0, 12.0),
    vec2<f32>(4.0, 13.0), vec2<f32>(7.0, 20.0), vec2<f32>(10.0, 19.0),
    vec2<f32>(4.0, 13.0), vec2<f32>(10.0, 19.0), vec2<f32>(7.0, 12.0),
);
var<private> OFFSETS: array<vec2<f32>, 5> = array<vec2<f32>, 5>(
    vec2<f32>(-2.0, 0.0), vec2<f32>(2.0, 0.0), vec2<f32>(0.0, -2.0), vec2<f32>(0.0, 2.0),
    vec2<f32>(0.0, 0.0),
);

struct VsOut {
    @builtin(position) pos: vec4<f32>,
    @location(0) color: vec4<f32>,
};

@vertex
fn vs_main(@builtin(vertex_index) vi: u32, @builtin(instance_index) ii: u32) -> VsOut {
    let p = SHAPE[vi] * params.scale + OFFSETS[ii] + params.pos;
    let ndc = vec2<f32>(p.x / params.viewport.x * 2.0 - 1.0, 1.0 - p.y / params.viewport.y * 2.0);
    var out: VsOut;
    out.pos = vec4<f32>(ndc, 0.0, 1.0);
    if (ii < 4u) {
        out.color = vec4<f32>(0.0, 0.0, 0.0, 1.0);
    } else {
        out.color = vec4<f32>(1.0, 1.0, 1.0, 1.0);
    }
    return out;
}

@fragment
fn fs_main(in: VsOut) -> @location(0) vec4<f32> {
    return in.color;
}
"#;

#[derive(Debug)]
pub struct CursorPainter {
    pipeline: wgpu::RenderPipeline,
    uniform: wgpu::Buffer,
    bind_group: wgpu::BindGroup,
    width: u32,
    height: u32,
}

impl CursorPainter {
    pub fn new(device: &wgpu::Device, format: wgpu::TextureFormat, width: u32, height: u32) -> Self {
        let module = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("cursor"),
            source: wgpu::ShaderSource::Wgsl(SHADER.into()),
        });
        let layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("cursor params"),
            entries: &[wgpu::BindGroupLayoutEntry {
                binding: 0,
                visibility: wgpu::ShaderStages::VERTEX,
                ty: wgpu::BindingType::Buffer {
                    ty: wgpu::BufferBindingType::Uniform,
                    has_dynamic_offset: false,
                    min_binding_size: wgpu::BufferSize::new(32),
                },
                count: None,
            }],
        });
        let uniform = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("cursor params"),
            size: 32,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("cursor params"),
            layout: &layout,
            entries: &[wgpu::BindGroupEntry {
                binding: 0,
                resource: uniform.as_entire_binding(),
            }],
        });
        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("cursor"),
            bind_group_layouts: &[Some(&layout)],
            immediate_size: 0,
        });
        let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("cursor"),
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
        Self {
            pipeline,
            uniform,
            bind_group,
            width,
            height,
        }
    }

    /// The pass that paints the arrow over `view`, or None when the cursor is hidden.
    pub fn paint(&self, device: &wgpu::Device, queue: &wgpu::Queue, view: &wgpu::TextureView, c: Cursor) -> Option<wgpu::CommandBuffer> {
        if !c.visible {
            return None;
        }
        let params: [f32; 8] = [
            c.x.round(),
            c.y.round(),
            self.width as f32,
            self.height as f32,
            c.scale,
            0.0,
            0.0,
            0.0,
        ];
        let bytes: Vec<u8> = params.iter().flat_map(|f| f.to_ne_bytes()).collect();
        queue.write_buffer(&self.uniform, 0, &bytes);
        let mut enc = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
            label: Some("cursor"),
        });
        {
            let mut pass = enc.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("cursor"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view,
                    depth_slice: None,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Load,
                        store: wgpu::StoreOp::Store,
                    },
                })],
                depth_stencil_attachment: None,
                timestamp_writes: None,
                occlusion_query_set: None,
                multiview_mask: None,
            });
            pass.set_pipeline(&self.pipeline);
            pass.set_bind_group(0, &self.bind_group, &[]);
            pass.draw(0..15, 0..5);
        }
        Some(enc.finish())
    }
}
