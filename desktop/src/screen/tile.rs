//! One book as its cover tile, the one every page laying books out as covers
//! draws: the cover, a bar of its progress, its title, author and Kindle, its
//! time and where it stands, and its highlights.

use std::fmt::Write as _;
use std::ops::RangeInclusive;

use readinglog_native::date;
use readinglog_native::view::highlights_said;

use super::{Ctx, esc, lang};

/// The book at `at` as a tile over the whole record, or over `over`: the time
/// read there, where it stood as `over` closed and the highlights made there.
pub(super) fn tile(cx: &Ctx, at: usize, over: Option<RangeInclusive<i64>>) -> String {
    let s = cx.s;
    let book = &cx.stats.books[at];
    let (secs, finished, percent, marks) = match over {
        None => (
            book.read_seconds(cx.from),
            book.is_finished(),
            book.has_percent().then(|| book.percent_shown()),
            cx.stats.highlights_counted(at),
        ),
        Some(days) => (
            cx.stats.book_seconds_over(at, days.clone()),
            book.is_finished() && days.contains(&book.last_day),
            cx.stats.percent_over(at, days.clone()),
            cx.stats.highlights_counted_over(at, days),
        ),
    };
    let bar = match finished {
        true => 100,
        false => percent.unwrap_or(0).clamp(0, 100),
    };
    let read = date::duration(secs, s);
    let stands = match (finished, percent) {
        (true, _) => format!("{read} · {}", s.shelf_finished),
        (false, Some(p)) => format!(
            "{read} · {}",
            s.percent_plain.replace("{d}", &p.to_string())
        ),
        (false, None) => read,
    };
    let kindle = cx
        .kindle(at)
        .map(|k| format!("<p class=\"meta\">{}</p>", esc(k)))
        .unwrap_or_default();
    let mut out = format!(
        "<a class=\"tile\" href=\"{}\"><div class=\"cover\">{}</div>\
         <div class=\"progress\"><i style=\"width: {bar}%\"></i></div>\
         <p class=\"title\"{}>{}</p><p class=\"author\"{}>{}</p>{kindle}<p class=\"meta\">{}</p>",
        cx.book_link(at),
        cx.cover(at),
        lang(&book.language, &book.title),
        esc(&book.title),
        lang(&book.language, &book.author),
        esc(&book.author),
        esc(&stands)
    );
    let marks = highlights_said(s, marks);
    if !marks.is_empty() {
        let _ = write!(out, "<p class=\"meta\">{}</p>", esc(&marks));
    }
    out.push_str("</a>");
    out
}
