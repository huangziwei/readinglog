//! One book: its cover, how far through it, how long it has taken, and what is
//! left.

use crate::date;
use crate::font::Script;
use crate::lang::Strings;
use crate::stats::BookStat;
use crate::ui::chrome;
use crate::ui::cover;
use crate::ui::paint::Rect;
use crate::ui::text::TextRenderer;
use crate::ui::{charts, theme::Theme};

use super::{Ask, Ctx, Hit, band};

/// The most columns the strip along the bottom is cut into.
const SPAN_COLUMNS: i64 = 30;

/// Lines a title takes before the rest of it is ellipsized.
const TITLE_LINES: usize = 2;

/// Rows of figures the reading section lists.
const LINES: usize = 10;

/// What a row states in place of a figure it has none of.
const DASH: &str = "—";

/// The cover's height: four rows.
fn cover_height(theme: &Theme) -> i32 {
    theme.row_h * 4
}

/// The labels of the two controls that hand the book over. Both stand under
/// [`BookStat::can_open`], and the first shortens where the second joins it.
fn control_labels(book: &BookStat, s: &Strings) -> [Option<&'static str>; 2] {
    let open = book.can_open();
    let again = open && book.can_restart();
    [
        open.then_some(match again {
            true => s.continue_short,
            false => s.continue_reading,
        }),
        again.then_some(s.restart),
    ]
}

/// Where the controls land across `width`, from the left edge.
fn controls_split(
    text: &mut TextRenderer,
    theme: &Theme,
    script: Script,
    width: i32,
    said: [Option<&str>; 2],
) -> Vec<Rect> {
    text.set_px(theme.body_px);
    let pad = chrome::chip_pad() * 4;
    let read: Vec<i32> = said
        .iter()
        .flatten()
        .map(|l| (text.measure_width_in(script, l) as i32 + pad).min(width))
        .collect();
    split_from(theme, width, &read)
}

/// [`controls_split`]'s arithmetic, over measured widths. The boxes run from
/// the left edge in the order given; a row that cannot hold what is left opens
/// another.
fn split_from(theme: &Theme, width: i32, read: &[i32]) -> Vec<Rect> {
    let h = chrome::chip_height(theme);
    let step = h + theme.gap;
    let (mut row, mut x) = (0, 0);
    let mut boxes = Vec::with_capacity(read.len());
    for w in read {
        let w = (*w).min(width);
        if x > 0 && x + w > width {
            row += 1;
            x = 0;
        }
        boxes.push(Rect::new(x, row * step, w, h));
        x += w + theme.gap;
    }
    boxes
}

/// The foot of the lowest control [`split_from`] placed, and 0 where it placed
/// none.
fn controls_bottom(read: &[Rect]) -> i32 {
    read.iter().map(Rect::bottom).max().unwrap_or(0)
}

/// The band the controls take, the gap above them included, and 0 where this
/// book offers none. The reset control alone is enough to open the band: a
/// book off the device carries no reading control and still carries this one.
fn open_height(
    text: &mut TextRenderer,
    theme: &Theme,
    script: Script,
    book: &BookStat,
    s: &Strings,
) -> i32 {
    let width = chrome::content_box(theme).w;
    let read = controls_split(text, theme, script, width, control_labels(book, s));
    let foot = controls_bottom(&read).max(chrome::chip_height(theme));
    theme.gap * 2 + foot
}

/// Where the reset control sits: against the right edge of the last row the
/// reading controls took, which is the slot the finished mark left.
fn reset_box(
    text: &mut TextRenderer,
    theme: &Theme,
    script: Script,
    band: Rect,
    s: &Strings,
) -> Rect {
    text.set_px(theme.body_px);
    let w = (text.measure_width_in(script, s.clear) as i32 + chrome::chip_pad() * 4).min(band.w);
    let h = chrome::chip_height(theme);
    let step = h + theme.gap;
    let rows = (band.h + theme.gap) / step.max(1);
    let y = band.y + (rows - 1).max(0) * step;
    Rect::new(band.right() - w, y, w, h)
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
        + band::height(text, theme)
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
    // `facts` takes [`LINES`] line heights; `chart` takes what `rest` has left.
    cx.text.set_px(theme.body_px);
    let deep = head_h + air + cx.text.line_height() as i32 * LINES as i32;
    let (facts, chart) = rest.split_top(deep.min(rest.h));
    let facts = Rect::new(facts.x, facts.y, facts.w, (facts.h - air).max(1));
    let inner = chrome::section(cx.fb, cx.text, theme, facts, s.the_reading);

    // [`figures`] states the other three.
    let lines: [(&str, String); LINES] = [
        (s.sittings, book.sittings.to_string()),
        (s.days, days_note(&book, s)),
        (s.average_a_day, date::duration(book.per_day(), s)),
        (s.average_a_sitting, date::duration(book.per_sitting(), s)),
        (s.words, date::words(book.words)),
        (
            s.reading_speed,
            book.wpm().map_or("—".into(), |w| format!("{w} {}", s.wpm)),
        ),
        (s.started, day_note(book.first_day, &book, s)),
        (s.last_read, day_note(book.last_day, &book, s)),
        (s.finished_on, finished_note(&book, s)),
        (s.on_the_device, where_note(&book, s)),
    ];
    let rows = inner.rows(lines.len() as i32, 0);
    for ((key, value), row) in lines.iter().zip(&rows) {
        chrome::row(cx.fb, cx.text, theme, *row, key, value);
    }

    // A `chart` shallower than its heading and a row draws no column, and a
    // book with no sitting has none to draw.
    if chart.h < head_h + theme.row_h || book.sittings == 0 {
        return;
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
/// controls under that. The bar's own band carries [`Hit::Finished`].
fn heading(cx: &mut Ctx, area: Rect, book: &BookStat, index: usize) {
    let theme: &Theme = cx.theme;
    let script = Script::of_language(&book.language);

    let band = open_height(cx.text, theme, cx.ui_script(), book, cx.s());
    let (open, area) = area.split_bottom(band);
    if band > 0 {
        controls(cx, open_box(theme, open), book, index);
    }

    let (foot, top) = area.split_bottom(band::height(cx.text, theme));
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

    let deep = title_lines(cx.text, theme, words.h, !book.author.is_empty());
    cx.text.set_px(theme.head_px);
    let lines = cx
        .text
        .wrap_and_clamp_in(script, &book.title, words.w as u32, deep);
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

    band::draw(cx, foot, band::Band::of(book, book.is_finished()));
    if book.can_mark() {
        cx.hit(Hit::Finished(index, !book.finished), foot);
    }
}

/// The controls under the progress bar: the reading pair from the left at the
/// places [`controls_split`] put them, and the reset control against the right
/// edge.
fn controls(cx: &mut Ctx, band: Rect, book: &BookStat, index: usize) {
    let theme: &Theme = cx.theme;
    let script = cx.ui_script();
    let said = control_labels(book, cx.s());
    let read = controls_split(cx.text, theme, script, band.w, said);
    let mut placed = read.iter();
    for (said, hit) in said.iter().zip([Hit::Open(index), Hit::Restart(index)]) {
        let (Some(said), Some(box_)) = (said, placed.next()) else {
            continue;
        };
        let box_ = onto(band, *box_);
        chrome::outlined(cx, box_, said);
        cx.hit(hit, box_);
    }

    let clear = cx.s().clear;
    let box_ = reset_box(cx.text, theme, script, band, cx.s());
    chrome::outlined(cx, box_, clear);
    cx.hit(Hit::Clear(index), box_);
}

/// A box [`split_from`] placed, moved onto `band`.
fn onto(band: Rect, box_: Rect) -> Rect {
    Rect::new(band.x + box_.x, band.y + box_.y, box_.w, box_.h)
}

/// What `ask` puts up: the headline, what it states, and the label on the
/// answer. [`Ask::Clear`] states figures and has two answers, so it is drawn
/// by [`asking`] itself.
fn question(ask: Ask, s: &Strings) -> (&'static str, &'static str, &'static str) {
    match ask {
        Ask::Restart => (s.restart_ask, s.restart_note, s.restart),
        Ask::Mark(true) => (s.mark_ask, s.mark_note, s.mark_finished),
        Ask::Mark(false) => (s.unmark_ask, s.unmark_note, s.mark_unfinished),
        Ask::Clear => (s.clear_ask, s.clear_note, s.clear_keep),
    }
}

/// What [`Ask::Clear`] states: the reading that goes, and the streak where
/// clearing this book moves it.
pub fn clearing_note(stats: &crate::stats::Stats, index: usize, s: &Strings) -> String {
    let Some(book) = stats.books.get(index) else {
        return s.clear_note.to_string();
    };
    let what = format!(
        "{} · {}",
        crate::lang::counted(s.n_sittings, book.sittings),
        date::duration(book.seconds, s)
    );
    let mut out = s.clear_note.replace("{what}", &what);
    let without = stats.streak_without(index);
    if without < stats.longest_streak {
        out.push(' ');
        out.push_str(
            &s.streak_note
                .replace("{a}", &stats.longest_streak.to_string())
                .replace("{b}", &without.to_string()),
        );
    }
    out
}

/// A question drawn over the book's own screen, through [`ui::dialog`].
pub fn asking(cx: &mut Ctx, area: Rect, ask: Ask, index: usize) {
    let s = cx.s();
    if ask == Ask::Clear {
        let note = clearing_note(cx.stats, index, s);
        crate::ui::dialog::draw(
            cx,
            area,
            &crate::ui::dialog::Question {
                heading: s.clear_ask,
                note: &note,
                answers: &[
                    (s.cancel, Hit::Dismiss),
                    (s.clear_keep, Hit::ClearBook(index)),
                    (s.clear_forget, Hit::ForgetBook(index)),
                ],
            },
        );
        return;
    }
    let (heading, note, answer) = question(ask, s);
    crate::ui::dialog::draw(
        cx,
        area,
        &crate::ui::dialog::Question {
            heading,
            note,
            answers: &[(s.cancel, Hit::Dismiss), (answer, Hit::Answer)],
        },
    );
}

/// Lines the title takes in a column `high` tall: [`TITLE_LINES`], fewer where
/// the author's line and [`figures`] leave room for fewer, and never none.
fn title_lines(text: &mut TextRenderer, theme: &Theme, high: i32, author: bool) -> usize {
    let stood = chrome::figure_height(text, theme);
    text.set_px(theme.small_px);
    let named = match author {
        true => theme.gap + text.line_height() as i32,
        false => 0,
    };
    text.set_px(theme.head_px);
    let each = (text.line_height() as i32).max(1);
    let room = (high - stood - named) / each;
    room.clamp(1, TITLE_LINES as i32) as usize
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
        false => s.no.into(),
    }
}

/// The day a book read through was last put down. A book short of the end,
/// or one with no reading, states none.
fn finished_note(book: &BookStat, s: &Strings) -> String {
    match book.is_finished() {
        true => day_note(book.last_day, book, s),
        false => DASH.into(),
    }
}

/// `day`, and [`DASH`] for a book carrying no reading.
fn day_note(day: i64, book: &BookStat, s: &Strings) -> String {
    match book.sittings {
        0 => DASH.into(),
        _ => date::year_day(day, s),
    }
}

/// The days read of the days between the first and the last.
fn days_note(book: &BookStat, s: &Strings) -> String {
    match book.sittings {
        0 => DASH.into(),
        _ => format!(
            "{} {} {}",
            book.days,
            s.of,
            (book.last_day - book.first_day + 1).max(1)
        ),
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
            cde_type: "EBOK".into(),
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
    fn only_a_book_the_reader_can_be_handed_takes_the_reading_controls() {
        // A book the device holds, one it does not, and one it holds under a
        // name the catalog never stated.
        let on_device = held("/mnt/us/documents/a.kfx");
        assert_eq!(
            control_labels(&on_device, en()),
            [Some("Continue reading"), None],
            "a book with no place in it has none to give up"
        );
        assert_eq!(control_labels(&book(600, 0, 0), en()), [None, None]);
        assert_eq!(control_labels(&held(""), en()), [None, None]);

        // A place to give up puts the restarting control beside the other, and
        // shortens the first.
        let part = BookStat {
            percent: 46.0,
            ..on_device
        };
        assert_eq!(
            control_labels(&part, en()),
            [Some("Continue"), Some("Restart")]
        );
        // The same book off the device offers neither: both hand it over.
        let gone = BookStat {
            percent: 46.0,
            ..book(600, 0, 0)
        };
        assert_eq!(control_labels(&gone, en()), [None, None]);
    }

    #[test]
    fn the_marking_question_names_the_state_an_answer_would_leave() {
        assert_eq!(question(Ask::Mark(true), en()).2, "Mark Finished");
        assert_eq!(question(Ask::Mark(false), en()).2, "Mark Unfinished");
        assert_eq!(question(Ask::Restart, en()).2, "Restart");
        // Each question states its own outcome.
        for ask in [Ask::Mark(true), Ask::Mark(false), Ask::Restart] {
            let (heading, note, _) = question(ask, en());
            assert!(heading.ends_with('?'), "{ask:?} puts no question");
            assert!(note.len() > heading.len(), "{ask:?} states nothing");
        }
    }

    #[test]
    fn a_book_read_to_the_end_leaves_the_mark_to_the_store() {
        // `percent` states this one read through, and `can_mark` is false.
        let done = BookStat {
            percent: 100.0,
            finished: true,
            ..held("/mnt/us/documents/a.kfx")
        };
        assert!(done.is_finished());
        assert!(
            !done.can_mark(),
            "the bar takes no tap on a book read through"
        );
        assert_eq!(
            control_labels(&done, en()),
            [Some("Continue"), Some("Restart")],
            "and reading it again is the way out of the mark"
        );
        // Short of [`FINISHED_PERCENT`], `can_mark` holds.
        let back = BookStat {
            percent: 96.0,
            ..done
        };
        assert!(back.can_mark());
    }

    #[test]
    fn every_control_stands_inside_the_width_it_was_given() {
        for (w, h) in [(1264, 1680), (1272, 1696), (1860, 2480)] {
            let theme = Theme::for_screen(w, h);
            let width = chrome::content_box(&theme).w;
            // Two that fit one row, and two that cannot.
            for read in [vec![300, 260], vec![width * 3 / 5, width * 3 / 5]] {
                let boxes = split_from(&theme, width, &read);
                assert_eq!(boxes.len(), read.len());
                assert_eq!(boxes[0].x, 0, "{w}x{h}: the first control floats");
                for box_ in &boxes {
                    assert!(
                        box_.right() <= width,
                        "{w}x{h}: a control runs past the edge"
                    );
                }
                assert!(
                    boxes[0].y != boxes[1].y || boxes[0].right() <= boxes[1].x,
                    "{w}x{h}: the two reading controls overlap"
                );
            }
            // With no control, `controls_bottom` is 0 and the band takes no
            // height.
            assert_eq!(controls_bottom(&split_from(&theme, width, &[])), 0);
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

    #[test]
    fn the_last_two_rows_answer_the_mark_and_the_catalog() {
        let day = crate::date::days_from_civil(2026, 9, 3);
        let read = BookStat {
            last_day: day,
            ..book(600, 0, 0)
        };
        assert_eq!(finished_note(&read, en()), "—");
        assert_eq!(where_note(&read, en()), "no");
        // The mark dates the row, and the day is the one the book was last
        // put down on.
        let done = BookStat {
            finished: true,
            on_device: true,
            ..read
        };
        assert_eq!(finished_note(&done, en()), "Sep 3, 2026");
        assert_eq!(where_note(&done, en()), "yes");
    }
}
