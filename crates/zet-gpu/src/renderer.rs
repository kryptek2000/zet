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

/// Renderer clear color as a quad tint (for inverted cursor glyphs).
const BG_RGBA: Rgba = Rgba {
    r: 0.008,
    g: 0.012,
    b: 0.027,
    a: 1.0,
};

/// Ghost-text ink: dimmed so suggestions read as preview, not content.
const GHOST_RGBA: Rgba = Rgba {
    r: 0.42,
    g: 0.44,
    b: 0.50,
    a: 1.0,
};

/// Mouse-selection fill: dark mint-teal, solid (no blending) with white
/// ink — distinct from the white-block cursor.
const SEL_RGBA: Rgba = Rgba {
    r: 0.10,
    g: 0.30,
    b: 0.27,
    a: 1.0,
};

/// Map a terminal cell color to linear display RGB.
/// M0 palette: classic 16 ANSI colors picked as sRGB display values and
/// converted to linear (the swapchain is an sRGB format); Rgb passes
/// through the same conversion. Bold lifts 0-7 to bright 8-15.
pub fn term_color(color: zet_term::Color, bold: bool) -> Rgba {
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
            let i = if bold && i < 8 { i + 8 } else { i };
            let (r, g, b) = PALETTE[(i % 16) as usize];
            Rgba::srgb(r, g, b)
        }
        zet_term::Color::Rgb(r, g, b) => {
            Rgba::srgb(r as f32 / 255.0, g as f32 / 255.0, b as f32 / 255.0)
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
    bg_slot: Option<Slot>,
    quads: Vec<Vertex>,
}

impl Renderer {
    pub async fn new(window: Arc<Window>) -> Result<Self, InitError> {
        crate::perf_mark("renderer_new");
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
        crate::perf_mark("wgpu_adapter");
        {
            let info = adapter.get_info();
            crate::perf_mark("backend");
            if std::env::var_os("ZET_PERF").is_some() {
                eprintln!("perf: backend={:?} device={}", info.backend, info.device);
            }
        }
        let (device, queue) = adapter
            .request_device(&wgpu::DeviceDescriptor::default())
            .await
            .map_err(InitError::Device)?;
        crate::perf_mark("wgpu_device");
        let config = surface
            .get_default_config(&adapter, size.width.max(1), size.height.max(1))
            .ok_or(InitError::NoSurfaceConfig)?;
        surface.configure(&device, &config);
        eprintln!(
            "zet-gpu: format={:?} present={:?} {}x{}",
            config.format, config.present_mode, config.width, config.height
        );

        let shaper = Shaper::new(16.0);
        crate::perf_mark("font_system");
        let (cw, ch) = shaper.cell_px();
        let (cell_w, cell_h) = (cw as f32, ch as f32);
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
        crate::perf_mark("pipeline");

        let mut renderer = Renderer {
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
            bg_slot: None,
            quads: Vec::new(),
        };
        renderer.write_uniform(size.width.max(1), size.height.max(1));
        // Reserve the first atlas slot as a solid-white texel for bg fills.
        if let Some(slot) = renderer.atlas_alloc.alloc() {
            let (cw, ch) = (renderer.cell_w as u32, renderer.cell_h as u32);
            renderer.write_slot(&slot, &vec![255u8; cw as usize * ch as usize * 4], cw, ch);
            renderer.bg_slot = Some(slot);
        }
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

    pub fn cell_px(&self) -> (u32, u32) {
        (self.cell_w as u32, self.cell_h as u32)
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
        let (cw, ch) = (self.cell_w as u32, self.cell_h as u32);
        let ascent = self.shaper.ascent_px();
        let rgba = crate::text::blit(cov, ascent, cw, ch);
        self.write_slot(&slot, &rgba, cw, ch);
        self.slots.insert(c, slot);
        Some(slot)
    }

    fn write_slot(&self, slot: &Slot, rgba: &[u8], cw: u32, ch: u32) {
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
            rgba,
            wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(cw * 4),
                rows_per_image: Some(ch),
            },
            wgpu::Extent3d {
                width: cw,
                height: ch,
                depth_or_array_layers: 1,
            },
        );
    }

    /// Stage terminal grid cells as textured quads for the next render().
    /// M0: one glyph per cell; CJK/combining alignment is a follow-up.
    /// The cursor cell is drawn inverted (block cursor); mouse-selected
    /// cells draw in selection tint (absolute rows via scrollback).
    pub fn draw_terminal(
        &mut self,
        term: &zet_term::Terminal,
        cols: usize,
        rows: usize,
        sel: Option<crate::selection::Selection>,
    ) {
        self.quads.clear();
        let (cur_x, cur_y) = term.cursor();
        let show_cursor = term.cursor_visible();
        let sb = term.abs_base();
        for y in 0..rows {
            let line: Vec<char> = term.line_text(y).chars().collect();
            for (x, &c) in line.iter().enumerate().take(cols) {
                let cell = term.cell(x, y);
                let fg = cell.map(|cell| cell.fg).unwrap_or_default();
                let bg = cell.map(|cell| cell.bg).unwrap_or_default();
                let bold = cell.map(|cell| cell.bold).unwrap_or(false);
                let cursor_here = show_cursor && x == cur_x && y == cur_y;
                let selected = sel.as_ref().is_some_and(|s| s.contains(x, sb + y));
                if bg != zet_term::Color::Default || cursor_here || selected {
                    let fill = if cursor_here {
                        match fg {
                            zet_term::Color::Default => Rgba::WHITE,
                            _ => term_color(fg, bold),
                        }
                    } else if selected {
                        SEL_RGBA
                    } else {
                        term_color(bg, false)
                    };
                    if let Some(bg_slot) = self.bg_slot {
                        self.quads.extend(cell_quad(
                            x as u32,
                            y as u32,
                            self.cell_w,
                            self.cell_h,
                            (bg_slot.x, bg_slot.y),
                            self.atlas_size,
                            fill,
                        ));
                    }
                }
                if c == ' ' {
                    continue;
                }
                let ink = if cursor_here {
                    match bg {
                        zet_term::Color::Default => BG_RGBA,
                        _ => term_color(bg, false),
                    }
                } else if selected {
                    Rgba::WHITE
                } else {
                    term_color(fg, bold)
                };
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
                    ink,
                ));
            }
        }
    }

    /// Ghost autocomplete suffix, dimmed, starting at (row, col) — the
    /// remainder of a suggestion after the typed prefix. Spaces advance
    /// without quads (matching draw_terminal). Clipped to grid width.
    pub fn draw_ghost(&mut self, row: usize, col: usize, text: &str, cols: usize) {
        for (x, c) in (col..).zip(text.chars()) {
            if x >= cols {
                break;
            }
            if c != ' ' {
                if let Some(slot) = self.slot_for(c) {
                    self.quads.extend(cell_quad(
                        x as u32,
                        row as u32,
                        self.cell_w,
                        self.cell_h,
                        (slot.x, slot.y),
                        self.atlas_size,
                        GHOST_RGBA,
                    ));
                }
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
