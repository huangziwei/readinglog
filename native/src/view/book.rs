//! One book: its cover, how far through it, how long it has taken, and what is
//! left.

use crate::date;
use crate::font::Script;
use crate::lang::Strings;
use crate::stats::BookStat;
use crate::ui::chrome;
use crate::ui::cover;
use crate::ui::paint::{self, INK, Rect};
use crate::ui::text::TextRenderer;
use crate::ui::{charts, theme::Theme};

use super::{Ctx, Hit};

/// The most columns the strip along the bottom is cut into. A book read over
/// more days than this gives each column a block of them.
const SPAN_COLUMNS: i64 = 30;

/// Lines a title takes before the rest of it is ellipsized.
const TITLE_LINES: usize = 2;

/// The bar stating how far through the book is.
fn bar_height(theme: &Theme) -> i32 {
    theme.gap.max(6)
}

/// The height the progress block takes: the bar and the figure over it.
fn progress_height(text: &mut TextRenderer, theme: &Theme) -> i32 {
    text.set_px(theme.small_px);
    bar_height(theme) + theme.gap / 2 + text.line_height() as i32
}

/// The cover's height: four rows.
fn cover_height(theme: &Theme) -> i32 {
    theme.row_h * 4
}

/// The band the Continue reading control takes, the gap above it included.
/// Nothing where the reader cannot be handed this book.
fn open_height(theme: &Theme, book: &BookStat) -> i32 {
    match book.can_open() {
        true => theme.gap * 2 + chrome::chip_height(theme),
        false => 0,
    }
}

/// Where the control sits inside the band [`open_height`] took: its own height
/// at the foot, the air above it. Separated from the paint so a control the
/// reader cannot reach is caught by a test.
fn open_box(theme: &Theme, band: Rect) -> Rect {
    band.split_bottom(chrome::chip_height(theme)).0
}

/// The height the heading draws into: the cover, the progress under it, and
/// the control under that.
fn heading_height(text: &mut TextRenderer, theme: &Theme, book: &BookStat) -> i32 {
    cover_height(theme) + theme.gap * 2 + progress_height(text, theme) + open_height(theme, book)
}

pub fn draw(cx: &mut Ctx, area: Rect, index: usize) {
    let Some(book) = cx.stats.books.get(index) else {
        return;
    };
    let theme: &Theme = cx.theme;
    let book = book.clone();

    let s = cx.s();
    // Each band takes what it draws into.
    let air = theme.gap * 2;
    let (head, rest) = area.split_top(heading_height(cx.text, theme, &book) + air);
    heading(
        cx,
        Rect::new(head.x, head.y, head.w, head.h - air),
        &book,
        index,
    );

    let head_h = chrome::section_height(cx.text, theme);
    let (chart, facts) = rest.split_bottom(head_h + theme.row_h * 3);
    let facts = Rect::new(facts.x, facts.y, facts.w, (facts.h - air).max(1));
    let inner = chrome::section(cx.fb, cx.text, theme, facts, s.the_reading);

    // [`figures`] states the other three.
    let lines = [
        (s.sittings, book.sittings.to_string()),
        (
            s.days,
            format!(
                "{} {} {}",
                book.days,
                s.of,
                (book.last_day - book.first_day + 1).max(1)
            ),
        ),
        (s.average_a_day, date::duration(book.per_day(), s)),
        (s.average_a_sitting, date::duration(book.per_sitting(), s)),
        (s.words, date::words(book.words)),
        (
            s.reading_speed,
            book.wpm().map_or("—".into(), |w| format!("{w} {}", s.wpm)),
        ),
        (s.started, date::short_day(book.first_day, s)),
        (s.last_read, date::short_day(book.last_day, s)),
        (s.measured_as, measure_note(&book, s)),
        (s.on_the_device, where_note(&book, s)),
    ];
    let rows = inner.rows(lines.len() as i32, 0);
    for ((key, value), row) in lines.iter().zip(&rows) {
        chrome::row(cx.fb, cx.text, theme, *row, key, value);
    }

    // The strip is anchored on the book's own stretch of days and never on
    // `cx.today`: a book put down in the spring states its reading, not an
    // empty summer.
    let (opened, closed) = (book.first_day, book.last_day);
    let span = (closed - opened + 1).max(1);
    // The dates are the axis's to state; two short days in the heading name no
    // year, and a book read across one then reads as a fortnight.
    let named = crate::lang::counted(s.the_journey, span);
    let inner = chrome::section(cx.fb, cx.text, theme, chart, &named);
    let (series, each) = journey(cx, index, opened, closed);
    charts::columns(
        cx.fb,
        cx.text,
        theme,
        cx.palette,
        inner,
        &series,
        move |at| date::short_day(opened + at as i64 * each, s),
        &|secs| super::alltime::duration_rows(secs, s),
        (series.len() / 4).max(1),
        None,
    );
}

/// The seconds read in each column of the strip, and the days one column
/// covers. A book read over [`SPAN_COLUMNS`] days or fewer gets a column each.
fn journey(cx: &Ctx, index: usize, opened: i64, closed: i64) -> (Vec<i64>, i64) {
    let span = (closed - opened + 1).max(1);
    let each = (span + SPAN_COLUMNS - 1) / SPAN_COLUMNS;
    let columns = ((span + each - 1) / each).max(1) as usize;
    let mut series = vec![0i64; columns];
    for (day, secs) in cx.stats.book_days(index) {
        let at = ((day - opened) / each).clamp(0, columns as i64 - 1) as usize;
        series[at] += secs;
    }
    (series, each)
}

/// The cover, the title beside it, the progress bar under both, and the
/// control that hands the book back to the reader.
fn heading(cx: &mut Ctx, area: Rect, book: &BookStat, index: usize) {
    let theme: &Theme = cx.theme;
    let script = Script::of_language(&book.language);

    let band = open_height(theme, book);
    let (open, area) = area.split_bottom(band);
    if band > 0 {
        open_button(cx, open_box(theme, open), book, index);
    }

    let (foot, top) = area.split_bottom(progress_height(cx.text, theme));
    // `top` stops `theme.gap * 2` above `foot`.
    let top = Rect::new(top.x, top.y, top.w, (top.h - theme.gap * 2).max(1));
    let (art, rest) = top.split_left(cover::width_for(top.h));
    // The words stand against the jacket's own edges: its title tops with the
    // cover and its figures stand on the same foot.
    let jacket = cx.covers.box_in(art, &book.thumbnail);
    cx.covers.draw(cx.fb, art, &book.thumbnail);

    let words = Rect::new(
        jacket.right() + theme.gap * 2,
        jacket.y,
        (rest.w + art.right() - jacket.right() - theme.gap * 2).max(1),
        jacket.h,
    );

    cx.text.set_px(theme.head_px);
    let lines = cx
        .text
        .wrap_and_clamp_in(script, &book.title, words.w as u32, TITLE_LINES);
    let mut y = words.y + cx.text.cap_height() as i32;
    for line in &lines {
        cx.text.draw_in(script, cx.fb, words.x, y, line, false);
        y += cx.text.line_height() as i32;
    }
    if !book.author.is_empty() {
        cx.text.set_px(theme.small_px);
        let author = cx
            .text
            .wrap_and_clamp_in(script, &book.author, words.w as u32, 1);
        y += theme.gap;
        cx.text.draw_in(
            script,
            cx.fb,
            words.x,
            y,
            author.first().map(String::as_str).unwrap_or_default(),
            false,
        );
    }

    figures(cx, words, book);

    let s = cx.s();
    if book.has_percent() {
        let track = Rect::new(
            foot.x,
            foot.bottom() - bar_height(theme),
            foot.w,
            bar_height(theme),
        );
        let shown = book.percent_shown();
        paint::progress(cx.fb, track, shown, 100, INK);
        cx.text.set_px(theme.small_px);
        let pct = format!("{shown}% {}", s.read);
        let baseline = track.y - theme.gap / 2;
        cx.text.draw(cx.fb, foot.x, baseline, &pct, false);
        if book.is_finished() {
            finished_chip(cx, foot, baseline);
        }
    }
}

/// A book read through, stated at the right of the line the progress bar
/// carries. Filled, where every other chip on a screen is outlined: this one
/// is not a control and states a book's one end state.
fn finished_chip(cx: &mut Ctx, line: Rect, baseline: i32) {
    let theme: &Theme = cx.theme;
    let said = cx.s().shelf_finished;
    let script = cx.ui_script();
    cx.text.set_px(theme.small_px);
    let (w, cap) = (
        cx.text.measure_width_in(script, said) as i32,
        cx.text.cap_height() as i32,
    );
    let h = cx.text.line_height() as i32 + theme.gap / 2;
    let chip = Rect::new(
        line.right() - w - theme.gap * 2,
        baseline - cap / 2 - h / 2,
        w + theme.gap * 2,
        h,
    );
    paint::fill(cx.fb, chip, INK);
    cx.text.draw_in(
        script,
        cx.fb,
        chip.x + theme.gap,
        chip.center_y() + cap / 2,
        said,
        true,
    );
}

/// What the control says. The reader opens a book read through at its cover
/// rather than where it was left, so that one is offered from the beginning.
fn open_label(book: &BookStat, s: &Strings) -> &'static str {
    match book.is_finished() {
        true => s.reread_from_beginning,
        false => s.continue_reading,
    }
}

/// The control that hands this book back to the Kindle's reader, under the
/// progress bar and along the same left edge. Outlined, as every control on
/// every screen is; a tap on it leaves the app.
fn open_button(cx: &mut Ctx, band: Rect, book: &BookStat, index: usize) {
    let theme: &Theme = cx.theme;
    let script = cx.ui_script();
    let said = open_label(book, cx.s());
    cx.text.set_px(theme.body_px);
    let w = cx.text.measure_width_in(script, said) as i32 + chrome::chip_pad() * 4;
    let chip = Rect::new(band.x, band.y, w.min(band.w), band.h);
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
    cx.hit(Hit::Open(index), chip);
}

/// The book's three headline figures, along the foot of the words column.
fn figures(cx: &mut Ctx, words: Rect, book: &BookStat) {
    let theme: &Theme = cx.theme;
    let s = cx.s();
    let stated = [
        (date::duration(book.seconds, s), s.read),
        (book.page_turns.to_string(), s.pages_turned),
        (
            book.time_left()
                .map_or("—".into(), |t| date::duration(t, s)),
            s.left,
        ),
    ];
    let height = chrome::figure_height(cx.text, theme);
    let row = Rect::new(words.x, words.bottom() - height, words.w, height);
    chrome::figures(cx.fb, cx.text, theme, row, &stated);
}

/// Whether the catalog names this book on the device.
fn where_note(book: &BookStat, s: &Strings) -> String {
    match book.on_device {
        true => s.yes.into(),
        false => s.no_removed.into(),
    }
}

/// Which of the three regimes produced this book's figure. `seconds`,
/// `dwell_seconds` and `awake_seconds` are not one claim: a counter, a per-page
/// measurement, and an upper bound.
fn measure_note(book: &BookStat, s: &Strings) -> String {
    let measured = book.seconds - book.dwell_seconds - book.awake_seconds;
    match (measured, book.dwell_seconds, book.awake_seconds) {
        (_, 0, 0) => s.kindle_timer.into(),
        (0, d, 0) if d > 0 => s.page_by_page.into(),
        (0, 0, _) => s.time_awake.into(),
        (_, _, 0) => s.timer_and_pages.into(),
        _ => s.part_bounded.into(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::lang::Lang;

    fn en() -> &'static Strings {
        Lang::English.strings()
    }

    fn book(seconds: i64, dwell: i64, awake: i64) -> BookStat {
        BookStat {
            extent: 1,
            title: "A Book".into(),
            author: String::new(),
            thumbnail: String::new(),
            percent: -1.0,
            on_device: false,
            location: String::new(),
            language: String::new(),
            seconds,
            dwell_seconds: dwell,
            awake_seconds: awake,
            sittings: 1,
            page_turns: 0,
            words: 0,
            days: 1,
            first_day: 0,
            last_day: 0,
            last_secs: 0,
        }
    }

    /// [`book`] with the catalog naming a file for it.
    fn held(location: &str) -> BookStat {
        BookStat {
            on_device: true,
            location: location.into(),
            ..book(600, 0, 0)
        }
    }

    #[test]
    fn only_a_book_the_reader_can_be_handed_takes_the_control() {
        let theme = Theme::for_screen(1264, 1680);
        // A book the device holds, one it does not, and one it holds under a
        // name the catalog never stated.
        assert!(open_height(&theme, &held("/mnt/us/documents/a.kfx")) > 0);
        assert_eq!(open_height(&theme, &book(600, 0, 0)), 0);
        assert_eq!(open_height(&theme, &held("")), 0);
    }

    #[test]
    fn the_control_stands_inside_the_band_the_heading_took_for_it() {
        let held = held("/mnt/us/documents/a.kfx");
        for (w, h) in [(1264, 1680), (1272, 1696), (1860, 2480)] {
            let theme = Theme::for_screen(w, h);
            let tall = open_height(&theme, &held);
            // The band as `heading` cuts it, at the foot of a heading.
            let band = Rect::new(0, 400, chrome::content_box(&theme).w, tall)
                .split_bottom(tall)
                .0;
            let chip = open_box(&theme, band);
            assert_eq!(chip.h, chrome::chip_height(&theme), "{w}x{h}");
            assert_eq!(chip.bottom(), band.bottom(), "{w}x{h}: the control floats");
            assert_eq!(
                chip.y - band.y,
                theme.gap * 2,
                "{w}x{h}: the control sits against the progress bar"
            );
        }
    }

    #[test]
    fn a_book_read_through_is_offered_from_its_beginning() {
        let held = held("/mnt/us/documents/a.kfx");
        let done = BookStat {
            percent: 100.0,
            ..held.clone()
        };
        assert_eq!(open_label(&done, en()), "Reread from beginning");
        // A book with a place left in it, and one the catalog states no
        // progress for at all.
        let part = BookStat {
            percent: 40.0,
            ..held.clone()
        };
        assert_eq!(open_label(&part, en()), "Continue reading");
        assert_eq!(open_label(&held, en()), "Continue reading");
    }

    #[test]
    fn a_figure_says_which_of_the_three_regimes_produced_it() {
        assert_eq!(
            measure_note(&book(600, 0, 0), en()),
            "the Kindle's own timer"
        );
        assert_eq!(measure_note(&book(600, 600, 0), en()), "page by page");
        assert_eq!(
            measure_note(&book(600, 0, 600), en()),
            "time awake, a bound"
        );
        assert_eq!(measure_note(&book(900, 300, 0), en()), "timer and pages");
        assert_eq!(measure_note(&book(900, 300, 300), en()), "part bounded");
    }

    #[test]
    fn the_cover_is_the_size_of_a_cover_and_still_leaves_the_words_room() {
        for (w, h) in [(1264, 1680), (1272, 1696), (1860, 2480)] {
            let theme = Theme::for_screen(w, h);
            let art = cover::width_for(cover_height(&theme));
            assert!(
                art > theme.screen.w / 8,
                "{w}x{h}: a {art} px cover is a stamp"
            );
            let box_ = chrome::content_box(&theme);
            assert!(
                box_.w - art - theme.gap * 2 > box_.w / 2,
                "{w}x{h}: the cover took the measure the title needs"
            );
        }
    }
}
