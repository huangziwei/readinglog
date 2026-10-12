//! [`draw`]: a book's readings, the days of the one picked, where each of its
//! sittings sat in the book, and the hours of the day it was read in.

use crate::date;
use crate::lang::Strings;
use crate::settings::WeekStart;
use crate::stats::Stats;
use crate::ui::charts;
use crate::ui::chrome;
use crate::ui::paint::{self, INK, PALE, Rect};
use crate::ui::theme::Theme;

use super::{Ctx, Hit};

/// The per cent a mark at the top of the scatter stands for.
const WHOLE_BOOK: i64 = 100;

/// One hour of the clock in this many is named.
pub const HOURS_NAMED: usize = 3;

/// `listed` takes at most one in this many of the page's height.
const LISTED: i32 = 2;

/// The narrowest column `Strip::of` lets `cut_to` keep before a coarser grain.
const THINNEST: i32 = 4;

/// What one column of a strip of days covers.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Grain {
    Day,
    Week,
    Month,
}

impl Grain {
    /// Finest first: `cut_to` takes the first of these its columns fit in.
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
    pub fn name(self, day: i64, year: bool, s: &Strings) -> String {
        let (y, m, _) = date::civil_from_days(day);
        match (self, year) {
            (Grain::Month, _) => date::month_name(y, m, s),
            (_, true) => date::year_day(day, s),
            (_, false) => date::short_day(day, s),
        }
    }
}

/// A strip of seconds over `from..=to` at one grain, with every run of columns
/// holding nothing cut down to one column. `ran` measures each cut run in days
/// at every grain.
fn strip(
    days: &[(i64, i64)],
    from: i64,
    to: i64,
    grain: Grain,
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
        // `run` at either end of `secs` is the span's own edge, never a break.
        if run > 0 && at > 0 && at + run < secs.len() {
            breaks.push(values.len());
            ran.push(run as i64 * grain.days());
            values.push(0);
            edges.push(bins[at]);
            at += run;
            continue;
        }
        let step = run.max(1);
        values.extend(&secs[at..at + step]);
        edges.extend(&bins[at..at + step]);
        at += step;
    }
    (values, edges, breaks, ran)
}

/// The columns a strip takes: the finest grain whose count comes under `most`
/// once its runs are cut.
pub fn cut_to(
    days: &[(i64, i64)],
    from: i64,
    to: i64,
    most: usize,
    week: WeekStart,
) -> (Vec<i64>, Vec<i64>, Vec<usize>, Vec<i64>, Grain) {
    let mut held = None;
    for grain in Grain::ALL {
        let (values, edges, breaks, ran) = strip(days, from, to, grain, week);
        let fits = values.len() <= most;
        held = Some((values, edges, breaks, ran, grain));
        if fits {
            break;
        }
    }
    held.expect("a grain")
}

/// Which runs state their length, spread evenly with the first and last kept,
/// while the stated take at most half of `room` at their `widths` and a `gap`
/// each; and the share cutting a stated break that wide against the rest.
pub fn labelled(widths: &[i32], count: usize, room: i32, gap: i32) -> (Vec<bool>, f32) {
    let runs = widths.len();
    for stating in (1..=runs).rev() {
        let marks = charts::spread(runs, stating);
        let want = marks.iter().map(|at| widths[*at]).max().unwrap_or(0) + gap;
        let taken = want * stating as i32;
        if taken * 2 > room {
            continue;
        }
        let others = count.saturating_sub(stating).max(1) as i32;
        let share = (want * others) as f32 / (room - taken).max(1) as f32;
        let stated = (0..runs).map(|at| marks.contains(&at)).collect();
        return (stated, share.max(1.0));
    }
    (vec![false; runs], 1.0)
}

/// "Aug 6 – Oct 22, 2024", or both years where the two fall in different ones.
/// A `whole` of false states the day it opened and no day it closed; `from`
/// meeting `to` states that day alone.
pub fn spanned(from: i64, to: i64, whole: bool, s: &Strings) -> String {
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

/// Whether reading `at` of `reads` was carried through the end of the book:
/// every one before the last, and the last where `finished`.
pub fn whole(at: usize, reads: usize, finished: bool) -> bool {
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

    // `days`, `where_you_sat` and `book_hours` are all cut to `from..=to`.
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
    where_you_sat(cx, inner, index, &strip, from, to);
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
/// closed by a rule; `daybooks::pager` steps between `reads` holding more than one.
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
    let (opened, closed) = reads[on];
    let secs = cx.stats.book_seconds_over(index, opened..=closed);
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
    // `air` over the heading's cap stands again under `baseline`, over the rule.
    let air = baseline - row.y - cap;
    paint::hline(cx.fb, area.x, baseline + air, area.w, PALE, theme.rule());
}

/// The one strip a reading's two bands are both drawn on, their columns and
/// their axis in the same places.
struct Strip {
    values: Vec<i64>,
    edges: Vec<i64>,
    grain: Grain,
    /// Whether the strip reaches over a new year, which every name states.
    year: bool,
    every: usize,
    /// The columns standing for a run the strip cut.
    breaks: Vec<usize>,
    /// How long each run of `breaks` ran, as the axis states it.
    said: Vec<String>,
    /// How wide a break column draws against an ordinary one.
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
        let said: Vec<String> = ran
            .iter()
            .map(|run| s.days_plain.replace("{d}", &run.to_string()))
            .collect();
        cx.text.set_px(cx.theme.small_px);
        let widths: Vec<i32> = said
            .iter()
            .map(|said| cx.text.measure_width(said) as i32)
            .collect();
        let count = values.len().max(1);
        let room = (wide - charts::cell_gap(cx.theme, count) * (count as i32 - 1)).max(1);
        let (stated, share) = labelled(&widths, count, room, cx.theme.gap);
        Strip {
            every: (values.len() / 4).max(1),
            year: date::civil_from_days(from).0 != date::civil_from_days(to).0,
            said: said
                .into_iter()
                .zip(stated)
                .map(|(said, on)| match on {
                    true => said,
                    false => String::new(),
                })
                .collect(),
            share,
            values,
            edges,
            grain,
            breaks,
        }
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
}

/// The column of `edges` holding `day`, each edge the first day of its column.
pub fn column_of(edges: &[i64], day: i64) -> usize {
    edges
        .partition_point(|edge| *edge <= day)
        .saturating_sub(1)
        .min(edges.len().saturating_sub(1))
}

/// Each sitting of one reading on the column of `edges` holding its day, with
/// the stretches it covered and `progress`, its own else the one before it,
/// where a sitting stating no stretch is marked.
pub fn placed(
    stats: &Stats,
    index: usize,
    edges: &[i64],
    from: i64,
    to: i64,
) -> Vec<charts::Sitting> {
    let mut progress = 0;
    let stretches = stats.book_stretches(index);
    stats
        .book_progress(index)
        .into_iter()
        .zip(stretches)
        .filter(|((day, _), _)| (from..=to).contains(day))
        .map(|((day, at), stretches)| {
            progress = at.unwrap_or(progress);
            charts::Sitting {
                column: column_of(edges, day),
                progress,
                stretches,
            }
        })
        .collect()
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

/// [`placed`] on the strip the days are drawn on.
fn where_you_sat(cx: &mut Ctx, area: Rect, index: usize, strip: &Strip, from: i64, to: i64) {
    let s: &Strings = cx.s();
    let at = placed(cx.stats, index, &strip.edges, from, to);
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
    use crate::log::sitting::Measure;
    use crate::stats::{BookStat, Sitting, Stats};

    /// A record of one book whose sittings ended at each of `progress`, one a
    /// day from day 20000, begun again on each of `again`.
    fn read(progress: &[Option<f64>], again: &[i64]) -> Stats {
        let sittings: Vec<Sitting> = progress
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
        let progress = vec![Some(0.5); n];
        read(&progress, again).book_readings(0)
    }

    #[test]
    fn a_reading_runs_from_its_first_day_to_the_day_before_the_next_began() {
        // Day 20_003 in `again` closes the first reading on 20_002.
        assert_eq!(readings(5, &[20_003]), [(20_000, 20_002), (20_003, 20_004)]);
        // `again` empty, then holding two days.
        assert_eq!(readings(5, &[]), [(20_000, 20_004)]);
        assert_eq!(
            readings(5, &[20_002, 20_004]),
            [(20_000, 20_001), (20_002, 20_003), (20_004, 20_004)]
        );
        // 20_099, past the last sitting, opens no reading.
        assert_eq!(readings(5, &[20_099]), [(20_000, 20_004)]);
        // 19_000, before the first sitting, leaves one reading.
        assert_eq!(readings(5, &[19_000]), [(20_000, 20_004)]);
        assert!(readings(0, &[]).is_empty());
    }

    #[test]
    fn a_strip_steps_up_a_grain_rather_than_draw_a_column_too_thin_to_see() {
        let week = WeekStart::Monday;
        let days: Vec<(i64, i64)> = (0..400).map(|d| (20_000 + d, 1800)).collect();
        let most = |wide: i32| (wide / THINNEST).max(2) as usize;
        // `most(400)` holds forty days at a column each, and four hundred at a coarser grain.
        let (values, _, _, _, grain) = cut_to(&days, 20_000, 20_039, most(400), week);
        assert_eq!((values.len(), grain == Grain::Day), (40, true));
        let (_, _, _, _, grain) = cut_to(&days, 20_000, 20_399, most(400), week);
        assert_eq!(grain, Grain::Week);
        let (_, _, _, _, grain) = cut_to(&days, 20_000, 22_000, most(400), week);
        assert_eq!(grain, Grain::Month);
        // `values` sums to every second of the span at every width.
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

    /// `cut_to` cuts every empty run between two days read, one day long or
    /// many, and draws a run at either end of the span whole.
    #[test]
    fn every_run_between_two_days_read_is_cut() {
        let week = WeekStart::Monday;
        let cut = |days: &[(i64, i64)], from: i64, to: i64| {
            let (values, _, breaks, ran, _) = cut_to(days, from, to, 400, week);
            (values.len(), breaks.len(), ran)
        };
        let span = |days: &[(i64, i64)]| (days[0].0, days[days.len() - 1].0);
        // Five days, 34 off, five days: one cut, and 44 columns become 11.
        let put_down = rhythm(2, 5, 1, 34);
        let (from, to) = span(&put_down);
        assert_eq!(cut(&put_down, from, to), (11, 1, vec![34]));
        // Three days, one off, three days: the one day is cut.
        let a_day_off = rhythm(2, 3, 1, 1);
        let (from, to) = span(&a_day_off);
        assert_eq!(cut(&a_day_off, from, to), (7, 1, vec![1]));
        // Four days of nothing either side of the span stand as its edges.
        assert_eq!(cut(&a_day_off, from - 4, to + 4), (15, 1, vec![1]));
        // A day every twelve: each of the eleven runs between them is cut.
        let slowly = rhythm(1, 12, 12, 0);
        let (from, to) = span(&slowly);
        assert_eq!(cut(&slowly, from, to), (23, 11, vec![11; 11]));
        // Nine days running hold no run to cut.
        let daily = rhythm(1, 9, 1, 0);
        let (from, to) = span(&daily);
        assert_eq!(cut(&daily, from, to), (9, 0, vec![]));
    }

    /// `labelled` states every run's length while the stated take at most half
    /// the row; past that fewer, spread evenly, the first and last kept.
    #[test]
    fn a_strip_states_as_many_run_lengths_as_half_its_row_carries() {
        // Nine short lengths fit inside half the row, and every one states.
        let (stated, share) = labelled(&[36; 9], 38, 1112, 14);
        assert!(stated.iter().all(|on| *on));
        // `share` cuts a stated break as wide as its length and the gap.
        let wide = 1112.0 * share / (29.0 + 9.0 * share);
        assert!((wide - 50.0).abs() < 0.5, "{wide}");
        // Twelve, the last one wider: six state, the first and the last among them.
        let mut widths = vec![36; 11];
        widths.push(72);
        let (stated, _) = labelled(&widths, 30, 1128, 14);
        assert_eq!(stated.iter().filter(|on| **on).count(), 6);
        assert!(stated[0] && stated[11]);
        // `room` too narrow for one: none states, and every break keeps one column.
        assert_eq!(labelled(&[72], 3, 100, 14), (vec![false], 1.0));
    }

    /// `cut_to` asks for a grain after the runs are cut.
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
        // `most` under 41 columns steps the grain and holds the cut.
        let (values, _, breaks, _, grain) = cut_to(&days, from, to, 20, week);
        assert_eq!(breaks.len(), 1);
        assert!(values.len() <= 20, "{} columns", values.len());
        assert_eq!(grain, Grain::Week);
    }

    /// `values` holds every second of the reading, cut or not, and a break
    /// column none of them.
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
        // `open` to `open`, carried through, states that day once.
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
