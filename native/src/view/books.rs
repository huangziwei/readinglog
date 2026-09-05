//! Every [`crate::stats::BookStat`], most recent first, with its cover, the
//! progress the catalog states, and a filled figure on a book read through.

use crate::date;
use crate::stats::{BookStat, Stats};
use crate::ui::chrome;
use crate::ui::cover;
use crate::ui::paint::{self, INK, LIGHT, Rect};
use crate::ui::theme::Theme;

use super::{Ctx, Hit, Shelf, Sort, State, band};

/// Lines a title takes before the rest of it is ellipsized.
const TITLE_LINES: usize = 2;

/// The marks either side of the page count, opening the first page and the
/// last. The bar is the end of the list, the chevron the way to it.
const JUMP_FIRST: &str = "|‹";
const JUMP_LAST: &str = "›|";

/// The height one book takes, set by the cover it carries.
fn row_height(theme: &Theme) -> i32 {
    theme.row_h * 5 / 2
}

/// The strip under the rows that the page counter sits in, with a line under
/// it for what the record holds beyond the list.
fn foot_height(theme: &Theme) -> i32 {
    theme.small_px as i32 * 7 / 2
}

/// The width of the figures column, from `figure` alone: the band under it
/// carries the percentage.
fn figures_width(cx: &mut Ctx, figure: &str) -> i32 {
    cx.text.measure_width(figure) as i32
}

/// Books on `shelf` in `order`, by their index in [`Stats::books`], which is
/// held most recently read first. [`Sort::Recent`] is that order untouched, and
/// the stable sorts below fall back to it on a tie.
pub fn listed(stats: &Stats, shelf: Shelf, order: Sort) -> Vec<usize> {
    let mut out: Vec<usize> = (0..stats.books.len())
        .filter(|at| match shelf {
            Shelf::All => true,
            Shelf::Finished => stats.books[*at].is_finished(),
            Shelf::Unfinished => !stats.books[*at].is_finished(),
        })
        .collect();
    match order {
        Sort::Recent => {}
        Sort::Longest => out.sort_by_key(|at| -stats.books[*at].seconds),
        // A book the catalog states no percent for sorts as though unopened.
        Sort::Furthest => out.sort_by_key(|at| -stats.books[*at].percent_shown().max(0)),
    }
    out
}

/// Whether a shelf holds anything read through, which is what the `Finished`
/// chip narrows to.
pub fn shelved(stats: &Stats) -> bool {
    stats.books.iter().any(|b| b.is_finished())
}

/// The shelf a tap on the second chip lands on. [`Shelf::Unfinished`] is
/// stepped over where every book is read through.
pub fn tapped_to(stats: &Stats, on: Shelf) -> Shelf {
    match on.cycled() {
        Shelf::Unfinished if stats.books.iter().all(BookStat::is_finished) => Shelf::All,
        next => next,
    }
}

/// The box the rows are drawn into, the shelf chips taken off the top.
pub fn list_box(theme: &Theme, area: Rect, chips: bool) -> Rect {
    match chips {
        true => area.split_top(chrome::chip_height(theme) + theme.gap * 2).1,
        false => area,
    }
}

/// Rows one page of the list holds, [`foot_height`] taken off first.
pub fn rows_per_page(theme: &Theme, area: Rect) -> usize {
    (((area.h - foot_height(theme)) / row_height(theme)).max(1)) as usize
}

/// Where the last page of `count` books opens, in `area`.
pub fn last_page_at(theme: &Theme, area: Rect, count: usize) -> usize {
    super::last_page_at(count, rows_per_page(theme, area))
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
    // The row stands whenever there are books: a shelf with nothing read
    // through offers no `Finished` chip, but every shelf can be reordered.
    let (head, _) = area.split_top(chrome::chip_height(theme) + theme.gap * 2);
    if shelved(cx.stats) {
        shelf_chips(cx, head, state.shelf);
    }
    sort_chip(cx, head, state.sort);
    let area = list_box(theme, area, true);
    let shelf = listed(cx.stats, state.shelf, state.sort);

    let row_h = row_span(theme, area);
    let fits = rows_per_page(theme, area);
    let from = state.books_from.min(last_page_at(theme, area, shelf.len()));
    let to = (from + fits).min(shelf.len());

    for (slot, index) in shelf[from..to].iter().enumerate() {
        let row = Rect::new(area.x, area.y + slot as i32 * row_h, area.w, row_h);
        book_row(cx, row, *index);
        cx.hit(Hit::Book(*index), row);
    }

    // `foot` takes a tap on either half.
    let (foot, _) = area.split_bottom(foot_height(theme));
    cx.text.set_px(theme.small_px);
    let line = cx.text.line_height() as i32;
    let counted = foot.bottom() - line;
    if from > 0 || to < shelf.len() {
        let label = format!("{}–{} {} {}", from + 1, to, cx.s().of, shelf.len());
        centred(cx, foot, counted, &label);
        let (left, right) = foot.split_left(foot.w / 2);
        if from > 0 {
            cx.hit(Hit::Prev, left);
        }
        if to < shelf.len() {
            cx.hit(Hit::Next, right);
        }
        // Drawn after the halves. A tap on one of these takes the whole way;
        // the half under it takes one step.
        let last = last_page_at(theme, area, shelf.len());
        let width = cx.text.measure_width(&label) as i32;
        let ends = Rect::new(foot.x + (foot.w - width) / 2, foot.y, width, foot.h);
        jump(cx, ends, counted, true, (from > 0).then_some(0));
        jump(cx, ends, counted, false, (to < shelf.len()).then_some(last));
    }
    // The record's own count closes the last page of the whole shelf.
    if to == shelf.len() && state.shelf == Shelf::All {
        record_line(cx, foot, counted + line);
    }
}

/// The order the list is in, at the right of the shelf chips' own row. One
/// chip, not one to an order: the row has no width for three more at every
/// text size. A tap opens the order after this one.
fn sort_chip(cx: &mut Ctx, area: Rect, on: Sort) {
    let theme: &Theme = cx.theme;
    let script = cx.ui_script();
    let said = on.label(cx.lang);
    cx.text.set_px(theme.body_px);
    let w = cx.text.measure_width_in(script, said) as i32 + chrome::chip_pad() * 2;
    let chip = Rect::new(
        area.right() - w,
        area.y,
        w.min(area.w),
        chrome::chip_height(theme),
    );
    paint::stroke(cx.fb, chip, INK, 2);
    let tw = cx.text.measure_width_in(script, said) as i32;
    let baseline = chip.center_y() + cx.text.cap_height() as i32 / 2;
    cx.text.draw_in(
        script,
        cx.fb,
        chip.x + (chip.w - tw) / 2,
        baseline,
        said,
        false,
    );
    cx.hit(Hit::Sorted(on.next()), chip);
}

/// The width a jump mark beside the page count takes, its air included.
fn jump_reach(cx: &mut Ctx) -> i32 {
    let theme: &Theme = cx.theme;
    cx.text.set_px(theme.head_px);
    cx.text.measure_width(JUMP_LAST) as i32 + theme.gap * 4
}

/// A mark to one side of the page count, opening the list at `at`, and nothing
/// where the list stands at that end. `count` is the box the count is
/// set in; the mark stands clear of it.
fn jump(cx: &mut Ctx, count: Rect, baseline: i32, at_left: bool, at: Option<usize>) {
    let theme: &Theme = cx.theme;
    let Some(at) = at else { return };
    let reach = jump_reach(cx);
    let said = match at_left {
        true => JUMP_FIRST,
        false => JUMP_LAST,
    };
    let box_ = match at_left {
        true => Rect::new(count.x - reach, count.y, reach, count.h),
        false => Rect::new(count.right(), count.y, reach, count.h),
    };
    cx.text.set_px(theme.head_px);
    let w = cx.text.measure_width(said) as i32;
    cx.text
        .draw(cx.fb, box_.x + (box_.w - w) / 2, baseline, said, false);
    cx.hit(Hit::BooksPage(at), box_);
}

/// How many books the record holds, and how many of them no row can name.
fn record_line(cx: &mut Ctx, foot: Rect, baseline: i32) {
    let s = cx.s();
    let unnamed = cx.stats.unnamed_books();
    if unnamed == 0 {
        return;
    }
    let said = format!(
        "{} {} · {unnamed} {}",
        cx.stats.books.len() + unnamed,
        s.in_the_record,
        s.unidentified
    );
    centred(cx, foot, baseline, &said);
}

/// `said` centred in `foot`, on `baseline`.
fn centred(cx: &mut Ctx, foot: Rect, baseline: i32, said: &str) {
    let script = cx.ui_script();
    cx.text.set_px(cx.theme.small_px);
    let w = cx.text.measure_width_in(script, said) as i32;
    cx.text.draw_in(
        script,
        cx.fb,
        foot.x + (foot.w - w) / 2,
        baseline,
        said,
        false,
    );
}

/// The shelves as a chip apiece, the one showing filled, each its own hit box.
/// The second chip carries [`tapped_to`].
fn shelf_chips(cx: &mut Ctx, area: Rect, on: Shelf) {
    let theme: &Theme = cx.theme;
    let script = cx.ui_script();
    let shelves = on.chips();
    let options: Vec<(&str, crate::font::Script)> = shelves
        .iter()
        .map(|shelf| (shelf.label(cx.lang), script))
        .collect();
    let placed = chrome::chip_layout(cx.text, theme, &options, area.w);
    let at = usize::from(on != Shelf::All);
    let cycled = tapped_to(cx.stats, on);
    let drawn = chrome::chips(cx.fb, cx.text, theme, area, &options, &placed, at);
    for (slot, chip) in drawn.into_iter().enumerate() {
        let to = match slot {
            0 => Shelf::All,
            _ => cycled,
        };
        cx.hit(Hit::Shelved(to), chip);
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
    let figure = date::duration(book.seconds, cx.s());
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
    let band_h = band::height(cx.text, theme);
    cx.text.set_px(theme.small_px);
    let block_h = title_h + author_h + theme.gap + band_h;

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

    // The band closes the block, across the whole of `body`. A book the catalog
    // states no progress for and no mark draws none.
    if book.has_percent() || book.is_finished() {
        let foot = Rect::new(body.x, y + theme.gap, body.w, band_h);
        band::draw(cx, foot, band::Band::of(book, book.is_finished()));
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
            crate::date::duration(cx.stats.total_seconds, cx.s())
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
    use crate::stats::BookStat;
    use crate::ui::chrome;

    /// A shelf of books at `percents`, held most recent first.
    fn shelf_of(percents: &[f64]) -> Stats {
        let books = percents
            .iter()
            .enumerate()
            .map(|(at, percent)| BookStat {
                extent: at as i64,
                cde_key: format!("KEY{at}"),
                cde_type: "EBOK".into(),
                // The mark the store puts on a place read through.
                finished: *percent >= crate::store::FINISHED_PERCENT,
                title: format!("Book {at}"),
                author: String::new(),
                thumbnail: String::new(),
                percent: *percent,
                on_device: true,
                location: String::new(),
                language: String::new(),
                seconds: 600,
                dwell_seconds: 0,
                awake_seconds: 0,
                sittings: 1,
                page_turns: 0,
                words: 0,
                days: 1,
                first_day: 0,
                last_day: 0,
                last_secs: 0,
            })
            .collect();
        Stats {
            books,
            ..Stats::default()
        }
    }

    #[test]
    fn the_second_chip_cycles_through_every_shelf_and_back() {
        assert_eq!(Shelf::All.cycled(), Shelf::Finished);
        assert_eq!(Shelf::Finished.cycled(), Shelf::Unfinished);
        assert_eq!(Shelf::Unfinished.cycled(), Shelf::All);
        // The chip states the shelf it is on, and rests on `Finished`.
        assert_eq!(Shelf::All.chips()[1], Shelf::Finished);
        assert_eq!(Shelf::Finished.chips()[1], Shelf::Finished);
        assert_eq!(Shelf::Unfinished.chips()[1], Shelf::Unfinished);
    }

    #[test]
    fn a_record_read_through_end_to_end_steps_over_the_empty_shelf() {
        let all_done = shelf_of(&[100.0, 100.0]);
        assert_eq!(tapped_to(&all_done, Shelf::All), Shelf::Finished);
        assert_eq!(tapped_to(&all_done, Shelf::Finished), Shelf::All);
        // A record with anything left in it takes the whole cycle.
        let mixed = shelf_of(&[100.0, 40.0]);
        assert_eq!(tapped_to(&mixed, Shelf::Finished), Shelf::Unfinished);
        assert_eq!(tapped_to(&mixed, Shelf::Unfinished), Shelf::All);
    }

    #[test]
    fn a_shelf_holding_the_finished_holds_none_of_them_on_the_next_tap() {
        // 100 and 99.9 are read through; 98 and a book with no figure are not.
        let stats = shelf_of(&[100.0, 98.0, -1.0, 99.9]);
        assert_eq!(listed(&stats, Shelf::All, Sort::Recent), [0, 1, 2, 3]);
        assert_eq!(listed(&stats, Shelf::Finished, Sort::Recent), [0, 3]);
        assert_eq!(listed(&stats, Shelf::Unfinished, Sort::Recent), [1, 2]);
    }

    #[test]
    fn furthest_over_the_unfinished_opens_on_the_nearest_to_the_end() {
        let stats = shelf_of(&[100.0, 40.0, -1.0, 100.0, 92.0]);
        // Every book read through leads on `Furthest`, and the shelf without
        // them opens where reading is left.
        assert_eq!(listed(&stats, Shelf::All, Sort::Furthest), [0, 3, 4, 1, 2]);
        assert_eq!(listed(&stats, Shelf::Unfinished, Sort::Furthest), [4, 1, 2]);
    }

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
    fn the_last_page_is_the_short_one() {
        let theme = Theme::for_screen(1264, 1680);
        let area = area_for(&theme, 6);
        // 20 books, 6 to a page: 0..6, 6..12, 12..18, and 18..20 to close.
        assert_eq!(last_page_at(&theme, area, 20), 18);
        // A page and one over opens its second page on that one book.
        assert_eq!(last_page_at(&theme, area, 7), 6);
        // Fewer books than a page: the list never scrolls.
        assert_eq!(last_page_at(&theme, area, 6), 0);
        assert_eq!(last_page_at(&theme, area, 2), 0);
        assert_eq!(last_page_at(&theme, area, 0), 0);
    }

    #[test]
    fn the_pages_tile_the_list_and_repeat_no_book() {
        let theme = Theme::for_screen(1264, 1680);
        let area = area_for(&theme, 6);
        let step = rows_per_page(&theme, area);
        for count in 0..=40usize {
            let last = last_page_at(&theme, area, count);
            let (mut from, mut seen) = (0usize, 0usize);
            loop {
                assert_eq!(from, seen, "{count} books: {from} repeats a row");
                seen = (from + step).min(count);
                let next = (from + step).min(last);
                if next == from {
                    break;
                }
                from = next;
            }
            assert_eq!(seen, count, "{count} books: {seen} of them reachable");
            assert_eq!(from, last, "{count} books: paging stops short of {last}");
        }
    }
}
