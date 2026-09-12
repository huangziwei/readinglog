//! One book's reading drawn: where it stood as each sitting ended, how long it
//! took day by day, and which hours of the day it was read in.

use crate::date;
use crate::lang::{self, Strings};
use crate::stats::Stats;
use crate::ui::charts;
use crate::ui::chrome;
use crate::ui::paint::Rect;
use crate::ui::theme::Theme;

use super::Ctx;

/// The most columns the journey is cut into.
const SPAN_COLUMNS: i64 = 30;

/// The per cent a full-height bar of the place band stands for.
const WHOLE_BOOK: i64 = 100;

/// The fewest sittings carrying a `progress` before the place band draws.
const PLACES: usize = 2;

/// One hour of the clock in this many is named, as `alltime::trends` names it.
const HOURS_NAMED: usize = 3;

/// One band of the page, as [`charts::columns`] takes it.
struct Band {
    title: String,
    values: Vec<i64>,
    axis: Vec<String>,
    figure: Box<dyn Fn(i64) -> Vec<String>>,
    every: usize,
    ceiling: Option<i64>,
}

/// The whole box under the head row, cut into equal bands. A band with nothing
/// to draw is left out and the rest take its height.
pub fn draw(cx: &mut Ctx, area: Rect, index: usize) {
    let s = cx.s();
    let bands = bands(cx.stats, index, s);
    let hours = cx.stats.book_hours(index);
    let clock = hours.iter().any(|secs| *secs > 0);
    let deep = bands.len() as i32 + clock as i32;
    if deep == 0 {
        return;
    }
    let rows = area.rows(deep, cx.theme.gap * 2);
    for (band, row) in bands.iter().zip(&rows) {
        let theme: &Theme = cx.theme;
        let inner = chrome::section(cx.fb, cx.text, theme, *row, &band.title);
        charts::columns(
            cx.fb,
            cx.text,
            theme,
            cx.palette,
            inner,
            &band.values,
            |at| band.axis[at].clone(),
            &|value| (band.figure)(value),
            band.every,
            None,
            band.ceiling,
        );
    }
    // `alltime::band` draws `hours` as it draws its own average day.
    if let Some(row) = rows.last().filter(|_| clock) {
        let fold = cx.stats.fold(hours.to_vec(), hours.iter().sum());
        let names: Vec<String> = (0..24).map(|at| format!("{at:02}")).collect();
        super::alltime::band(cx, *row, s.the_clock, &fold, &names, HOURS_NAMED);
    }
}

/// The bands this book has the record for, in the order the page stacks them.
fn bands(stats: &Stats, index: usize, s: &'static Strings) -> Vec<Band> {
    let mut out: Vec<Band> = Vec::new();
    let Some(book) = stats.books.get(index) else {
        return out;
    };

    let places = stats.book_places(index);
    if places.iter().filter(|(_, at)| at.is_some()).count() >= PLACES {
        let axis = places.iter().map(|(day, _)| *day).collect::<Vec<i64>>();
        // A sitting with no `progress` holds the place before it.
        let mut place = 0;
        let values: Vec<i64> = places
            .iter()
            .map(|(_, at)| {
                place = at.unwrap_or(place);
                place
            })
            .collect();
        out.push(Band {
            title: lang::counted(s.the_place, book.sittings),
            axis: axis.iter().map(|day| date::short_day(*day, s)).collect(),
            every: (values.len() / 4).max(1),
            values,
            figure: Box::new(move |at| vec![s.percent_plain.replace("{d}", &at.to_string())]),
            ceiling: Some(WHOLE_BOOK),
        });
    }

    // `opened` and `closed` are the book's own stretch, never the record's.
    let (opened, closed) = (book.first_day, book.last_day);
    let span = (closed - opened + 1).max(1);
    let (series, each) = journey(stats, index, opened, closed);
    if series.iter().any(|secs| *secs > 0) {
        out.push(Band {
            title: lang::counted(s.the_journey, span),
            axis: (0..series.len())
                .map(|at| date::short_day(opened + at as i64 * each, s))
                .collect(),
            every: (series.len() / 4).max(1),
            values: series,
            figure: Box::new(move |secs| super::alltime::duration_rows(secs, s)),
            ceiling: None,
        });
    }

    out
}

/// The seconds read in each column of the strip, and the days one column
/// covers. A book read over [`SPAN_COLUMNS`] days or fewer gets a column each.
fn journey(stats: &Stats, index: usize, opened: i64, closed: i64) -> (Vec<i64>, i64) {
    let span = (closed - opened + 1).max(1);
    let each = (span + SPAN_COLUMNS - 1) / SPAN_COLUMNS;
    let columns = ((span + each - 1) / each).max(1) as usize;
    let mut series = vec![0i64; columns];
    for (day, secs) in stats.book_days(index) {
        let at = ((day - opened) / each).clamp(0, columns as i64 - 1) as usize;
        series[at] += secs;
    }
    (series, each)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::lang::Lang;
    use crate::log::session::Measure;
    use crate::stats::{BookStat, Sitting, Stats};

    /// A record of one book whose sittings ended at each of `places`, one a day.
    /// `BookStat::sittings` counts all of them, `None` or not.
    fn read(places: &[Option<f64>]) -> Stats {
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
            ..BookStat::default()
        };
        Stats {
            sittings,
            books: vec![book],
            ..Stats::default()
        }
    }

    /// The bars the place band draws for the one book `read` built.
    fn drawn(stats: &Stats) -> Vec<i64> {
        bands(stats, 0, Lang::English.strings())[0].values.clone()
    }

    /// The five shapes the record holds, none of them a climb.
    const SHAPES: [&[i64]; 5] = [
        &[89, 6, 9, 12, 15],
        &[85, 18, 11, 21],
        &[
            1, 7, 13, 20, 33, 33, 40, 51, 3, 51, 0, 55, 64, 67, 85, 89, 100,
        ],
        &[8, 44, 0],
        &[9, 13, 22, 34, 43, 55, 100, 98, 100, 100],
    ];

    #[test]
    fn the_place_band_draws_a_series_that_does_not_climb_as_it_stands() {
        for shape in SHAPES {
            let places: Vec<Option<f64>> =
                shape.iter().map(|at| Some(*at as f64 / 100.0)).collect();
            let stats = read(&places);
            let drawn = drawn(&stats);
            assert_eq!(drawn, shape, "the band states the record's own places");
            // `drawn` stands between the foot and `WHOLE_BOOK`.
            assert!(drawn.iter().all(|at| (0..=WHOLE_BOOK).contains(at)));
        }
    }

    #[test]
    fn a_sitting_with_no_place_holds_the_place_before_it() {
        let stats = read(&[Some(0.10), None, Some(0.40), None, Some(0.25)]);
        assert_eq!(drawn(&stats), [10, 10, 40, 40, 25]);
        // The first sitting has no place before it and opens at 0.
        let stats = read(&[None, Some(0.40), Some(0.55)]);
        assert_eq!(drawn(&stats), [0, 40, 55]);
    }

    #[test]
    fn a_book_under_two_known_places_draws_no_place_band() {
        let s = Lang::English.strings();
        for places in [vec![], vec![Some(0.40)], vec![None, Some(0.40), None]] {
            let stats = read(&places);
            assert!(bands(&stats, 0, s).iter().all(|b| b.ceiling.is_none()));
        }
        // `PLACES` places draw the band.
        let stats = read(&[Some(0.10), Some(0.40)]);
        assert_eq!(bands(&stats, 0, s)[0].values.len(), PLACES);
    }

    /// `bands` draws one bar for each of [`Stats::book_sittings`] and heads
    /// them with `BookStat::sittings`, the count `view::book` states.
    #[test]
    fn the_place_band_draws_a_bar_for_every_sitting() {
        let places = [Some(0.10), Some(0.22), None, Some(0.40)];
        let stats = read(&places);
        let s = Lang::English.strings();
        let band = &bands(&stats, 0, s)[0];

        assert_eq!(stats.books[0].sittings, places.len() as i64);
        assert_eq!(band.values.len() as i64, stats.books[0].sittings);
        assert_eq!(band.axis.len() as i64, stats.books[0].sittings);
        assert_eq!(&band.title, "THE PLACE · 4 SITTINGS");
        assert_eq!(band.values, [10, 22, 22, 40]);
    }

    #[test]
    fn the_clock_holds_every_hour_this_book_was_read_in() {
        let stats = read(&[Some(0.10), Some(0.40)]);
        let hours = stats.book_hours(0);
        assert_eq!(hours[1], 3600, "both sittings fell in the same hour");
        assert_eq!(hours.iter().sum::<i64>(), 3600);
        assert_eq!(hours.len(), 24);
    }
}
