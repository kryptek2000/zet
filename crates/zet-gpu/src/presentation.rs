//! Native presentation primitives shared by ZET's application chrome.
//!
//! Terminal content remains cell-based. These roles describe the independent
//! pixel-positioned presentation layer around it: display labels, section
//! headings, metadata, and evidence text can choose their own size, weight,
//! family, and tracking without changing PTY geometry.

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum UiFontFamily {
    SansSerif,
    Monospace,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct UiTextStyle {
    pub size_px: f32,
    pub line_height_px: f32,
    pub weight: u16,
    pub tracking_px: f32,
    pub family: UiFontFamily,
}

impl UiTextStyle {
    pub const fn new(
        size_px: f32,
        line_height_px: f32,
        weight: u16,
        tracking_px: f32,
        family: UiFontFamily,
    ) -> Self {
        Self {
            size_px,
            line_height_px,
            weight,
            tracking_px,
            family,
        }
    }

    pub const fn baseline_offset(self) -> f32 {
        self.size_px * 0.82
    }
}

/// Compact geometric wordmark role for the application identity.
pub const UI_BRAND: UiTextStyle = UiTextStyle::new(24.0, 29.0, 700, 1.1, UiFontFamily::SansSerif);

/// Upper application/header labels.
pub const UI_HEADER: UiTextStyle = UiTextStyle::new(13.0, 18.0, 700, 1.5, UiFontFamily::Monospace);

/// Indexed section headings such as `COMMAND LEDGER`.
pub const UI_SECTION: UiTextStyle = UiTextStyle::new(12.5, 18.0, 700, 1.1, UiFontFamily::Monospace);

/// Ordinary native labels and observed values.
pub const UI_BODY: UiTextStyle = UiTextStyle::new(12.5, 18.0, 400, 0.15, UiFontFamily::Monospace);

/// Compact state, path, and provenance metadata.
pub const UI_META: UiTextStyle = UiTextStyle::new(10.5, 15.0, 500, 0.15, UiFontFamily::Monospace);

/// Selected command identity and other high-salience evidence labels.
pub const UI_COMMAND: UiTextStyle =
    UiTextStyle::new(16.0, 21.0, 600, 0.05, UiFontFamily::Monospace);

/// Important observed values such as exit status and duration.
pub const UI_VALUE: UiTextStyle = UiTextStyle::new(14.0, 18.0, 500, 0.05, UiFontFamily::Monospace);

/// Captured output stays readable and dense without inheriting terminal cells.
pub const UI_OUTPUT: UiTextStyle = UiTextStyle::new(11.5, 15.0, 400, 0.0, UiFontFamily::Monospace);

/// Pixel-space rectangle used by presentation geometry and hit-test helpers.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct UiRect {
    pub x: u32,
    pub y: u32,
    pub width: u32,
    pub height: u32,
}

impl UiRect {
    pub const fn new(x: u32, y: u32, width: u32, height: u32) -> Self {
        Self {
            x,
            y,
            width,
            height,
        }
    }

    pub fn contains(self, px: f64, py: f64) -> bool {
        px >= self.x as f64
            && py >= self.y as f64
            && px < self.x.saturating_add(self.width) as f64
            && py < self.y.saturating_add(self.height) as f64
    }

    pub const fn inset(self, amount: u32) -> Self {
        let double = amount.saturating_mul(2);
        Self {
            x: self.x.saturating_add(amount),
            y: self.y.saturating_add(amount),
            width: self.width.saturating_sub(double),
            height: self.height.saturating_sub(double),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    #[allow(clippy::assertions_on_constants)]
    fn typography_roles_are_independent() {
        assert!(UI_COMMAND.size_px > UI_BODY.size_px);
        assert!(UI_VALUE.size_px > UI_BODY.size_px);
        assert!(UI_OUTPUT.size_px >= 11.0);
        assert!(UI_BRAND.weight > UI_BODY.weight);
        assert!(UI_SECTION.tracking_px > UI_BODY.tracking_px);
        assert_ne!(UI_BRAND.family, UI_OUTPUT.family);
    }

    #[test]
    fn pixel_rect_hit_testing_is_half_open() {
        let rect = UiRect::new(10, 20, 30, 40);
        assert!(rect.contains(10.0, 20.0));
        assert!(rect.contains(39.99, 59.99));
        assert!(!rect.contains(40.0, 30.0));
        assert!(!rect.contains(20.0, 60.0));
    }

    #[test]
    fn inset_never_underflows() {
        assert_eq!(UiRect::new(2, 3, 4, 5).inset(10), UiRect::new(12, 13, 0, 0));
    }
}
