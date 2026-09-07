//! Physical sizing.
//!
//! Every type size in `ui::theme` is a *design pixel*, written for a
//! [`DESIGN_DPI`] panel; [`Scale::font`] turns one into the device pixels that
//! set it at the same size on the page anywhere else. A reading size is a
//! physical size, so a constant that reaches the panel unscaled is 42 % too
//! large at 212 ppi and 80 % too large at 167.
//!
//! The density is read off the panel width, because that is what the X server
//! reports and nothing on the device names the model. Every width the shipped
//! firmware reports is in [`PANELS`], and an unknown one takes [`DESIGN_DPI`] —
//! the density of every Kindle from the Voyage on.

/// The density every design pixel is written at: 300 ppi, the Voyage and
/// everything after it.
pub const DESIGN_DPI: i32 = 300;

/// Panel density by framebuffer width, for the widths that are not
/// [`DESIGN_DPI`].
///
/// | width | device | ppi |
/// |:--|:--|--:|
/// | 600 | 6″ Kindle Touch, Basic 1–2 | 167 |
/// | 758 | 6″ Paperwhite 1–2 | 212 |
///
/// The rest are 300: 1072 (Voyage, Paperwhite 3–4, Basic 4), 1236 (Paperwhite
/// 5–6, Colorsoft), 1264 (Oasis 2–3), 1860 (Scribe). The nominal 768 px
/// Paperwhite panel is not among them: the X server reports the 758 px
/// framebuffer, which is what a window is laid out in.
const PANELS: &[(u32, i32)] = &[(600, 167), (758, 212)];

/// What one design pixel is worth on the panel being drawn.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Scale {
    dpi: i32,
}

impl Scale {
    /// The density [`PANELS`] gives `xres`, else [`DESIGN_DPI`].
    pub fn of_width(xres: u32) -> Self {
        let dpi = PANELS
            .iter()
            .find(|(width, _)| *width == xres)
            .map(|(_, dpi)| *dpi)
            .unwrap_or(DESIGN_DPI);
        Self { dpi }
    }

    pub fn dpi(self) -> i32 {
        self.dpi
    }

    /// `design` in device pixels, keeping its sign. A non-zero constant never
    /// scales to nothing: a rule that rounds away leaves the shape it divides
    /// unreadable.
    pub fn px(self, design: i32) -> i32 {
        match design {
            0 => 0,
            _ => {
                let scaled = (design.abs() * self.dpi / DESIGN_DPI).max(1);
                match design < 0 {
                    true => -scaled,
                    false => scaled,
                }
            }
        }
    }

    /// [`Scale::px`] for a type size, which keeps its fraction.
    pub fn font(self, design: f32) -> f32 {
        design * self.dpi as f32 / DESIGN_DPI as f32
    }
}

#[cfg(test)]
mod tests {
    use super::{DESIGN_DPI, Scale};

    /// The two panels that are not [`DESIGN_DPI`], and the ones that are.
    #[test]
    fn the_shipped_widths_carry_their_own_density() {
        assert_eq!(Scale::of_width(600).dpi(), 167);
        assert_eq!(Scale::of_width(758).dpi(), 212);
        for wide in [1072, 1236, 1264, 1272, 1860] {
            assert_eq!(Scale::of_width(wide).dpi(), DESIGN_DPI, "{wide}");
        }
        // An unreported width draws at the density most devices have.
        assert_eq!(Scale::of_width(999).dpi(), DESIGN_DPI);
    }

    /// A design pixel is itself at [`DESIGN_DPI`] and smaller below it.
    #[test]
    fn a_design_pixel_shrinks_with_the_panel() {
        let oasis = Scale::of_width(1264);
        assert_eq!(oasis.px(360), 360);
        assert_eq!(oasis.font(38.0), 38.0);

        let pw2 = Scale::of_width(758);
        assert_eq!(pw2.px(360), 360 * 212 / 300);
        assert_eq!(pw2.px(80), 56);

        let basic = Scale::of_width(600);
        assert_eq!(basic.px(360), 360 * 167 / 300);
    }

    /// A constant that would round to nothing keeps one pixel, and a zero
    /// stays zero.
    #[test]
    fn a_rule_survives_the_smallest_panel() {
        let basic = Scale::of_width(600);
        assert_eq!(basic.px(1), 1);
        assert_eq!(basic.px(2), 1);
        assert_eq!(basic.px(0), 0);
    }

    /// A design pixel keeps its sign, for an offset drawn back up the page.
    #[test]
    fn a_signed_constant_scales_both_ways() {
        let pw2 = Scale::of_width(758);
        assert_eq!(pw2.px(-44), -pw2.px(44));
        assert_eq!(pw2.px(-1), -1);
    }

    /// Type set at a density is the same size on the page at every other.
    #[test]
    fn a_body_line_is_one_size_on_every_panel() {
        // Within a fiftieth of an inch of 38 px at 300 ppi, on every panel.
        let want = 38.0 / DESIGN_DPI as f32;
        for width in [600, 758, 1072, 1236, 1264, 1860] {
            let scale = Scale::of_width(width);
            let inches = scale.font(38.0) / scale.dpi() as f32;
            assert!((inches - want).abs() < 0.02, "{width}: {inches}″");
        }
    }
}
