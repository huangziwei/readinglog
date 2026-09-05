//! Text rasterization over [`crate::font`]'s chain. ab_glyph coverage past
//! 96/255 is a black pixel; an uncovered character draws a hollow box. Glyphs
//! cache per (codepoint, px, face, band).

use std::collections::HashMap;

use ab_glyph::{Font as _, FontVec, ScaleFont as _};
use anyhow::Result;

use crate::eink::fb::Framebuffer;
use crate::font::{self, Band, FontChain};

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
    cache: HashMap<(char, u32, usize, usize), Raster>,
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

    /// Set the size the next draws are at. `px` is the em, whichever face
    /// draws the row — see [`font::scale_of`]. The glyph cache is keyed by size:
    /// switching back and forth costs one pass.
    pub fn set_px(&mut self, px: f32) {
        self.px = px;
    }

    /// How far above the baseline a capital stands, for centring a line inside
    /// a box. [`FontChain::centring`] draws CJK onto that same centre: one
    /// figure places both.
    pub fn cap_height(&self) -> u32 {
        (self.px * font::CAP).round().max(1.0) as u32
    }

    pub fn line_height(&self) -> u32 {
        // Always the primary face's metrics, rounded up: a row keeps its height
        // whichever face draws the text, every face being scaled to the same em
        // with CJK ink centred on the Latin cap.
        let face = self
            .chain
            .primary()
            .as_scaled(font::scale_of(self.chain.primary(), self.px));
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
        let run = font::Script::resolve(script, s);
        let px = self.px;
        let px_key = px.to_bits();
        let mut w = 0u32;
        for ch in s.chars() {
            if font::is_invisible(ch) {
                continue;
            }
            let band = font::band_of(ch, run);
            let advance = match self.glyph(band, ch, px, px_key) {
                Some(glyph) => glyph.advance.round().max(0.0) as u32,
                None => missing_advance(px),
            };
            w = w.saturating_add(advance);
        }
        w
    }

    /// The ink of `s` at the current px, as rows either side of the baseline:
    /// the topmost the glyphs cover and the row past the lowest. `None` where
    /// `s` inks nothing.
    pub fn ink_box(&mut self, s: &str) -> Option<(i32, i32)> {
        let run = font::Script::resolve(font::Script::Unknown, s);
        let (px, px_key) = (self.px, self.px.to_bits());
        let mut box_: Option<(i32, i32)> = None;
        for ch in s.chars().filter(|c| !font::is_invisible(*c)) {
            let band = font::band_of(ch, run);
            let Some(glyph) = self.glyph(band, ch, px, px_key) else {
                continue;
            };
            if glyph.height == 0 {
                continue;
            }
            let (top, bottom) = (glyph.top, glyph.top + glyph.height as i32);
            box_ = Some(match box_ {
                Some((t, b)) => (t.min(top), b.max(bottom)),
                None => (top, bottom),
            });
        }
        box_
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

    /// `ch` rasterized for `band`, from the cache or into it. `None` where no
    /// face in the chain has the character.
    fn glyph(&mut self, band: Band, ch: char, px: f32, px_key: u32) -> Option<&Raster> {
        let face = self.chain.face_for(band, ch)?;
        let drop = self.chain.centring(face, band) * px;
        let font = self.chain.font(face)?;
        Some(
            self.cache
                .entry((ch, px_key, face, band.slot()))
                .or_insert_with(|| rasterize(font, ch, px, drop)),
        )
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

    /// [`TextRenderer::draw`] under a known `script`, which decides the Han
    /// convention and the order faces are tried in. Each character comes from
    /// its own band: Latin inside a CJK title comes off the UI face.
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
        let run = font::Script::resolve(script, s);
        let px = self.px;
        let px_key = px.to_bits();
        let mut cur_x = x;
        for ch in s.chars() {
            if font::is_invisible(ch) {
                continue;
            }
            let band = font::band_of(ch, run);
            match self.glyph(band, ch, px, px_key) {
                Some(glyph) => {
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

/// `ch` outlined from `font` at an em of `px`, with its coverage, dropped by
/// `drop` pixels. ab_glyph works in screen space, y downward from the
/// baseline: its bounds are the blit's offsets.
fn rasterize(font: &FontVec, ch: char, px: f32, drop: f32) -> Raster {
    let scale = font::scale_of(font, px);
    let id = font.glyph_id(ch);
    let advance = font.as_scaled(scale).h_advance(id);
    let Some(outline) = font.outline_glyph(id.with_scale(scale)) else {
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
        top: (bounds.min.y + drop).round() as i32,
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
    // put_pixel applies the orientation transform + bounds check, at a
    // per-pixel cost a glyph of ≤32x32 absorbs.
    for row in 0..h {
        let cov_row = &coverage[row * w..row * w + w];
        for (col, &cov) in cov_row.iter().enumerate() {
            if cov >= COVERAGE_THRESHOLD {
                fb.put_pixel(x + col as i32, y + row as i32, fg);
            }
        }
    }
}
