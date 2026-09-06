//! `Rect`, the fills and rules drawn into one, and the ink scale a value is
//! banded onto. Every size here is an argument; no dimension is a constant.

use crate::eink::fb::Framebuffer;
use crate::settings::ColorScheme;

/// Ink levels, white through black.
pub const WHITE: u8 = 0xFF;
pub const PALE: u8 = 0xE0;
pub const LIGHT: u8 = 0xC0;
pub const DARK: u8 = 0x60;
pub const INK: u8 = 0x00;

/// [`WHITE`] in three channels, for [`fill_rgb`].
pub const WHITE_RGB: [u8; 3] = [WHITE; 3];

/// The Rec. 601 luma of each [`Palette::steps`] entry, lightest first.
pub const STEP_LUMAS: [u8; 5] = [225, 193, 142, 93, 16];

/// The five steps a value is banded onto, lightest first, and the ink on the
/// marked one. Level zero draws [`WHITE`].
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Palette {
    pub steps: [[u8; 3]; 5],
    pub mark: [u8; 3],
}

impl Palette {
    /// The ink an unmarked bar takes.
    pub fn bar(&self) -> [u8; 3] {
        self.steps[2]
    }

    /// The [`Palette::steps`] entry at `level`, and `None` at zero.
    pub fn level(&self, level: usize) -> Option<[u8; 3]> {
        (level > 0)
            .then(|| self.steps.get(level))
            .flatten()
            .copied()
    }

    /// [`STEP_LUMAS`] in three equal channels. `mark` sits 67 under
    /// [`STEP_LUMAS`]`[2]`, where every other `mark` sits about 30 off it.
    pub const GREY: Palette = Palette {
        steps: [
            [STEP_LUMAS[0]; 3],
            [STEP_LUMAS[1]; 3],
            [STEP_LUMAS[2]; 3],
            [STEP_LUMAS[3]; 3],
            [STEP_LUMAS[4]; 3],
        ],
        mark: [75; 3],
    };

    /// One azure hue across the ramp, marked in warm red.
    pub const AZURE: Palette = Palette {
        steps: [
            [0xCF, 0xE6, 0xF7],
            [0x9F, 0xCB, 0xE8],
            [0x5B, 0x9D, 0xCB],
            [0x2F, 0x6B, 0x96],
            [0x0A, 0x12, 0x18],
        ],
        mark: [0xC4, 0x45, 0x36],
    };

    /// 浅葱 `#6B9BB0` at `steps[2]`, marked in 朱 `#D8453A`.
    pub const ASAGI_SHU: Palette = Palette {
        steps: [
            [0xD6, 0xE5, 0xEB],
            [0xAD, 0xC8, 0xD5],
            [0x6B, 0x9B, 0xB0],
            [0x3F, 0x67, 0x79],
            [0x0B, 0x12, 0x14],
        ],
        mark: [0xD8, 0x45, 0x3A],
    };

    /// 鳶 `#8A5A3B` at `steps[3]`, marked in 黄金 `#D4AF37` — a `mark` lighter
    /// than [`Palette::bar`].
    pub const TOBI_KOGANE: Palette = Palette {
        steps: [
            [0xEE, 0xDE, 0xD3],
            [0xDA, 0xBA, 0xA5],
            [0xBA, 0x81, 0x5D],
            [0x8A, 0x5A, 0x3B],
            [0x16, 0x0F, 0x0A],
        ],
        mark: [0xD4, 0xAF, 0x37],
    };

    /// 若竹 and 松葉 across the ramp, marked at 桜's hue and luma 112.
    pub const SAKURA_WAKATAKE: Palette = Palette {
        steps: [
            [0xDB, 0xE7, 0xD3],
            [0xB4, 0xCD, 0xA5],
            [0x79, 0xA2, 0x60],
            [0x51, 0x69, 0x40],
            [0x0E, 0x12, 0x0B],
        ],
        mark: [0xD2, 0x3F, 0x6B],
    };

    /// 紺's hue across the ramp, marked at 紅's hue and luma 108.
    pub const KURENAI_KON: Palette = Palette {
        steps: [
            [0xDA, 0xE2, 0xF3],
            [0xB3, 0xC2, 0xE6],
            [0x74, 0x8F, 0xCF],
            [0x3D, 0x5E, 0xAF],
            [0x0A, 0x10, 0x1F],
        ],
        mark: [0xCC, 0x43, 0x43],
    };

    /// The colours `scheme` names.
    pub fn of(scheme: ColorScheme) -> Self {
        match scheme {
            ColorScheme::Azure => Self::AZURE,
            ColorScheme::AsagiShu => Self::ASAGI_SHU,
            ColorScheme::TobiKogane => Self::TOBI_KOGANE,
            ColorScheme::SakuraWakatake => Self::SAKURA_WAKATAKE,
            ColorScheme::KurenaiKon => Self::KURENAI_KON,
            ColorScheme::Grey => Self::GREY,
        }
    }

    /// [`Palette::of`] under `colour`, and [`Palette::GREY`] without it.
    pub fn for_panel(scheme: ColorScheme, colour: bool) -> Self {
        match colour {
            true => Self::of(scheme),
            false => Self::GREY,
        }
    }
}

/// A box on the screen.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Rect {
    pub x: i32,
    pub y: i32,
    pub w: i32,
    pub h: i32,
}

impl Rect {
    pub fn new(x: i32, y: i32, w: i32, h: i32) -> Self {
        Self { x, y, w, h }
    }

    pub fn contains(&self, px: i32, py: i32) -> bool {
        px >= self.x && px < self.x + self.w && py >= self.y && py < self.y + self.h
    }

    pub fn right(&self) -> i32 {
        self.x + self.w
    }

    pub fn bottom(&self) -> i32 {
        self.y + self.h
    }

    pub fn center_y(&self) -> i32 {
        self.y + self.h / 2
    }

    /// The same box inset on every side.
    pub fn inset(&self, by: i32) -> Self {
        Self::new(
            self.x + by,
            self.y + by,
            (self.w - by * 2).max(0),
            (self.h - by * 2).max(0),
        )
    }

    /// The top `h` of this box, and what is left below it.
    pub fn split_top(&self, h: i32) -> (Self, Self) {
        let h = h.clamp(0, self.h);
        (
            Self::new(self.x, self.y, self.w, h),
            Self::new(self.x, self.y + h, self.w, self.h - h),
        )
    }

    /// The bottom `h` of this box, and what is left above it.
    pub fn split_bottom(&self, h: i32) -> (Self, Self) {
        let h = h.clamp(0, self.h);
        (
            Self::new(self.x, self.bottom() - h, self.w, h),
            Self::new(self.x, self.y, self.w, self.h - h),
        )
    }

    /// The left `w` of this box, and what is left beside it.
    pub fn split_left(&self, w: i32) -> (Self, Self) {
        let w = w.clamp(0, self.w);
        (
            Self::new(self.x, self.y, w, self.h),
            Self::new(self.x + w, self.y, self.w - w, self.h),
        )
    }

    /// This box cut into `n` columns with `gap` between them.
    pub fn columns(&self, n: i32, gap: i32) -> Vec<Self> {
        if n <= 0 {
            return Vec::new();
        }
        let each = (self.w - gap * (n - 1)) / n;
        (0..n)
            .map(|i| Self::new(self.x + i * (each + gap), self.y, each, self.h))
            .collect()
    }

    /// Boxes of the given widths laid across this one: the first flush left,
    /// the last flush right, the same air between every pair. A single width
    /// centres, and `min_gap` floors the air when `widths` overrun.
    pub fn spread(&self, widths: &[i32], min_gap: i32) -> Vec<Self> {
        let Some(last) = widths.len().checked_sub(1) else {
            return Vec::new();
        };
        if last == 0 {
            let w = widths[0].min(self.w);
            return vec![Self::new(self.x + (self.w - w) / 2, self.y, w, self.h)];
        }
        let taken: i32 = widths.iter().sum();
        let gap = ((self.w - taken) / last as i32).max(min_gap);
        let mut x = self.x;
        widths
            .iter()
            .map(|w| {
                let cell = Self::new(x, self.y, *w, self.h);
                x += w + gap;
                cell
            })
            .collect()
    }

    /// This box cut into `n` rows with `gap` between them.
    pub fn rows(&self, n: i32, gap: i32) -> Vec<Self> {
        if n <= 0 {
            return Vec::new();
        }
        let each = (self.h - gap * (n - 1)) / n;
        (0..n)
            .map(|i| Self::new(self.x, self.y + i * (each + gap), self.w, each))
            .collect()
    }
}

pub fn fill(fb: &mut Framebuffer, r: Rect, value: u8) {
    if r.w <= 0 || r.h <= 0 {
        return;
    }
    fb.fill_rect(
        r.y.max(0) as u32,
        r.x.max(0) as u32,
        r.w as u32,
        r.h as u32,
        value,
    );
}

/// [`fill`] in three channels.
pub fn fill_rgb(fb: &mut Framebuffer, r: Rect, rgb: [u8; 3]) {
    if r.w <= 0 || r.h <= 0 {
        return;
    }
    fb.fill_rect_rgb(
        r.y.max(0) as u32,
        r.x.max(0) as u32,
        r.w as u32,
        r.h as u32,
        rgb,
    );
}

/// An outline `width` thick, drawn inside the box.
pub fn stroke(fb: &mut Framebuffer, r: Rect, value: u8, width: i32) {
    if r.w <= 0 || r.h <= 0 || width <= 0 {
        return;
    }
    fill(fb, Rect::new(r.x, r.y, r.w, width), value);
    fill(fb, Rect::new(r.x, r.bottom() - width, r.w, width), value);
    fill(fb, Rect::new(r.x, r.y, width, r.h), value);
    fill(fb, Rect::new(r.right() - width, r.y, width, r.h), value);
}

pub fn hline(fb: &mut Framebuffer, x: i32, y: i32, w: i32, value: u8, thickness: i32) {
    fill(fb, Rect::new(x, y, w, thickness.max(1)), value);
}

pub fn vline(fb: &mut Framebuffer, x: i32, y: i32, h: i32, value: u8, thickness: i32) {
    fill(fb, Rect::new(x, y, thickness.max(1), h), value);
}

/// A filled bar inside `track`, `value` of `max` across.
///
/// The empty part of `track` keeps a hairline outline.
pub fn progress(fb: &mut Framebuffer, track: Rect, value: i64, max: i64, ink: u8) {
    stroke(fb, track, LIGHT, 1);
    if max <= 0 || value <= 0 {
        return;
    }
    let filled = (track.w as i64 * value.min(max) / max) as i32;
    fill(fb, Rect::new(track.x, track.y, filled, track.h), ink);
}

/// How wide [`notch`] cuts, from `gap`.
pub fn notch_width(gap: i32) -> i32 {
    (gap / 2).max(3)
}

/// Where a notch `w` wide stands in `track`, `at` per cent along it.
pub fn notch_x(track: Rect, at: i64, w: i32) -> i32 {
    let x = track.x + (track.w as i64 * at.clamp(0, 100) / 100) as i32;
    x.clamp(track.x, track.right() - w)
}

/// Cut [`WHITE`] down `track` at `at` per cent, [`notch_width`] wide.
pub fn notch(fb: &mut Framebuffer, track: Rect, at: i64, gap: i32) {
    let w = notch_width(gap);
    fill(
        fb,
        Rect::new(notch_x(track, at, w), track.y, w, track.h),
        WHITE,
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Rec. 601 luma, with the weights `eink::fb` collapses a pixel by.
    fn luma(rgb: [u8; 3]) -> i32 {
        (rgb[0] as i32 * 77 + rgb[1] as i32 * 150 + rgb[2] as i32 * 29) >> 8
    }

    /// How far a luma may sit from [`STEP_LUMAS`].
    const SLACK: i32 = 8;

    /// Every [`Palette`] const in this module.
    const SCHEMES: [(&str, Palette); 6] = [
        ("grey", Palette::GREY),
        ("azure", Palette::AZURE),
        ("asagi-shu", Palette::ASAGI_SHU),
        ("tobi-kogane", Palette::TOBI_KOGANE),
        ("sakura-wakatake", Palette::SAKURA_WAKATAKE),
        ("kurenai-kon", Palette::KURENAI_KON),
    ];

    #[test]
    fn every_scheme_is_offered_and_every_scheme_is_checked() {
        assert_eq!(SCHEMES.len(), ColorScheme::ALL.len());
        for scheme in ColorScheme::ALL {
            let want = Palette::of(scheme);
            assert!(
                SCHEMES.iter().any(|(_, pal)| *pal == want),
                "{scheme:?} is drawn and no SCHEMES row holds it"
            );
        }
    }

    #[test]
    fn every_scheme_bands_onto_the_lumas_a_grey_panel_draws() {
        for (name, pal) in SCHEMES {
            for (at, step) in pal.steps.iter().enumerate() {
                let (got, want) = (luma(*step), STEP_LUMAS[at] as i32);
                assert!(
                    (got - want).abs() <= SLACK,
                    "{name} step {at} is luma {got}, off the ladder's {want}"
                );
            }
        }
    }

    #[test]
    fn the_ramp_shares_four_rungs_with_the_ink_scale() {
        for (name, pal) in SCHEMES {
            for (at, rung) in [(0, PALE), (1, LIGHT), (3, DARK)] {
                let apart = (luma(pal.steps[at]) - rung as i32).abs();
                assert!(apart <= 4, "{name} step {at} sits {apart} off its rung");
            }
            // `steps[4]` sits near [`INK`], off it by up to 20.
            assert!(luma(pal.steps[4]) <= 20, "{name} step 4 is not near INK");
        }
    }

    #[test]
    fn a_ramp_never_steps_backwards() {
        for (name, pal) in SCHEMES {
            for (at, pair) in pal.steps.windows(2).enumerate() {
                assert!(
                    luma(pair[0]) > luma(pair[1]),
                    "{name} is lighter at step {} than at {at}",
                    at + 1
                );
            }
        }
    }

    #[test]
    fn a_mark_is_never_mistaken_for_the_bar_beside_it() {
        for (name, pal) in SCHEMES {
            let apart = (luma(pal.mark) - luma(pal.bar())).abs();
            assert!(apart >= 25, "{name}: mark and bar only {apart} lumas apart");
        }
    }

    #[test]
    fn the_grey_panel_s_mark_carries_the_whole_difference() {
        let grey = Palette::GREY;
        let below = luma(grey.bar()) - luma(grey.mark);
        assert!(below >= 60, "the grey mark is only {below} below the bar");
        assert!(
            luma(grey.mark) >= 40,
            "a mark this dark reads as the black body text is set in"
        );
    }

    #[test]
    fn a_grey_panel_is_given_grey_whatever_the_reader_picked() {
        for scheme in ColorScheme::ALL {
            assert_eq!(Palette::for_panel(scheme, false), Palette::GREY);
            assert_eq!(Palette::for_panel(scheme, true), Palette::of(scheme));
        }
        // Every channel of `Palette::GREY` is equal.
        for step in Palette::GREY.steps {
            assert!(step[0] == step[1] && step[1] == step[2], "{step:?}");
        }
    }

    #[test]
    fn a_box_knows_what_is_inside_it() {
        let r = Rect::new(10, 20, 100, 50);
        assert!(r.contains(10, 20));
        assert!(r.contains(109, 69));
        assert!(!r.contains(110, 69));
        assert!(!r.contains(9, 20));
        assert_eq!(r.right(), 110);
        assert_eq!(r.bottom(), 70);
    }

    #[test]
    fn a_split_gives_back_the_whole_box() {
        let r = Rect::new(0, 0, 100, 200);
        let (top, rest) = r.split_top(60);
        assert_eq!(top, Rect::new(0, 0, 100, 60));
        assert_eq!(rest, Rect::new(0, 60, 100, 140));
        let (bottom, above) = r.split_bottom(60);
        assert_eq!(bottom, Rect::new(0, 140, 100, 60));
        assert_eq!(above, Rect::new(0, 0, 100, 140));
        let (left, right) = r.split_left(30);
        assert_eq!(left, Rect::new(0, 0, 30, 200));
        assert_eq!(right, Rect::new(30, 0, 70, 200));
    }

    #[test]
    fn a_split_taller_than_the_box_takes_the_box() {
        let r = Rect::new(0, 0, 100, 50);
        let (top, rest) = r.split_top(999);
        assert_eq!(top, r);
        assert_eq!(rest.h, 0);
    }

    #[test]
    fn columns_and_rows_leave_the_gaps_between_them() {
        let cells = Rect::new(0, 0, 100, 20).columns(4, 4);
        assert_eq!(cells.len(), 4);
        assert_eq!(cells[0], Rect::new(0, 0, 22, 20));
        assert_eq!(cells[3].x, 3 * 26);
        assert!(cells[3].right() <= 100);
        let rows = Rect::new(0, 0, 20, 100).rows(4, 4);
        assert_eq!(rows[0], Rect::new(0, 0, 20, 22));
    }

    #[test]
    fn a_spread_row_reaches_both_margins_with_even_air_between() {
        let cells = Rect::new(10, 0, 300, 40).spread(&[100, 60, 40], 8);
        assert_eq!(cells.len(), 3);
        assert_eq!(cells[0].x, 10, "flush left");
        assert_eq!(cells[2].right(), 310, "flush right");
        let gaps = [cells[1].x - cells[0].right(), cells[2].x - cells[1].right()];
        assert_eq!(gaps[0], gaps[1], "the air between is even");
    }

    #[test]
    fn a_spread_wider_than_its_box_keeps_the_smallest_air_it_was_given() {
        let cells = Rect::new(0, 0, 100, 10).spread(&[80, 80], 6);
        assert_eq!(cells[0].x, 0);
        assert_eq!(cells[1].x, 86, "never overlapping, even when it overruns");
        assert!(Rect::new(0, 0, 100, 10).spread(&[], 6).is_empty());
        // One width centres.
        assert_eq!(Rect::new(0, 0, 100, 10).spread(&[40], 6)[0].x, 30);
    }
}
