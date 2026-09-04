//! `draw` fills the content box with `cx.today`: its figures, its timeline,
//! and the books read on it.

use crate::date;
use crate::ui::paint::Rect;
use crate::ui::{charts, chrome, theme::Theme};

use super::{Ctx, daybooks};

/// Rows of `theme.row_h` that `bands` gives the timeline, axis labels
/// included.
const STRIP_ROWS: i32 = 2;

/// Half-rows of air `bands` sets over the figures and under them.
const FIGURE_AIR: (i32, i32) = (1, 2);

/// The three bands `draw` fills, top to bottom. `top` takes `figures` and
/// `strip` takes `head` plus `STRIP_ROWS`, each with its air; `list` takes
/// what is left.
pub(super) fn bands(area: Rect, theme: &Theme, figures: i32, head: i32) -> [Rect; 3] {
    let air = theme.gap * 2;
    let (over, under) = (
        theme.row_h * FIGURE_AIR.0 / 2,
        theme.row_h * FIGURE_AIR.1 / 2,
    );
    let (top, rest) = area.split_top(over + figures + under);
    let (strip, list) = rest.split_top(head + theme.row_h * STRIP_ROWS + air);
    [
        Rect::new(top.x, top.y + over, top.w, figures.min(top.h)),
        Rect::new(strip.x, strip.y, strip.w, (strip.h - air).max(0)),
        list,
    ]
}

/// `from` is where the day's book list opens, held inside it.
pub fn draw(cx: &mut Ctx, area: Rect, from: usize) {
    let theme: &Theme = cx.theme;
    let today = cx.today;
    let s = cx.s();

    let figures = chrome::figure_height(cx.text, theme);
    let head = chrome::section_height(cx.text, theme);
    let [top, strip, list] = bands(area, theme, figures, head);

    let secs = cx.stats.day_seconds(today);
    let turns: i64 = cx.stats.sittings_on(today).map(|s| s.page_turns).sum();
    let stated = [
        (date::duration(secs, s), s.read_today),
        (turns.to_string(), s.pages_turned),
        (cx.stats.current_streak.to_string(), s.current_streak),
    ];
    chrome::figures(cx.fb, cx.text, theme, top, &stated);

    let inner = chrome::section(
        cx.fb,
        cx.text,
        theme,
        strip,
        &date::long_day(today, s).to_uppercase(),
    );
    let spans = cx.stats.day_blocks(today);
    charts::timeline(cx.fb, cx.text, theme, inner, &spans, Some(cx.now));

    daybooks::paged(cx, list, today, from);
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Stand-ins for `figure_height` and `section_height`.
    const FIGURES: i32 = 90;
    const HEAD: i32 = 40;

    #[test]
    fn the_bands_run_in_order_with_air_between_and_fill_the_page() {
        let theme = Theme::for_screen(1264, 1680);
        let area = crate::ui::chrome::content_box(&theme);
        let [top, strip, list] = bands(area, &theme, FIGURES, HEAD);

        assert_eq!(list.bottom(), area.bottom());
        for pair in [[top, strip], [strip, list]] {
            let air = pair[1].y - pair[0].bottom();
            assert!(air >= theme.gap, "bands touch: {air}");
        }
        assert_eq!(top.h, FIGURES, "the figures get the height they asked for");
        assert_eq!(strip.h, HEAD + theme.row_h * STRIP_ROWS);
    }

    #[test]
    fn the_figures_stand_clear_of_the_edge_and_of_the_day_under_them() {
        let theme = Theme::for_screen(1264, 1680);
        let area = crate::ui::chrome::content_box(&theme);
        let [top, strip, _] = bands(area, &theme, FIGURES, HEAD);

        assert!(
            top.y - area.y >= theme.gap * 2,
            "the figures crowd the top of the page: {}",
            top.y - area.y
        );
        assert!(
            strip.y - top.bottom() >= theme.row_h,
            "the day crowds the figures: {}",
            strip.y - top.bottom()
        );
    }

    #[test]
    fn the_strip_stays_a_strip_and_the_books_take_the_page() {
        // `strip.h` holds at `STRIP_ROWS` on every panel.
        let theme = Theme::for_screen(1264, 1680);
        for (w, h) in [(1264, 1680), (1860, 2480)] {
            let area = crate::ui::chrome::content(&theme, Rect::new(0, 0, w, h));
            let [_, strip, list] = bands(area, &theme, FIGURES, HEAD);
            assert_eq!(strip.h, HEAD + theme.row_h * STRIP_ROWS, "{w}x{h}");
            assert!(
                list.h > strip.h,
                "{w}x{h}: the strip outgrew the list it heads, {} against {}",
                strip.h,
                list.h
            );
        }
    }

    #[test]
    fn a_day_of_several_books_fits_on_one_page() {
        // `bands` leaves `list` four rows of `row_floor` on every panel.
        for (w, h) in [(1264, 1680), (1272, 1696), (1860, 2480)] {
            let theme = Theme::for_screen(w, h);
            let area = crate::ui::chrome::content(&theme, Rect::new(0, 0, w as i32, h as i32));
            let [_, _, list] = bands(area, &theme, FIGURES, HEAD);
            let rows = daybooks::fits(&theme, list.h - HEAD, 99);
            assert!(rows >= 4, "{w}x{h}: room for {rows} of the day's books");
        }
    }

    #[test]
    fn a_page_too_short_for_every_band_keeps_them_all_on_it() {
        let theme = Theme::for_screen(1264, 1680);
        let area = Rect::new(0, 0, 1186, 300);
        let out = bands(area, &theme, FIGURES, HEAD);
        for band in out {
            assert!(band.h >= 0, "{band:?}");
            assert!(band.y >= area.y, "{band:?} starts above the page");
            assert!(band.bottom() <= area.bottom(), "{band:?} runs off the page");
        }
        for pair in out.windows(2) {
            assert!(pair[1].y >= pair[0].bottom(), "{pair:?} overlap");
        }
    }
}
