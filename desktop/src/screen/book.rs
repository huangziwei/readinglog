//! One book, the Kindle's three tabs on one page: its cover and headline
//! figures, the reading picked as the Graphs tab draws it, its rows of
//! figures, and its highlights in either order and narrowed by a search.

use std::fmt::Write as _;

use readinglog_native::date;
use readinglog_native::view::Way;
use readinglog_native::view::book as book_view;
use readinglog_native::view::book_graphs;
use readinglog_native::view::highlights as highlights_view;

use super::chart::{self, BookStrip};
use super::look::{Look, hex};
use super::{Ctx, Route, esc, key_of, lang};

pub(super) fn page(cx: &Ctx, at: usize, reading: Option<usize>, way: Way, query: &str) -> String {
    let s = cx.s;
    let book = &cx.stats.books[at];
    let tone = cx.tones[at];
    let percent = book.bar_percent().max(0);
    let standing = match (book.is_finished(), book.has_percent()) {
        (true, _) => s.shelf_finished.to_string(),
        (false, true) => s.percent_plain.replace("{d}", &percent.to_string()),
        (false, false) => String::new(),
    };
    let mut out = format!(
        "<div class=\"hero\"><div class=\"cover\">{}</div><div class=\"words\"><h1{}>{}</h1><p class=\"author\">{}</p>\
         <div class=\"meter\"><div class=\"track\"><i style=\"width: {percent}%; background: {}\"></i></div><b>{}</b></div>\
         <dl class=\"figures\">",
        cx.cover(at),
        lang(&book.language),
        esc(&book.title),
        esc(&book.author),
        hex(tone),
        esc(&standing)
    );
    // `s.left` stands only on a book short of `BookStat::is_finished`.
    let stated = book_view::headline(book, cx.from, s);
    let told = stated
        .iter()
        .filter(|(value, _)| value != book_view::DASH)
        .filter(|(_, name)| !(book.is_finished() && *name == s.left));
    for (value, name) in told {
        let _ = write!(
            out,
            "<div><dt>{}</dt><dd>{}</dd></div>",
            esc(name),
            esc(value)
        );
    }
    out.push_str("</dl></div></div>");
    let reads = cx.stats.book_readings(at);
    let picked = reads
        .len()
        .checked_sub(1)
        .map(|last| reading.unwrap_or(last).min(last));
    if reads.len() > 1 {
        out.push_str(&readings(cx, at, &reads, picked.unwrap_or(0), way, query));
    }
    if let Some(on) = picked {
        let strip = BookStrip::of(cx.stats, at, reads[on], cx.week, s, cx.width);
        let whole = book_graphs::whole(on, reads.len(), book.is_finished());
        let _ = write!(
            out,
            "<section><h2>{}<span class=\"said\">{} · {}</span></h2><div class=\"card\">{}</div></section>\
             <section><h2>{}</h2><div class=\"card\">{}</div></section>",
            esc(s.the_days),
            esc(&date::duration(
                cx.stats.book_seconds_over(at, strip.from..=strip.to),
                s
            )),
            esc(&book_graphs::spanned(strip.from, strip.to, whole, s)),
            chart::book_days(&strip, s, &cx.look, tone),
            esc(s.where_you_sat),
            chart::book_sat(cx.stats, at, &strip, s, &cx.look, tone)
        );
    }
    let half = (cx.width - 24.0) / 2.0 - 40.0;
    let (from, to) = picked.map_or((book.first_day, book.last_day), |on| reads[on]);
    let hours = cx.stats.book_hours(at, from..=to);
    let names: Vec<String> = (0..24).map(|h| format!("{h:02}")).collect();
    let busiest = hours
        .iter()
        .enumerate()
        .max_by_key(|(_, secs)| **secs)
        .filter(|(_, secs)| **secs > 0)
        .map(|(at, _)| at);
    let toned = Look {
        bar: tone,
        ..cx.look
    };
    let _ = write!(
        out,
        "<section class=\"split\"><div><h2>{}</h2><div class=\"card\">{}</div></div>\
         <div><h2>{}</h2><div class=\"card\"><dl class=\"rows\">",
        esc(s.the_clock),
        chart::columns(
            &hours,
            &names,
            3,
            busiest,
            &|secs| date::duration(secs, s),
            &toned,
            half,
            140.0
        ),
        esc(s.the_reading)
    );
    let mut rows = book_view::reading_rows(book, cx.from, s);
    rows.push((s.on_the_kindle, book_view::where_note(book, s)));
    for (name, value) in rows.iter().filter(|(_, value)| value != book_view::DASH) {
        let _ = write!(
            out,
            "<div><dt>{}</dt><dd>{}</dd></div>",
            esc(name),
            esc(value)
        );
    }
    out.push_str("</dl></div></div></section>");
    out.push_str(&marks(cx, at, reading, way, query));
    out
}

/// Every reading the book has been given, each its span and the time it
/// took, the one `picked` lit and each a link onto its own graphs.
fn readings(
    cx: &Ctx,
    at: usize,
    reads: &[(i64, i64)],
    picked: usize,
    way: Way,
    query: &str,
) -> String {
    let s = cx.s;
    let book = &cx.stats.books[at];
    let mut out = format!(
        "<section><h2>{}<span class=\"said\">{}</span></h2><div class=\"chips\">",
        esc(s.read_through_band),
        reads.len()
    );
    for (on, (from, to)) in reads.iter().enumerate() {
        let whole = book_graphs::whole(on, reads.len(), book.is_finished());
        let to_route = Route::Book {
            key: key_of(book),
            reading: Some(on),
            way,
            query: query.to_string(),
        };
        let said = format!(
            "{} · {}",
            date::duration(cx.stats.book_seconds_over(at, *from..=*to), s),
            book_graphs::spanned(*from, *to, whole, s)
        );
        super::chip(&mut out, &to_route.link(), &said, on == picked);
    }
    out.push_str("</div></section>");
    out
}

/// The book's highlights as its Highlights tab lists them through
/// `highlights::listing`: newest first or turned over, narrowed by `query`.
fn marks(cx: &Ctx, at: usize, reading: Option<usize>, way: Way, query: &str) -> String {
    let s = cx.s;
    let book = &cx.stats.books[at];
    if cx.stats.highlights_held(at) == 0 {
        return String::new();
    }
    let listed = highlights_view::listing(
        cx.stats,
        at,
        book,
        way,
        (!query.is_empty()).then_some(query),
    );
    let flipped = Route::Book {
        key: key_of(book),
        reading,
        way: way.flipped(),
        query: query.to_string(),
    };
    let mut out = format!(
        "<section><h2>{}<span class=\"said\">{} <a class=\"chip\" href=\"{}\">{}</a></span></h2>\
         <input class=\"search wide\" type=\"search\" data-search=\"book\" value=\"{}\" placeholder=\"{}\">\
         <div class=\"highlights\">",
        esc(s.highlights_tab),
        listed.len(),
        flipped.link(),
        way.caret(),
        esc(query),
        esc(s.search_hint_highlights)
    );
    if listed.is_empty() {
        let _ = write!(out, "<p class=\"lede\">{}</p>", esc(s.no_highlights_here));
    }
    for row in &listed {
        out.push_str(&highlight(cx, at, &row.highlight, row.note.as_ref()));
    }
    out.push_str("</div></section>");
    out
}

/// One highlight of the book at `at`: its passage, the note written on it,
/// and where and when it was made.
pub(super) fn highlight(
    cx: &Ctx,
    at: usize,
    highlight: &readinglog_native::highlights::Highlight,
    note: Option<&readinglog_native::highlights::Highlight>,
) -> String {
    let book = &cx.stats.books[at];
    let mut out = format!(
        "<div class=\"highlight\"><blockquote{}>{}</blockquote>",
        lang(&book.language),
        esc(&highlight.body)
    );
    if let Some(note) = note {
        let _ = write!(out, "<p class=\"note\">{}</p>", esc(&note.body));
    }
    let made = date::parse_day(date::day_of(&highlight.at))
        .map(|day| format!(" · {}", date::short_day(day, cx.s)))
        .unwrap_or_default();
    let _ = write!(
        out,
        "<p class=\"where\">{}{}</p></div>",
        esc(&highlights_view::label(highlight, book.extent, cx.s)),
        esc(&made)
    );
    out
}
