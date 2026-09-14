//! One book's reading drawn: each reading it has been given, the days of the
//! one picked, where each of its sittings sat in the book, and which hours of
//! the day it was read in.

use crate::date;
use crate::lang::{self, Strings};
use crate::settings::WeekStart;
use crate::ui::charts;
use crate::ui::chrome;
use crate::ui::paint::{self, DARK, INK, Rect};
use crate::ui::theme::Theme;

use super::{Ctx, Hit};

/// The per cent a mark at the top of the scatter stands for.
const WHOLE_BOOK: i64 = 100;

/// One hour of the clock in this many is named, as `alltime::trends` names it.
const HOURS_NAMED: usize = 3;

/// The list of readings takes at most one in this many of the page's height.
const LISTED: i32 = 2;

/// How wide a column of the days band may fall before the strip steps up to a
/// coarser one.
const THINNEST: i32 = 4;

/// What one column of a strip of days covers.
#[derive(Debug, Clone, Copy, PartialEq)]
enum Grain {
    Day,
    Week,
    Month,
}

impl Grain {
    /// The coarsest column a span of `days` needs to come under `most`.
    fn for_span(days: i64, most: i64) -> Grain {
        match (days <= most, days / 7 < most) {
            (true, _) => Grain::Day,
            (_, true) => Grain::Week,
            _ => Grain::Month,
        }
    }

    /// The first day of every column between `from` and `to`.
    fn edges(self, from: i64, to: i64, week: WeekStart) -> Vec<i64> {
        let mut out = Vec::new();
        match self {
            Grain::Day => out.extend(from..=to),
            Grain::Week => {
                let mut at = from - week.column_of(date::weekday(from)) as i64;
                while at <= to {
                    out.push(at);
                    at += 7;
                }
            }
            Grain::Month => {
                let (y, m, _) = date::civil_from_days(from);
                let mut at = date::days_from_civil(y, m, 1);
                while at <= to {
                    out.push(at);
                    at = date::shift_months(at, 1);
                }
            }
        }
        out
    }

    /// What a column opening on `day` is called. A strip reaching over a new
    /// year states one, so two columns a year apart never read alike.
    fn name(self, day: i64, year: bool, s: &Strings) -> String {
        let (y, m, _) = date::civil_from_days(day);
        match (self, year) {
            (Grain::Month, _) => date::month_name(y, m, s),
            (_, true) => date::year_day(day, s),
            (_, false) => date::short_day(day, s),
        }
    }
}

/// A strip of seconds over `from..=to`, no column narrower than [`THINNEST`].
fn strip(
    days: &[(i64, i64)],
    from: i64,
    to: i64,
    wide: i32,
    week: WeekStart,
) -> (Vec<i64>, Vec<i64>, Grain) {
    let most = (wide / THINNEST).max(2) as i64;
    let grain = Grain::for_span(to - from + 1, most);
    let edges = grain.edges(from, to, week);
    let mut values = vec![0i64; edges.len().max(1)];
    let last = values.len() - 1;
    for (day, secs) in days {
        if *day < from || *day > to {
            continue;
        }
        let at = edges.partition_point(|edge| edge <= day).saturating_sub(1);
        values[at.min(last)] += secs;
    }
    (values, edges, grain)
}

/// "Aug 6 – Oct 22, 2024", or both years where the two fall in different ones.
fn spanned(from: i64, to: i64, s: &Strings) -> String {
    let (opened, closed) = (date::civil_from_days(from).0, date::civil_from_days(to).0);
    let from = match opened == closed {
        true => date::short_day(from, s),
        false => date::year_day(from, s),
    };
    format!("{from} – {}", date::year_day(to, s))
}

/// The whole box under the head row: the readings listed, then the one picked
/// drawn out under them.
pub fn draw(cx: &mut Ctx, area: Rect, index: usize, on: Option<usize>) {
    let s = cx.s();
    let reads = cx.stats.book_readings(index);
    let Some((on, (from, to))) = picked(on, &reads) else {
        return;
    };
    let theme: &Theme = cx.theme;
    // A row is a tap target, so it takes a list row's height where the
    // readings are few and gives way to the line height where they are many.
    let head = chrome::section_height(cx.text, theme);
    let line = line(cx);
    let room = (area.h / LISTED - head).max(line);
    let pitch = (room / reads.len() as i32).clamp(line, theme.row_h);
    let deep = (head + pitch * reads.len() as i32).min(head + room) + theme.gap * 2;
    let (top, rest) = area.split_top(deep);
    listed(cx, top, index, &reads, on, pitch);

    let hours = cx.stats.book_hours(index);
    let clock = hours.iter().any(|secs| *secs > 0);
    let rows = rest.rows(2 + clock as i32, cx.theme.gap * 2);
    let theme: &Theme = cx.theme;
    let title = lang::counted(s.nth_reading, on as i64 + 1);
    let said = spanned(from, to, s);
    let head = format!("{} · {title}", s.the_days);
    let inner = chrome::section_stating(cx.fb, cx.text, theme, rows[0], &head, Some(&said));
    days(cx, inner, index, from, to);
    let inner = chrome::section(cx.fb, cx.text, theme, rows[1], s.where_you_sat);
    sat(cx, inner, index, from, to);
    if let Some(row) = rows.last().filter(|_| clock) {
        let fold = cx.stats.fold(hours.to_vec(), hours.iter().sum());
        let names: Vec<String> = (0..24).map(|at| format!("{at:02}")).collect();
        super::alltime::band(cx, *row, s.the_clock, &fold, &names, HOURS_NAMED);
    }
}

/// The reading drawn: the one `State::reading` names, or the standing one.
fn picked(on: Option<usize>, reads: &[(i64, i64)]) -> Option<(usize, (i64, i64))> {
    let last = reads.len().checked_sub(1)?;
    let on = on.unwrap_or(last).min(last);
    Some((on, reads[on]))
}

fn line(cx: &mut Ctx) -> i32 {
    cx.text.set_px(cx.theme.small_px);
    cx.text.line_height() as i32
}

/// Each reading on its own row, newest first, the picked one inked black. A
/// reading carried through the end of the book takes a filled mark.
fn listed(cx: &mut Ctx, area: Rect, index: usize, reads: &[(i64, i64)], on: usize, pitch: i32) {
    let s = cx.s();
    let theme: &Theme = cx.theme;
    let title = lang::counted(s.readings_band, reads.len() as i64);
    let inner = chrome::section(cx.fb, cx.text, theme, area, &title);
    let days = cx.stats.book_days(index);
    let standing = cx.stats.books[index].finished;
    let script = cx.ui_script();
    line(cx);
    let cap = cx.text.cap_height() as i32;
    let side = cap * 2 / 3;
    for (at, (opened, closed)) in reads.iter().enumerate().rev() {
        let down = (reads.len() - 1 - at) as i32 * pitch;
        let row = Rect::new(inner.x, inner.y + down, inner.w, pitch);
        if row.bottom() > inner.bottom() {
            break;
        }
        let secs: i64 = days
            .iter()
            .filter(|(day, _)| day >= opened && day <= closed)
            .map(|(_, secs)| secs)
            .sum();
        let whole = at + 1 < reads.len() || standing;
        let baseline = row.y + (pitch + cap) / 2;
        let mark = Rect::new(row.x, baseline - side, side, side);
        match whole {
            true => paint::fill_rgb(cx.fb, mark, cx.palette.bar()),
            false => paint::stroke(cx.fb, mark, DARK, theme.rule()),
        }
        let ink = match at == on {
            true => INK,
            false => DARK,
        };
        let said = format!("{}   {}", at + 1, spanned(*opened, *closed, s));
        let x = mark.right() + theme.gap;
        cx.text.draw_inked(script, cx.fb, x, baseline, &said, ink);
        let (rest, spark) = row.split_left(row.w - row.w / 5);
        let over = date::duration(secs, s);
        let w = cx.text.measure_width(&over) as i32;
        cx.text
            .draw_inked(script, cx.fb, rest.right() - w, baseline, &over, ink);
        let tall = (cap * 3 / 2).min(pitch - 2);
        let strip = Rect::new(
            spark.x + theme.gap,
            baseline - tall,
            spark.w - theme.gap,
            tall,
        );
        sparkline(cx, strip, &days, *opened, *closed);
        cx.hit(Hit::Reading(at), row);
    }
}

/// One reading's own days, small enough to stand on a row of the list.
fn sparkline(cx: &mut Ctx, area: Rect, days: &[(i64, i64)], from: i64, to: i64) {
    let (values, _, _) = strip(days, from, to, area.w, cx.week);
    let top = values.iter().copied().max().unwrap_or(0).max(1);
    let ink = cx.palette.bar();
    for (secs, cell) in values.iter().zip(area.columns(values.len() as i32, 0)) {
        let h = (area.h as i64 * secs / top) as i32;
        if h > 0 {
            let bar = Rect::new(cell.x, area.bottom() - h, cell.w.max(1), h);
            paint::fill_rgb(cx.fb, bar, ink);
        }
    }
}

/// The seconds read on each day of one reading.
fn days(cx: &mut Ctx, area: Rect, index: usize, from: i64, to: i64) {
    let s = cx.s();
    let read = cx.stats.book_days(index);
    let (values, edges, grain) = strip(&read, from, to, area.w, cx.week);
    let over = date::civil_from_days(from).0 != date::civil_from_days(to).0;
    let theme: &Theme = cx.theme;
    charts::columns(
        cx.fb,
        cx.text,
        theme,
        cx.palette,
        area,
        &values,
        |at| grain.name(edges[at], over, s),
        &|secs| super::alltime::duration_rows(secs, s),
        (values.len() / 4).max(1),
        None,
        None,
    );
}

/// Where each sitting of one reading ended, as a mark with nothing under it.
fn sat(cx: &mut Ctx, area: Rect, index: usize, from: i64, to: i64) {
    let s = cx.s();
    let mut place = 0;
    let at: Vec<(i64, i64)> = cx
        .stats
        .book_places(index)
        .into_iter()
        .filter(|(day, _)| (from..=to).contains(day))
        .map(|(day, at)| {
            place = at.unwrap_or(place);
            (day, place)
        })
        .collect();
    let theme: &Theme = cx.theme;
    let ends = (date::short_day(from, s), date::short_day(to, s));
    charts::scatter(
        cx.fb,
        cx.text,
        theme,
        cx.palette,
        area,
        &at,
        from,
        to,
        WHOLE_BOOK,
        &|share| s.percent_plain.replace("{d}", &share.to_string()),
        (&ends.0, &ends.1),
    );
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::lang::Lang;
    use crate::log::session::Measure;
    use crate::stats::{BookStat, Sitting, Stats};

    /// A record of one book whose sittings ended at each of `places`, one a
    /// day from day 20000, begun again on each of `again`.
    fn read(places: &[Option<f64>], again: &[i64]) -> Stats {
        let sittings: Vec<Sitting> = places
            .iter()
            .enumerate()
            .map(|(at, progress)| Sitting {
                day: 20_000 + at as i64,
                from_secs: 3600,
                to_secs: 5400,
                seconds: 1800,
                book: Some(0),
                key: 1,
                measure: Measure::Counted,
                page_turns: 40,
                hours: vec![(1, 1800)],
                progress: *progress,
            })
            .collect();
        let book = BookStat {
            sittings: sittings.len() as i64,
            seconds: 1800 * sittings.len() as i64,
            first_day: 20_000,
            last_day: 20_000 + sittings.len() as i64 - 1,
            restarted_on: again.to_vec(),
            ..BookStat::default()
        };
        Stats {
            sittings,
            books: vec![book],
            ..Stats::default()
        }
    }

    /// The days of a book read `n` days running, begun again on `again`.
    fn readings(n: usize, again: &[i64]) -> Vec<(i64, i64)> {
        let places = vec![Some(0.5); n];
        read(&places, again).book_readings(0)
    }

    #[test]
    fn a_reading_runs_from_its_first_day_to_the_day_before_the_next_began() {
        // Begun again on day 20003: the first reading closes the day before.
        assert_eq!(readings(5, &[20_003]), [(20_000, 20_002), (20_003, 20_004)]);
        // Never begun again, and begun again twice.
        assert_eq!(readings(5, &[]), [(20_000, 20_004)]);
        assert_eq!(
            readings(5, &[20_002, 20_004]),
            [(20_000, 20_001), (20_002, 20_003), (20_004, 20_004)]
        );
        // A day past the last sitting closes the record and opens nothing.
        assert_eq!(readings(5, &[20_099]), [(20_000, 20_004)]);
        // A day before it opens leaves one reading, which it opens.
        assert_eq!(readings(5, &[19_000]), [(20_000, 20_004)]);
        assert!(readings(0, &[]).is_empty());
    }

    #[test]
    fn a_strip_steps_up_a_grain_rather_than_draw_a_column_too_thin_to_see() {
        let week = WeekStart::Monday;
        let days: Vec<(i64, i64)> = (0..400).map(|d| (20_000 + d, 1800)).collect();
        // Forty days in 400px is a column a day; four hundred is not.
        let (values, _, grain) = strip(&days, 20_000, 20_039, 400, week);
        assert_eq!((values.len(), grain == Grain::Day), (40, true));
        let (_, _, grain) = strip(&days, 20_000, 20_399, 400, week);
        assert_eq!(grain, Grain::Week);
        let (_, _, grain) = strip(&days, 20_000, 22_000, 400, week);
        assert_eq!(grain, Grain::Month);
        // Every second of the span lands in a column, wherever it is cut.
        for wide in [40, 120, 400] {
            let (values, _, _) = strip(&days, 20_000, 20_399, wide, week);
            assert_eq!(values.iter().sum::<i64>(), 400 * 1800);
        }
    }

    #[test]
    fn a_span_states_a_year_once_where_it_holds_to_one() {
        let s = Lang::English.strings();
        let (open, close) = (
            date::days_from_civil(2024, 8, 6),
            date::days_from_civil(2024, 10, 22),
        );
        assert_eq!(spanned(open, close, s), "Aug 6 – Oct 22, 2024");
        let close = date::days_from_civil(2026, 8, 7);
        assert_eq!(spanned(open, close, s), "Aug 6, 2024 – Aug 7, 2026");
    }

    #[test]
    fn the_reading_drawn_is_the_standing_one_until_a_row_is_tapped() {
        let reads = readings(5, &[20_003]);
        assert_eq!(picked(None, &reads), Some((1, (20_003, 20_004))));
        assert_eq!(picked(Some(0), &reads), Some((0, (20_000, 20_002))));
        // A picked row the record no longer holds falls back to the last.
        assert_eq!(picked(Some(9), &reads), Some((1, (20_003, 20_004))));
        assert_eq!(picked(None, &[]), None);
    }

    #[test]
    fn the_clock_holds_every_hour_this_book_was_read_in() {
        let stats = read(&[Some(0.10), Some(0.40)], &[]);
        let hours = stats.book_hours(0);
        assert_eq!(hours[1], 3600, "both sittings fell in the same hour");
        assert_eq!(hours.iter().sum::<i64>(), 3600);
        assert_eq!(hours.len(), 24);
    }
}
