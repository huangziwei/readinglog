//! One day's books, a row each: the cover, the title, how long it was read
//! that day, how far through it is, and where in the day the reading fell.
//!
//! Today and Rhythm both end in this list, so a day reads the same whichever
//! screen reached it.

use crate::date;
use crate::font::Script;
use crate::ui::cover;
use crate::ui::paint::{self, INK, PALE, Rect};
use crate::ui::theme::Theme;

use super::{Ctx, Hit};

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

/// `read` down `area`, one book to a row, each row a hit box onto that book.
///
/// The span strips are `day`'s own, so they line up under the timeline the
/// caller drew for it.
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
    let floor = row_floor(theme);
    let each = (area.h / read.len() as i32).clamp(floor, floor * 3 / 2);
    for (slot, (index, secs)) in read.iter().enumerate() {
        let box_ = Rect::new(area.x, area.y + slot as i32 * each, area.w, each);
        row(cx, box_, day, *index, *secs);
        cx.hit(Hit::Book(*index), box_);
    }
}

/// The book at `index`: its cover, title, author, `secs`, its `percent`, and
/// across the foot of `area` the spans `day` holds on it.
fn row(cx: &mut Ctx, area: Rect, day: i64, index: usize, secs: i64) {
    let theme: &Theme = cx.theme;
    let book = &cx.stats.books[index];
    let script = Script::of_language(&book.language);
    let title = book.title.clone();
    let author = book.author.clone();
    let percent = book.has_percent().then_some(book.percent);

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

    let blocks = cx.stats.day_blocks_of(day, Some(index));
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
