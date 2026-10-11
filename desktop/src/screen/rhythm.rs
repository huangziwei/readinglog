//! Rhythm as the Kindle's states it at each zoom: the picker, the span between
//! its arrows, what it came to, its grid — a week's bars, a month's calendar, a
//! year's strip — and what was read; and All Time, a strip a year over its board.

use std::fmt::Write as _;

use readinglog_native::date;
use readinglog_native::lang::counted;
use readinglog_native::stats::SITTING_BANDS;
use readinglog_native::view::book as book_view;
use readinglog_native::view::{Shelf, Span, Window, alltime, covered, rhythm};

use super::chart::{self, Weeks};
use super::{Ctx, Route, esc, lang};

/// The span picker: All Time, Year, Month and Week as the Kindle orders them,
/// each a link onto that span around `day`, `on` lit.
pub(super) fn head(cx: &Ctx, on: Option<Span>, day: i64) -> String {
    let mut out = String::from("<div class=\"chips picker\">");
    for span in Span::ALL {
        let to = match span {
            Span::AllTime => Route::Span(Span::AllTime, 0),
            other => Route::Span(other, day),
        };
        super::chip(&mut out, &to.link(), span.label(cx.lang), on == Some(span));
    }
    out.push_str("</div>");
    out
}

/// The span holding `day`: its name between the arrows, what it came to, its
/// grid, and what was read over it.
pub(super) fn span(cx: &Ctx, span: Span, day: i64) -> String {
    let days = span.days(day, cx.week);
    let mut out = head(cx, Some(span), day);
    out.push_str(&nav(cx, span, day));
    out.push_str(&lede(cx, span, day));
    let grid = match span {
        Span::Week => chart::week(
            cx.stats,
            *days.start(),
            cx.today,
            &cx.look,
            cx.s,
            cx.width,
            &|d| Route::Day(d).link(),
        ),
        Span::Month => chart::month(
            cx.stats,
            *days.start(),
            cx.week,
            cx.today,
            &cx.look,
            cx.s,
            cx.tones,
            cx.width,
            &|d| Route::Day(d).link(),
        ),
        _ => year_strip(cx, date::civil_from_days(day).0, cx.width, true),
    };
    let _ = write!(out, "<div class=\"card\">{grid}</div>");
    out.push_str(&what_was_read(cx, days));
    out
}

/// The span's name between the arrows onto the span either side, an arrow
/// standing only where the record reaches that way, and `s.now` where the span
/// does not hold the record's newest day.
fn nav(cx: &Ctx, span: Span, day: i64) -> String {
    let days = span.days(day, cx.week);
    let arrow = |by: i64, key: &str, said: &str, open: bool| match open {
        true => format!(
            "<a class=\"arrow\" data-key=\"{key}\" href=\"{}\">{said}</a>",
            Route::Span(span, span.step(day, by)).link()
        ),
        false => format!("<span class=\"arrow off\">{said}</span>"),
    };
    let now = match days.contains(&cx.today) {
        true => String::new(),
        false => format!(
            "<a class=\"chip\" href=\"{}\">{}</a>",
            Route::Span(span, cx.today).link(),
            esc(cx.s.now)
        ),
    };
    format!(
        "<div class=\"spannav\">{}<h1>{}</h1>{}{now}</div>",
        arrow(-1, "prev", "‹", *days.start() > cx.opened),
        esc(&span.name(day, cx.week, cx.s)),
        arrow(1, "next", "›", *days.end() < cx.today)
    )
}

/// `rhythm::tallied` over the span holding `day`, as one line; its finished
/// books open the Books list under the same window.
fn lede(cx: &Ctx, span: Span, day: i64) -> String {
    let days = span.days(day, cx.week);
    let finished = cx.stats.finished_over(days.clone());
    let mut out = String::from("<p class=\"lede\">");
    for (n, (value, name)) in rhythm::tallied(cx.stats, days, cx.s).iter().enumerate() {
        if n > 0 {
            out.push_str("<span class=\"sep\">·</span>");
        }
        let said = format!("<b>{}</b> {}", esc(value), esc(name));
        match n == 3 && finished > 0 {
            true => {
                let window = Some(Window { span, day });
                let _ = write!(
                    out,
                    "<a href=\"{}\">{said}</a>",
                    Route::shelf(Shelf::Finished, window).link()
                );
            }
            false => out.push_str(&said),
        }
    }
    out.push_str("</p>");
    out
}

/// One year as one strip, each day with reading opening its page and each
/// month's name its month.
fn year_strip(cx: &Ctx, year: i64, width: f32, months: bool) -> String {
    let weeks = Weeks::of(year, cx.week, 30.0, width);
    chart::strip(
        cx.stats,
        &weeks,
        cx.week,
        cx.s,
        &cx.look,
        cx.today,
        months,
        &|d| Route::Day(d).link(),
        &|d| Route::Span(Span::Month, d).link(),
    )
}

/// Every book read over `days` as its cover, most recently put down first, as
/// the Kindle's grid under a span states it: what was read of it there and
/// where it stood as the span closed. [`Ctx::unidentified`] closes it.
fn what_was_read(cx: &Ctx, days: std::ops::RangeInclusive<i64>) -> String {
    let s = cx.s;
    let mut read = cx.stats.book_totals_recent(days.clone());
    covered(cx.stats, cx.uncovered, &mut read);
    let mut out = format!(
        "<section><h2>{}<span class=\"said\">{}</span></h2>",
        esc(s.what_was_read),
        esc(&date::duration(cx.stats.span_seconds(days.clone()), s))
    );
    if read.is_empty() {
        let _ = write!(out, "<p class=\"lede\">{}</p>", esc(s.nothing_read));
    } else {
        out.push_str("<div class=\"grid small\">");
        for (at, secs) in &read {
            let book = &cx.stats.books[*at];
            let percent = cx
                .stats
                .percent_over(*at, days.clone())
                .map(|p| counted(s.percent_reached, p))
                .unwrap_or_default();
            let _ = write!(
                out,
                "<a class=\"tile\" href=\"{}\"><div class=\"cover\">{}</div><p class=\"title\"{}>{}</p>\
                 <p class=\"meta\">{}</p><p class=\"meta\">{}</p></a>",
                cx.book_link(*at),
                cx.cover(*at),
                lang(&book.language),
                esc(&book.title),
                esc(&date::duration_tight(*secs, s)),
                esc(&percent)
            );
        }
        out.push_str("</div>");
    }
    out.push_str(&cx.unidentified(days));
    out.push_str("</section>");
    out
}

/// All Time: what the record covers, a strip a year, the Kindle's four trends,
/// and its board, the finished books, the best day and the longest sitting
/// each opening their own page.
pub(super) fn all_time(cx: &Ctx, years: &[i64]) -> String {
    let s = cx.s;
    let mut out = head(cx, Some(Span::AllTime), cx.today);
    let _ = write!(
        out,
        "<div class=\"spannav\"><h1>{}</h1></div><p class=\"lede\">{}</p><div class=\"card years\">",
        esc(s.all_time),
        esc(&alltime::since(cx.stats, cx.today, s))
    );
    for (n, year) in years.iter().enumerate() {
        let start = date::days_from_civil(*year, 1, 1);
        let read = cx
            .stats
            .span_seconds(start..=date::days_from_civil(*year, 12, 31));
        let _ = write!(
            out,
            "<div class=\"year\"><a class=\"named\" href=\"{}\"><b>{year}</b><span>{}</span></a>{}</div>",
            Route::Span(Span::Year, start).link(),
            esc(&date::duration_coarse(read, s)),
            year_strip(cx, *year, cx.width - 110.0, n == 0)
        );
    }
    out.push_str("</div>");
    out.push_str(&trends(cx));
    let opens: [Option<Route>; 12] = {
        let finished = cx.stats.finished_in(i64::MIN..=i64::MAX).next().is_some();
        let best = cx.stats.busiest_day(i64::MIN..=i64::MAX);
        let longest = cx.stats.longest_sitting();
        let mut opens: [Option<Route>; 12] = Default::default();
        opens[4] = finished.then(|| Route::shelf(Shelf::Finished, None));
        opens[9] = best
            .filter(|(_, secs)| *secs > 0)
            .map(|(day, _)| Route::Day(day));
        opens[10] = longest
            .filter(|(_, secs)| *secs > 0)
            .map(|(day, _)| Route::Day(day));
        opens
    };
    out.push_str("<section><div class=\"card board\">");
    let board = alltime::board(cx.stats, cx.today, cx.week, s);
    for ((value, name), opens) in board.iter().zip(opens) {
        if value == book_view::DASH {
            continue;
        }
        let said = format!("<dd>{}</dd><dt>{}</dt>", esc(value), esc(name));
        match opens {
            Some(to) => {
                let _ = write!(out, "<a href=\"{}\">{said}</a>", to.link());
            }
            None => {
                let _ = write!(out, "<div>{said}</div>");
            }
        }
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
