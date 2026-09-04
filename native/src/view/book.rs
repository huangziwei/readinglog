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

use super::Ctx;

/// How many days of a book's own history the strip along the bottom shows.
const RECENT_DAYS: i64 = 30;

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

/// The height the heading draws into: the cover, and the progress under it.
fn heading_height(text: &mut TextRenderer, theme: &Theme) -> i32 {
    cover_height(theme) + theme.gap * 2 + progress_height(text, theme)
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
    let (head, rest) = area.split_top(heading_height(cx.text, theme) + air);
    heading(cx, Rect::new(head.x, head.y, head.w, head.h - air), &book);

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

    let inner = chrome::section(cx.fb, cx.text, theme, chart, s.last_thirty_days);
    let last = cx.today;
    let series: Vec<i64> = {
        let days = cx.stats.book_days(index);
        (0..RECENT_DAYS)
            .map(|i| {
                let day = last - RECENT_DAYS + 1 + i;
                days.iter()
                    .find(|(d, _)| *d == day)
                    .map_or(0, |(_, secs)| *secs)
            })
            .collect()
    };
    charts::columns(
        cx.fb,
        cx.text,
        theme,
        inner,
        &series,
        move |i| {
            let day = last - RECENT_DAYS + 1 + i as i64;
            let (_, _, dom) = date::civil_from_days(day);
            dom.to_string()
        },
        // A fortnight of bars this narrow has no room for figures on them.
        &|_| Vec::new(),
        7,
        None,
    );
}

/// The cover, the title beside it, and the progress bar under both.
fn heading(cx: &mut Ctx, area: Rect, book: &BookStat) {
    let theme: &Theme = cx.theme;
    let script = Script::of_language(&book.language);

    let (foot, top) = area.split_bottom(progress_height(cx.text, theme));
    // `top` stops `theme.gap * 2` above `foot`.
    let top = Rect::new(top.x, top.y, top.w, (top.h - theme.gap * 2).max(1));
    let (art, rest) = top.split_left(cover::width_for(top.h));
    cx.covers.draw(cx.fb, art, &book.thumbnail);

    let words = Rect::new(
        art.right() + theme.gap * 2,
        top.y,
        (rest.w - theme.gap * 2).max(1),
        top.h,
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
        paint::progress(cx.fb, track, book.percent as i64, 100, INK);
        cx.text.set_px(theme.small_px);
        let pct = format!("{}% {}", book.percent.round() as i64, s.read);
        cx.text
            .draw(cx.fb, foot.x, track.y - theme.gap / 2, &pct, false);
    }
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

/// Which of the three regimes produced this book's figure.
///
/// `seconds`, `dwell_seconds` and `awake_seconds` are not one claim: a counter,
/// a per-page measurement, and an upper bound.
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
