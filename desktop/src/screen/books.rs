//! Books: every book as its cover, on the shelves `view::books::shelves`
//! offers, in an order and its way, narrowed to a span's window and by a
//! search the way the Kindle's own search narrows it.

use std::fmt::Write as _;

use readinglog_native::date;
use readinglog_native::lang::counted;
use readinglog_native::view::books as books_view;
use readinglog_native::view::search;
use readinglog_native::view::{Shelf, Sort, Way, Window, highlights_said};

use super::{Ctx, Route, chip, esc, lang};

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
        tile(cx, &mut out, at);
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

/// One book as the Kindle's row states it: its cover, a bar of its progress,
/// its title and author, its highlights, its time and where it stands.
fn tile(cx: &Ctx, out: &mut String, at: usize) {
    let s = cx.s;
    let book = &cx.stats.books[at];
    let width = book.bar_percent().max(0);
    let read = date::duration(book.read_seconds(cx.from), s);
    let meta = match (book.is_finished(), book.has_percent()) {
        (true, _) => format!("{read} · {}", s.shelf_finished),
        (false, true) => format!(
            "{read} · {}",
            s.percent_plain
                .replace("{d}", &book.percent_shown().to_string())
        ),
        (false, false) => read,
    };
    let marks = highlights_said(s, cx.stats.highlights_counted(at));
    let _ = write!(
        out,
        "<a class=\"tile\" href=\"{}\"><div class=\"cover\">{}</div>\
         <div class=\"progress\"><i style=\"width: {width}%\"></i></div>\
         <p class=\"title\"{}>{}</p><p class=\"author\">{}</p><p class=\"meta\">{}</p>{}</a>",
        cx.book_link(at),
        cx.cover(at),
        lang(&book.language),
        esc(&book.title),
        esc(&book.author),
        esc(&meta),
        match marks.is_empty() {
            true => String::new(),
            false => format!("<p class=\"meta\">{}</p>", esc(&marks)),
        }
    );
}
