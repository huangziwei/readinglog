//! Every size on screen, from the panel and a [`TextSize`]. Type is a physical
//! size — see `BODY_PX` — as is everything derived from it, and the panel's
//! density decides what that comes to in pixels. [`Theme::pad`] and
//! [`Theme::gap`] alone read the panel's width.

use crate::settings::TextSize;

use super::paint::Rect;
use super::scale::Scale;

pub struct Theme {
    pub screen: Rect,
    /// What one design pixel is worth on this panel, for the rules and
    /// outlines the drawing holds — [`Theme::px`].
    scale: Scale,
    /// The margin the content stands in.
    pub pad: i32,
    /// The gap between two things that belong together.
    pub gap: i32,
    /// A headline figure, set clear of [`Theme::head_px`].
    pub display_px: f32,
    /// A section heading.
    pub head_px: f32,
    pub body_px: f32,
    /// The strip along the bottom, which never scales.
    pub tab_px: f32,
    /// An axis label, a date under a bar, a unit beside a figure.
    pub small_px: f32,
    /// One row of a list.
    pub row_h: i32,
    pub tabs_h: i32,
}

/// A row of the UI, in design pixels of em: a size on the page, the same on
/// every panel, which [`Scale`] puts into that panel's own pixels.
const BODY_PX: f32 = 38.0;

/// A section heading.
const HEAD_PX: f32 = 50.0;

/// An ordinary rule or outline, in design pixels — [`Theme::rule`].
const RULE: i32 = 2;

impl Theme {
    pub fn for_screen(xres: u32, yres: u32) -> Self {
        Self::sized(xres, yres, TextSize::default())
    }

    /// [`Theme::for_screen`] at `size`.
    pub fn sized(xres: u32, yres: u32, size: TextSize) -> Self {
        let w = xres as i32;
        let scale = Scale::of_width(xres);
        let body = scale.font(BODY_PX * size.scale()).round();
        let tab = scale.font(BODY_PX);
        Self {
            screen: Rect::new(0, 0, w, yres as i32),
            scale,
            pad: (w / 32).max(12),
            gap: (w / 90).max(6),
            display_px: (body * 2.6).round(),
            head_px: scale.font(HEAD_PX * size.scale()).round(),
            body_px: body,
            small_px: (body * 0.78).round(),
            row_h: (body * 2.7) as i32,
            tab_px: tab,
            tabs_h: (tab * 2.8) as i32,
        }
    }

    /// A rule, an outline or any other physical length written at
    /// `scale::DESIGN_DPI`, in this panel's pixels. Never rounds a rule away.
    pub fn px(&self, design: i32) -> i32 {
        self.scale.px(design)
    }

    /// The thickness of an ordinary rule or outline: the line under the tab
    /// strip, the edge of a chip, the mark under a figure that opens. Two
    /// design pixels, and never less than one of the panel's own.
    pub fn rule(&self) -> i32 {
        self.px(RULE)
    }

    /// The density the panel is drawn at, which the log states at startup.
    pub fn dpi(&self) -> i32 {
        self.scale.dpi()
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    /// Every shipped framebuffer, and one panel larger than all of them. The
    /// layout tests across `ui` and `view` all draw against this list: a screen
    /// that holds on the Oasis and folds on the Paperwhite 2 is not laid out.
    pub(crate) const PANELS: [(u32, u32); 7] = [
        (600, 800),
        (758, 1024),
        (1072, 1448),
        (1236, 1648),
        (1264, 1680),
        (1860, 2480),
        (2400, 3200),
    ];

    /// The panels that are 300 ppi, which the density leaves alone.
    const FULL: [(u32, u32); 5] = [
        (1072, 1448),
        (1236, 1648),
        (1264, 1680),
        (1860, 2480),
        (2400, 3200),
    ];

    #[test]
    fn every_panel_gets_a_readable_body_size() {
        for (w, h) in PANELS {
            let t = Theme::for_screen(w, h);
            assert!(t.body_px >= 20.0, "{w}x{h} body {}", t.body_px);
            assert!(t.small_px < t.body_px);
            assert!(t.head_px > t.body_px);
            assert!(t.display_px > t.head_px);
        }
    }

    #[test]
    fn type_is_the_same_size_on_every_panel_of_one_density() {
        // These panels are all 300 ppi, and a size in pixels is a size on
        // the page.
        let reference = Theme::for_screen(1264, 1680);
        for (w, h) in FULL {
            let t = Theme::for_screen(w, h);
            for (name, got, want) in [
                ("display", t.display_px, reference.display_px),
                ("head", t.head_px, reference.head_px),
                ("body", t.body_px, reference.body_px),
                ("small", t.small_px, reference.small_px),
            ] {
                assert_eq!(got, want, "{name} differs at {w}x{h}");
            }
            // The rows type sits in are fixed with it; the taller panel fits
            // more of them.
            assert_eq!(t.row_h, reference.row_h, "row_h differs at {w}x{h}");
            assert_eq!(t.tabs_h, reference.tabs_h, "tabs_h differs at {w}x{h}");
        }
        assert!(
            Theme::for_screen(1860, 2480).screen.h / reference.row_h
                > reference.screen.h / reference.row_h,
            "a taller panel must fit more rows"
        );
    }

    /// The whole point of the density: a line of body text covers the same
    /// stretch of paper on a 167 ppi panel as on a 300 ppi one.
    #[test]
    fn a_body_line_is_one_size_on_the_page_at_every_density() {
        let reference = Theme::for_screen(1264, 1680);
        let want = reference.body_px / reference.dpi() as f32;
        for (w, h) in PANELS {
            let t = Theme::for_screen(w, h);
            let inches = t.body_px / t.dpi() as f32;
            assert!(
                (inches - want).abs() < 0.005,
                "{w}x{h}: a body line is {inches}″ against {want}″"
            );
        }
        // And the pixels it comes to fall with the density.
        assert_eq!(Theme::for_screen(758, 1024).body_px, 27.0);
        assert_eq!(Theme::for_screen(600, 800).body_px, 21.0);
    }

    /// A rule is physical too, and never rounds away.
    #[test]
    fn a_rule_holds_its_thickness_down_to_one_pixel() {
        assert_eq!(Theme::for_screen(1264, 1680).px(2), 2);
        assert_eq!(Theme::for_screen(758, 1024).px(2), 1);
        assert_eq!(Theme::for_screen(600, 800).px(3), 1);
        for (w, h) in PANELS {
            assert_eq!(Theme::for_screen(w, h).px(1), 1, "{w}x{h} lost a hairline");
        }
    }

    #[test]
    fn the_margins_leave_the_page_most_of_the_panel() {
        for (w, h) in PANELS {
            let t = Theme::for_screen(w, h);
            assert!(t.pad > 0);
            assert!(t.screen.w - t.pad * 2 > t.screen.w / 2);
            assert!(t.gap < t.pad);
        }
    }

    #[test]
    fn a_row_is_taller_than_the_text_in_it() {
        for (w, h) in PANELS {
            let t = Theme::for_screen(w, h);
            assert!(t.row_h > t.body_px as i32);
            assert!(t.tabs_h > t.body_px as i32);
        }
    }

    /// The two 6-inch families are the same page at two densities: 758 px at
    /// 212 ppi and 1072 px at 300 both measure about 3.6 by 4.8 inches. Every
    /// size the theme holds must land within a hair of the same physical
    /// measure on the two, or the density is not being read.
    #[test]
    fn one_page_at_two_densities_lays_out_the_same() {
        let pw2 = Theme::for_screen(758, 1024);
        let voyage = Theme::for_screen(1072, 1448);
        let inches = |px: f32, t: &Theme| px / t.dpi() as f32;
        for (name, a, b) in [
            ("body", pw2.body_px, voyage.body_px),
            ("head", pw2.head_px, voyage.head_px),
            ("display", pw2.display_px, voyage.display_px),
            ("small", pw2.small_px, voyage.small_px),
            ("row", pw2.row_h as f32, voyage.row_h as f32),
            ("tabs", pw2.tabs_h as f32, voyage.tabs_h as f32),
            ("pad", pw2.pad as f32, voyage.pad as f32),
            ("gap", pw2.gap as f32, voyage.gap as f32),
        ] {
            let (a, b) = (inches(a, &pw2), inches(b, &voyage));
            assert!(
                (a - b).abs() < 0.01,
                "{name}: {a}\u{2033} on the Paperwhite 2 against {b}\u{2033} on the Voyage"
            );
        }
    }

    /// The tab strip is one physical height, whatever the reader sets the
    /// content to: a thumb is a thumb.
    #[test]
    fn the_tab_strip_stands_clear_of_the_text_size() {
        for (w, h) in PANELS {
            let base = Theme::for_screen(w, h);
            for size in TextSize::ALL {
                let t = Theme::sized(w, h, size);
                assert_eq!(t.tabs_h, base.tabs_h, "{w}x{h} at {size:?}");
                assert_eq!(t.tab_px, base.tab_px, "{w}x{h} at {size:?}");
            }
        }
    }
}
