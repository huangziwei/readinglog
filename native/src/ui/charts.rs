//! The shapes a record is read in: grids of days, a day's hours as a strip, a
//! row of columns, labelled bars, and a day's sittings along the clock.
//! [`week_cells`], [`month_cells`] and [`heatmap`] place a day with no draw.

use crate::date;
use crate::eink::fb::Framebuffer;
use crate::lang::Strings;
use crate::settings::WeekStart;

use super::chrome;
use super::paint::{self, DARK, INK, LIGHT, PALE, Palette, Rect};
use super::text::TextRenderer;
use super::theme::Theme;

/// One day of a month grid: which day, and the box it occupies. Seven columns
/// from whichever day `week` starts on, over the five or six rows the month
/// reaches into, filling `area` either way.
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

/// A year of days, one column to a week, seven rows deep from whichever day
/// `week` starts on, in square cells. The fifty-three weeks are cut into blocks
/// stacked down the page, which doubles the cell size on a narrow panel.
pub struct Heatmap {
    /// Every day of the year and the box it occupies.
    pub cells: Vec<(i64, Rect)>,
    /// Each month, and the box its name stands in.
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

/// One book's run down a lane of a week: which book, the column the run opens
/// on, and how many columns it covers.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Run {
    pub book: usize,
    pub start: usize,
    pub span: usize,
}

/// `days` — one entry per day of a week, each holding that day's books longest
/// first — laid into `depth` lanes a column. A book read on consecutive days
/// holds one lane across them, as one bar for the run.
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

/// One day's twenty-four hours as bars across `area`, in `rgb`, against `peak`
/// — the busiest hour of every day drawn beside this one, one scale across
/// them all.
pub fn hour_shape(fb: &mut Framebuffer, area: Rect, hours: &[i64; 24], peak: i64, rgb: [u8; 3]) {
    if peak <= 0 || area.h <= 0 || area.w < 24 {
        return;
    }
    // A bar takes the whole hour it stands for: the hours run together into
    // one shape.
    let step = (area.w / 24).max(1);
    for (hour, secs) in hours.iter().enumerate() {
        let h = ((area.h as i64 * secs / peak) as i32).max(2 * (*secs > 0) as i32);
        if h > 0 {
            let x = area.x + hour as i32 * step;
            paint::fill_rgb(fb, Rect::new(x, area.bottom() - h, step, h), rgb);
        }
    }
}

/// How a column's own figure is set, as the rows it takes. An empty answer
/// states nothing, which is what a bar that is not there carries.
pub type Figure<'a> = &'a dyn Fn(i64) -> Vec<String>;

/// A row of columns, one per entry in `values`, each carrying its own figure.
/// `every` names one bucket of the axis in that many, and the last always;
/// `highlight` marks the fullest bar drawn; `ceiling` is [`scale`]'s.
#[allow(clippy::too_many_arguments)]
pub fn columns(
    fb: &mut Framebuffer,
    text: &mut TextRenderer,
    theme: &Theme,
    palette: Palette,
    area: Rect,
    values: &[i64],
    axis: impl Fn(usize) -> String,
    figure: Figure,
    every: usize,
    highlight: Option<usize>,
    ceiling: Option<i64>,
) {
    if values.is_empty() {
        return;
    }
    text.set_px(theme.small_px);
    let line = text.line_height() as i32;
    let (band, foot) = area.split_top((area.h - line - theme.gap / 2).max(1));
    let max = scale(values, ceiling);
    let gap = match values.len() > 16 {
        true => 2,
        false => theme.gap / 2,
    };
    let cell_w = band.columns(values.len() as i32, gap)[0].w;
    let cut = Cut {
        h: band.h,
        line,
        bar_w: bar_width(theme, cell_w),
        cell_w,
        gap,
    };
    let set = settle(theme, values, max, &cut, figure, &mut |px, said| {
        text.set_px(px);
        text.measure_width(said) as i32
    });

    // The bars stand under the air a figure over the tallest of them takes,
    // and on the foot of the band either way.
    let plot = Rect::new(
        band.x,
        band.y + set.head,
        band.w,
        (band.h - set.head).max(1),
    );
    let cells = plot.columns(values.len() as i32, gap);
    paint::hline(fb, plot.x, plot.bottom(), plot.w, LIGHT, 1);
    let bars: Vec<Rect> = values
        .iter()
        .zip(&cells)
        .map(|(value, cell)| {
            let h = (plot.h as i64 * value / max) as i32;
            Rect::new(
                cell.x + (cell.w - cut.bar_w) / 2,
                cell.bottom() - h,
                cut.bar_w,
                h,
            )
        })
        .collect();
    for (at, bar) in bars.iter().enumerate() {
        if bar.h > 0 {
            let ink = match highlight == Some(at) {
                true => palette.mark,
                false => palette.bar(),
            };
            paint::fill_rgb(fb, *bar, ink);
        }
    }
    // The figures come after every bar of the row: one reaching over its own
    // cell stands on its neighbours, never under them.
    for (at, bar) in bars.iter().enumerate() {
        if !set.said[at].is_empty() {
            draw_figure(fb, text, theme, band, *bar, &set, at);
        }
    }

    text.set_px(theme.small_px);
    let stated = named(&cells, every, theme.gap, &axis, &mut |said| {
        text.measure_width(said) as i32
    });
    for (x, name) in stated {
        text.draw(fb, x, foot.y + line, &name, false);
    }
}

/// The value a full-height bar stands for: `ceiling`, or the tallest of
/// `values` where `ceiling` is `None`. A value over `ceiling` raises it.
fn scale(values: &[i64], ceiling: Option<i64>) -> i64 {
    let top = values.iter().copied().max().unwrap_or(0);
    ceiling.unwrap_or(top).max(top).max(1)
}

/// What the axis states and where, taken from the right: one of `cells` in
/// `every` and the last always, less a name the cell to its right states and a
/// name reaching within `gap` of it. `width` measures a name.
fn named(
    cells: &[Rect],
    every: usize,
    gap: i32,
    axis: &dyn Fn(usize) -> String,
    width: &mut dyn FnMut(&str) -> i32,
) -> Vec<(i32, String)> {
    let Some(last) = cells.len().checked_sub(1) else {
        return Vec::new();
    };
    let mut marks: Vec<usize> = (0..cells.len()).step_by(every.max(1)).collect();
    if marks.last() != Some(&last) {
        marks.push(last);
    }
    // A name centred on the first or the last cell keeps inside the row.
    let (left, right) = (cells[0].x, cells[last].right());
    let mut out: Vec<(i32, String)> = Vec::new();
    let mut reach = i32::MAX;
    for at in marks.into_iter().rev() {
        let name = axis(at);
        let w = width(&name);
        let x = (cells[at].x + (cells[at].w - w) / 2).clamp(left, (right - w).max(left));
        if out.last().is_some_and(|(_, last)| *last == name) || x + w + gap > reach {
            continue;
        }
        reach = x;
        out.push((x, name));
    }
    out
}

/// The room a row of columns is cut to: the height the bars are drawn into,
/// one line of [`Theme::small_px`], the width of a bar and of the cell it
/// stands in, and the air between one cell and the next.
struct Cut {
    h: i32,
    line: i32,
    bar_w: i32,
    cell_w: i32,
    gap: i32,
}

/// How a row of columns states its figures.
struct Figures {
    /// What each bar states, in the lines it is set in. An empty answer states
    /// nothing.
    said: Vec<Vec<String>>,
    /// Whether each figure stands inside its own bar, in place of over it.
    inside: Vec<bool>,
    /// The size every figure of the row is set at.
    px: f32,
    /// The air at the head of the band a figure over the tallest bar stands in.
    head: i32,
    /// How many cells one figure of the row spreads over. A row stating every
    /// bar spreads over one.
    #[cfg_attr(not(test), allow(dead_code))]
    step: usize,
}

impl Figures {
    /// A row stating nothing.
    fn none(bars: usize, px: f32) -> Self {
        Figures {
            said: vec![Vec::new(); bars],
            inside: vec![false; bars],
            px,
            head: 0,
            step: 1,
        }
    }
}

/// Where a row of columns sets its figures and at what size: inside each bar
/// that has the height for one, over its head where not, at the one size every
/// figure fits its [`rooms`] at. A row too tight for that goes to [`thinned`].
fn settle(
    theme: &Theme,
    values: &[i64],
    max: i64,
    cut: &Cut,
    figure: Figure,
    width: &mut dyn FnMut(f32, &str) -> i32,
) -> Figures {
    let rows: Vec<Vec<String>> = values.iter().map(|value| figure(*value)).collect();
    let placed = |plot_h: i32| -> (Vec<Vec<String>>, Vec<bool>) {
        values
            .iter()
            .zip(&rows)
            .map(|(value, said)| {
                let h = (plot_h as i64 * value / max) as i32;
                match holds(theme, h, cut.line, said.len()) {
                    true => (said.clone(), true),
                    // The air over a bar carries the lines it has the height
                    // for. Lines with neither the bar nor the air for them
                    // join onto one.
                    false => match clears(theme, cut.h - h, cut.line, said.len()) {
                        true => (said.clone(), false),
                        false => {
                            let one = joined(said.clone());
                            let inside = holds(theme, h, cut.line, one.len());
                            (one, inside)
                        }
                    },
                }
            })
            .unzip()
    };
    let (said, inside) = placed(cut.h);
    let room = rooms(theme, cut, &said, &inside);
    let (px, fits) = sized(theme, &said, &|at| room[at], width);
    if fits {
        return Figures {
            said,
            inside,
            px,
            head: 0,
            step: 1,
        };
    }

    // Every figure of the row over its bar, the tallest of them standing in
    // the air the head of the band keeps.
    let lines = rows.iter().map(Vec::len).max().unwrap_or(0) as i32;
    let head = cut.line * lines + inset(theme) / 2;
    let (flat, _) = placed(cut.h - head);
    let over = vec![false; values.len()];
    let room = rooms(theme, cut, &flat, &over);
    let (px, fits) = sized(theme, &flat, &|at| room[at], width);
    if fits {
        return Figures {
            said: flat,
            inside: over,
            px,
            head,
            step: 1,
        };
    }

    thinned(theme, values, max, cut, &rows, width)
        .unwrap_or_else(|| Figures::none(values.len(), px))
}

/// The figures spread evenly across the row, as many as the width carries: the
/// first and last bar always state theirs, and the ones between them thin out
/// a step at a time until what is stated fits the room the skipped cells leave.
fn thinned(
    theme: &Theme,
    values: &[i64],
    max: i64,
    cut: &Cut,
    rows: &[Vec<String>],
    width: &mut dyn FnMut(f32, &str) -> i32,
) -> Option<Figures> {
    let bars = values.len();
    if bars < 2 {
        return None;
    }
    let lines = rows.iter().map(Vec::len).max().unwrap_or(0) as i32;
    let head = cut.line * lines + inset(theme) / 2;
    let plot = cut.h - head;
    // From every bar but one down to the two ends alone.
    for stating in (2..bars).rev() {
        let marks = spread(bars, stating);
        let said: Vec<Vec<String>> = rows
            .iter()
            .enumerate()
            .map(|(at, lines)| match marks.contains(&at) {
                true => lines.clone(),
                false => Vec::new(),
            })
            .collect();
        // A stated figure has its own cell and the skipped ones beside it; the
        // closest pair of them settles the room for the whole row.
        let step = marks
            .windows(2)
            .map(|pair| pair[1] - pair[0])
            .min()
            .unwrap_or(1);
        if step < 2 {
            continue;
        }
        let room = cut.cell_w * step as i32 - cut.gap;
        let (px, fits) = sized(theme, &said, &|_| room, width);
        // Every stated figure has the air over its own bar to stand in.
        let stands = values.iter().enumerate().all(|(at, value)| {
            let h = (plot as i64 * value / max) as i32;
            said[at].is_empty() || clears(theme, cut.h - h, cut.line, said[at].len())
        });
        if fits && stands {
            return Some(Figures {
                said,
                inside: vec![false; bars],
                px,
                head,
                step,
            });
        }
    }
    None
}

/// The width each figure of the row has: a figure `inside` its bar has the
/// bar's own, and one over a bar the run of cells reaching to the nearest
/// figure either side of it.
fn rooms(theme: &Theme, cut: &Cut, said: &[Vec<String>], inside: &[bool]) -> Vec<i32> {
    let stated: Vec<usize> = (0..said.len()).filter(|at| !said[*at].is_empty()).collect();
    said.iter()
        .enumerate()
        .map(|(at, lines)| {
            if lines.is_empty() {
                return cut.cell_w;
            }
            if inside[at] {
                return cut.bar_w - theme.gap / 2;
            }
            let before = stated.iter().rev().find(|o| **o < at).map(|o| at - o);
            let after = stated.iter().find(|o| **o > at).map(|o| o - at);
            let near = before.into_iter().chain(after).min().unwrap_or(said.len());
            cut.cell_w * near as i32 - cut.gap
        })
        .collect()
}

/// `stating` marks spread evenly over `bars`, the first and the last included.
fn spread(bars: usize, stating: usize) -> Vec<usize> {
    match stating {
        0 => Vec::new(),
        1 => vec![0],
        _ => (0..stating)
            .map(|at| at * (bars - 1) / (stating - 1))
            .collect(),
    }
}

/// The size `said` is set at, and whether every line of it lands inside the
/// `room` its own bar leaves at that size.
fn sized(
    theme: &Theme,
    said: &[Vec<String>],
    room: &dyn Fn(usize) -> i32,
    width: &mut dyn FnMut(f32, &str) -> i32,
) -> (f32, bool) {
    let px = figure_px(theme, said, room, &mut *width);
    (px, all_fit(said, room, |line| width(px, line)))
}

/// The air a figure keeps inside the head of its own bar. Half of it stands
/// between a figure over a bar and that bar's head.
fn inset(theme: &Theme) -> i32 {
    theme.gap
}

/// Whether a bar `h` tall holds `rows` lines of `line` with that air around
/// them.
fn holds(theme: &Theme, h: i32, line: i32, rows: usize) -> bool {
    rows > 0 && h >= line * rows as i32 + inset(theme) * 2
}

/// Whether `air` over a bar's head holds `rows` lines of `line` standing there.
fn clears(theme: &Theme, air: i32, line: i32, rows: usize) -> bool {
    rows > 0 && air >= line * rows as i32 + inset(theme) / 2
}

/// `rows` as the one line a bar too short for them carries.
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
/// every figure inside the `room` its own bar leaves. `width` measures a
/// string at a size.
fn figure_px(
    theme: &Theme,
    said: &[Vec<String>],
    room: &dyn Fn(usize) -> i32,
    width: impl FnMut(f32, &str) -> i32,
) -> f32 {
    // Each bar's lines carry its own index into `room`.
    let (lines, at): (Vec<&str>, Vec<usize>) = said
        .iter()
        .enumerate()
        .flat_map(|(bar, lines)| lines.iter().map(move |line| (line.as_str(), bar)))
        .unzip();
    let floor = theme.small_px * FIGURE_FLOOR;
    chrome::shrink_to_fit(theme.small_px, floor, &lines, &|i| room(at[i]), width)
}

/// How far under [`Theme::small_px`] a figure may be set. A figure gives up
/// size a long way before it gives up being on the page at all.
const FIGURE_FLOOR: f32 = 0.45;

/// Whether every figure of `said` measures within the `room` it stands in. A
/// row failing this states no figure at all.
fn all_fit(
    said: &[Vec<String>],
    room: &dyn Fn(usize) -> i32,
    mut width: impl FnMut(&str) -> i32,
) -> bool {
    said.iter()
        .enumerate()
        .all(|(at, lines)| lines.iter().all(|line| width(line) <= room(at)))
}

/// The figure bar `at` carries, centred on the bar: at its head where it
/// stands inside, white on the bar's own ink, and over its head in the page's
/// own ink where not. A figure wider than its bar keeps inside `band`.
fn draw_figure(
    fb: &mut Framebuffer,
    text: &mut TextRenderer,
    theme: &Theme,
    band: Rect,
    bar: Rect,
    set: &Figures,
    at: usize,
) {
    let (said, inside) = (&set.said[at], set.inside[at]);
    text.set_px(set.px);
    let line = text.line_height() as i32;
    let cap = text.cap_height() as i32;
    let mut baseline = match inside {
        true => bar.y + inset(theme) + cap,
        false => bar.y - inset(theme) / 2 - line * (said.len() as i32 - 1),
    };
    for row in said {
        let w = text.measure_width(row) as i32;
        let x = (bar.x + (bar.w - w) / 2).clamp(band.x, (band.right() - w).max(band.x));
        text.draw(fb, x, baseline, row, inside);
        baseline += line;
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
        paint::vline(
            fb,
            x,
            strip.y - theme.gap / 2,
            strip.h + theme.gap,
            DARK,
            theme.rule(),
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ui::theme::tests::PANELS;

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

    /// A metric with no font behind it: every character 0.6 em, wider than
    /// Ember sets and narrower than an ideograph.
    fn stub_width(px: f32, s: &str) -> i32 {
        (s.chars().count() as f32 * px * 0.6).round() as i32
    }

    #[test]
    fn a_figure_is_set_narrow_enough_for_the_bar_it_stands_in() {
        // One size for the whole row, taken from its widest figure.
        for (w, h) in PANELS {
            let theme = Theme::for_screen(w, h);
            for count in [7usize, 12, 24, 25] {
                let plot = Rect::new(0, 0, theme.screen.w - theme.pad * 2, 400);
                let gap = if count > 16 { 2 } else { theme.gap / 2 };
                let cell = plot.columns(count as i32, gap)[0];
                let room = bar_width(&theme, cell.w) - theme.gap / 2;
                let said: Vec<Vec<String>> = (0..count)
                    .map(|_| vec!["38h".into(), "51m".into()])
                    .collect();
                let px = figure_px(&theme, &said, &|_| room, stub_width);
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
                    all_fit(&said, &|_| room, |s| stub_width(px, s)),
                    widest <= room,
                    "{w}x{h}, {count} bars: {widest} px of figure in {room}"
                );
            }
        }
    }

    /// The rows the Trends page stacks, each as the values it draws and the
    /// figure every bar of it states: a day's hours and a week's days in
    /// minutes, a year's months in hours, and the sitting lengths as counts.
    fn trend_rows() -> Vec<(Vec<i64>, Figure<'static>)> {
        static SPAN: fn(i64) -> Vec<String> = |secs| match secs {
            0 => Vec::new(),
            secs if secs < 3600 => vec![format!("{}m", secs / 60)],
            secs => vec![
                format!("{}h", secs / 3600),
                format!("{}m", secs % 3600 / 60),
            ],
        };
        static COUNTED: fn(i64) -> Vec<String> = |n| match n {
            0 => Vec::new(),
            n => vec![n.to_string()],
        };
        static PLACE: fn(i64) -> Vec<String> = |at| vec![format!("{at}%")];
        vec![
            ((0..24).map(|at| at * 300).collect(), &SPAN as Figure),
            ((0..7).map(|at| 3600 + at * 900).collect(), &SPAN),
            ((0..12).map(|at| 40 * 3600 + at * 3600).collect(), &SPAN),
            ((0..25).map(|at| (260 >> at).max(1)).collect(), &COUNTED),
            // A book's place, one bar a sitting: the densest row drawn, and
            // the only one whose length the record decides.
            ((0..32).map(|at| at * 3).collect(), &PLACE),
            ((0..60).map(|at| at + 20).collect(), &PLACE),
        ]
    }

    #[test]
    fn every_bar_of_a_row_states_its_own_figure() {
        for (w, h) in PANELS {
            let theme = Theme::for_screen(w, h);
            let line = (theme.small_px * 1.2) as i32;
            for (values, figure) in trend_rows() {
                for deep in [theme.row_h * 2, theme.row_h * 3] {
                    let band = Rect::new(0, 0, w as i32 - theme.pad * 2, deep);
                    let gap = match values.len() > 16 {
                        true => 2,
                        false => theme.gap / 2,
                    };
                    let cell_w = band.columns(values.len() as i32, gap)[0].w;
                    let cut = Cut {
                        h: deep,
                        line,
                        bar_w: bar_width(&theme, cell_w),
                        cell_w,
                        gap,
                    };
                    let max = *values.iter().max().expect("a bar");
                    let set = settle(&theme, &values, max, &cut, figure, &mut stub_width);
                    // A cell holding the widest figure of the row at the
                    // smallest size type is set at states every one of them.
                    let floor = theme.small_px * FIGURE_FLOOR;
                    let every = values
                        .iter()
                        .flat_map(|value| figure(*value))
                        .all(|said| stub_width(floor, &said) <= cut.cell_w - gap);
                    let bars = values.len();
                    let wide = |at: usize| {
                        set.said[at]
                            .iter()
                            .map(|said| stub_width(set.px, said))
                            .max()
                            .unwrap_or(0)
                    };
                    // No two figures of the row run into each other, and one
                    // inside its bar keeps the bar's own edges clear.
                    let mut reach: Option<i32> = None;
                    for (at, value) in values.iter().enumerate() {
                        // An empty bucket draws no bar and states no figure.
                        if every {
                            assert_eq!(
                                set.said[at].is_empty(),
                                figure(*value).is_empty(),
                                "{w}x{h}, {bars} bars {deep} deep: bar {at} of {value}"
                            );
                        }
                        if set.said[at].is_empty() {
                            continue;
                        }
                        if set.inside[at] {
                            let bar = cut.bar_w - theme.gap / 2;
                            assert!(
                                wide(at) <= bar,
                                "{w}x{h}, {bars} bars: bar {at} holds {} px in {bar}",
                                wide(at)
                            );
                            continue;
                        }
                        let centre = at as i32 * cut.cell_w + cut.cell_w / 2;
                        let left = centre - wide(at) / 2;
                        if let Some(edge) = reach {
                            assert!(
                                left >= edge,
                                "{w}x{h}, {bars} bars: figure {at} opens at {left}, past {edge}"
                            );
                        }
                        reach = Some(centre + wide(at) / 2 + cut.gap);
                    }
                }
            }
        }
    }

    #[test]
    fn a_bar_holding_neither_the_lines_nor_the_air_takes_them_joined() {
        let theme = Theme::for_screen(1264, 1680);
        let line = theme.small_px as i32;
        let cut = Cut {
            // One line inside the tallest bar, and short of two either way.
            h: line + inset(&theme) * 2 + 1,
            line,
            bar_w: theme.row_h,
            cell_w: theme.row_h * 2,
            gap: theme.gap / 2,
        };
        let figure: Figure = &|secs| vec![format!("{}h", secs / 3600), "30m".into()];
        let set = settle(&theme, &[3600], 3600, &cut, figure, &mut stub_width);
        assert_eq!(set.said[0], vec!["1h30m".to_string()], "onto one line");
        assert!(set.inside[0], "inside the bar it is too wide to stand over");
        assert_eq!(set.head, 0, "and the band keeps no air for it");
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
        // Two blocks over one year: a day of twice the side.
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
        let pal = paint::Palette::KURENAI_KON;
        assert!(pal.level(0).is_none());
        assert!(pal.level(1).is_some());
        assert_eq!(pal.level(4), Some(pal.steps[4]));
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
        // The same days in the same order, starting a column later. The grid
        // holds every day of the month.
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

    #[test]
    fn a_row_too_dense_to_state_every_figure_thins_them_evenly() {
        for (w, h, bars) in [(1264u32, 1680u32, 60usize), (600, 800, 40)] {
            let theme = Theme::for_screen(w, h);
            let line = (theme.small_px * 1.2) as i32;
            let values: Vec<i64> = (1..=bars as i64).map(|at| at + 20).collect();
            let figure: Figure = &|at| vec![format!("{at}%")];
            let band = Rect::new(0, 0, chrome::content_box(&theme).w, theme.row_h * 3);
            let cell_w = band.columns(bars as i32, 2)[0].w;
            let cut = Cut {
                h: band.h,
                line,
                bar_w: bar_width(&theme, cell_w),
                cell_w,
                gap: 2,
            };
            // A bar this narrow holds no figure at the smallest size type is
            // set at.
            let floor = theme.small_px * FIGURE_FLOOR;
            assert!(stub_width(floor, "80%") > cut.bar_w - theme.gap / 2);

            let max = *values.iter().max().expect("a bar");
            let set = settle(&theme, &values, max, &cut, figure, &mut stub_width);
            let stated: Vec<usize> = (0..bars).filter(|at| !set.said[*at].is_empty()).collect();
            assert!(set.step >= 2, "{w}x{h}: step {}", set.step);
            // The two ends always state theirs, with more between them.
            assert_eq!(stated.first(), Some(&0), "{w}x{h}");
            assert_eq!(stated.last(), Some(&(bars - 1)), "{w}x{h}");
            assert!(stated.len() > 2, "{w}x{h}: {} stated", stated.len());
            // Evenly spread: no two runs differ by more than a cell.
            let runs: Vec<usize> = stated.windows(2).map(|two| two[1] - two[0]).collect();
            let (near, far) = (
                runs.iter().min().expect("a run"),
                runs.iter().max().expect("a run"),
            );
            assert!(far - near <= 1, "{w}x{h}: runs {near}..{far}");
            // Every stated figure fits the run of cells it was given.
            for at in stated {
                for said in &set.said[at] {
                    let wide = stub_width(set.px, said);
                    let room = cut.cell_w * (*near).max(1) as i32 - cut.gap;
                    assert!(wide <= room, "{w}x{h}: `{said}` is {wide} px in {room}");
                }
            }
        }
    }

    #[test]
    fn a_bounded_quantity_fills_its_band_at_the_bound() {
        // `places` tops out at 8 of a bound of 100.
        let places = [6i64, 6, 7, 8, 8];
        assert_eq!(scale(&places, Some(100)), 100);
        assert_eq!(scale(&places, None), 8);
        assert_eq!(scale(&[1, 55, 100], Some(100)), 100);
        // 140 over a `ceiling` of 100 scales to 140.
        assert_eq!(scale(&[40, 140], Some(100)), 140);
        // An all-zero row and an empty one both scale to a band of their own.
        assert_eq!(scale(&[0, 0], Some(100)), 100);
        assert_eq!(scale(&[0, 0], None), 1);
        assert_eq!(scale(&[], None), 1);
    }

    #[test]
    fn buckets_sharing_a_name_state_it_once() {
        // `days` names three days across five cells.
        let days = ["Aug 28", "Aug 31", "Sep 1", "Sep 1", "Sep 7"];
        let cells = Rect::new(0, 0, 1200, 300).columns(days.len() as i32, 6);
        let stated = named(&cells, 1, 12, &|at| days[at].to_string(), &mut |said| {
            stub_width(24.0, said)
        });
        let said: Vec<&str> = stated.iter().map(|(_, name)| name.as_str()).collect();
        assert_eq!(said, ["Sep 7", "Sep 1", "Aug 31", "Aug 28"]);
        // "Sep 1" stands over the rightmost cell carrying it.
        let at = stated
            .iter()
            .position(|(_, name)| name == "Sep 1")
            .expect("Sep 1");
        assert!(stated[at].0 > cells[2].x, "Sep 1 names the earlier bucket");

        // `same` names one day over six cells and states it once.
        let same = ["Aug 30"; 6];
        let cells = Rect::new(0, 0, 1200, 300).columns(same.len() as i32, 6);
        let stated = named(&cells, 1, 12, &|at| same[at].to_string(), &mut |said| {
            stub_width(24.0, said)
        });
        assert_eq!(stated.len(), 1);
    }

    #[test]
    fn an_axis_name_keeps_inside_the_row() {
        // `1月22日` is wider than the cell the first of 29 buckets has.
        let names: Vec<String> = (0..29).map(|at| format!("{}月22日", at % 12 + 1)).collect();
        let band = Rect::new(40, 0, 1186, 300);
        let cells = band.columns(names.len() as i32, 2);
        let stated = named(&cells, 7, 12, &|at| names[at].clone(), &mut |said| {
            stub_width(24.0, said)
        });
        assert!(!stated.is_empty());
        for (x, name) in &stated {
            let w = stub_width(24.0, name);
            assert!(*x >= band.x, "`{name}` opens at {x}, left of {}", band.x);
            let right = band.right();
            assert!(x + w <= right, "`{name}` closes at {}, past {right}", x + w);
        }
    }

    #[test]
    fn the_axis_always_names_its_last_bucket() {
        let names: Vec<String> = (0..25).map(|at| format!("{at:02}")).collect();
        let cells = Rect::new(0, 0, 1200, 300).columns(names.len() as i32, 2);
        for every in [1usize, 3, 6, 24, 40] {
            let stated = named(&cells, every, 12, &|at| names[at].clone(), &mut |said| {
                stub_width(20.0, said)
            });
            assert_eq!(
                stated.first().map(|(_, name)| name.as_str()),
                Some("24"),
                "every {every}"
            );
        }
    }
}
