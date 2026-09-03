//! Today: what has been read since midnight, this week against the one before
//! it, and the standing totals.

use crate::date;
use crate::font::Script;
use crate::ui::chrome;
use crate::ui::paint::{self, INK, Rect};
use crate::ui::{charts, theme::Theme};

use super::{Ctx, Hit};

/// Rows the all-time block draws: five figures and the book in hand.
const TOTAL_ROWS: i32 = 6;

/// The four bands, top to bottom: the figures, the timeline, the week and the
/// totals.
///
/// Each states the height it draws into. What is left over is shared out
/// between them as air, capped at `theme.row_h`.
fn bands(area: Rect, theme: &Theme, figures: i32, head: i32) -> [Rect; 4] {
    let needs = [
        figures,
        head + theme.row_h * 2,
        head + theme.row_h * 4,
        head + theme.row_h * TOTAL_ROWS,
    ];
    let spare = (area.h - needs.iter().sum::<i32>()).max(0);
    let air = (spare / (needs.len() as i32 - 1)).clamp(theme.gap, theme.row_h);

    let mut out = [Rect::default(); 4];
    let mut rest = area;
    for (i, need) in needs.iter().enumerate() {
        // The last band takes the remainder.
        let last = i + 1 == needs.len();
        let take = match last {
            true => rest.h,
            false => (need + air).min(rest.h),
        };
        let (band, left) = rest.split_top(take);
        out[i] = match last {
            true => band,
            // `air` sits between bands. A band with nothing left is empty.
            false => Rect::new(band.x, band.y, band.w, (band.h - air).max(0)),
        };
        rest = left;
    }
    out
}

pub fn draw(cx: &mut Ctx, area: Rect) {
    let theme: &Theme = cx.theme;
    let today = cx.today;

    let figures = chrome::figure_height(cx.text, theme);
    let head = chrome::section_height(cx.text, theme);
    let [top, strip, week, totals] = bands(area, theme, figures, head);

    let secs = cx.stats.day_seconds(today);
    let turns: i64 = cx.stats.sittings_on(today).map(|s| s.page_turns).sum();
    let books = cx.stats.books_on(today);
    let s = cx.s();
    let stated = [
        (date::duration(secs, s), s.read_today),
        (turns.to_string(), s.pages_turned),
        (books.to_string(), s.books_unit),
    ];
    // `spread` sizes each figure to its own width across `top`.
    let widths: Vec<i32> = stated
        .iter()
        .map(|(value, label)| chrome::figure_width(cx.text, theme, value, label))
        .collect();
    let cells = top.spread(&widths, theme.gap * 2);
    for (cell, (value, label)) in cells.into_iter().zip(&stated) {
        chrome::figure(cx.fb, cx.text, theme, cell, value, label);
    }

    let inner = chrome::section(cx.fb, cx.text, theme, strip, s.when_today);
    let spans = cx.stats.day_blocks(today);
    charts::timeline(cx.fb, cx.text, theme, inner, &spans, Some(cx.now));

    let inner = chrome::section(cx.fb, cx.text, theme, week, s.last_seven_days);
    let values = cx.stats.week_ending(today);
    charts::columns(
        cx.fb,
        cx.text,
        theme,
        inner,
        &values,
        |i| {
            let day = today - 6 + i as i64;
            s.weekdays_initial[date::weekday(day)].to_string()
        },
        1,
        Some(6),
    );

    let inner = chrome::section(cx.fb, cx.text, theme, totals, s.all_time);
    let lines = [
        (s.total_read, date::duration(cx.stats.total_seconds, s)),
        (s.books_row, cx.stats.books.len().to_string()),
        (s.days_read, cx.stats.days_read().to_string()),
        (
            s.current_streak,
            plural(cx.stats.current_streak, s.day_one, s.day_many),
        ),
        (
            s.longest_streak,
            plural(cx.stats.longest_streak, s.day_one, s.day_many),
        ),
    ];
    let rows = inner.rows(lines.len() as i32 + 1, 0);
    for ((key, value), row) in lines.iter().zip(&rows) {
        chrome::row(cx.fb, cx.text, theme, *row, key, value);
    }

    // `current_book`, in the last row.
    if let (Some(book), Some(row)) = (cx.stats.current_book(), rows.last()) {
        let script = Script::of_language(&book.language);
        chrome::row(cx.fb, cx.text, theme, *row, s.now_reading, "");
        // Clamped, measured and drawn at `body_px` in `script`.
        cx.text.set_px(theme.body_px);
        let title = cx
            .text
            .wrap_and_clamp_in(script, &book.title, (row.w * 2 / 3) as u32, 1)
            .first()
            .cloned()
            .unwrap_or_default();
        let baseline = row.center_y() + cx.text.cap_height() as i32 / 2;
        let w = cx.text.measure_width_in(script, &title) as i32;
        cx.text
            .draw_in(script, cx.fb, row.right() - w, baseline, &title, false);
        if book.has_percent() {
            let bottom = baseline + cx.text.descent() as i32;
            let track = progress_track(*row, bottom, theme.gap);
            paint::progress(cx.fb, track, book.percent as i64, 100, INK);
        }
        // `books` is most-recent-first: the book in hand is the first.
        cx.hit(Hit::Book(0), *row);
    }
}

/// The progress track under a row of text: at the row's foot, and never
/// closer to the line than `text_bottom`, which its descenders reach.
fn progress_track(row: Rect, text_bottom: i32, gap: i32) -> Rect {
    let thickness = (gap / 2).max(1);
    let top = (row.bottom() - gap)
        .max(text_bottom + 2)
        .min(row.bottom() - thickness);
    Rect::new(row.x, top, row.w, thickness)
}

/// A count with the right noun beside it.
fn plural(n: i64, one: &str, many: &str) -> String {
    match n {
        1 => format!("1 {one}"),
        n => format!("{n} {many}"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_progress_bar_clears_the_line_it_sits_under() {
        // A row barely taller than its text: the bar drops to the foot of the
        // row rather than crossing the descenders of "Now reading".
        let row = Rect::new(0, 0, 400, 69);
        let (baseline, descent, gap) = (47, 11, 14);
        let track = progress_track(row, baseline + descent, gap);
        assert!(
            track.y > baseline + descent,
            "the bar crosses the descenders: bar at {}, text reaches {}",
            track.y,
            baseline + descent
        );
        assert!(track.bottom() <= row.bottom(), "the bar leaves the row");

        // A tall row leaves the bar at the foot, inset as before.
        let tall = Rect::new(0, 0, 400, 200);
        let track = progress_track(tall, 120, gap);
        assert_eq!(track.y, tall.bottom() - gap);
        assert_eq!(track.h, gap / 2);
    }

    #[test]
    fn a_count_takes_the_noun_that_fits_it() {
        assert_eq!(plural(0, "day", "days"), "0 days");
        assert_eq!(plural(1, "day", "days"), "1 day");
        assert_eq!(plural(9, "day", "days"), "9 days");
    }

    #[test]
    fn the_bands_run_in_order_with_air_between_and_fill_the_page() {
        let theme = Theme::for_screen(1264, 1680);
        let area = crate::ui::chrome::content_box(&theme);
        // Stand-ins for `figure_height` and `section_height`.
        let [top, strip, week, totals] = bands(area, &theme, 90, 40);

        assert_eq!(top.y, area.y);
        assert_eq!(totals.bottom(), area.bottom());
        for pair in [[top, strip], [strip, week], [week, totals]] {
            let air = pair[1].y - pair[0].bottom();
            assert!(air >= theme.gap, "bands touch: {air}");
            assert!(air <= theme.row_h, "the air outgrew a row: {air}");
        }
        assert!(week.h > 0);
    }

    #[test]
    fn no_band_takes_the_page_over_from_the_figures_it_serves() {
        let theme = Theme::for_screen(1264, 1680);
        let area = crate::ui::chrome::content_box(&theme);
        let [top, strip, week, totals] = bands(area, &theme, 90, 40);

        assert!(top.h >= 90, "the figures get their own height at least");
        assert!(
            week.h < area.h / 3,
            "the week's chart takes a third of the page at most, got {} of {}",
            week.h,
            area.h
        );
        assert!(
            totals.h > week.h,
            "the standing figures outweigh the chart illustrating them: {} against {}",
            totals.h,
            week.h
        );
        assert!(strip.h > 0);
    }

    #[test]
    fn a_page_too_short_for_every_band_keeps_them_all_on_it() {
        let theme = Theme::for_screen(1264, 1680);
        let area = Rect::new(0, 0, 1186, 300);
        let out = bands(area, &theme, 90, 40);
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
