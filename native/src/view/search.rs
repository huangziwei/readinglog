//! Books by title or author: [`field`] carries the query, [`listed`] names
//! the books, and [`results_box`] holds the rows the Books list draws.

use crate::stats::Stats;
use crate::ui::chrome;
use crate::ui::paint::{self, INK, Rect};
use crate::ui::text::TextRenderer;
use crate::ui::theme::Theme;

use super::{Ctx, Hit, Search, books, pager};

/// Half the width of [`field`]'s mark, as a share of its height.
const CROSS: f32 = 0.19;

/// The head [`field`] stands in, and the box left below it: the Books list's
/// own head, holding `books::search_button` and every row below it in place.
fn split(theme: &Theme, area: Rect) -> (Rect, Rect) {
    area.split_top(chrome::chip_height(theme) + theme.gap * 2)
}

/// The books `query` names, by their index in [`Stats::books`], in the order
/// they are held.
///
/// A book matches where its `title` or its `author` holds `query`, case
/// folded. An empty `query` names every book.
pub fn listed(stats: &Stats, query: &str, uncovered: bool) -> Vec<usize> {
    let needle = query.to_lowercase();
    (0..stats.books.len())
        .filter(|at| uncovered || stats.books[*at].has_cover())
        .filter(|at| {
            let book = &stats.books[*at];
            needle.is_empty()
                || book.title.to_lowercase().contains(&needle)
                || book.author.to_lowercase().contains(&needle)
        })
        .collect()
}

/// The box the rows are drawn into: the page under [`field`], floored at the
/// keyboard's top edge where `keyboard`.
pub fn results_box(theme: &Theme, area: Rect, keyboard: bool) -> Rect {
    let (_, under) = split(theme, area);
    if !keyboard {
        return under;
    }
    let over = theme.screen.bottom() - crate::keyboard::height(theme.screen.h);
    Rect::new(
        under.x,
        under.y,
        under.w,
        (over - theme.gap - under.y).max(0),
    )
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

pub fn draw(cx: &mut Ctx, area: Rect, search: &Search) {
    let theme: &Theme = cx.theme;
    let (head, _) = split(theme, area);
    field(
        cx,
        Rect::new(head.x, head.y, head.w, chrome::chip_height(theme)),
        &search.query,
        &search.preedit,
    );

    let row_h = row_span(theme, area);
    let fits = rows_per_page(theme, area, search.keyboard);
    let box_ = results_box(theme, area, search.keyboard);
    let found = listed(cx.stats, &search.query, cx.uncovered);
    if found.is_empty() {
        nothing(cx, box_);
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

/// An outline the width of the page, the magnifier in the square at its left
/// end and [`Hit::SearchClear`]'s mark in the square at its right. `query` and
/// `preedit` are set tail-first, the caret after them, a rule under `preedit`.
fn field(cx: &mut Ctx, at: Rect, query: &str, preedit: &str) {
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
        let said = cx
            .text
            .wrap_and_clamp_in(script, cx.s().search_hint, room, 1);
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

/// The line drawn where [`listed`] is empty.
fn nothing(cx: &mut Ctx, area: Rect) {
    let script = cx.ui_script();
    let said = cx.s().nothing_on_the_shelf;
    cx.text.set_px(cx.theme.body_px);
    let baseline = area.y + cx.text.line_height() as i32;
    cx.text
        .draw_in(script, cx.fb, area.x, baseline, said, false);
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::stats::BookStat;
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
}
