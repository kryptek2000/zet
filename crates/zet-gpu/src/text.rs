// CPU text shaping via cosmic-text: layout monospace lines and rasterize
// glyph coverage masks. The renderer uploads these into the atlas texture.

use cosmic_text::{Attrs, Buffer, Family, FontSystem, Metrics, Shaping, SwashCache, Weight};

use crate::presentation::UiFontFamily;

#[derive(Debug, Clone)]
pub struct ShapedGlyph {
    pub advance: f32,
    /// (x, y, alpha) coverage pixels relative to the pen origin.
    pub pixels: Vec<(i32, i32, u8)>,
}

/// A variable-size native UI glyph rasterized independently of terminal cells.
#[derive(Debug, Clone)]
pub struct UiGlyphRaster {
    pub pixels: Vec<u8>,
    pub width: u32,
    pub height: u32,
    pub offset_x: i32,
    pub offset_y: i32,
    pub advance: f32,
}

pub struct Shaper {
    font_system: FontSystem,
    cache: SwashCache,
    metrics: Metrics,
    ascent: f32,
    descent: f32,
    advance: f32,
    left_pad: i32,
}

impl Shaper {
    pub fn new(font_px: f32) -> Self {
        let metrics = Metrics::new(font_px, font_px * 1.25);
        crate::perf_mark("fontdb_load_start");
        let font_system = FontSystem::new();
        crate::perf_mark("fontdb_load_done");
        let mut shaper = Shaper {
            font_system,
            cache: SwashCache::new(),
            metrics,
            ascent: font_px * 0.8,
            descent: font_px * 0.25,
            advance: font_px * 0.6,
            left_pad: 1,
        };
        crate::perf_mark("probe_start");
        if let Some((ascent, descent, advance, left_pad)) = shaper.probe() {
            shaper.ascent = ascent;
            shaper.descent = descent;
            shaper.advance = advance;
            shaper.left_pad = left_pad;
        }
        crate::perf_mark("probe_done");
        shaper
    }

    /// Probe the selected monospace font for rasterized bounds. Ascent and
    /// descent come from actual glyph coverage, including tall capitals and
    /// negative horizontal sidebearings. Advance and left padding define the
    /// terminal cell width. Falls back to estimates when fontless.
    fn probe(&mut self) -> Option<(f32, f32, f32, i32)> {
        let mut buffer = Buffer::new(&mut self.font_system, self.metrics);
        buffer.set_size(Some(4096.0), Some(self.metrics.line_height));
        let attrs = Attrs::new().family(Family::Monospace);
        buffer.set_text("AgjpqyMÅÉÑ─Ɲ", &attrs, Shaping::Advanced, None);
        buffer.shape_until_scroll(&mut self.font_system, false);
        let run = buffer.layout_runs().next()?;
        let layout_ascent = run.line_y - run.line_top;
        let mut max_y = 0i32;
        let mut min_y = 0i32;
        let mut min_x = 0i32;
        let mut advance = 0.0f32;
        for glyph in run.glyphs {
            advance = advance.max(glyph.w);
            let physical = glyph.physical((0.0, 0.0), 1.0);
            self.cache.with_pixels(
                &mut self.font_system,
                physical.cache_key,
                cosmic_text::Color::rgb(0xFF, 0xFF, 0xFF),
                |x, y, _| {
                    max_y = max_y.max(y);
                    min_y = min_y.min(y);
                    min_x = min_x.min(x);
                },
            );
        }
        if advance <= 0.0 {
            return None;
        }
        // Guard against fonts/hinting where the rasterized glyph extends
        // above the layout ascent: use the actual top extent so caps are
        // not clipped. Descent from rasterized bottom extent.
        let ascent = layout_ascent.max((-min_y) as f32);
        let left_pad = (-min_x).max(1);
        // max_y is pen-relative (baseline = 0, below = positive):
        // descent is the extent below the baseline.
        Some((ascent, (max_y + 1).max(0) as f32, advance.ceil(), left_pad))
    }

    pub fn ascent_px(&self) -> f32 {
        self.ascent
    }

    pub fn descent_px(&self) -> f32 {
        self.descent
    }

    pub fn left_pad(&self) -> i32 {
        self.left_pad
    }

    /// Terminal cell size: advance plus left padding wide, tall enough for
    /// ascent, descent, and baseline placement.
    pub fn cell_px(&self) -> (u32, u32) {
        (
            (self.advance.ceil() + self.left_pad as f32).max(1.0) as u32,
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

    /// Rasterize one native presentation glyph at its requested size and
    /// weight. Terminal shaping continues to use the fixed probe metrics;
    /// this method deliberately creates independent metrics for chrome text.
    pub fn rasterize_ui_glyph(
        &mut self,
        c: char,
        font_px: f32,
        line_height_px: f32,
        weight: u16,
        family: UiFontFamily,
    ) -> UiGlyphRaster {
        let metrics = Metrics::new(font_px.max(1.0), line_height_px.max(font_px));
        let mut buffer = Buffer::new(&mut self.font_system, metrics);
        buffer.set_size(Some(256.0), Some(metrics.line_height));
        let family = match family {
            UiFontFamily::SansSerif => Family::SansSerif,
            UiFontFamily::Monospace => Family::Monospace,
        };
        let attrs = Attrs::new().family(family).weight(Weight(weight));
        let text = c.to_string();
        buffer.set_text(&text, &attrs, Shaping::Advanced, None);
        buffer.shape_until_scroll(&mut self.font_system, false);

        let mut advance = font_px * 0.6;
        let mut points: Vec<(i32, i32, u8)> = Vec::new();
        if let Some(run) = buffer.layout_runs().next() {
            if let Some(glyph) = run.glyphs.first() {
                advance = glyph.w.max(0.0);
                let physical = glyph.physical((0.0, 0.0), 1.0);
                self.cache.with_pixels(
                    &mut self.font_system,
                    physical.cache_key,
                    cosmic_text::Color::rgb(0xFF, 0xFF, 0xFF),
                    |x, y, color| {
                        let alpha = (color.0 >> 24) as u8;
                        if alpha > 0 {
                            points.push((x, y, alpha));
                        }
                    },
                );
            }
        }

        if points.is_empty() {
            return UiGlyphRaster {
                pixels: Vec::new(),
                width: 0,
                height: 0,
                offset_x: 0,
                offset_y: 0,
                advance,
            };
        }

        let min_x = points.iter().map(|(x, _, _)| *x).min().unwrap_or(0);
        let max_x = points.iter().map(|(x, _, _)| *x).max().unwrap_or(0);
        let min_y = points.iter().map(|(_, y, _)| *y).min().unwrap_or(0);
        let max_y = points.iter().map(|(_, y, _)| *y).max().unwrap_or(0);
        let padding = 1i32;
        let width = (max_x - min_x + 1 + padding * 2).max(1) as u32;
        let height = (max_y - min_y + 1 + padding * 2).max(1) as u32;
        let mut pixels = vec![0u8; width as usize * height as usize * 4];
        for (x, y, alpha) in points {
            let x = (x - min_x + padding) as u32;
            let y = (y - min_y + padding) as u32;
            let index = ((y * width + x) * 4) as usize;
            pixels[index] = 255;
            pixels[index + 1] = 255;
            pixels[index + 2] = 255;
            pixels[index + 3] = pixels[index + 3].max(alpha);
        }

        UiGlyphRaster {
            pixels,
            width,
            height,
            offset_x: min_x - padding,
            offset_y: min_y - padding,
            advance,
        }
    }
}

/// Blit pen-relative coverage into a cell bitmap (white RGBA, alpha mask).
/// The pen is placed on the baseline at row `ascent`; `x_offset` applies the
/// font's left padding so negative sidebearings remain inside the cell.
/// Pixels outside the padded cell are dropped (never wrap into neighbors).
pub fn blit(
    pixels: &[(i32, i32, u8)],
    ascent: f32,
    cell_w: u32,
    cell_h: u32,
    x_offset: i32,
) -> Vec<u8> {
    let mut rgba = vec![0u8; cell_w as usize * cell_h as usize * 4];
    let base = ascent.round() as i32;
    for &(x, y, alpha) in pixels {
        let (cx, cy) = (x + x_offset, y + base);
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
