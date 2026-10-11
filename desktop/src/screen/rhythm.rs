//! A year and All Time: the strips of days, each week cut into its books, what
//! was read and finished, and the Kindle's four trends and board.

use std::fmt::Write as _;

use readinglog_native::date;
use readinglog_native::lang::counted;
use readinglog_native::stats::SITTING_BANDS;
use readinglog_native::view::book as book_view;
use readinglog_native::view::{alltime, rhythm};

use super::chart::{self, Weeks};
use super::look::hex;
use super::{Ctx, Route, esc, lang};

/// `year`: its four figures, the strip, the weeks cut by book, what was read
/// longest first, and what was finished.
pub(super) fn year(cx: &Ctx, year: i64) -> String {
    let s = cx.s;
    let weeks = Weeks::of(year, cx.week, 30.0, cx.width);
    let days = weeks.first..=weeks.last;
    let mut out = format!(
        "<header class=\"head\"><h1>{year}</h1></header>{}<div class=\"card\">{}</div>",
        lede(cx, days.clone()),
        chart::strip(cx.stats, &weeks, cx.week, s, &cx.look, cx.today, true)
    );
    let _ = write!(
        out,
        "<section><h2>{}<span class=\"said\">{}</span></h2><div class=\"card\">{}</div></section>",
        esc(cx.w.by_week),
        esc(&date::duration(cx.stats.span_seconds(days.clone()), s)),
        chart::weeks_by_book(cx.stats, &weeks, s, &cx.look, cx.tones)
    );
    out.push_str(&what_was_read(cx, days.clone(), 10));
    out.push_str(&finished(cx, days));
    out
}

/// `rhythm::tallied` over `days`, as one line.
fn lede(cx: &Ctx, days: std::ops::RangeInclusive<i64>) -> String {
    let mut out = String::from("<p class=\"lede\">");
    for (n, (value, name)) in rhythm::tallied(cx.stats, days, cx.s).iter().enumerate() {
        if n > 0 {
            out.push_str("<span class=\"sep\">·</span>");
        }
        let _ = write!(out, "<b>{}</b> {}", esc(value), esc(name));
    }
    out.push_str("</p>");
    out
}

/// The books read over `days`, longest first, `top` of them each a bar as
/// long as its share of the first, and how many more there were.
fn what_was_read(cx: &Ctx, days: std::ops::RangeInclusive<i64>, top: usize) -> String {
    let s = cx.s;
    let totals = cx.stats.book_totals(days);
    let Some(first) = totals.first().map(|(_, secs)| *secs) else {
        return String::new();
    };
    let mut out = format!(
        "<section><h2>{}<span class=\"said\">{}</span></h2><div class=\"card ranks\">",
        esc(s.what_was_read),
        esc(&counted(s.n_books, totals.len() as i64))
    );
    for (at, secs) in totals.iter().take(top) {
        let book = &cx.stats.books[*at];
        let _ = write!(
            out,
            "<a class=\"rank\" href=\"{}\"><div class=\"thumb cover\">{}</div>\
             <div class=\"what\"><p class=\"t\"{}>{}</p><p class=\"a\">{}</p></div>\
             <div class=\"track\"><i style=\"width: {:.1}%; background: {}\"></i></div><b>{}</b></a>",
            cx.book_link(*at),
            cx.cover(*at),
            lang(&book.language),
            esc(&book.title),
            esc(&book.author),
            100.0 * *secs as f32 / first.max(1) as f32,
            hex(cx.tones[*at]),
            esc(&date::duration(*secs, s))
        );
    }
    if totals.len() > top {
        let rest: i64 = totals[top..].iter().map(|(_, secs)| secs).sum();
        let _ = write!(
            out,
            "<p class=\"more\">+ {} · {}</p>",
            esc(&counted(s.n_books, (totals.len() - top) as i64)),
            esc(&date::duration(rest, s))
        );
    }
    out.push_str("</div></section>");
    out
}

/// The covers of the books read through over `days`, in the order each was
/// finished, the day under each.
fn finished(cx: &Ctx, days: std::ops::RangeInclusive<i64>) -> String {
    let mut done: Vec<usize> = cx.stats.finished_in(days).collect();
    if done.is_empty() {
        return String::new();
    }
    done.sort_by_key(|at| cx.stats.books[*at].last_day);
    let mut out = format!(
        "<section><h2>{}<span class=\"said\">{}</span></h2><div class=\"shelf\">",
        esc(cx.s.shelf_finished),
        done.len()
    );
    for at in done {
        let _ = write!(
            out,
            "<a class=\"spine\" href=\"{}\"><div class=\"cover\">{}</div><p>{}</p></a>",
            cx.book_link(at),
            cx.cover(at),
            esc(&date::short_day(cx.stats.books[at].last_day, cx.s))
        );
    }
    out.push_str("</div></section>");
    out
}

/// Every year of `years` as a strip with its reading, then the four trends,
/// then the board.
pub(super) fn all_time(cx: &Ctx, years: &[i64]) -> String {
    let s = cx.s;
    let mut out = format!(
        "<header class=\"head\"><h1>{}</h1></header>{}<div class=\"card years\">",
        esc(s.all_time),
        lede(cx, 0..=cx.today)
    );
    for (n, year) in years.iter().enumerate() {
        let weeks = Weeks::of(*year, cx.week, 30.0, cx.width - 110.0);
        let read = cx.stats.span_seconds(weeks.first..=weeks.last);
        let _ = write!(
            out,
            "<div class=\"year\"><a class=\"named\" href=\"{}\"><b>{year}</b><span>{}</span></a>{}</div>",
            Route::Year(*year).link(),
            esc(&date::duration_coarse(read, s)),
            chart::strip(cx.stats, &weeks, cx.week, s, &cx.look, cx.today, n == 0)
        );
    }
    out.push_str("</div>");
    out.push_str(&trends(cx));
    let board = alltime::board(cx.stats, cx.today, cx.week, s);
    out.push_str("<section><div class=\"card board\">");
    for (value, name) in board.iter().filter(|(value, _)| value != book_view::DASH) {
        let _ = write!(
            out,
            "<div><dd>{}</dd><dt>{}</dt></div>",
            esc(value),
            esc(name)
        );
    }
    out.push_str("</div></section>");
    out
}

/// The Kindle's four trends, two to a row: the average day and week, the
/// months, and the sittings by length.
fn trends(cx: &Ctx) -> String {
    let s = cx.s;
    let half = (cx.width - 24.0) / 2.0 - 40.0;
    let day = cx.stats.average_day(cx.today);
    let hours: Vec<String> = (0..24).map(|h| format!("{h:02}")).collect();
    let week = cx.stats.average_week(cx.today, cx.week);
    let weekdays: Vec<String> = (0..7)
        .map(|at| s.weekdays_short[cx.week.day_in(at)].to_string())
        .collect();
    let month = cx.stats.by_month(cx.today);
    let months: Vec<String> = s.months_short.iter().map(|m| m.to_string()).collect();
    let bands = cx.stats.sitting_bands();
    let first = cx.stats.first_band();
    let lengths: Vec<String> = (0..bands.len())
        .map(|at| match first + at + 1 == SITTING_BANDS {
            true => format!("{}+", (first + at) * 5),
            false => format!("{}", (first + at + 1) * 5),
        })
        .collect();
    let busiest = bands
        .iter()
        .enumerate()
        .max_by_key(|(_, n)| **n)
        .filter(|(_, n)| **n > 0)
        .map(|(at, _)| at);
    let panels = [
        (
            s.an_average_day,
            date::duration(day.each, s),
            chart::fold(&day, &hours, 3, s, &cx.look, half),
        ),
        (
            s.an_average_week,
            date::duration(week.each, s),
            chart::fold(&week, &weekdays, 1, s, &cx.look, half),
        ),
        (
            s.by_month,
            date::duration_coarse(month.each, s),
            chart::fold(&month, &months, 1, s, &cx.look, half),
        ),
        (
            s.sitting_lengths,
            format!("{} {}", bands.iter().sum::<i64>(), s.in_all),
            chart::columns(
                &bands,
                &lengths,
                2,
                busiest,
                &|n| n.to_string(),
                &cx.look,
                half,
                120.0,
            ),
        ),
    ];
    let mut out = String::new();
    for pair in panels.chunks(2) {
        out.push_str("<section class=\"split\">");
        for (name, said, svg) in pair {
            let _ = write!(
                out,
                "<div><h2>{}<span class=\"said\">{}</span></h2><div class=\"card\">{svg}</div></div>",
                esc(name),
                esc(said)
            );
        }
        out.push_str("</section>");
    }
    out
}
