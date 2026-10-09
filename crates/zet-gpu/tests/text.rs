use zet_gpu::text::Shaper;

#[test]
fn monospace_advances_are_uniform_and_positive() {
    let mut shaper = Shaper::new(16.0);
    let glyphs = shaper.shape_line("hello");
    assert_eq!(glyphs.len(), 5);
    let adv = glyphs[0].advance;
    assert!(adv > 0.0, "advance was {adv}");
    for g in &glyphs {
        assert!(
            (g.advance - adv).abs() < 0.01,
            "non-uniform advance {} vs {adv}",
            g.advance
        );
    }
}

#[test]
fn glyph_coverage_present_for_a_absent_for_space() {
    let mut shaper = Shaper::new(16.0);
    let a = shaper.shape_line("A");
    assert_eq!(a.len(), 1);
    assert!(!a[0].pixels.is_empty(), "A has no coverage");
    assert!(
        a[0].pixels.iter().any(|&(_, _, alpha)| alpha > 128),
        "A has no solid pixels"
    );
    let space = shaper.shape_line(" ");
    assert_eq!(space.len(), 1);
    assert!(space[0].pixels.is_empty(), "space should have no coverage");
}
