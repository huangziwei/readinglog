//! One day's books, a row each: the cover, the title, how long it was read
//! that day, the place it stood at as the day ended, and where in the day the
//! reading fell. `home` and `rhythm` both end here.

use crate::date;
use crate::font::Script;
use crate::ui::cover;
use crate::ui::paint::{self, INK, LIGHT, PALE, Rect};
use crate::ui::theme::Theme;

use crate::ui::chrome;

use super::{Ctx, Hit, band};

/// Lines a title takes before the rest of it is ellipsized.
const TITLE_LINES: usize = 2;

/// The shortest height [`row`] draws into: a cover, a title, an author, a
/// progress bar and a span strip.
pub fn row_floor(theme: &Theme) -> i32 {
    theme.row_h * 2
}

/// Books an `h`-tall list holds, of `count`.
pub fn fits(theme: &Theme, h: i32, count: usize) -> usize {
    count.min(((h / row_floor(theme)).max(1)) as usize)
}

/// One day's books under their own heading, opened at `from`, which is held
/// inside the list. `home` and `rhythm::day_page` both call it: a day's books
/// read and page the same from either.
pub fn paged(cx: &mut Ctx, area: Rect, day: i64, from: usize) {
    let theme: &Theme = cx.theme;
    // The strip `section` sets its title in, taken before the call.
    let bar = Rect::new(
        area.x,
        area.y,
        area.w,
        chrome::section_height(cx.text, theme),
    );
    let inner = chrome::section(cx.fb, cx.text, theme, area, cx.s().what_was_read);
    let read = cx.stats.book_totals(day..=day);
    let box_ = rows_box(cx, inner, day);
    let deep = fits(theme, box_.h, read.len());
    let from = from.min(super::last_page_at(read.len(), deep));
    let to = (from + deep).min(read.len());
    if read.len() > deep {
        pager(cx, bar, from, to, read.len(), deep);
    }
    draw_noting(cx, inner, day, &read[from..to]);
}

/// `from`–`to` of `count` at the right of the list's heading, a chip either
/// side of it stepping by `deep` and carrying the index it opens at. The two
/// straddle the count, each its own target.
fn pager(cx: &mut Ctx, head: Rect, from: usize, to: usize, count: usize, deep: usize) {
    let theme: &Theme = cx.theme;
    let last = super::last_page_at(count, deep);
    let of = format!("{}–{to} {} {count}", from + 1, cx.s().of);
    let row = chrome::heading_row(cx.text, theme, head);
    let script = cx.ui_script();
    cx.text.set_px(theme.small_px);
    let said = cx.text.measure_width(&of) as i32;
    let steps = [
        ("‹", Hit::ListPage(from.saturating_sub(deep))),
        ("›", Hit::ListPage((from + deep).min(last))),
    ];
    let chips: Vec<i32> = steps
        .iter()
        .map(|(label, _)| cx.text.measure_width_in(script, label) as i32 + theme.gap * 2)
        .collect();

    let air = theme.gap * 2;
    let whole = chips.iter().sum::<i32>() + said + air * 2;
    let mut x = head.right() - whole;
    heading_chip(cx, row, x, steps[0].0, steps[0].1, chips[0]);
    x += chips[0] + air;

    cx.text.set_px(theme.small_px);
    let baseline = row.center_y() + cx.text.cap_height() as i32 / 2;
    cx.text.draw(cx.fb, x, baseline, &of, false);
    x += said + air;
    heading_chip(cx, row, x, steps[1].0, steps[1].1, chips[1]);
}

/// One chip of a heading, `w` wide at `x` on `row`, taking a tap onto `hit`.
fn heading_chip(cx: &mut Ctx, row: Rect, x: i32, label: &str, hit: Hit, w: i32) {
    let theme: &Theme = cx.theme;
    let script = cx.ui_script();
    let chip = Rect::new(x, row.y, w, row.h);
    paint::stroke(cx.fb, chip, LIGHT, 1);
    cx.text.set_px(theme.small_px);
    let tw = cx.text.measure_width_in(script, label) as i32;
    let baseline = chip.center_y() + cx.text.cap_height() as i32 / 2;
    cx.text.draw_in(
        script,
        cx.fb,
        chip.x + (chip.w - tw) / 2,
        baseline,
        label,
        false,
    );
    cx.hit(hit, chip);
}

/// The height each of `rows` is drawn at: they share `area`, held between
/// [`row_floor`] and half again.
fn row_span(theme: &Theme, area: Rect, rows: usize) -> i32 {
    let floor = row_floor(theme);
    (area.h / rows.max(1) as i32).clamp(floor, floor * 3 / 2)
}

/// Where [`note`] stands under `rows` rows of `box_`. A day of no rows draws
/// `nothing_read`, `empty` tall, and the note stands under that.
fn note_at(theme: &Theme, box_: Rect, rows: usize, empty: i32) -> i32 {
    let drawn = match rows {
        0 => empty,
        n => n as i32 * row_span(theme, box_, n),
    };
    (box_.y + drawn).min(box_.bottom())
}

/// The height [`note`] takes, its rule and its air included.
pub fn note_height(cx: &mut Ctx) -> i32 {
    cx.text.set_px(cx.theme.small_px);
    cx.text.line_height() as i32 + cx.theme.gap * 3
}

/// The line closing a list of books: how many of `books` it could not draw a
/// row for, and the `seconds` on them. A list holding every book draws none.
pub fn note(cx: &mut Ctx, at: Rect, books: usize, seconds: i64) {
    if books == 0 {
        return;
    }
    let theme: &Theme = cx.theme;
    let s = cx.s();
    let said = format!(
        "{books} {} · {}",
        s.unidentified,
        date::duration(seconds, s)
    );
    let script = cx.ui_script();
    cx.text.set_px(theme.small_px);
    paint::hline(cx.fb, at.x, at.y, at.w, PALE, 1);
    let baseline = at.y + theme.gap * 2 + cx.text.cap_height() as i32;
    cx.text.draw_in(script, cx.fb, at.x, baseline, &said, false);
}

/// The part of `area` the rows take, [`note`] holding the foot where `day`
/// holds books no row can name.
pub fn rows_box(cx: &mut Ctx, area: Rect, day: i64) -> Rect {
    let foot = (cx.stats.unnamed_over(day..=day).0 > 0) as i32 * note_height(cx);
    Rect::new(area.x, area.y, area.w, (area.h - foot).max(0))
}

/// `read` down `area`, closed by [`note`] where `day` holds books it cannot
/// name.
pub fn draw_noting(cx: &mut Ctx, area: Rect, day: i64, read: &[(usize, i64)]) {
    let theme: &Theme = cx.theme;
    let (books, seconds) = cx.stats.unnamed_over(day..=day);
    let box_ = rows_box(cx, area, day);
    let shown = fits(theme, box_.h, read.len());
    draw(cx, box_, day, &read[..shown]);
    cx.text.set_px(theme.body_px);
    let empty = cx.text.line_height() as i32 + theme.gap * 2;
    let under = note_at(theme, box_, shown, empty);
    note(
        cx,
        Rect::new(area.x, under, area.w, area.bottom() - under),
        books,
        seconds,
    );
}

/// `read` down `area`, one book to a row, each row a hit box onto that book.
/// The span strips are `day`'s own, lining up under the timeline the caller
/// drew for it.
pub fn draw(cx: &mut Ctx, area: Rect, day: i64, read: &[(usize, i64)]) {
    let theme: &Theme = cx.theme;
    if read.is_empty() {
        cx.text.set_px(theme.body_px);
        let baseline = area.y + cx.text.line_height() as i32;
        let script = cx.ui_script();
        cx.text
            .draw_in(script, cx.fb, area.x, baseline, cx.s().nothing_read, false);
        return;
    }
    let each = row_span(theme, area, read.len());
    for (slot, (index, secs)) in read.iter().enumerate() {
        let box_ = Rect::new(area.x, area.y + slot as i32 * each, area.w, each);
        row(cx, box_, day, *index, *secs);
        cx.hit(Hit::Book(*index), box_);
    }
}

/// The book at `index`: its cover, title, author, `secs`, the place it stood
/// at as `day` ended, and across the foot of `area` the spans `day` holds on
/// it.
fn row(cx: &mut Ctx, area: Rect, day: i64, index: usize, secs: i64) {
    let theme: &Theme = cx.theme;
    let percent = cx.stats.percent_over(index, day..=day);
    let done = cx.stats.finished_on(index, day);
    let book = &cx.stats.books[index];
    let script = Script::of_language(&book.language);
    let title = book.title.clone();
    let author = book.author.clone();

    // `spans` takes the full width of `area`; `over` holds the book.
    let (spans, over) = area.split_bottom(theme.gap * 3);
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
    let band_h = band::height(cx.text, theme);
    cx.text.set_px(theme.small_px);
    let block_h = title_h + author_h + theme.gap + band_h;

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

    // The band closes the block, across the whole of `body`.
    if percent.is_some() || done {
        let foot = Rect::new(body.x, y + theme.gap, body.w, band_h);
        let of = band::Band {
            fill: match done {
                true => 100,
                false => percent.unwrap_or(-1),
            },
            at: percent,
            finished: done,
        };
        band::draw(cx, foot, of);
    }

    let blocks = cx.stats.day_blocks_of(day, Some(index));
    day_spans(cx.fb, spans, &blocks, theme);
}

/// The width of the figures column, from `figure` alone: the band under it
/// carries the percentage.
fn figures_width(cx: &mut Ctx, figure: &str) -> i32 {
    cx.text.measure_width(figure) as i32
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

    /// A stand-in for the height `nothing_read` takes.
    const EMPTY: i32 = 60;

    #[test]
    fn the_note_stands_under_what_the_list_drew() {
        let theme = Theme::for_screen(1264, 1680);
        let box_ = Rect::new(0, 100, 1186, 900);
        // A list of no rows wrote a line, which the note clears.
        assert_eq!(note_at(&theme, box_, 0, EMPTY), box_.y + EMPTY);
        for rows in 1..=4usize {
            let at = note_at(&theme, box_, rows, EMPTY);
            let each = row_span(&theme, box_, rows);
            assert_eq!(at, box_.y + rows as i32 * each, "{rows} rows");
            assert!(at > box_.y, "{rows} rows: the note sits on the first row");
        }
        // More rows than the box holds keep the note inside it.
        assert!(note_at(&theme, box_, 40, EMPTY) <= box_.bottom());
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
}
