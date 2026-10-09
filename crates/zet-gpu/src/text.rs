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
}

impl Shaper {
    pub fn new(font_px: f32) -> Self {
        Shaper {
            font_system: FontSystem::new(),
            cache: SwashCache::new(),
            metrics: Metrics::new(font_px, font_px * 1.25),
        }
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
