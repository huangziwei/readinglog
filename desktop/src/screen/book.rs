//! One book: its cover and headline figures, its last reading as the Graphs
//! tab draws it, the clock, the rest of its figures, and its highlights.

use std::fmt::Write as _;

use readinglog_native::date;
use readinglog_native::view::book as book_view;
use readinglog_native::view::book_graphs;
use readinglog_native::view::highlights as highlights_view;

use super::chart::{self, BookStrip};
use super::look::{Look, hex};
use super::{Ctx, esc, lang};

pub(super) fn page(cx: &Ctx, at: usize) -> String {
    let s = cx.s;
    let book = &cx.stats.books[at];
    let tone = cx.tones[at];
    let percent = match book.is_finished() {
        true => 100,
        false => book.percent_shown().max(0),
    };
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
    let strip = BookStrip::of(cx.stats, at, cx.week, s, cx.width);
    if let Some(strip) = &strip {
        let reads = cx.stats.book_readings(at).len();
        let whole = book_graphs::whole(reads - 1, reads, book.is_finished());
        let _ = write!(
            out,
            "<section><h2>{}<span class=\"said\">{}</span></h2><div class=\"card\">{}</div></section>\
             <section><h2>{}</h2><div class=\"card\">{}</div></section>",
            esc(s.the_days),
            esc(&book_graphs::spanned(strip.from, strip.to, whole, s)),
            chart::book_days(strip, s, &cx.look, tone),
            esc(s.where_you_sat),
            chart::book_sat(cx.stats, at, strip, s, &cx.look, tone)
        );
    }
    let half = (cx.width - 24.0) / 2.0 - 40.0;
    let (from, to) = strip
        .as_ref()
        .map_or((book.first_day, book.last_day), |c| (c.from, c.to));
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
    let rows = book_view::reading_rows(book, cx.from, s);
    for (name, value) in rows.iter().filter(|(_, value)| value != book_view::DASH) {
        let _ = write!(
            out,
            "<div><dt>{}</dt><dd>{}</dd></div>",
            esc(name),
            esc(value)
        );
    }
    out.push_str("</dl></div></div></section>");
    let held = cx.stats.book_highlights(at);
    if !held.is_empty() {
        let _ = write!(
            out,
            "<section><h2>{}<span class=\"said\">{}</span></h2><div class=\"highlights\">",
            esc(s.highlights_tab),
            held.len()
        );
        for paired in &held {
            out.push_str(&highlight(cx, at, paired.highlight, paired.note));
        }
        out.push_str("</div></section>");
    }
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
