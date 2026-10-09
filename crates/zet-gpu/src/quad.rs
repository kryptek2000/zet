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
