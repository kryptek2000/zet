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
