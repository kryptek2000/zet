// wgpu renderer: window surface, glyph atlas, textured text pipeline.
// term -> quads -> atlas texture -> present. M0: per-frame vertex upload,
// monochrome coverage tinted by cell color; LRU/persistent buffers later.

use std::collections::HashMap;
use std::fmt;
use std::sync::Arc;
use winit::window::Window;

use crate::atlas::{Atlas, Slot};
use crate::quad::{cell_quad, Rgba, Vertex};
use crate::text::Shaper;

const ATLAS_COLS: u32 = 16;
const ATLAS_ROWS: u32 = 16;

const SHADER: &str = r#"
struct Uniforms { screen: vec2<f32> };
@group(0) @binding(0) var<uniform> u: Uniforms;
@group(1) @binding(0) var atlas_tex: texture_2d<f32>;
@group(1) @binding(1) var atlas_smp: sampler;

struct VsOut {
    @builtin(position) pos: vec4<f32>,
    @location(0) uv: vec2<f32>,
    @location(1) color: vec4<f32>,
};

@vertex
fn vs(@location(0) p: vec2<f32>, @location(1) uv: vec2<f32>,
       @location(2) col: vec4<f32>) -> VsOut {
    var o: VsOut;
    o.pos = vec4<f32>(p.x / u.screen.x * 2.0 - 1.0, 1.0 - p.y / u.screen.y * 2.0, 0.0, 1.0);
    o.uv = uv;
    o.color = col;
    return o;
}

@fragment
fn fs(in: VsOut) -> @location(0) vec4<f32> {
    let tex = textureSample(atlas_tex, atlas_smp, in.uv);
    return vec4<f32>(in.color.rgb, in.color.a * tex.a);
}
"#;

#[derive(Debug)]
pub enum InitError {
    NoAdapter,
    NoSurfaceConfig,
    Device(wgpu::RequestDeviceError),
    Surface(wgpu::CreateSurfaceError),
}

impl fmt::Display for InitError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            InitError::NoAdapter => write!(f, "no suitable GPU adapter found"),
            InitError::NoSurfaceConfig => write!(f, "no surface configuration available"),
            InitError::Device(e) => write!(f, "device request failed: {e}"),
            InitError::Surface(e) => write!(f, "surface creation failed: {e}"),
        }
    }
}

impl std::error::Error for InitError {}

/// Map a terminal cell color to linear-ish display RGB.
/// M0 palette: classic 16 ANSI colors; Rgb passes through normalized.
pub fn term_color(color: zet_term::Color) -> Rgba {
    const PALETTE: [(f32, f32, f32); 16] = [
        (0.10, 0.10, 0.10), // 0 black
        (0.80, 0.20, 0.20), // 1 red
        (0.20, 0.70, 0.20), // 2 green
        (0.80, 0.70, 0.20), // 3 yellow
        (0.25, 0.40, 0.90), // 4 blue
        (0.70, 0.30, 0.80), // 5 magenta
        (0.20, 0.75, 0.80), // 6 cyan
        (0.85, 0.85, 0.85), // 7 white
        (0.40, 0.40, 0.40), // 8 bright black
        (1.00, 0.35, 0.35), // 9 bright red
        (0.35, 0.95, 0.35), // 10 bright green
        (1.00, 0.90, 0.40), // 11 bright yellow
        (0.45, 0.60, 1.00), // 12 bright blue
        (0.90, 0.50, 1.00), // 13 bright magenta
        (0.40, 0.95, 1.00), // 14 bright cyan
        (1.00, 1.00, 1.00), // 15 bright white
    ];
    match color {
        zet_term::Color::Default => Rgba::WHITE,
        zet_term::Color::Indexed(i) => {
            let (r, g, b) = PALETTE[(i % 16) as usize];
            Rgba::rgb(r, g, b)
        }
        zet_term::Color::Rgb(r, g, b) => {
            Rgba::rgb(r as f32 / 255.0, g as f32 / 255.0, b as f32 / 255.0)
        }
    }
}

pub struct Renderer {
    surface: wgpu::Surface<'static>,
    device: wgpu::Device,
    queue: wgpu::Queue,
    config: wgpu::SurfaceConfiguration,
    bg: wgpu::Color,
    pipeline: wgpu::RenderPipeline,
    uniform_buf: wgpu::Buffer,
    uniform_group: wgpu::BindGroup,
    text_group: wgpu::BindGroup,
    shaper: Shaper,
    cell_w: f32,
    cell_h: f32,
    atlas_alloc: Atlas,
    atlas_tex: wgpu::Texture,
    atlas_size: (u32, u32),
    coverage: HashMap<char, Vec<(i32, i32, u8)>>,
    slots: HashMap<char, Slot>,
    quads: Vec<Vertex>,
}

impl Renderer {
    pub async fn new(window: Arc<Window>) -> Result<Self, InitError> {
        let size = window.inner_size();
        let instance = wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle());
        let surface = instance
            .create_surface(window)
            .map_err(InitError::Surface)?;
        let adapter = instance
            .request_adapter(&wgpu::RequestAdapterOptions {
                power_preference: wgpu::PowerPreference::HighPerformance,
                compatible_surface: Some(&surface),
                force_fallback_adapter: false,
                ..Default::default()
            })
            .await
            .map_err(|_| InitError::NoAdapter)?;
        let (device, queue) = adapter
            .request_device(&wgpu::DeviceDescriptor::default())
            .await
            .map_err(InitError::Device)?;
        let config = surface
            .get_default_config(&adapter, size.width.max(1), size.height.max(1))
            .ok_or(InitError::NoSurfaceConfig)?;
        surface.configure(&device, &config);

        let mut shaper = Shaper::new(16.0);
        let adv = shaper.shape_line("M")[0].advance.ceil().max(1.0);
        let cell_w = adv;
        let cell_h = shaper.cell_size().1.ceil().max(1.0);
        let (cw, ch) = (cell_w as u32, cell_h as u32);
        let atlas_w = ATLAS_COLS * cw;
        let atlas_h = ATLAS_ROWS * ch;
        let atlas_tex = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("zet-atlas"),
            size: wgpu::Extent3d {
                width: atlas_w,
                height: atlas_h,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::Rgba8Unorm,
            usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
            view_formats: &[],
        });

        let uniform_buf = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("zet-uniform"),
            size: 8,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let uniform_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("zet-uniform-layout"),
            entries: &[wgpu::BindGroupLayoutEntry {
                binding: 0,
                visibility: wgpu::ShaderStages::VERTEX,
                ty: wgpu::BindingType::Buffer {
                    ty: wgpu::BufferBindingType::Uniform,
                    has_dynamic_offset: false,
                    min_binding_size: None,
                },
                count: None,
            }],
        });
        let uniform_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("zet-uniform-group"),
            layout: &uniform_layout,
            entries: &[wgpu::BindGroupEntry {
                binding: 0,
                resource: uniform_buf.as_entire_binding(),
            }],
        });

        let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("zet-atlas-sampler"),
            address_mode_u: wgpu::AddressMode::ClampToEdge,
            address_mode_v: wgpu::AddressMode::ClampToEdge,
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            ..Default::default()
        });
        let atlas_view = atlas_tex.create_view(&wgpu::TextureViewDescriptor::default());
        let text_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("zet-text-layout"),
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
        let text_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("zet-text-group"),
            layout: &text_layout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: wgpu::BindingResource::TextureView(&atlas_view),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: wgpu::BindingResource::Sampler(&sampler),
                },
            ],
        });

        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("zet-text-shader"),
            source: wgpu::ShaderSource::Wgsl(std::borrow::Cow::Borrowed(SHADER)),
        });
        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("zet-pipeline-layout"),
            bind_group_layouts: &[Some(&uniform_layout), Some(&text_layout)],
            immediate_size: 0,
        });
        let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("zet-text-pipeline"),
            layout: Some(&pipeline_layout),
            vertex: wgpu::VertexState {
                module: &shader,
                entry_point: Some("vs"),
                buffers: &[Some(wgpu::VertexBufferLayout {
                    array_stride: 32,
                    step_mode: wgpu::VertexStepMode::Vertex,
                    attributes: &Vertex::LAYOUT,
                })],
                compilation_options: wgpu::PipelineCompilationOptions::default(),
            },
            fragment: Some(wgpu::FragmentState {
                module: &shader,
                entry_point: Some("fs"),
                targets: &[Some(wgpu::ColorTargetState {
                    format: config.format,
                    blend: Some(wgpu::BlendState::ALPHA_BLENDING),
                    write_mask: wgpu::ColorWrites::ALL,
                })],
                compilation_options: wgpu::PipelineCompilationOptions::default(),
            }),
            primitive: wgpu::PrimitiveState::default(),
            depth_stencil: None,
            multisample: wgpu::MultisampleState::default(),
            multiview_mask: None,
            cache: None,
        });

        let renderer = Renderer {
            surface,
            device,
            queue,
            config,
            bg: wgpu::Color {
                r: 0.008,
                g: 0.012,
                b: 0.027,
                a: 1.0,
            },
            pipeline,
            uniform_buf,
            uniform_group,
            text_group,
            shaper,
            cell_w,
            cell_h,
            atlas_alloc: Atlas::new(ATLAS_COLS, ATLAS_ROWS, cw, ch),
            atlas_tex,
            atlas_size: (atlas_w, atlas_h),
            coverage: HashMap::new(),
            slots: HashMap::new(),
            quads: Vec::new(),
        };
        renderer.write_uniform(size.width.max(1), size.height.max(1));
        Ok(renderer)
    }

    fn write_uniform(&self, width: u32, height: u32) {
        let data = [width as f32, height as f32];
        self.queue
            .write_buffer(&self.uniform_buf, 0, bytemuck::cast_slice(&data));
    }

    pub fn resize(&mut self, width: u32, height: u32) {
        self.config.width = width.max(1);
        self.config.height = height.max(1);
        self.surface.configure(&self.device, &self.config);
        self.write_uniform(self.config.width, self.config.height);
    }

    fn slot_for(&mut self, c: char) -> Option<Slot> {
        if let Some(&slot) = self.slots.get(&c) {
            return Some(slot);
        }
        let cov = self.coverage.entry(c).or_insert_with(|| {
            let shaped = self.shaper.shape_line(&c.to_string());
            shaped
                .into_iter()
                .next()
                .map(|g| g.pixels)
                .unwrap_or_default()
        });
        if cov.is_empty() {
            return None;
        }
        let slot = match self.atlas_alloc.alloc() {
            Some(s) => s,
            None => {
                // Atlas full: reset and start over (LRU is a follow-up).
                self.atlas_alloc = Atlas::new(
                    ATLAS_COLS,
                    ATLAS_ROWS,
                    self.cell_w as u32,
                    self.cell_h as u32,
                );
                self.slots.clear();
                self.atlas_alloc.alloc()?
            }
        };
        let (cw, ch) = (self.cell_w as usize, self.cell_h as usize);
        let mut rgba = vec![0u8; cw * ch * 4];
        for &(x, y, alpha) in cov.iter() {
            if x >= 0 && y >= 0 && (x as usize) < cw && (y as usize) < ch {
                let i = ((y as usize) * cw + x as usize) * 4;
                rgba[i] = 255;
                rgba[i + 1] = 255;
                rgba[i + 2] = 255;
                rgba[i + 3] = alpha;
            }
        }
        self.queue.write_texture(
            wgpu::TexelCopyTextureInfo {
                texture: &self.atlas_tex,
                mip_level: 0,
                origin: wgpu::Origin3d {
                    x: slot.x,
                    y: slot.y,
                    z: 0,
                },
                aspect: wgpu::TextureAspect::All,
            },
            &rgba,
            wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some((cw * 4) as u32),
                rows_per_image: Some(ch as u32),
            },
            wgpu::Extent3d {
                width: cw as u32,
                height: ch as u32,
                depth_or_array_layers: 1,
            },
        );
        self.slots.insert(c, slot);
        Some(slot)
    }

    /// Stage terminal grid cells as textured quads for the next render().
    /// M0: one glyph per cell; CJK/combining alignment is a follow-up.
    pub fn draw_terminal(&mut self, term: &zet_term::Terminal, cols: usize, rows: usize) {
        self.quads.clear();
        for y in 0..rows {
            let line: Vec<char> = term.line_text(y).chars().collect();
            for (x, &c) in line.iter().enumerate().take(cols) {
                if c == ' ' {
                    continue;
                }
                let color = term.cell(x, y).map(|cell| cell.fg).unwrap_or_default();
                let Some(slot) = self.slot_for(c) else {
                    continue;
                };
                self.quads.extend(cell_quad(
                    x as u32,
                    y as u32,
                    self.cell_w,
                    self.cell_h,
                    (slot.x, slot.y),
                    self.atlas_size,
                    term_color(color),
                ));
            }
        }
    }

    pub fn render(&mut self) {
        use wgpu::CurrentSurfaceTexture as Cst;
        let frame = match self.surface.get_current_texture() {
            Cst::Success(frame) | Cst::Suboptimal(frame) => frame,
            Cst::Outdated | Cst::Lost => {
                self.surface.configure(&self.device, &self.config);
                return;
            }
            Cst::Timeout | Cst::Occluded | Cst::Validation => return,
        };
        let view = frame
            .texture
            .create_view(&wgpu::TextureViewDescriptor::default());
        let vbuf = self.device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("zet-quads"),
            size: (self.quads.len() * 32).max(32) as u64,
            usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        if !self.quads.is_empty() {
            self.queue
                .write_buffer(&vbuf, 0, bytemuck::cast_slice(&self.quads));
        }
        let mut encoder = self
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                label: Some("zet-frame"),
            });
        {
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("zet-text"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &view,
                    resolve_target: None,
                    depth_slice: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(self.bg),
                        store: wgpu::StoreOp::Store,
                    },
                })],
                depth_stencil_attachment: None,
                timestamp_writes: None,
                occlusion_query_set: None,
                multiview_mask: None,
            });
            if !self.quads.is_empty() {
                pass.set_pipeline(&self.pipeline);
                pass.set_bind_group(0, &self.uniform_group, &[]);
                pass.set_bind_group(1, &self.text_group, &[]);
                pass.set_vertex_buffer(0, vbuf.slice(..));
                pass.draw(0..self.quads.len() as u32, 0..1);
            }
        }
        self.queue.submit(Some(encoder.finish()));
        self.queue.present(frame);
    }
}
