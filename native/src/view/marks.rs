//! What a book was marked with: every highlight and note, most recent first,
//! and where in the book each one falls.
//!
//! A row states three things and the sources for them are not the same. The
//! words are the clippings file's — the only copy of them outside the book.
//! The place is the `.sdr` sidecar's, and it is a real position, so a book
//! whose sidecar still holds the mark states a **percentage**; one that does
//! not can only state the display location the clipping named, which is on
//! another axis and converts to nothing. See [`crate::annotate`].

use crate::annotate::Mark;
use crate::clippings::Kind;
use crate::font::Script;
use crate::lang::Strings;
use crate::stats::BookStat;
use crate::stats::Stats;
use crate::ui::chrome;
use crate::ui::paint::{self, PALE, Rect};
use crate::ui::text::TextRenderer;
use crate::ui::theme::Theme;

use super::{Ctx, Hit, pager};

/// Lines of a passage a row draws before the rest of it is ellipsized, and the
/// lines it is laid out for. The list is set at [`BODY_LINES`] and takes
/// [`BODY_MOST`] where there are few enough marks to have the room: the words
/// are what the page is for, and a passage cut after one line is a passage
/// nobody can place.
const BODY_LINES: usize = 2;
const BODY_MOST: usize = 3;

/// Lines of a note a row draws under the passage it was written on.
const NOTE_LINES: usize = 2;

/// What separates the parts of a row's own label: `Location 608 | Highlight`.
const BAR: &str = " | ";
const DOT: &str = " · ";

/// What a book with no place for a mark states in place of one.
const DASH: &str = "—";

/// How wide the rule beside a passage is, and how far the words stand clear of
/// it.
fn rule_width(theme: &Theme) -> i32 {
    (theme.gap / 2).max(3)
}

fn indent(theme: &Theme) -> i32 {
    theme.gap * 3
}

/// The heights a row is set in.
#[derive(Clone, Copy)]
struct Metrics {
    /// How far the label's line stands over its own baseline.
    cap: i32,
    /// The label's line.
    small: i32,
    /// A line of the passage, which the note is set on too. Carries
    /// [`leading`] over what the face itself asks for.
    line: i32,
}

/// What a line of a passage is opened up by over the face's own line height.
/// A quotation is read slower than a list of figures and is set looser for it.
fn leading(theme: &Theme) -> i32 {
    theme.gap
}

impl Metrics {
    fn of(text: &mut TextRenderer, theme: &Theme) -> Self {
        text.set_px(theme.small_px);
        let (cap, small) = (text.cap_height() as i32, text.line_height() as i32);
        text.set_px(theme.body_px);
        Self {
            cap,
            small,
            line: text.line_height() as i32 + leading(theme),
        }
    }

    /// The height the passage takes, the air over it included. **This block
    /// alone is what the coloured rule stands beside**: the colour is the
    /// highlight's, and the note is the reader's own writing about it, not
    /// part of what was marked.
    fn quoted(self, theme: &Theme, lines: usize) -> i32 {
        theme.gap * 2 + lines.max(1) as i32 * self.line
    }

    /// The height the note under a passage takes, and 0 where there is none.
    fn noted(self, theme: &Theme, note: usize) -> i32 {
        match note {
            0 => 0,
            n => theme.gap + n as i32 * self.line,
        }
    }

    /// The height a row takes: the passage, the note under it, the label
    /// under that, and the air closing the row.
    ///
    /// The label is **under** the words, not over them. It states where and
    /// when the mark was made, which is what the row is about only once the
    /// passage has been read.
    fn row(self, theme: &Theme, lines: usize, note: usize) -> i32 {
        self.quoted(theme, lines)
            + self.noted(theme, note)
            + theme.gap * 2
            + self.small
            + theme.gap * 3
    }
}

/// Passage lines every row of a list `h` tall is allowed, of [`BODY_MOST`]:
/// the most that lets all `rows` of them stand, and never fewer than
/// [`BODY_LINES`].
fn lines_in(m: Metrics, theme: &Theme, h: i32, rows: usize) -> usize {
    let each = (h / rows.max(1) as i32).max(1);
    (BODY_LINES + 1..=BODY_MOST)
        .rev()
        .find(|&lines| m.row(theme, lines, 0) <= each)
        .unwrap_or(BODY_LINES)
}

/// Where each page of one book's marks opens in `area`, ascending and starting
/// at 0. Empty for a book carrying none.
///
/// A row is as tall as its own words, so the passages have to be wrapped to
/// know where a page ends. [`draw`] pages from this and so does the step a
/// swipe or a page button makes, which is what keeps the two together.
pub fn pages(
    text: &mut TextRenderer,
    theme: &Theme,
    stats: &Stats,
    book: usize,
    area: Rect,
) -> Vec<usize> {
    let held: Vec<(Mark, Option<Mark>)> = stats
        .marked(book)
        .iter()
        .map(|m| (m.mark.clone(), m.note.cloned()))
        .collect();
    let Some(record) = stats.books.get(book) else {
        return Vec::new();
    };
    if held.is_empty() {
        return Vec::new();
    }
    let inner = list_box(text, theme, area);
    let at = Layout {
        book: record,
        m: Metrics::of(text, theme),
        lines: lines_in(Metrics::of(text, theme), theme, inner.h, held.len()),
        width: inner.w,
    };
    starts(text, theme, &held, &at, inner.h)
}

/// [`pages`] over rows already gathered.
fn starts(
    text: &mut TextRenderer,
    theme: &Theme,
    held: &[(Mark, Option<Mark>)],
    at: &Layout,
    high: i32,
) -> Vec<usize> {
    let mut out = Vec::new();
    let mut opens = 0;
    while opens < held.len() {
        out.push(opens);
        opens += fits(text, theme, &held[opens..], at, high);
    }
    out
}

/// The box the rows are drawn in: `area` under the section heading and over
/// the pager's own strip.
fn list_box(text: &mut TextRenderer, theme: &Theme, area: Rect) -> Rect {
    // `split_bottom` answers the strip first and what is left above it second.
    let (_, rest) = area.split_bottom(pager::height(theme));
    rest.split_top(chrome::section_height(text, theme)).1
}

/// One book's marks under their own heading, opened at `from`, which is held
/// inside the list.
pub fn draw(cx: &mut Ctx, area: Rect, book: usize, from: usize) {
    let theme: &Theme = cx.theme;
    let s = cx.s();
    // Most recently marked first: what was marked last is what is being
    // looked for. `Stats::marked` is what decides which kinds are here and
    // which note belongs under which passage.
    let held: Vec<(Mark, Option<Mark>)> = cx
        .stats
        .marked(book)
        .iter()
        .map(|m| (m.mark.clone(), m.note.cloned()))
        .collect();
    let named = heading(cx, book);
    let inner = list_box(cx.text, theme, area);
    chrome::section(cx.fb, cx.text, theme, area, &named);

    if held.is_empty() {
        cx.text.set_px(theme.body_px);
        let script = cx.ui_script();
        let baseline = inner.y + theme.gap + cx.text.cap_height() as i32;
        cx.text
            .draw_in(script, cx.fb, inner.x, baseline, s.marks_none, false);
        return;
    }

    let Some(record) = cx.stats.books.get(book).cloned() else {
        return;
    };
    let m = Metrics::of(cx.text, theme);
    // A list short enough to give every row its full [`BODY_MOST`] does; a
    // longer one is set at [`BODY_LINES`] so more of it stands at once.
    let at = Layout {
        book: &record,
        m,
        lines: lines_in(m, theme, inner.h, held.len()),
        width: inner.w,
    };
    // The page showing is the last one opening at or before `from`, so an
    // index left over from another book lands on a real page.
    let opens = starts(cx.text, theme, &held, &at, inner.h);
    let page = opens.iter().rposition(|o| *o <= from).unwrap_or(0);
    let from = opens[page];
    let to = opens.get(page + 1).copied().unwrap_or(held.len());
    if opens.len() > 1 {
        // The strip along the foot, which is where every screen paged as a
        // whole is paged from.
        let label = format!("{}–{to} {} {}", from + 1, s.of, held.len());
        pager::draw(
            cx,
            pager::foot(theme, area),
            &label,
            [page > 0, page + 1 < opens.len()],
            [
                Hit::MarksPage(0),
                Hit::MarksPage(*opens.last().unwrap_or(&0)),
            ],
        );
    }

    let shown = &held[from..to];
    let mut y = inner.y;
    for (n, (mark, note)) in shown.iter().enumerate() {
        let high = height(cx.text, theme, &at, mark, note.as_ref());
        row(
            cx,
            Rect::new(inner.x, y, inner.w, high),
            mark,
            note.as_ref(),
            &at,
        );
        // A rule between rows, and none under the last.
        if n + 1 < shown.len() {
            paint::hline(
                cx.fb,
                inner.x,
                y + high - theme.gap * 3 / 2,
                inner.w,
                PALE,
                1,
            );
        }
        y += high;
    }
}

/// What every row of one list is measured and drawn against.
#[derive(Clone, Copy)]
struct Layout<'a> {
    /// The book, whose language picks the face the words are set in.
    book: &'a BookStat,
    m: Metrics,
    /// Passage lines a row is allowed, of [`BODY_MOST`].
    lines: usize,
    /// The width a row draws into.
    width: i32,
}

/// Rows of `held` from `at` that a box `high` tall holds, each at its own
/// height, and never fewer than one.
fn fits(
    text: &mut TextRenderer,
    theme: &Theme,
    held: &[(Mark, Option<Mark>)],
    at: &Layout,
    high: i32,
) -> usize {
    let (mut deep, mut used) = (0, 0);
    for (mark, note) in held {
        let row = height(text, theme, at, mark, note.as_ref());
        if deep > 0 && used + row > high {
            break;
        }
        used += row;
        deep += 1;
    }
    deep.max(1)
}

/// How many lines the passage and the note actually wrap to.
fn wrapped(
    text: &mut TextRenderer,
    theme: &Theme,
    at: &Layout,
    mark: &Mark,
    note: Option<&Mark>,
) -> (usize, usize) {
    let script = Script::of_language(&at.book.language);
    let room = (at.width - indent(theme)).max(1) as u32;
    text.set_px(theme.body_px);
    let said = text
        .wrap_and_clamp_in(script, &mark.body, room, at.lines)
        .len();
    // The note is set beside the rule, not past it, so it has the whole width.
    let noted = match note {
        Some(note) => text
            .wrap_and_clamp_in(script, &note.body, at.width.max(1) as u32, NOTE_LINES)
            .len(),
        None => 0,
    };
    (said, noted)
}

/// The height one row takes, the words wrapped to see how many lines they
/// actually run to.
fn height(
    text: &mut TextRenderer,
    theme: &Theme,
    at: &Layout,
    mark: &Mark,
    note: Option<&Mark>,
) -> i32 {
    let (said, noted) = wrapped(text, theme, at, mark, note);
    at.m.row(theme, said, noted)
}

/// One mark: the passage behind a rule in the colour the highlight was made
/// in, the note written on it under that, and the label stating where and when
/// below both.
///
/// The rule takes the page's own left margin and the words run to its right
/// one, so the white either side of the block is the same.
fn row(cx: &mut Ctx, area: Rect, mark: &Mark, note: Option<&Mark>, at: &Layout) {
    let theme: &Theme = cx.theme;
    let s = cx.s();
    let (m, book) = (at.m, at.book);
    let script = Script::of_language(&book.language);
    let (said, noted) = wrapped(cx.text, theme, at, mark, note);
    let x = area.x + indent(theme);

    // The rule beside the passage, in the colour the sidecar stated. A mark
    // no sidecar reached states none, and takes the neutral.
    let coloured = paint::is_coloured(&cx.palette);
    let ink = paint::mark_colour(&mark.colour, coloured);
    let quoted = m.quoted(theme, said);
    paint::fill_rgb(
        cx.fb,
        Rect::new(
            area.x,
            area.y + theme.gap,
            rule_width(theme),
            (quoted - theme.gap).max(1),
        ),
        ink,
    );

    // The passage, set in the book's own script.
    cx.text.set_px(theme.body_px);
    let room = (area.right() - x).max(1) as u32;
    let lines_of = cx
        .text
        .wrap_and_clamp_in(script, &mark.body, room, at.lines);
    let mut y = area.y + theme.gap * 2 + cx.text.cap_height() as i32;
    for line in &lines_of {
        cx.text.draw_in(script, cx.fb, x, y, line, false);
        y += m.line;
    }

    // The reader's own note, under the passage and past the end of its rule,
    // set in the same face and standing at the rule's own left edge. The rule
    // and the indent mark the passage; the note has neither, and that is what
    // tells the two apart.
    if let Some(note) = note {
        let lines_of =
            cx.text
                .wrap_and_clamp_in(script, &note.body, area.w.max(1) as u32, NOTE_LINES);
        let mut baseline = area.y + quoted + theme.gap + cx.text.cap_height() as i32;
        for line in &lines_of {
            cx.text
                .draw_in(script, cx.fb, area.x, baseline, line, false);
            baseline += m.line;
        }
    }

    // The label last: it is what the row is about only once the words are
    // read.
    cx.text.set_px(theme.small_px);
    let ui = cx.ui_script();
    let baseline = area.y + quoted + m.noted(theme, noted) + theme.gap * 2 + m.cap;
    cx.text
        .draw_in(ui, cx.fb, area.x, baseline, &label(mark, book, s), false);
    // The day it was made, against the right edge.
    if let Some(day) = mark.day() {
        let when = crate::date::year_day(day, s);
        let w = cx.text.measure_width_in(ui, &when) as i32;
        cx.text
            .draw_in(ui, cx.fb, area.right() - w, baseline, &when, false);
    }
}

/// What stands over the list: the book it belongs to. The count is the tab's
/// own, and the title is cut to two thirds of the strip, the rest being the
/// pager's.
fn heading(cx: &mut Ctx, book: usize) -> String {
    let theme: &Theme = cx.theme;
    let Some(book) = cx.stats.books.get(book) else {
        return String::new();
    };
    let (title, language) = (book.title.clone(), book.language.clone());
    let script = Script::of_language(&language);
    cx.text.set_px(theme.small_px);
    let room = (chrome::content_box(theme).w * 2 / 3).max(1) as u32;
    let cut = cx.text.wrap_and_clamp_in(script, &title, room, 1);
    cut.first().cloned().unwrap_or_default()
}

/// What a row's label reads: where the mark falls and what kind it is —
/// `Location 608 | Highlight`. The day stands at the other end of the line.
fn label(mark: &Mark, book: &BookStat, s: &Strings) -> String {
    format!("{}{BAR}{}", place(mark, book, s), kind_name(mark.kind, s))
}

/// Where a mark falls, as a row states it: every one of the three the two
/// sources between them named, in the order a reader meets them.
///
/// The publisher's page and the display location are the clipping's, and the
/// percentage is the sidecar's own position against the book's extent. They
/// are **three different axes** and none converts to another, so each stands
/// only where its own source stated it. A mark with none says so.
fn place(mark: &Mark, book: &BookStat, s: &Strings) -> String {
    let mut said: Vec<String> = Vec::with_capacity(3);
    if !mark.page.is_empty() {
        said.push(format!("{} {}", s.at_page, mark.page));
    }
    if mark.location >= 0 {
        said.push(format!("{} {}", s.at_location, mark.location));
    }
    if let Some(through) = mark.through(book.extent) {
        said.push(
            s.percent_plain
                .replace("{d}", &format!("{:.0}", through * 100.0)),
        );
    }
    match said.is_empty() {
        true => DASH.into(),
        false => said.join(DOT),
    }
}

/// What a kind is called. The five only a `.sdr` carries share one word: they
/// are rare, they carry no words of their own, and naming each would be five
/// strings in five languages for a row that says nothing else.
pub fn kind_name(kind: Kind, s: &Strings) -> &'static str {
    match kind {
        Kind::Bookmark => s.kind_bookmark,
        Kind::Highlight => s.kind_highlight,
        Kind::Note => s.kind_note,
        Kind::Article => s.kind_article,
        Kind::Underline => s.kind_underline,
        Kind::Circle => s.kind_circle,
        Kind::Asterisk => s.kind_asterisk,
        _ => s.kind_other,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::annotate::State;
    use crate::lang::Lang;
    use crate::ui::theme::tests::PANELS;

    fn en() -> &'static Strings {
        Lang::English.strings()
    }

    fn mark(state: State, start: i64, location: i64) -> Mark {
        Mark {
            extent: 1_000,
            title: "A Book".into(),
            kind: Kind::Highlight,
            at: "2026-09-07T09:41:55".into(),
            state,
            start,
            end: start + 60,
            location,
            page: String::new(),
            colour: "orange".into(),
            body: "a line".into(),
        }
    }

    /// A book of a known extent, so a position reads as a fraction of it.
    fn book(extent: i64) -> BookStat {
        BookStat {
            extent,
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
            seconds: 0,
            counted_seconds: 0,
            dwell_seconds: 0,
            awake_seconds: 0,
            sittings: 0,
            page_turns: 0,
            words: 0,
            days: 0,
            first_day: 0,
            last_day: 0,
            last_secs: 0,
            stated_time_left: None,
            stated_wpm: None,
            device_seconds: 0,
            device_words: 0,
            marks: Vec::new(),
        }
    }

    #[test]
    fn a_row_states_every_place_its_sources_named() {
        // The page and the location came off the clipping, the percentage off
        // the sidecar's own position: 330 of 1000.
        let mut held = mark(State::Live, 330, 111);
        held.page = "15".into();
        assert_eq!(
            place(&held, &book(1_000), en()),
            "page 15 · Location 111 · 33%"
        );
    }

    #[test]
    fn a_place_no_source_named_is_never_derived_from_another() {
        // A location is roughly 150 positions wide and is on another axis
        // altogether: stating a percentage off one would be inventing it.
        let held = mark(State::Unconfirmed, -1, 111);
        assert_eq!(place(&held, &book(1_000), en()), "Location 111");
        // A sidecar record no clipping speaks for has the fraction alone.
        let mut held = mark(State::Live, 330, -1);
        assert_eq!(place(&held, &book(1_000), en()), "33%");
        // And a book whose extent nothing states says nothing at all.
        held.location = -1;
        assert_eq!(place(&held, &book(0), en()), DASH);
    }

    #[test]
    fn every_kind_is_named() {
        for kind in Kind::ALL {
            assert!(!kind_name(kind, en()).is_empty(), "{kind:?}");
        }
    }

    #[test]
    fn a_row_stands_as_tall_as_the_words_it_holds() {
        // The heights a face would answer, so the arithmetic is read without
        // one: a small line, a cap, and a body line.
        let m = Metrics {
            cap: 14,
            small: 20,
            line: 26,
        };
        for (w, h) in PANELS {
            let theme = Theme::for_screen(w, h);
            // A row is as tall as its own words: a one-line passage takes a
            // shorter row than a two-line one.
            assert!(m.row(&theme, 2, 0) > m.row(&theme, 1, 0));
            assert_eq!(m.row(&theme, 0, 0), m.row(&theme, 1, 0), "never no lines");
            // A row carrying a note stands taller than the same row without.
            assert!(m.row(&theme, 2, NOTE_LINES) > m.row(&theme, 2, 0));
            // A crowded list holds every row to [`BODY_LINES`].
            assert_eq!(lines_in(m, &theme, 1, 4), BODY_LINES);
            // A short one gives them all [`BODY_MOST`].
            assert_eq!(
                lines_in(m, &theme, m.row(&theme, BODY_MOST, 0) * 4, 4),
                BODY_MOST
            );
        }
    }
}
