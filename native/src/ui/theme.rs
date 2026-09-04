//! Every size on screen, from the panel and the text size the reader set.
//!
//! Type is a fixed pixel size — see [`BODY_PX`] — and every size derived from
//! it is too. [`Theme::pad`] and [`Theme::gap`] are the only fields the
//! panel's own width reaches.

use crate::settings::TextSize;

use super::paint::Rect;

pub struct Theme {
    pub screen: Rect,
    /// The margin the content stands in.
    pub pad: i32,
    /// The gap between two things that belong together.
    pub gap: i32,
    /// A headline figure, set clear of [`Theme::head_px`].
    pub display_px: f32,
    /// A section heading.
    pub head_px: f32,
    pub body_px: f32,
    /// The strip along the bottom, which never scales: its five cells hold
    /// Rhythmus at [`BODY_PX`] and nothing wider.
    pub tab_px: f32,
    /// An axis label, a date under a bar, a unit beside a figure.
    pub small_px: f32,
    /// One row of a list.
    pub row_h: i32,
    pub tabs_h: i32,
}

/// A row of the UI, in pixels of em.
///
/// Fixed, not a share of the panel. Every Kindle this runs on is a ~300 ppi
/// display — 300 on the Colorsoft and the Paperwhite, 304 on the Scribe — so a
/// pixel is the same fraction of a millimetre on all of them and a size in
/// pixels is a size on the page. Scaling type with `xres` instead would set
/// the Scribe's text half again as large as the Paperwhite's for no reason:
/// the larger panel's job is to show *more*, not bigger.
const BODY_PX: f32 = 38.0;

/// A section heading.
const HEAD_PX: f32 = 50.0;

impl Theme {
    pub fn for_screen(xres: u32, yres: u32) -> Self {
        Self::sized(xres, yres, TextSize::default())
    }

    /// [`Theme::for_screen`] at the size the reader set.
    pub fn sized(xres: u32, yres: u32, size: TextSize) -> Self {
        let w = xres as i32;
        let body = (BODY_PX * size.scale()).round();
        Self {
            screen: Rect::new(0, 0, w, yres as i32),
            pad: (w / 32).max(12),
            gap: (w / 90).max(6),
            display_px: (body * 2.6).round(),
            head_px: (HEAD_PX * size.scale()).round(),
            body_px: body,
            small_px: (body * 0.78).round(),
            row_h: (body * 2.7) as i32,
            tab_px: BODY_PX,
            tabs_h: (BODY_PX * 2.8) as i32,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Three shipped panel sizes, and one larger than all of them.
    const PANELS: [(u32, u32); 4] = [(1264, 1680), (1272, 1696), (1860, 2480), (2400, 3200)];

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
    fn type_is_the_same_size_on_every_panel() {
        // These panels are all ~300 ppi, so a size in pixels is a size on the
        // page. A 10.2" Scribe shows more rows than a 7" Paperwhite; it does
        // not show larger ones.
        let reference = Theme::for_screen(1264, 1680);
        for (w, h) in PANELS {
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
}
