//! Semantic native chrome tokens for ZET.
//!
//! The terminal's ANSI palette remains governed by terminal semantics. These
//! roles cover only the surrounding native chrome so a future user theme can
//! replace the visual system without changing panel or evidence meaning.

use crate::quad::Rgba;

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Theme {
    pub space: Rgba,
    pub terminal: Rgba,
    pub surface: Rgba,
    pub surface_raised: Rgba,
    pub surface_inset: Rgba,
    pub border: Rgba,
    pub border_strong: Rgba,
    pub text: Rgba,
    pub secondary: Rgba,
    pub muted: Rgba,
    pub cyan: Rgba,
    pub violet: Rgba,
    pub teal: Rgba,
    pub red: Rgba,
    pub amber: Rgba,
    pub selected: Rgba,
    pub active: Rgba,
    pub danger: Rgba,
    pub top_bar: Rgba,
    pub footer: Rgba,
}

/// Default ZET visual language: deep-space graphite with restrained cyan,
/// violet, teal, red and amber semantic accents.
pub const NEON_GENESIS: Theme = Theme {
    space: Rgba::from_hex(0x080B13),
    terminal: Rgba::from_hex(0x101522),
    surface: Rgba::from_hex(0x0D1422),
    surface_raised: Rgba::from_hex(0x141C2B),
    surface_inset: Rgba::from_hex(0x0A101C),
    border: Rgba::from_hex(0x27354B),
    border_strong: Rgba::from_hex(0x34506C),
    text: Rgba::from_hex(0xE8F1FF),
    secondary: Rgba::from_hex(0x9AAFC6),
    // Lifted slightly from the initial reference value so compact metadata
    // remains readable against the deep-space surfaces at normal DPI.
    muted: Rgba::from_hex(0x7188A1),
    cyan: Rgba::from_hex(0x49DFFF),
    violet: Rgba::from_hex(0xAA8AFF),
    teal: Rgba::from_hex(0x46D4BD),
    red: Rgba::from_hex(0xFF667B),
    amber: Rgba::from_hex(0xF0B96A),
    selected: Rgba::from_hex(0x1B2941),
    active: Rgba::from_hex(0x17263B),
    danger: Rgba::from_hex(0x261B2A),
    top_bar: Rgba::from_hex(0x0B111D),
    footer: Rgba::from_hex(0x0A101B),
};

#[cfg(test)]
mod tests {
    use super::*;

    fn luminance(c: Rgba) -> f32 {
        0.2126 * c.r + 0.7152 * c.g + 0.0722 * c.b
    }

    fn contrast(a: Rgba, b: Rgba) -> f32 {
        let (a, b) = (luminance(a), luminance(b));
        let (hi, lo) = if a >= b { (a, b) } else { (b, a) };
        (hi + 0.05) / (lo + 0.05)
    }

    #[test]
    fn primary_roles_have_readable_contrast() {
        assert!(contrast(NEON_GENESIS.text, NEON_GENESIS.surface) >= 4.5);
        assert!(contrast(NEON_GENESIS.secondary, NEON_GENESIS.surface) >= 4.5);
        assert!(contrast(NEON_GENESIS.cyan, NEON_GENESIS.surface) >= 3.0);
        assert!(contrast(NEON_GENESIS.red, NEON_GENESIS.surface) >= 3.0);
    }

    #[test]
    fn semantic_roles_are_distinct() {
        assert_ne!(NEON_GENESIS.surface, NEON_GENESIS.surface_raised);
        assert_ne!(NEON_GENESIS.cyan, NEON_GENESIS.violet);
        assert_ne!(NEON_GENESIS.teal, NEON_GENESIS.red);
    }
}
