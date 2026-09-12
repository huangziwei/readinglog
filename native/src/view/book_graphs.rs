//! One book's reading drawn: where it stood as each sitting ended, how long it
//! took day by day, and which hours of the day it was read in.

use crate::date;
use crate::lang::{self, Strings};
use crate::ui::charts;
use crate::ui::chrome;
use crate::ui::paint::Rect;
use crate::ui::theme::Theme;

use super::Ctx;

/// The most columns the journey is cut into.
const SPAN_COLUMNS: i64 = 30;

/// The per cent a full-height bar of the place band stands for.
const WHOLE_BOOK: i64 = 100;

/// The fewest places a book states before the place band draws.
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
    let bands = bands(cx, index, s);
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
    // The clock is `alltime`'s own average-day band over this book's hours:
    // the same heading, ticks and figures the Trends page draws.
    if let Some(row) = rows.last().filter(|_| clock) {
        let fold = cx.stats.fold(hours.to_vec(), hours.iter().sum());
        let names: Vec<String> = (0..24).map(|at| format!("{at:02}")).collect();
        super::alltime::band(cx, *row, s.the_clock, &fold, &names, HOURS_NAMED);
    }
}

/// The bands this book has the record for, in the order the page stacks them.
fn bands(cx: &Ctx, index: usize, s: &'static Strings) -> Vec<Band> {
    let mut out: Vec<Band> = Vec::new();
    let Some(book) = cx.stats.books.get(index) else {
        return out;
    };

    let places = cx.stats.book_places(index);
    if places.len() >= PLACES {
        let axis = places.iter().map(|(day, _)| *day).collect::<Vec<i64>>();
        let values: Vec<i64> = places.iter().map(|(_, at)| *at).collect();
        out.push(Band {
            title: lang::counted(s.the_place, values.len() as i64),
            axis: axis.iter().map(|day| date::short_day(*day, s)).collect(),
            every: (values.len() / 4).max(1),
            values,
            figure: Box::new(move |at| vec![s.percent_plain.replace("{d}", &at.to_string())]),
            ceiling: Some(WHOLE_BOOK),
        });
    }

    // The strip is anchored on the book's own stretch of days and never on
    // `cx.today`: a book put down in the spring states its reading, not an
    // empty summer.
    let (opened, closed) = (book.first_day, book.last_day);
    let span = (closed - opened + 1).max(1);
    let (series, each) = journey(cx, index, opened, closed);
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
fn journey(cx: &Ctx, index: usize, opened: i64, closed: i64) -> (Vec<i64>, i64) {
    let span = (closed - opened + 1).max(1);
    let each = (span + SPAN_COLUMNS - 1) / SPAN_COLUMNS;
    let columns = ((span + each - 1) / each).max(1) as usize;
    let mut series = vec![0i64; columns];
    for (day, secs) in cx.stats.book_days(index) {
        let at = ((day - opened) / each).clamp(0, columns as i64 - 1) as usize;
        series[at] += secs;
    }
    (series, each)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::log::session::Measure;
    use crate::stats::{Sitting, Stats};

    /// A record of one book whose sittings ended at each of `places`, one a day.
    fn read(places: &[Option<f64>]) -> Stats {
        let sittings = places
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
        Stats {
            sittings,
            ..Stats::default()
        }
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
            let drawn: Vec<i64> = stats.book_places(0).iter().map(|(_, at)| *at).collect();
            assert_eq!(drawn, shape, "the band states the record's own places");
            // Every bar stands inside the band: none is over the bound, and
            // none is under the foot.
            assert!(drawn.iter().all(|at| (0..=WHOLE_BOOK).contains(at)));
        }
    }

    #[test]
    fn a_sitting_with_no_place_is_left_out_and_the_rest_keep_their_order() {
        let stats = read(&[Some(0.10), None, Some(0.40), None, Some(0.25)]);
        assert_eq!(
            stats.book_places(0),
            vec![(20_000, 10), (20_002, 40), (20_004, 25)]
        );
    }

    #[test]
    fn a_book_under_two_places_draws_no_place_band() {
        for places in [vec![], vec![Some(0.40)], vec![None, Some(0.40), None]] {
            let stats = read(&places);
            assert!(stats.book_places(0).len() < PLACES);
        }
        // Two is enough.
        let stats = read(&[Some(0.10), Some(0.40)]);
        assert_eq!(stats.book_places(0).len(), PLACES);
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
