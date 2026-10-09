use zet_gpu::quad::Rgba;
use zet_gpu::renderer::term_color;
use zet_term::Color;

#[test]
fn term_colors_map_to_expected_rgb() {
    assert_eq!(term_color(Color::Default), Rgba::WHITE);
    assert_eq!(term_color(Color::Indexed(1)), Rgba::rgb(0.80, 0.20, 0.20));
    assert_eq!(term_color(Color::Indexed(2)), Rgba::rgb(0.20, 0.70, 0.20));
    assert_eq!(term_color(Color::Indexed(9)), Rgba::rgb(1.00, 0.35, 0.35));
    assert_eq!(
        term_color(Color::Rgb(10, 20, 30)),
        Rgba::rgb(10.0 / 255.0, 20.0 / 255.0, 30.0 / 255.0)
    );
}
