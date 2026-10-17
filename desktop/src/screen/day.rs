//! One day, as the Kindle's Today and a day picked off Rhythm state it: three
//! figures, the twenty-four hours with the reading along them, and each book
//! read on it a row; and the highlights made on it.

use std::fmt::Write as _;

use readinglog_native::date;
use readinglog_native::view::{covered, highlights_said, home, rhythm};

use super::{Ctx, Route, chart, esc, lang};

/// `day` in full: `today` states `home::today_figures`, the current streak
/// among them, and a picked day `rhythm::day_figures`.
pub(super) fn page(cx: &Ctx, day: i64, today: bool) -> String {
    let s = cx.s;
    let mut out = super::rhythm::head(cx, None, day);
    out.push_str(&nav(cx, day));
    let stated = match today {
        true => home::today_figures(cx.stats, day, s),
        false => rhythm::day_figures(cx.stats, day, s),
    };
    out.push_str("<dl class=\"figures day\">");
    for (value, name) in &stated {
        let _ = write!(
            out,
            "<div><dt>{}</dt><dd>{}</dd></div>",
            esc(name),
            esc(value)
        );
    }
    out.push_str("</dl>");
    let _ = write!(
        out,
        "<div class=\"card\">{}</div>",
        chart::timeline(&cx.stats.day_blocks(day), &cx.look, cx.width, s)
    );
    out.push_str(&books(cx, day));
    out.push_str(&highlights(cx, day));
    out
}

/// The day's name between the arrows onto the day either side, an arrow
/// standing only where the record holds a day that way, and `s.now` where
/// `day` is not `cx.today`.
fn nav(cx: &Ctx, day: i64) -> String {
    let arrow = |to: i64, key: &str, said: &str, open: bool| match open {
        true => format!(
            "<a class=\"arrow\" data-key=\"{key}\" href=\"{}\">{said}</a>",
            Route::Day(to).link()
        ),
        false => format!("<span class=\"arrow off\">{said}</span>"),
    };
    let now = match day == cx.today {
        true => String::new(),
        false => format!(
            "<a class=\"chip\" href=\"{}\">{}</a>",
            Route::Day(cx.today).link(),
            esc(cx.s.now)
        ),
    };
    format!(
        "<div class=\"spannav\">{}<h1>{}</h1>{}{now}</div>",
        arrow(day - 1, "prev", "‹", day > cx.opened),
        esc(&date::long_day(day, cx.s)),
        arrow(day + 1, "next", "›", day < cx.today)
    )
}

/// The books read on `day`, longest first, a row each as the Kindle's day
/// list draws it, and the unidentified books' note under them.
fn books(cx: &Ctx, day: i64) -> String {
    let s = cx.s;
    let mut read = cx.stats.book_totals(day..=day);
    covered(cx.stats, cx.uncovered, &mut read);
    let mut out = format!(
        "<section><h2>{}<span class=\"said\">{}</span></h2>",
        esc(s.what_was_read),
        esc(&date::duration(cx.stats.day_seconds(day), s))
    );
    if read.is_empty() {
        let _ = write!(out, "<p class=\"lede\">{}</p>", esc(s.nothing_read));
    } else {
        out.push_str("<div class=\"card dayrows\">");
        for (at, secs) in &read {
            out.push_str(&row(cx, day, *at, *secs));
        }
        out.push_str("</div>");
    }
    out.push_str(&cx.unidentified(day..=day));
    out.push_str("</section>");
    out
}

/// One book of `day`: its cover, title, author, Kindle and that day's
/// highlights, the time read, the band at the day's close, and its own
/// stretches under the day's hours.
fn row(cx: &Ctx, day: i64, at: usize, secs: i64) -> String {
    let s = cx.s;
    let book = &cx.stats.books[at];
    let marks = highlights_said(s, cx.stats.highlights_counted_on(at, day));
    let author = match book.author.is_empty() {
        true => String::new(),
        false => format!(
            "<span{}>{}</span>",
            lang(&book.language, &book.author),
            esc(&book.author)
        ),
    };
    let kindle = cx.kindle(at).map(esc).unwrap_or_default();
    let under = [author, kindle, esc(&marks)]
        .into_iter()
        .filter(|said| !said.is_empty())
        .collect::<Vec<_>>()
        .join(" · ");
    let band = match (
        cx.stats.percent_over(at, day..=day),
        cx.stats.finished_on(at, day),
    ) {
        (_, true) => format!(
            "<span class=\"band\"><i style=\"width: 100%\"></i></span><b class=\"done\">{}</b>",
            esc(s.shelf_finished)
        ),
        (Some(percent), false) => format!(
            "<span class=\"band\"><i style=\"width: {}%\"></i></span><b>{}</b>",
            percent.clamp(0, 100),
            esc(&s.percent_plain.replace("{d}", &percent.to_string()))
        ),
        (None, false) => String::new(),
    };
    format!(
        "<a class=\"dayrow\" href=\"{}\"><div class=\"thumb cover\">{}</div><div class=\"words\">\
         <p class=\"t\"{}>{}</p><p class=\"a\">{under}</p><div class=\"meter\">{band}</div></div>\
         <b class=\"time\">{}</b><div class=\"spans\">{}</div></a>",
        cx.book_link(at),
        cx.cover(at),
        lang(&book.language, &book.title),
        esc(&book.title),
        esc(&date::duration(secs, s)),
        chart::spans(
            &cx.stats.day_blocks_of(day, Some(at)),
            &cx.look,
            cx.tones[at],
            cx.width
        )
    )
}

/// The highlights whose passage or a note on it was made on `day`, newest first,
/// each book's run as `journal::run` sets it over `day`, the count beside the
/// heading's name.
fn highlights(cx: &Ctx, day: i64) -> String {
    let made = |at: &str| date::parse_day(date::day_of(at)) == Some(day);
    let rows: Vec<_> = cx
        .stats
        .highlights_across()
        .into_iter()
        .filter(|row| made(&row.highlight.at) || row.notes.iter().any(|n| made(&n.at)))
        .collect();
    if rows.is_empty() {
        return String::new();
    }
    let mut out = format!(
        "<section><h2><span>{}<span class=\"count\">{}</span></span></h2>",
        esc(cx.s.journal_tab),
        rows.len()
    );
    for held in rows.chunk_by(|a, b| a.book == b.book) {
        out.push_str(&super::journal::run(cx, held, "", Some(day..=day)));
    }
    out.push_str("</section>");
    out
}
