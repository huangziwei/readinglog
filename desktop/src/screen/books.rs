//! Books: every book as its cover, on the shelves `view::books::shelves`
//! offers, in an order and its way, narrowed to a span's window and by a
//! search the way the Kindle's own search narrows it.

use std::fmt::Write as _;

use readinglog_native::date;
use readinglog_native::lang::counted;
use readinglog_native::view::books as books_view;
use readinglog_native::view::search;
use readinglog_native::view::{Shelf, Sort, Way, Window};

use super::{Ctx, Route, chip, esc, tile};

pub(super) fn page(
    cx: &Ctx,
    shelf: Shelf,
    order: Sort,
    way: Way,
    query: &str,
    window: Option<Window>,
) -> String {
    let s = cx.s;
    let to = |shelf: Shelf, order: Sort, way: Way, window: Option<Window>| {
        Route::Books {
            shelf,
            order,
            way,
            query: query.to_string(),
            window,
        }
        .link()
    };
    let mut out = format!(
        "<header class=\"head\"><h1>{}</h1><input class=\"search\" type=\"search\" data-search=\"books\" \
         value=\"{}\" placeholder=\"{}\"></header><div class=\"bar\"><div class=\"chips\">",
        esc(s.books),
        esc(query),
        esc(s.search_hint)
    );
    for each in books_view::shelves(cx.stats, shelf, cx.uncovered) {
        chip(
            &mut out,
            &to(*each, order, way, window),
            each.label(cx.lang),
            *each == shelf,
        );
    }
    if let Some(window) = window {
        let _ = write!(
            out,
            "<a class=\"chip on\" href=\"{}\">{} ▾</a>",
            to(shelf, order, way, None),
            esc(&window.name(cx.week, s))
        );
    }
    out.push_str("</div><div class=\"chips\">");
    for each in Sort::ALL {
        chip(
            &mut out,
            &to(shelf, each, way, window),
            each.label(cx.lang),
            each == order,
        );
    }
    let _ = write!(
        out,
        "<a class=\"chip\" href=\"{}\">{}</a></div></div>",
        to(shelf, order, way.flipped(), window),
        way.caret()
    );
    let named = search::listed(cx.stats, query, cx.uncovered);
    let over = window.map(|window| window.days(cx.week));
    let (drawn, nameless): (Vec<usize>, Vec<usize>) =
        books_view::listed(cx.stats, shelf, order, way, over, cx.uncovered)
            .into_iter()
            .filter(|at| named.contains(at))
            .partition(|at| {
                let book = &cx.stats.books[*at];
                !book.title.is_empty() || book.has_cover()
            });
    if drawn.is_empty() && nameless.is_empty() {
        let _ = write!(out, "<p class=\"lede\">{}</p>", esc(s.nothing_on_the_shelf));
        return out;
    }
    out.push_str("<div class=\"grid\">");
    for at in drawn {
        out.push_str(&tile::tile(cx, at, None));
    }
    out.push_str("</div>");
    if !nameless.is_empty() {
        let secs: i64 = nameless.iter().map(|at| cx.stats.books[*at].seconds).sum();
        let _ = write!(
            out,
            "<p class=\"more\">{} · {}</p>",
            esc(&counted(s.n_books, nameless.len() as i64)),
            esc(&date::duration(secs, s))
        );
    }
    out
}
