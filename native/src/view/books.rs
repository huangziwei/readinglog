//! Every [`crate::stats::BookStat`], most recent first, with its cover, the
//! progress the catalog states, and a filled figure on a book read through.

use crate::date;
use crate::stats::{BookStat, Stats};
use crate::ui::chrome;
use crate::ui::cover;
use crate::ui::paint::{self, INK, LIGHT, Rect};
use crate::ui::theme::Theme;

use super::{Ctx, Hit, Shelf, Sort, State, Window, band, pager};

/// Lines a title takes before the rest of it is ellipsized.
const TITLE_LINES: usize = 2;

/// The mark on the window chip, which a tap on it takes off the list.
const DROP: &str = "×";

/// The share of a search box's side [`magnifier`] draws into.
const GLYPH: (i32, i32) = (3, 5);

/// The height one book takes, set by the cover it carries.
fn row_height(theme: &Theme) -> i32 {
    theme.row_h * 5 / 2
}

/// The width `figure` measures at.
fn figures_width(cx: &mut Ctx, figure: &str) -> i32 {
    cx.text.measure_width(figure) as i32
}

/// Books on `shelf` in `order`, by their index in [`Stats::books`], held most
/// recently read first. [`Sort::Recent`] is that order untouched, and the
/// stable sorts fall back to it on a tie.
pub fn listed(
    stats: &Stats,
    shelf: Shelf,
    order: Sort,
    days: Option<std::ops::RangeInclusive<i64>>,
    uncovered: bool,
) -> Vec<usize> {
    let mut out: Vec<usize> = (0..stats.books.len())
        .filter(|at| match shelf {
            Shelf::All => true,
            Shelf::Finished => stats.books[*at].is_finished(),
            Shelf::Unfinished => !stats.books[*at].is_finished(),
        })
        .filter(|at| {
            days.as_ref()
                .is_none_or(|days| days.contains(&stats.books[*at].last_day))
        })
        .filter(|at| uncovered || stats.books[*at].has_cover())
        .collect();
    match order {
        Sort::Recent => {}
        Sort::Time => out.sort_by_key(|at| -stats.books[*at].seconds),
        // A book the catalog states no percent for sorts as though unopened.
        Sort::Progress => out.sort_by_key(|at| -stats.books[*at].percent_shown().max(0)),
    }
    out
}

/// Whether any [`on_show`] book passes [`BookStat::is_finished`]. The
/// `Finished` chip stands on it.
pub fn shelved(stats: &Stats, uncovered: bool) -> bool {
    on_show(stats, uncovered).any(|b| b.is_finished())
}

/// [`Stats::books`], less those failing [`BookStat::has_cover`] where
/// `uncovered` is unset.
fn on_show(stats: &Stats, uncovered: bool) -> impl Iterator<Item = &BookStat> {
    stats
        .books
        .iter()
        .filter(move |b| uncovered || b.has_cover())
}

/// The shelves [`shelf_chips`] draws a chip apiece for. [`Shelf::Unfinished`]
/// drops where every [`on_show`] book passes [`BookStat::is_finished`], and
/// stands where `on` names it.
pub fn shelves(stats: &Stats, on: Shelf, uncovered: bool) -> &'static [Shelf] {
    const EVERY: [Shelf; 3] = [Shelf::All, Shelf::Finished, Shelf::Unfinished];
    let read_through = on_show(stats, uncovered).all(BookStat::is_finished);
    match read_through && on != Shelf::Unfinished {
        true => &EVERY[..2],
        false => &EVERY,
    }
}

/// The box the rows are drawn into, the shelf chips taken off the top.
pub fn list_box(theme: &Theme, area: Rect, chips: bool) -> Rect {
    match chips {
        true => area.split_top(chrome::chip_height(theme) + theme.gap * 2).1,
        false => area,
    }
}

/// Rows one page of the list holds, the pager's own strip taken off first.
pub fn rows_per_page(theme: &Theme, area: Rect) -> usize {
    (((area.h - pager::height(theme)) / row_height(theme)).max(1)) as usize
}

/// Where the last page of `count` books opens, in `area`.
pub fn last_page_at(theme: &Theme, area: Rect, count: usize) -> usize {
    super::last_page_at(count, rows_per_page(theme, area))
}

/// The height a row is drawn at: the page's rows share `area`, capped at
/// [`row_height`] and half again.
pub(super) fn row_span(theme: &Theme, area: Rect) -> i32 {
    let fits = rows_per_page(theme, area) as i32;
    ((area.h - pager::height(theme)) / fits).clamp(row_height(theme), row_height(theme) * 3 / 2)
}

pub fn draw(cx: &mut Ctx, area: Rect, state: &State) {
    let theme: &Theme = cx.theme;
    if cx.stats.books.is_empty() {
        empty(cx, area);
        return;
    }
    // `search_button` and `sort_chip` stand on every shelf.
    let (head, _) = area.split_top(chrome::chip_height(theme) + theme.gap * 2);
    let sort = sort_chip(cx, head, state.sort);
    let search = search_button(cx, head);
    let opens_at = search.right() + chrome::chip_gap(theme);
    let head = Rect::new(opens_at, head.y, head.right() - opens_at, head.h);
    let opens = match shelved(cx.stats, cx.uncovered) {
        true => shelf_chips(cx, head, state.shelf, state.window) + theme.gap * 2,
        false => head.x,
    };
    if let Some(window) = state.window {
        window_chip(cx, head, opens, sort.x - theme.gap * 2, window, state.shelf);
    }
    let area = list_box(theme, area, true);
    let over = state.window.map(|window| window.days(cx.week));
    let shelf = listed(cx.stats, state.shelf, state.sort, over, cx.uncovered);
    if shelf.is_empty() {
        let said = cx.s().nothing_on_the_shelf;
        bare(cx, area, said);
        return;
    }

    let row_h = row_span(theme, area);
    let fits = rows_per_page(theme, area);
    let from = state.books_from.min(last_page_at(theme, area, shelf.len()));
    let to = (from + fits).min(shelf.len());

    for (slot, index) in shelf[from..to].iter().enumerate() {
        let row = Rect::new(area.x, area.y + slot as i32 * row_h, area.w, row_h);
        book_row(cx, row, *index);
        cx.hit(Hit::Book(*index), row);
    }

    if from > 0 || to < shelf.len() {
        let last = last_page_at(theme, area, shelf.len());
        let label = format!("{}–{} {} {}", from + 1, to, cx.s().of, shelf.len());
        pager::draw(
            cx,
            pager::foot(theme, area),
            &label,
            [from > 0, to < shelf.len()],
            [Hit::BooksPage(0), Hit::BooksPage(last)],
        );
    }
}

/// The square the search stands in at the head of a row: `chip_height` a
/// side, at the left edge of `area`. The book screen opens its own head row
/// with this one.
pub(super) fn search_box(theme: &Theme, area: Rect) -> Rect {
    let side = chrome::chip_height(theme);
    Rect::new(area.x, area.y, side, side)
}

/// The search, at the head of the shelf chips' own row: a square of
/// `chrome::chip_height` a side, in their outline. Answers the box it took.
fn search_button(cx: &mut Ctx, area: Rect) -> Rect {
    let theme: &Theme = cx.theme;
    let box_ = search_box(theme, area);
    paint::stroke(cx.fb, box_, INK, theme.rule());
    magnifier(cx, box_);
    cx.hit(Hit::Search, box_);
    box_
}

/// The magnifier centred in `box_`, [`GLYPH`] of its shorter side across.
pub(super) fn magnifier(cx: &mut Ctx, box_: Rect) {
    let glyph = box_.w.min(box_.h) * GLYPH.0 / GLYPH.1;
    paint::magnifier(
        cx.fb,
        Rect::new(
            box_.x + (box_.w - glyph) / 2,
            box_.y + (box_.h - glyph) / 2,
            glyph,
            glyph,
        ),
        INK,
        paint::WHITE,
        (glyph / 7).max(cx.theme.rule()),
    );
}

/// The order the list is in, at the right of the shelf chips' own row. One
/// chip for all three orders. A tap opens the order after this one, and the
/// box it took is returned.
fn sort_chip(cx: &mut Ctx, area: Rect, on: Sort) -> Rect {
    let theme: &Theme = cx.theme;
    let script = cx.ui_script();
    let said = on.label(cx.lang);
    cx.text.set_px(theme.body_px);
    let w = cx.text.measure_width_in(script, said) as i32 + chrome::chip_pad(theme) * 2;
    let chip = Rect::new(
        area.right() - w,
        area.y,
        w.min(area.w),
        chrome::chip_height(theme),
    );
    paint::stroke(cx.fb, chip, INK, theme.rule());
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
    chip
}

/// The shelves as a chip apiece, the one showing filled, each its own hit box,
/// answering the right edge of the last of them. A chip stands for the shelf
/// it names and keeps the window the list is under.
fn shelf_chips(cx: &mut Ctx, area: Rect, on: Shelf, window: Option<Window>) -> i32 {
    let theme: &Theme = cx.theme;
    let script = cx.ui_script();
    let shelves = shelves(cx.stats, on, cx.uncovered);
    let options: Vec<(&str, crate::font::Script)> = shelves
        .iter()
        .map(|shelf| (shelf.label(cx.lang), script))
        .collect();
    let placed = chrome::chip_layout(cx.text, theme, &options, None, area.w);
    let at = shelves.iter().position(|shelf| *shelf == on).unwrap_or(0);
    let drawn = chrome::chips(cx.fb, cx.text, theme, area, &options, &placed, at);
    let mut edge = area.x;
    for (shelf, chip) in shelves.iter().zip(drawn) {
        cx.hit(Hit::Shelved(*shelf, window), chip);
        edge = edge.max(chip.right());
    }
    edge
}

/// The stretch the list is narrowed to, standing between `opens` and `until`,
/// filled the way the shelf showing is. Its name carries [`DROP`], and a tap
/// opens the same shelf over the whole record.
fn window_chip(cx: &mut Ctx, area: Rect, opens: i32, until: i32, window: Window, shelf: Shelf) {
    let theme: &Theme = cx.theme;
    let script = cx.ui_script();
    let said = format!("{} {DROP}", window.name(cx.week, cx.s()));
    cx.text.set_px(theme.body_px);
    let w = cx.text.measure_width_in(script, &said) as i32 + chrome::chip_pad(theme) * 2;
    let chip = Rect::new(
        opens,
        area.y,
        w.min((until - opens).max(1)),
        chrome::chip_height(theme),
    );
    paint::fill(cx.fb, chip, INK);
    let tw = cx.text.measure_width_in(script, &said) as i32;
    let baseline = chip.center_y() + cx.text.cap_height() as i32 / 2;
    cx.text.draw_in(
        script,
        cx.fb,
        chip.x + (chip.w - tw) / 2,
        baseline,
        &said,
        true,
    );
    cx.hit(Hit::Shelved(shelf, None), chip);
}

/// `said` at the head of `area`, for a list with no row to draw.
fn bare(cx: &mut Ctx, area: Rect, said: &str) {
    let script = cx.ui_script();
    cx.text.set_px(cx.theme.body_px);
    let baseline = area.y + cx.text.line_height() as i32;
    cx.text
        .draw_in(script, cx.fb, area.x, baseline, said, false);
}

pub(super) fn book_row(cx: &mut Ctx, row: Rect, index: usize) {
    let theme: &Theme = cx.theme;
    let book = &cx.stats.books[index];
    // `art` and `figures` keep `row`'s own edges.
    let inner = row.inset_y(theme.gap);
    let (art, rest) = inner.split_left(cover::width_for(inner.h));
    // `cover::note` writes into a box holding no jacket.
    if !cx.covers.draw(cx.fb, art, &book.thumbnail) {
        cover::note(cx, art);
    }

    let body = Rect::new(
        art.right() + theme.gap * 2,
        inner.y,
        rest.w - theme.gap * 2,
        inner.h,
    );
    let script = crate::font::Script::of_language(&book.language);

    // Two columns: the words on the left, the figures on the right.
    let figure = date::duration(book.read_seconds(cx.figures), cx.s());
    cx.text.set_px(theme.body_px);
    let column_w = figures_width(cx, &figure) + theme.gap * 2;
    let (words, figures) = body.split_left((body.w - column_w).max(theme.gap));
    let words = Rect::new(words.x, words.y, (words.w - theme.gap * 2).max(1), words.h);

    let lines = cx
        .text
        .wrap_and_clamp_in(script, &book.title, words.w as u32, TITLE_LINES);
    let title_h = lines.len() as i32 * cx.text.line_height() as i32;
    cx.text.set_px(theme.small_px);
    // The line under the title carries `book.author` at its left and
    // `marks_said` at its right. A row with neither gives the line's height
    // back to the block.
    let marked = super::marks_said(cx.s(), cx.stats.marks_counted(index));
    let author_h = match book.author.is_empty() && marked.is_empty() {
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
    if author_h > 0 {
        y += theme.gap / 2;
        let line = Rect::new(words.x, words.y, body.right() - words.x, words.h);
        let author = book.author.clone();
        super::under_title(cx, line, y, script, &author, &marked);
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

/// The line an empty list stands under: reading on nothing the catalog names,
/// nothing since a reset, or nothing at all.
fn empty_line(stats: &crate::stats::Stats, floored: bool, s: &crate::lang::Strings) -> String {
    if stats.total_seconds > 0 {
        return s
            .unnamed_only
            .replace("{t}", &crate::date::duration(stats.total_seconds, s));
    }
    match floored {
        true => s.nothing_since_reset.to_string(),
        false => s.no_reading_yet.to_string(),
    }
}

fn empty(cx: &mut Ctx, area: Rect) {
    let theme: &Theme = cx.theme;
    cx.text.set_px(theme.body_px);
    let said = empty_line(cx.stats, cx.floored, cx.s());
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
    use crate::ui::theme::tests::PANELS;

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
                counted_seconds: 0,
                dwell_seconds: 0,
                awake_seconds: 0,
                sittings: 1,
                page_turns: 0,
                words: 0,
                days: 1,
                first_day: 0,
                last_day: 0,
                last_secs: 0,
                stated_time_left: None,
                stated_wpm: None,
                marks: Vec::new(),
            })
            .collect();
        Stats {
            books,
            ..Stats::default()
        }
    }

    #[test]
    fn every_shelf_holding_something_gets_its_own_chip() {
        let mixed = shelf_of(&[100.0, 40.0]);
        assert_eq!(
            shelves(&mixed, Shelf::All, true),
            [Shelf::All, Shelf::Finished, Shelf::Unfinished]
        );
    }

    #[test]
    fn a_record_read_through_end_to_end_offers_no_unfinished_chip() {
        let all_done = shelf_of(&[100.0, 100.0]);
        assert_eq!(
            shelves(&all_done, Shelf::All, true),
            [Shelf::All, Shelf::Finished]
        );
        // The shelf showing is named wherever the list stands.
        assert_eq!(
            shelves(&all_done, Shelf::Unfinished, true),
            [Shelf::All, Shelf::Finished, Shelf::Unfinished]
        );
    }

    #[test]
    fn a_shelf_holding_the_finished_holds_none_of_them_on_the_next_tap() {
        // 100 and 99.9 are read through; 98 and a book with no figure are not.
        let stats = shelf_of(&[100.0, 98.0, -1.0, 99.9]);
        assert_eq!(
            listed(&stats, Shelf::All, Sort::Recent, None, true),
            [0, 1, 2, 3]
        );
        assert_eq!(
            listed(&stats, Shelf::Finished, Sort::Recent, None, true),
            [0, 3]
        );
        assert_eq!(
            listed(&stats, Shelf::Unfinished, Sort::Recent, None, true),
            [1, 2]
        );
    }

    #[test]
    fn a_chip_never_opens_a_shelf_hiding_holds_nothing_of() {
        // The one book read through is also the one with no jacket.
        let mut stats = shelf_of(&[100.0, 40.0]);
        stats.books[1].thumbnail = "/covers/1.jpg".into();
        assert!(shelved(&stats, true), "the Finished chip stands");
        assert!(!shelved(&stats, false), "it opens an empty shelf");

        // Hiding leaves nothing unfinished.
        let mut done = shelf_of(&[100.0, 40.0]);
        done.books[0].thumbnail = "/covers/0.jpg".into();
        assert_eq!(
            shelves(&done, Shelf::All, true),
            [Shelf::All, Shelf::Finished, Shelf::Unfinished]
        );
        assert_eq!(
            shelves(&done, Shelf::All, false),
            [Shelf::All, Shelf::Finished]
        );
    }

    #[test]
    fn a_shelf_hiding_the_uncovered_keeps_only_the_books_with_a_jacket() {
        let mut stats = shelf_of(&[100.0, 40.0, 60.0, 100.0]);
        for at in [1, 3] {
            stats.books[at].thumbnail = format!("/covers/{at}.jpg");
        }
        assert_eq!(
            listed(&stats, Shelf::All, Sort::Recent, None, true),
            [0, 1, 2, 3]
        );
        assert_eq!(
            listed(&stats, Shelf::All, Sort::Recent, None, false),
            [1, 3]
        );
        // A shelf narrows what is left and never brings one back.
        assert_eq!(
            listed(&stats, Shelf::Finished, Sort::Recent, None, false),
            [3]
        );
    }

    #[test]
    fn hiding_the_uncovered_takes_their_rows_and_leaves_the_seconds_alone() {
        let mut stats = shelf_of(&[100.0, 40.0, 60.0]);
        for at in [0, 2] {
            stats.books[at].thumbnail = format!("/covers/{at}.jpg");
        }
        let read = vec![(0usize, 600i64), (1, 300), (2, 900)];

        let mut every = read.clone();
        crate::view::covered(&stats, true, &mut every);
        assert_eq!(every, read, "a row went while they were shown");

        let mut some = read.clone();
        crate::view::covered(&stats, false, &mut some);
        assert_eq!(some, [(0, 600), (2, 900)]);
        // `stats.books[1].seconds` holds the hidden book's reading.
        assert_eq!(stats.books[1].seconds, 600);
    }

    #[test]
    fn furthest_over_the_unfinished_opens_on_the_nearest_to_the_end() {
        let stats = shelf_of(&[100.0, 40.0, -1.0, 100.0, 92.0]);
        // `Sort::Progress` leads on the books read through.
        assert_eq!(
            listed(&stats, Shelf::All, Sort::Progress, None, true),
            [0, 3, 4, 1, 2]
        );
        assert_eq!(
            listed(&stats, Shelf::Unfinished, Sort::Progress, None, true),
            [4, 1, 2]
        );
    }

    /// A content box holding exactly `rows` rows and the page counter.
    fn area_for(theme: &Theme, rows: i32) -> Rect {
        let box_ = chrome::content_box(theme);
        let h = row_height(theme) * rows + pager::height(theme);
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
        for (w, h) in PANELS {
            let theme = Theme::for_screen(w, h);
            let area = chrome::content_box(&theme);
            let rows = rows_per_page(&theme, area) as i32;
            let bottom = area.y + rows * row_span(&theme, area);
            let foot = area.bottom() - pager::height(&theme);
            assert!(
                bottom <= foot,
                "{w}x{h}: rows end at {bottom}, foot at {foot}"
            );
        }
    }

    #[test]
    fn the_page_counter_centres_on_the_white_it_is_read_against() {
        for (w, h) in PANELS {
            let theme = Theme::for_screen(w, h);
            let box_ = chrome::content_box(&theme);
            let area = list_box(&theme, box_, true);
            let foot = pager::foot(&theme, area);
            // `foot.bottom()` is the tab strip's top edge.
            assert_eq!(
                foot.bottom(),
                h as i32 - theme.tabs_h,
                "{w}x{h}: the strip stops short of the tab strip"
            );
            // The rows meet it: nothing but the counter stands in the band.
            let rows = rows_per_page(&theme, area) as i32;
            let under = foot.y - (area.y + rows * row_span(&theme, area));
            assert!(
                (0..=rows).contains(&under),
                "{w}x{h}: {under} px of nothing between the rows and the strip"
            );
        }
    }

    #[test]
    fn a_cover_is_worth_looking_at_on_every_panel() {
        for (w, h) in PANELS {
            let theme = Theme::for_screen(w, h);
            let art = cover::width_for(row_height(&theme) - theme.gap * 2);
            // `art` against the panel's own density and width.
            assert!(
                art >= theme.px(100),
                "{w}x{h}: a {art} px cover is a smudge"
            );
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

    #[test]
    fn a_window_holds_the_books_last_put_down_inside_it() {
        use crate::settings::WeekStart;
        let mut stats = shelf_of(&[100.0, 40.0, 100.0]);
        let inside = crate::date::days_from_civil(2026, 3, 4);
        let before = crate::date::days_from_civil(2025, 12, 30);
        for (book, day) in stats.books.iter_mut().zip([inside, inside, before]) {
            book.last_day = day;
        }
        let year = Window {
            span: crate::view::Span::Year,
            day: inside,
        }
        .days(WeekStart::Monday);
        let over = |shelf| listed(&stats, shelf, Sort::Recent, Some(year.clone()), true);
        assert_eq!(over(Shelf::All), [0, 1]);
        assert_eq!(over(Shelf::Finished), [0]);
        assert_eq!(over(Shelf::Unfinished), [1]);
        // The two shelves under a window sum to the whole of it.
        assert_eq!(
            over(Shelf::Finished).len() + over(Shelf::Unfinished).len(),
            2
        );
        // With no window, the book of the year before stands with them.
        assert_eq!(
            listed(&stats, Shelf::All, Sort::Recent, None, true),
            [0, 1, 2]
        );
    }
}
