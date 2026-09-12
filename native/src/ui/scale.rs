//! Panel sizing. Every length on a screen — type, rules, insets — is a *design
//! pixel* written for the [`REFERENCE`] panel, multiplied by this panel's own
//! width. [`Scale::dpi`] states the density for the startup line, sizing none.

/// The panel every design pixel is written for: the Colorsoft, Paperwhite 3-4
/// and Oasis 2-3 framebuffer.
pub const REFERENCE: u32 = 1264;

/// Panel density by framebuffer width, for the widths that are not 300 ppi.
/// The nominal 768 px Paperwhite is not one, since the X server reports its
/// 758 px framebuffer.
const PANELS: &[(u32, i32)] = &[(600, 167), (758, 212)];

/// The density most shipped panels have.
const COMMON_DPI: i32 = 300;

/// What one design pixel is worth on the panel being drawn.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Scale {
    xres: u32,
}

impl Scale {
    /// The scale a panel `xres` pixels across draws at.
    pub fn of_width(xres: u32) -> Self {
        Self { xres: xres.max(1) }
    }

    /// The density [`PANELS`] gives this panel, for the startup line.
    pub fn dpi(self) -> i32 {
        PANELS
            .iter()
            .find(|(width, _)| *width == self.xres)
            .map(|(_, dpi)| *dpi)
            .unwrap_or(COMMON_DPI)
    }

    /// `design` in device pixels, keeping its sign. A non-zero constant never
    /// scales to nothing: a rule that rounds away leaves the shape it divides
    /// unreadable.
    pub fn px(self, design: i32) -> i32 {
        match design {
            0 => 0,
            _ => {
                let scaled = (design.abs() * self.xres as i32 / REFERENCE as i32).max(1);
                match design < 0 {
                    true => -scaled,
                    false => scaled,
                }
            }
        }
    }

    /// [`Scale::px`] for a type size, which keeps its fraction.
    pub fn font(self, design: f32) -> f32 {
        design * self.xres as f32 / REFERENCE as f32
    }
}

#[cfg(test)]
mod tests {
    use super::{COMMON_DPI, REFERENCE, Scale};

    /// The two panels that are not [`COMMON_DPI`], and the ones that are. The
    /// density reaches the startup line and no drawing.
    #[test]
    fn the_shipped_widths_carry_their_own_density() {
        assert_eq!(Scale::of_width(600).dpi(), 167);
        assert_eq!(Scale::of_width(758).dpi(), 212);
        for wide in [1072, 1236, 1264, 1272, 1860] {
            assert_eq!(Scale::of_width(wide).dpi(), COMMON_DPI, "{wide}");
        }
        // An unreported width draws at the density most devices have.
        assert_eq!(Scale::of_width(999).dpi(), COMMON_DPI);
    }

    /// A design pixel is itself on [`REFERENCE`], smaller under it and larger
    /// over it.
    #[test]
    fn a_design_pixel_follows_the_panel_width() {
        let oasis = Scale::of_width(REFERENCE);
        assert_eq!(oasis.px(360), 360);
        assert_eq!(oasis.font(38.0), 38.0);

        let basic = Scale::of_width(600);
        assert_eq!(basic.px(360), 360 * 600 / REFERENCE as i32);

        let scribe = Scale::of_width(1860);
        assert!(scribe.font(38.0) > 38.0, "{}", scribe.font(38.0));
    }

    /// A constant rounding to nothing keeps one pixel, and a zero stays zero.
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

    /// Every length holds the same share of the panel it is drawn on.
    #[test]
    fn a_length_is_one_share_of_every_panel() {
        let want = 38.0 / REFERENCE as f32;
        for width in [600u32, 758, 1072, 1236, 1264, 1860] {
            let share = Scale::of_width(width).font(38.0) / width as f32;
            assert!((share - want).abs() < 0.0001, "{width}: {share}");
        }
    }
}
