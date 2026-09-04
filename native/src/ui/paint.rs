//! `Rect`, the fills and rules drawn into one, and the ink scale a value is
//! banded onto.
//!
//! Every size here is an argument. No dimension is a constant.

use crate::eink::fb::Framebuffer;

/// Ink levels, white through black.
pub const WHITE: u8 = 0xFF;
pub const PALE: u8 = 0xE0;
pub const LIGHT: u8 = 0xC0;
pub const DARK: u8 = 0x60;
pub const INK: u8 = 0x00;

/// The five steps a value is drawn at, lightest first, in one hue. Their
/// Rec. 601 lumas sit within a few of [`PALE`], [`LIGHT`], half way between
/// the two remaining levels, [`DARK`] and [`INK`]. Zero is [`WHITE`], off this
/// scale.
pub const STEPS_RGB: [[u8; 3]; 5] = [
    [0xCF, 0xE6, 0xF7],
    [0x9F, 0xCB, 0xE8],
    [0x5B, 0x9D, 0xCB],
    [0x2F, 0x6B, 0x96],
    [0x0A, 0x12, 0x18],
];

/// The ink a bar takes where the panel has colour, and the warm mark on the
/// one the page is about. The Colorsoft is the panel this is drawn for.
pub const BAR_RGB: [u8; 3] = STEPS_RGB[2];
pub const MARK_RGB: [u8; 3] = [0xC4, 0x45, 0x36];

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
    /// the last flush right, and the same air between every pair.
    ///
    /// A single width centres. `min_gap` floors the air when `widths` overrun.
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

#[cfg(test)]
mod tests {
    use super::*;

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
