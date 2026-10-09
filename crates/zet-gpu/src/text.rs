// CPU text shaping via cosmic-text: layout monospace lines and rasterize
// glyph coverage masks. The renderer uploads these into the atlas texture.

use cosmic_text::{Attrs, Buffer, Family, FontSystem, Metrics, Shaping, SwashCache};

#[derive(Debug, Clone)]
pub struct ShapedGlyph {
    pub advance: f32,
    /// (x, y, alpha) coverage pixels relative to the pen origin.
    pub pixels: Vec<(i32, i32, u8)>,
}

pub struct Shaper {
    font_system: FontSystem,
    cache: SwashCache,
    metrics: Metrics,
    ascent: f32,
    descent: f32,
    advance: f32,
}

impl Shaper {
    pub fn new(font_px: f32) -> Self {
        let metrics = Metrics::new(font_px, font_px * 1.25);
        let mut shaper = Shaper {
            font_system: FontSystem::new(),
            cache: SwashCache::new(),
            metrics,
            ascent: font_px * 0.8,
            descent: font_px * 0.25,
            advance: font_px * 0.6,
        };
        if let Some((ascent, descent, advance)) = shaper.probe() {
            shaper.ascent = ascent;
            shaper.descent = descent;
            shaper.advance = advance;
        }
        shaper
    }

    /// Probe the real monospace font: baseline offset from cosmic's line
    /// layout, descent + advance from rasterized coverage. Falls back to
    /// estimates when fontless so nothing panics.
    fn probe(&mut self) -> Option<(f32, f32, f32)> {
        let mut buffer = Buffer::new(&mut self.font_system, self.metrics);
        buffer.set_size(Some(4096.0), Some(self.metrics.line_height));
        let attrs = Attrs::new().family(Family::Monospace);
        buffer.set_text("AgjpqyM", &attrs, Shaping::Advanced, None);
        buffer.shape_until_scroll(&mut self.font_system, false);
        let run = buffer.layout_runs().next()?;
        let ascent = run.line_y - run.line_top;
        let mut max_y = 0i32;
        let mut advance = 0.0f32;
        for glyph in run.glyphs {
            advance = advance.max(glyph.w);
            let physical = glyph.physical((0.0, 0.0), 1.0);
            self.cache.with_pixels(
                &mut self.font_system,
                physical.cache_key,
                cosmic_text::Color::rgb(0xFF, 0xFF, 0xFF),
                |_, y, _| {
                    max_y = max_y.max(y);
                },
            );
        }
        if advance <= 0.0 {
            return None;
        }
        // max_y is pen-relative (baseline = 0, below = positive):
        // descent is the extent below the baseline.
        Some((ascent, (max_y + 1).max(0) as f32, advance.ceil()))
    }

    pub fn ascent_px(&self) -> f32 {
        self.ascent
    }

    pub fn descent_px(&self) -> f32 {
        self.descent
    }

    /// Terminal cell size: advance-wide, tall enough for ascent + descent.
    pub fn cell_px(&self) -> (u32, u32) {
        (
            self.advance.ceil().max(1.0) as u32,
            (self.ascent + self.descent + 1.0).ceil().max(1.0) as u32,
        )
    }

    pub fn cell_size(&self) -> (f32, f32) {
        (self.metrics.font_size, self.metrics.line_height)
    }

    pub fn shape_line(&mut self, text: &str) -> Vec<ShapedGlyph> {
        let mut buffer = Buffer::new(&mut self.font_system, self.metrics);
        buffer.set_size(Some(4096.0), Some(self.metrics.line_height));
        let attrs = Attrs::new().family(Family::Monospace);
        buffer.set_text(text, &attrs, Shaping::Advanced, None);
        buffer.shape_until_scroll(&mut self.font_system, false);

        let mut out = Vec::new();
        for run in buffer.layout_runs() {
            for glyph in run.glyphs {
                let physical = glyph.physical((0.0, 0.0), 1.0);
                let mut pixels = Vec::new();
                self.cache.with_pixels(
                    &mut self.font_system,
                    physical.cache_key,
                    cosmic_text::Color::rgb(0xFF, 0xFF, 0xFF),
                    |x, y, color| {
                        pixels.push((x, y, (color.0 >> 24) as u8));
                    },
                );
                out.push(ShapedGlyph {
                    advance: glyph.w,
                    pixels,
                });
            }
        }
        out
    }
}

/// Blit pen-relative coverage into a cell bitmap (white RGBA, alpha mask),
/// placing the pen on the baseline: cell row = y + ascent.
/// Pixels outside the cell are dropped (never wrap into neighbors).
pub fn blit(pixels: &[(i32, i32, u8)], ascent: f32, cell_w: u32, cell_h: u32) -> Vec<u8> {
    let mut rgba = vec![0u8; cell_w as usize * cell_h as usize * 4];
    let base = ascent.round() as i32;
    for &(x, y, alpha) in pixels {
        let (cx, cy) = (x, y + base);
        if cx >= 0 && cy >= 0 && (cx as u32) < cell_w && (cy as u32) < cell_h {
            let i = ((cy as usize) * cell_w as usize + cx as usize) * 4;
            rgba[i] = 255;
            rgba[i + 1] = 255;
            rgba[i + 2] = 255;
            rgba[i + 3] = alpha.max(rgba[i + 3]);
        }
    }
    rgba
}
