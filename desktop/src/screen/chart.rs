//! The charts, as SVG drawn off `Stats` series: [`Look`] holds every colour as
//! a literal, `charts::level` gives a day's step, and `book_graphs::cut_to`
//! cuts the columns a book's two bands stand on.

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
}

/// One year as one strip, a column a week. `opened` and `today` bound the days
/// filled, one before `opened` an outline; a day with reading opens `day_link`,
/// a month's name `month_link`.
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

/// One reading of a book, `from` to `to`, cut into columns by
/// `book_graphs::cut_to` as the Kindle's Graphs tab cuts it, and laid across
/// `width`: each column's left edge and width, a cut run as wide as its words.
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
    pub fn of(
        stats: &Stats,
        at: usize,
        (from, to): (i64, i64),
        week: WeekStart,
        s: &Strings,
        width: f32,
    ) -> Self {
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
        Self {
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
        }
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
/// cut into its foot; a day with reading opens `link`.
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

/// `text` cut to stand inside `room` at `px`, closed by `…` where it is cut;
/// `width` sets a Han or kana character a full `px` wide, any other about half.
fn clip(text: &str, room: f32, px: f32) -> String {
    let width = |c: char| match c as u32 {
        0x2E80.. => px,
        _ => px * 0.58,
    };
    let whole: f32 = text.chars().map(width).sum();
    if whole <= room {
        return text.to_string();
    }
    let mut out = String::new();
    let mut taken = width('…');
    for c in text.chars() {
        taken += width(c);
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
/// hours along its foot; a day with reading opens `link`.
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
                let _ = write!(
                    svg,
                    "<rect x=\"{x:.1}\" y=\"{ly:.1}\" width=\"{w:.1}\" height=\"{:.1}\" rx=\"3\" fill=\"{}\"><title>{}</title></rect>\
                     <text x=\"{:.1}\" y=\"{:.1}\" font-size=\"10.5\" fill=\"{}\" pointer-events=\"none\">{}</text>",
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
