//! All Time, in two pages.
//!
//! The board is four rows of three figures over the span the log covers, three
//! of the twelve carrying a hit box onto what they name. Trends is the record
//! folded onto a day, a week and a year, over how long a sitting runs.
//!
//! One line at the head of each page tells them apart and pages between them:
//! what the record covers and a `›` on the board, a `‹` and the page's name on
//! Trends. A swipe does the same thing.

use crate::date;
use crate::lang::Strings;
use crate::stats::{Fold, SITTING_BANDS, SITTING_FLOOR_SECS, SITTING_STEP_SECS};
use crate::ui::paint::{self, LIGHT, PALE, Rect};
use crate::ui::theme::Theme;
use crate::ui::{charts, chrome};

use super::{Ctx, Hit, Shelf};

/// How many pages All Time holds.
pub const PAGES: usize = 2;

/// Rows of three the board holds.
const ROWS: i32 = 4;

/// Bands the Trends page stacks: the three folds and the sittings.
const BANDS: i32 = 4;

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

/// The page at `page`, and the arrows onto the one either side of it.
pub fn draw(cx: &mut Ctx, area: Rect, page: usize) {
    let (line, body) = area.split_top(edge_height(cx.theme));
    match page {
        0 => {
            span_line(cx, line);
            edges(cx, line, None, Some(FORWARD));
            let cells = cells(cx);
            board_of(cx, body, &cells);
        }
        _ => {
            let name = cx.s().trends.to_string();
            edges(cx, line, Some(BACK), Some(&name));
            trends(cx, body);
        }
    }
}

/// The arrows onto the page either side. Each is its own hit box, and a swipe
/// does the same thing.
const BACK: &str = "‹";
const FORWARD: &str = "›";

/// The height of the line at the head of a page, which the board sets what the
/// record covers in.
fn edge_height(theme: &Theme) -> i32 {
    theme.small_px as i32 + theme.gap * 2
}

/// A marker at each end of `line`: an arrow onto the page either side, and the
/// page's own name where it has one.
fn edges(cx: &mut Ctx, line: Rect, left: Option<&str>, right: Option<&str>) {
    let theme: &Theme = cx.theme;
    let script = cx.ui_script();
    // One baseline for both ends, taken from the type the line is set in, so
    // an arrow and a name stand on the same line whatever size each takes.
    cx.text.set_px(theme.small_px);
    let baseline = line.y + cx.text.cap_height() as i32;
    for (said, at_left) in [(left, true), (right, false)] {
        let Some(said) = said else { continue };
        // An arrow takes the size the span pages set theirs at; a name sits
        // with the line's own type.
        let arrow = said == BACK || said == FORWARD;
        cx.text.set_px(match arrow {
            true => theme.head_px,
            false => theme.small_px,
        });
        let w = cx.text.measure_width_in(script, said) as i32;
        let x = match at_left {
            true => line.x,
            false => line.right() - w,
        };
        cx.text.draw_in(script, cx.fb, x, baseline, said, false);
        if arrow {
            // The hit box reaches past the arrow's own width, as it does on
            // the span pages.
            let reach = line.w / 6;
            let at = match at_left {
                true => line.x,
                false => line.right() - reach,
            };
            let hit = match at_left {
                true => Hit::Prev,
                false => Hit::Next,
            };
            cx.hit(hit, Rect::new(at, line.y, reach, line.h));
        }
    }
}

/// The three folds over the sitting histogram, in equal bands down the page.
fn trends(cx: &mut Ctx, area: Rect) {
    let rows = area.rows(BANDS, cx.theme.gap * 3);

    let day = cx.stats.average_day(cx.today);
    let names: Vec<String> = (0..24).map(|at| format!("{at:02}")).collect();
    band(cx, rows[0], cx.s().an_average_day, &day, &names, 3);

    let week = cx.stats.average_week(cx.today, cx.week);
    let names: Vec<String> = (0..7)
        .map(|at| cx.s().weekdays_short[cx.week.day_in(at)].to_string())
        .collect();
    band(cx, rows[1], cx.s().an_average_week, &week, &names, 1);

    let year = cx.stats.by_month(cx.today);
    let names: Vec<String> = cx.s().months_short.iter().map(|m| m.to_string()).collect();
    band(cx, rows[2], cx.s().by_month, &year, &names, 1);

    sittings(cx, rows[3]);
}

/// One fold under its own heading: the name, what one turn of the cycle
/// averages, and the fullest bucket.
fn band(cx: &mut Ctx, area: Rect, name: &str, fold: &Fold, axis: &[String], every: usize) {
    let theme: &Theme = cx.theme;
    let s = cx.s();
    let short = format!("{name} · {}", date::duration(fold.each, s));
    let title = match fold.busiest {
        Some(at) => format!("{short} · {} {}", s.most, axis[at]),
        None => short.clone(),
    };
    // The fullest bucket is named only where the row has the width for it.
    cx.text.set_px(theme.small_px);
    let title = match cx.text.measure_width(&title) as i32 <= area.w {
        true => title,
        false => short,
    };
    let inner = chrome::section(cx.fb, cx.text, theme, area, &title);
    let names = axis.to_vec();
    charts::columns(
        cx.fb,
        cx.text,
        theme,
        inner,
        &fold.values,
        |at| names[at].clone(),
        &|secs| duration_rows(secs, s),
        every,
        fold.busiest,
    );
}

/// A duration over its bar, as the rows it is set in: an hour part and a
/// minute part stack, so `2h 22m` is fitted to the width of `22m` and not of
/// both. A bucket holding under a minute states nothing, its bar being a
/// hairline that a figure would stand over as a speck of dirt.
pub(super) fn duration_rows(secs: i64, s: &Strings) -> Vec<String> {
    if secs < 60 {
        return Vec::new();
    }
    let space = if s.unit_space { " " } else { "" };
    // Rounded, as [`date::duration`] is: a bar and the heading over it state
    // the same seconds and must state them alike.
    let (hours, mins) = date::hours_and_minutes(secs);
    let hour = format!("{hours}{space}{}", s.hours);
    let min = format!("{mins}{space}{}", s.minutes);
    match (hours, mins) {
        (0, _) => vec![min],
        (_, 0) => vec![hour],
        _ => vec![hour, min],
    }
}

/// How many sittings of the record ran each length, five minutes to a band.
fn sittings(cx: &mut Ctx, area: Rect) {
    let theme: &Theme = cx.theme;
    let s = cx.s();
    let counted = cx.stats.sitting_bands();
    let total: i64 = counted.iter().sum();
    let names: Vec<String> = (0..SITTING_BANDS).map(|at| sitting_name(at, s)).collect();
    let busiest = counted
        .iter()
        .enumerate()
        .max_by_key(|(_, n)| **n)
        .filter(|(_, n)| **n > 0)
        .map(|(at, _)| at);
    let head = format!("{} · {total} {}", s.sitting_lengths, s.in_all);
    let title = match busiest {
        Some(at) => format!("{head} · {} {}", s.most, names[at]),
        None => head,
    };
    let inner = chrome::section(cx.fb, cx.text, theme, area, &title);
    let axis = names.clone();
    charts::columns(
        cx.fb,
        cx.text,
        theme,
        inner,
        &counted,
        |at| axis[at].clone(),
        &|n| match n {
            0 => Vec::new(),
            n => vec![n.to_string()],
        },
        6,
        busiest,
    );
}

/// The length the band at `at` opens at, for the axis under it. The last band
/// holds everything above its own opening, and says so.
fn sitting_name(at: usize, s: &Strings) -> String {
    // The scale opens where a run first counts as reading, not at zero.
    if at == 0 {
        return date::duration_tight(SITTING_FLOOR_SECS, s);
    }
    let secs = at as i64 * SITTING_STEP_SECS;
    let opens = match secs % 3600 {
        0 => format!("{}{}", secs / 3600, s.hours),
        _ => date::duration_tight(secs, s),
    };
    match at + 1 == SITTING_BANDS {
        true => format!("{opens}+"),
        false => opens,
    }
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
    // The board states what the whole record came to, the same way a span
    // page states its own days.
    let all = cx.stats.tally(opened(cx)..=cx.today);
    let sittings = (cx.stats.sittings.len() as i64).max(1);
    // Every book the record holds, whether or not the shelf can name it.
    let books = (cx.stats.books.len() + cx.stats.unnamed_books()) as i64;
    let finished = finished_books(cx);
    let (best_day, best_day_secs) = best_day(cx);
    let (sat_on, sat_secs) = longest_sitting(cx);

    vec![
        plain(date::duration_coarse(all.read, s), s.total_read),
        plain(all.days_read.to_string(), s.days_read),
        plain(date::duration(all.a_day, s), s.a_day),
        plain(books.to_string(), s.book_count),
        Cell {
            value: all.finished.to_string(),
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
    let line = crate::lang::counted(s.since_days, over)
        .replace("{m}", &date::month_name(year, month, s).to_uppercase());
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
