//! One book: its cover, how far through it, how long it has taken, and what is
//! left.

use crate::date;
use crate::font::Script;
use crate::lang::Strings;
use crate::stats::BookStat;
use crate::ui::chrome;
use crate::ui::cover;
use crate::ui::paint::{self, INK, Rect, WHITE};
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

/// The labels of the two controls. The opening one is `None` outside
/// [`BookStat::can_open`]; the marking one names the state a tap leaves.
fn control_labels(book: &BookStat, s: &Strings) -> (Option<&'static str>, &'static str) {
    let mark = match book.finished {
        true => s.mark_unfinished,
        false => s.mark_finished,
    };
    (book.can_open().then_some(s.continue_reading), mark)
}

/// Where the two controls land across `width`: the opening one against the
/// left edge, the marking one against the right, on a second row where the two
/// overrun `width` together.
fn controls_split(
    text: &mut TextRenderer,
    theme: &Theme,
    script: Script,
    width: i32,
    said: (Option<&str>, &str),
) -> (Option<Rect>, Rect) {
    text.set_px(theme.body_px);
    let pad = chrome::chip_pad() * 4;
    let mut wide = |label: &str| (text.measure_width_in(script, label) as i32 + pad).min(width);
    let mark = wide(said.1);
    let open = said.0.map(&mut wide);
    split_from(theme, width, open, mark)
}

/// [`controls_split`]'s arithmetic, over widths already measured. Separated
/// from the paint: a test reaches it without a `TextRenderer`.
fn split_from(theme: &Theme, width: i32, open: Option<i32>, mark: i32) -> (Option<Rect>, Rect) {
    let h = chrome::chip_height(theme);
    let Some(open) = open else {
        return (None, Rect::new(width - mark, 0, mark, h));
    };
    let row = match open + theme.gap * 2 + mark <= width {
        true => 0,
        false => h + theme.gap,
    };
    (
        Some(Rect::new(0, 0, open, h)),
        Rect::new(width - mark, row, mark, h),
    )
}

/// The band the controls take, the gap above them included.
fn open_height(
    text: &mut TextRenderer,
    theme: &Theme,
    script: Script,
    book: &BookStat,
    s: &Strings,
) -> i32 {
    let width = chrome::content_box(theme).w;
    let (_, mark) = controls_split(text, theme, script, width, control_labels(book, s));
    theme.gap * 2 + mark.bottom()
}

/// Where the controls sit inside the band [`open_height`] took: their own
/// height at the foot, the air above them.
fn open_box(theme: &Theme, band: Rect) -> Rect {
    band.split_bottom((band.h - theme.gap * 2).max(1)).0
}

/// The height the heading draws into: the cover, the progress under it, and
/// the controls under that.
fn heading_height(
    text: &mut TextRenderer,
    theme: &Theme,
    script: Script,
    book: &BookStat,
    s: &Strings,
) -> i32 {
    cover_height(theme)
        + theme.gap * 2
        + progress_height(text, theme)
        + open_height(text, theme, script, book, s)
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
    let ui = cx.ui_script();
    let (head, rest) = area.split_top(heading_height(cx.text, theme, ui, &book, s) + air);
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

/// The cover, the title beside it, the progress bar under both, and the two
/// controls under that.
fn heading(cx: &mut Ctx, area: Rect, book: &BookStat, index: usize) {
    let theme: &Theme = cx.theme;
    let script = Script::of_language(&book.language);

    let band = open_height(cx.text, theme, cx.ui_script(), book, cx.s());
    let (open, area) = area.split_bottom(band);
    controls(cx, open_box(theme, open), book, index);

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
    if book.has_percent() || book.is_finished() {
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
        if let Some(at) = book.position_shown() {
            position_mark(cx, foot, track, at);
        }
        if book.is_finished() {
            finished_chip(cx, foot, baseline);
        }
    }
}

/// How wide [`position_mark`] cuts.
fn mark_width(theme: &Theme) -> i32 {
    (theme.gap / 2).max(3)
}

/// Where [`position_mark`] cuts in `track`, `at` per cent along it.
fn mark_x(theme: &Theme, track: Rect, at: i64) -> i32 {
    let w = mark_width(theme);
    let x = track.x + (track.w as i64 * at.clamp(0, 100) / 100) as i32;
    x.clamp(track.x, track.right() - w)
}

/// [`BookStat::position_shown`] cut white through the filled bar, with `at`
/// set small over the cut.
fn position_mark(cx: &mut Ctx, line: Rect, track: Rect, at: i64) {
    let theme: &Theme = cx.theme;
    let (w, x) = (mark_width(theme), mark_x(theme, track, at));
    paint::fill(cx.fb, Rect::new(x, track.y, w, track.h), WHITE);
    let script = cx.ui_script();
    cx.text.set_px(theme.small_px);
    let said = crate::lang::counted(cx.s().percent_now, at);
    let tw = cx.text.measure_width_in(script, &said) as i32;
    let put = (x + w / 2 - tw / 2).clamp(line.x, (line.right() - tw).max(line.x));
    cx.text
        .draw_in(script, cx.fb, put, track.y - theme.gap, &said, false);
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

/// The two controls under the progress bar, at the places [`controls_split`]
/// put them: [`Hit::Open`] on the left, [`Hit::Finished`] on the right.
fn controls(cx: &mut Ctx, band: Rect, book: &BookStat, index: usize) {
    let theme: &Theme = cx.theme;
    let script = cx.ui_script();
    let said = control_labels(book, cx.s());
    let (open, mark) = controls_split(cx.text, theme, script, band.w, said);
    if let (Some(box_), Some(label)) = (open, said.0) {
        let box_ = Rect::new(band.x + box_.x, band.y + box_.y, box_.w, box_.h);
        outlined(cx, box_, label);
        cx.hit(Hit::Open(index), box_);
    }
    let box_ = Rect::new(band.x + mark.x, band.y + mark.y, mark.w, mark.h);
    outlined(cx, box_, said.1);
    cx.hit(Hit::Finished(index, !book.finished), box_);
}

/// One control: `said` centred in a 2 px outline.
fn outlined(cx: &mut Ctx, box_: Rect, said: &str) {
    let theme: &Theme = cx.theme;
    let script = cx.ui_script();
    cx.text.set_px(theme.body_px);
    paint::stroke(cx.fb, box_, INK, 2);
    let tw = cx.text.measure_width_in(script, said) as i32;
    let baseline = box_.center_y() + cx.text.cap_height() as i32 / 2;
    cx.text.draw_in(
        script,
        cx.fb,
        box_.x + (box_.w - tw) / 2,
        baseline,
        said,
        false,
    );
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
            cde_key: "KEY1".into(),
            finished: false,
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
    fn only_a_book_the_reader_can_be_handed_takes_the_opening_control() {
        // A book the device holds, one it does not, and one it holds under a
        // name the catalog never stated.
        let on_device = held("/mnt/us/documents/a.kfx");
        assert_eq!(control_labels(&on_device, en()).0, Some("Continue reading"));
        assert_eq!(control_labels(&book(600, 0, 0), en()).0, None);
        assert_eq!(control_labels(&held(""), en()).0, None);
    }

    #[test]
    fn the_marking_control_names_the_state_a_tap_would_leave() {
        let held = held("/mnt/us/documents/a.kfx");
        assert_eq!(control_labels(&held, en()).1, "Mark as Finished");
        let marked = BookStat {
            finished: true,
            ..held
        };
        assert_eq!(control_labels(&marked, en()).1, "Mark as Unfinished");
    }

    #[test]
    fn both_controls_stand_inside_the_width_they_were_given() {
        for (w, h) in [(1264, 1680), (1272, 1696), (1860, 2480)] {
            let theme = Theme::for_screen(w, h);
            let width = chrome::content_box(&theme).w;
            // Two that fit side by side, and two that cannot.
            for (open, mark) in [(300, 400), (width * 3 / 5, width * 3 / 5)] {
                let (open_box, mark_box) = split_from(&theme, width, Some(open), mark);
                let open_box = open_box.expect("a book on the device takes both");
                assert_eq!(open_box.x, 0, "{w}x{h}: the opening control floats");
                assert_eq!(
                    mark_box.right(),
                    width,
                    "{w}x{h}: the marking control leaves the right edge"
                );
                assert!(
                    mark_box.y >= open_box.bottom() || mark_box.x >= open_box.right(),
                    "{w}x{h}: the two controls overlap"
                );
            }
            // A book the device does not hold takes the right-hand control
            // alone, on the first row.
            let (open_box, mark_box) = split_from(&theme, width, None, 400);
            assert!(open_box.is_none());
            assert_eq!((mark_box.y, mark_box.right()), (0, width));
        }
    }

    #[test]
    fn the_controls_stand_inside_the_band_the_heading_took_for_them() {
        for (w, h) in [(1264, 1680), (1272, 1696), (1860, 2480)] {
            let theme = Theme::for_screen(w, h);
            let one = chrome::chip_height(&theme);
            for rows in [1, 2] {
                let tall = theme.gap * 2 + one * rows + theme.gap * (rows - 1);
                let band = Rect::new(0, 400, chrome::content_box(&theme).w, tall);
                let box_ = open_box(&theme, band);
                assert_eq!(box_.bottom(), band.bottom(), "{w}x{h}: the controls float");
                assert_eq!(
                    box_.y - band.y,
                    theme.gap * 2,
                    "{w}x{h}: the controls sit against the progress bar"
                );
            }
        }
    }

    #[test]
    fn the_mark_stands_inside_the_track_at_either_end() {
        let theme = Theme::for_screen(1264, 1680);
        let track = Rect::new(39, 900, 1186, bar_height(&theme));
        for at in [0, 1, 55, 99, 100] {
            let x = mark_x(&theme, track, at);
            assert!(x >= track.x, "{at}%: the mark starts left of the track");
            assert!(
                x + mark_width(&theme) <= track.right(),
                "{at}%: the mark runs past the track"
            );
        }
        // The mark tracks the figure it stands for.
        assert!(mark_x(&theme, track, 20) < mark_x(&theme, track, 80));
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
