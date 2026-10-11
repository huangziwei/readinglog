//! Highlights: every one the record holds, newest first, each run of one book's
//! under its cover and title, narrowed by the Kindle's own search.

use std::fmt::Write as _;

use readinglog_native::lang::counted;
use readinglog_native::view::search;

use super::{Ctx, book, esc, lang};

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
        "<p class=\"lede\">{}</p><div class=\"highlights\">",
        esc(&counted(s.n_highlights, rows.len() as i64))
    );
    let mut last: Option<usize> = None;
    for row in &rows {
        if last != Some(row.book) {
            let named = &cx.stats.books[row.book];
            let _ = write!(
                out,
                "<h3><span class=\"thumb cover\">{}</span><a href=\"{}\"{}>{}</a></h3>",
                cx.cover(row.book),
                cx.book_link(row.book),
                lang(&named.language),
                esc(&named.title)
            );
            last = Some(row.book);
        }
        out.push_str(&book::highlight(cx, row.book, row.highlight, row.note));
    }
    out.push_str("</div>");
    out
}
