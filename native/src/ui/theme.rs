//! Every size on screen, derived from the panel at runtime.
//!
//! [`Theme::for_screen`] scales each field off `xres`. No field is a constant.

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
    /// An axis label, a date under a bar, a unit beside a figure.
    pub small_px: f32,
    /// One row of a list.
    pub row_h: i32,
    pub header_h: i32,
    pub tabs_h: i32,
}

impl Theme {
    pub fn for_screen(xres: u32, yres: u32) -> Self {
        let w = xres as i32;
        // 28 px on a 1264 px panel.
        let body = (w as f32 / 45.0).round().max(14.0);
        Self {
            screen: Rect::new(0, 0, w, yres as i32),
            pad: (w / 32).max(12),
            gap: (w / 90).max(6),
            display_px: (body * 2.6).round(),
            head_px: (body * 1.25).round(),
            body_px: body,
            small_px: (body * 0.78).round(),
            row_h: (body * 2.7) as i32,
            header_h: (body * 3.4) as i32,
            tabs_h: (body * 2.8) as i32,
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
            assert!(t.header_h > t.head_px as i32);
            assert!(t.tabs_h > t.body_px as i32);
        }
    }
}
