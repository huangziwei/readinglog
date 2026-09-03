//! `draw` fills the content box with `cx.today`: its figures, its timeline,
//! and the books read on it.

use crate::date;
use crate::font::Script;
use crate::ui::cover;
use crate::ui::paint::{self, INK, PALE, Rect};
use crate::ui::{charts, chrome, theme::Theme};

use super::{Ctx, Hit};

/// Rows of `theme.row_h` that `bands` gives the timeline, axis labels
/// included.
const STRIP_ROWS: i32 = 2;

/// Half-rows of air `bands` sets over the figures and under them.
const FIGURE_AIR: (i32, i32) = (1, 2);

/// The shortest height `book_row` draws into: a cover, a title, an author, a
/// progress bar and a span strip.
fn row_floor(theme: &Theme) -> i32 {
    theme.row_h * 2
}

/// Lines a title takes before the rest of it is ellipsized.
const TITLE_LINES: usize = 2;

/// The three bands `draw` fills, top to bottom. `top` takes `figures` and
/// `strip` takes `head` plus `STRIP_ROWS`, each with its air; `list` takes
/// what is left.
fn bands(area: Rect, theme: &Theme, figures: i32, head: i32) -> [Rect; 3] {
    let air = theme.gap * 2;
    let (over, under) = (
        theme.row_h * FIGURE_AIR.0 / 2,
        theme.row_h * FIGURE_AIR.1 / 2,
    );
    let (top, rest) = area.split_top(over + figures + under);
    let (strip, list) = rest.split_top(head + theme.row_h * STRIP_ROWS + air);
    [
        Rect::new(top.x, top.y + over, top.w, figures.min(top.h)),
        Rect::new(strip.x, strip.y, strip.w, (strip.h - air).max(0)),
        list,
    ]
}

pub fn draw(cx: &mut Ctx, area: Rect) {
    let theme: &Theme = cx.theme;
    let today = cx.today;
    let s = cx.s();

    let read = cx.stats.book_totals(today..=today);
    let figures = chrome::figure_height(cx.text, theme);
    let head = chrome::section_height(cx.text, theme);
    let [top, strip, list] = bands(area, theme, figures, head);

    let secs = cx.stats.day_seconds(today);
    let turns: i64 = cx.stats.sittings_on(today).map(|s| s.page_turns).sum();
    let stated = [
        (date::duration(secs, s), s.read_today),
        (turns.to_string(), s.pages_turned),
        (cx.stats.current_streak.to_string(), s.current_streak),
    ];
    // `spread` sizes each figure to its own width across `top`.
    let widths: Vec<i32> = stated
        .iter()
        .map(|(value, label)| chrome::figure_width(cx.text, theme, value, label))
        .collect();
    let cells = top.spread(&widths, theme.gap * 2);
    for (cell, (value, label)) in cells.into_iter().zip(&stated) {
        chrome::figure(cx.fb, cx.text, theme, cell, value, label);
    }

    let inner = chrome::section(
        cx.fb,
        cx.text,
        theme,
        strip,
        &date::long_day(today, s).to_uppercase(),
    );
    let spans = cx.stats.day_blocks(today);
    charts::timeline(cx.fb, cx.text, theme, inner, &spans, Some(cx.now));

    // `title` names `shown` against `read.len()` where `fits` drops a book.
    let shown = fits(theme, list.h - head, read.len());
    let title = match shown < read.len() {
        true => format!("{} — {shown} {} {}", s.what_was_read, s.of, read.len()),
        false => s.what_was_read.to_string(),
    };
    let inner = chrome::section(cx.fb, cx.text, theme, list, &title);
    books(cx, inner, &read[..shown]);
}

/// Books an `h`-tall list holds, of `count`.
fn fits(theme: &Theme, h: i32, count: usize) -> usize {
    count.min(((h / row_floor(theme)).max(1)) as usize)
}

/// `read` down `area`, one book to a row, each row a hit box onto that book.
fn books(cx: &mut Ctx, area: Rect, read: &[(usize, i64)]) {
    let theme: &Theme = cx.theme;
    if read.is_empty() {
        cx.text.set_px(theme.body_px);
        let baseline = area.y + cx.text.line_height() as i32;
        let script = cx.ui_script();
        cx.text
            .draw_in(script, cx.fb, area.x, baseline, cx.s().nothing_read, false);
        return;
    }
    let floor = row_floor(theme);
    let each = (area.h / read.len() as i32).clamp(floor, floor * 3 / 2);
    for (slot, (index, secs)) in read.iter().enumerate() {
        let row = Rect::new(area.x, area.y + slot as i32 * each, area.w, each);
        book_row(cx, row, *index, *secs);
        cx.hit(Hit::Book(*index), row);
    }
}

/// The book at `index`: its cover, title, author, `secs`, its `percent`, and
/// across the foot of `row` the spans `cx.today` holds on it.
fn book_row(cx: &mut Ctx, row: Rect, index: usize, secs: i64) {
    let theme: &Theme = cx.theme;
    let book = &cx.stats.books[index];
    let script = Script::of_language(&book.language);
    let title = book.title.clone();
    let author = book.author.clone();
    let percent = book.has_percent().then_some(book.percent);

    // `spans` takes the full width of `row`; `over` holds the book.
    let (spans, over) = row.split_bottom(theme.gap * 3);
    let inner = over.inset(theme.gap);
    let (art, rest) = inner.split_left(cover::width_for(inner.h));
    cx.covers.draw(cx.fb, art, &book.thumbnail);

    let body = Rect::new(
        art.right() + theme.gap * 2,
        inner.y,
        rest.w - theme.gap * 2,
        inner.h,
    );

    // `words` on the left, `figures` on the right.
    let figure = date::duration(secs, cx.s());
    cx.text.set_px(theme.body_px);
    let column_w = figures_width(cx, &figure) + theme.gap * 2;
    let (words, figures) = body.split_left((body.w - column_w).max(theme.gap));
    let words = Rect::new(words.x, words.y, (words.w - theme.gap * 2).max(1), words.h);

    let lines = cx
        .text
        .wrap_and_clamp_in(script, &title, words.w as u32, TITLE_LINES);
    let title_h = lines.len() as i32 * cx.text.line_height() as i32;
    cx.text.set_px(theme.small_px);
    let author_h = match author.is_empty() {
        true => 0,
        false => theme.gap / 2 + cx.text.line_height() as i32,
    };
    let bar_h = (theme.gap / 2).max(3);
    let block_h = title_h + author_h + theme.gap + bar_h;

    // `block_h` centres in `body`.
    cx.text.set_px(theme.body_px);
    let top = body.y + (body.h - block_h).max(0) / 2 + cx.text.cap_height() as i32;
    let mut y = top;
    for line in &lines {
        cx.text.draw_in(script, cx.fb, words.x, y, line, false);
        y += cx.text.line_height() as i32;
    }

    cx.text.set_px(theme.small_px);
    if !author.is_empty() {
        y += theme.gap / 2;
        let clipped = cx
            .text
            .wrap_and_clamp_in(script, &author, words.w as u32, 1);
        cx.text.draw_in(
            script,
            cx.fb,
            words.x,
            y,
            clipped.first().map(String::as_str).unwrap_or_default(),
            false,
        );
    }

    cx.text.set_px(theme.body_px);
    let fw = cx.text.measure_width(&figure) as i32;
    cx.text
        .draw(cx.fb, figures.right() - fw, top, &figure, false);

    // `track` takes the width of `words`, under the last line drawn.
    if let Some(percent) = percent {
        let track = Rect::new(words.x, y + theme.gap, words.w, bar_h);
        paint::progress(cx.fb, track, percent as i64, 100, INK);
        cx.text.set_px(theme.small_px);
        let pct = format!("{}%", percent.round() as i64);
        let pw = cx.text.measure_width(&pct) as i32;
        cx.text
            .draw(cx.fb, figures.right() - pw, track.bottom(), &pct, false);
    }

    let blocks = cx.stats.day_blocks_of(cx.today, Some(index));
    day_spans(cx.fb, spans, &blocks, theme);
}

/// The width of the figures column: the wider of `figure` and `100%`.
fn figures_width(cx: &mut Ctx, figure: &str) -> i32 {
    let duration = cx.text.measure_width(figure) as i32;
    cx.text.set_px(cx.theme.small_px);
    let pct = cx.text.measure_width("100%") as i32;
    cx.text.set_px(cx.theme.body_px);
    duration.max(pct)
}

/// `spans` laid across `area` on the twenty-four hours `charts::timeline`
/// covers. The track takes the width of `area`.
fn day_spans(
    fb: &mut crate::eink::fb::Framebuffer,
    area: Rect,
    spans: &[(i64, i64)],
    theme: &Theme,
) {
    let h = theme.gap.max(3);
    let track = Rect::new(area.x, area.center_y() - h / 2, area.w, h);
    paint::fill(fb, track, PALE);
    let at = |secs: i64| track.x + (track.w as i64 * secs.clamp(0, 86_400) / 86_400) as i32;
    for (from, to) in spans {
        // `w` floors at 3 px.
        let x = at(*from);
        let w = (at(*to) - x).max(3);
        paint::fill(fb, Rect::new(x, track.y, w, track.h), INK);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Stand-ins for `figure_height` and `section_height`.
    const FIGURES: i32 = 90;
    const HEAD: i32 = 40;

    #[test]
    fn the_bands_run_in_order_with_air_between_and_fill_the_page() {
        let theme = Theme::for_screen(1264, 1680);
        let area = crate::ui::chrome::content_box(&theme);
        let [top, strip, list] = bands(area, &theme, FIGURES, HEAD);

        assert_eq!(list.bottom(), area.bottom());
        for pair in [[top, strip], [strip, list]] {
            let air = pair[1].y - pair[0].bottom();
            assert!(air >= theme.gap, "bands touch: {air}");
        }
        assert_eq!(top.h, FIGURES, "the figures get the height they asked for");
        assert_eq!(strip.h, HEAD + theme.row_h * STRIP_ROWS);
    }

    #[test]
    fn the_figures_stand_clear_of_the_edge_and_of_the_day_under_them() {
        let theme = Theme::for_screen(1264, 1680);
        let area = crate::ui::chrome::content_box(&theme);
        let [top, strip, _] = bands(area, &theme, FIGURES, HEAD);

        assert!(
            top.y - area.y >= theme.gap * 2,
            "the figures crowd the top of the page: {}",
            top.y - area.y
        );
        assert!(
            strip.y - top.bottom() >= theme.row_h,
            "the day crowds the figures: {}",
            strip.y - top.bottom()
        );
    }

    #[test]
    fn the_strip_stays_a_strip_and_the_books_take_the_page() {
        // `strip.h` holds at `STRIP_ROWS` on every panel.
        let theme = Theme::for_screen(1264, 1680);
        for (w, h) in [(1264, 1680), (1860, 2480)] {
            let area = crate::ui::chrome::content(&theme, Rect::new(0, 0, w, h));
            let [_, strip, list] = bands(area, &theme, FIGURES, HEAD);
            assert_eq!(strip.h, HEAD + theme.row_h * STRIP_ROWS, "{w}x{h}");
            assert!(
                list.h > strip.h,
                "{w}x{h}: the strip outgrew the list it heads, {} against {}",
                strip.h,
                list.h
            );
        }
    }

    #[test]
    fn a_day_of_several_books_fits_on_one_page() {
        // `bands` leaves `list` four rows of `row_floor` on every panel.
        for (w, h) in [(1264, 1680), (1272, 1696), (1860, 2480)] {
            let theme = Theme::for_screen(w, h);
            let area = crate::ui::chrome::content(&theme, Rect::new(0, 0, w as i32, h as i32));
            let [_, _, list] = bands(area, &theme, FIGURES, HEAD);
            let rows = fits(&theme, list.h - HEAD, 99);
            assert!(rows >= 4, "{w}x{h}: room for {rows} of the day's books");
        }
    }

    #[test]
    fn a_list_never_counts_more_rows_than_it_can_draw() {
        let theme = Theme::for_screen(1264, 1680);
        for h in [200, 400, 900, 1100] {
            for count in 0..=9usize {
                let shown = fits(&theme, h, count);
                assert!(shown <= count, "{h} px: {shown} rows out of {count} books");
                if shown > 0 {
                    let floor = row_floor(&theme);
                    let each = (h / shown as i32).clamp(floor, floor * 3 / 2);
                    assert!(
                        shown as i32 * each <= h.max(floor),
                        "{h} px: {shown} rows of {each} overrun it"
                    );
                }
            }
        }
    }

    #[test]
    fn a_page_too_short_for_every_band_keeps_them_all_on_it() {
        let theme = Theme::for_screen(1264, 1680);
        let area = Rect::new(0, 0, 1186, 300);
        let out = bands(area, &theme, FIGURES, HEAD);
        for band in out {
            assert!(band.h >= 0, "{band:?}");
            assert!(band.y >= area.y, "{band:?} starts above the page");
            assert!(band.bottom() <= area.bottom(), "{band:?} runs off the page");
        }
        for pair in out.windows(2) {
            assert!(pair[1].y >= pair[0].bottom(), "{pair:?} overlap");
        }
    }
}
