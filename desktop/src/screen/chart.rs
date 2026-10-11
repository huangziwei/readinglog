//! The charts, as SVG drawn off `Stats` series. Every colour is a literal off
//! [`Look`]; a day's step is `charts::level`, and a book's two bands stand on
//! the columns `book_graphs::cut_to` cuts.

use std::fmt::Write as _;

use readinglog_native::date;
use readinglog_native::lang::Strings;
use readinglog_native::settings::WeekStart;
use readinglog_native::stats::{Fold, Stats};
use readinglog_native::ui::charts;
use readinglog_native::view::book_graphs;

use super::esc;
use super::look::{Look, hex};

fn open(width: f32, height: f32) -> String {
    format!(
        "<svg xmlns=\"http://www.w3.org/2000/svg\" viewBox=\"0 0 {width} {height}\" \
         width=\"{width}\" height=\"{height}\" font-family=\"-apple-system, system-ui, Helvetica Neue, Hiragino Sans, sans-serif\">"
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

    /// The days of column `at` inside the year.
    pub fn days(&self, at: i64) -> std::ops::RangeInclusive<i64> {
        let from = (self.origin + at * 7).max(self.first);
        let to = (self.origin + at * 7 + 6).min(self.last);
        from..=to
    }
}

/// One year as one strip, 53 or 54 weeks across and seven days down. `opened`
/// and `today` bound the days filled: one before `opened` an outline, one past
/// `today` not drawn.
pub fn strip(
    stats: &Stats,
    weeks: &Weeks,
    week: WeekStart,
    s: &Strings,
    look: &Look,
    today: i64,
    months: bool,
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
                "<text x=\"{x:.1}\" y=\"11\" font-size=\"11\" fill=\"{}\">{}</text>",
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
        let _ = write!(
            svg,
            "<rect x=\"{x:.1}\" y=\"{y:.1}\" width=\"{cell:.1}\" height=\"{cell:.1}\" rx=\"2.5\" fill=\"{}\"{ring}><title>{}</title></rect>",
            hex(fill),
            esc(&tip)
        );
    }
    svg.push_str("</svg>");
    svg
}

/// Seconds each week of the year, each bar cut into the books read in it in
/// the colour of each book, the unidentified share in grey at the top.
pub fn weeks_by_book(
    stats: &Stats,
    weeks: &Weeks,
    s: &Strings,
    look: &Look,
    tones: &[[u8; 3]],
) -> String {
    let plot = 132.0;
    let top = 8.0;
    let foot = 4.0;
    let width = weeks.left + weeks.columns as f32 * weeks.pitch;
    let height = top + plot + foot;
    let totals: Vec<i64> = (0..weeks.columns)
        .map(|at| stats.span_seconds(weeks.days(at)))
        .collect();
    let max = totals.iter().copied().max().unwrap_or(0).max(1);
    let step = nice_hours(max);
    let mut svg = open(width, height);
    let mut line = step;
    while line <= max {
        let y = top + plot - plot * line as f32 / max as f32;
        let _ = write!(
            svg,
            "<line x1=\"{:.1}\" y1=\"{y:.1}\" x2=\"{width:.1}\" y2=\"{y:.1}\" stroke=\"{}\" stroke-dasharray=\"2 3\"/>\
             <text x=\"{:.1}\" y=\"{:.1}\" font-size=\"10\" text-anchor=\"end\" fill=\"{}\">{}</text>",
            weeks.left,
            hex(look.rule),
            weeks.left - 6.0,
            y + 3.5,
            hex(look.faint),
            esc(&date::duration(line, s))
        );
        line += step;
    }
    // `order` keeps each book's place in every bar: the day it was first read.
    let order = |book: usize| stats.books[book].first_day;
    for at in 0..weeks.columns {
        let days = weeks.days(at);
        let mut books = stats.book_totals(days.clone());
        books.sort_by_key(|(book, _)| (order(*book), *book));
        let x = weeks.left + at as f32 * weeks.pitch + 1.5;
        let w = weeks.pitch - 3.0;
        let mut y = top + plot;
        let total = totals[at as usize];
        let named: i64 = books.iter().map(|(_, secs)| secs).sum();
        let mut parts: Vec<(Option<usize>, i64)> =
            books.into_iter().map(|(b, secs)| (Some(b), secs)).collect();
        if total > named {
            parts.push((None, total - named));
        }
        for (book, secs) in parts {
            let h = plot * secs as f32 / max as f32;
            y -= h;
            let (fill, name) = match book {
                Some(book) => (tones[book], stats.books[book].title.clone()),
                None => (look.faint, s.unidentified.to_string()),
            };
            let _ = write!(
                svg,
                "<rect x=\"{x:.1}\" y=\"{y:.2}\" width=\"{w:.1}\" height=\"{h:.2}\" fill=\"{}\"><title>{} · {}</title></rect>",
                hex(fill),
                esc(&name),
                esc(&date::duration(secs, s))
            );
        }
    }
    let base = top + plot + 0.5;
    let _ = write!(
        svg,
        "<line x1=\"{:.1}\" y1=\"{base:.1}\" x2=\"{width:.1}\" y2=\"{base:.1}\" stroke=\"{}\"/></svg>",
        weeks.left,
        hex(look.rule)
    );
    svg
}

/// The step between the hour lines under `max` seconds: one, two, five or ten
/// hours, whichever draws two to five lines.
fn nice_hours(max: i64) -> i64 {
    for hours in [1, 2, 5, 10, 20, 50] {
        if max / (hours * 3600) <= 4 {
            return hours * 3600;
        }
    }
    100 * 3600
}

/// One fold as bars, the busiest in the mark, every `every`th bucket named,
/// each `<title>` holding its bar's figure.
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
        &|v| date::duration(v, s),
        look,
        width,
        120.0,
    )
}

#[allow(clippy::too_many_arguments)]
pub fn columns(
    values: &[i64],
    axis: &[String],
    every: usize,
    busiest: Option<usize>,
    said: &dyn Fn(i64) -> String,
    look: &Look,
    width: f32,
    plot: f32,
) -> String {
    let foot = 18.0;
    let height = plot + foot;
    let n = values.len().max(1) as f32;
    let pitch = width / n;
    let bar = (pitch * 0.68).max(2.0);
    let max = values.iter().copied().max().unwrap_or(1).max(1);
    let mut svg = open(width, height);
    for (at, value) in values.iter().enumerate() {
        let h = (plot - 2.0) * *value as f32 / max as f32;
        let x = at as f32 * pitch + (pitch - bar) / 2.0;
        let fill = match busiest == Some(at) {
            true => look.mark,
            false => look.bar,
        };
        let _ = write!(
            svg,
            "<rect x=\"{x:.1}\" y=\"{:.1}\" width=\"{bar:.1}\" height=\"{h:.1}\" rx=\"1.5\" fill=\"{}\"><title>{} · {}</title></rect>",
            plot - h,
            hex(fill),
            esc(&axis[at]),
            esc(&said(*value))
        );
        if at % every == 0 {
            let _ = write!(
                svg,
                "<text x=\"{:.1}\" y=\"{:.1}\" font-size=\"10\" text-anchor=\"middle\" fill=\"{}\">{}</text>",
                at as f32 * pitch + pitch / 2.0,
                height - 4.0,
                hex(look.faint),
                esc(&axis[at])
            );
        }
    }
    let _ = write!(
        svg,
        "<line x1=\"0\" y1=\"{:.1}\" x2=\"{width:.1}\" y2=\"{:.1}\" stroke=\"{}\"/></svg>",
        plot + 0.5,
        plot + 0.5,
        hex(look.rule)
    );
    svg
}

/// The last reading of one book cut into columns by `book_graphs::cut_to`, as
/// the Kindle's Graphs tab cuts it, and laid across `width`: each column's
/// left edge and width, a cut run as wide as the words naming it.
pub struct BookStrip {
    pub from: i64,
    pub to: i64,
    pub values: Vec<i64>,
    pub edges: Vec<i64>,
    pub breaks: Vec<usize>,
    /// What each break states, empty where the breaks take more than half the row.
    pub said: Vec<String>,
    pub grain: book_graphs::Grain,
    pub year: bool,
    pub cells: Vec<(f32, f32)>,
    pub left: f32,
    pub right: f32,
    pub width: f32,
}

impl BookStrip {
    pub fn of(stats: &Stats, at: usize, week: WeekStart, s: &Strings, width: f32) -> Option<Self> {
        let (from, to) = *stats.book_readings(at).last()?;
        let (left, right) = (44.0, 12.0);
        let inner = width - left - right;
        let most = ((inner / 4.0) as usize).max(2);
        let (values, edges, breaks, ran, grain) =
            book_graphs::cut_to(&stats.book_days(at), from, to, most, week);
        let count = values.len().max(1);
        let gap = if count > 16 { 2.0 } else { 6.0 };
        let room = inner - gap * (count as f32 - 1.0);
        let said: Vec<String> = ran
            .iter()
            .map(|run| s.days_plain.replace("{d}", &run.to_string()))
            .collect();
        let words: Vec<f32> = said
            .iter()
            .map(|t| t.chars().count() as f32 * 6.2 + 10.0)
            .collect();
        let stating = words.iter().sum::<f32>() * 2.0 <= room;
        let ordinary = match stating {
            true => (room - words.iter().sum::<f32>()) / (count - breaks.len()).max(1) as f32,
            false => room / count as f32,
        };
        let mut cells = Vec::with_capacity(count);
        let mut x = left;
        for column in 0..count {
            let w = match (stating, breaks.iter().position(|b| *b == column)) {
                (true, Some(nth)) => words[nth],
                _ => ordinary,
            };
            cells.push((x, w));
            x += w + gap;
        }
        Some(Self {
            from,
            to,
            year: date::civil_from_days(from).0 != date::civil_from_days(to).0,
            said: match stating {
                true => said,
                false => vec![String::new(); breaks.len()],
            },
            values,
            edges,
            breaks,
            grain,
            cells,
            left,
            right,
            width,
        })
    }

    fn name(&self, column: usize, s: &Strings) -> String {
        self.grain.name(self.edges[column], self.year, s)
    }

    fn is_break(&self, column: usize) -> bool {
        self.breaks.contains(&column)
    }

    /// The marks and words along the axis at `axis`: two slashes over each
    /// cut, the run it held over them, and every `every`th column named from
    /// the right, the last always.
    fn axis(&self, svg: &mut String, axis: f32, s: &Strings, look: &Look) {
        for (nth, column) in self.breaks.iter().enumerate() {
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
            if let Some(said) = self.said.get(nth).filter(|t| !t.is_empty()) {
                let _ = write!(
                    svg,
                    "<text x=\"{mid:.1}\" y=\"{:.1}\" font-size=\"10\" text-anchor=\"middle\" fill=\"{}\">{}</text>",
                    axis - 10.0,
                    hex(look.soft),
                    esc(said)
                );
            }
        }
        let every = (self.values.len() / 4).max(1);
        let mut room = self.width;
        let mut column = self.values.len();
        while column > 0 {
            column -= 1;
            if self.is_break(column) || !(self.values.len() - 1 - column).is_multiple_of(every) {
                continue;
            }
            let name = self.name(column, s);
            let (x, w) = self.cells[column];
            let half = name.chars().count() as f32 * 3.1;
            let mid = (x + w / 2.0).min(self.width - half);
            if mid + half + 8.0 > room {
                continue;
            }
            room = mid - half;
            let _ = write!(
                svg,
                "<text x=\"{mid:.1}\" y=\"{:.1}\" font-size=\"10\" text-anchor=\"middle\" fill=\"{}\">{}</text>",
                axis + 16.0,
                hex(look.faint),
                esc(&name)
            );
        }
    }
}

/// The seconds of each column of the reading, as `book_graphs` draws the
/// days, each `<title>` holding its column's figure.
pub fn book_days(strip: &BookStrip, s: &Strings, look: &Look, tone: [u8; 3]) -> String {
    let (top, plot, foot) = (10.0, 140.0, 24.0);
    let height = top + plot + foot;
    let max = strip.values.iter().copied().max().unwrap_or(1).max(1);
    let step = nice_hours(max).min(match max {
        m if m < 3600 => 900,
        m if m < 7200 => 1800,
        _ => i64::MAX,
    });
    let mut svg = open(strip.width, height);
    let mut line = step;
    while line <= max {
        let y = top + plot - plot * line as f32 / max as f32;
        let _ = write!(
            svg,
            "<line x1=\"{:.1}\" y1=\"{y:.1}\" x2=\"{:.1}\" y2=\"{y:.1}\" stroke=\"{}\" stroke-dasharray=\"2 3\"/>\
             <text x=\"{:.1}\" y=\"{:.1}\" font-size=\"10\" text-anchor=\"end\" fill=\"{}\">{}</text>",
            strip.left,
            strip.width - strip.right,
            hex(look.rule),
            strip.left - 6.0,
            y + 3.5,
            hex(look.faint),
            esc(&date::duration(line, s))
        );
        line += step;
    }
    for (column, secs) in strip.values.iter().enumerate() {
        if strip.is_break(column) || *secs <= 0 {
            continue;
        }
        let (x, w) = strip.cells[column];
        let h = plot * *secs as f32 / max as f32;
        let _ = write!(
            svg,
            "<rect x=\"{x:.1}\" y=\"{:.1}\" width=\"{w:.1}\" height=\"{h:.1}\" rx=\"1\" fill=\"{}\"><title>{} · {}</title></rect>",
            top + plot - h,
            hex(tone),
            esc(&strip.name(column, s)),
            esc(&date::duration(*secs, s))
        );
    }
    let base = top + plot + 0.5;
    let _ = write!(
        svg,
        "<line x1=\"{:.1}\" y1=\"{base:.1}\" x2=\"{:.1}\" y2=\"{base:.1}\" stroke=\"{}\"/>",
        strip.left,
        strip.width - strip.right,
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
            "<line x1=\"{:.1}\" y1=\"{:.1}\" x2=\"{:.1}\" y2=\"{:.1}\" stroke=\"{}\"/>",
            strip.left,
            y(share) + 0.5,
            strip.width - strip.right,
            y(share) + 0.5,
            hex(look.rule)
        );
    }
    // 0, 50 and 100 named at the closing end, over their lines, as `charts::scatter` names them.
    for share in [0, 50, 100] {
        let _ = write!(
            svg,
            "<text x=\"{:.1}\" y=\"{:.1}\" font-size=\"10\" text-anchor=\"end\" fill=\"{}\">{}</text>",
            strip.width - strip.right,
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
