//! Every [`BookStat`], most recent first, with its cover and the progress the
//! catalog states.

use crate::date;
use crate::ui::cover;
use crate::ui::paint::{self, INK, LIGHT, Rect};
use crate::ui::theme::Theme;

use super::{Ctx, Hit, State};

/// Lines a title takes before the rest of it is ellipsized.
const TITLE_LINES: usize = 2;

/// The height one book takes, set by the cover it carries.
fn row_height(theme: &Theme) -> i32 {
    theme.row_h * 5 / 2
}

/// The strip under the rows that the page counter sits in.
fn foot_height(theme: &Theme) -> i32 {
    theme.small_px as i32 * 2
}

/// The width of the figures column: the wider of `figure` and `100%`.
fn figures_width(cx: &mut Ctx, figure: &str) -> i32 {
    let duration = cx.text.measure_width(figure) as i32;
    cx.text.set_px(cx.theme.small_px);
    let pct = cx.text.measure_width("100%") as i32;
    cx.text.set_px(cx.theme.body_px);
    duration.max(pct)
}

/// Rows one page of the list holds, [`foot_height`] taken off first.
pub fn rows_per_page(theme: &Theme, area: Rect) -> usize {
    (((area.h - foot_height(theme)) / row_height(theme)).max(1)) as usize
}

/// The largest `books_from` that fills a page, for `count` books.
pub fn last_page_at(theme: &Theme, area: Rect, count: usize) -> usize {
    count.saturating_sub(rows_per_page(theme, area))
}

/// The height a row is drawn at: the page's rows share `area`, capped at
/// [`row_height`] and half again.
fn row_span(theme: &Theme, area: Rect) -> i32 {
    let fits = rows_per_page(theme, area) as i32;
    ((area.h - foot_height(theme)) / fits).clamp(row_height(theme), row_height(theme) * 3 / 2)
}

pub fn draw(cx: &mut Ctx, area: Rect, state: &State) {
    let theme: &Theme = cx.theme;
    if cx.stats.books.is_empty() {
        empty(cx, area);
        return;
    }

    let row_h = row_span(theme, area);
    let fits = rows_per_page(theme, area);
    let from = state
        .books_from
        .min(last_page_at(theme, area, cx.stats.books.len()));
    let to = (from + fits).min(cx.stats.books.len());

    for (slot, index) in (from..to).enumerate() {
        let row = Rect::new(area.x, area.y + slot as i32 * row_h, area.w, row_h);
        book_row(cx, row, index);
        cx.hit(Hit::Book(index), row);
    }

    // `foot` takes a tap on either half.
    let (foot, _) = area.split_bottom(foot_height(theme));
    if from > 0 || to < cx.stats.books.len() {
        cx.text.set_px(theme.small_px);
        let label = format!("{}–{} of {}", from + 1, to, cx.stats.books.len());
        let w = cx.text.measure_width(&label) as i32;
        cx.text.draw(
            cx.fb,
            foot.x + (foot.w - w) / 2,
            foot.bottom(),
            &label,
            false,
        );
        let (left, right) = foot.split_left(foot.w / 2);
        if from > 0 {
            cx.hit(Hit::Prev, left);
        }
        if to < cx.stats.books.len() {
            cx.hit(Hit::Next, right);
        }
    }
}

fn book_row(cx: &mut Ctx, row: Rect, index: usize) {
    let theme: &Theme = cx.theme;
    let book = &cx.stats.books[index];
    let inner = row.inset(theme.gap);
    let (art, rest) = inner.split_left(cover::width_for(inner.h));
    cx.covers.draw(cx.fb, art, &book.thumbnail);

    let body = Rect::new(
        art.right() + theme.gap * 2,
        inner.y,
        rest.w - theme.gap * 2,
        inner.h,
    );
    let script = crate::font::Script::of_language(&book.language);

    // Two columns: the words on the left, the figures on the right.
    let figure = date::duration(book.seconds);
    cx.text.set_px(theme.body_px);
    let column_w = figures_width(cx, &figure) + theme.gap * 2;
    let (words, figures) = body.split_left((body.w - column_w).max(theme.gap));
    let words = Rect::new(words.x, words.y, (words.w - theme.gap * 2).max(1), words.h);

    let lines = cx
        .text
        .wrap_and_clamp_in(script, &book.title, words.w as u32, TITLE_LINES);
    let title_h = lines.len() as i32 * cx.text.line_height() as i32;
    cx.text.set_px(theme.small_px);
    let author_h = match book.author.is_empty() {
        true => 0,
        false => theme.gap / 2 + cx.text.line_height() as i32,
    };
    let bar_h = (theme.gap / 2).max(3);
    let block_h = title_h + author_h + theme.gap + bar_h;

    // The block sets against the middle of `body`.
    cx.text.set_px(theme.body_px);
    let top = body.y + (body.h - block_h).max(0) / 2 + cx.text.cap_height() as i32;
    let mut y = top;
    for line in &lines {
        cx.text.draw_in(script, cx.fb, words.x, y, line, false);
        y += cx.text.line_height() as i32;
    }

    cx.text.set_px(theme.small_px);
    if !book.author.is_empty() {
        y += theme.gap / 2;
        let author = cx
            .text
            .wrap_and_clamp_in(script, &book.author, words.w as u32, 1);
        cx.text.draw_in(
            script,
            cx.fb,
            words.x,
            y,
            author.first().map(String::as_str).unwrap_or_default(),
            false,
        );
    }

    cx.text.set_px(theme.body_px);
    let fw = cx.text.measure_width(&figure) as i32;
    cx.text
        .draw(cx.fb, figures.right() - fw, top, &figure, false);

    // The track runs the width of the words column, closing the block.
    let track = Rect::new(words.x, y + theme.gap, words.w, bar_h);
    // A book the catalog states no progress for draws no track.
    if book.has_percent() {
        paint::progress(cx.fb, track, book.percent as i64, 100, INK);
        cx.text.set_px(theme.small_px);
        let pct = format!("{}%", book.percent.round() as i64);
        let pw = cx.text.measure_width(&pct) as i32;
        cx.text
            .draw(cx.fb, figures.right() - pw, track.bottom(), &pct, false);
    }
    paint::hline(cx.fb, row.x, row.bottom() - 1, row.w, LIGHT, 1);
}

fn empty(cx: &mut Ctx, area: Rect) {
    let theme: &Theme = cx.theme;
    cx.text.set_px(theme.body_px);
    let said = match cx.stats.total_seconds > 0 {
        true => format!(
            "{} read, on books the catalog names none of. A book is listed \
             once the device has said what it is.",
            crate::date::duration(cx.stats.total_seconds)
        ),
        false => "No reading yet. Open a book, read a few pages, then come \
             back — the log starts from the day this first runs."
            .into(),
    };
    let lines = cx.text.wrap_and_clamp(&said, area.w as u32, 4);
    let mut y = area.y + area.h / 3;
    for line in lines {
        cx.text.draw(cx.fb, area.x, y, &line, false);
        y += cx.text.line_height() as i32;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ui::chrome;

    /// A content box holding exactly `rows` rows and the page counter.
    fn area_for(theme: &Theme, rows: i32) -> Rect {
        let box_ = chrome::content_box(theme);
        let h = row_height(theme) * rows + foot_height(theme);
        Rect::new(box_.x, box_.y, box_.w, h)
    }

    #[test]
    fn a_page_holds_what_fits_and_never_none() {
        let theme = Theme::for_screen(1264, 1680);
        assert_eq!(rows_per_page(&theme, area_for(&theme, 6)), 6);
        assert_eq!(rows_per_page(&theme, area_for(&theme, 1)), 1);
        // A box under one row tall shows one.
        assert_eq!(rows_per_page(&theme, Rect::new(0, 0, 100, 1)), 1);
    }

    #[test]
    fn the_page_counter_never_lands_on_the_last_row() {
        for (w, h) in [(1264, 1680), (1272, 1696), (1860, 2480)] {
            let theme = Theme::for_screen(w, h);
            let area = chrome::content_box(&theme);
            let rows = rows_per_page(&theme, area) as i32;
            let bottom = area.y + rows * row_span(&theme, area);
            let foot = area.bottom() - foot_height(&theme);
            assert!(
                bottom <= foot,
                "{w}x{h}: rows end at {bottom}, foot at {foot}"
            );
        }
    }

    #[test]
    fn a_cover_is_worth_looking_at_on_every_panel() {
        for (w, h) in [(1264, 1680), (1272, 1696), (1860, 2480)] {
            let theme = Theme::for_screen(w, h);
            let art = cover::width_for(row_height(&theme) - theme.gap * 2);
            // `art` against the panel's own width.
            assert!(art >= 100, "{w}x{h}: a {art} px cover is a smudge");
            assert!(
                art < theme.screen.w / 6,
                "{w}x{h}: a {art} px cover crowds the words"
            );
        }
    }

    #[test]
    fn the_last_page_is_a_full_one() {
        let theme = Theme::for_screen(1264, 1680);
        let area = area_for(&theme, 6);
        // 20 books, 6 to a page: the last page opens at 14 and holds 14..20.
        assert_eq!(last_page_at(&theme, area, 20), 14);
        // Fewer books than a page: the list never scrolls.
        assert_eq!(last_page_at(&theme, area, 6), 0);
        assert_eq!(last_page_at(&theme, area, 2), 0);
        assert_eq!(last_page_at(&theme, area, 0), 0);
    }

    #[test]
    fn paging_walks_the_whole_list_without_a_stranded_row() {
        let theme = Theme::for_screen(1264, 1680);
        let area = area_for(&theme, 6);
        let (count, step) = (20usize, rows_per_page(&theme, area));
        let last = last_page_at(&theme, area, count);

        let mut from = 0usize;
        let mut seen = 0usize;
        loop {
            let to = (from + step).min(count);
            seen = seen.max(to);
            let next = (from + step).min(last);
            if next == from {
                break;
            }
            from = next;
        }
        assert_eq!(seen, count, "every book is reachable");
        assert_eq!(from, last);
    }
}
