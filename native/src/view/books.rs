//! Every [`crate::stats::BookStat`], most recent first, with its cover, the
//! progress the catalog states, and a filled figure on a book read through.

use crate::date;
use crate::stats::{BookStat, Stats};
use crate::ui::chrome;
use crate::ui::cover;
use crate::ui::paint::{self, INK, LIGHT, Rect};
use crate::ui::theme::Theme;

use super::{Ctx, Hit, Shelf, Sort, State, Window, band};

/// Lines a title takes before the rest of it is ellipsized.
const TITLE_LINES: usize = 2;

/// The four marks along the foot: both ends, and one step either way.
const JUMP_FIRST: &str = "«";
const JUMP_LAST: &str = "»";
const STEP_BACK: &str = "‹";
const STEP_ON: &str = "›";

/// The mark on the window chip, which a tap on it takes off the list.
const DROP: &str = "×";

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

/// Books on `shelf` in `order`, by their index in [`Stats::books`], held most
/// recently read first. [`Sort::Recent`] is that order untouched, and the
/// stable sorts fall back to it on a tie.
pub fn listed(
    stats: &Stats,
    shelf: Shelf,
    order: Sort,
    days: Option<std::ops::RangeInclusive<i64>>,
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
        .collect();
    match order {
        Sort::Recent => {}
        Sort::Time => out.sort_by_key(|at| -stats.books[*at].seconds),
        // A book the catalog states no percent for sorts as though unopened.
        Sort::Progress => out.sort_by_key(|at| -stats.books[*at].percent_shown().max(0)),
    }
    out
}

/// Whether a shelf holds anything read through, which is what the `Finished`
/// chip narrows to.
pub fn shelved(stats: &Stats) -> bool {
    stats.books.iter().any(|b| b.is_finished())
}

/// The shelves the row draws a chip apiece for. A record read through end to
/// end offers no `Unfinished` chip, that shelf holding nothing — unless it is
/// the shelf showing, which the row names wherever the list stands.
pub fn shelves(stats: &Stats, on: Shelf) -> &'static [Shelf] {
    const EVERY: [Shelf; 3] = [Shelf::All, Shelf::Finished, Shelf::Unfinished];
    let read_through = stats.books.iter().all(BookStat::is_finished);
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

/// Rows one page of the list holds, `foot_height` taken off first.
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
    let sort = sort_chip(cx, head, state.sort);
    let opens = match shelved(cx.stats) {
        true => shelf_chips(cx, head, state.shelf, state.window) + theme.gap * 2,
        false => head.x,
    };
    if let Some(window) = state.window {
        window_chip(cx, head, opens, sort.x - theme.gap * 2, window, state.shelf);
    }
    let area = list_box(theme, area, true);
    let over = state.window.map(|window| window.days(cx.week));
    let shelf = listed(cx.stats, state.shelf, state.sort, over);
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

    let (foot, _) = area.split_bottom(foot_height(theme));
    cx.text.set_px(theme.small_px);
    let line = cx.text.line_height() as i32;
    let counted = foot.bottom() - line;
    if from > 0 || to < shelf.len() {
        let last = last_page_at(theme, area, shelf.len());
        let label = format!("{}–{} {} {}", from + 1, to, cx.s().of, shelf.len());
        pager(cx, foot, &label, [from > 0, to < shelf.len()], last);
    }
    // The record's own count closes the last page of the whole shelf.
    if to == shelf.len() && state.shelf == Shelf::All && state.window.is_none() {
        record_line(cx, foot, counted + line);
    }
}

/// The order the list is in, at the right of the shelf chips' own row. One
/// chip for all three orders. A tap opens the order after this one, and the
/// box it took is returned.
fn sort_chip(cx: &mut Ctx, area: Rect, on: Sort) -> Rect {
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
    chip
}

/// The width a mark along the foot takes, its air included.
fn mark_reach(cx: &mut Ctx) -> i32 {
    let theme: &Theme = cx.theme;
    cx.text.set_px(theme.small_px);
    cx.text.measure_width(JUMP_LAST) as i32 + theme.gap * 4
}

/// The baseline `said` takes for its own ink to centre on `foot`, through
/// [`TextRenderer::ink_box`]. `foot.center_y()` where `said` inks nothing.
fn on_centre(cx: &mut Ctx, foot: Rect, said: &str) -> i32 {
    let Some((top, bottom)) = cx.text.ink_box(said) else {
        return foot.center_y();
    };
    foot.center_y() - (top + bottom) / 2
}

/// The pager across `foot`: [`JUMP_FIRST`] and [`JUMP_LAST`] at the ends of
/// the row, [`STEP_BACK`] and [`STEP_ON`] either side of `label`, and `label`
/// itself in the middle. `open` states whether each way leads anywhere.
fn pager(cx: &mut Ctx, foot: Rect, label: &str, open: [bool; 2], last: usize) {
    let theme: &Theme = cx.theme;
    let reach = mark_reach(cx);
    cx.text.set_px(theme.small_px);
    let width = cx.text.measure_width(label) as i32;
    let middle = foot.x + (foot.w - width) / 2;
    // `back` and `on` keep clear of the ends, whatever width `label` takes.
    let back = (middle - reach).max(foot.x + reach);
    let on = (middle + width).min(foot.right() - reach * 2);
    let marks = [
        (foot.x, JUMP_FIRST, open[0].then_some(Hit::BooksPage(0))),
        (back, STEP_BACK, open[0].then_some(Hit::Prev)),
        (on, STEP_ON, open[1].then_some(Hit::Next)),
        (
            foot.right() - reach,
            JUMP_LAST,
            open[1].then_some(Hit::BooksPage(last)),
        ),
    ];
    for (x, said, hit) in marks {
        mark(cx, foot, x, said, hit);
    }
    cx.text.set_px(theme.small_px);
    let baseline = on_centre(cx, foot, label);
    let script = cx.ui_script();
    cx.text
        .draw_in(script, cx.fb, middle, baseline, label, false);
}

/// One mark of the pager, [`mark_reach`] wide at `x`, taking a tap onto
/// `hit`. A `hit` of `None` draws nothing: the list stands at that end.
fn mark(cx: &mut Ctx, foot: Rect, x: i32, said: &str, hit: Option<Hit>) {
    let Some(hit) = hit else { return };
    let reach = mark_reach(cx);
    let box_ = Rect::new(x, foot.y, reach, foot.h);
    cx.text.set_px(cx.theme.small_px);
    let w = cx.text.measure_width(said) as i32;
    let baseline = on_centre(cx, foot, said);
    cx.text
        .draw(cx.fb, box_.x + (box_.w - w) / 2, baseline, said, false);
    cx.hit(hit, box_);
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

/// The shelves as a chip apiece, the one showing filled, each its own hit box,
/// answering the right edge of the last of them. A chip stands for the shelf
/// it names and keeps the window the list is under.
fn shelf_chips(cx: &mut Ctx, area: Rect, on: Shelf, window: Option<Window>) -> i32 {
    let theme: &Theme = cx.theme;
    let script = cx.ui_script();
    let shelves = shelves(cx.stats, on);
    let options: Vec<(&str, crate::font::Script)> = shelves
        .iter()
        .map(|shelf| (shelf.label(cx.lang), script))
        .collect();
    let placed = chrome::chip_layout(cx.text, theme, &options, area.w);
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
    let w = cx.text.measure_width_in(script, &said) as i32 + chrome::chip_pad() * 2;
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
    fn every_shelf_holding_something_gets_its_own_chip() {
        let mixed = shelf_of(&[100.0, 40.0]);
        assert_eq!(
            shelves(&mixed, Shelf::All),
            [Shelf::All, Shelf::Finished, Shelf::Unfinished]
        );
    }

    #[test]
    fn a_record_read_through_end_to_end_offers_no_unfinished_chip() {
        let all_done = shelf_of(&[100.0, 100.0]);
        assert_eq!(
            shelves(&all_done, Shelf::All),
            [Shelf::All, Shelf::Finished]
        );
        // The shelf showing is named wherever the list stands.
        assert_eq!(
            shelves(&all_done, Shelf::Unfinished),
            [Shelf::All, Shelf::Finished, Shelf::Unfinished]
        );
    }

    #[test]
    fn a_shelf_holding_the_finished_holds_none_of_them_on_the_next_tap() {
        // 100 and 99.9 are read through; 98 and a book with no figure are not.
        let stats = shelf_of(&[100.0, 98.0, -1.0, 99.9]);
        assert_eq!(listed(&stats, Shelf::All, Sort::Recent, None), [0, 1, 2, 3]);
        assert_eq!(listed(&stats, Shelf::Finished, Sort::Recent, None), [0, 3]);
        assert_eq!(
            listed(&stats, Shelf::Unfinished, Sort::Recent, None),
            [1, 2]
        );
    }

    #[test]
    fn furthest_over_the_unfinished_opens_on_the_nearest_to_the_end() {
        let stats = shelf_of(&[100.0, 40.0, -1.0, 100.0, 92.0]);
        // Every book read through leads on `Furthest`, and the shelf without
        // them opens where reading is left.
        assert_eq!(
            listed(&stats, Shelf::All, Sort::Progress, None),
            [0, 3, 4, 1, 2]
        );
        assert_eq!(
            listed(&stats, Shelf::Unfinished, Sort::Progress, None),
            [4, 1, 2]
        );
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
        let over = |shelf| listed(&stats, shelf, Sort::Recent, Some(year.clone()));
        assert_eq!(over(Shelf::All), [0, 1]);
        assert_eq!(over(Shelf::Finished), [0]);
        assert_eq!(over(Shelf::Unfinished), [1]);
        // The two shelves under a window cut the whole of it in two, which is
        // what lets a Finished figure state the count the list holds.
        assert_eq!(
            over(Shelf::Finished).len() + over(Shelf::Unfinished).len(),
            2
        );
        // With no window, the book of the year before stands with them.
        assert_eq!(listed(&stats, Shelf::All, Sort::Recent, None), [0, 1, 2]);
    }
}
