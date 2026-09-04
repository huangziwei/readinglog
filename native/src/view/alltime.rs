//! The whole record as a board of figures: four rows of three over the span
//! the log covers, one row each to the totals, the shelf, the streaks and the
//! records.

use crate::date;
use crate::lang::Strings;
use crate::ui::paint::{self, PALE, Rect};
use crate::ui::theme::Theme;

use super::Ctx;

/// Rows of three the board holds.
const ROWS: i32 = 4;

/// A book this far through is finished.
const FINISHED_PERCENT: f64 = 99.0;

pub fn draw(cx: &mut Ctx, area: Rect) {
    let theme: &Theme = cx.theme;
    let (since, board) = area.split_top(theme.small_px as i32 + theme.gap * 2);
    span_line(cx, since);
    board_of(cx, board, &cells(cx));
}

/// The twelve figures, row by row.
fn cells(cx: &Ctx) -> Vec<(String, &'static str)> {
    let s = cx.s();
    let over = (cx.today - opened(cx) + 1).max(1);
    let read = cx.stats.days_read().max(1);
    let sittings = (cx.stats.sittings.len() as i64).max(1);
    let books = (cx.stats.books.len() as i64).max(1);
    let finished = cx
        .stats
        .books
        .iter()
        .filter(|b| b.has_percent() && b.percent >= FINISHED_PERCENT)
        .count();
    let best_day = cx
        .stats
        .days
        .iter()
        .map(|(_, secs)| *secs)
        .max()
        .unwrap_or(0);
    let longest = cx
        .stats
        .sittings
        .iter()
        .map(|s| s.seconds)
        .max()
        .unwrap_or(0);

    vec![
        (hours(cx.stats.total_seconds, s), s.total_read),
        (read.to_string(), s.days_read),
        (date::duration(cx.stats.total_seconds / over, s), s.a_day),
        (books.to_string(), s.book_count),
        (finished.to_string(), s.finished),
        (hours(cx.stats.total_seconds / books, s), s.a_book),
        (cx.stats.current_streak.to_string(), s.current_streak),
        (cx.stats.longest_streak.to_string(), s.longest_streak),
        (weeks_running(cx).to_string(), s.weeks_running),
        (date::duration(best_day, s), s.best_day),
        (date::duration(longest, s), s.longest_sitting),
        (
            date::duration(cx.stats.total_seconds / sittings, s),
            s.a_sitting,
        ),
    ]
}

/// The first day the record holds.
fn opened(cx: &Ctx) -> i64 {
    cx.stats.days.first().map(|(d, _)| *d).unwrap_or(cx.today)
}

/// Weeks back from the one holding today with a day read in each.
///
/// A week with nothing read in it yet leaves the count on the week before it.
fn weeks_running(cx: &Ctx) -> i64 {
    let opens = |day: i64| day - cx.week.column_of(date::weekday(day)) as i64;
    let any = |week: i64| (week..week + 7).any(|d| cx.stats.day_seconds(d) > 0);
    let mut week = opens(cx.today);
    if !any(week) {
        week -= 7;
    }
    let mut running = 0;
    while any(week) {
        running += 1;
        week -= 7;
    }
    running
}

/// A whole-hour total: minutes carry nothing over a record this wide.
fn hours(secs: i64, s: &Strings) -> String {
    let space = if s.unit_space { " " } else { "" };
    format!("{}{space}{}", (secs + 1800) / 3600, s.hours)
}

/// What the record covers, over the board.
fn span_line(cx: &mut Ctx, area: Rect) {
    let s = cx.s();
    let first = opened(cx);
    let over = (cx.today - first + 1).max(1);
    let (year, month, _) = date::civil_from_days(first);
    let line = s
        .since_days
        .replace("{m}", &date::month_name(year, month, s).to_uppercase())
        .replace("{d}", &over.to_string());
    cx.text.set_px(cx.theme.small_px);
    let baseline = area.y + cx.text.cap_height() as i32;
    let script = cx.ui_script();
    cx.text
        .draw_in(script, cx.fb, area.x, baseline, &line, false);
}

/// `figures` in rows of three down `area`, each row at its own size. Every
/// figure is centred in a fixed third, and the three column centres hold
/// across rows of unequal size.
fn board_of(cx: &mut Ctx, area: Rect, figures: &[(String, &'static str)]) {
    let theme: &Theme = cx.theme;
    let rows = area.rows(ROWS, 0);
    let column_w = area.w / 3 - theme.gap * 2;

    for (line, band) in figures.chunks(3).zip(&rows) {
        let px = fitting_px(cx, line, column_w);
        for ((value, label), cell) in line.iter().zip(band.columns(3, 0)) {
            figure(cx, cell, value, label, px);
        }
    }
    for row in rows.iter().skip(1) {
        paint::hline(cx.fb, area.x, row.y, area.w, PALE, 1);
    }
}

/// The largest size at or under [`Theme::display_px`] that sets every value in
/// `figures` inside one column.
fn fitting_px(cx: &mut Ctx, figures: &[(String, &'static str)], column_w: i32) -> f32 {
    let theme: &Theme = cx.theme;
    let mut px = theme.display_px;
    while px > theme.body_px {
        cx.text.set_px(px);
        let widest = figures
            .iter()
            .map(|(value, _)| cx.text.measure_width(value) as i32)
            .max()
            .unwrap_or(0);
        if widest <= column_w {
            break;
        }
        // A width is near enough proportional to `px` to land in one step; the
        // pixel taken off it settles the rounding.
        px = (px * column_w as f32 / widest.max(1) as f32)
            .min(px - 1.0)
            .max(theme.body_px);
    }
    px
}

/// One figure centred in `cell`, its name under it.
fn figure(cx: &mut Ctx, cell: Rect, value: &str, label: &str, px: f32) {
    let theme: &Theme = cx.theme;
    cx.text.set_px(px);
    let cap = cx.text.cap_height() as i32;
    cx.text.set_px(theme.small_px);
    let line = cx.text.line_height() as i32;
    let block = cap + theme.gap + line;
    let top = cell.y + (cell.h - block).max(0) / 2 + cap;
    let script = cx.ui_script();

    cx.text.set_px(px);
    let w = cx.text.measure_width(value) as i32;
    cx.text
        .draw(cx.fb, cell.x + (cell.w - w) / 2, top, value, false);

    cx.text.set_px(theme.small_px);
    let lw = cx.text.measure_width_in(script, label) as i32;
    cx.text.draw_in(
        script,
        cx.fb,
        cell.x + (cell.w - lw) / 2,
        top + theme.gap + line,
        label,
        false,
    );
}
