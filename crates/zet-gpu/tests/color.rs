use zet_gpu::quad::Rgba;
use zet_gpu::renderer::term_color;
use zet_term::Color;

#[test]
fn term_colors_map_to_expected_rgb() {
    assert_eq!(term_color(Color::Default, false), Rgba::WHITE);
    assert_eq!(
        term_color(Color::Indexed(1), false),
        Rgba::rgb(0.80, 0.20, 0.20)
    );
    assert_eq!(
        term_color(Color::Indexed(2), false),
        Rgba::rgb(0.20, 0.70, 0.20)
    );
    assert_eq!(
        term_color(Color::Indexed(9), false),
        Rgba::rgb(1.00, 0.35, 0.35)
    );
    assert_eq!(
        term_color(Color::Rgb(10, 20, 30), false),
        Rgba::rgb(10.0 / 255.0, 20.0 / 255.0, 30.0 / 255.0)
    );
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
