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

/// Lines a title takes before the rest of it is ellipsized, where the column
/// it is set in has room for them.
const TITLE_LINES: usize = 2;

/// Rows of figures the reading section lists.
const LINES: usize = 10;

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

/// The labels of the controls: the two that hand the book over, then the mark.
/// The pair needs [`BookStat::can_open`], the first shortens where the second
/// joins it, and the mark stands under [`BookStat::can_mark`].
fn control_labels(
    book: &BookStat,
    s: &Strings,
) -> ([Option<&'static str>; 2], Option<&'static str>) {
    let open = book.can_open();
    let again = open && book.can_reread();
    (
        [
            open.then_some(match again {
                true => s.continue_short,
                false => s.continue_reading,
            }),
            again.then_some(s.reread),
        ],
        book.can_mark().then_some(match book.finished {
            true => s.mark_unfinished,
            false => s.mark_finished,
        }),
    )
}

/// Where the controls land across `width`: the reading ones from the left
/// edge, the mark against the right.
fn controls_split(
    text: &mut TextRenderer,
    theme: &Theme,
    script: Script,
    width: i32,
    said: ([Option<&str>; 2], Option<&str>),
) -> (Vec<Rect>, Option<Rect>) {
    text.set_px(theme.body_px);
    let pad = chrome::chip_pad() * 4;
    let mut wide = |label: &str| (text.measure_width_in(script, label) as i32 + pad).min(width);
    let read: Vec<i32> = said.0.iter().flatten().map(|l| wide(l)).collect();
    let mark = said.1.map(&mut wide);
    split_from(theme, width, &read, mark)
}

/// [`controls_split`]'s arithmetic, over measured widths. `read` runs from the
/// left edge in the order given and `mark` keeps the right; a row that cannot
/// hold what is left opens another.
fn split_from(
    theme: &Theme,
    width: i32,
    read: &[i32],
    mark: Option<i32>,
) -> (Vec<Rect>, Option<Rect>) {
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
    let mark = mark.map(|mark| {
        let mark = mark.min(width);
        if x > 0 && x + theme.gap + mark > width {
            row += 1;
        }
        Rect::new(width - mark, row * step, mark, h)
    });
    (boxes, mark)
}

/// The foot of the lowest control [`split_from`] placed, and 0 where it placed
/// none.
fn controls_bottom(read: &[Rect], mark: Option<Rect>) -> i32 {
    read.iter()
        .chain(mark.iter())
        .map(Rect::bottom)
        .max()
        .unwrap_or(0)
}

/// The band the controls take, the gap above them included, and 0 where this
/// book offers none.
fn open_height(
    text: &mut TextRenderer,
    theme: &Theme,
    script: Script,
    book: &BookStat,
    s: &Strings,
) -> i32 {
    let width = chrome::content_box(theme).w;
    let (read, mark) = controls_split(text, theme, script, width, control_labels(book, s));
    match controls_bottom(&read, mark) {
        0 => 0,
        foot => theme.gap * 2 + foot,
    }
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
    // `facts` takes [`LINES`] line heights; `chart` takes what `rest` has left.
    cx.text.set_px(theme.body_px);
    let deep = head_h + air + cx.text.line_height() as i32 * LINES as i32;
    let (facts, chart) = rest.split_top(deep.min(rest.h));
    let facts = Rect::new(facts.x, facts.y, facts.w, (facts.h - air).max(1));
    let inner = chrome::section(cx.fb, cx.text, theme, facts, s.the_reading);

    // [`figures`] states the other three.
    let lines: [(&str, String); LINES] = [
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

    // A `chart` shallower than its heading and a row draws no column.
    if chart.h < head_h + theme.row_h {
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

/// The cover, the title beside it, the progress bar under both, and the two
/// controls under that.
fn heading(cx: &mut Ctx, area: Rect, book: &BookStat, index: usize) {
    let theme: &Theme = cx.theme;
    let script = Script::of_language(&book.language);

    let band = open_height(cx.text, theme, cx.ui_script(), book, cx.s());
    let (open, area) = area.split_bottom(band);
    if band > 0 {
        controls(cx, open_box(theme, open), book, index);
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
        let baseline = track.y - theme.gap / 2;
        // The chip takes its place first; the figure keeps to what is left.
        let chip = book
            .is_finished()
            .then(|| finished_chip(cx, foot, baseline));
        let room = left_of(foot, chip, theme.gap * 2);
        if let Some(at) = book.position_shown() {
            position_mark(cx, room, track, at, shown > at);
        }
    }
}

/// `line` up to `chip`, `air` clear of it.
fn left_of(line: Rect, chip: Option<Rect>, air: i32) -> Rect {
    let right = chip.map_or(line.right(), |c| c.x - air);
    Rect::new(line.x, line.y, (right - line.x).max(1), line.h)
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

/// [`BookStat::position_shown`] set small over the place it names, inside
/// `line`. Under `cut` a white notch cuts the ink there.
fn position_mark(cx: &mut Ctx, line: Rect, track: Rect, at: i64, cut: bool) {
    let theme: &Theme = cx.theme;
    let (w, x) = (mark_width(theme), mark_x(theme, track, at));
    if cut {
        paint::fill(cx.fb, Rect::new(x, track.y, w, track.h), WHITE);
    }
    let script = cx.ui_script();
    cx.text.set_px(theme.small_px);
    let said = crate::lang::counted(cx.s().percent_at, at);
    let tw = cx.text.measure_width_in(script, &said) as i32;
    let put = (x + w / 2 - tw / 2).clamp(line.x, (line.right() - tw).max(line.x));
    cx.text
        .draw_in(script, cx.fb, put, track.y - theme.gap, &said, false);
}

/// A book read through, stated at the right of the line the progress bar
/// carries, and answering the box it took. Filled, where every other chip on a
/// screen is outlined: this one is not a control and states an end state.
fn finished_chip(cx: &mut Ctx, line: Rect, baseline: i32) -> Rect {
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
    chip
}

/// The controls under the progress bar, at the places [`controls_split`] put
/// them: [`Hit::Open`] and [`Hit::Reread`] from the left, [`Hit::Finished`]
/// against the right.
fn controls(cx: &mut Ctx, band: Rect, book: &BookStat, index: usize) {
    let theme: &Theme = cx.theme;
    let script = cx.ui_script();
    let said = control_labels(book, cx.s());
    let (read, mark) = controls_split(cx.text, theme, script, band.w, said);
    let mut placed = read.iter();
    for (said, hit) in said.0.iter().zip([Hit::Open(index), Hit::Reread(index)]) {
        let (Some(said), Some(box_)) = (said, placed.next()) else {
            continue;
        };
        let box_ = onto(band, *box_);
        outlined(cx, box_, said);
        cx.hit(hit, box_);
    }
    if let (Some(box_), Some(said)) = (mark, said.1) {
        let box_ = onto(band, box_);
        outlined(cx, box_, said);
        cx.hit(Hit::Finished(index, !book.finished), box_);
    }
}

/// A box [`split_from`] placed, moved onto `band`.
fn onto(band: Rect, box_: Rect) -> Rect {
    Rect::new(band.x + box_.x, band.y + box_.y, box_.w, box_.h)
}

/// The question a reread puts, drawn over the book's own screen. The panel
/// carries [`Hit::Again`] and [`Hit::Dismiss`]; `area` behind it is
/// [`Hit::Dismiss`] whole.
pub fn asking(cx: &mut Ctx, area: Rect) {
    let theme: &Theme = cx.theme;
    let script = cx.ui_script();
    let s = cx.s();
    cx.hit(Hit::Dismiss, area);

    let ways = [(s.cancel, Hit::Dismiss), (s.reread, Hit::Again)];
    let pad = theme.gap * 3;
    let width = area.w - theme.gap * 6;
    let inner = width - pad * 2;

    cx.text.set_px(theme.head_px);
    let ask = cx
        .text
        .wrap_and_clamp_in(script, s.reread_ask, inner as u32, 2);
    let ask_h = ask.len() as i32 * cx.text.line_height() as i32;
    cx.text.set_px(theme.body_px);
    let note = cx
        .text
        .wrap_and_clamp_in(script, s.reread_note, inner as u32, 6);
    let note_h = note.len() as i32 * cx.text.line_height() as i32;
    let said: Vec<i32> = ways
        .iter()
        .map(|(l, _)| {
            (cx.text.measure_width_in(script, l) as i32 + chrome::chip_pad() * 4).min(inner)
        })
        .collect();

    let chip = chrome::chip_height(theme);
    let abreast = said.iter().sum::<i32>() + theme.gap * 4 <= inner;
    let rows = match abreast {
        true => 1,
        false => said.len() as i32,
    };
    let feet_h = chip * rows + theme.gap * (rows - 1);
    let high = (pad * 2 + ask_h + theme.gap * 2 + note_h + theme.gap * 3 + feet_h).min(area.h);
    let panel = Rect::new(
        area.x + (area.w - width) / 2,
        area.y + (area.h - high) / 2,
        width,
        high,
    );
    paint::fill(cx.fb, panel, WHITE);
    paint::stroke(cx.fb, panel, INK, 3);

    let (heads, rest) = panel.inset(pad).split_top(ask_h + theme.gap * 2);
    let (notes, feet) = rest.split_top(note_h + theme.gap * 3);
    cx.text.set_px(theme.head_px);
    lines(cx, heads, script, &ask);
    cx.text.set_px(theme.body_px);
    lines(cx, notes, script, &note);

    let boxes = match abreast {
        true => {
            let mut x = feet.right() - (said.iter().sum::<i32>() + theme.gap * 4);
            said.iter()
                .map(|w| {
                    let box_ = Rect::new(x, feet.y, *w, chip);
                    x += w + theme.gap * 2;
                    box_
                })
                .collect()
        }
        false => feet.rows(said.len() as i32, theme.gap),
    };
    for ((said, hit), box_) in ways.iter().zip(&boxes) {
        outlined(cx, *box_, said);
        cx.hit(*hit, *box_);
    }
}

/// `said` set down `box_` from its top, at whatever size is set.
fn lines(cx: &mut Ctx, box_: Rect, script: Script, said: &[String]) {
    let step = cx.text.line_height() as i32;
    let mut y = box_.y + cx.text.cap_height() as i32;
    for line in said {
        cx.text.draw_in(script, cx.fb, box_.x, y, line, false);
        y += step;
    }
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
    fn only_a_book_the_reader_can_be_handed_takes_the_reading_controls() {
        // A book the device holds, one it does not, and one it holds under a
        // name the catalog never stated.
        let on_device = held("/mnt/us/documents/a.kfx");
        assert_eq!(
            control_labels(&on_device, en()).0,
            [Some("Continue reading"), None],
            "a book with no place in it has none to give up"
        );
        assert_eq!(control_labels(&book(600, 0, 0), en()).0, [None, None]);
        assert_eq!(control_labels(&held(""), en()).0, [None, None]);

        // A place to give up puts the rereading control beside the other, and
        // shortens the first.
        let part = BookStat {
            percent: 46.0,
            ..on_device
        };
        assert_eq!(
            control_labels(&part, en()).0,
            [Some("Continue"), Some("Reread")]
        );
        // The same book off the device offers neither: both hand it over.
        let gone = BookStat {
            percent: 46.0,
            ..book(600, 0, 0)
        };
        assert_eq!(control_labels(&gone, en()).0, [None, None]);
    }

    #[test]
    fn the_marking_control_names_the_state_a_tap_would_leave() {
        let held = held("/mnt/us/documents/a.kfx");
        assert_eq!(control_labels(&held, en()).1, Some("Mark as Finished"));
        let marked = BookStat {
            finished: true,
            ..held
        };
        assert_eq!(control_labels(&marked, en()).1, Some("Mark as Unfinished"));
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
        assert_eq!(control_labels(&done, en()).1, None);
        assert_eq!(
            control_labels(&done, en()).0,
            [Some("Continue"), Some("Reread")],
            "and reading it again is the way out of the mark"
        );
        // Short of [`FINISHED_PERCENT`], `can_mark` holds.
        let back = BookStat {
            percent: 96.0,
            ..done
        };
        assert_eq!(control_labels(&back, en()).1, Some("Mark as Unfinished"));
    }

    #[test]
    fn every_control_stands_inside_the_width_it_was_given() {
        for (w, h) in [(1264, 1680), (1272, 1696), (1860, 2480)] {
            let theme = Theme::for_screen(w, h);
            let width = chrome::content_box(&theme).w;
            // Two that fit beside the mark, and two that cannot.
            for read in [vec![300, 260], vec![width * 3 / 5, width * 3 / 5]] {
                let (boxes, mark) = split_from(&theme, width, &read, Some(400));
                let mark = mark.expect("the mark was asked for");
                assert_eq!(boxes.len(), read.len());
                assert_eq!(boxes[0].x, 0, "{w}x{h}: the first control floats");
                assert_eq!(
                    mark.right(),
                    width,
                    "{w}x{h}: the marking control leaves the right edge"
                );
                for box_ in &boxes {
                    assert!(
                        box_.right() <= width,
                        "{w}x{h}: a control runs past the edge"
                    );
                    assert!(
                        box_.y != mark.y || box_.right() <= mark.x,
                        "{w}x{h}: a reading control overlaps the mark"
                    );
                }
                assert!(
                    boxes[0].y != boxes[1].y || boxes[0].right() <= boxes[1].x,
                    "{w}x{h}: the two reading controls overlap"
                );
            }
            // A book the device does not hold takes the mark alone, on the
            // first row.
            let (boxes, mark) = split_from(&theme, width, &[], Some(400));
            assert!(boxes.is_empty());
            assert_eq!((mark.unwrap().y, mark.unwrap().right()), (0, width));
            // With neither, `controls_bottom` is 0 and the band takes no height.
            let (boxes, mark) = split_from(&theme, width, &[], None);
            assert_eq!(controls_bottom(&boxes, mark), 0);
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
