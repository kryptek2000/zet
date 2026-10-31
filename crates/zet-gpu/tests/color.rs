use zet_gpu::quad::Rgba;
use zet_gpu::renderer::term_color;
use zet_term::Color;

#[test]
fn term_colors_map_to_expected_rgb() {
    // Palette values are sRGB display colors converted to linear light
    // (sRGB swapchain); red 0.8 -> ~0.604 linear, 0.2 -> ~0.033.
    let red = term_color(Color::Indexed(1), false);
    assert!((red.r - 0.6037).abs() < 0.002, "red.r = {}", red.r);
    assert!((red.g - 0.0331).abs() < 0.002, "red.g = {}", red.g);
    assert_eq!(term_color(Color::Default, false), Rgba::WHITE);
    let rgb = term_color(Color::Rgb(255, 255, 255), false);
    assert!((rgb.r - 1.0).abs() < 1e-6 && (rgb.g - 1.0).abs() < 1e-6);
    // Low end exercises the linear branch (10/255 = 0.0392 <= 0.04045).
    let low = term_color(Color::Rgb(10, 20, 30), false);
    assert!(
        (low.r - 10.0 / 255.0 / 12.92).abs() < 1e-6,
        "low.r = {}",
        low.r
    );
    assert!((low.g - 0.0063).abs() < 0.001, "low.g = {}", low.g);
}

#[test]
fn bold_brightens_standard_colors() {
    // Classic terminal semantics: bold lifts colors 0-7 to 8-15.
    assert_eq!(
        term_color(Color::Indexed(6), true),
        term_color(Color::Indexed(14), false)
    );
    assert_eq!(
        term_color(Color::Indexed(0), true),
        term_color(Color::Indexed(8), false)
    );
    // Already-bright, default, and truecolor are unaffected.
    assert_eq!(
        term_color(Color::Indexed(9), true),
        term_color(Color::Indexed(9), false)
    );
    assert_eq!(term_color(Color::Default, true), Rgba::WHITE);
}

#[test]
fn indexed_256_palette_uses_standard_cube_and_grayscale() {
    // Cube endpoints and a mid-gray entry.
    assert_eq!(
        term_color(Color::Indexed(16), false),
        Rgba::srgb(0.0, 0.0, 0.0)
    );
    assert_eq!(
        term_color(Color::Indexed(231), false),
        Rgba::srgb(1.0, 1.0, 1.0)
    );
    assert_eq!(
        term_color(Color::Indexed(232), false),
        Rgba::srgb(8.0 / 255.0, 8.0 / 255.0, 8.0 / 255.0)
    );
    assert_eq!(
        term_color(Color::Indexed(255), false),
        Rgba::srgb(238.0 / 255.0, 238.0 / 255.0, 238.0 / 255.0)
    );

    // Representative cube entries.
    let c21 = term_color(Color::Indexed(21), false);
    assert!((c21.b - 1.0).abs() < 1e-3, "21 b = {}", c21.b);
    let c46 = term_color(Color::Indexed(46), false);
    assert!((c46.g - 1.0).abs() < 1e-3, "46 g = {}", c46.g);
    let c196 = term_color(Color::Indexed(196), false);
    assert!((c196.r - 1.0).abs() < 1e-3, "196 r = {}", c196.r);

    // Index 244 must be grayscale, not wrap into the 16-color palette.
    let c244 = term_color(Color::Indexed(244), false);
    let expected244 = Rgba::srgb(128.0 / 255.0, 128.0 / 255.0, 128.0 / 255.0);
    assert!((c244.r - expected244.r).abs() < 1e-3, "244 r = {}", c244.r);
    assert!((c244.g - expected244.g).abs() < 1e-3, "244 g = {}", c244.g);
    assert!((c244.b - expected244.b).abs() < 1e-3, "244 b = {}", c244.b);
    assert_ne!(
        term_color(Color::Indexed(244), false),
        term_color(Color::Indexed(8), false)
    );
}
