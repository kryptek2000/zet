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

/// Owner-approved Aperture Instrument visual language: deep-space near-black
/// graphite surfaces with restrained cyan, violet, teal, red and amber semantic accents.
pub const APERTURE_INSTRUMENT: Theme = Theme {
    space: Rgba::from_hex(0x05080E),
    terminal: Rgba::from_hex(0x080C14),
    surface: Rgba::from_hex(0x0A0F1A),
    surface_raised: Rgba::from_hex(0x111A28),
    surface_inset: Rgba::from_hex(0x070B12),
    border: Rgba::from_hex(0x192638),
    border_strong: Rgba::from_hex(0x24364E),
    text: Rgba::from_hex(0xE8F1FF),
    secondary: Rgba::from_hex(0x889EB8),
    // Compact metadata and muted line numbers on dark graphite surfaces.
    muted: Rgba::from_hex(0x7188A1),
    cyan: Rgba::from_hex(0x38D9FA),
    violet: Rgba::from_hex(0x9678E8),
    teal: Rgba::from_hex(0x34D399),
    red: Rgba::from_hex(0xF43F5E),
    amber: Rgba::from_hex(0xF59E0B),
    selected: Rgba::from_hex(0x0E2333),
    active: Rgba::from_hex(0x0B1E2B),
    danger: Rgba::from_hex(0x251219),
    top_bar: Rgba::from_hex(0x05080E),
    footer: Rgba::from_hex(0x070B12),
};

/// Compatibility name for code and downstream theme experiments that still
/// refer to the earlier Neon Genesis token set. The values are now the
/// owner-approved Aperture Instrument palette.
pub const NEON_GENESIS: Theme = APERTURE_INSTRUMENT;

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
        assert!(contrast(APERTURE_INSTRUMENT.text, APERTURE_INSTRUMENT.surface) >= 4.5);
        assert!(contrast(APERTURE_INSTRUMENT.secondary, APERTURE_INSTRUMENT.surface) >= 4.5);
        assert!(contrast(APERTURE_INSTRUMENT.muted, APERTURE_INSTRUMENT.surface) >= 3.0);
        assert!(contrast(APERTURE_INSTRUMENT.cyan, APERTURE_INSTRUMENT.surface) >= 3.0);
        assert!(contrast(APERTURE_INSTRUMENT.red, APERTURE_INSTRUMENT.surface) >= 3.0);
    }

    #[test]
    fn semantic_roles_are_distinct() {
        assert_ne!(
            APERTURE_INSTRUMENT.surface,
            APERTURE_INSTRUMENT.surface_raised
        );
        assert_ne!(APERTURE_INSTRUMENT.cyan, APERTURE_INSTRUMENT.violet);
        assert_ne!(APERTURE_INSTRUMENT.teal, APERTURE_INSTRUMENT.red);
    }

    #[test]
    fn aperture_stays_predominantly_near_black() {
        let theme = APERTURE_INSTRUMENT;
        assert!(theme.space.r < 0.1);
        assert!(theme.surface.r < 0.1);
        assert!(theme.surface_inset.r < 0.1);
        assert_eq!(NEON_GENESIS, theme);
    }
}
