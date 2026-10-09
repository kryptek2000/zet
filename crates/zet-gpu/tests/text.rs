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

#[test]
fn blit_preserves_caps_and_descenders() {
    use zet_gpu::text::blit;
    let mut shaper = Shaper::new(16.0);
    let ascent = shaper.ascent_px();
    assert!(
        (8.0..20.0).contains(&ascent),
        "implausible ascent {ascent} for 16px mono"
    );
    let (cw, ch) = shaper.cell_px();
    assert!(
        ch >= ascent.ceil() as u32 + 2,
        "cell too short for descenders"
    );

    let caps = &shaper.shape_line("A")[0].pixels;
    let in_sum: u32 = caps.iter().map(|&(_, _, a)| a as u32).sum();
    let bmp = blit(caps, ascent, cw, ch);
    let px: &[[u8; 4]] = bmp.as_chunks::<4>().0;
    let out_sum: u32 = px.iter().map(|px| px[3] as u32).sum();
    assert_eq!(in_sum, out_sum, "cap pixels clipped");
    let top_ink = px.iter().take(cw as usize * 6).any(|px| px[3] > 0);
    assert!(top_ink, "cap top clipped (ascent {ascent})");

    let desc = &shaper.shape_line("g")[0].pixels;
    let bmp = blit(desc, ascent, cw, ch);
    let px: &[[u8; 4]] = bmp.as_chunks::<4>().0;
    let below: usize = ((ascent.ceil() as usize) + 1) * cw as usize;
    let desc_ink = px.iter().skip(below).any(|px| px[3] > 0);
    assert!(desc_ink, "descender clipped (ascent {ascent})");
}
