use zet_gpu::atlas::Atlas;

#[test]
fn alloc_fills_then_frees_and_reuses() {
    let mut atlas = Atlas::new(2, 2, 8, 16);
    let a = atlas.alloc().unwrap();
    let b = atlas.alloc().unwrap();
    let c = atlas.alloc().unwrap();
    let d = atlas.alloc().unwrap();
    assert!(atlas.alloc().is_none());
    assert_eq!((a.x, a.y), (0, 0));
    assert_eq!((d.x, d.y, d.w, d.h), (8, 16, 8, 16));
    atlas.free(b);
    atlas.free(c);
    assert_eq!(atlas.free_slots(), 2);
    let e = atlas.alloc().unwrap();
    let f = atlas.alloc().unwrap();
    assert_ne!((e.x, e.y), (f.x, f.y));
    assert!(atlas.alloc().is_none());
}
