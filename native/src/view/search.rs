//! Books by title or author, and highlights by the words in them: [`field`]
//! carries the query, [`listed`] and [`listed_highlights`] name what it found, and
//! [`results_box`] holds the rows the Books list draws.

use crate::font::Script;
use crate::hanfold;
use crate::settings::Scope;
use crate::stats::{Across, BookStat, Stats};
use crate::ui::chrome;
use crate::ui::paint::{self, INK, Rect};
use crate::ui::text::TextRenderer;
use crate::ui::theme::Theme;

use super::{Ctx, Hit, Search, books, journal, pager};

/// Half the width of [`field`]'s cross, as a share of its height.
const CROSS: f32 = 0.19;

/// The head [`field`] stands in, and the box left below it: the Books list's
/// own head, holding `books::search_button` and every row below it in place.
fn split(theme: &Theme, area: Rect) -> (Rect, Rect) {
    area.split_top(chrome::chip_height(theme) + theme.gap * 2)
}

/// The books `query` names, by their index in [`Stats::books`], in the order
/// they are held: those whose `title` or `author` [`Needle::holds`]. An empty
/// `query` names every book.
pub fn listed(stats: &Stats, query: &str, uncovered: bool) -> Vec<usize> {
    let needle = Needle::of(query);
    (0..stats.books.len())
        .filter(|at| uncovered || stats.books[*at].has_cover())
        .filter(|at| {
            let book = &stats.books[*at];
            needle.holds(&book.title) || needle.holds(&book.author)
        })
        .collect()
}

/// A query as every list tests it: `said` case folded, and `folded` taken
/// through [`hanfold::fold`] as well. [`Needle::holds`] takes either, which
/// only ever adds matches.
pub(super) struct Needle {
    said: String,
    folded: String,
}

impl Needle {
    pub(super) fn of(query: &str) -> Self {
        let said = query.to_lowercase();
        let folded = hanfold::fold(&said).into_owned();
        Self { said, folded }
    }

    /// Whether `body` holds it. An empty query is held by everything.
    pub(super) fn holds(&self, body: &str) -> bool {
        if self.said.is_empty() {
            return true;
        }
        let body = body.to_lowercase();
        body.contains(&self.said) || hanfold::fold(&body).contains(&self.folded)
    }

    /// Whether `folded`, a body lowercased and folded as [`Stats::folded`] holds
    /// one per highlight, holds it: the answer [`Self::holds`] gives that body.
    pub(super) fn holds_folded(&self, folded: &str) -> bool {
        self.said.is_empty() || folded.contains(&self.folded)
    }
}

/// The highlights `query` names, across every book, most recently made first:
/// those whose `highlight.body` or a note's `body` [`Needle::holds`]. An empty `query`
/// names every highlight, and `uncovered` is the filter [`listed`] takes.
pub fn listed_highlights<'a>(stats: &'a Stats, query: &str, uncovered: bool) -> Vec<Across<'a>> {
    let needle = Needle::of(query);
    stats
        .highlights_across()
        .into_iter()
        .filter(|row| uncovered || stats.books.get(row.book).is_some_and(BookStat::has_cover))
        .filter(|row| {
            // `Stats::folded` holds each highlight's body folded once; `needle.holds`
            // folds a note's `body` here.
            let body = match stats.folded.get(row.held).and_then(Option::as_deref) {
                Some(folded) => needle.holds_folded(folded),
                None => needle.holds(&row.highlight.body),
            };
            body || row.notes.iter().any(|n| needle.holds(&n.body))
        })
        .collect()
}

/// What [`listed_highlights`] found, as the rows a list of highlights is drawn from.
fn highlight_rows(stats: &Stats, query: &str, uncovered: bool) -> Vec<journal::Row> {
    listed_highlights(stats, query, uncovered)
        .into_iter()
        .filter_map(|found| {
            let record = stats.books.get(found.book)?;
            Some(journal::Row::new(
                found.book,
                found.at,
                record,
                found.highlight,
                &found.notes,
                stats,
            ))
        })
        .collect()
}

/// Where each page of the highlights `search` named opens, which is the step a page
/// button takes. Every row is measured, each being as tall as its own words, a
/// desktop note's in `ui`, the window's own script.
pub fn highlight_pages(
    text: &mut TextRenderer,
    theme: &Theme,
    stats: &Stats,
    area: Rect,
    search: &Search,
    uncovered: bool,
    ui: Script,
) -> Vec<usize> {
    let held = highlight_rows(stats, &search.query, uncovered);
    let inner = rows_box(theme, area, search.keyboard);
    let at = journal::Layout::of(text, theme, inner, true, ui).found(theme, inner.h, &search.query);
    journal::openings(text, theme, &held, &at, inner.h)
}

/// The box the rows are drawn into: the page under [`field`], floored at the
/// keyboard's top edge where `keyboard`.
pub fn results_box(theme: &Theme, area: Rect, keyboard: bool) -> Rect {
    let (_, under) = split(theme, area);
    match keyboard {
        true => super::over_keyboard(theme, under),
        false => under,
    }
}

/// The box the rows themselves take: [`results_box`] over the pager's strip.
fn rows_box(theme: &Theme, area: Rect, keyboard: bool) -> Rect {
    // `split_bottom` answers the strip first and what is left above it second.
    results_box(theme, area, keyboard)
        .split_bottom(pager::height(theme))
        .1
}

/// The height one result row is drawn at: the height the Books list gives a
/// row of the whole page, held while the keyboard stands over part of it.
fn row_span(theme: &Theme, area: Rect) -> i32 {
    books::row_span(theme, split(theme, area).1)
}

/// Rows one page of the results holds.
pub fn rows_per_page(theme: &Theme, area: Rect, keyboard: bool) -> usize {
    let box_ = results_box(theme, area, keyboard);
    (((box_.h - pager::height(theme)) / row_span(theme, area)).max(1)) as usize
}

/// Where the last page of `count` results opens.
pub fn last_page_at(theme: &Theme, area: Rect, keyboard: bool, count: usize) -> usize {
    super::last_page_at(count, rows_per_page(theme, area, keyboard))
}

pub fn draw(cx: &mut Ctx, area: Rect, search: &Search, scope: Scope) {
    let theme: &Theme = cx.theme;
    let (head, _) = split(theme, area);
    let head = Rect::new(head.x, head.y, head.w, chrome::chip_height(theme));
    // [`scope_chips`] keeps the right end of `head`; [`field`] takes what is
    // left of it, at `head.x` and `head.h`.
    let taken = scope_chips(cx, head, scope);
    let hint = match scope {
        Scope::Books => cx.s().search_hint,
        Scope::Journal => cx.s().search_hint_journal,
    };
    field(
        cx,
        Rect::new(head.x, head.y, (head.w - taken).max(0), head.h),
        &search.query,
        &search.preedit,
        hint,
    );
    match scope {
        Scope::Books => shelf(cx, area, search),
        Scope::Journal => highlighted(cx, area, search),
    }
}

/// The two lists as a chip apiece at the right of the head row, the one
/// showing filled, each its own hit box. Answers the width it took, the air
/// before it included, which is what the field gives up.
fn scope_chips(cx: &mut Ctx, area: Rect, on: Scope) -> i32 {
    let theme: &Theme = cx.theme;
    let script = cx.ui_script();
    let s = cx.s();
    let options: Vec<(&str, crate::font::Script)> =
        vec![(s.books, script), (s.journal_tab, script)];
    let placed = chrome::chip_layout(cx.text, theme, &options, None, area.w);
    let wide = placed.iter().map(Rect::right).max().unwrap_or(0);
    let at = Rect::new(area.right() - wide, area.y, wide, area.h);
    let picked = Scope::ALL
        .iter()
        .position(|scope| *scope == on)
        .unwrap_or(0);
    let drawn = chrome::chips(cx.fb, cx.text, theme, at, &options, &placed, picked);
    for (scope, chip) in Scope::ALL.iter().zip(drawn) {
        cx.hit(Hit::Scoped(*scope), chip);
    }
    wide + chrome::chip_gap(theme)
}

/// The books the query named, in the rows the Books list gives them.
fn shelf(cx: &mut Ctx, area: Rect, search: &Search) {
    let theme: &Theme = cx.theme;
    let row_h = row_span(theme, area);
    let fits = rows_per_page(theme, area, search.keyboard);
    let box_ = results_box(theme, area, search.keyboard);
    let found = listed(cx.stats, &search.query, cx.uncovered);
    if found.is_empty() {
        nothing(cx, box_, cx.s().nothing_on_the_shelf);
        return;
    }
    let last = super::last_page_at(found.len(), fits);
    let from = search.from.min(last);
    let to = (from + fits).min(found.len());
    for (slot, index) in found[from..to].iter().enumerate() {
        let row = Rect::new(box_.x, box_.y + slot as i32 * row_h, box_.w, row_h);
        books::book_row(cx, row, *index);
        cx.hit(Hit::Book(*index), row);
    }
    if from > 0 || to < found.len() {
        let label = format!("{}–{} {} {}", from + 1, to, cx.s().of, found.len());
        pager::draw(
            cx,
            pager::foot(theme, box_),
            &label,
            [from > 0, to < found.len()],
            [Hit::SearchPage(0), Hit::SearchPage(last)],
        );
    }
}

/// The highlights the query named, most recently made first: every row as tall
/// as its own words, paged the way one book's own list is.
fn highlighted(cx: &mut Ctx, area: Rect, search: &Search) {
    let theme: &Theme = cx.theme;
    let box_ = results_box(theme, area, search.keyboard);
    let held = highlight_rows(cx.stats, &search.query, cx.uncovered);
    if held.is_empty() {
        nothing(cx, box_, cx.s().no_highlights_here);
        return;
    }
    let inner = rows_box(theme, area, search.keyboard);
    let at = journal::Layout::of(cx.text, theme, inner, true, cx.ui_script()).found(
        theme,
        inner.h,
        &search.query,
    );
    let opens = journal::openings(cx.text, theme, &held, &at, inner.h);
    // The page showing opens at the last opening at or before `search.from`.
    let page = opens.iter().rposition(|o| *o <= search.from).unwrap_or(0);
    let from = opens.get(page).copied().unwrap_or(0);
    let to = opens.get(page + 1).copied().unwrap_or(held.len());

    let shown = &held[from..to];
    let mut y = inner.y;
    for (n, row) in shown.iter().enumerate() {
        let high = journal::height(cx.text, theme, &at, row);
        let box_of = Rect::new(inner.x, y, inner.w, high);
        journal::row(cx, box_of, row, &at);
        cx.hit(Hit::Highlight(row.book, row.at), box_of);
        // A rule between rows, and none under the last.
        if n + 1 < shown.len() {
            paint::hline(
                cx.fb,
                inner.x,
                y + high - journal::rule_over(theme),
                inner.w,
                paint::PALE,
                1,
            );
        }
        y += high;
    }

    if opens.len() > 1 {
        let label = format!("{}–{to} {} {}", from + 1, cx.s().of, held.len());
        pager::draw(
            cx,
            pager::foot(theme, box_),
            &label,
            [page > 0, page + 1 < opens.len()],
            [
                Hit::SearchPage(0),
                Hit::SearchPage(opens.last().copied().unwrap_or(0)),
            ],
        );
    }
}

/// An outline over `at`, `books::magnifier` at its left end and
/// [`Hit::SearchClear`]'s cross at its right. `query` and `preedit` are set
/// tail-first, the caret after them; `hint` stands for both empty.
pub(super) fn field(cx: &mut Ctx, at: Rect, query: &str, preedit: &str, hint: &str) {
    let theme: &Theme = cx.theme;
    paint::stroke(cx.fb, at, INK, theme.rule());
    books::magnifier(cx, Rect::new(at.x, at.y, at.h, at.h));
    cx.hit(Hit::SearchField, at);

    // [`Hit::SearchClear`] stands on an empty `query` as well.
    let zone = Rect::new(at.right() - at.h, at.y, at.h, at.h);
    let arm = (at.h as f32 * CROSS) as i32;
    paint::cross(cx.fb, zone, arm, INK, theme.rule() + 1);
    cx.hit(Hit::SearchClear, zone);

    let script = cx.ui_script();
    cx.text.set_px(theme.body_px);
    let baseline = at.center_y() + cx.text.cap_height() as i32 / 2;
    let x = at.x + at.h;
    let room = (zone.x - theme.gap - x).max(0) as u32;
    if query.is_empty() && preedit.is_empty() {
        let said = cx.text.wrap_and_clamp_in(script, hint, room, 1);
        let said = said.first().map(String::as_str).unwrap_or_default();
        cx.text.draw_in(script, cx.fb, x, baseline, said, false);
        return;
    }
    // `tail` drops the head of a line wider than `room`.
    let said = tail(cx.text, script, &format!("{query}{preedit}"), room);
    let w = cx.text.measure_width_in(script, &said) as i32;
    cx.text.draw_in(script, cx.fb, x, baseline, &said, false);
    if !preedit.is_empty() {
        let under = cx.text.measure_width_in(script, preedit) as i32;
        let rule = theme.rule();
        paint::hline(cx.fb, x + w - under, baseline + rule * 2, under, INK, rule);
    }
    let caret = cx.text.cap_height() as i32 * 3 / 2;
    paint::vline(
        cx.fb,
        x + w + theme.gap / 2,
        at.center_y() - caret / 2,
        caret,
        INK,
        theme.rule(),
    );
}

/// The trailing part of `said` that fits `room`.
fn tail(text: &mut TextRenderer, script: crate::font::Script, said: &str, room: u32) -> String {
    if text.measure_width_in(script, said) <= room {
        return said.to_string();
    }
    let chars: Vec<char> = said.chars().collect();
    for start in 1..chars.len() {
        let tail: String = chars[start..].iter().collect();
        if text.measure_width_in(script, &tail) <= room {
            return tail;
        }
    }
    String::new()
}

/// The line drawn where a scope found nothing.
fn nothing(cx: &mut Ctx, area: Rect, said: &str) {
    let script = cx.ui_script();
    cx.text.set_px(cx.theme.body_px);
    let baseline = area.y + cx.text.line_height() as i32;
    cx.text
        .draw_in(script, cx.fb, area.x, baseline, said, false);
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::stats::BookStat;
    use crate::stats::tests::highlighted_books;
    use crate::ui::theme::tests::PANELS;

    /// Two books sharing no word.
    fn shelf() -> Stats {
        let one = BookStat {
            title: "The Ninth Winter".into(),
            author: "Cordelia Nash".into(),
            thumbnail: "a".into(),
            ..BookStat::default()
        };
        let two = BookStat {
            title: "夢遊症候群".into(),
            author: "林素".into(),
            thumbnail: "b".into(),
            ..BookStat::default()
        };
        Stats {
            books: vec![one, two],
            ..Stats::default()
        }
    }

    #[test]
    fn a_query_names_a_book_by_either_of_its_two_lines() {
        let stats = shelf();
        assert_eq!(listed(&stats, "ninth", true), [0]);
        assert_eq!(listed(&stats, "NINTH", true), [0]);
        assert_eq!(listed(&stats, "nash", true), [0]);
        assert_eq!(listed(&stats, "林", true), [1]);
        assert_eq!(listed(&stats, "症候", true), [1]);
    }

    #[test]
    fn an_empty_query_names_every_book() {
        let stats = shelf();
        assert_eq!(listed(&stats, "", true), [0, 1]);
        assert!(listed(&stats, "zzz", true).is_empty());
    }

    #[test]
    fn a_query_in_one_script_names_a_book_written_in_the_other() {
        let mut stats = shelf();
        stats.books[1].title = "萬國博覽會".into();
        // 万国 is what a pinyin keyboard commits.
        assert_eq!(listed(&stats, "万国", true), [1]);
        assert_eq!(listed(&stats, "萬國", true), [1]);
        // And a Traditional query against a Simplified `title`.
        stats.books[1].title = "万国博览会".into();
        assert_eq!(listed(&stats, "萬國", true), [1]);
        assert_eq!(listed(&stats, "万国", true), [1]);
        // 楽 is no key of `hanfold::FOLD`, and 楽園 folds onto neither.
        stats.books[0].title = "楽園".into();
        assert!(listed(&stats, "乐", true).is_empty());
    }

    #[test]
    fn a_passage_is_found_across_the_scripts_too() {
        let mut stats = highlighted_books();
        stats.highlights[2].body = "萬國博覽會".into();
        let found = listed_highlights(&stats, "万国", true);
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].book, 1);
        assert_eq!(listed_highlights(&stats, "萬國", true).len(), 1);
    }

    #[test]
    fn the_head_and_the_rows_stand_where_the_books_list_puts_them() {
        for panel in PANELS {
            let theme = crate::ui::theme::Theme::for_screen(panel.0, panel.1);
            let area = chrome::content_box(&theme);
            let list = books::list_box(&theme, area, true);
            assert_eq!(results_box(&theme, area, false), list, "{panel:?}");
            assert_eq!(
                row_span(&theme, area),
                books::row_span(&theme, list),
                "{panel:?}"
            );
            assert_eq!(
                rows_per_page(&theme, area, false),
                books::rows_per_page(&theme, list),
                "{panel:?}"
            );
        }
    }

    #[test]
    fn the_keyboard_costs_the_list_rows_while_it_stands() {
        for panel in PANELS {
            let theme = crate::ui::theme::Theme::for_screen(panel.0, panel.1);
            let area = chrome::content_box(&theme);
            let up = rows_per_page(&theme, area, true);
            let down = rows_per_page(&theme, area, false);
            assert!(up >= 1, "{panel:?} holds a row with the keyboard up");
            assert!(up < down, "{panel:?} holds fewer rows: {up} against {down}");
        }
    }

    #[test]
    fn a_query_names_a_passage_by_its_own_words_or_by_the_note_on_it() {
        let stats = highlighted_books();
        let found = listed_highlights(&stats, "sky", true);
        assert_eq!(found.len(), 1);
        assert_eq!((found[0].book, found[0].at), (0, 0));
        // The note written on that passage, which the passage itself does not
        // hold, names the same row.
        let found = listed_highlights(&stats, "BORROWED", true);
        assert_eq!(found.len(), 1);
        assert_eq!((found[0].book, found[0].at), (0, 0));
        // The other book's passage, in its own script.
        let found = listed_highlights(&stats, "港", true);
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].book, 1);
        assert!(listed_highlights(&stats, "zzz", true).is_empty());
    }

    #[test]
    fn an_empty_query_names_every_highlighted_passage_and_never_a_bookmark() {
        let stats = highlighted_books();
        let found = listed_highlights(&stats, "", true);
        assert_eq!(stats.highlights.len(), 4);
        assert_eq!(found.len(), 2);
        assert!(found.iter().all(|row| row.highlight.kind.spans_a_passage()));
    }

    #[test]
    fn a_word_in_two_books_names_both_of_them_newest_first() {
        let stats = highlighted_books();
        let found = listed_highlights(&stats, "port", true);
        assert_eq!(found.iter().map(|row| row.book).collect::<Vec<_>>(), [1, 0]);
    }

    #[test]
    fn a_book_the_shelf_leaves_out_takes_its_highlights_with_it() {
        let mut stats = highlighted_books();
        stats.books[1].thumbnail.clear();
        assert_eq!(listed_highlights(&stats, "port", false).len(), 1);
        assert_eq!(listed_highlights(&stats, "port", true).len(), 2);
    }

    /// `Stats::folded` must answer exactly what folding at the keystroke
    /// answered, Traditional against Simplified included, and a `Stats` built
    /// without it must come to the same rows.
    #[test]
    fn the_folded_body_answers_what_folding_at_the_keystroke_answered() {
        let queries = ["port", "PORT", "", "臺灣", "台湾", "nothing here"];

        let bare = highlighted_books();
        assert!(bare.folded.is_empty(), "the fixture holds no folded bodies");

        let mut held = highlighted_books();
        held.folded = held
            .highlights
            .iter()
            .map(|m| Some(crate::hanfold::fold(&m.body.to_lowercase()).into_owned()))
            .collect();

        for (at, highlight) in held.highlights.iter().enumerate() {
            for query in queries {
                let needle = Needle::of(query);
                assert_eq!(
                    needle.holds_folded(held.folded[at].as_deref().expect("a folded body")),
                    needle.holds(&highlight.body),
                    "{query:?} against {:?}",
                    highlight.body,
                );
            }
        }

        // The fast path and the fallback name the same highlights.
        for query in queries {
            let with: Vec<usize> = listed_highlights(&held, query, true)
                .iter()
                .map(|r| r.held)
                .collect();
            let without: Vec<usize> = listed_highlights(&bare, query, true)
                .iter()
                .map(|r| r.held)
                .collect();
            assert_eq!(with, without, "{query:?}");
        }
    }
}
