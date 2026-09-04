//! The whole record as a board of figures: four rows of three over the span
//! the log covers, one row each to the totals, the shelf, the streaks and the
//! records. Three of the twelve carry a hit box onto what they name.

use crate::date;
use crate::lang::Strings;
use crate::ui::paint::{self, LIGHT, PALE, Rect};
use crate::ui::theme::Theme;

use super::{Ctx, Hit, Shelf};

/// Rows of three the board holds.
const ROWS: i32 = 4;

/// One figure of the board: what it states, what it is called, and the page it
/// opens.
struct Cell {
    value: String,
    label: &'static str,
    opens: Option<Hit>,
}

/// The figure alone, opening nothing.
fn plain(value: String, label: &'static str) -> Cell {
    Cell {
        value,
        label,
        opens: None,
    }
}

pub fn draw(cx: &mut Ctx, area: Rect) {
    let theme: &Theme = cx.theme;
    let (since, board) = area.split_top(theme.small_px as i32 + theme.gap * 2);
    span_line(cx, since);
    let cells = cells(cx);
    board_of(cx, board, &cells);
}

/// Books the shelf states as read through, longest first.
fn finished_books(cx: &Ctx) -> Vec<usize> {
    let mut out: Vec<usize> = (0..cx.stats.books.len())
        .filter(|at| cx.stats.books[*at].is_finished())
        .collect();
    out.sort_by_key(|at| -cx.stats.books[*at].seconds);
    out
}

/// The twelve figures, row by row.
fn cells(cx: &Ctx) -> Vec<Cell> {
    let s = cx.s();
    let over = (cx.today - opened(cx) + 1).max(1);
    let read = cx.stats.days_read().max(1);
    let sittings = (cx.stats.sittings.len() as i64).max(1);
    // Every book the record holds, whether or not the shelf can name it.
    let books = (cx.stats.books.len() + cx.stats.unnamed_books()) as i64;
    let finished = finished_books(cx);
    let (best_day, best_day_secs) = best_day(cx);
    let (sat_on, sat_secs) = longest_sitting(cx);

    vec![
        plain(hours(cx.stats.total_seconds, s), s.total_read),
        plain(read.to_string(), s.days_read),
        plain(date::duration(cx.stats.total_seconds / over, s), s.a_day),
        plain(books.to_string(), s.book_count),
        Cell {
            value: finished.len().to_string(),
            label: s.finished,
            opens: (!finished.is_empty()).then_some(Hit::Shelved(Shelf::Finished)),
        },
        plain(a_book(cx, &finished, s), s.a_book),
        plain(cx.stats.current_streak.to_string(), s.current_streak),
        plain(cx.stats.longest_streak.to_string(), s.longest_streak),
        plain(weeks_running(cx).to_string(), s.weeks_running),
        Cell {
            value: date::duration(best_day_secs, s),
            label: s.best_day,
            opens: (best_day_secs > 0).then_some(Hit::Day(best_day)),
        },
        Cell {
            value: date::duration(sat_secs, s),
            label: s.longest_sitting,
            opens: (sat_secs > 0).then_some(Hit::Day(sat_on)),
        },
        plain(
            date::duration(cx.stats.total_seconds / sittings, s),
            s.a_sitting,
        ),
    ]
}

/// How long a finished book took, averaged over the books in `finished`.
///
/// A shelf with none of them read through states no average.
fn a_book(cx: &Ctx, finished: &[usize], s: &Strings) -> String {
    if finished.is_empty() {
        return "—".into();
    }
    let spent: i64 = finished.iter().map(|at| cx.stats.books[*at].seconds).sum();
    hours(spent / finished.len() as i64, s)
}

/// The first day the record holds.
fn opened(cx: &Ctx) -> i64 {
    cx.stats.days.first().map(|(d, _)| *d).unwrap_or(cx.today)
}

/// The fullest day of the record, and what was read on it.
fn best_day(cx: &Ctx) -> (i64, i64) {
    cx.stats
        .days
        .iter()
        .max_by_key(|(_, secs)| *secs)
        .map(|(day, secs)| (*day, *secs))
        .unwrap_or((cx.today, 0))
}

/// The longest single sitting of the record, and the day it fell on.
fn longest_sitting(cx: &Ctx) -> (i64, i64) {
    cx.stats
        .sittings
        .iter()
        .max_by_key(|s| s.seconds)
        .map(|s| (s.day, s.seconds))
        .unwrap_or((cx.today, 0))
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

/// `cells` in rows of three down `area`, each row at its own size. Every
/// figure is centred in a fixed third, and the three column centres hold
/// across rows of unequal size.
fn board_of(cx: &mut Ctx, area: Rect, cells: &[Cell]) {
    let theme: &Theme = cx.theme;
    let rows = area.rows(ROWS, 0);
    let column_w = area.w / 3 - theme.gap * 2;

    for (line, band) in cells.chunks(3).zip(&rows) {
        let px = fitting_px(cx, line, column_w);
        for (cell, box_) in line.iter().zip(band.columns(3, 0)) {
            figure(cx, box_, cell, px);
            if let Some(hit) = cell.opens {
                cx.hit(hit, box_);
            }
        }
    }
    for row in rows.iter().skip(1) {
        paint::hline(cx.fb, area.x, row.y, area.w, PALE, 1);
    }
}

/// The largest size at or under [`Theme::display_px`] that sets every value in
/// `cells` inside one column.
fn fitting_px(cx: &mut Ctx, cells: &[Cell], column_w: i32) -> f32 {
    let theme: &Theme = cx.theme;
    let mut px = theme.display_px;
    while px > theme.body_px {
        cx.text.set_px(px);
        let widest = cells
            .iter()
            .map(|cell| cx.text.measure_width(&cell.value) as i32)
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

/// One figure centred in `box_`, its name under it. A name carrying
/// [`Cell::opens`] is underlined.
fn figure(cx: &mut Ctx, box_: Rect, cell: &Cell, px: f32) {
    let theme: &Theme = cx.theme;
    cx.text.set_px(px);
    let cap = cx.text.cap_height() as i32;
    cx.text.set_px(theme.small_px);
    let line = cx.text.line_height() as i32;
    let block = cap + theme.gap + line;
    let top = box_.y + (box_.h - block).max(0) / 2 + cap;
    let script = cx.ui_script();

    cx.text.set_px(px);
    let w = cx.text.measure_width(&cell.value) as i32;
    cx.text
        .draw(cx.fb, box_.x + (box_.w - w) / 2, top, &cell.value, false);

    cx.text.set_px(theme.small_px);
    let lw = cx.text.measure_width_in(script, cell.label) as i32;
    let at = box_.x + (box_.w - lw) / 2;
    let baseline = top + theme.gap + line;
    cx.text
        .draw_in(script, cx.fb, at, baseline, cell.label, false);
    if cell.opens.is_some() {
        paint::hline(cx.fb, at, baseline + theme.gap / 2, lw, LIGHT, 2);
    }
}
