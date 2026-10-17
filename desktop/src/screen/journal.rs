//! Journal: every highlight and note the record holds, laid out by a [`Mode`]
//! and running either way, each run of one book's beside its tile, narrowed by
//! the Kindle's own search.

use std::fmt::Write as _;
use std::ops::RangeInclusive;

use readinglog_native::lang::counted;
use readinglog_native::stats::Across;
use readinglog_native::view::book as book_view;
use readinglog_native::view::{Way, search};

use super::{Ctx, Route, Words, book, chip, esc, tile};

/// How the Journal lays its rows out, each as it stands under `Way::Down`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Mode {
    /// Every row by `Highlight::at`, newest first, a book standing once for
    /// each run of its rows.
    #[default]
    Timeline,
    /// Each book in one run, most recently read first, its rows newest first.
    Book,
}

impl Mode {
    pub const ALL: [Mode; 2] = [Mode::Timeline, Mode::Book];

    /// What this mode's chip says, in `w`'s language.
    fn label(self, w: &Words) -> &'static str {
        match self {
            Mode::Timeline => w.mode_timeline,
            Mode::Book => w.mode_book,
        }
    }
}

pub(super) fn page(cx: &Ctx, mode: Mode, way: Way, query: &str) -> String {
    let s = cx.s;
    let mut rows = search::listed_highlights(cx.stats, query, cx.uncovered);
    // `sort_by_key` is stable: each book's rows keep their newest-first order.
    if mode == Mode::Book {
        rows.sort_by_key(|row| row.book);
    }
    if way == Way::Up {
        rows.reverse();
    }
    let mut out = format!(
        "<header class=\"head\"><h1>{}</h1><input class=\"search\" type=\"search\" data-search=\"highlights\" \
         value=\"{}\" placeholder=\"{}\"></header>",
        esc(s.journal_tab),
        esc(query),
        esc(s.search_hint_journal)
    );
    let said = match rows.is_empty() {
        true => s.no_highlights_here.to_string(),
        false => counted(s.n_highlights, rows.len() as i64),
    };
    // `rows` and `query` both empty: the lede alone, with no `.bar`.
    if rows.is_empty() && query.is_empty() {
        let _ = write!(out, "<p class=\"lede\">{}</p>", esc(&said));
        return out;
    }
    let to = |mode: Mode, way: Way| {
        Route::Journal {
            mode,
            way,
            query: query.to_string(),
        }
        .link()
    };
    let _ = write!(
        out,
        "<div class=\"bar\"><p class=\"lede\">{}</p><div class=\"chips\">",
        esc(&said)
    );
    let w = Words::of(cx.lang);
    for each in Mode::ALL {
        chip(&mut out, &to(each, way), each.label(w), each == mode);
    }
    let _ = write!(
        out,
        "<a class=\"chip\" href=\"{}\">{}</a></div></div>",
        to(mode, way.flipped()),
        way.caret()
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
            &row.notes,
            needle,
        ));
    }
    out.push_str("</div></section>");
    out
}
