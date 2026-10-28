// Textured-quad geometry for terminal cells. Pure math: pixel rects in,
// normalized UVs out. The renderer uploads these as the vertex buffer.

use bytemuck::{Pod, Zeroable};

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Rgba {
    pub r: f32,
    pub g: f32,
    pub b: f32,
    pub a: f32,
}

impl Rgba {
    pub const WHITE: Rgba = Rgba {
        r: 1.0,
        g: 1.0,
        b: 1.0,
        a: 1.0,
    };

    pub fn rgb(r: f32, g: f32, b: f32) -> Self {
        Rgba { r, g, b, a: 1.0 }
    }

    /// A compact semantic-token helper for the native chrome palette.
    ///
    /// Native chrome is presented through the existing `Rgba8Unorm` surface,
    /// so these values keep the design system's familiar six-digit display
    /// colors readable at the token definition site. Terminal ANSI colors
    /// continue to use `srgb` where their separate color semantics require
    /// linear-light conversion.
    pub const fn from_hex(hex: u32) -> Self {
        Self {
            r: ((hex >> 16) & 0xff) as f32 / 255.0,
            g: ((hex >> 8) & 0xff) as f32 / 255.0,
            b: (hex & 0xff) as f32 / 255.0,
            a: 1.0,
        }
    }

    /// sRGB display colors (as picked on a color wheel) converted to linear
    /// light for the shader. The swapchain is an sRGB format, so writing
    /// these raw would wash everything out (hardware encodes linear->sRGB).
    pub fn srgb(r: f32, g: f32, b: f32) -> Self {
        fn dec(c: f32) -> f32 {
            if c <= 0.04045 {
                c / 12.92
            } else {
                ((c + 0.055) / 1.055).powf(2.4)
            }
        }
        Rgba {
            r: dec(r),
            g: dec(g),
            b: dec(b),
            a: 1.0,
        }
    }
}

#[repr(C)]
#[derive(Debug, Clone, Copy, Pod, Zeroable)]
pub struct Vertex {
    pub pos: [f32; 2],
    pub uv: [f32; 2],
    pub color: [f32; 4],
}

impl Vertex {
    pub const LAYOUT: [wgpu::VertexAttribute; 3] = wgpu::vertex_attr_array![
        0 => Float32x2,
        1 => Float32x2,
        2 => Float32x4,
    ];
}

/// Two triangles covering cell (cx, cy) of size cell_w x cell_h pixels,
/// sampling the atlas slot at slot_px in an atlas_w x atlas_h texture.
pub fn cell_quad(
    cx: u32,
    cy: u32,
    cell_w: f32,
    cell_h: f32,
    slot_px: (u32, u32),
    atlas_px: (u32, u32),
    color: Rgba,
) -> [Vertex; 6] {
    let x0 = cx as f32 * cell_w;
    let y0 = cy as f32 * cell_h;
    let x1 = x0 + cell_w;
    let y1 = y0 + cell_h;
    let u0 = slot_px.0 as f32 / atlas_px.0 as f32;
    let v0 = slot_px.1 as f32 / atlas_px.1 as f32;
    let u1 = (slot_px.0 as f32 + cell_w) / atlas_px.0 as f32;
    let v1 = (slot_px.1 as f32 + cell_h) / atlas_px.1 as f32;
    let c = [color.r, color.g, color.b, color.a];
    let tl = Vertex {
        pos: [x0, y0],
        uv: [u0, v0],
        color: c,
    };
    let tr = Vertex {
        pos: [x1, y0],
        uv: [u1, v0],
        color: c,
    };
    let bl = Vertex {
        pos: [x0, y1],
        uv: [u0, v1],
        color: c,
    };
    let br = Vertex {
        pos: [x1, y1],
        uv: [u1, v1],
        color: c,
    };
    [tl, bl, br, tl, br, tr]
}

/// A pixel-aligned solid rectangle using one stable atlas sample for all
/// vertices. Unlike `cell_quad`, this primitive is used only for native
/// chrome surfaces and never changes terminal cell geometry.
pub fn solid_rect_px(
    x0: f32,
    y0: f32,
    x1: f32,
    y1: f32,
    atlas_uv: [f32; 2],
    color: Rgba,
) -> [Vertex; 6] {
    let c = [color.r, color.g, color.b, color.a];
    let tl = Vertex {
        pos: [x0, y0],
        uv: atlas_uv,
        color: c,
    };
    let tr = Vertex {
        pos: [x1, y0],
        uv: atlas_uv,
        color: c,
    };
    let bl = Vertex {
        pos: [x0, y1],
        uv: atlas_uv,
        color: c,
    };
    let br = Vertex {
        pos: [x1, y1],
        uv: atlas_uv,
        color: c,
    };
    [tl, bl, br, tl, br, tr]
}

/// One solid triangle for small pixel geometry such as the ZET identity
/// mark. The atlas UV is constant so the existing textured pipeline remains
/// the only render pipeline in the application.
pub fn solid_triangle_px(
    a: [f32; 2],
    b: [f32; 2],
    c_pos: [f32; 2],
    atlas_uv: [f32; 2],
    color: Rgba,
) -> [Vertex; 3] {
    let color = [color.r, color.g, color.b, color.a];
    [
        Vertex {
            pos: a,
            uv: atlas_uv,
            color,
        },
        Vertex {
            pos: b,
            uv: atlas_uv,
            color,
        },
        Vertex {
            pos: c_pos,
            uv: atlas_uv,
            color,
        },
    ]
}
