//! The charts, as SVG drawn off `Stats` series: [`Look`] holds every colour as
//! a literal, `charts::level` gives a day's step, and `book_graphs::cut_to`
//! cuts the columns a book's two bands stand on.

use std::fmt::Write as _;

use readinglog_native::date;
use readinglog_native::lang::Strings;
use readinglog_native::settings::WeekStart;
use readinglog_native::stats::{Fold, Stats};
use readinglog_native::ui::{charts, chrome};
use readinglog_native::view::{alltime, book_graphs};

use super::esc;
use super::look::{Look, hex};

/// Opens an `<svg>` `width` by `height`, its `font-family` drawn wherever a
/// `<text>` takes no family from the page.
fn open(width: f32, height: f32) -> String {
    format!(
        "<svg xmlns=\"http://www.w3.org/2000/svg\" viewBox=\"0 0 {width} {height}\" \
         width=\"{width}\" height=\"{height}\" font-family=\"-apple-system, system-ui, Helvetica Neue, sans-serif\">"
    )
}

/// The year laid out a week to a column, `week` deciding the first row:
/// the column of each day and how wide one column is.
#[derive(Clone, Copy)]
pub struct Weeks {
    pub first: i64,
    pub last: i64,
    /// The day the first column's first row stands for.
    pub origin: i64,
    pub columns: i64,
    pub left: f32,
    pub pitch: f32,
}

impl Weeks {
    pub fn of(year: i64, week: WeekStart, left: f32, width: f32) -> Self {
        let first = date::days_from_civil(year, 1, 1);
        let last = date::days_from_civil(year, 12, 31);
        let origin = first - week.column_of(date::weekday(first)) as i64;
        let columns = (last - origin) / 7 + 1;
        Self {
            first,
            last,
            origin,
            columns,
            left,
            pitch: (width - left) / columns as f32,
        }
    }

    pub fn column(&self, day: i64) -> i64 {
        (day - self.origin) / 7
    }
}

/// One year as one strip, a column a week. `opened` and `today` bound the days
/// filled, one before `opened` an outline; `day_link` opens each day with
/// reading, and `month_link` each month's name.
#[allow(clippy::too_many_arguments)]
pub fn strip(
    stats: &Stats,
    weeks: &Weeks,
    week: WeekStart,
    s: &Strings,
    look: &Look,
    today: i64,
    months: bool,
    day_link: &dyn Fn(i64) -> String,
    month_link: &dyn Fn(i64) -> String,
) -> String {
    let gap = 3.0;
    let cell = weeks.pitch - gap;
    let top = if months { 18.0 } else { 0.0 };
    let height = top + 7.0 * weeks.pitch;
    let width = weeks.left + weeks.columns as f32 * weeks.pitch;
    let opened = stats.opened(today);
    let peak = (weeks.first..=weeks.last)
        .map(|d| stats.day_seconds(d))
        .max()
        .unwrap_or(0);
    let mut svg = open(width, height);
    if months {
        for month in 1..=12 {
            let day = date::days_from_civil(date::civil_from_days(weeks.first).0, month, 1);
            let x = weeks.left + weeks.column(day) as f32 * weeks.pitch;
            let _ = write!(
                svg,
                "<a href=\"{}\"><text x=\"{x:.1}\" y=\"11\" font-size=\"11\" fill=\"{}\">{}</text></a>",
                month_link(day),
                hex(look.soft),
                esc(s.months_short[(month - 1) as usize])
            );
        }
    }
    for row in [0usize, 2, 4] {
        let weekday = week.day_in(row);
        let _ = write!(
            svg,
            "<text x=\"{:.1}\" y=\"{:.1}\" font-size=\"10\" text-anchor=\"end\" fill=\"{}\">{}</text>",
            weeks.left - 6.0,
            top + row as f32 * weeks.pitch + cell / 2.0 + 3.5,
            hex(look.faint),
            esc(s.weekdays_short[weekday])
        );
    }
    for day in weeks.first..=weeks.last.min(today) {
        let x = weeks.left + weeks.column(day) as f32 * weeks.pitch;
        let y = top + week.column_of(date::weekday(day)) as f32 * weeks.pitch;
        if day < opened {
            let _ = write!(
                svg,
                "<rect x=\"{:.1}\" y=\"{:.1}\" width=\"{:.1}\" height=\"{:.1}\" rx=\"2.5\" fill=\"none\" stroke=\"{}\"/>",
                x + 0.5,
                y + 0.5,
                cell - 1.0,
                cell - 1.0,
                hex(look.rule)
            );
            continue;
        }
        let secs = stats.day_seconds(day);
        let fill = look.level(charts::level(secs, peak));
        let ring = match day == today {
            true => format!(" stroke=\"{}\" stroke-width=\"1.5\"", hex(look.ink)),
            false => String::new(),
        };
        let tip = match secs {
            0 => date::long_day(day, s),
            _ => format!("{} · {}", date::long_day(day, s), date::duration(secs, s)),
        };
        let rect = format!(
            "<rect x=\"{x:.1}\" y=\"{y:.1}\" width=\"{cell:.1}\" height=\"{cell:.1}\" rx=\"2.5\" fill=\"{}\"{ring}><title>{}</title></rect>",
            hex(fill),
            esc(&tip)
        );
        // `secs` 0 takes no link.
        match secs > 0 {
            true => {
                let _ = write!(svg, "<a href=\"{}\">{rect}</a>", day_link(day));
            }
            false => svg.push_str(&rect),
        }
    }
    svg.push_str("</svg>");
    svg
}

/// The widest a bar draws: the Kindle's `Theme::row_h` at the window's 14 px body.
const ROW_H: f32 = 38.0;

/// The size a bar's figure and a cut's run are set at.
const FIGURE_PX: f32 = 11.0;

/// The least a figure shrinks to before [`thinned`] states fewer.
const FIGURE_FLOOR: f32 = 8.0;

/// How far one line of figures stands under the line over it.
const LINE: f32 = 14.0;

/// The air a figure keeps inside its bar's head, and twice what it keeps over one.
const INSET: f32 = 5.0;

/// The size an axis names its columns at.
const AXIS_PX: f32 = 10.0;

/// How wide `c` sets at `px`: a Han or kana character a full `px`, any other
/// six tenths of it.
fn char_wide(c: char, px: f32) -> f32 {
    match c as u32 {
        0x2E80.. => px,
        _ => px * 0.6,
    }
}

/// How wide `text` sets at `px`, by [`char_wide`].
fn wide(text: &str, px: f32) -> f32 {
    text.chars().map(|c| char_wide(c, px)).sum()
}

/// The air that stands between two cells of a row `count` columns across.
fn cell_gap(count: usize) -> f32 {
    match count > 16 {
        true => 2.0,
        false => 6.0,
    }
}

/// A row of columns as `charts::columns` lays one out: each cell's left edge and
/// width, the air between two, the cuts with the run each states, and the
/// `width` every figure and name stays inside.
struct Row<'a> {
    cells: &'a [(f32, f32)],
    gap: f32,
    breaks: &'a [usize],
    said: &'a [String],
    width: f32,
}

impl Row<'_> {
    fn holds(&self, at: usize) -> bool {
        self.breaks.contains(&at)
    }

    /// The length of the run break `at` stands for, where it states one.
    fn figure(&self, at: usize) -> Option<&String> {
        let nth = self.breaks.iter().position(|b| *b == at)?;
        self.said.get(nth).filter(|said| !said.is_empty())
    }

    /// The width a cell standing for no break has, which a bar and a figure
    /// are cut to.
    fn ordinary(&self) -> f32 {
        (0..self.cells.len())
            .find(|at| !self.holds(*at))
            .map_or(1.0, |at| self.cells[at].1)
    }

    /// How wide a bar stands: three quarters of its cell, never wider than a
    /// row.
    fn bar(&self) -> f32 {
        (self.ordinary() * 0.75).clamp(1.0, ROW_H)
    }

    /// Where the bar of column `at` opens, centred in its cell.
    fn opens(&self, at: usize) -> f32 {
        let (x, w) = self.cells[at];
        x + (w - self.bar()) / 2.0
    }

    /// The columns a break falls between, in the order they are named.
    fn flanks(&self) -> Vec<usize> {
        let last = self.cells.len().saturating_sub(1);
        let mut out = Vec::new();
        for at in self.breaks {
            out.extend(at.checked_sub(1));
            out.extend((*at < last).then_some(at + 1));
        }
        out.retain(|at| !self.holds(*at));
        out
    }
}

/// How a row states its figures: what each bar states, line by line, whether
/// each stands inside its own bar's head, the one size every figure is set at,
/// and the air the head of the band keeps for figures over the tallest bars.
struct Figures {
    said: Vec<Vec<String>>,
    inside: Vec<bool>,
    px: f32,
    head: f32,
}

/// Whether a bar `h` tall holds `lines` lines with [`INSET`] round them.
fn holds(h: f32, lines: usize) -> bool {
    lines > 0 && h >= LINE * lines as f32 + INSET * 2.0
}

/// Whether `air` over a bar's head holds `lines` lines standing there.
fn clears(air: f32, lines: usize) -> bool {
    lines > 0 && air >= LINE * lines as f32 + INSET / 2.0
}

/// How tall each of `values` stands in a plot `plot` high, `max` at the top.
fn heights(values: &[i64], max: i64, plot: f32) -> Vec<f32> {
    values
        .iter()
        .map(|value| plot * *value as f32 / max as f32)
        .collect()
}

/// Where a band `h` tall sets `rows`, as `charts::settle` does: inside each bar
/// tall enough and over the rest, at the one size all fit their [`rooms`] at;
/// else every figure over its bar under air kept at the head; else [`thinned`].
fn settle(row: &Row, values: &[i64], max: i64, h: f32, rows: &[Vec<String>]) -> Figures {
    let placed = |plot: f32| -> (Vec<Vec<String>>, Vec<bool>) {
        values
            .iter()
            .zip(rows)
            .map(|(value, said)| {
                let tall = plot * *value as f32 / max as f32;
                match holds(tall, said.len()) {
                    true => (said.clone(), true),
                    // `clears` answers for the air over the bar; with neither,
                    // `said` joins onto one line.
                    false => match clears(h - tall, said.len()) {
                        true => (said.clone(), false),
                        false => {
                            let one = match said.is_empty() {
                                true => Vec::new(),
                                false => vec![said.concat()],
                            };
                            let inside = holds(tall, one.len());
                            (one, inside)
                        }
                    },
                }
            })
            .unzip()
    };
    let (said, inside) = placed(h);
    let room = rooms(row, &said, &inside, &heights(values, max, h));
    let (px, fits) = sized(&said, &|at| room[at]);
    if fits {
        return Figures {
            said,
            inside,
            px,
            head: 0.0,
        };
    }
    let lines = rows.iter().map(Vec::len).max().unwrap_or(0) as f32;
    let head = LINE * lines + INSET / 2.0;
    let (flat, _) = placed(h - head);
    let over = vec![false; values.len()];
    let room = rooms(row, &flat, &over, &heights(values, max, h - head));
    let (px, fits) = sized(&flat, &|at| room[at]);
    if fits {
        return Figures {
            said: flat,
            inside: over,
            px,
            head,
        };
    }
    thinned(row, values, max, h, rows).unwrap_or(Figures {
        said: vec![Vec::new(); values.len()],
        inside: vec![false; values.len()],
        px,
        head: 0.0,
    })
}

/// The figures `charts::spread` spreads evenly, both ends always: `stating`
/// steps down until what is stated fits the room the skipped cells leave and
/// clears its own bar.
fn thinned(row: &Row, values: &[i64], max: i64, h: f32, rows: &[Vec<String>]) -> Option<Figures> {
    let bars = values.len();
    if bars < 2 {
        return None;
    }
    let lines = rows.iter().map(Vec::len).max().unwrap_or(0) as f32;
    let head = LINE * lines + INSET / 2.0;
    let tall = heights(values, max, h - head);
    for stating in (2..bars).rev() {
        let marks = charts::spread(bars, stating);
        let said: Vec<Vec<String>> = rows
            .iter()
            .enumerate()
            .map(|(at, lines)| match marks.contains(&at) {
                true => lines.clone(),
                false => Vec::new(),
            })
            .collect();
        // `even` spans the cells from one stated figure to the closest next.
        let step = marks
            .windows(2)
            .map(|pair| pair[1] - pair[0])
            .min()
            .unwrap_or(1);
        if step < 2 {
            continue;
        }
        let even = row.ordinary() * step as f32 - row.gap;
        let room = |at: usize| clear_of_bars(row, &tall, at).map_or(even, |clear| even.min(clear));
        let (px, fits) = sized(&said, &room);
        let stands =
            (0..bars).all(|at| said[at].is_empty() || clears(h - tall[at], said[at].len()));
        if fits && stands {
            return Some(Figures {
                said,
                inside: vec![false; bars],
                px,
                head,
            });
        }
    }
    None
}

/// The width each figure of `said` has: its bar's own where `inside`, a cut's
/// own column for its run, else the run of cells to the nearest figure either
/// side, short of any bar at `heights` standing taller than its foot.
fn rooms(row: &Row, said: &[Vec<String>], inside: &[bool], heights: &[f32]) -> Vec<f32> {
    let stated: Vec<usize> = (0..said.len()).filter(|at| !said[*at].is_empty()).collect();
    said.iter()
        .enumerate()
        .map(|(at, lines)| {
            if lines.is_empty() {
                return row.ordinary();
            }
            if row.holds(at) {
                return row.cells[at].1 - row.gap;
            }
            if inside[at] {
                return row.bar() - INSET / 2.0;
            }
            let before = stated.iter().rev().find(|o| **o < at).map(|o| at - o);
            let after = stated.iter().find(|o| **o > at).map(|o| o - at);
            let near = before.into_iter().chain(after).min().unwrap_or(said.len());
            let room = row.ordinary() * near as f32 - row.gap;
            clear_of_bars(row, heights, at).map_or(room, |clear| room.min(clear))
        })
        .collect()
}

/// The widest figure bar `at` carries over its head clear of every bar at
/// `heights` standing taller than the figure's foot, centred as [`bars`]
/// centres it; `None` where no bar beside it stands that tall.
fn clear_of_bars(row: &Row, heights: &[f32], at: usize) -> Option<f32> {
    let foot = heights[at] + INSET / 2.0;
    let left = (0..at).rev().find(|j| heights[*j] > foot);
    let right = (at + 1..heights.len()).find(|j| heights[*j] > foot);
    if left.is_none() && right.is_none() {
        return None;
    }
    let lo = left.map_or(0.0, |j| row.opens(j) + row.bar() + row.gap);
    let hi = right.map_or(row.width, |j| row.opens(j) - row.gap);
    let (x, w) = row.cells[at];
    let centre = x + w / 2.0;
    let (west, east) = (centre - lo, hi - centre);
    // `widest` spans `lo` to `hi` where the row's open side is the nearer.
    let widest = match (left, right) {
        (None, _) if west < east => hi - lo,
        (_, None) if east < west => hi - lo,
        _ => 2.0 * west.min(east),
    };
    Some(widest.max(0.0))
}

/// The size `said` is set at, as `chrome::shrink_to_fit` takes it down from
/// [`FIGURE_PX`] to no less than [`FIGURE_FLOOR`], and whether every line
/// lands inside the `room` its own bar has at that size.
fn sized(said: &[Vec<String>], room: &dyn Fn(usize) -> f32) -> (f32, bool) {
    let (lines, at): (Vec<&str>, Vec<usize>) = said
        .iter()
        .enumerate()
        .flat_map(|(bar, lines)| lines.iter().map(move |line| (line.as_str(), bar)))
        .unzip();
    let px = chrome::shrink_to_fit(
        FIGURE_PX,
        FIGURE_FLOOR,
        &lines,
        &|nth| room(at[nth]) as i32,
        |px, line| wide(line, px).ceil() as i32,
    );
    let fits = lines
        .iter()
        .zip(&at)
        .all(|(line, bar)| wide(line, px) <= room(*bar));
    (px, fits)
}

/// The ink a figure inside a bar of `fill` reads in: `look.card`, or `look.ink`
/// where that stands further off `fill`.
fn on(fill: [u8; 3], look: &Look) -> [u8; 3] {
    let luma = |rgb: [u8; 3]| {
        let [r, g, b] = rgb.map(|c| {
            let c = c as f32 / 255.0;
            match c <= 0.04045 {
                true => c / 12.92,
                false => ((c + 0.055) / 1.055).powf(2.4),
            }
        });
        0.2126 * r + 0.7152 * g + 0.0722 * b
    };
    let apart = |a: f32, b: f32| (a.max(b) + 0.05) / (a.min(b) + 0.05);
    let bar = luma(fill);
    match apart(luma(look.card), bar) >= apart(luma(look.ink), bar) {
        true => look.card,
        false => look.ink,
    }
}

/// Where a box `w` wide opens inside the cell at `x`, `cell` wide, clear by
/// `air` of every box in `taken` between `top` and `bottom`: centred where that
/// is clear, else flush with either edge, else nowhere.
fn clear_in(
    (x, cell): (f32, f32),
    w: f32,
    (top, bottom): (f32, f32),
    air: f32,
    taken: &[[f32; 4]],
) -> Option<f32> {
    if w > cell {
        return None;
    }
    let clear = |at: f32| {
        taken.iter().all(|[tx, ty, tw, th]| {
            at + w + air <= *tx
                || tx + tw + air <= at
                || bottom + air <= *ty
                || ty + th + air <= top
        })
    };
    [x + (cell - w) / 2.0, x, x + cell - w]
        .into_iter()
        .find(|at| clear(*at))
}

/// Draws `values` as `charts::columns` does: bars [`Row::bar`] wide on the foot
/// of a band `top` down and `h` tall, in `fill` with `tip` on hover, each figure
/// of `rows` where [`settle`] sets it, and each cut's run over its mark.
#[allow(clippy::too_many_arguments)]
fn bars(
    svg: &mut String,
    row: &Row,
    values: &[i64],
    rows: &[Vec<String>],
    top: f32,
    h: f32,
    fill: &dyn Fn(usize) -> [u8; 3],
    tip: &dyn Fn(usize) -> String,
    look: &Look,
) {
    let max = values.iter().copied().max().unwrap_or(1).max(1);
    // `row.figure` puts each cut's run among the figures `settle` spaces.
    let rows: Vec<Vec<String>> = rows
        .iter()
        .enumerate()
        .map(|(at, lines)| match row.figure(at) {
            Some(said) => vec![said.clone()],
            None => lines.clone(),
        })
        .collect();
    let set = settle(row, values, max, h, &rows);
    let (foot, plot, bar) = (top + h, h - set.head, row.bar());
    let (line, cap) = (set.px * LINE / FIGURE_PX, set.px * 0.72);
    let mut taken: Vec<[f32; 4]> = Vec::new();
    for (at, value) in values.iter().enumerate() {
        if row.holds(at) || *value <= 0 {
            continue;
        }
        let tall = plot * *value as f32 / max as f32;
        let (x, y) = (row.opens(at), foot - tall);
        let colour = fill(at);
        let _ = write!(
            svg,
            "<rect x=\"{x:.1}\" y=\"{y:.1}\" width=\"{bar:.1}\" height=\"{tall:.1}\" rx=\"1.5\" fill=\"{}\"><title>{}</title></rect>",
            hex(colour),
            esc(&tip(at))
        );
        let said = &set.said[at];
        if said.is_empty() {
            continue;
        }
        let first = match set.inside[at] {
            true => y + INSET + cap,
            false => y - INSET / 2.0 - line * (said.len() as f32 - 1.0),
        };
        let ink = match set.inside[at] {
            true => on(colour, look),
            false => look.ink,
        };
        let w = said.iter().map(|l| wide(l, set.px)).fold(0.0, f32::max);
        let centre = (x + bar / 2.0).clamp(w / 2.0, (row.width - w / 2.0).max(w / 2.0));
        for (nth, text) in said.iter().enumerate() {
            let _ = write!(
                svg,
                "<text x=\"{centre:.1}\" y=\"{:.1}\" font-size=\"{:.1}\" text-anchor=\"middle\" fill=\"{}\" pointer-events=\"none\">{}</text>",
                first + line * nth as f32,
                set.px,
                hex(ink),
                esc(text)
            );
        }
        taken.push([
            centre - w / 2.0,
            first - cap,
            w,
            cap + line * (said.len() as f32 - 1.0),
        ]);
    }
    for at in row.breaks {
        let Some(said) = row.figure(*at) else {
            continue;
        };
        let baseline = foot - LINE / 2.0 - INSET / 2.0;
        let (w, cap) = (wide(said, FIGURE_PX), FIGURE_PX * 0.72);
        let spot = (baseline - cap, baseline);
        let Some(x) = clear_in(row.cells[*at], w, spot, INSET / 2.0, &taken) else {
            continue;
        };
        let _ = write!(
            svg,
            "<text x=\"{:.1}\" y=\"{baseline:.1}\" font-size=\"{FIGURE_PX}\" text-anchor=\"middle\" fill=\"{}\">{}</text>",
            x + w / 2.0,
            hex(look.soft),
            esc(said)
        );
    }
}

/// Names the axis at baseline `y` as `charts::named` does: from the right, one
/// column in `every` and the last, each [`INSET`] clear of the next; a cut goes
/// unnamed, and the columns either side of it are named first.
fn names(
    svg: &mut String,
    row: &Row,
    every: usize,
    name: &dyn Fn(usize) -> String,
    y: f32,
    look: &Look,
) {
    let Some(last) = row.cells.len().checked_sub(1) else {
        return;
    };
    let (left, right) = (row.cells[0].0, row.cells[last].0 + row.cells[last].1);
    let fouls = |x: f32, w: f32, held: &[(f32, f32)]| {
        held.iter()
            .any(|(hx, hw)| x < hx + hw + INSET && *hx < x + w + INSET)
    };
    let mut held: Vec<(f32, f32)> = row.breaks.iter().map(|at| row.cells[*at]).collect();
    let mut flanking: Vec<(usize, f32, f32, String)> = Vec::new();
    for at in row.flanks() {
        let said = name(at);
        let w = wide(&said, AXIS_PX);
        // `room` stops a name wider than its own cell at the mark's own column.
        let mut room = (left, right);
        for cut in row.breaks {
            let (x, cell) = row.cells[*cut];
            if cut.checked_sub(1) == Some(at) {
                room.1 = room.1.min(x - INSET);
            }
            if cut + 1 == at {
                room.0 = room.0.max(x + cell + INSET);
            }
        }
        let (x, cell) = row.cells[at];
        let x = (x + (cell - w) / 2.0).clamp(room.0, (room.1 - w).max(room.0));
        let x = x.clamp(left, (right - w).max(left));
        if fouls(x, w, &held) {
            continue;
        }
        held.push((x, w));
        flanking.push((at, x, w, said));
    }
    let mut marks: Vec<usize> = (0..=last).step_by(every.max(1)).collect();
    if marks.last() != Some(&last) {
        marks.push(last);
    }
    let mut named: Vec<(f32, f32, String)> = Vec::new();
    let mut reach = f32::MAX;
    for at in marks.into_iter().rev() {
        if row.holds(at) || flanking.iter().any(|(flank, ..)| *flank == at) {
            continue;
        }
        let said = name(at);
        let w = wide(&said, AXIS_PX);
        let (x, cell) = row.cells[at];
        let x = (x + (cell - w) / 2.0).clamp(left, (right - w).max(left));
        if fouls(x, w, &held)
            || named.last().is_some_and(|(_, _, last)| *last == said)
            || x + w + INSET > reach
        {
            continue;
        }
        reach = x;
        named.push((x, w, said));
    }
    let flanked = flanking.into_iter().map(|(_, x, w, said)| (x, w, said));
    for (x, w, said) in named.into_iter().chain(flanked) {
        let _ = write!(
            svg,
            "<text x=\"{:.1}\" y=\"{y:.1}\" font-size=\"{AXIS_PX}\" text-anchor=\"middle\" fill=\"{}\">{}</text>",
            x + w / 2.0,
            hex(look.faint),
            esc(&said)
        );
    }
}

/// Draws `fold` as [`columns`]: each bar states its time as
/// `alltime::duration_rows` sets it, and the busiest stands in the mark.
pub fn fold(
    fold: &Fold,
    axis: &[String],
    every: usize,
    s: &Strings,
    look: &Look,
    width: f32,
) -> String {
    columns(
        &fold.values,
        axis,
        every,
        fold.busiest,
        &|secs| alltime::duration_rows(secs, s),
        &|secs| date::duration(secs, s),
        look,
        width,
        120.0,
    )
}

/// What a fold's heading states beside its name, as the Kindle's
/// `alltime::band` heads one: `each`, then `s.most` and the bucket of `axis`
/// that is fullest, where one is.
pub fn stated(fold: &Fold, axis: &[String], each: String, s: &Strings) -> String {
    match fold.busiest {
        Some(at) => format!("{each} · {} {}", s.most, axis[at]),
        None => each,
    }
}

/// Draws `values` as [`bars`] `width` wide and at most `plot` tall, `busiest` in
/// the mark and the rest in `look.bar`, each stating `figure` of its value and
/// `said` on hover, over [`names`] for every `every`th of `axis`.
#[allow(clippy::too_many_arguments)]
pub fn columns(
    values: &[i64],
    axis: &[String],
    every: usize,
    busiest: Option<usize>,
    figure: &dyn Fn(i64) -> Vec<String>,
    said: &dyn Fn(i64) -> String,
    look: &Look,
    width: f32,
    plot: f32,
) -> String {
    let foot = 18.0;
    let gap = cell_gap(values.len());
    let cells = even_cells(values.len(), gap, width);
    let row = Row {
        cells: &cells,
        gap,
        breaks: &[],
        said: &[],
        width,
    };
    let rows: Vec<Vec<String>> = values.iter().map(|value| figure(*value)).collect();
    let mut svg = open(width, plot + foot);
    bars(
        &mut svg,
        &row,
        values,
        &rows,
        0.0,
        plot,
        &|at| match busiest == Some(at) {
            true => look.mark,
            false => look.bar,
        },
        &|at| format!("{} · {}", axis[at], said(values[at])),
        look,
    );
    let _ = write!(
        svg,
        "<line x1=\"0\" y1=\"{:.1}\" x2=\"{width:.1}\" y2=\"{:.1}\" stroke=\"{}\"/>",
        plot + 0.5,
        plot + 0.5,
        hex(look.rule)
    );
    names(
        &mut svg,
        &row,
        every,
        &|at| axis[at].clone(),
        plot + foot - 4.0,
        look,
    );
    svg.push_str("</svg>");
    svg
}

/// `count` cells, each as wide as the rest, that fill `width` with `gap`
/// between each two.
fn even_cells(count: usize, gap: f32, width: f32) -> Vec<(f32, f32)> {
    let cell = (width - gap * (count as f32 - 1.0)) / count.max(1) as f32;
    (0..count)
        .map(|at| (at as f32 * (cell + gap), cell))
        .collect()
}

/// One reading of a book, `from` to `to`, cut by `book_graphs::cut_to` and laid
/// across `width` as the Kindle's `Strip` lays it: a cut stating its run as wide
/// as `book_graphs::labelled` makes it, every other column one width.
pub struct BookStrip {
    pub from: i64,
    pub to: i64,
    pub values: Vec<i64>,
    pub edges: Vec<i64>,
    pub breaks: Vec<usize>,
    /// What each break states, empty where `book_graphs::labelled` states nothing.
    pub said: Vec<String>,
    pub grain: book_graphs::Grain,
    pub year: bool,
    pub cells: Vec<(f32, f32)>,
    pub gap: f32,
    pub width: f32,
}

impl BookStrip {
    pub fn of(
        stats: &Stats,
        at: usize,
        (from, to): (i64, i64),
        week: WeekStart,
        s: &Strings,
        width: f32,
    ) -> Self {
        let most = ((width / 4.0) as usize).max(2);
        let (values, edges, breaks, ran, grain) =
            book_graphs::cut_to(&stats.book_days(at), from, to, most, week);
        let count = values.len().max(1);
        let gap = cell_gap(count);
        let room = (width - gap * (count as f32 - 1.0)).max(1.0);
        let said: Vec<String> = ran
            .iter()
            .map(|run| s.days_plain.replace("{d}", &run.to_string()))
            .collect();
        let widths: Vec<i32> = said
            .iter()
            .map(|said| wide(said, FIGURE_PX).ceil() as i32)
            .collect();
        let (stated, share) = book_graphs::labelled(&widths, count, room as i32, INSET as i32);
        let said: Vec<String> = said
            .into_iter()
            .zip(stated)
            .map(|(said, on)| match on {
                true => said,
                false => String::new(),
            })
            .collect();
        // `weight` gives a cut that states its run `share` of an ordinary column.
        let weight = |column: usize| match breaks.iter().position(|b| *b == column) {
            Some(nth) if !said[nth].is_empty() => share,
            _ => 1.0,
        };
        let whole: f32 = (0..count).map(weight).sum();
        let mut cells = Vec::with_capacity(count);
        let mut x = 0.0;
        for column in 0..count {
            let w = room * weight(column) / whole;
            cells.push((x, w));
            x += w + gap;
        }
        Self {
            from,
            to,
            year: date::civil_from_days(from).0 != date::civil_from_days(to).0,
            said,
            values,
            edges,
            breaks,
            grain,
            cells,
            gap,
            width,
        }
    }

    fn name(&self, column: usize, s: &Strings) -> String {
        self.grain.name(self.edges[column], self.year, s)
    }

    fn row(&self) -> Row<'_> {
        Row {
            cells: &self.cells,
            gap: self.gap,
            breaks: &self.breaks,
            said: &self.said,
            width: self.width,
        }
    }

    /// Draws the axis at `axis`: two slashes over each cut, and one column in
    /// a quarter of them named by [`names`].
    fn axis(&self, svg: &mut String, axis: f32, s: &Strings, look: &Look) {
        for column in &self.breaks {
            let (x, w) = self.cells[*column];
            let mid = x + w / 2.0;
            let _ = write!(
                svg,
                "<rect x=\"{:.1}\" y=\"{:.1}\" width=\"{w:.1}\" height=\"4\" fill=\"{}\"/>\
                 <path d=\"M{:.1},{:.1} l4,-9 M{:.1},{:.1} l4,-9\" stroke=\"{}\" stroke-width=\"1.5\"/>",
                x,
                axis - 2.0,
                hex(look.card),
                mid - 5.0,
                axis + 4.5,
                mid + 1.0,
                axis + 4.5,
                hex(look.soft)
            );
        }
        let every = (self.values.len() / 4).max(1);
        names(
            svg,
            &self.row(),
            every,
            &|at| self.name(at, s),
            axis + 16.0,
            look,
        );
    }
}

/// Draws the seconds of each column of the reading as the Kindle's Graphs tab
/// draws the days: [`bars`] in the book's `tone` that state their time as
/// `alltime::duration_rows` sets it, over the strip's own axis.
pub fn book_days(strip: &BookStrip, s: &Strings, look: &Look, tone: [u8; 3]) -> String {
    let (band, foot) = (150.0, 24.0);
    let mut svg = open(strip.width, band + foot);
    let rows: Vec<Vec<String>> = strip
        .values
        .iter()
        .map(|secs| alltime::duration_rows(*secs, s))
        .collect();
    bars(
        &mut svg,
        &strip.row(),
        &strip.values,
        &rows,
        0.0,
        band,
        &|_| tone,
        &|at| {
            format!(
                "{} · {}",
                strip.name(at, s),
                date::duration(strip.values[at], s)
            )
        },
        look,
    );
    let base = band + 0.5;
    let _ = write!(
        svg,
        "<line x1=\"0\" y1=\"{base:.1}\" x2=\"{:.1}\" y2=\"{base:.1}\" stroke=\"{}\"/>",
        strip.width,
        hex(look.rule)
    );
    strip.axis(&mut svg, base, s, look);
    svg.push_str("</svg>");
    svg
}

/// `book_graphs::placed` on the days' own columns, 0 % at the foot: each
/// stretch a bar, a sitting stating none a mark at its progress, a column's
/// sittings side by side, a tick under each.
pub fn book_sat(
    stats: &Stats,
    at: usize,
    strip: &BookStrip,
    s: &Strings,
    look: &Look,
    tone: [u8; 3],
) -> String {
    let (top, plot, foot) = (16.0, 140.0, 24.0);
    let height = top + plot + foot;
    let (side, air) = (7.0f32, 3.0f32);
    let y = |share: i64| top + plot - plot * share.clamp(0, 100) as f32 / 100.0;
    let mut svg = open(strip.width, height);
    for share in [0, 25, 50, 75, 100] {
        let _ = write!(
            svg,
            "<line x1=\"0\" y1=\"{:.1}\" x2=\"{:.1}\" y2=\"{:.1}\" stroke=\"{}\"/>",
            y(share) + 0.5,
            strip.width,
            y(share) + 0.5,
            hex(look.rule)
        );
    }
    // 0, 50 and 100 named at the closing end, over their lines, as `charts::scatter` names them.
    for share in [0, 50, 100] {
        let _ = write!(
            svg,
            "<text x=\"{:.1}\" y=\"{:.1}\" font-size=\"10\" text-anchor=\"end\" fill=\"{}\">{}</text>",
            strip.width,
            y(share) - 3.0,
            hex(look.faint),
            esc(&s.percent_plain.replace("{d}", &share.to_string()))
        );
    }
    let placed = book_graphs::placed(stats, at, &strip.edges, strip.from, strip.to);
    let days: Vec<i64> = stats
        .book_progress(at)
        .into_iter()
        .map(|(day, _)| day)
        .filter(|day| (strip.from..=strip.to).contains(day))
        .collect();
    let mut from = 0;
    while from < placed.len() {
        let column = placed[from].column;
        let to = placed[from..]
            .iter()
            .position(|p| p.column != column)
            .map_or(placed.len(), |n| from + n);
        let held = &placed[from..to];
        let (cx, cw) = strip.cells[column];
        let deep = held.len() as f32;
        let (step, wide, left) = match deep > cw {
            true => (0.0, side, cx + (cw - side) / 2.0),
            false => {
                let step = (side + air).min(cw / deep);
                (step, step.min(side).max(1.0), cx + (cw - step * deep) / 2.0)
            }
        };
        for (nth, sitting) in held.iter().enumerate() {
            let x = left + step * nth as f32;
            let day = date::short_day(days.get(from + nth).copied().unwrap_or(strip.to), s);
            let _ = write!(
                svg,
                "<rect x=\"{x:.1}\" y=\"{:.1}\" width=\"{wide:.1}\" height=\"1.5\" fill=\"{}\"/>",
                top + plot + air,
                hex(tone)
            );
            let mark = |svg: &mut String, share: i64, tip: &str| {
                let _ = write!(
                    svg,
                    "<rect x=\"{x:.1}\" y=\"{:.1}\" width=\"{wide:.1}\" height=\"{side}\" fill=\"{}\"><title>{}</title></rect>",
                    y(share) - side / 2.0,
                    hex(tone),
                    esc(tip)
                );
            };
            if sitting.stretches.is_empty() {
                let tip = format!(
                    "{day} · {}",
                    s.percent_plain
                        .replace("{d}", &sitting.progress.to_string())
                );
                mark(&mut svg, sitting.progress, &tip);
                continue;
            }
            for (opened, closed) in &sitting.stretches {
                let tip = format!("{day} · {opened}% – {closed}%");
                let (a, b) = (y(*opened), y(*closed));
                let (high, tall) = (a.min(b), (a - b).abs());
                match tall < side {
                    true => mark(&mut svg, *closed, &tip),
                    false => {
                        let _ = write!(
                            svg,
                            "<rect x=\"{x:.1}\" y=\"{high:.1}\" width=\"{wide:.1}\" height=\"{tall:.1}\" fill=\"{}\"><title>{}</title></rect>",
                            hex(tone),
                            esc(&tip)
                        );
                    }
                }
            }
        }
        from = to;
    }
    strip.axis(&mut svg, top + plot + 0.5, s, look);
    svg.push_str("</svg>");
    svg
}

/// `secs` into a day as `HH:MM`.
fn clock(secs: i64) -> String {
    format!("{:02}:{:02}", secs / 3600, secs % 3600 / 60)
}

/// Where `secs` into a day stands across `width`.
fn hour_x(secs: i64, width: f32) -> f32 {
    width * secs.clamp(0, 86_400) as f32 / 86_400.0
}

/// One day's reading along its twenty-four hours, as `charts::timeline` draws
/// it: a block for each stretch `Stats::day_blocks` holds, the hours named
/// every six, each block's times its `<title>`.
pub fn timeline(blocks: &[(i64, i64)], look: &Look, width: f32, s: &Strings) -> String {
    let (strip, axis) = (34.0, 18.0);
    let mut svg = open(width, strip + axis);
    let _ = write!(
        svg,
        "<rect x=\"0.5\" y=\"0.5\" width=\"{:.1}\" height=\"{:.1}\" rx=\"4\" fill=\"none\" stroke=\"{}\"/>",
        width - 1.0,
        strip - 1.0,
        hex(look.rule)
    );
    for hour in (0..=24).step_by(6) {
        let x = hour_x(hour * 3600, width);
        let anchor = match hour {
            0 => "start",
            24 => "end",
            _ => "middle",
        };
        let _ = write!(
            svg,
            "<line x1=\"{x:.1}\" y1=\"1\" x2=\"{x:.1}\" y2=\"{:.1}\" stroke=\"{}\"/>\
             <text x=\"{x:.1}\" y=\"{:.1}\" font-size=\"10\" text-anchor=\"{anchor}\" fill=\"{}\">{hour:02}</text>",
            strip - 1.0,
            hex(look.edge),
            strip + 13.0,
            hex(look.faint)
        );
    }
    for (from, to) in blocks {
        let (x0, x1) = (hour_x(*from, width), hour_x(*to, width));
        let _ = write!(
            svg,
            "<rect x=\"{x0:.1}\" y=\"3\" width=\"{:.1}\" height=\"{:.1}\" rx=\"2\" fill=\"{}\"><title>{}–{} · {}</title></rect>",
            (x1 - x0).max(3.0),
            strip - 6.0,
            hex(look.accent),
            clock(*from),
            clock(*to),
            esc(&date::duration(to - from, s))
        );
    }
    svg.push_str("</svg>");
    svg
}

/// One book's stretches of a day across the same twenty-four hours as
/// [`timeline`], in the book's `tone`, over a track the width of the day.
pub fn spans(blocks: &[(i64, i64)], look: &Look, tone: [u8; 3], width: f32) -> String {
    let mut svg = open(width, 8.0);
    let _ = write!(
        svg,
        "<rect x=\"0\" y=\"3\" width=\"{width:.1}\" height=\"2\" rx=\"1\" fill=\"{}\"/>",
        hex(look.l0)
    );
    for (from, to) in blocks {
        let (x0, x1) = (hour_x(*from, width), hour_x(*to, width));
        let _ = write!(
            svg,
            "<rect x=\"{x0:.1}\" y=\"0\" width=\"{:.1}\" height=\"8\" rx=\"2\" fill=\"{}\"><title>{}–{}</title></rect>",
            (x1 - x0).max(3.0),
            hex(tone),
            clock(*from),
            clock(*to)
        );
    }
    svg.push_str("</svg>");
    svg
}

/// The week opening on `first`, as `rhythm::week_columns` draws it: each day's
/// name over a bar against the busiest day, its figure over it and its hours
/// cut into its foot; `link` opens each day with reading.
#[allow(clippy::too_many_arguments)]
pub fn week(
    stats: &Stats,
    first: i64,
    today: i64,
    look: &Look,
    s: &Strings,
    width: f32,
    link: &dyn Fn(i64) -> String,
) -> String {
    let (head, plot, gap) = (24.0, 170.0, 10.0);
    let height = head + plot + 4.0;
    let column = (width - gap * 6.0) / 7.0;
    let most = stats
        .busiest_day(first..=first + 6)
        .map_or(0, |(_, secs)| secs)
        .max(1);
    let mut svg = open(width, height);
    for at in 0..7 {
        let day = first + at;
        let x = at as f32 * (column + gap);
        let name = readinglog_native::view::rhythm::day_name(day, s);
        // `past` days hold no data: `name` stands faint over no line.
        let past = day > today;
        let mut drawn = format!(
            "<text x=\"{:.1}\" y=\"14\" font-size=\"12\" text-anchor=\"middle\" fill=\"{}\"{}>{}</text>",
            x + column / 2.0,
            hex(if past { look.faint } else { look.ink }),
            if day == today {
                " font-weight=\"700\""
            } else {
                ""
            },
            esc(&name)
        );
        let base = head + plot;
        if !past {
            let _ = write!(
                drawn,
                "<line x1=\"{x:.1}\" y1=\"{base:.1}\" x2=\"{:.1}\" y2=\"{base:.1}\" stroke=\"{}\"/>",
                x + column,
                hex(look.rule)
            );
        }
        let secs = stats.day_seconds(day);
        if secs > 0 {
            let label_h = 16.0;
            let h = ((plot - label_h) * secs as f32 / most as f32).max(2.0);
            let bw = column * 7.0 / 8.0;
            let (bx, by) = (x + (column - bw) / 2.0, base - h);
            let _ = write!(
                drawn,
                "<rect x=\"{bx:.1}\" y=\"{by:.1}\" width=\"{bw:.1}\" height=\"{h:.1}\" rx=\"2\" fill=\"{}\"/>",
                hex(look.bar)
            );
            // `hours` cut into the bar's foot in `look.card`.
            let hours = stats.hours_over(day..=day);
            let busiest = hours.iter().copied().max().unwrap_or(0).max(1);
            let pad = 3.0;
            let step = ((bw - pad * 2.0) / 24.0).floor().max(1.0);
            let box_w = step * 24.0;
            let box_h = (h / 3.0).min(28.0) - pad;
            let left = bx + (bw - box_w) / 2.0;
            for (hour, at_hour) in hours.iter().enumerate() {
                if *at_hour <= 0 || box_h <= 1.0 {
                    continue;
                }
                let hh = (box_h * *at_hour as f32 / busiest as f32).max(2.0);
                let _ = write!(
                    drawn,
                    "<rect x=\"{:.1}\" y=\"{:.1}\" width=\"{step:.1}\" height=\"{hh:.1}\" fill=\"{}\"/>",
                    left + hour as f32 * step,
                    base - pad - hh,
                    hex(look.card)
                );
            }
            let _ = write!(
                drawn,
                "<text x=\"{:.1}\" y=\"{:.1}\" font-size=\"12\" font-weight=\"600\" text-anchor=\"middle\" fill=\"{}\">{}</text>",
                x + column / 2.0,
                by - 5.0,
                hex(look.ink),
                esc(&date::duration_tight(secs, s))
            );
            let _ = write!(
                svg,
                "<a href=\"{}\"><rect x=\"{x:.1}\" y=\"0\" width=\"{column:.1}\" height=\"{height:.1}\" fill=\"{}\" fill-opacity=\"0\"><title>{} · {}</title></rect>{drawn}</a>",
                link(day),
                hex(look.card),
                esc(&date::long_day(day, s)),
                esc(&date::duration(secs, s))
            );
        } else {
            svg.push_str(&drawn);
        }
    }
    svg.push_str("</svg>");
    svg
}

/// How many books a day of a month names in lanes, the rest counted in `+n`.
const LANES: usize = 4;

/// `text` as it stands inside `room` at `px`, which [`char_wide`] measures:
/// cut, and closed by `…`, where it runs longer.
fn clip(text: &str, room: f32, px: f32) -> String {
    if wide(text, px) <= room {
        return text.to_string();
    }
    let mut out = String::new();
    let mut taken = char_wide('…', px);
    for c in text.chars() {
        taken += char_wide(c, px);
        if taken > room {
            break;
        }
        out.push(c);
    }
    out.push('…');
    out
}

/// `tone` taken most of the way to white, the ground a lane's title is set on.
fn wash([r, g, b]: [u8; 3]) -> [u8; 3] {
    let lift = |c: u8| (c as f32 + (255.0 - c as f32) * 0.72).round() as u8;
    [lift(r), lift(g), lift(b)]
}

/// The month opening on `first`, as `rhythm::month_grid` draws it: each cell
/// its date, total and level, its books in lanes off `charts::lanes` and its
/// hours along its foot; `link` opens each day with reading.
#[allow(clippy::too_many_arguments)]
pub fn month(
    stats: &Stats,
    first: i64,
    week: WeekStart,
    today: i64,
    look: &Look,
    s: &Strings,
    tones: &[[u8; 3]],
    width: f32,
    link: &dyn Fn(i64) -> String,
) -> String {
    let (year, month, _) = date::civil_from_days(first);
    let last = first + date::days_in_month(year, month) - 1;
    let (head, gap, cell_h) = (22.0, 6.0, 112.0);
    let rows = charts::month_rows(year, month, week) as usize;
    let cell_w = (width - gap * 6.0) / 7.0;
    let height = head + rows as f32 * (cell_h + gap);
    let peak = stats.busiest_day(first..=last).map_or(0, |(_, secs)| secs);
    let tallest = (first..=last)
        .filter_map(|day| stats.hours_over(day..=day).into_iter().max())
        .max()
        .unwrap_or(0)
        .max(1);
    let lead = week.column_of(date::weekday(first)) as i64;
    let mut svg = open(width, height);
    for column in 0..7 {
        let _ = write!(
            svg,
            "<text x=\"{:.1}\" y=\"14\" font-size=\"11\" text-anchor=\"middle\" fill=\"{}\">{}</text>",
            column as f32 * (cell_w + gap) + cell_w / 2.0,
            hex(look.faint),
            esc(s.weekdays_short[week.day_in(column)])
        );
    }
    let (lane_top, lane_h) = (30.0, 17.0);
    for row in 0..rows {
        let days: Vec<Option<i64>> = (0..7)
            .map(|column| {
                let day = first + (row * 7 + column) as i64 - lead;
                (first..=last).contains(&day).then_some(day)
            })
            .collect();
        let books: Vec<Vec<usize>> = days
            .iter()
            .map(|day| match day {
                Some(day) => stats
                    .book_totals(*day..=*day)
                    .into_iter()
                    .map(|(b, _)| b)
                    .collect(),
                None => Vec::new(),
            })
            .collect();
        let lanes = charts::lanes(&books, LANES);
        let y = head + row as f32 * (cell_h + gap);
        for (column, day) in days.iter().enumerate() {
            let Some(day) = day else { continue };
            let x = column as f32 * (cell_w + gap);
            let secs = stats.day_seconds(*day);
            let mut cell = format!(
                "<rect x=\"{:.1}\" y=\"{:.1}\" width=\"{:.1}\" height=\"{:.1}\" rx=\"5\" fill=\"{}\" stroke=\"{}\"{}/>",
                x + 0.5,
                y + 0.5,
                cell_w - 1.0,
                cell_h - 1.0,
                hex(look.card),
                hex(match *day == today {
                    true => look.ink,
                    false => look.rule,
                }),
                if *day == today {
                    " stroke-width=\"1.5\""
                } else {
                    ""
                }
            );
            let level = charts::level(secs, peak);
            if level > 0 {
                let _ = write!(
                    cell,
                    "<rect x=\"{:.1}\" y=\"{:.1}\" width=\"{:.1}\" height=\"5\" rx=\"2\" fill=\"{}\"/>",
                    x + 1.0,
                    y + 1.0,
                    cell_w - 2.0,
                    hex(look.level(level))
                );
            }
            let (_, _, dom) = date::civil_from_days(*day);
            let _ = write!(
                cell,
                "<text x=\"{:.1}\" y=\"{:.1}\" font-size=\"12\" font-weight=\"600\" fill=\"{}\">{dom}</text>",
                x + 7.0,
                y + 21.0,
                hex(look.ink)
            );
            if secs > 0 {
                let _ = write!(
                    cell,
                    "<text x=\"{:.1}\" y=\"{:.1}\" font-size=\"11\" text-anchor=\"end\" fill=\"{}\">{}</text>",
                    x + cell_w - 7.0,
                    y + 21.0,
                    hex(look.soft),
                    esc(&date::duration_tight(secs, s))
                );
                let hours = stats.hours_over(*day..=*day);
                let step = ((cell_w - 14.0) / 24.0).max(1.0);
                let foot = y + cell_h - 6.0;
                for (hour, at) in hours.iter().enumerate() {
                    if *at <= 0 {
                        continue;
                    }
                    let h = (10.0 * *at as f32 / tallest as f32).max(1.5);
                    let _ = write!(
                        cell,
                        "<rect x=\"{:.1}\" y=\"{:.1}\" width=\"{step:.1}\" height=\"{h:.1}\" fill=\"{}\"/>",
                        x + 7.0 + hour as f32 * step,
                        foot - h,
                        hex(look.bar)
                    );
                }
                let _ = write!(
                    svg,
                    "<a href=\"{}\">{cell}<rect x=\"{x:.1}\" y=\"{y:.1}\" width=\"{cell_w:.1}\" height=\"{cell_h:.1}\" fill=\"{}\" fill-opacity=\"0\"><title>{} · {}</title></rect></a>",
                    link(*day),
                    hex(look.card),
                    esc(&date::long_day(*day, s)),
                    esc(&date::duration(secs, s))
                );
            } else {
                svg.push_str(&cell);
            }
        }
        // `lanes` runs reach past their own cell, each bar drawn over the cells.
        for (column, here) in lanes.iter().enumerate() {
            let x = column as f32 * (cell_w + gap) + 6.0;
            for (lane, run) in here.iter().enumerate() {
                let Some(run) = run else { continue };
                if run.start != column {
                    continue;
                }
                let w = cell_w - 12.0 + (cell_w + gap) * (run.span as f32 - 1.0);
                let ly = y + lane_top + lane as f32 * lane_h;
                let book = &stats.books[run.book];
                let named = match super::language_of(&book.language, &book.title) {
                    language if language.is_empty() => String::new(),
                    language => format!(" xml:lang=\"{}\"", esc(&language)),
                };
                let _ = write!(
                    svg,
                    "<rect x=\"{x:.1}\" y=\"{ly:.1}\" width=\"{w:.1}\" height=\"{:.1}\" rx=\"3\" fill=\"{}\"><title>{}</title></rect>\
                     <text x=\"{:.1}\" y=\"{:.1}\" font-size=\"10.5\" fill=\"{}\" pointer-events=\"none\"{named}>{}</text>",
                    lane_h - 3.0,
                    hex(wash(tones[run.book])),
                    esc(&book.title),
                    x + 4.0,
                    ly + 10.5,
                    hex(look.ink),
                    esc(&clip(&book.title, w - 8.0, 10.5))
                );
            }
            let named = here.iter().flatten().count();
            let over = books[column].len().saturating_sub(named);
            if over > 0 {
                let ly = y + lane_top + named.min(LANES - 1) as f32 * lane_h;
                let _ = write!(
                    svg,
                    "<text x=\"{:.1}\" y=\"{:.1}\" font-size=\"10.5\" text-anchor=\"end\" fill=\"{}\" pointer-events=\"none\">+{over}</text>",
                    x + cell_w - 12.0,
                    ly + 10.5 + if named >= LANES { lane_h } else { 0.0 },
                    hex(look.soft)
                );
            }
        }
    }
    svg.push_str("</svg>");
    svg
}
