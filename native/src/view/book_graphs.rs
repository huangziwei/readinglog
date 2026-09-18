//! One book's reading drawn: each reading it has been given, the days of the
//! one picked, where each of its sittings sat in the book, and which hours of
//! the day it was read in.

use crate::date;
use crate::lang::Strings;
use crate::settings::WeekStart;
use crate::ui::charts;
use crate::ui::chrome;
use crate::ui::paint::{self, INK, PALE, Rect};
use crate::ui::theme::Theme;

use super::{Ctx, Hit};

/// The per cent a mark at the top of the scatter stands for.
const WHOLE_BOOK: i64 = 100;

/// One hour of the clock in this many is named.
const HOURS_NAMED: usize = 3;

/// The reading row takes at most one in this many of the page's height.
const LISTED: i32 = 2;

/// A column narrower than this steps the strip up to a coarser grain.
const THINNEST: i32 = 4;

/// A run of days holding nothing is cut where it runs at least this long.
const GAP: i64 = 14;

/// A run is cut where it also covers this many times `usual`.
const UNUSUAL: i64 = 3;

/// What one column of a strip of days covers.
#[derive(Debug, Clone, Copy, PartialEq)]
enum Grain {
    Day,
    Week,
    Month,
}

impl Grain {
    /// Finest first: a strip takes the first of these its columns fit in.
    const ALL: [Grain; 3] = [Grain::Day, Grain::Week, Grain::Month];

    /// About how many days one column of this grain covers.
    fn days(self) -> i64 {
        match self {
            Grain::Day => 1,
            Grain::Week => 7,
            Grain::Month => 30,
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

    /// What a column opening on `day` is called. A `year` of true states the
    /// year with every name.
    fn name(self, day: i64, year: bool, s: &Strings) -> String {
        let (y, m, _) = date::civil_from_days(day);
        match (self, year) {
            (Grain::Month, _) => date::month_name(y, m, s),
            (_, true) => date::year_day(day, s),
            (_, false) => date::short_day(day, s),
        }
    }
}

/// A strip of seconds over `from..=to` at one grain, with every run of columns
/// holding nothing that covers `floor` days or more cut down to one column.
/// The run a break stands for is measured in days at every grain.
fn strip(
    days: &[(i64, i64)],
    from: i64,
    to: i64,
    grain: Grain,
    floor: i64,
    week: WeekStart,
) -> (Vec<i64>, Vec<i64>, Vec<usize>, Vec<i64>) {
    let bins = grain.edges(from, to, week);
    let mut secs = vec![0i64; bins.len().max(1)];
    let last = secs.len() - 1;
    for (day, seconds) in days {
        if *day < from || *day > to {
            continue;
        }
        let at = bins.partition_point(|edge| edge <= day).saturating_sub(1);
        secs[at.min(last)] += seconds;
    }
    let (mut values, mut edges) = (Vec::new(), Vec::new());
    let (mut breaks, mut ran) = (Vec::new(), Vec::new());
    let mut at = 0;
    while at < secs.len() {
        let run = match secs[at] {
            0 => secs[at..].iter().take_while(|s| **s == 0).count(),
            _ => 0,
        };
        let held = run as i64 * grain.days();
        // A run at either end of the strip is the span's own edge, never a break.
        if held >= floor && at > 0 && at + run < secs.len() {
            breaks.push(values.len());
            ran.push(held);
            values.push(0);
            edges.push(bins[at]);
            at += run;
            continue;
        }
        values.push(secs[at]);
        edges.push(bins[at]);
        at += 1;
    }
    (values, edges, breaks, ran)
}

/// The columns a strip takes: the finest grain whose count comes under `most`
/// once its runs are cut.
fn cut_to(
    days: &[(i64, i64)],
    from: i64,
    to: i64,
    most: usize,
    week: WeekStart,
) -> (Vec<i64>, Vec<i64>, Vec<usize>, Vec<i64>, Grain) {
    let floor = GAP.max(usual(days, from, to) * UNUSUAL);
    let mut held = None;
    for grain in Grain::ALL {
        let (values, edges, breaks, ran) = strip(days, from, to, grain, floor, week);
        let fits = values.len() <= most;
        held = Some((values, edges, breaks, ran, grain));
        if fits {
            break;
        }
    }
    held.expect("a grain")
}

/// The reading's own rhythm: the median run of days between one day of `days`
/// inside `from..=to` and the next, never under one.
fn usual(days: &[(i64, i64)], from: i64, to: i64) -> i64 {
    let read: Vec<i64> = days
        .iter()
        .map(|(day, _)| *day)
        .filter(|day| (from..=to).contains(day))
        .collect();
    let mut gaps: Vec<i64> = read.windows(2).map(|two| two[1] - two[0]).collect();
    gaps.sort_unstable();
    gaps.get(gaps.len() / 2).copied().unwrap_or(1).max(1)
}

/// "Aug 6 – Oct 22, 2024", or both years where the two fall in different ones.
/// A `whole` of false states the day it opened and no day it closed; `from`
/// meeting `to` states that day alone.
fn spanned(from: i64, to: i64, whole: bool, s: &Strings) -> String {
    if !whole {
        return format!("{} –", date::year_day(from, s));
    }
    if from == to {
        return date::year_day(to, s);
    }
    let (opened, closed) = (date::civil_from_days(from).0, date::civil_from_days(to).0);
    let from = match opened == closed {
        true => date::short_day(from, s),
        false => date::year_day(from, s),
    };
    format!("{from} – {}", date::year_day(to, s))
}

/// Whether the reading standing `at` was carried through the end of the book.
/// Only the standing one can have been left open, and only where the record
/// says the book is unfinished.
fn whole(at: usize, reads: usize, finished: bool) -> bool {
    at + 1 < reads || finished
}

/// The whole box under the head row: the reading picked, then that reading
/// drawn out under it.
pub fn draw(cx: &mut Ctx, area: Rect, index: usize, on: Option<usize>) {
    let s = cx.s();
    let reads = cx.stats.book_readings(index);
    let Some((on, (from, to))) = picked(on, &reads) else {
        return;
    };
    let theme: &Theme = cx.theme;
    let head = chrome::section_height(cx.text, theme);
    let line = line(cx);
    let room = (area.h / LISTED - head).max(line);
    let pitch = room.clamp(line, theme.row_h);
    let (top, rest) = area.split_top(head + pitch + theme.gap * 2);
    listed(cx, top, index, &reads, on, pitch);

    // `days`, `sat` and `book_hours` are all cut to `from..=to`.
    let hours = cx.stats.book_hours(index, from..=to);
    let clock = hours.iter().any(|secs| *secs > 0);
    let rows = rest.rows(2 + clock as i32, cx.theme.gap * 2);
    let theme: &Theme = cx.theme;
    let carried = whole(on, reads.len(), cx.stats.books[index].finished);
    let said = spanned(from, to, carried, s);
    let strip = Strip::of(cx, index, rows[0].w, from, to);
    let inner = chrome::heading_stating(cx.fb, cx.text, theme, rows[0], s.the_days, Some(&said));
    days(cx, inner, &strip, s);
    let inner = chrome::heading_stating(cx.fb, cx.text, theme, rows[1], s.where_you_sat, None);
    sat(cx, inner, index, &strip, from, to);
    if let Some(row) = rows.last().filter(|_| clock) {
        let fold = cx.stats.fold(hours.to_vec(), hours.iter().sum());
        let names: Vec<String> = (0..24).map(|at| format!("{at:02}")).collect();
        super::alltime::band(cx, *row, s.the_clock, &fold, &names, HOURS_NAMED, false);
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

/// The reading picked: its span at the left, the seconds it took at the right,
/// closed by a rule. One reading fills the band, and `‹ ›` step between them.
fn listed(cx: &mut Ctx, area: Rect, index: usize, reads: &[(i64, i64)], on: usize, pitch: i32) {
    let s = cx.s();
    let theme: &Theme = cx.theme;
    let bar = Rect::new(
        area.x,
        area.y,
        area.w,
        chrome::section_height(cx.text, theme),
    );
    let title = s.read_through_band;
    let inner = chrome::heading_stating(cx.fb, cx.text, theme, area, title, None);
    if reads.len() > 1 {
        let of = format!("{} / {}", on + 1, reads.len());
        let last = reads.len() - 1;
        super::daybooks::pager(
            cx,
            bar,
            &of,
            &[
                Hit::Reading(on.saturating_sub(1)),
                Hit::Reading((on + 1).min(last)),
            ],
        );
    }
    let days = cx.stats.book_days(index);
    let (opened, closed) = reads[on];
    let secs: i64 = days
        .iter()
        .filter(|(day, _)| *day >= opened && *day <= closed)
        .map(|(_, secs)| secs)
        .sum();
    let script = cx.ui_script();
    line(cx);
    let cap = cx.text.cap_height() as i32;
    let row = Rect::new(inner.x, inner.y, inner.w, pitch);
    let baseline = row.y + (pitch + cap) / 2;
    let carried = whole(on, reads.len(), cx.stats.books[index].finished);
    let said = spanned(opened, closed, carried, s);
    cx.text
        .draw_inked(script, cx.fb, row.x, baseline, &said, INK);
    let over = date::duration(secs, s);
    let w = cx.text.measure_width(&over) as i32;
    cx.text
        .draw_inked(script, cx.fb, row.right() - w, baseline, &over, INK);
    // The rule closes the band the air over the heading's cap opens it by.
    let air = baseline - row.y - cap;
    paint::hline(cx.fb, area.x, baseline + air, area.w, PALE, theme.rule());
}

/// The one strip a reading's two bands are both drawn on. Their columns and
/// their axis stand in the same places.
struct Strip {
    values: Vec<i64>,
    edges: Vec<i64>,
    grain: Grain,
    /// Whether the strip reaches over a new year, which every name states.
    year: bool,
    every: usize,
    /// The columns standing for a run the strip cut, how long each ran, and
    /// how wide they draw.
    breaks: Vec<usize>,
    said: Vec<String>,
    share: f32,
}

impl Strip {
    /// The strip one reading draws on, at the finest grain whose columns fit
    /// `wide` once its runs are cut.
    fn of(cx: &mut Ctx, index: usize, wide: i32, from: i64, to: i64) -> Strip {
        let s = cx.s();
        let days = cx.stats.book_days(index);
        let most = (wide / THINNEST).max(2) as usize;
        let (values, edges, breaks, ran, grain) = cut_to(&days, from, to, most, cx.week);
        let mut strip = Strip {
            every: (values.len() / 4).max(1),
            year: date::civil_from_days(from).0 != date::civil_from_days(to).0,
            said: ran
                .iter()
                .map(|run| s.days_plain.replace("{d}", &run.to_string()))
                .collect(),
            share: 1.0,
            values,
            edges,
            grain,
            breaks,
        };
        strip.share = strip.share_of(cx, wide);
        strip
    }

    /// How wide a break column stands against an ordinary one, never under one
    /// column. A strip `count` wide cuts a break of `share`
    /// `room * share / (count - 1 + share)` wide.
    fn share_of(&self, cx: &mut Ctx, wide: i32) -> f32 {
        if self.breaks.is_empty() {
            return 1.0;
        }
        cx.text.set_px(cx.theme.small_px);
        let widest = self
            .said
            .iter()
            .map(|said| cx.text.measure_width(said) as i32)
            .max()
            .unwrap_or(0);
        let count = self.values.len().max(1) as i32;
        let gap = charts::cell_gap(cx.theme, self.values.len());
        let room = (wide - gap * (count - 1)).max(1);
        let want = widest + cx.theme.gap * 2;
        // A run needing a third of the row keeps one column.
        if want * 3 >= room {
            return 1.0;
        }
        (want * (count - 1)) as f32 / (room - want) as f32
    }

    /// Where this strip was cut, as `charts` takes it.
    fn breaks(&self) -> charts::Breaks<'_> {
        charts::Breaks {
            at: &self.breaks,
            said: &self.said,
            share: self.share,
        }
    }

    fn name(&self, at: usize, s: &Strings) -> String {
        self.grain.name(self.edges[at], self.year, s)
    }

    fn column(&self, day: i64) -> usize {
        let last = self.values.len().saturating_sub(1);
        self.edges
            .partition_point(|edge| *edge <= day)
            .saturating_sub(1)
            .min(last)
    }
}

/// The seconds read on each column of one reading.
fn days(cx: &mut Ctx, area: Rect, strip: &Strip, s: &'static Strings) {
    let theme: &Theme = cx.theme;
    charts::columns(
        cx.fb,
        cx.text,
        theme,
        cx.palette,
        area,
        &strip.values,
        |at| strip.name(at, s),
        &|secs| super::alltime::duration_rows(secs, s),
        strip.every,
        None,
        None,
        &strip.breaks(),
    );
}

/// The run of the book each sitting of one reading covered, opening place to
/// closing place. A sitting the record states no run for keeps its own mark,
/// and the place before it where it states no place either.
fn sat(cx: &mut Ctx, area: Rect, index: usize, strip: &Strip, from: i64, to: i64) {
    let s: &Strings = cx.s();
    let mut place = 0;
    let runs = cx.stats.book_stretches(index);
    let at: Vec<charts::Sat> = cx
        .stats
        .book_places(index)
        .into_iter()
        .zip(runs)
        .filter(|((day, _), _)| (from..=to).contains(day))
        .map(|((day, at), runs)| {
            place = at.unwrap_or(place);
            charts::Sat {
                column: strip.column(day),
                place,
                runs,
            }
        })
        .collect();
    let theme: &Theme = cx.theme;
    charts::scatter(
        cx.fb,
        cx.text,
        theme,
        cx.palette,
        area,
        &at,
        strip.values.len(),
        |at| strip.name(at, s),
        strip.every,
        WHOLE_BOOK,
        &|share| s.percent_plain.replace("{d}", &share.to_string()),
        &strip.breaks(),
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
                stretches: Vec::new(),
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
        let most = |wide: i32| (wide / THINNEST).max(2) as usize;
        // Forty days in 400px is a column a day; four hundred is not.
        let (values, _, _, _, grain) = cut_to(&days, 20_000, 20_039, most(400), week);
        assert_eq!((values.len(), grain == Grain::Day), (40, true));
        let (_, _, _, _, grain) = cut_to(&days, 20_000, 20_399, most(400), week);
        assert_eq!(grain, Grain::Week);
        let (_, _, _, _, grain) = cut_to(&days, 20_000, 22_000, most(400), week);
        assert_eq!(grain, Grain::Month);
        // Every second of the span lands in a column, wherever it is cut.
        for wide in [40, 120, 400] {
            let (values, _, _, _, _) = cut_to(&days, 20_000, 20_399, most(wide), week);
            assert_eq!(values.iter().sum::<i64>(), 400 * 1800);
        }
    }

    /// The days a book was read on: `runs` runs of `run` days `on` days
    /// apart, with `off` days holding nothing between one run and the next.
    fn rhythm(runs: usize, run: i64, on: i64, off: i64) -> Vec<(i64, i64)> {
        let mut out: Vec<(i64, i64)> = Vec::new();
        let mut day = 20_000;
        for nth in 0..runs {
            if nth > 0 {
                day += off;
            }
            for step in 0..run {
                out.push((day, 1800));
                if step + 1 < run {
                    day += on;
                }
            }
            day += 1;
        }
        out
    }

    /// A run is cut where it is both long and unlike the reading around it.
    #[test]
    fn a_run_is_cut_only_where_it_is_long_and_unlike_the_reading() {
        let week = WeekStart::Monday;
        let cut = |days: &[(i64, i64)]| {
            let (from, to) = (days[0].0, days[days.len() - 1].0);
            let (values, _, breaks, ran, _) = cut_to(days, from, to, 400, week);
            (values.len(), breaks.len(), ran)
        };
        // Five days, 34 off, five days: one cut, and 44 columns become 11.
        let put_down = rhythm(2, 5, 1, 34);
        assert_eq!(cut(&put_down), (11, 1, vec![34]));
        // A day every twelve keeps every column: the gap is this rhythm.
        let slowly = rhythm(1, 12, 12, 0);
        assert_eq!(cut(&slowly), (12 * 11 + 1, 0, vec![]));
        // And a run under `GAP` is never cut however unlike the rhythm it is.
        let a_week_off = rhythm(2, 5, 1, 12);
        assert_eq!(cut(&a_week_off), (22, 0, vec![]));
    }

    /// The grain is asked after the runs are cut.
    #[test]
    fn the_grain_steps_only_after_the_runs_are_cut() {
        let week = WeekStart::Monday;
        let days = rhythm(2, 20, 1, 380);
        let (from, to) = (days[0].0, days[days.len() - 1].0);
        // 420 days of span, but only 40 of them were read on.
        assert_eq!(to - from + 1, 420);
        let (values, _, breaks, ran, grain) = cut_to(&days, from, to, 100, week);
        assert_eq!((values.len(), breaks.len(), ran), (41, 1, vec![380]));
        assert_eq!(grain, Grain::Day, "the span alone would have said weeks");
        // A row too narrow for 41 columns steps the grain and holds the cut.
        let (values, _, breaks, _, grain) = cut_to(&days, from, to, 20, week);
        assert_eq!(breaks.len(), 1);
        assert!(values.len() <= 20, "{} columns", values.len());
        assert_eq!(grain, Grain::Week);
    }

    /// Every second of the reading lands in a column, cut or not, and a break
    /// column carries none of them.
    #[test]
    fn a_cut_strip_holds_every_second_the_reading_held() {
        let days = rhythm(3, 4, 1, 60);
        let (from, to) = (days[0].0, days[days.len() - 1].0);
        let (values, _, breaks, _, _) = cut_to(&days, from, to, 400, WeekStart::Monday);
        assert_eq!(values.iter().sum::<i64>(), 12 * 1800);
        assert_eq!(breaks.len(), 2);
        for at in breaks {
            assert_eq!(values[at], 0, "a break column carries no bar");
        }
    }

    #[test]
    fn a_span_states_a_year_once_where_it_holds_to_one() {
        let s = Lang::English.strings();
        let (open, close) = (
            date::days_from_civil(2024, 8, 6),
            date::days_from_civil(2024, 10, 22),
        );
        assert_eq!(spanned(open, close, true, s), "Aug 6 – Oct 22, 2024");
        let close = date::days_from_civil(2026, 8, 7);
        assert_eq!(spanned(open, close, true, s), "Aug 6, 2024 – Aug 7, 2026");
        // A reading carried through in a day states that day once.
        assert_eq!(spanned(open, open, true, s), "Aug 6, 2024");
        // A `whole` of false states no day it closed on.
        assert_eq!(spanned(open, close, false, s), "Aug 6, 2024 –");
    }

    #[test]
    fn every_reading_but_the_standing_one_was_carried_through_the_end() {
        assert_eq!(
            (0..3).map(|at| whole(at, 3, false)).collect::<Vec<_>>(),
            [true, true, false]
        );
        assert!(whole(2, 3, true), "a finished book carries its last");
        assert!(!whole(0, 1, false));
    }

    #[test]
    fn the_reading_drawn_is_the_standing_one_until_a_row_is_tapped() {
        let reads = readings(5, &[20_003]);
        assert_eq!(picked(None, &reads), Some((1, (20_003, 20_004))));
        assert_eq!(picked(Some(0), &reads), Some((0, (20_000, 20_002))));
        // An `on` past the last reading falls back to it.
        assert_eq!(picked(Some(9), &reads), Some((1, (20_003, 20_004))));
        assert_eq!(picked(None, &[]), None);
    }

    #[test]
    fn the_clock_holds_every_hour_this_book_was_read_in() {
        let stats = read(&[Some(0.10), Some(0.40)], &[]);
        let hours = stats.book_hours(0, i64::MIN..=i64::MAX);
        assert_eq!(hours[1], 3600, "both sittings fell in the same hour");
        assert_eq!(hours.iter().sum::<i64>(), 3600);
        assert_eq!(hours.len(), 24);
    }

    /// [`Stats::book_hours`] over one reading's span holds that reading's
    /// seconds and no others.
    #[test]
    fn the_clock_holds_only_the_hours_of_the_reading_picked() {
        let stats = read(&[Some(0.10), Some(0.40)], &[]);
        let days = stats.book_days(0);
        assert_eq!(days.len(), 2, "a day each");
        let whole = stats.book_hours(0, i64::MIN..=i64::MAX);
        for (day, secs) in &days {
            let cut = stats.book_hours(0, *day..=*day);
            assert_eq!(cut.iter().sum::<i64>(), *secs, "day {day}");
            assert!(cut.iter().sum::<i64>() < whole.iter().sum::<i64>());
        }
    }
}
