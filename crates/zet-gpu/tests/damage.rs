use zet_gpu::damage::{Damage, Rect};

#[test]
fn overlapping_rects_merge_and_take_drains() {
    let mut dmg = Damage::new();
    assert!(dmg.take().is_empty());
    dmg.add(Rect::new(0, 0, 10, 5));
    dmg.add(Rect::new(5, 2, 10, 5));
    dmg.add(Rect::new(40, 40, 2, 2));
    let rects = dmg.take();
    assert_eq!(rects.len(), 2);
    assert!(rects.contains(&Rect::new(0, 0, 15, 7)));
    assert!(rects.contains(&Rect::new(40, 40, 2, 2)));
    assert!(dmg.take().is_empty());
}

#[test]
fn chained_overlap_merging_backward_collapses_to_one() {
    let mut dmg = Damage::new();
    dmg.add(Rect::new(0, 0, 10, 10));
    dmg.add(Rect::new(5, 12, 12, 10));
    dmg.add(Rect::new(12, 5, 10, 10));
    let rects = dmg.take();
    assert_eq!(rects.len(), 1);
    assert_eq!(rects[0], Rect::new(0, 0, 22, 22));
}
