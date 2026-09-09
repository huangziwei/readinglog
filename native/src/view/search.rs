//! Books by title or author, typed on the device's own keyboard. The field is
//! a rounded pill at the head of the page and the results are the Books
//! list's own rows; a tap on one opens the book.
//!
//! The keyboard stands over the foot of the screen without moving anything of
//! ours, so the list lays itself out above it while it is up and takes the
//! whole page back when it goes.

use crate::stats::Stats;
use crate::ui::chrome;
use crate::ui::paint::{self, INK, Rect, WHITE};
use crate::ui::text::TextRenderer;
use crate::ui::theme::Theme;

use super::{Ctx, Hit, Search, books, pager};

/// The field's proportions, as shares of its own height: where the glyph's
/// centre falls, how big it is, the air between it and the query, and the
/// width of the mark's own end.
const GLYPH_R: f32 = 0.205;
const GLYPH_AT: f32 = 0.568;
const TEXT_GAP: f32 = 0.273;
const CLEAR_W: f32 = 1.70;

/// How tall the field is, against a chip on the row it stands in place of.
fn field_height(theme: &Theme) -> i32 {
    chrome::chip_height(theme) * 5 / 4
}

/// The head the field stands in, and the box left below it.
fn split(theme: &Theme, area: Rect) -> (Rect, Rect) {
    area.split_top(field_height(theme) + theme.gap * 2)
}

/// The books `query` names, by their index in [`Stats::books`], in the order
/// the list already holds them: most recently read first.
///
/// A query matches a book whose title or author holds it, case folded. An
/// empty one names every book, so the screen is never blank.
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

/// The box the results are drawn into: the page under the field, floored at
/// the keyboard's own top edge while it stands.
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

/// Rows one page of the results holds.
pub fn rows_per_page(theme: &Theme, area: Rect, keyboard: bool) -> usize {
    books::rows_per_page(theme, results_box(theme, area, keyboard))
}

pub fn draw(cx: &mut Ctx, area: Rect, search: &Search) {
    let theme: &Theme = cx.theme;
    let (head, _) = split(theme, area);
    field(
        cx,
        Rect::new(head.x, head.y, head.w, field_height(theme)),
        &search.query,
    );

    let area = results_box(cx.theme, area, search.keyboard);
    let found = listed(cx.stats, &search.query, cx.uncovered);
    if found.is_empty() {
        nothing(cx, area);
        return;
    }
    let row_h = books::row_span(cx.theme, area);
    let fits = books::rows_per_page(cx.theme, area);
    let from = search
        .from
        .min(books::last_page_at(cx.theme, area, found.len()));
    let to = (from + fits).min(found.len());
    for (slot, index) in found[from..to].iter().enumerate() {
        let row = Rect::new(area.x, area.y + slot as i32 * row_h, area.w, row_h);
        books::book_row(cx, row, *index);
        cx.hit(Hit::Book(*index), row);
    }
    if from > 0 || to < found.len() {
        let last = books::last_page_at(cx.theme, area, found.len());
        let label = format!("{}–{} {} {}", from + 1, to, cx.s().of, found.len());
        pager::draw(
            cx,
            pager::foot(cx.theme, area),
            &label,
            [from > 0, to < found.len()],
            [Hit::SearchPage(0), Hit::SearchPage(last)],
        );
    }
}

/// The field: a rounded outline, the glyph in its left end, the query set
/// tail-first with the caret after it, and the mark that clears it in the
/// right end. A tap on it raises the keyboard again.
fn field(cx: &mut Ctx, at: Rect, query: &str) {
    let theme: &Theme = cx.theme;
    let h = at.h as f32;
    let weight = (at.h / 26).max(theme.rule());
    paint::round_stroke(cx.fb, at, at.h / 2, INK, weight);
    let r = (h * GLYPH_R) as i32;
    let centre = at.x + (h * GLYPH_AT) as i32;
    paint::magnifier(
        cx.fb,
        Rect::new(centre - r, at.center_y() - r, r * 2, r * 2),
        INK,
        WHITE,
        weight,
    );
    cx.hit(Hit::SearchField, at);

    // The mark stands on an empty field too: it is the way out, and while the
    // keyboard is up it is the only one — the tab strip is under it.
    let clear = (h * CLEAR_W) as i32;
    let zone = Rect::new(at.right() - clear, at.y, clear, at.h);
    paint::cross(cx.fb, zone, (h * 0.17) as i32, INK, theme.rule() + 1);
    cx.hit(Hit::SearchClear, zone);

    let script = cx.ui_script();
    cx.text.set_px(theme.body_px);
    let baseline = at.center_y() + cx.text.cap_height() as i32 / 2;
    let x = centre + r + (h * TEXT_GAP) as i32;
    let room = (zone.x - theme.gap - x).max(0) as u32;
    if query.is_empty() {
        let said = cx
            .text
            .wrap_and_clamp_in(script, cx.s().search_hint, room, 1);
        let said = said.first().map(String::as_str).unwrap_or_default();
        cx.text.draw_in(script, cx.fb, x, baseline, said, false);
        return;
    }
    // The tail is what is being typed, so a long query loses its head.
    let said = tail(cx.text, script, query, room);
    let w = cx.text.measure_width_in(script, &said) as i32;
    cx.text.draw_in(script, cx.fb, x, baseline, &said, false);
    paint::vline(
        cx.fb,
        x + w + theme.gap / 2,
        at.y + theme.gap,
        at.h - theme.gap * 2,
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

/// The line a query naming no book stands under.
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

    /// Two books with nothing in common but a letter.
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

    /// A query names a book by its title or by its author, whatever case it
    /// is typed in.
    #[test]
    fn a_query_names_a_book_by_either_of_its_two_lines() {
        let stats = shelf();
        assert_eq!(listed(&stats, "ninth", true), [0]);
        assert_eq!(listed(&stats, "NINTH", true), [0]);
        assert_eq!(listed(&stats, "nash", true), [0]);
        assert_eq!(listed(&stats, "林", true), [1]);
        assert_eq!(listed(&stats, "症候", true), [1]);
    }

    /// Nothing typed names every book, and a query naming none is empty.
    #[test]
    fn an_empty_query_names_every_book() {
        let stats = shelf();
        assert_eq!(listed(&stats, "", true), [0, 1]);
        assert!(listed(&stats, "zzz", true).is_empty());
    }

    /// The keyboard takes rows off the page while it stands, and gives them
    /// back when it goes.
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
