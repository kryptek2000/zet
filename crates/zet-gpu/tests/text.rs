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
    let bmp = blit(caps, ascent, cw, ch, shaper.left_pad());
    let px: &[[u8; 4]] = bmp.as_chunks::<4>().0;
    let out_sum: u32 = px.iter().map(|px| px[3] as u32).sum();
    assert_eq!(in_sum, out_sum, "cap pixels clipped");
    let top_row = px
        .iter()
        .enumerate()
        .filter(|(_, px)| px[3] > 0)
        .map(|(i, _)| i / cw as usize)
        .min();
    assert!(top_row.is_some(), "cap has no ink");
    assert!(
        top_row.unwrap() < ascent.ceil() as usize,
        "cap top clipped (ascent {ascent}, top row {})",
        top_row.unwrap()
    );

    let desc = &shaper.shape_line("g")[0].pixels;
    let bmp = blit(desc, ascent, cw, ch, shaper.left_pad());
    let px: &[[u8; 4]] = bmp.as_chunks::<4>().0;
    let below: usize = ((ascent.ceil() as usize) + 1) * cw as usize;
    let desc_ink = px.iter().skip(below).any(|px| px[3] > 0);
    assert!(desc_ink, "descender clipped (ascent {ascent})");
}

#[test]
fn ascent_covers_rasterized_cap_top() {
    let mut shaper = Shaper::new(16.0);
    let ascent = shaper.ascent_px();
    for s in ["A", "B", "M", "W", "@", "Å", "É", "Ñ"] {
        let glyph = &shaper.shape_line(s)[0];
        let min_y = glyph.pixels.iter().map(|p| p.1).min().unwrap_or(0);
        assert!(
            ascent >= -min_y as f32,
            "ascent {ascent} does not cover top extent {} for {s}",
            -min_y
        );
    }
}

#[test]
fn cell_height_covers_descenders() {
    let mut shaper = Shaper::new(16.0);
    let ascent = shaper.ascent_px();
    let (_cw, ch) = shaper.cell_px();
    for s in ["g", "j", "p", "q", "y"] {
        let glyph = &shaper.shape_line(s)[0];
        let max_y = glyph.pixels.iter().map(|p| p.1).max().unwrap_or(0);
        assert!(
            (max_y as f32 + ascent) < ch as f32,
            "descender clipped for {s}: {} >= {ch}",
            max_y as f32 + ascent
        );
    }
}

#[test]
fn blit_does_not_clip_glyph_pixels_geometrically() {
    let mut shaper = Shaper::new(16.0);
    let ascent = shaper.ascent_px();
    let (cw, ch) = shaper.cell_px();
    let left_pad = shaper.left_pad();
    let base = ascent.round() as i32;
    for s in ["A", "Å", "É", "Ñ", "g", "j", "p", "q", "y"] {
        let glyph = &shaper.shape_line(s)[0];
        let clipped: usize = glyph
            .pixels
            .iter()
            .filter(|&&(x, y, _)| {
                let (cx, cy) = (x + left_pad, y + base);
                !(cx >= 0 && cy >= 0 && (cx as u32) < cw && (cy as u32) < ch)
            })
            .count();
        assert_eq!(clipped, 0, "glyph {s} has {clipped} clipped pixels");
    }
}

#[test]
fn blit_preserves_negative_sidebearing_pixels() {
    use zet_gpu::text::blit;
    let pixels = vec![(-1, 0, 123u8)];
    let bmp = blit(&pixels, 16.0, 11, 20, 1);
    let px: &[[u8; 4]] = bmp.as_chunks::<4>().0;
    assert_eq!(px[16 * 11][3], 123);
}

#[test]
fn blit_preserves_original_180_alpha_sidebearing_example() {
    use zet_gpu::text::blit;
    let mut pixels = Vec::new();
    for i in 0..10 {
        pixels.push((-1, -12 + i, 0u8));
    }
    pixels.push((-1, -2, 30u8));
    pixels.push((-1, -1, 150u8));
    let in_sum: u32 = pixels.iter().map(|&(_, _, a)| a as u32).sum();
    assert_eq!(in_sum, 180);
    let bmp = blit(&pixels, 16.0, 11, 22, 1);
    let px: &[[u8; 4]] = bmp.as_chunks::<4>().0;
    let out_sum: u32 = px.iter().map(|px| px[3] as u32).sum();
    assert_eq!(out_sum, 180, "sidebearing coverage lost");
}

#[test]
fn blit_applies_x_offset() {
    use zet_gpu::text::blit;
    let pixels = vec![(-1, 0, 77u8)];
    let bmp = blit(&pixels, 16.0, 11, 20, 1);
    let px: &[[u8; 4]] = bmp.as_chunks::<4>().0;
    assert_eq!(px[16 * 11][3], 77);
    let bmp = blit(&pixels, 16.0, 10, 20, 0);
    let px: &[[u8; 4]] = bmp.as_chunks::<4>().0;
    assert_eq!(px[16 * 10][3], 0, "unpadded glyph should clip");
}

#[test]
fn blit_padding_does_not_clip_right_edge() {
    use zet_gpu::text::blit;
    let pixels = vec![(9, 0, 55u8)];
    let bmp = blit(&pixels, 16.0, 11, 20, 1);
    let px: &[[u8; 4]] = bmp.as_chunks::<4>().0;
    assert_eq!(px[16 * 11 + 10][3], 55);
}

#[test]
fn shaper_left_padding_matches_rasterized_sidebearing() {
    let shaper = Shaper::new(16.0);
    assert!(
        shaper.left_pad() >= 1,
        "probe did not discover negative sidebearing"
    );
    let (cw, _ch) = shaper.cell_px();
    assert!(cw > shaper.left_pad() as u32);
}
