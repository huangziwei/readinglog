//! Text rasterization over [`crate::font`]'s chain. ab_glyph coverage past
//! 96/255 is a black pixel; an uncovered character draws a hollow box. Glyphs
//! cache per (codepoint, px, face, band), bounded at [`CACHE_CAP`].

use std::collections::HashMap;

use ab_glyph::{Font as _, FontVec, ScaleFont as _};
use anyhow::Result;

use crate::eink::fb::Framebuffer;
use crate::font::{self, Band, FontChain};

const COVERAGE_THRESHOLD: u8 = 96;

/// The most glyphs held at once. The key carries the size and face too, so a
/// CJK reader fills this several times in one session and the cache must give
/// ground rather than grow. A miss costs one re-rasterization.
const CACHE_CAP: usize = 4_096;

/// How much of the cache one eviction gives up. Dropping a quarter at a time
/// makes the scan that finds the oldest amortize over the inserts that follow,
/// rather than running on every insert once the cap is reached.
const CACHE_EVICT: usize = CACHE_CAP / 4;

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

/// A cached glyph and when it was last drawn, on [`TextRenderer::clock`].
struct Cached {
    raster: Raster,
    used: u64,
}

pub struct TextRenderer {
    chain: FontChain,
    px: f32,
    cache: HashMap<(char, u32, usize, usize), Cached>,
    /// Ticks once per glyph reached, ordering the cache for eviction.
    clock: u64,
}

impl TextRenderer {
    pub fn load(px: f32) -> Result<Self> {
        Ok(Self {
            chain: FontChain::load(&font::discover())?,
            px,
            cache: HashMap::new(),
            clock: 0,
        })
    }

    /// The fallback chain in one line for the startup log: how many faces
    /// resolved and the one Latin is set in. The paths run to kilobytes and
    /// belong nowhere in a log.
    pub fn chain_summary(&self) -> String {
        let faces = self.chain.paths().count();
        let primary = self
            .chain
            .paths()
            .next()
            .and_then(|path| path.file_name())
            .map(|name| name.to_string_lossy().into_owned())
            .unwrap_or_default();
        format!("{faces} faces, primary={primary}")
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

    /// 四分アキ at this size: a quarter of the em, to the pixel. `crate::wrap`
    /// names the pairs it stands between.
    fn aki_px(&self) -> u32 {
        (self.px / 4.0).round() as u32
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

    /// Where [`TextRenderer::draw_in`] sets its pen to draw the character at
    /// the byte offset `at` of `s`: the run up to there, and the 四分アキ
    /// standing at that boundary.
    pub fn measure_upto_in(&mut self, script: font::Script, s: &str, at: usize) -> u32 {
        let (head, tail) = s.split_at(at);
        let gap = match (head.chars().next_back(), tail.chars().next()) {
            (Some(a), Some(b)) if crate::wrap::aki(a, b) => self.aki_px(),
            _ => 0,
        };
        self.measure_width_in(script, head).saturating_add(gap)
    }

    /// [`TextRenderer::measure_width`] for text whose language is known — see
    /// [`TextRenderer::draw_in`].
    pub fn measure_width_in(&mut self, script: font::Script, s: &str) -> u32 {
        self.measured(script, s, self.aki_px())
    }

    /// [`TextRenderer::measure_width`] with the script boundaries set solid.
    /// A figure and the counter after it read as one number — see
    /// [`TextRenderer::draw_solid`].
    pub fn measure_solid(&mut self, s: &str) -> u32 {
        self.measured(font::Script::Unknown, s, 0)
    }

    fn measured(&mut self, script: font::Script, s: &str, quarter: u32) -> u32 {
        let run = font::Script::resolve(script, s);
        let px = self.px;
        let px_key = px.to_bits();
        let mut w = 0u32;
        let mut prev: Option<char> = None;
        for ch in s.chars() {
            if font::is_invisible(ch) {
                continue;
            }
            let band = font::band_of(ch, run);
            let advance = match self.glyph(band, ch, px, px_key) {
                Some(glyph) => glyph.advance.round().max(0.0) as u32,
                None => missing_advance(px),
            };
            if prev.is_some_and(|a| crate::wrap::aki(a, ch)) {
                w = w.saturating_add(quarter);
            }
            w = w.saturating_add(advance);
            prev = Some(ch);
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
        let key = (ch, px_key, face, band.slot());
        self.clock += 1;
        let now = self.clock;
        if !self.cache.contains_key(&key) {
            self.evict();
            let drop = self.chain.centring(face, band) * px;
            let font = self.chain.font(face)?;
            let raster = rasterize(font, ch, px, drop);
            self.cache.insert(key, Cached { raster, used: now });
        }
        let held = self.cache.get_mut(&key)?;
        held.used = now;
        Some(&held.raster)
    }

    /// Give up the oldest [`CACHE_EVICT`] glyphs, where the cache is full.
    fn evict(&mut self) {
        if self.cache.len() < CACHE_CAP {
            return;
        }
        let mut ages: Vec<u64> = self.cache.values().map(|held| held.used).collect();
        // The age of the youngest glyph that still goes: everything at or
        // under it is given up.
        let at = CACHE_EVICT.min(ages.len() - 1);
        let (_, &mut oldest, _) = ages.select_nth_unstable(at);
        self.cache.retain(|_, held| held.used > oldest);
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
        let quarter = self.aki_px();
        self.pen(script, fb, x, y_baseline, s, inverted, quarter)
    }

    /// [`TextRenderer::draw`] with the script boundaries set solid: `1時間`
    /// stands as one number, against the air a row of figures keeps between
    /// two of them.
    pub fn draw_solid(
        &mut self,
        fb: &mut Framebuffer,
        x: i32,
        y_baseline: i32,
        s: &str,
        inverted: bool,
    ) -> i32 {
        self.pen(font::Script::Unknown, fb, x, y_baseline, s, inverted, 0)
    }

    #[allow(clippy::too_many_arguments)]
    fn pen(
        &mut self,
        script: font::Script,
        fb: &mut Framebuffer,
        x: i32,
        y_baseline: i32,
        s: &str,
        inverted: bool,
        quarter: u32,
    ) -> i32 {
        let fg = if inverted { 0xFF } else { 0x00 };
        let run = font::Script::resolve(script, s);
        let px = self.px;
        let px_key = px.to_bits();
        let mut cur_x = x;
        let quarter = quarter as i32;
        let mut prev: Option<char> = None;
        for ch in s.chars() {
            if font::is_invisible(ch) {
                continue;
            }
            if prev.is_some_and(|a| crate::wrap::aki(a, ch)) {
                cur_x += quarter;
            }
            prev = Some(ch);
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
/// chain has. `STROKE` is two pixels: a hairline outline is what
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

#[cfg(test)]
mod tests {
    use super::*;

    /// A renderer over the device's own faces, where `READINGLOG_FONTS` names
    /// their directories. `None` skips the assertions below: they measure a
    /// device's files.
    fn device_renderer(px: f32) -> Option<TextRenderer> {
        std::env::var("READINGLOG_FONTS").ok()?;
        TextRenderer::load(px).ok()
    }

    /// The cache is bounded, and what it gives up is the oldest. Drawing far
    /// more distinct characters than it holds must not grow it without end,
    /// and the ones still in use must survive.
    #[test]
    fn the_glyph_cache_is_bounded_and_gives_up_the_oldest() {
        let Some(mut text) = device_renderer(26.0) else {
            return;
        };
        let ja = font::Script::Japanese;
        // Far past the cap, so eviction runs many times over.
        let many: String = (0x4E00u32..0x4E00 + (CACHE_CAP as u32 * 2))
            .filter_map(char::from_u32)
            .collect();
        for ch in many.chars() {
            let _ = text.measure_width_in(ja, &ch.to_string());
        }
        assert!(
            text.cache.len() <= CACHE_CAP,
            "bounded, held {}",
            text.cache.len()
        );

        // A character drawn now is held now, whatever came before it.
        let recent = '\u{6771}';
        let _ = text.measure_width_in(ja, &recent.to_string());
        assert!(
            text.cache.keys().any(|(ch, ..)| *ch == recent),
            "the newest glyph is not the one evicted",
        );
    }

    /// The chain is stated in one short line: the faces that resolved and the
    /// one Latin is set in. The paths themselves run to kilobytes.
    #[test]
    fn the_summary_counts_the_chain_and_names_the_primary() {
        let Some(text) = device_renderer(26.0) else {
            return;
        };
        let said = text.chain_summary();
        assert!(said.contains(" faces, primary="), "{said}");
        assert!(!said.contains('/'), "no paths in the summary: {said}");
        assert!(said.len() < 80, "one short line, not kilobytes: {said}");
    }

    #[test]
    fn a_run_crossing_scripts_is_charged_a_quarter_em() {
        let Some(mut text) = device_renderer(40.0) else {
            return;
        };
        let ja = font::Script::Japanese;
        let quarter = text.aki_px();
        assert_eq!(quarter, 10);
        // One boundary in `本R`, two in `本R本`, and none in either half.
        let han = text.measure_width_in(ja, "本");
        let latin = text.measure_width_in(ja, "R");
        assert_eq!(text.measure_width_in(ja, "本R"), han + latin + quarter);
        assert_eq!(
            text.measure_width_in(ja, "本R本"),
            han * 2 + latin + quarter * 2
        );
        assert_eq!(text.measure_width_in(ja, "本本"), han * 2);
        assert_eq!(text.measure_width_in(ja, "RR"), latin * 2);
    }

    #[test]
    fn a_prefix_measures_to_the_pen_and_not_to_the_glyph_before_it() {
        let Some(mut text) = device_renderer(40.0) else {
            return;
        };
        let ja = font::Script::Japanese;
        let (quarter, said) = (text.aki_px(), "本Rust本");
        let han = text.measure_width_in(ja, "本");
        let latin = text.measure_width_in(ja, "Rust");
        // 本 is three bytes, and a run of Latin opens at 3 and closes at 7.
        assert_eq!(text.measure_upto_in(ja, said, 0), 0);
        assert_eq!(text.measure_upto_in(ja, said, 3), han + quarter);
        assert_eq!(
            text.measure_upto_in(ja, said, 7),
            han + quarter + latin + quarter
        );
        // The glyph before the boundary ends a quarter em short of the pen.
        assert_eq!(text.measure_width_in(ja, &said[..7]), han + quarter + latin);
    }

    #[test]
    fn a_drawn_run_advances_by_what_it_measures() {
        let Some(mut text) = device_renderer(40.0) else {
            return;
        };
        let mut fb = Framebuffer::offscreen(200, 80);
        let ja = font::Script::Japanese;
        for said in ["本R本", "第3章", "hello", "世界"] {
            let end = text.draw_in(ja, &mut fb, 0, 60, said, false);
            assert_eq!(end, text.measure_width_in(ja, said) as i32, "{said}");
        }
    }
}
