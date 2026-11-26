//! [`page`]: Books, each book's cover on the shelves `books_view::shelves` offers, in an order,
//! narrowed to a window, the values [`narrows`] holds and a search over titles, authors,
//! publishers and tags, the tags in use over the grid, its tiles feeding the bulk editor.

use std::fmt::Write as _;

use readinglog_native::date;
use readinglog_native::hanfold;
use readinglog_native::lang::counted;
use readinglog_native::view::books as books_view;
use readinglog_native::view::search;
use readinglog_native::view::{Shelf, Sort, Way, Window};

use super::words::Words;
use super::{Ctx, Route, chip, esc, metadata, paper, tile};
use crate::metadata::links::Name;
use crate::metadata::{By, narrows};

#[allow(clippy::too_many_arguments)]
pub(super) fn page(
    cx: &Ctx,
    shelf: Shelf,
    order: Sort,
    way: Way,
    query: &str,
    window: Option<Window>,
    by: &[By],
    editing: bool,
) -> String {
    let s = cx.s;
    let w = Words::of(cx.lang);
    let route = |shelf: Shelf,
                 order: Sort,
                 way: Way,
                 window: Option<Window>,
                 by: Vec<By>,
                 editing: bool| {
        Route::Books {
            shelf,
            order,
            way,
            query: query.to_string(),
            window,
            by,
            editing,
        }
        .link()
    };
    let to = |shelf: Shelf, order: Sort, way: Way, window: Option<Window>| {
        route(shelf, order, way, window, by.to_vec(), editing)
    };
    // `named`: the author or publisher Books is narrowed to, drawn as a chip; `tags`, the rest.
    let named = by.iter().find(|by| !matches!(by, By::Tag(..)));
    let tags: Vec<By> = by
        .iter()
        .filter(|by| matches!(by, By::Tag(..)))
        .cloned()
        .collect();
    // `adding`: the button adding a paper book beside the search, and the form it opens.
    let (add, adding) = paper::adding(cx);
    let mut out = format!(
        "<header class=\"head\"><h1>{}</h1><span class=\"tools\">{add}<input class=\"search\" type=\"search\" \
         data-search=\"books\" value=\"{}\" placeholder=\"{}\"></span></header>{adding}\
         <div class=\"bar\"><div class=\"chips\">",
        esc(s.books),
        esc(query),
        esc(w.search_books)
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
    if let Some(named) = named {
        out.push_str(&metadata::chip(
            cx,
            named,
            &route(shelf, order, way, window, tags.clone(), editing),
        ));
    }
    let found = searched(cx, query);
    let over = window.map(|window| window.days(cx.week));
    let (drawn, nameless): (Vec<usize>, Vec<usize>) =
        books_view::listed(cx.stats, shelf, order, way, over, cx.uncovered, None)
            .into_iter()
            .filter(|at| found.contains(at))
            .filter(|at| narrows(by, &metadata::values(cx, *at), cx.linked))
            .partition(|at| {
                let book = &cx.stats.books[*at];
                !book.title.is_empty() || book.has_cover()
            });
    out.push_str("</div>");
    out.push_str(&metadata::chosen(cx, drawn.len()));
    out.push_str("<div class=\"chips\">");
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
    let shown: Vec<usize> = drawn.iter().chain(&nameless).copied().collect();
    out.push_str(&metadata::listed(cx, by, &shown, editing, |by, editing| {
        route(shelf, order, way, window, by, editing)
    }));
    let valued = named.filter(|by| matches!(by, By::Author(Some(_)) | By::Publisher(Some(_))));
    if valued.is_some() || editing {
        out.push_str(&metadata::linking(cx, valued));
    }
    if let Some(named) = named {
        out.push_str(&metadata::renaming(cx, named, &drawn));
    }
    let not_set = by.iter().any(|by| {
        matches!(
            by,
            By::Author(None) | By::Publisher(None) | By::Tag(_, None)
        )
    });
    if not_set {
        out.push_str(&metadata::suggestions(cx, &drawn));
    }
    if drawn.is_empty() && nameless.is_empty() {
        let _ = write!(out, "<p class=\"lede\">{}</p>", esc(s.nothing_on_the_shelf));
        return out;
    }
    out.push_str(&metadata::sheet(cx));
    out.push_str("<div class=\"grid\" data-pickable>");
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

/// `searched`: the books `query` finds, `search::listed`'s over titles and authors
/// and each book whose publisher or a tag holds it, or another name of a value it holds.
fn searched(cx: &Ctx, query: &str) -> Vec<usize> {
    let mut out = search::listed(cx.stats, query, cx.uncovered);
    let wanted = hanfold::fold(&query.trim().to_lowercase()).into_owned();
    if wanted.is_empty() {
        return out;
    }
    let holds = |text: &str| hanfold::fold(&text.to_lowercase()).contains(&wanted);
    let any = |name: Name| cx.linked.every(name).into_iter().any(holds);
    for at in 0..cx.stats.books.len() {
        if out.contains(&at) {
            continue;
        }
        let values = metadata::values(cx, at);
        let found = values
            .authors
            .iter()
            .any(|a| cx.linked.others(Name::Author(a), a).into_iter().any(holds))
            || values
                .publisher
                .as_deref()
                .is_some_and(|p| any(Name::Publisher(p)))
            || values
                .tags
                .iter()
                .any(|t| any(Name::Tag(&t.r#type, &t.tag)) || any(Name::Type(&t.r#type)));
        if found {
            out.push(at);
        }
    }
    out
}
