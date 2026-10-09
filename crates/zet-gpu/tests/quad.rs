use zet_gpu::quad::{cell_quad, Rgba};

#[test]
fn quad_maps_cell_to_pixels_and_atlas_uv() {
    let verts = cell_quad(2, 1, 8.0, 16.0, (16, 32), (128, 128), Rgba::WHITE);
    assert_eq!(verts.len(), 6);
    // First triangle: top-left, bottom-left, bottom-right of the cell.
    assert_eq!((verts[0].pos, verts[0].uv), ([16.0, 16.0], [0.125, 0.25]));
    assert_eq!((verts[1].pos, verts[1].uv), ([16.0, 32.0], [0.125, 0.375]));
    assert_eq!((verts[2].pos, verts[2].uv), ([24.0, 32.0], [0.1875, 0.375]));
    // Color rides on every vertex.
    for v in &verts {
        assert_eq!(v.color, [1.0, 1.0, 1.0, 1.0]);
    }
}
