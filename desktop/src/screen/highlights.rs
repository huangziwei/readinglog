//! Highlights: every one the record holds, newest first, each run of one book's
//! beside its tile, narrowed by the Kindle's own search.

use std::fmt::Write as _;
use std::ops::RangeInclusive;

use readinglog_native::lang::counted;
use readinglog_native::stats::Across;
use readinglog_native::view::book as book_view;
use readinglog_native::view::search;

use super::{Ctx, book, esc, tile};

pub(super) fn page(cx: &Ctx, query: &str) -> String {
    let s = cx.s;
    let rows = search::listed_highlights(cx.stats, query, cx.uncovered);
    let mut out = format!(
        "<header class=\"head\"><h1>{}</h1><input class=\"search\" type=\"search\" data-search=\"highlights\" \
         value=\"{}\" placeholder=\"{}\"></header>",
        esc(s.highlights_tab),
        esc(query),
        esc(s.search_hint_highlights)
    );
    if rows.is_empty() {
        let _ = write!(out, "<p class=\"lede\">{}</p>", esc(s.no_highlights_here));
        return out;
    }
    let _ = write!(
        out,
        "<p class=\"lede\">{}</p>",
        esc(&counted(s.n_highlights, rows.len() as i64))
    );
    for held in rows.chunk_by(|a, b| a.book == b.book) {
        out.push_str(&run(cx, held, query, None));
    }
    out
}

/// One book's `rows` beside its [`tile::tile`] over `over`, each as
/// [`book::highlight`] sets it with `needle` washed; `over` `None` adds the
/// day the book was last read.
pub(super) fn run(
    cx: &Ctx,
    rows: &[Across<'_>],
    needle: &str,
    over: Option<RangeInclusive<i64>>,
) -> String {
    let at = rows[0].book;
    let book = &cx.stats.books[at];
    let last = match over {
        Some(_) => String::new(),
        None => match book_view::day_note(book.last_day, book, cx.s) {
            said if said == book_view::DASH => String::new(),
            said => format!(
                "<p class=\"meta\">{} {}</p>",
                esc(cx.s.last_read),
                esc(&said)
            ),
        },
    };
    let mut out = format!(
        "<section class=\"run\"><aside>{}{last}</aside><div class=\"highlights\">",
        tile::tile(cx, at, over)
    );
    for row in rows {
        out.push_str(&book::highlight(
            cx,
            row.book,
            row.highlight,
            row.note,
            needle,
        ));
    }
    out.push_str("</div></section>");
    out
}
