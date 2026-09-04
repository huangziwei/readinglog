//! The shapes a record is read in: a grid of days, the names around one, a
//! day's hours as a strip, a row of columns, a stack of labelled bars, and one
//! day's sittings laid along the clock.
//!
//! [`week_cells`], [`month_cells`] and [`heatmap`] state where a day lands,
//! apart from any draw.

use crate::date;
use crate::eink::fb::Framebuffer;
use crate::font::Script;
use crate::lang::Strings;
use crate::settings::WeekStart;

use super::paint::{self, DARK, INK, LIGHT, PALE, Rect};
use super::text::TextRenderer;
use super::theme::Theme;

/// One day of a month grid: which day, and the box it occupies.
///
/// Seven columns from whichever day `week` starts on, over as many rows as the
/// month reaches into — five or six, and the rows fill `area` either way.
pub fn month_cells(
    area: Rect,
    year: i64,
    month: i64,
    gap: i32,
    week: WeekStart,
) -> Vec<(i64, Rect)> {
    let first = date::days_from_civil(year, month, 1);
    let lead = week.column_of(date::weekday(first)) as i64;
    let cols = area.columns(7, gap);
    let rows = area.rows(month_rows(year, month, week), gap);
    let mut out = Vec::new();
    for d in 1..=date::days_in_month(year, month) {
        let slot = lead + d - 1;
        let (col, row) = ((slot % 7) as usize, (slot / 7) as usize);
        if let (Some(c), Some(r)) = (cols.get(col), rows.get(row)) {
            out.push((first + d - 1, Rect::new(c.x, r.y, c.w, r.h)));
        }
    }
    out
}

/// A week's seven days, one column each, starting at `first`.
pub fn week_cells(area: Rect, first: i64, gap: i32) -> Vec<(i64, Rect)> {
    area.columns(7, gap)
        .into_iter()
        .enumerate()
        .map(|(column, cell)| (first + column as i64, cell))
        .collect()
}

/// Rows of seven a month reaches into, from whichever day `week` starts on.
pub fn month_rows(year: i64, month: i64, week: WeekStart) -> i32 {
    let first = date::days_from_civil(year, month, 1);
    let lead = week.column_of(date::weekday(first)) as i64;
    ((lead + date::days_in_month(year, month) + 6) / 7) as i32
}

/// The height [`weekday_head`] draws into.
pub fn weekday_head_height(theme: &Theme) -> i32 {
    theme.small_px as i32 * 2
}

/// The weekday names across the head of a grid, each over its own column.
pub fn weekday_head(
    fb: &mut Framebuffer,
    text: &mut TextRenderer,
    theme: &Theme,
    s: &Strings,
    area: Rect,
    week: WeekStart,
) {
    text.set_px(theme.small_px);
    for (column, cell) in area.columns(7, theme.gap).into_iter().enumerate() {
        let name = s.weekdays_short[week.day_in(column)];
        let w = text.measure_width(name) as i32;
        text.draw(
            fb,
            cell.x + (cell.w - w) / 2,
            area.y + text.line_height() as i32,
            name,
            false,
        );
    }
}

/// A year of days laid out one column to a week, seven rows deep from
/// whichever day `week` starts on, in square cells.
///
/// The year's fifty-three weeks are cut into blocks stacked down the page: one
/// block is the shape GitHub draws, and two of them set a cell twice the size
/// on a panel this narrow.
pub struct Heatmap {
    /// Every day of the year and the box it occupies.
    pub cells: Vec<(i64, Rect)>,
    /// Each month, and the box its name stands in over the first week column
    /// opening inside it.
    pub months: Vec<(i64, Rect)>,
    /// Seven weekday rows for each block, in order, for the names beside them.
    pub rows: Vec<Rect>,
    /// The side of one cell, which the blocks are cut to widen.
    #[cfg_attr(not(test), allow(dead_code))]
    pub side: i32,
    /// The height the blocks come to, the air between them included.
    pub height: i32,
}

/// [`Heatmap`] over `year` in `blocks` bands, each headed by `label` px of
/// month names and set `air` apart. The cell is sized by `area`'s width.
#[allow(clippy::too_many_arguments)]
pub fn heatmap(
    area: Rect,
    year: i64,
    gap: i32,
    week: WeekStart,
    blocks: i32,
    label: i32,
    air: i32,
) -> Heatmap {
    let first = date::days_from_civil(year, 1, 1);
    let last = date::days_from_civil(year, 12, 31);
    let start = first - week.column_of(date::weekday(first)) as i64;
    let columns = ((last - start) / 7 + 1).max(1) as i32;
    let blocks = blocks.max(1);
    let per = (columns + blocks - 1) / blocks;
    let side = ((area.w - gap * (per - 1)) / per).max(1);
    let step = side + gap;
    let block_h = label + side * 7 + gap * 6;

    let mut cells = Vec::new();
    let mut months = Vec::new();
    let mut rows = Vec::new();
    for block in 0..blocks {
        let top = area.y + block * (block_h + air);
        for at in 0..per {
            let column = block * per + at;
            if column >= columns {
                break;
            }
            let x = area.x + at * step;
            let opens = start + column as i64 * 7;
            let (_, month, dom) = date::civil_from_days(opens);
            // A month is named over the first week opening inside its own
            // first seven days, which every month has exactly one of.
            if dom <= 7 && (first..=last).contains(&opens) {
                months.push((month, Rect::new(x, top, side * 3, label)));
            }
            for row in 0..7 {
                let day = opens + row as i64;
                if (first..=last).contains(&day) {
                    cells.push((day, Rect::new(x, top + label + row * step, side, side)));
                }
            }
        }
        rows.extend((0..7).map(|row| Rect::new(area.x, top + label + row * step, area.w, side)));
    }
    cells.sort_unstable_by_key(|(day, _)| *day);
    Heatmap {
        cells,
        months,
        rows,
        side,
        height: block_h * blocks + air * (blocks - 1),
    }
}

/// Which of four steps a day sits on against the busiest beside it, or zero
/// where nothing was read.
pub fn level(secs: i64, peak: i64) -> usize {
    if secs <= 0 || peak <= 0 {
        return 0;
    }
    let ratio = secs as f64 / peak as f64;
    match ratio {
        r if r > 0.66 => 4,
        r if r > 0.4 => 3,
        r if r > 0.15 => 2,
        _ => 1,
    }
}

/// The ink a [`level`] draws in, off the darker four of [`paint::STEPS_RGB`],
/// and `None` at zero.
pub fn level_rgb(level: usize) -> Option<[u8; 3]> {
    (level > 0)
        .then(|| paint::STEPS_RGB.get(level))
        .flatten()
        .copied()
}

/// One book's run down a lane of a week: which book, the column the run opens
/// on, and how many columns it covers.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Run {
    pub book: usize,
    pub start: usize,
    pub span: usize,
}

/// `days` — one entry per day of a week, each holding that day's books
/// longest first — laid into `depth` lanes a column.
///
/// A book read on consecutive days holds one lane across them and carries the
/// whole run, so the caller draws one bar where `start` is its own column.
pub fn lanes(days: &[Vec<usize>], depth: usize) -> Vec<Vec<Option<Run>>> {
    let mut out: Vec<Vec<Option<Run>>> = Vec::with_capacity(days.len());
    for (column, books) in days.iter().enumerate() {
        let mut here: Vec<Option<Run>> = (0..depth)
            .map(|lane| {
                books.get(lane).map(|book| Run {
                    book: *book,
                    start: column,
                    span: 1,
                })
            })
            .collect();
        if column > 0 {
            for (lane, before) in out[column - 1].clone().into_iter().enumerate() {
                let Some(before) = before else {
                    continue;
                };
                let Some(found) = here
                    .iter()
                    .position(|r| r.is_some_and(|r| r.book == before.book))
                else {
                    continue;
                };
                let span = before.span + 1;
                if let Some(run) = &mut here[found] {
                    run.start = before.start;
                    run.span = span;
                }
                // Every column the run covers carries its new length.
                for back in 1..span {
                    if let Some(run) = &mut out[column - back][lane] {
                        run.span = span;
                    }
                }
                here.swap(found, lane);
            }
        }
        out.push(here);
    }
    out
}

/// One day's twenty-four hours as bars across `area`, against `peak`.
///
/// `peak` is the busiest hour of every day drawn beside this one, so a quiet
/// day and a busy one are read off the same scale.
pub fn hour_shape(fb: &mut Framebuffer, area: Rect, hours: &[i64; 24], peak: i64) {
    if peak <= 0 || area.h <= 0 {
        return;
    }
    // A bar takes the whole hour it stands for, so the hours run together
    // into one shape and not a row of needles.
    let step = (area.w / 24).max(1);
    for (hour, secs) in hours.iter().enumerate() {
        let h = ((area.h as i64 * secs / peak) as i32).max(2 * (*secs > 0) as i32);
        if h > 0 {
            let x = area.x + hour as i32 * step;
            paint::fill_rgb(fb, Rect::new(x, area.bottom() - h, step, h), paint::BAR_RGB);
        }
    }
}

/// How a column's own figure is set, as the rows it takes. An empty answer
/// states nothing, which is what an empty bucket wants: a figure standing over
/// a bar that is not there reads as a mark on the page.
pub type Figure<'a> = &'a dyn Fn(i64) -> Vec<String>;

/// A row of columns, one per entry in `values`, each carrying its own figure.
///
/// `every` names one bucket of the axis in that many, and the last always,
/// that being the one which can name an overflow. `highlight` marks the
/// fullest bar of what is drawn.
///
/// A figure of two parts stacks inside its bar where the bar is tall enough
/// for two lines, so `2h 22m` is fitted to the width of `22m` and not of both.
/// A bar too short for the lines carries them over itself in ink instead.
#[allow(clippy::too_many_arguments)]
pub fn columns(
    fb: &mut Framebuffer,
    text: &mut TextRenderer,
    theme: &Theme,
    area: Rect,
    values: &[i64],
    axis: impl Fn(usize) -> String,
    figure: Figure,
    every: usize,
    highlight: Option<usize>,
) {
    if values.is_empty() {
        return;
    }
    text.set_px(theme.small_px);
    let line = text.line_height() as i32;
    let (plot, foot) = area.split_top((area.h - line - theme.gap / 2).max(1));
    let max = values.iter().copied().max().unwrap_or(0).max(1);
    let gap = match values.len() > 16 {
        true => 2,
        false => theme.gap / 2,
    };
    let cells = plot.columns(values.len() as i32, gap);
    let bar_w = bar_width(theme, cells.first().map_or(0, |c| c.w));

    // The rows a bar carries, settled at the largest size a figure can take:
    // a bar holding two lines there holds two at any size below it.
    let said: Vec<Vec<String>> = values
        .iter()
        .map(|value| {
            let rows = figure(*value);
            let h = (plot.h as i64 * value / max) as i32;
            match holds(theme, h, line, rows.len()) {
                true => rows,
                false => joined(rows),
            }
        })
        .collect();
    let room = bar_w - theme.gap / 2;
    let px = figure_px(theme, &said, room, |px, s| {
        text.set_px(px);
        text.measure_width(s) as i32
    });
    // The row states its figures only where every bar has the width for one,
    // so a row is either figured throughout or bare.
    text.set_px(px);
    let stated = all_fit(&said, |s| text.measure_width(s) as i32, room);

    paint::hline(fb, plot.x, plot.bottom(), plot.w, LIGHT, 1);
    for (at, (value, cell)) in values.iter().zip(&cells).enumerate() {
        let h = (plot.h as i64 * value / max) as i32;
        let bar = Rect::new(cell.x + (cell.w - bar_w) / 2, cell.bottom() - h, bar_w, h);
        if h > 0 {
            let ink = match highlight == Some(at) {
                true => paint::MARK_RGB,
                false => paint::BAR_RGB,
            };
            paint::fill_rgb(fb, bar, ink);
        }
        if stated && !said[at].is_empty() {
            draw_figure(fb, text, theme, bar, &said[at], px);
        }
        if at % every.max(1) == 0 || at + 1 == values.len() {
            text.set_px(theme.small_px);
            let name = axis(at);
            let w = text.measure_width(&name) as i32;
            text.draw(fb, cell.x + (cell.w - w) / 2, foot.y + line, &name, false);
        }
    }
}

/// The air a figure keeps from the head of the bar it stands in, and from the
/// foot of the bar above.
fn inset(theme: &Theme) -> i32 {
    theme.gap
}

/// Whether a bar `h` tall holds `rows` lines of `line` with that air around
/// them.
fn holds(theme: &Theme, h: i32, line: i32, rows: usize) -> bool {
    rows > 0 && h >= line * rows as i32 + inset(theme) * 2
}

/// `rows` as the one line a bar too short for them carries instead.
fn joined(rows: Vec<String>) -> Vec<String> {
    match rows.is_empty() {
        true => rows,
        false => vec![rows.concat()],
    }
}

/// How wide a bar stands inside a cell `cell_w` across.
fn bar_width(theme: &Theme, cell_w: i32) -> i32 {
    (cell_w * 3 / 4).min(theme.row_h).max(1)
}

/// The largest size at or under [`Theme::small_px`] that sets every line of
/// every figure inside `room`, floored where type stops being readable.
///
/// `width` measures a string at a size. Kept apart from the paint so a figure
/// wider than the bar it stands in is caught by a test rather than by looking
/// at a screenshot.
fn figure_px(
    theme: &Theme,
    said: &[Vec<String>],
    room: i32,
    mut width: impl FnMut(f32, &str) -> i32,
) -> f32 {
    let floor = theme.small_px * FIGURE_FLOOR;
    let mut px = theme.small_px;
    while px > floor {
        let widest = said
            .iter()
            .flatten()
            .map(|s| width(px, s.as_str()))
            .max()
            .unwrap_or(0);
        if widest <= room {
            break;
        }
        // A width is near enough proportional to `px` to land in one step; the
        // pixel taken off it settles the rounding.
        px = (px * room as f32 / widest.max(1) as f32)
            .min(px - 1.0)
            .max(floor);
    }
    px
}

/// How far under [`Theme::small_px`] a figure may be set before it stops being
/// readable on the panel.
const FIGURE_FLOOR: f32 = 0.6;

/// Whether every figure of `said` measures within `room`, which is what
/// decides whether the row states them at all.
fn all_fit(said: &[Vec<String>], mut width: impl FnMut(&str) -> i32, room: i32) -> bool {
    said.iter().flatten().all(|s| width(s) <= room)
}

/// `said` inside `bar`, stacked from its head, white on the bar's own ink.
fn draw_figure(
    fb: &mut Framebuffer,
    text: &mut TextRenderer,
    theme: &Theme,
    bar: Rect,
    said: &[String],
    px: f32,
) {
    text.set_px(px);
    let line = text.line_height() as i32;
    let cap = text.cap_height() as i32;
    let inside = holds(theme, bar.h, line, said.len());
    let mut baseline = match inside {
        true => bar.y + inset(theme) + cap,
        false => bar.y - inset(theme) / 2 - line * (said.len() as i32 - 1),
    };
    for row in said {
        let w = text.measure_width(row) as i32;
        text.draw(fb, bar.x + (bar.w - w) / 2, baseline, row, inside);
        baseline += line;
    }
}

/// A stack of named bars with the figure at the right. Each row carries the
/// convention its name is set in, so a book's title takes the same face here
/// as it does on its own screen.
pub fn bars(
    fb: &mut Framebuffer,
    text: &mut TextRenderer,
    theme: &Theme,
    s: &Strings,
    area: Rect,
    rows: &[(Script, String, i64)],
) {
    if rows.is_empty() {
        return;
    }
    let max = rows.iter().map(|(_, _, v)| *v).max().unwrap_or(0).max(1);
    let each = (area.h / rows.len() as i32).min(theme.row_h);
    text.set_px(theme.body_px);
    for (i, (script, name, value)) in rows.iter().enumerate() {
        let row = Rect::new(area.x, area.y + i as i32 * each, area.w, each);
        let figure = date::duration(*value, s);
        let fw = text.measure_width(&figure) as i32;
        let baseline = row.center_y() + text.cap_height() as i32 / 2;

        // The bar draws behind `name`.
        let track = Rect::new(
            row.x,
            row.y + theme.gap / 2,
            row.w - fw - theme.gap * 2,
            each - theme.gap,
        );
        let filled = (track.w as i64 * value / max) as i32;
        paint::fill_rgb(
            fb,
            Rect::new(track.x, track.y, filled, track.h),
            paint::STEPS_RGB[0],
        );
        let clipped = text.wrap_and_clamp_in(*script, name, (track.w - theme.gap) as u32, 1);
        text.draw_in(
            *script,
            fb,
            row.x + theme.gap / 2,
            baseline,
            clipped.first().map(String::as_str).unwrap_or(name),
            false,
        );
        text.draw(fb, row.right() - fw, baseline, &figure, false);
    }
}

/// One day laid along the clock, each of `spans` a block where it happened.
///
/// The strip covers all 24 hours whatever `spans` holds.
pub fn timeline(
    fb: &mut Framebuffer,
    text: &mut TextRenderer,
    theme: &Theme,
    area: Rect,
    spans: &[(i64, i64)],
    now: Option<i64>,
) {
    text.set_px(theme.small_px);
    let label_h = text.line_height() as i32 + theme.gap / 2;
    let (strip, axis) = area.split_top(area.h - label_h);
    paint::stroke(fb, strip, LIGHT, 1);

    let at = |secs: i64| strip.x + (strip.w as i64 * secs.clamp(0, 86_400) / 86_400) as i32;
    for hour in (0..=24).step_by(6) {
        let x = at(hour as i64 * 3600);
        paint::vline(fb, x, strip.y, strip.h, PALE, 1);
        let name = format!("{hour:02}");
        let w = text.measure_width(&name) as i32;
        text.draw(
            fb,
            (x - w / 2).clamp(area.x, area.right() - w),
            axis.y + text.line_height() as i32,
            &name,
            false,
        );
    }
    for (from, to) in spans {
        let (x0, x1) = (at(*from), at(*to));
        // A span under 3 px wide draws 3 px.
        let w = (x1 - x0).max(3);
        paint::fill(fb, Rect::new(x0, strip.y + 1, w, strip.h - 2), INK);
    }
    // `now` down the strip.
    if let Some(secs) = now {
        let x = at(secs);
        paint::vline(fb, x, strip.y - theme.gap / 2, strip.h + theme.gap, DARK, 2);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_month_takes_the_rows_it_reaches_into_and_fills_them() {
        // September 2026 opens on a Tuesday and needs five; August 2026 opens
        // on a Saturday and needs six.
        assert_eq!(month_rows(2026, 9, WeekStart::Monday), 5);
        assert_eq!(month_rows(2026, 8, WeekStart::Monday), 6);
        // A Sunday-first week pushes that Saturday into a seventh column.
        assert_eq!(month_rows(2026, 8, WeekStart::Sunday), 6);
        // February on the day it starts on takes four.
        assert_eq!(month_rows(2027, 2, WeekStart::Monday), 4);

        let area = Rect::new(0, 40, 700, 600);
        for (year, month) in [(2026, 9), (2026, 8), (2027, 2)] {
            let cells = month_cells(area, year, month, 0, WeekStart::Monday);
            let last = cells.last().expect("a day").1;
            assert_eq!(last.bottom(), area.bottom(), "{year}-{month} leaves a void");
        }
    }

    /// The panels this primitive has to hold up on.
    const PANELS: [(i32, i32); 3] = [(1264, 1680), (1272, 1696), (1860, 2480)];

    /// A metric with no font behind it: every character 0.6 em, which is wider
    /// than Ember sets and narrower than an ideograph, so a figure that fits
    /// under it fits on the device.
    fn stub_width(px: f32, s: &str) -> i32 {
        (s.chars().count() as f32 * px * 0.6).round() as i32
    }

    #[test]
    fn a_figure_is_set_narrow_enough_for_the_bar_it_stands_in() {
        // One size for the whole row, taken from the widest figure. A unit
        // of two ideographs sets far wider than an `h`, so the widest line is
        // what the row has to hold.
        for (w, h) in PANELS {
            let theme = Theme::for_screen(w as u32, h as u32);
            for count in [7usize, 12, 24, 25] {
                let plot = Rect::new(0, 0, w - theme.pad * 2, 400);
                let gap = if count > 16 { 2 } else { theme.gap / 2 };
                let cell = plot.columns(count as i32, gap)[0];
                let room = bar_width(&theme, cell.w) - theme.gap / 2;
                let said: Vec<Vec<String>> = (0..count)
                    .map(|_| vec!["38h".into(), "51m".into()])
                    .collect();
                let px = figure_px(&theme, &said, room, stub_width);
                assert!(px >= theme.small_px * FIGURE_FLOOR, "{w}x{h}: {px} px");
                // Either every figure of the row fits its bar at that size, or
                // the row states none: a figure is never drawn over its edge.
                let widest = said
                    .iter()
                    .flatten()
                    .map(|s| stub_width(px, s))
                    .max()
                    .unwrap_or(0);
                assert_eq!(
                    all_fit(&said, |s| stub_width(px, s), room),
                    widest <= room,
                    "{w}x{h}, {count} bars: {widest} px of figure in {room}"
                );
            }
        }
    }

    #[test]
    fn a_bar_holds_its_figure_only_where_the_lines_fit_with_air() {
        let theme = Theme::for_screen(1264, 1680);
        let line = 40;
        let air = inset(&theme) * 2;
        assert!(
            !holds(&theme, line + air - 1, line, 1),
            "one line, too short"
        );
        assert!(holds(&theme, line + air, line, 1), "one line, exactly");
        assert!(
            !holds(&theme, line * 2 + air - 1, line, 2),
            "two, too short"
        );
        assert!(holds(&theme, line * 2 + air, line, 2), "two, exactly");
        assert!(!holds(&theme, 9_999, line, 0), "no lines never fit");
    }

    #[test]
    fn a_figure_of_two_parts_joins_onto_one_line() {
        let rows = vec!["2h".to_string(), "22m".to_string()];
        assert_eq!(joined(rows), vec!["2h22m".to_string()]);
        assert!(joined(Vec::new()).is_empty(), "nothing stays nothing");
    }

    /// The blocks the assertions below are written against.
    const BLOCKS: i32 = 2;

    #[test]
    fn a_year_of_weeks_holds_every_day_once_in_square_cells() {
        for year in [2024, 2026] {
            let area = Rect::new(20, 30, 1100, 700);
            let week = WeekStart::Monday;
            let map = heatmap(area, year, 2, week, BLOCKS, 20, 10);
            let want: i64 = (1..=12).map(|m| date::days_in_month(year, m)).sum();
            assert_eq!(map.cells.len(), want as usize, "{year}");
            for pair in map.cells.windows(2) {
                assert_eq!(pair[1].0, pair[0].0 + 1, "{year} skips a day");
            }
            for (_, cell) in &map.cells {
                assert_eq!(cell.w, cell.h, "{year}: a cell is not square");
                assert!(cell.x >= area.x && cell.right() <= area.right(), "{cell:?}");
                assert!(cell.y >= area.y, "{cell:?}");
                assert!(cell.bottom() <= area.y + map.height, "{cell:?}");
            }

            // A day sits on the weekday row it fell on, in the block its own
            // week was cut into.
            let jan = date::days_from_civil(year, 1, 1);
            let lead = week.column_of(date::weekday(jan)) as i64;
            let columns = map.rows.len() as i64 / 7;
            let per = (map.cells.last().expect("a day").0 - jan + lead) / 7 / columns + 1;
            for (day, cell) in &map.cells {
                let slot = day - jan + lead;
                let row = slot.rem_euclid(7) as usize;
                let block = (slot / 7 / per).min(columns - 1) as usize;
                assert_eq!(
                    cell.y,
                    map.rows[block * 7 + row].y,
                    "{year}: {day} is off its weekday row"
                );
            }

            assert_eq!(map.months.len(), 12, "{year} names {:?}", map.months);
            for pair in map.months.windows(2) {
                let (before, after) = (pair[0].1, pair[1].1);
                assert!(
                    after.y > before.y || after.x > before.x,
                    "{year} names its months out of order"
                );
            }
            assert_eq!(map.rows.len(), 7 * BLOCKS as usize);
        }
    }

    #[test]
    fn cutting_a_year_into_blocks_doubles_the_cell() {
        // What the blocks buy: a day a finger can find.
        let area = Rect::new(0, 0, 1100, 900);
        let one = heatmap(area, 2026, 2, WeekStart::Monday, 1, 20, 10);
        let two = heatmap(area, 2026, 2, WeekStart::Monday, 2, 20, 10);
        assert!(
            two.side >= one.side * 2,
            "one block sets {} px, two set {}",
            one.side,
            two.side
        );
        assert_eq!(one.cells.len(), two.cells.len());
        assert!(two.height > one.height);
    }

    #[test]
    fn a_days_level_bands_it_against_the_busiest_of_the_span() {
        assert_eq!(level(0, 100), 0);
        assert_eq!(level(100, 0), 0);
        assert_eq!(level(10, 100), 1);
        assert_eq!(level(20, 100), 2);
        assert_eq!(level(50, 100), 3);
        assert_eq!(level(100, 100), 4);
        // A day with nothing on it takes no ink at all.
        assert!(level_rgb(0).is_none());
        assert!(level_rgb(1).is_some());
        assert_eq!(level_rgb(4), Some(paint::STEPS_RGB[4]));
    }

    #[test]
    fn a_book_read_two_days_running_holds_one_lane_across_them() {
        // Three days: the same book on the first two, another under it.
        let days = vec![vec![7usize, 3], vec![7], vec![3]];
        let out = lanes(&days, 4);
        let run = out[0][0].expect("a run");
        assert_eq!((run.book, run.start, run.span), (7, 0, 2));
        assert_eq!(out[1][0].expect("the same run"), run);
        // The run is drawn once, where it opens.
        let drawn: Vec<Run> = out
            .iter()
            .enumerate()
            .flat_map(|(col, lane)| {
                lane.iter()
                    .flatten()
                    .filter(move |r| r.start == col)
                    .copied()
            })
            .collect();
        assert_eq!(drawn.len(), 3, "{drawn:?}");
        assert_eq!(drawn.iter().filter(|r| r.book == 7).count(), 1);

        // A day between breaks the run in two.
        let out = lanes(&[vec![7usize], vec![], vec![7]], 4);
        assert_eq!(out[0][0].expect("a run").span, 1);
        assert_eq!(out[2][0].expect("a run").span, 1);
    }

    #[test]
    fn a_lane_never_holds_more_books_than_it_has_depth_for() {
        let out = lanes(&[vec![1usize, 2, 3, 4, 5, 6]], 3);
        assert_eq!(out[0].len(), 3);
        let books: Vec<usize> = out[0].iter().flatten().map(|r| r.book).collect();
        assert_eq!(books, vec![1, 2, 3], "the longest read come first");
    }

    #[test]
    fn a_sunday_week_moves_every_day_one_column_right() {
        // The same days in the same order, starting a column later — and the
        // grid still holds every day of the month.
        let area = Rect::new(0, 0, 700, 600);
        for (year, month) in [(2026, 8), (2026, 2), (2024, 2), (2026, 11)] {
            let mon = month_cells(area, year, month, 0, WeekStart::Monday);
            let sun = month_cells(area, year, month, 0, WeekStart::Sunday);
            assert_eq!(mon.len(), sun.len(), "{year}-{month} loses a day");
            let days: Vec<i64> = sun.iter().map(|(d, _)| *d).collect();
            let want: Vec<i64> = mon.iter().map(|(d, _)| *d).collect();
            assert_eq!(days, want, "{year}-{month} reorders the days");
        }
        // 2026-08-01 is a Saturday: column 5 Monday-first, column 6 Sunday-first.
        let first = date::days_from_civil(2026, 8, 1);
        assert_eq!(date::weekday(first), 5);
        assert_eq!(WeekStart::Monday.column_of(5), 5);
        assert_eq!(WeekStart::Sunday.column_of(5), 6);
    }

    #[test]
    fn a_month_starts_on_the_weekday_it_really_starts_on() {
        // 1 August 2026 is a Saturday, the sixth column.
        let area = Rect::new(0, 0, 700, 600);
        let cells = month_cells(area, 2026, 8, 0, WeekStart::Monday);
        assert_eq!(cells.len(), 31);
        let (first, rect) = cells[0];
        assert_eq!(date::weekday(first), 5);
        assert_eq!(rect.x, 5 * 100);
        assert_eq!(rect.y, 0);
    }

    #[test]
    fn the_days_of_a_month_run_in_order_across_and_down() {
        let cells = month_cells(Rect::new(0, 0, 700, 600), 2026, 8, 0, WeekStart::Monday);
        for pair in cells.windows(2) {
            assert_eq!(pair[1].0, pair[0].0 + 1);
        }
        // The 2nd is a Sunday, ending the first row.
        assert_eq!(cells[1].1.x, 6 * 100);
        // The 3rd is a Monday, opening the second.
        assert_eq!(cells[2].1.x, 0);
        assert_eq!(cells[2].1.y, 100);
    }

    #[test]
    fn every_cell_of_every_month_stays_inside_the_grid() {
        let area = Rect::new(10, 20, 700, 600);
        for year in [2024, 2026] {
            for month in 1..=12 {
                for (_, cell) in month_cells(area, year, month, 4, WeekStart::Monday) {
                    assert!(cell.x >= area.x, "{year}-{month}");
                    assert!(cell.right() <= area.right(), "{year}-{month}");
                    assert!(cell.y >= area.y, "{year}-{month}");
                    assert!(cell.bottom() <= area.bottom(), "{year}-{month}");
                }
            }
        }
    }

    #[test]
    fn a_leap_february_gets_its_extra_day() {
        assert_eq!(
            month_cells(Rect::new(0, 0, 700, 600), 2024, 2, 0, WeekStart::Monday).len(),
            29
        );
        assert_eq!(
            month_cells(Rect::new(0, 0, 700, 600), 2026, 2, 0, WeekStart::Monday).len(),
            28
        );
    }

    #[test]
    fn no_two_days_of_a_month_share_a_box() {
        let cells = month_cells(Rect::new(0, 0, 700, 600), 2026, 8, 2, WeekStart::Monday);
        for (i, (_, a)) in cells.iter().enumerate() {
            for (_, b) in &cells[i + 1..] {
                assert!(a != b, "{a:?}");
            }
        }
    }
}
