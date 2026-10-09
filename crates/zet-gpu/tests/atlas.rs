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

#[test]
#[should_panic(expected = "cell dimensions must be nonzero")]
fn zero_cell_dimensions_panic() {
    let _ = Atlas::new(2, 2, 0, 0);
}

#[test]
fn misaligned_free_is_ignored() {
    let mut atlas = Atlas::new(2, 2, 8, 16);
    let a = atlas.alloc().unwrap();
    // Half-cell offset: not a real slot, must not corrupt the free list.
    atlas.free(zet_gpu::atlas::Slot {
        x: a.x + 4,
        y: a.y,
        w: 8,
        h: 16,
    });
    assert_eq!(atlas.free_slots(), 3);
    let b = atlas.alloc().unwrap();
    atlas.free(b);
    atlas.free(a);
    assert_eq!(atlas.free_slots(), 4);
}
