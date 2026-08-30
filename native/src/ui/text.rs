//! Text rasterization over [`crate::font`]'s chain. ab_glyph coverage past
//! 96/255 is a black pixel; an uncovered character draws a hollow box. Glyphs
//! cache per (codepoint, px, face).

use std::collections::HashMap;

use ab_glyph::{Font as _, FontVec, ScaleFont as _};
use anyhow::Result;

use crate::eink::fb::Framebuffer;
use crate::font::{self, FontChain};

const COVERAGE_THRESHOLD: u8 = 96;

/// One rasterized glyph. `left` runs from the pen's x, `top` from the baseline
/// downward. A glyph with no outline keeps its advance over an empty bitmap.
struct Raster {
    advance: f32,
    left: i32,
    top: i32,
    width: usize,
    height: usize,
    coverage: Vec<u8>,
}

pub struct TextRenderer {
    chain: FontChain,
    px: f32,
    cache: HashMap<(char, u32, usize), Raster>,
}

impl TextRenderer {
    pub fn load(px: f32) -> Result<Self> {
        Ok(Self {
            chain: FontChain::load(&font::discover())?,
            px,
            cache: HashMap::new(),
        })
    }

    /// The fallback chain this device ended up with, primary first, for the
    /// startup log — see [`FontChain::paths`].
    pub fn chain_description(&self) -> String {
        self.chain
            .paths()
            .map(|path| path.display().to_string())
            .collect::<Vec<_>>()
            .join(" -> ")
    }

    /// Set the size the next draws are at.
    ///
    /// A screen mixes half a dozen sizes and one chain serves them all: the
    /// glyph cache is keyed by size as well as by codepoint and face, so
    /// switching back and forth costs nothing after the first pass.
    pub fn set_px(&mut self, px: f32) {
        self.px = px;
    }

    /// How far above the baseline a capital stands, for centring a line inside
    /// a box rather than hanging it off the baseline.
    pub fn cap_height(&self) -> u32 {
        (self.px * 0.72).round().max(1.0) as u32
    }

    pub fn line_height(&self) -> u32 {
        // The face's own vertical metrics; round up so adjacent rows don't
        // tear into each other. Always the primary face's, so a row keeps its
        // height whichever face draws the text.
        let face = self.chain.primary().as_scaled(self.px);
        (face.height() + face.line_gap()).ceil().max(1.0) as u32
    }

    /// Total advance width of `s` at the current px, resolving faces the way
    /// [`TextRenderer::draw`] does over the same string.
    pub fn measure_width(&mut self, s: &str) -> u32 {
        self.measure_width_in(font::Script::Unknown, s)
    }

    /// [`TextRenderer::measure_width`] for text whose language is known — see
    /// [`TextRenderer::draw_in`].
    pub fn measure_width_in(&mut self, script: font::Script, s: &str) -> u32 {
        let selection = self.chain.select(s, script);
        let px = self.px;
        let px_key = px.to_bits();
        let mut w = 0u32;
        for ch in s.chars() {
            if font::is_invisible(ch) {
                continue;
            }
            let advance = match self.chain.glyph_source(selection, ch) {
                Some((face, font)) => {
                    let entry = self
                        .cache
                        .entry((ch, px_key, face))
                        .or_insert_with(|| rasterize(font, ch, px));
                    entry.advance.round().max(0.0) as u32
                }
                None => missing_advance(px),
            };
            w = w.saturating_add(advance);
        }
        w
    }

    /// A font-backed [`crate::wrap::wrap_and_clamp`]: `text` to `max_width` per
    /// line, clamped to `max_lines` with the dropped tail ellipsized.
    pub fn wrap_and_clamp(&mut self, text: &str, max_width: u32, max_lines: usize) -> Vec<String> {
        self.wrap_and_clamp_in(font::Script::Unknown, text, max_width, max_lines)
    }

    /// [`TextRenderer::wrap_and_clamp`] for text whose language is known — see
    /// [`TextRenderer::draw_in`].
    pub fn wrap_and_clamp_in(
        &mut self,
        script: font::Script,
        text: &str,
        max_width: u32,
        max_lines: usize,
    ) -> Vec<String> {
        crate::wrap::wrap_and_clamp(text, max_width, max_lines, |s| {
            self.measure_width_in(script, s)
        })
    }
}

impl TextRenderer {
    /// `s` from the baseline `(x, y_baseline)`, returning the advanced x.
    /// `inverted` draws white-on-black.
    pub fn draw(
        &mut self,
        fb: &mut Framebuffer,
        x: i32,
        y_baseline: i32,
        s: &str,
        inverted: bool,
    ) -> i32 {
        self.draw_in(font::Script::Unknown, fb, x, y_baseline, s, inverted)
    }

    /// [`TextRenderer::draw`] under a known `script`, which orders the faces
    /// tried. Coverage decides which one draws.
    pub fn draw_in(
        &mut self,
        script: font::Script,
        fb: &mut Framebuffer,
        x: i32,
        y_baseline: i32,
        s: &str,
        inverted: bool,
    ) -> i32 {
        let fg = if inverted { 0xFF } else { 0x00 };
        let selection = self.chain.select(s, script);
        let px = self.px;
        let px_key = px.to_bits();
        let mut cur_x = x;
        for ch in s.chars() {
            if font::is_invisible(ch) {
                continue;
            }
            match self.chain.glyph_source(selection, ch) {
                // Cache key uses bit pattern of f32 — same px always keys the same.
                Some((face, font)) => {
                    let glyph = self
                        .cache
                        .entry((ch, px_key, face))
                        .or_insert_with(|| rasterize(font, ch, px));
                    blit_threshold(
                        fb,
                        cur_x + glyph.left,
                        y_baseline + glyph.top,
                        glyph.width,
                        glyph.height,
                        &glyph.coverage,
                        fg,
                    );
                    cur_x += glyph.advance.round() as i32;
                }
                None => {
                    draw_missing(fb, cur_x, y_baseline, px, fg);
                    cur_x += missing_advance(px) as i32;
                }
            }
        }
        cur_x
    }
}

/// `ch` outlined from `font` at `px`, with its coverage. ab_glyph works in
/// screen space, y downward from the baseline, so its bounds are the blit's
/// offsets.
fn rasterize(font: &FontVec, ch: char, px: f32) -> Raster {
    let id = font.glyph_id(ch);
    let advance = font.as_scaled(px).h_advance(id);
    let Some(outline) = font.outline_glyph(id.with_scale(px)) else {
        return Raster {
            advance,
            left: 0,
            top: 0,
            width: 0,
            height: 0,
            coverage: Vec::new(),
        };
    };
    // Whole pixels, floored and ceiled. ab_glyph sizes its grid with this same
    // expression, holding the buffer to the extent `draw` emits into.
    let bounds = outline.px_bounds();
    let (width, height) = (bounds.width() as usize, bounds.height() as usize);
    let mut coverage = vec![0u8; width * height];
    outline.draw(|x, y, c| {
        let (x, y) = (x as usize, y as usize);
        if x < width && y < height {
            coverage[y * width + x] = (c * 255.0).round().clamp(0.0, 255.0) as u8;
        }
    });
    Raster {
        advance,
        left: bounds.min.x.round() as i32,
        top: bounds.min.y.round() as i32,
        width,
        height,
        coverage,
    }
}

/// Advance of the missing-glyph mark, an ideograph's share of the line. Shared
/// by [`TextRenderer::measure_width`] and [`TextRenderer::draw`].
fn missing_advance(px: f32) -> u32 {
    (px * 0.72).round().max(6.0) as u32
}

/// A hollow box standing on the baseline, for a character no face in the
/// chain has. Stroked 2px on purpose: a hairline outline is exactly what
/// makes a font's own `.notdef` fall apart under [`COVERAGE_THRESHOLD`].
fn draw_missing(fb: &mut Framebuffer, x: i32, y_baseline: i32, px: f32, fg: u8) {
    const STROKE: i32 = 2;
    let (left, right) = (x + STROKE, x + missing_advance(px) as i32 - STROKE * 2);
    let (top, bottom) = (y_baseline - (px * 0.66).round() as i32, y_baseline - STROKE);
    if right - left < STROKE * 2 || bottom - top < STROKE * 2 {
        return;
    }
    for row in top..=bottom {
        let horizontal_edge = row < top + STROKE || row > bottom - STROKE;
        for col in left..=right {
            let vertical_edge = col < left + STROKE || col > right - STROKE;
            if horizontal_edge || vertical_edge {
                fb.put_pixel(col, row, fg);
            }
        }
    }
}

fn blit_threshold(
    fb: &mut Framebuffer,
    x: i32,
    y: i32,
    w: usize,
    h: usize,
    coverage: &[u8],
    fg: u8,
) {
    if w == 0 || h == 0 {
        return;
    }
    // put_pixel applies the orientation transform + bounds check. Glyphs
    // are small (≤32x32 typically), so per-pixel call overhead is fine.
    for row in 0..h {
        let cov_row = &coverage[row * w..row * w + w];
        for (col, &cov) in cov_row.iter().enumerate() {
            if cov >= COVERAGE_THRESHOLD {
                fb.put_pixel(x + col as i32, y + row as i32, fg);
            }
        }
    }
}
