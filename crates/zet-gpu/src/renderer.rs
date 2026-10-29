// wgpu renderer: window surface, glyph atlas, textured text pipeline.
// term -> quads -> atlas texture -> present. M0: per-frame vertex upload,
// monochrome coverage tinted by cell color; LRU/persistent buffers later.

use std::collections::HashMap;
use std::fmt;
use std::sync::Arc;
use winit::window::Window;

use crate::atlas::{Atlas, Slot};
use crate::quad::{cell_quad, solid_rect_px, solid_triangle_px, Rgba, Vertex};
use crate::text::Shaper;
use crate::theme::APERTURE_INSTRUMENT;

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
const BG_RGBA: Rgba = APERTURE_INSTRUMENT.terminal;

/// Ghost-text ink: dimmed so suggestions read as preview, not content.
const GHOST_RGBA: Rgba = APERTURE_INSTRUMENT.muted;

/// Mouse-selection fill: dark mint-teal, solid (no blending) with white
/// ink — distinct from the white-block cursor.
const SEL_RGBA: Rgba = APERTURE_INSTRUMENT.selected;

/// Tab bar: near-black strip; the active segment lifts lighter with
/// white ink, inactive segments dim. Same quad machinery as cells.
pub const TABBAR_BG: Rgba = APERTURE_INSTRUMENT.top_bar;
pub const TAB_ACTIVE_BG: Rgba = APERTURE_INSTRUMENT.active;
pub const TAB_HOVER_BG: Rgba = APERTURE_INSTRUMENT.surface_raised;
pub const TAB_DIM_INK: Rgba = APERTURE_INSTRUMENT.secondary;
/// Inactive tab label ink; aliases the historical dim ink.
pub const TAB_INACTIVE_INK: Rgba = TAB_DIM_INK;
/// One-cell active-tab marker at the left edge of the active segment.
pub const TAB_ACTIVE_ACCENT: Rgba = APERTURE_INSTRUMENT.cyan;
/// One-cell separator between tab segments.
pub const TAB_SEPARATOR: Rgba = APERTURE_INSTRUMENT.border;

/// Rail panel surface: one step above the terminal's near-black grid.
pub const RAIL_BG: Rgba = APERTURE_INSTRUMENT.surface;
/// Rail right-edge rule separating chrome from the live terminal.
pub const RAIL_BORDER: Rgba = APERTURE_INSTRUMENT.border;
/// Selected session-card fill.
pub const RAIL_SELECTED_BG: Rgba = APERTURE_INSTRUMENT.selected;
/// Subtle fill for the active session when it is not the rail selection.
pub const RAIL_ACTIVE_BG: Rgba = APERTURE_INSTRUMENT.active;
/// Card hover surface, reserved for pointer-state wiring.
pub const RAIL_HOVER_BG: Rgba = APERTURE_INSTRUMENT.surface_raised;
/// Brand and active-session name ink.
pub const RAIL_HEADER_INK: Rgba = APERTURE_INSTRUMENT.text;
/// Section labels (`SESSIONS`, `OBSERVED COMMANDS`).
pub const RAIL_SECTION_INK: Rgba = APERTURE_INSTRUMENT.secondary;
/// Session names and command text.
pub const RAIL_TEXT_INK: Rgba = APERTURE_INSTRUMENT.text;
/// Paths, counts and secondary detail.
pub const RAIL_DIM_INK: Rgba = APERTURE_INSTRUMENT.muted;
/// Restrained cyan marker for the active/focused rail state.
pub const RAIL_ACCENT: Rgba = APERTURE_INSTRUMENT.cyan;
/// Brighter focus marker used only while the rail owns keyboard focus.
pub const RAIL_FOCUS: Rgba = APERTURE_INSTRUMENT.cyan;
/// Live/healthy session state.
pub const RAIL_OK: Rgba = APERTURE_INSTRUMENT.teal;
/// Exited/failed session state.
pub const RAIL_BAD: Rgba = APERTURE_INSTRUMENT.red;
/// Footer strip surface and ink.
pub const RAIL_FOOTER_BG: Rgba = APERTURE_INSTRUMENT.footer;
pub const RAIL_FOOTER_INK: Rgba = APERTURE_INSTRUMENT.secondary;

/// Shared contextual-panel tokens. These are public so the composition
/// layer can use the same semantic surfaces as the rail and tab bar.
pub const PANEL_BG: Rgba = APERTURE_INSTRUMENT.surface;
pub const PANEL_RAISED: Rgba = APERTURE_INSTRUMENT.surface_raised;
pub const PANEL_INSET: Rgba = APERTURE_INSTRUMENT.surface_inset;
pub const PANEL_BORDER: Rgba = APERTURE_INSTRUMENT.border;
pub const PANEL_BORDER_STRONG: Rgba = APERTURE_INSTRUMENT.border_strong;
pub const PANEL_TEXT: Rgba = APERTURE_INSTRUMENT.text;
pub const PANEL_SECONDARY: Rgba = APERTURE_INSTRUMENT.secondary;
pub const PANEL_MUTED: Rgba = APERTURE_INSTRUMENT.muted;
pub const PANEL_CYAN: Rgba = APERTURE_INSTRUMENT.cyan;
pub const PANEL_VIOLET: Rgba = APERTURE_INSTRUMENT.violet;
pub const PANEL_TEAL: Rgba = APERTURE_INSTRUMENT.teal;
pub const PANEL_RED: Rgba = APERTURE_INSTRUMENT.red;
pub const PANEL_AMBER: Rgba = APERTURE_INSTRUMENT.amber;
pub const PANEL_DANGER: Rgba = APERTURE_INSTRUMENT.danger;

/// Search hits: amber wash for matches, orange for the current one
/// (white ink, selection precedent — fill + re-inked glyphs).
const SEARCH_RGBA: Rgba = Rgba {
    r: 0.35,
    g: 0.26,
    b: 0.08,
    a: 1.0,
};
const SEARCH_CUR_RGBA: Rgba = Rgba {
    r: 0.65,
    g: 0.35,
    b: 0.08,
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

/// A cell-aligned rectangle in the window grid.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Viewport {
    pub x: u32,
    pub y: u32,
    pub cols: u32,
    pub rows: u32,
}

/// Conservative upper bound for cells staged by one chrome primitive.
pub const MAX_RENDERABLE_VIEWPORT_DIMENSION: u32 = 100_000;
/// Conservative upper bound for cells staged by one chrome primitive.
pub const MAX_RENDERABLE_VIEWPORT_CELLS: usize = 1_000_000;
/// Conservative upper bound for one vertical rule.
pub const MAX_RENDERABLE_RULE_ROWS: usize = 100_000;

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
        crate::perf_mark("instance_start");
        let instance = wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle());
        crate::perf_mark("instance_done");

        crate::perf_mark("surface_create_start");
        let surface = instance
            .create_surface(window)
            .map_err(InitError::Surface)?;
        crate::perf_mark("surface_create_done");

        crate::perf_mark("adapter_request_start");
        let adapter = instance
            .request_adapter(&wgpu::RequestAdapterOptions {
                power_preference: wgpu::PowerPreference::HighPerformance,
                compatible_surface: Some(&surface),
                force_fallback_adapter: false,
                ..Default::default()
            })
            .await
            .map_err(|_| InitError::NoAdapter)?;
        crate::perf_mark("adapter_request_done");
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
        crate::perf_mark("surface_config_start");
        let config = surface
            .get_default_config(&adapter, size.width.max(1), size.height.max(1))
            .ok_or(InitError::NoSurfaceConfig)?;
        surface.configure(&device, &config);
        crate::perf_mark("surface_config_done");
        eprintln!(
            "zet-gpu: format={:?} present={:?} {}x{}",
            config.format, config.present_mode, config.width, config.height
        );

        crate::perf_mark("shaper_new_start");
        let shaper = Shaper::new(16.0);
        crate::perf_mark("shaper_new_done");
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
                r: APERTURE_INSTRUMENT.space.r as f64,
                g: APERTURE_INSTRUMENT.space.g as f64,
                b: APERTURE_INSTRUMENT.space.b as f64,
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

    /// Configured default terminal colors as 8-bit RGB triples.
    pub fn default_colors(&self) -> ((u8, u8, u8), (u8, u8, u8)) {
        (
            (255, 255, 255),
            (
                (self.bg.r * 255.0).round() as u8,
                (self.bg.g * 255.0).round() as u8,
                (self.bg.b * 255.0).round() as u8,
            ),
        )
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
        let rgba = crate::text::blit(cov, ascent, cw, ch, self.shaper.left_pad());
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
        y_off: u32,
    ) {
        let vp = Viewport {
            x: 0,
            y: y_off,
            cols: cols as u32,
            rows: rows as u32,
        };
        self.draw_terminal_in(term, vp, sel);
    }

    /// Generalised terminal draw into a cell-aligned viewport. The viewport
    /// origin is applied exactly once and drawing is clipped to its bounds.
    pub fn draw_terminal_in(
        &mut self,
        term: &zet_term::Terminal,
        vp: Viewport,
        sel: Option<crate::selection::Selection>,
    ) {
        self.quads.clear();
        let (cur_x, cur_y) = term.cursor();
        let show_cursor = term.cursor_visible() && term.view_offset() == 0;
        let sb = term.visible_base();
        for vy in 0..vp.rows as usize {
            if vy >= term.rows() {
                break;
            }
            let line: Vec<char> = term.visible_line_text(vy).chars().collect();
            for (vx, &c) in line.iter().enumerate().take(vp.cols as usize) {
                let cell = term.visible_cell(vx, vy);
                let fg = cell.map(|cell| cell.fg).unwrap_or_default();
                let bg = cell.map(|cell| cell.bg).unwrap_or_default();
                let bold = cell.map(|cell| cell.bold).unwrap_or(false);
                let cursor_here = show_cursor && vx == cur_x && vy == cur_y;
                let selected = sel.as_ref().is_some_and(|s| s.contains(vx, sb + vy));
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
                        if let Some((cx, cy)) = viewport_cell_origin(vp, vx as u32, vy as u32) {
                            self.quads.extend(cell_quad(
                                cx,
                                cy,
                                self.cell_w,
                                self.cell_h,
                                (bg_slot.x, bg_slot.y),
                                self.atlas_size,
                                fill,
                            ));
                        }
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
                if let Some((cx, cy)) = viewport_cell_origin(vp, vx as u32, vy as u32) {
                    self.quads.extend(cell_quad(
                        cx,
                        cy,
                        self.cell_w,
                        self.cell_h,
                        (slot.x, slot.y),
                        self.atlas_size,
                        ink,
                    ));
                }
            }
        }
    }

    /// Tab bar on row 0: equal segments, active lifted + white ink,
    /// inactive dim. Labels pre-truncated by the caller; overlong text
    /// clips at the grid edge.
    ///
    /// With a zero-width rail this is byte-identical to the historical
    /// full-window tab bar. With a nonzero rail, tab controls occupy only
    /// the drawable strip `[rail_cols, cols)` so rail pixels are never
    /// painted as tab controls and every tab with a nonempty strip has a
    /// reachable hit region.
    pub fn draw_tabbar(&mut self, labels: &[String], active: usize, cols: usize) {
        self.draw_tabbar_with_rail(labels, active, cols, 0);
    }

    /// Tab bar variant with an excluded leading rail region.
    pub fn draw_tabbar_with_rail(
        &mut self,
        labels: &[String],
        active: usize,
        cols: usize,
        rail_cols: usize,
    ) {
        let n = labels.len().max(1);
        let segments = crate::selection::tabbar_segments(cols, rail_cols, n);
        for x in 0..cols {
            let seg = crate::selection::tabbar_segment_for_x(x, rail_cols, cols, n);
            let is_active = seg == Some(active);
            let seg_start = seg.and_then(|s| segments.get(s)).map(|(start, _)| *start);
            let is_seg_start = seg_start == Some(x);
            let fill = if is_seg_start {
                if is_active {
                    TAB_ACTIVE_ACCENT
                } else {
                    TAB_SEPARATOR
                }
            } else if is_active {
                TAB_ACTIVE_BG
            } else {
                TABBAR_BG
            };
            if let Some(bg_slot) = self.bg_slot {
                self.quads.extend(cell_quad(
                    x as u32,
                    0,
                    self.cell_w,
                    self.cell_h,
                    (bg_slot.x, bg_slot.y),
                    self.atlas_size,
                    fill,
                ));
            }
            let Some(seg) = seg else {
                continue;
            };
            let (start, _) = segments[seg];
            let lx = x - start;
            // The label carries one leading padding cell. That cell is the
            // segment's accent/separator block, so text starts at lx == 1.
            if lx == 0 {
                continue;
            }
            let text: Vec<char> = labels
                .get(seg)
                .map(|s| s.chars().collect())
                .unwrap_or_default();
            if lx < text.len() {
                let c = text[lx];
                if c != ' ' {
                    if let Some(slot) = self.slot_for(c) {
                        self.quads.extend(cell_quad(
                            x as u32,
                            0,
                            self.cell_w,
                            self.cell_h,
                            (slot.x, slot.y),
                            self.atlas_size,
                            if is_active {
                                Rgba::WHITE
                            } else {
                                TAB_INACTIVE_INK
                            },
                        ));
                    }
                }
            }
        }
    }

    /// Search hits over the visible grid: absolute (row, col, len)
    /// triples mapped through the current view; current hit orange,
    /// the rest amber. Follows the selection precedent (fill quad +
    /// re-inked glyph so text stays readable).
    pub fn draw_search(
        &mut self,
        term: &zet_term::Terminal,
        matches: &[(usize, usize, usize)],
        current: usize,
        vp: Viewport,
    ) {
        let sb = term.visible_base();
        let rows = term.rows();
        for (k, &(abs, col, len)) in matches.iter().enumerate() {
            let fill = if k == current {
                SEARCH_CUR_RGBA
            } else {
                SEARCH_RGBA
            };
            let line_row = abs as isize - sb as isize;
            if line_row < 0 || line_row as usize >= rows {
                continue;
            }
            let line: Vec<char> = term.visible_line_text(line_row as usize).chars().collect();
            for dx in 0..len {
                let x = col + dx;
                if x >= line.len() {
                    break;
                }
                let Some((cell_x, cell_y)) = viewport_search_cell(vp, abs, x, sb, rows) else {
                    // Keep scanning within the same hit only if it can still
                    // fit later columns; viewport_search_cell already clips.
                    if x as u32 >= vp.cols {
                        break;
                    }
                    continue;
                };
                if let Some(bg_slot) = self.bg_slot {
                    self.quads.extend(cell_quad(
                        cell_x,
                        cell_y,
                        self.cell_w,
                        self.cell_h,
                        (bg_slot.x, bg_slot.y),
                        self.atlas_size,
                        fill,
                    ));
                }
                let c = line[x];
                if c != ' ' {
                    if let Some(slot) = self.slot_for(c) {
                        self.quads.extend(cell_quad(
                            cell_x,
                            cell_y,
                            self.cell_w,
                            self.cell_h,
                            (slot.x, slot.y),
                            self.atlas_size,
                            Rgba::WHITE,
                        ));
                    }
                }
            }
        }
    }

    /// Ghost autocomplete suffix, dimmed, starting at (row, col) — the
    /// remainder of a suggestion after the typed prefix. Spaces advance
    /// without quads (matching draw_terminal). Wraps across rows so long
    /// previews and notices stay visible instead of clipping.
    pub fn draw_ghost(&mut self, vp: Viewport, row: usize, col: usize, text: &str) {
        for (x, y, c) in viewport_ghost_cells(vp, row, col, text) {
            if c != ' ' {
                if let Some(slot) = self.slot_for(c) {
                    self.quads.extend(cell_quad(
                        x,
                        y,
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
    /// Solid, cell-aligned rectangle covering every cell of `vp`.
    /// Appends to the current quad buffer; it never clears terminal
    /// geometry. Uses the existing solid atlas texel.
    pub fn draw_fill(&mut self, vp: Viewport, color: Rgba) {
        let Some(bg_slot) = self.bg_slot else {
            return;
        };
        let (window_cols, window_rows) = self.window_cell_dimensions();
        let Some(clipped) = clip_viewport_to_window(vp, window_cols, window_rows) else {
            return;
        };
        for (cx, cy) in viewport_fill_iter(clipped) {
            self.quads.extend(cell_quad(
                cx,
                cy,
                self.cell_w,
                self.cell_h,
                (bg_slot.x, bg_slot.y),
                self.atlas_size,
                color,
            ));
        }
    }

    /// One row of text at (`vp.x + col`, `vp.y + row`), clipped to the
    /// viewport. Reuses the existing glyph cache and appends to the
    /// current quad buffer.
    /// Newline and other control characters are non-rendering: they still
    /// consume one column in `viewport_text_cells`, but never reach
    /// `slot_for`, so a multi-line string stays on one row with gaps.
    pub fn draw_text_row(&mut self, vp: Viewport, row: u32, col: u32, text: &str, ink: Rgba) {
        if row >= vp.rows || col >= vp.cols {
            return;
        }
        let (window_cols, window_rows) = self.window_cell_dimensions();
        let Some(clipped) = clip_viewport_to_window(vp, window_cols, window_rows) else {
            return;
        };
        if row >= clipped.rows || col >= clipped.cols {
            return;
        }
        for (cx, cy, c) in viewport_text_iter(clipped, row, col, text) {
            if !is_renderable_text_char(c) {
                continue;
            }
            if let Some(slot) = self.slot_for(c) {
                self.quads.extend(cell_quad(
                    cx,
                    cy,
                    self.cell_w,
                    self.cell_h,
                    (slot.x, slot.y),
                    self.atlas_size,
                    ink,
                ));
            }
        }
    }

    /// A one-cell-wide vertical rule spanning rows `[y0, y1)` at column
    /// `x`. Appends to the current quad buffer and is a no-op for empty
    /// or inverted ranges.
    pub fn draw_rule(&mut self, x: u32, y0: u32, y1: u32, color: Rgba) {
        let Some(bg_slot) = self.bg_slot else {
            return;
        };
        let (window_cols, window_rows) = self.window_cell_dimensions();
        if x >= window_cols {
            return;
        }
        let Some((start, end)) = clip_rule_rows(y0, y1, window_rows) else {
            return;
        };
        for y in rule_rows_iter(start, end) {
            self.quads.extend(cell_quad(
                x,
                y,
                self.cell_w,
                self.cell_h,
                (bg_slot.x, bg_slot.y),
                self.atlas_size,
                color,
            ));
        }
    }

    /// Stage a bounded pixel-aligned rectangle for native chrome. Pixel
    /// geometry is deliberately separate from terminal-cell geometry so
    /// rounded panels never alter PTY sizing or hit testing.
    pub fn draw_rect_px(&mut self, x: u32, y: u32, width: u32, height: u32, color: Rgba) {
        if width == 0 || height == 0 || x >= self.config.width || y >= self.config.height {
            return;
        }
        let Some(atlas_uv) = self.solid_uv() else {
            return;
        };
        let x1 = x.saturating_add(width).min(self.config.width);
        let y1 = y.saturating_add(height).min(self.config.height);
        if x1 <= x || y1 <= y {
            return;
        }
        self.quads.extend(solid_rect_px(
            x as f32, y as f32, x1 as f32, y1 as f32, atlas_uv, color,
        ));
    }

    /// Stage a rounded pixel-aligned rectangle as a small triangle fan. Four
    /// segments per corner keep the silhouette smooth at normal display
    /// scales without introducing a second shader or a full-screen effect.
    pub fn draw_rounded_rect_px(
        &mut self,
        x: u32,
        y: u32,
        width: u32,
        height: u32,
        radius: u32,
        color: Rgba,
    ) {
        if width == 0 || height == 0 || x >= self.config.width || y >= self.config.height {
            return;
        }
        let Some(atlas_uv) = self.solid_uv() else {
            return;
        };
        let x1 = x.saturating_add(width).min(self.config.width);
        let y1 = y.saturating_add(height).min(self.config.height);
        if x1 <= x || y1 <= y {
            return;
        }
        let radius = radius.min((x1 - x) / 2).min((y1 - y) / 2) as f32;
        if radius < 0.5 {
            self.quads.extend(solid_rect_px(
                x as f32, y as f32, x1 as f32, y1 as f32, atlas_uv, color,
            ));
            return;
        }
        let x0 = x as f32;
        let y0 = y as f32;
        let x1 = x1 as f32;
        let y1 = y1 as f32;
        let corners = [
            (x0 + radius, y0 + radius, std::f32::consts::PI),
            (x1 - radius, y0 + radius, std::f32::consts::FRAC_PI_2 * 3.0),
            (x1 - radius, y1 - radius, 0.0),
            (x0 + radius, y1 - radius, std::f32::consts::FRAC_PI_2),
        ];
        let mut points = Vec::with_capacity(16);
        for (cx, cy, start) in corners {
            for segment in 0..4 {
                let angle = start + std::f32::consts::FRAC_PI_2 * segment as f32 / 4.0;
                points.push([cx + radius * angle.cos(), cy + radius * angle.sin()]);
            }
        }
        let center = [(x0 + x1) * 0.5, (y0 + y1) * 0.5];
        for i in 0..points.len() {
            let next = points[(i + 1) % points.len()];
            self.quads
                .extend(solid_triangle_px(center, points[i], next, atlas_uv, color));
        }
    }

    /// Stage an architectural panel with clipped corners. Aperture uses this
    /// for the global frame and selected evidence surfaces; ordinary rows can
    /// continue to use the cheaper rectangular/rounded primitives.
    pub fn draw_chamfered_rect_px(
        &mut self,
        x: u32,
        y: u32,
        width: u32,
        height: u32,
        cut: u32,
        color: Rgba,
    ) {
        if width == 0 || height == 0 || x >= self.config.width || y >= self.config.height {
            return;
        }
        let Some(atlas_uv) = self.solid_uv() else {
            return;
        };
        let x1 = x.saturating_add(width).min(self.config.width);
        let y1 = y.saturating_add(height).min(self.config.height);
        if x1 <= x || y1 <= y {
            return;
        }
        let cut = cut.min((x1 - x) / 2).min((y1 - y) / 2) as f32;
        if cut < 0.5 {
            self.quads.extend(solid_rect_px(
                x as f32, y as f32, x1 as f32, y1 as f32, atlas_uv, color,
            ));
            return;
        }
        let x0 = x as f32;
        let y0 = y as f32;
        let x1 = x1 as f32;
        let y1 = y1 as f32;
        let points = [
            [x0 + cut, y0],
            [x1 - cut, y0],
            [x1, y0 + cut],
            [x1, y1 - cut],
            [x1 - cut, y1],
            [x0 + cut, y1],
            [x0, y1 - cut],
            [x0, y0 + cut],
        ];
        let center = [(x0 + x1) * 0.5, (y0 + y1) * 0.5];
        for index in 0..points.len() {
            self.quads.extend(solid_triangle_px(
                center,
                points[index],
                points[(index + 1) % points.len()],
                atlas_uv,
                color,
            ));
        }
    }

    /// A one-pixel precision border plus a chamfered inset surface.
    #[allow(clippy::too_many_arguments)]
    pub fn draw_chamfered_panel_px(
        &mut self,
        x: u32,
        y: u32,
        width: u32,
        height: u32,
        cut: u32,
        fill: Rgba,
        border: Rgba,
    ) {
        self.draw_chamfered_rect_px(x, y, width, height, cut, border);
        if width > 2 && height > 2 {
            self.draw_chamfered_rect_px(
                x.saturating_add(1),
                y.saturating_add(1),
                width.saturating_sub(2),
                height.saturating_sub(2),
                cut.saturating_sub(1),
                fill,
            );
        }
    }

    /// Draw the compact geometric Aperture identity mark. It is a stylized Z
    /// built from three precise bars, so it remains legible at small DPI and
    /// does not depend on a bundled logo font or an external image asset.
    pub fn draw_aperture_mark_px(&mut self, x: u32, y: u32, size: u32, color: Rgba) {
        let Some(atlas_uv) = self.solid_uv() else {
            return;
        };
        if size < 4 {
            return;
        }
        let x = x as f32;
        let y = y as f32;
        let s = size as f32;
        let t = (s * 0.22).max(2.0);
        let polygon = |a: [f32; 2], b: [f32; 2], c: [f32; 2], d: [f32; 2]| {
            [
                solid_triangle_px(a, b, c, atlas_uv, color),
                solid_triangle_px(a, c, d, atlas_uv, color),
            ]
        };
        let bars = [
            polygon([x, y], [x + s, y], [x + s - t, y + t], [x + t, y + t]),
            polygon(
                [x + s - t, y + t],
                [x + s, y + t],
                [x + t, y + s - t],
                [x, y + s - t],
            ),
            polygon(
                [x + t, y + s - t],
                [x + s, y + s - t],
                [x + s - t, y + s],
                [x, y + s],
            ),
        ];
        for bar in bars {
            self.quads.extend(bar.into_iter().flatten());
        }
    }

    /// Draw the faceted Aperture cyber crystal mark matching the owner-approved
    /// Aperture Instrument concept. Uses multi-tonal polygons for depth and
    /// electric cyan luminance.
    pub fn draw_aperture_crystal_mark_px(&mut self, x: u32, y: u32, width: u32, height: u32) {
        let Some(atlas_uv) = self.solid_uv() else {
            return;
        };
        if width < 6 || height < 6 {
            return;
        }
        let x = x as f32;
        let y = y as f32;
        let w = width as f32;
        let h = height as f32;

        let hi_color = Rgba::from_hex(0x7CE8FF);
        let main_color = Rgba::from_hex(0x38D9FA);
        let dark_color = Rgba::from_hex(0x0284C7);

        // Top highlight facet
        let p0 = [x + w * 0.15, y];
        let p1 = [x + w * 0.70, y];
        let p2 = [x + w * 0.55, y + h * 0.28];
        let p3 = [x + w * 0.08, y + h * 0.28];
        self.quads.extend(
            [
                solid_triangle_px(p0, p1, p2, atlas_uv, hi_color),
                solid_triangle_px(p0, p2, p3, atlas_uv, hi_color),
            ]
            .into_iter()
            .flatten(),
        );

        // Main diagonal body
        let p4 = [x, y + h * 0.82];
        let p5 = [x + w * 0.12, y + h];
        let p6 = [x + w * 0.48, y + h * 0.40];
        self.quads.extend(
            [
                solid_triangle_px(p3, p2, p6, atlas_uv, main_color),
                solid_triangle_px(p3, p6, p4, atlas_uv, main_color),
                solid_triangle_px(p4, p6, p5, atlas_uv, main_color),
            ]
            .into_iter()
            .flatten(),
        );

        // Lower right wedge
        let p7 = [x + w * 0.42, y + h];
        let p8 = [x + w * 0.92, y + h];
        let p9 = [x + w * 0.92, y + h * 0.45];
        self.quads
            .extend(solid_triangle_px(p7, p8, p9, atlas_uv, dark_color));

        // Wedge highlight
        let p10 = [x + w * 0.65, y + h * 0.45];
        self.quads
            .extend(solid_triangle_px(p7, p10, p9, atlas_uv, main_color));
    }

    /// Draw large, distinctive geometric ZET branding ('Z', 'E', 'T' vector letters).
    /// Returns the total width consumed in pixels.
    pub fn draw_zet_brand_px(&mut self, x: u32, y: u32, height: u32, color: Rgba) -> u32 {
        if height < 6 {
            return 0;
        }
        let h = height as f32;
        let w = (h * 0.72).round().max(4.0);
        let t = (h * 0.22).round().max(2.0);
        let gap = (h * 0.32).round().max(3.0);
        let x0 = x as f32;
        let y0 = y as f32;

        let Some(atlas_uv) = self.solid_uv() else {
            return 0;
        };

        // Letter 'Z'
        self.quads
            .extend(solid_rect_px(x0, y0, x0 + w, y0 + t, atlas_uv, color));
        self.quads.extend(solid_rect_px(
            x0,
            y0 + h - t,
            x0 + w,
            y0 + h,
            atlas_uv,
            color,
        ));
        // Diagonal bar of Z
        let z_diag = [
            solid_triangle_px(
                [x0 + w, y0 + t],
                [x0 + w - t * 0.75, y0 + t],
                [x0, y0 + h - t],
                atlas_uv,
                color,
            ),
            solid_triangle_px(
                [x0 + w, y0 + t],
                [x0, y0 + h - t],
                [x0 + t * 0.75, y0 + h - t],
                atlas_uv,
                color,
            ),
        ];
        self.quads.extend(z_diag.into_iter().flatten());

        // Letter 'E'
        let ex = x0 + w + gap;
        self.quads
            .extend(solid_rect_px(ex, y0, ex + t, y0 + h, atlas_uv, color));
        self.quads
            .extend(solid_rect_px(ex + t, y0, ex + w, y0 + t, atlas_uv, color));
        let mid_y = y0 + ((h - t) * 0.5).round();
        self.quads.extend(solid_rect_px(
            ex + t,
            mid_y,
            ex + w * 0.85,
            mid_y + t,
            atlas_uv,
            color,
        ));
        self.quads.extend(solid_rect_px(
            ex + t,
            y0 + h - t,
            ex + w,
            y0 + h,
            atlas_uv,
            color,
        ));

        // Letter 'T'
        let tx = ex + w + gap;
        self.quads
            .extend(solid_rect_px(tx, y0, tx + w, y0 + t, atlas_uv, color));
        let stem_x = tx + ((w - t) * 0.5).round();
        self.quads.extend(solid_rect_px(
            stem_x,
            y0 + t,
            stem_x + t,
            y0 + h,
            atlas_uv,
            color,
        ));

        (3.0 * w + 2.0 * gap) as u32
    }

    /// Draw precision cyan corner brackets/seams for Aperture Instrument panels.
    pub fn draw_corner_brackets_px(
        &mut self,
        x0: u32,
        y0: u32,
        x1: u32,
        y1: u32,
        cut: u32,
        color: Rgba,
    ) {
        if x1 <= x0 || y1 <= y0 || cut == 0 {
            return;
        }
        let Some(atlas_uv) = self.solid_uv() else {
            return;
        };
        let (x0, y0, x1, y1, cut) = (x0 as f32, y0 as f32, x1 as f32, y1 as f32, cut as f32);
        let arm = (cut * 0.8).max(5.0);
        let t = 2.0f32;

        // Top-Left: angled seam from (x0, y0+cut) to (x0+cut, y0) + arms
        self.draw_line_px(x0, y0 + cut, x0 + cut, y0, t, color);
        self.quads.extend(solid_rect_px(
            x0 + cut,
            y0,
            x0 + cut + arm,
            y0 + t,
            atlas_uv,
            color,
        ));
        self.quads.extend(solid_rect_px(
            x0,
            y0 + cut,
            x0 + t,
            y0 + cut + arm,
            atlas_uv,
            color,
        ));

        // Top-Right: angled seam from (x1-cut, y0) to (x1, y0+cut) + arms
        self.draw_line_px(x1 - cut, y0, x1, y0 + cut, t, color);
        self.quads.extend(solid_rect_px(
            x1 - cut - arm,
            y0,
            x1 - cut,
            y0 + t,
            atlas_uv,
            color,
        ));
        self.quads.extend(solid_rect_px(
            x1 - t,
            y0 + cut,
            x1,
            y0 + cut + arm,
            atlas_uv,
            color,
        ));

        // Bottom-Left: angled seam from (x0, y1-cut) to (x0+cut, y1) + arms
        self.draw_line_px(x0, y1 - cut, x0 + cut, y1, t, color);
        self.quads.extend(solid_rect_px(
            x0 + cut,
            y1 - t,
            x0 + cut + arm,
            y1,
            atlas_uv,
            color,
        ));
        self.quads.extend(solid_rect_px(
            x0,
            y1 - cut - arm,
            x0 + t,
            y1 - cut,
            atlas_uv,
            color,
        ));

        // Bottom-Right: angled seam from (x1-cut, y1) to (x1, y1-cut) + arms
        self.draw_line_px(x1 - cut, y1, x1, y1 - cut, t, color);
        self.quads.extend(solid_rect_px(
            x1 - cut - arm,
            y1 - t,
            x1 - cut,
            y1,
            atlas_uv,
            color,
        ));
        self.quads.extend(solid_rect_px(
            x1 - t,
            y1 - cut - arm,
            x1,
            y1 - cut,
            atlas_uv,
            color,
        ));
    }

    /// Draw a line segment of specified thickness with solid tint.
    pub fn draw_line_px(
        &mut self,
        x0: f32,
        y0: f32,
        x1: f32,
        y1: f32,
        thickness: f32,
        color: Rgba,
    ) {
        let Some(atlas_uv) = self.solid_uv() else {
            return;
        };
        let dx = x1 - x0;
        let dy = y1 - y0;
        let len = (dx * dx + dy * dy).sqrt();
        if len < 0.001 {
            return;
        }
        let nx = -dy / len * (thickness * 0.5);
        let ny = dx / len * (thickness * 0.5);

        let p0 = [x0 + nx, y0 + ny];
        let p1 = [x1 + nx, y1 + ny];
        let p2 = [x1 - nx, y1 - ny];
        let p3 = [x0 - nx, y0 - ny];

        self.quads.extend(
            [
                solid_triangle_px(p0, p1, p2, atlas_uv, color),
                solid_triangle_px(p0, p2, p3, atlas_uv, color),
            ]
            .into_iter()
            .flatten(),
        );
    }

    /// Draw a circular status dot with optional subtle aura/halo.
    pub fn draw_status_dot_px(
        &mut self,
        cx: u32,
        cy: u32,
        radius: u32,
        fill: Rgba,
        halo: Option<Rgba>,
    ) {
        let Some(atlas_uv) = self.solid_uv() else {
            return;
        };
        let (cx, cy) = (cx as f32, cy as f32);
        if let Some(halo_col) = halo {
            let hr = radius as f32 + 3.0;
            let segments = 12;
            for i in 0..segments {
                let a0 = i as f32 * std::f32::consts::TAU / segments as f32;
                let a1 = (i + 1) as f32 * std::f32::consts::TAU / segments as f32;
                let p0 = [cx + hr * a0.cos(), cy + hr * a0.sin()];
                let p1 = [cx + hr * a1.cos(), cy + hr * a1.sin()];
                self.quads
                    .extend(solid_triangle_px([cx, cy], p0, p1, atlas_uv, halo_col));
            }
        }
        let r = radius as f32;
        let segments = 12;
        for i in 0..segments {
            let a0 = i as f32 * std::f32::consts::TAU / segments as f32;
            let a1 = (i + 1) as f32 * std::f32::consts::TAU / segments as f32;
            let p0 = [cx + r * a0.cos(), cy + r * a0.sin()];
            let p1 = [cx + r * a1.cos(), cy + r * a1.sin()];
            self.quads
                .extend(solid_triangle_px([cx, cy], p0, p1, atlas_uv, fill));
        }
    }

    /// Stage a small geometric diamond for the ZET identity mark.
    pub fn draw_diamond_px(&mut self, center: (u32, u32), radius: u32, color: Rgba) {
        let Some(atlas_uv) = self.solid_uv() else {
            return;
        };
        let (cx, cy) = (center.0 as f32, center.1 as f32);
        let r = radius as f32;
        let top = [cx, cy - r];
        let right = [cx + r, cy];
        let bottom = [cx, cy + r];
        let left = [cx - r, cy];
        self.quads
            .extend(solid_triangle_px(top, right, bottom, atlas_uv, color));
        self.quads
            .extend(solid_triangle_px(top, bottom, left, atlas_uv, color));
    }

    fn solid_uv(&self) -> Option<[f32; 2]> {
        let slot = self.bg_slot?;
        Some([
            (slot.x as f32 + self.cell_w * 0.5) / self.atlas_size.0 as f32,
            (slot.y as f32 + self.cell_h * 0.5) / self.atlas_size.1 as f32,
        ])
    }
}

/// Visible cells for ghost/notice text starting at (row, col), wrapped
/// across rows and clipped to the grid. Pure coordinates — unit-tested
/// without a GPU.
pub fn wrap_ghost(
    row: usize,
    col: usize,
    text: &str,
    cols: usize,
    rows: usize,
) -> Vec<(u32, u32, char)> {
    let mut out = Vec::new();
    let (mut x, mut y) = (col, row);
    for c in text.chars() {
        if c == '\n' {
            x = 0;
            y += 1;
        } else {
            if x >= cols {
                x = 0;
                y += 1;
            }
            if y >= rows {
                break;
            }
            out.push((x as u32, y as u32, c));
            x += 1;
        }
    }
    out
}

impl Renderer {
    fn window_cell_dimensions(&self) -> (u32, u32) {
        let cell_w = self.cell_w.max(1.0);
        let cell_h = self.cell_h.max(1.0);
        let cols = (self.config.width as f64 / cell_w as f64).floor();
        let rows = (self.config.height as f64 / cell_h as f64).floor();
        (
            cols.min(u32::MAX as f64) as u32,
            rows.min(u32::MAX as f64) as u32,
        )
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

/// Pure coordinate helper: map an absolute scrollback row/column to a
/// viewport cell, applying the viewport origin exactly once and clipping
/// to the viewport and terminal bounds. Returns `None` when the hit is
/// outside the viewport.
pub fn viewport_search_cell(
    vp: Viewport,
    abs_row: usize,
    col: usize,
    visible_base: usize,
    term_rows: usize,
) -> Option<(u32, u32)> {
    let y = abs_row as isize - visible_base as isize;
    if y < 0 || y as usize >= term_rows || y as usize >= vp.rows as usize {
        return None;
    }
    if col as u32 >= vp.cols {
        return None;
    }
    let x = vp.x.checked_add(col as u32)?;
    let y = vp.y.checked_add(y as u32)?;
    Some((x, y))
}

/// Pure coordinate helper: ghost cells wrapped and clipped to the
/// terminal viewport, then translated by the viewport origin.
pub fn viewport_ghost_cells(
    vp: Viewport,
    row: usize,
    col: usize,
    text: &str,
) -> Vec<(u32, u32, char)> {
    wrap_ghost(row, col, text, vp.cols as usize, vp.rows as usize)
        .into_iter()
        .filter_map(|(x, y, c)| {
            let x = vp.x.checked_add(x)?;
            let y = vp.y.checked_add(y)?;
            Some((x, y, c))
        })
        .collect()
}

/// Pure hit test for a terminal viewport. The viewport origin is
/// subtracted exactly once and out-of-bounds coordinates are rejected
/// instead of clamped.
pub fn viewport_hit_test(
    px: f64,
    py: f64,
    vp: Viewport,
    cell_w: u32,
    cell_h: u32,
    scrollback: usize,
) -> Option<crate::selection::CellPos> {
    if cell_w == 0 || cell_h == 0 || vp.cols == 0 || vp.rows == 0 {
        return None;
    }
    let x = px - vp.x as f64 * cell_w as f64;
    let y = py - vp.y as f64 * cell_h as f64;
    if x < 0.0 || y < 0.0 {
        return None;
    }
    let col = (x / cell_w as f64).floor() as usize;
    let row = (y / cell_h as f64).floor() as usize;
    if col >= vp.cols as usize || row >= vp.rows as usize {
        return None;
    }
    let row = row.checked_add(scrollback)?;
    Some(crate::selection::CellPos { col, row })
}

/// True when a requested fill viewport is within the primitive's
/// defensive bounds. Used by both the production iterator and tests.
pub fn viewport_fill_allowed(cols: u32, rows: u32) -> bool {
    if cols == 0
        || rows == 0
        || cols > MAX_RENDERABLE_VIEWPORT_DIMENSION
        || rows > MAX_RENDERABLE_VIEWPORT_DIMENSION
    {
        return false;
    }
    (cols as usize)
        .checked_mul(rows as usize)
        .is_some_and(|area| area <= MAX_RENDERABLE_VIEWPORT_CELLS)
}

/// Pure geometry helper for `draw_fill`: every cell covered by `vp`,
/// generated lazily and without an eager area-sized allocation.
pub fn viewport_fill_iter(vp: Viewport) -> impl Iterator<Item = (u32, u32)> {
    let allowed = viewport_fill_allowed(vp.cols, vp.rows);
    let max_cols = if allowed { vp.cols } else { 0 };
    let max_rows = if allowed { vp.rows } else { 0 };
    let mut x = 0;
    let mut y = 0;
    std::iter::from_fn(move || {
        while y < max_rows {
            if x >= max_cols {
                x = 0;
                y += 1;
                continue;
            }
            let Some(cx) = vp.x.checked_add(x) else {
                x = max_cols;
                continue;
            };
            let Some(cy) = vp.y.checked_add(y) else {
                break;
            };
            x += 1;
            return Some((cx, cy));
        }
        None
    })
}

/// Pure geometry helper for `draw_fill`: every cell covered by `vp`.
pub fn viewport_fill_cells(vp: Viewport) -> Vec<(u32, u32)> {
    viewport_fill_iter(vp).collect()
}

/// The documented single-line policy for text rows: spaces and control
/// characters consume a cell but never produce glyph output.
pub fn is_renderable_text_char(c: char) -> bool {
    c != ' ' && !c.is_control()
}

/// Pure geometry helper for `draw_text_row`: clipped text cells,
/// generated lazily.
pub fn viewport_text_iter<'a>(
    vp: Viewport,
    row: u32,
    col: u32,
    text: &'a str,
) -> impl Iterator<Item = (u32, u32, char)> + 'a {
    let valid = row < vp.rows && col < vp.cols;
    let cy = vp.y.checked_add(row);
    let max_cols = vp.cols.saturating_sub(col) as usize;
    text.chars()
        .enumerate()
        .take(if valid { max_cols } else { 0 })
        .filter_map(move |(i, c)| {
            let x = col.checked_add(i as u32)?;
            if x >= vp.cols {
                return None;
            }
            let cx = vp.x.checked_add(x)?;
            Some((cx, cy?, c))
        })
}

/// Pure geometry helper for `draw_text_row`: clipped text cells.
pub fn viewport_text_cells(vp: Viewport, row: u32, col: u32, text: &str) -> Vec<(u32, u32, char)> {
    viewport_text_iter(vp, row, col, text).collect()
}

/// Pure helper: clip a requested viewport to the drawable window cell
/// rectangle. Returns `None` when there is no intersection. Saturating
/// extent arithmetic prevents `vp.x + vp.cols` overflow from wrapping.
pub fn clip_viewport_to_window(
    vp: Viewport,
    window_cols: u32,
    window_rows: u32,
) -> Option<Viewport> {
    if window_cols == 0 || window_rows == 0 || vp.cols == 0 || vp.rows == 0 {
        return None;
    }
    let x0 = vp.x.min(window_cols);
    let x1 = vp.x.saturating_add(vp.cols).min(window_cols);
    let y0 = vp.y.min(window_rows);
    let y1 = vp.y.saturating_add(vp.rows).min(window_rows);
    if x0 >= x1 || y0 >= y1 {
        return None;
    }
    Some(Viewport {
        x: x0,
        y: y0,
        cols: x1 - x0,
        rows: y1 - y0,
    })
}

/// Pure helper: clip a vertical rule to the window's row bounds.
pub fn clip_rule_rows(y0: u32, y1: u32, window_rows: u32) -> Option<(u32, u32)> {
    if y1 <= y0 || window_rows == 0 {
        return None;
    }
    let start = y0.min(window_rows);
    let end = y1.min(window_rows);
    if start >= end {
        None
    } else {
        Some((start, end))
    }
}

/// Lazy, bounded rule rows. Empty for inverted or oversized ranges.
pub fn rule_rows_iter(y0: u32, y1: u32) -> impl Iterator<Item = u32> {
    let len = if y1 > y0 { (y1 - y0) as usize } else { 0 };
    let count = if len <= MAX_RENDERABLE_RULE_ROWS {
        len
    } else {
        0
    };
    (0..count).map(move |i| y0 + i as u32)
}

/// Pure geometry helper for `draw_rule`: rows in `[y0, y1)`.
pub fn rule_rows(y0: u32, y1: u32) -> Vec<u32> {
    rule_rows_iter(y0, y1).collect()
}

/// Pure origin helper for terminal cells: apply viewport X/Y exactly
/// once and clip to the viewport. Returns `None` for out-of-bounds or
/// overflowing coordinates.
pub fn viewport_cell_origin(vp: Viewport, col: u32, row: u32) -> Option<(u32, u32)> {
    if col >= vp.cols || row >= vp.rows {
        return None;
    }
    let x = vp.x.checked_add(col)?;
    let y = vp.y.checked_add(row)?;
    Some((x, y))
}

#[cfg(test)]
mod viewport_tests {
    use super::*;

    #[test]
    fn viewport_origin_offsets_search_cell() {
        let vp = Viewport {
            x: 24,
            y: 3,
            cols: 40,
            rows: 20,
        };
        assert_eq!(viewport_search_cell(vp, 103, 5, 100, 20), Some((29, 6)));
        assert_eq!(viewport_search_cell(vp, 99, 5, 100, 20), None);
        assert_eq!(viewport_search_cell(vp, 120, 5, 100, 20), None);
        assert_eq!(viewport_search_cell(vp, 103, 40, 100, 20), None);
    }

    #[test]
    fn viewport_origin_offsets_ghost_cells() {
        let vp = Viewport {
            x: 7,
            y: 2,
            cols: 5,
            rows: 4,
        };
        let cells = viewport_ghost_cells(vp, 0, 3, "abcde");
        assert_eq!(
            cells,
            vec![
                (10, 2, 'a'),
                (11, 2, 'b'),
                (7, 3, 'c'),
                (8, 3, 'd'),
                (9, 3, 'e')
            ]
        );
    }

    #[test]
    fn viewport_hit_test_rejects_out_of_bounds() {
        let vp = Viewport {
            x: 24,
            y: 3,
            cols: 40,
            rows: 20,
        };
        assert_eq!(
            viewport_hit_test(24.0 * 10.0, 3.0 * 20.0, vp, 10, 20, 7),
            Some(crate::selection::CellPos { col: 0, row: 7 })
        );
        assert_eq!(
            viewport_hit_test(23.0 * 10.0, 3.0 * 20.0, vp, 10, 20, 7),
            None
        );
        assert_eq!(
            viewport_hit_test(24.0 * 10.0, 2.0 * 20.0, vp, 10, 20, 7),
            None
        );
        assert_eq!(
            viewport_hit_test(64.0 * 10.0, 3.0 * 20.0, vp, 10, 20, 7),
            None
        );
        assert_eq!(
            viewport_hit_test(24.0 * 10.0, 23.0 * 20.0, vp, 10, 20, 7),
            None
        );
    }

    #[test]
    fn viewport_fill_covers_every_cell_once() {
        let vp = Viewport {
            x: 4,
            y: 7,
            cols: 3,
            rows: 2,
        };
        assert_eq!(
            viewport_fill_cells(vp),
            vec![(4, 7), (5, 7), (6, 7), (4, 8), (5, 8), (6, 8)]
        );
    }

    #[test]
    fn viewport_fill_is_empty_for_zero_size() {
        let vp = Viewport {
            x: 4,
            y: 7,
            cols: 0,
            rows: 2,
        };
        assert!(viewport_fill_cells(vp).is_empty());
    }

    #[test]
    fn viewport_text_clips_to_viewport() {
        let vp = Viewport {
            x: 2,
            y: 9,
            cols: 4,
            rows: 3,
        };
        assert_eq!(
            viewport_text_cells(vp, 1, 2, "abcdef"),
            vec![(4, 10, 'a'), (5, 10, 'b')]
        );
        assert!(viewport_text_cells(vp, 3, 0, "x").is_empty());
        assert!(viewport_text_cells(vp, 0, 4, "x").is_empty());
        assert!(viewport_text_cells(vp, 0, 0, "").is_empty());
    }

    #[test]
    fn viewport_text_handles_unicode_chars() {
        let vp = Viewport {
            x: 0,
            y: 0,
            cols: 4,
            rows: 1,
        };
        let cells = viewport_text_cells(vp, 0, 0, "aé😀");
        assert_eq!(cells.len(), 3);
        assert_eq!(cells[2].2, '😀');
    }

    #[test]
    fn rule_rows_is_safe_and_ordered() {
        assert_eq!(rule_rows(2, 5), vec![2, 3, 4]);
        assert!(rule_rows(5, 5).is_empty());
        assert!(rule_rows(7, 3).is_empty());
        assert!(rule_rows(u32::MAX, u32::MAX).is_empty());
    }

    #[test]
    fn fill_limits_reject_huge_requests_before_iteration() {
        assert!(!viewport_fill_allowed(
            MAX_RENDERABLE_VIEWPORT_DIMENSION + 1,
            1
        ));
        assert!(!viewport_fill_allowed(
            1,
            MAX_RENDERABLE_VIEWPORT_DIMENSION + 1
        ));
        assert!(!viewport_fill_allowed(1001, 1000));
        assert!(viewport_fill_allowed(1000, 1000));
        assert!(viewport_fill_allowed(MAX_RENDERABLE_VIEWPORT_DIMENSION, 1));
        assert!(viewport_fill_iter(Viewport {
            x: 0,
            y: 0,
            cols: u32::MAX,
            rows: 1,
        })
        .next()
        .is_none());
        assert!(viewport_fill_cells(Viewport {
            x: 0,
            y: 0,
            cols: 1001,
            rows: 1000,
        })
        .is_empty());
    }

    #[test]
    fn rule_limit_rejects_huge_nonempty_range() {
        assert!(rule_rows(0, u32::MAX).is_empty());
        assert!(rule_rows_iter(0, u32::MAX).next().is_none());
        assert_eq!(
            rule_rows_iter(0, MAX_RENDERABLE_RULE_ROWS as u32)
                .take(1)
                .count(),
            1
        );
        assert!(rule_rows_iter(0, MAX_RENDERABLE_RULE_ROWS as u32 + 1)
            .next()
            .is_none());
    }

    #[test]
    fn near_max_origins_do_not_wrap() {
        let base = u32::MAX - 1;
        let fill = viewport_fill_cells(Viewport {
            x: base,
            y: base,
            cols: 5,
            rows: 5,
        });
        assert_eq!(fill.len(), 4);
        assert!(fill.iter().all(|&(x, y)| x >= base && y >= base));

        let text = viewport_text_cells(
            Viewport {
                x: base,
                y: base,
                cols: 5,
                rows: 1,
            },
            0,
            0,
            "abc",
        );
        assert_eq!(text, vec![(base, base, 'a'), (u32::MAX, base, 'b')]);

        let vp = Viewport {
            x: base,
            y: base,
            cols: 3,
            rows: 1,
        };
        assert_eq!(viewport_search_cell(vp, 0, 1, 0, 1), Some((u32::MAX, base)));
        assert_eq!(viewport_search_cell(vp, 0, 2, 0, 1), None);

        let ghost = viewport_ghost_cells(vp, 0, 0, "abc");
        assert_eq!(ghost, vec![(base, base, 'a'), (u32::MAX, base, 'b')]);
    }

    #[test]
    fn clip_viewport_to_window_cases() {
        let inside = Viewport {
            x: 2,
            y: 3,
            cols: 4,
            rows: 2,
        };
        assert_eq!(clip_viewport_to_window(inside, 10, 10), Some(inside));

        let partial = Viewport {
            x: 8,
            y: 8,
            cols: 5,
            rows: 5,
        };
        assert_eq!(
            clip_viewport_to_window(partial, 10, 10),
            Some(Viewport {
                x: 8,
                y: 8,
                cols: 2,
                rows: 2,
            })
        );

        assert_eq!(
            clip_viewport_to_window(
                Viewport {
                    x: 10,
                    y: 0,
                    cols: 5,
                    rows: 5,
                },
                10,
                10,
            ),
            None
        );
        assert_eq!(
            clip_viewport_to_window(
                Viewport {
                    x: 95,
                    y: 0,
                    cols: u32::MAX,
                    rows: 1,
                },
                100,
                10,
            ),
            Some(Viewport {
                x: 95,
                y: 0,
                cols: 5,
                rows: 1,
            })
        );
        assert_eq!(
            clip_viewport_to_window(
                Viewport {
                    x: 0,
                    y: 0,
                    cols: 1,
                    rows: 1,
                },
                0,
                10,
            ),
            None
        );
    }

    #[test]
    fn clip_rule_rows_cases() {
        assert_eq!(clip_rule_rows(2, 5, 10), Some((2, 5)));
        assert_eq!(clip_rule_rows(8, 20, 10), Some((8, 10)));
        assert_eq!(clip_rule_rows(10, 20, 10), None);
        assert_eq!(clip_rule_rows(5, 5, 10), None);
        assert_eq!(clip_rule_rows(7, 3, 10), None);
        assert_eq!(clip_rule_rows(0, u32::MAX, 10), Some((0, 10)));
        assert_eq!(clip_rule_rows(0, 5, 0), None);
    }

    #[test]
    fn text_control_characters_consume_one_cell_without_newline() {
        assert!(is_renderable_text_char('a'));
        assert!(!is_renderable_text_char(' '));
        assert!(!is_renderable_text_char('\n'));
        assert!(!is_renderable_text_char('\t'));

        let vp = Viewport {
            x: 4,
            y: 6,
            cols: 5,
            rows: 1,
        };
        assert_eq!(
            viewport_text_cells(vp, 0, 0, "a\nb"),
            vec![(4, 6, 'a'), (5, 6, '\n'), (6, 6, 'b')]
        );
        assert_eq!(viewport_text_iter(vp, 0, 0, "a\nb").count(), 3);
        assert_eq!(viewport_text_iter(vp, 1, 0, "a").count(), 0);
    }

    #[test]
    fn viewport_cell_origin_applies_offsets_once() {
        let vp = Viewport {
            x: 24,
            y: 3,
            cols: 40,
            rows: 20,
        };
        assert_eq!(viewport_cell_origin(vp, 0, 0), Some((24, 3)));
        assert_eq!(viewport_cell_origin(vp, 39, 19), Some((63, 22)));
        assert_eq!(viewport_cell_origin(vp, 40, 0), None);
        assert_eq!(viewport_cell_origin(vp, 0, 20), None);
    }

    #[test]
    fn viewport_cell_origin_does_not_wrap() {
        let vp = Viewport {
            x: u32::MAX - 1,
            y: u32::MAX - 1,
            cols: 3,
            rows: 2,
        };
        assert_eq!(
            viewport_cell_origin(vp, 0, 0),
            Some((u32::MAX - 1, u32::MAX - 1))
        );
        assert_eq!(
            viewport_cell_origin(vp, 1, 0),
            Some((u32::MAX, u32::MAX - 1))
        );
        assert_eq!(viewport_cell_origin(vp, 2, 0), None);
        assert_eq!(
            viewport_cell_origin(vp, 0, 1),
            Some((u32::MAX - 1, u32::MAX))
        );
        assert_eq!(viewport_cell_origin(vp, 0, 2), None);
    }

    #[test]
    fn search_cell_nonzero_origin_and_scrollback() {
        let vp = Viewport {
            x: 12,
            y: 2,
            cols: 10,
            rows: 4,
        };
        assert_eq!(viewport_search_cell(vp, 103, 3, 100, 4), Some((15, 5)));
        assert_eq!(viewport_search_cell(vp, 100, 0, 100, 4), Some((12, 2)));
        assert_eq!(viewport_search_cell(vp, 102, 9, 100, 4), Some((21, 4)));
        assert_eq!(viewport_search_cell(vp, 103, 10, 100, 4), None);
        assert_eq!(viewport_search_cell(vp, 99, 0, 100, 4), None);
        assert_eq!(viewport_search_cell(vp, 104, 0, 100, 4), None);
    }

    #[test]
    fn search_cell_clips_right_edge() {
        let vp = Viewport {
            x: 0,
            y: 0,
            cols: 3,
            rows: 2,
        };
        assert_eq!(viewport_search_cell(vp, 0, 2, 0, 2), Some((2, 0)));
        assert_eq!(viewport_search_cell(vp, 0, 3, 0, 2), None);
    }

    #[test]
    fn ghost_cells_use_viewport_origin_and_width() {
        let vp = Viewport {
            x: 5,
            y: 1,
            cols: 4,
            rows: 3,
        };
        assert_eq!(
            viewport_ghost_cells(vp, 0, 2, "abcdef"),
            vec![
                (7, 1, 'a'),
                (8, 1, 'b'),
                (5, 2, 'c'),
                (6, 2, 'd'),
                (7, 2, 'e'),
                (8, 2, 'f')
            ]
        );
    }

    #[test]
    fn ghost_cells_reject_overflowing_origin() {
        let vp = Viewport {
            x: u32::MAX - 1,
            y: u32::MAX - 1,
            cols: 3,
            rows: 2,
        };
        assert_eq!(
            viewport_ghost_cells(vp, 0, 0, "abc"),
            vec![
                (u32::MAX - 1, u32::MAX - 1, 'a'),
                (u32::MAX, u32::MAX - 1, 'b')
            ]
        );
    }

    #[test]
    fn multiple_tab_offsets_are_viewport_relative() {
        let vp = Viewport {
            x: 24,
            y: 1,
            cols: 80,
            rows: 23,
        };
        assert_eq!(viewport_cell_origin(vp, 0, 0), Some((24, 1)));
        assert_eq!(viewport_cell_origin(vp, 79, 22), Some((103, 23)));
        assert_eq!(viewport_cell_origin(vp, 80, 0), None);
    }

    fn contrast(a: Rgba, b: Rgba) -> f32 {
        // Tokens are stored in linear light, which is also the space WCAG
        // relative luminance is defined in.
        fn lum(c: Rgba) -> f32 {
            0.2126 * c.r + 0.7152 * c.g + 0.0722 * c.b
        }
        let (l1, l2) = (lum(a), lum(b));
        let (hi, lo) = if l1 >= l2 { (l1, l2) } else { (l2, l1) };
        (hi + 0.05) / (lo + 0.05)
    }

    #[test]
    fn chrome_tokens_meet_text_and_marker_contrast() {
        // Text: WCAG AA 4.5:1. Markers/focus bars: 3:1.
        for (name, ink, bg) in [
            ("header", RAIL_HEADER_INK, RAIL_BG),
            ("section", RAIL_SECTION_INK, RAIL_BG),
            ("session", RAIL_TEXT_INK, RAIL_BG),
            ("detail", RAIL_DIM_INK, RAIL_BG),
            ("footer", RAIL_FOOTER_INK, RAIL_FOOTER_BG),
            ("inactive tab", TAB_INACTIVE_INK, TABBAR_BG),
            ("active tab", Rgba::WHITE, TAB_ACTIVE_BG),
        ] {
            let ratio = contrast(ink, bg);
            assert!(ratio >= 4.5, "{name} contrast {ratio:.2} < 4.5");
        }
        for (name, ink, bg) in [
            ("accent", RAIL_ACCENT, RAIL_BG),
            ("focus", RAIL_FOCUS, RAIL_SELECTED_BG),
            ("live", RAIL_OK, RAIL_BG),
            ("failed", RAIL_BAD, RAIL_BG),
        ] {
            let ratio = contrast(ink, bg);
            assert!(ratio >= 3.0, "{name} marker contrast {ratio:.2} < 3.0");
        }
    }
}
