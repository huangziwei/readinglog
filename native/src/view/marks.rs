//! One book's highlights and notes, most recent first. `Mark::body` comes
//! from the clippings file and `Mark::start` from the `.sdr` sidecar;
//! [`place`] states each on its own axis.

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
use crate::wrap::{MORE, mark_more};

use super::{Ctx, Hit, Search, pager, search};

/// What separates the parts of a row's own label: `Location 608 | Highlight`.
const BAR: &str = " | ";
const DOT: &str = " · ";

/// What a book with no place for a mark states in place of one.
const DASH: &str = "—";

/// The most passage lines one row of a [`Layout::found`] list shows.
const WINDOW_LINES: usize = 4;

/// The most lines a row's `title` takes, wrapped to the row's own width.
const NAME_LINES: usize = 3;

/// One row of a list of marks: `mark`, the `note` written on it, and the
/// `title`, `language` and `extent` of the book it was made in.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(super) struct Row {
    /// The book, by its index in `Stats::books`.
    pub book: usize,
    /// The row's place in the book's `Stats::marked` list, where its Marks tab opens.
    pub at: usize,
    pub mark: Mark,
    pub note: Option<Mark>,
    /// The book's title, standing over the row in a list running across books.
    pub title: String,
    /// The book's catalog language, which picks the face the words are set in.
    pub language: String,
    /// The book's extent, which the percentage is measured against.
    pub extent: i64,
}

impl Row {
    pub(super) fn new(
        book: usize,
        at: usize,
        record: &BookStat,
        mark: &Mark,
        note: Option<&Mark>,
    ) -> Self {
        Self {
            book,
            at,
            mark: mark.clone(),
            note: note.cloned(),
            title: record.title.clone(),
            language: record.language.clone(),
            extent: record.extent,
        }
    }

    fn script(&self) -> Script {
        Script::of_language(&self.language)
    }
}

/// How wide the rule beside a passage is.
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
    /// How far a line of the passage or its note stands over its own baseline.
    body_cap: i32,
    /// The label's line.
    small: i32,
    /// A line of the passage or its note, [`leading`] over the face's own line.
    line: i32,
}

/// What a line of a passage is opened up by over the face's own line height.
fn leading(theme: &Theme) -> i32 {
    theme.gap
}

/// The white the rule holds over a passage's first cap and under its last
/// baseline.
fn air(theme: &Theme) -> i32 {
    theme.gap * 3 / 2
}

impl Metrics {
    fn of(text: &mut TextRenderer, theme: &Theme) -> Self {
        text.set_px(theme.small_px);
        let (cap, small) = (text.cap_height() as i32, text.line_height() as i32);
        text.set_px(theme.body_px);
        Self {
            cap,
            body_cap: text.cap_height() as i32,
            small,
            line: text.line_height() as i32 + leading(theme),
        }
    }

    /// The height `lines` of passage take, the air over them included, which
    /// the note and the label stand under. Never less than [`Metrics::ruled`].
    fn quoted(self, theme: &Theme, lines: usize) -> i32 {
        let (from, deep) = self.ruled(theme, lines);
        (theme.gap * 2 + lines.max(1) as i32 * self.line).max(from + deep)
    }

    /// Where the first line of the passage sets its baseline, from the head
    /// of the block.
    fn baseline(self, theme: &Theme) -> i32 {
        theme.gap + air(theme) + self.body_cap
    }

    /// The rule `paint::mark_colour` draws beside those lines, as its head and
    /// height from the head of the block. [`air`] stands over the first line's
    /// cap and under the last line's baseline.
    fn ruled(self, theme: &Theme, lines: usize) -> (i32, i32) {
        let from = theme.gap;
        let last = self.baseline(theme) + (lines.max(1) as i32 - 1) * self.line;
        (from, last + air(theme) - from)
    }

    /// The height the note under a passage takes, and 0 where there is none.
    fn noted(self, theme: &Theme, note: usize) -> i32 {
        match note {
            0 => 0,
            n => theme.gap + n as i32 * self.line,
        }
    }

    /// The height a row takes: `name` lines over the passage's `lines`, the
    /// `note` under those, [`label`]'s own line under that, and the air
    /// closing the row.
    fn row(self, theme: &Theme, name: usize, lines: usize, note: usize) -> i32 {
        name as i32 * self.small
            + self.quoted(theme, lines)
            + self.noted(theme, note)
            + theme.gap * 2
            + self.small
            + theme.gap * 3
    }
}

/// The passage lines a [`Layout::found`] row shows in a box `high` tall:
/// [`WINDOW_LINES`], less wherever two rows that deep overflow `high`. The
/// row measured carries one line of `title` and no `note`.
fn window_lines(m: Metrics, theme: &Theme, high: i32) -> usize {
    let mut lines = WINDOW_LINES;
    while lines > 1 && m.row(theme, 1, lines, 0) * 2 > high {
        lines -= 1;
    }
    lines
}

/// The most passage lines one row can take in a box `h` tall. A `mark.body`
/// running past it is ellipsized there.
fn lines_in(m: Metrics, theme: &Theme, h: i32) -> usize {
    let mut lines = 1;
    while m.row(theme, 0, lines + 1, 0) <= h {
        lines += 1;
    }
    lines
}

/// Where each page of `book`'s marks opens in `area`, ascending from 0, and
/// empty for a book carrying none. [`draw`] and the page step read this one
/// list; `search` names the passages it holds.
pub fn pages(
    text: &mut TextRenderer,
    theme: &Theme,
    stats: &Stats,
    book: usize,
    area: Rect,
    search: Option<&Search>,
) -> Vec<usize> {
    let Some(record) = stats.books.get(book) else {
        return Vec::new();
    };
    let query = search.map(|search| search.query.as_str());
    let held = listing(stats, book, record, query);
    if held.is_empty() {
        return Vec::new();
    }
    let page = page_box(theme, area, search.is_some_and(|search| search.keyboard));
    let inner = list_box(text, theme, page);
    let at = layout(text, theme, inner, query);
    openings(text, theme, &held, &at, inner.h)
}

/// One book's own rows, most recently marked first, as its list holds them.
fn rows(stats: &Stats, book: usize, record: &BookStat) -> Vec<Row> {
    stats
        .marked(book)
        .iter()
        .enumerate()
        .map(|(at, m)| Row::new(book, at, record, m.mark, m.note))
        .collect()
}

/// The rows a list holds: every one of `book`'s, or those a `query` names by
/// the words of the passage or of the note on it.
fn listing(stats: &Stats, book: usize, record: &BookStat, query: Option<&str>) -> Vec<Row> {
    let held = rows(stats, book, record);
    let Some(needle) = query.map(search::Needle::of) else {
        return held;
    };
    held.into_iter()
        .filter(|row| {
            needle.holds(&row.mark.body)
                || row
                    .note
                    .as_ref()
                    .is_some_and(|note| needle.holds(&note.body))
        })
        .collect()
}

/// What the rows are measured against: a list a query found windows each
/// passage on it, and a book's own list gives a row every line its box holds.
fn layout<'a>(
    text: &mut TextRenderer,
    theme: &Theme,
    inner: Rect,
    query: Option<&'a str>,
) -> Layout<'a> {
    let at = Layout::of(text, theme, inner, false);
    match query {
        Some(query) => at.found(theme, inner.h, query),
        None => at,
    }
}

/// Where each page of `held` opens, ascending and starting at 0.
pub(super) fn openings(
    text: &mut TextRenderer,
    theme: &Theme,
    held: &[Row],
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

/// The page the list and its pager share: `area`, floored at the keyboard's
/// top edge where the keyboard stands.
fn page_box(theme: &Theme, area: Rect, keyboard: bool) -> Rect {
    match keyboard {
        true => super::over_keyboard(theme, area),
        false => area,
    }
}

/// The box the rows are drawn in: `area` under the section heading and over
/// the pager's own strip.
fn list_box(text: &mut TextRenderer, theme: &Theme, area: Rect) -> Rect {
    // `split_bottom` answers the strip first and what is left above it second.
    let (_, rest) = area.split_bottom(pager::height(theme));
    rest.split_top(chrome::section_height(text, theme)).1
}

/// One book's marks under their own heading, opened at `from`, which is held
/// inside the list. `search` names the passages listed, opens the list at its
/// own `from`, and floors the box while the keyboard stands.
pub fn draw(cx: &mut Ctx, area: Rect, book: usize, from: usize, search: Option<&Search>) {
    let theme: &Theme = cx.theme;
    let s = cx.s();
    // `Stats::marked` sets the order, the kinds that are here, and which
    // note belongs under which passage.
    let Some(record) = cx.stats.books.get(book).cloned() else {
        return;
    };
    let query = search.map(|search| search.query.as_str());
    let held = listing(cx.stats, book, &record, query);
    let named = heading(cx, book);
    let page = page_box(theme, area, search.is_some_and(|search| search.keyboard));
    let inner = list_box(cx.text, theme, page);
    chrome::section(cx.fb, cx.text, theme, area, &named);

    if held.is_empty() {
        cx.text.set_px(theme.body_px);
        let script = cx.ui_script();
        let baseline = inner.y + theme.gap + cx.text.cap_height() as i32;
        // An empty list under a query holding nothing is a book with no
        // marks of its own.
        let said = match query.is_some_and(|query| !query.is_empty()) {
            true => s.nothing_marked,
            false => s.marks_none,
        };
        cx.text
            .draw_in(script, cx.fb, inner.x, baseline, said, false);
        return;
    }

    let at = layout(cx.text, theme, inner, query);
    // The page showing opens at the last opening at or before `from`.
    let from = search.map_or(from, |search| search.from);
    let opens = openings(cx.text, theme, &held, &at, inner.h);
    let page_at = opens.iter().rposition(|o| *o <= from).unwrap_or(0);
    let from = opens[page_at];
    let to = opens.get(page_at + 1).copied().unwrap_or(held.len());
    if opens.len() > 1 {
        // The strip along the foot, which every screen paged whole carries.
        let label = format!("{}–{to} {} {}", from + 1, s.of, held.len());
        let last = opens.last().copied().unwrap_or(0);
        let steps = match search {
            Some(_) => [Hit::SearchPage(0), Hit::SearchPage(last)],
            None => [Hit::MarksPage(0), Hit::MarksPage(last)],
        };
        pager::draw(
            cx,
            pager::foot(theme, page),
            &label,
            [page_at > 0, page_at + 1 < opens.len()],
            steps,
        );
    }

    let shown = &held[from..to];
    let mut y = inner.y;
    for (n, held) in shown.iter().enumerate() {
        let high = height(cx.text, theme, &at, held);
        let box_of = Rect::new(inner.x, y, inner.w, high);
        row(cx, box_of, held, &at);
        // A row a search found opens the book's own list at that passage.
        if search.is_some() {
            cx.hit(Hit::Mark(held.book, held.at), box_of);
        }
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
pub(super) struct Layout<'a> {
    m: Metrics,
    /// The most passage lines a row may take, which is what the box holds.
    lines: usize,
    /// The width a row draws into.
    width: i32,
    /// Whether a row states the book it was made in, over its own words.
    named: bool,
    /// The query a list is drawn against, where one names its rows.
    needle: Option<&'a str>,
}

impl<'a> Layout<'a> {
    /// What a list drawn into `box_` is measured against. `named` states
    /// whether a row carries the book it was made in.
    pub(super) fn of(text: &mut TextRenderer, theme: &Theme, box_: Rect, named: bool) -> Self {
        let m = Metrics::of(text, theme);
        Self {
            m,
            lines: lines_in(m, theme, box_.h),
            width: box_.w,
            named,
            needle: None,
        }
    }

    /// The same list in a box `high` tall, over rows a search found: each
    /// shows [`window_lines`] of its passage at the most, opening on the line
    /// holding `query`. An empty `query` opens every row at its head.
    pub(super) fn found(self, theme: &Theme, high: i32, query: &'a str) -> Self {
        Self {
            lines: self.lines.min(window_lines(self.m, theme, high)),
            needle: (!query.is_empty()).then_some(query),
            ..self
        }
    }
}

/// Rows of `held` from `at` that a box `high` tall holds, each at its own
/// height, and never fewer than one.
fn fits(text: &mut TextRenderer, theme: &Theme, held: &[Row], at: &Layout, high: i32) -> usize {
    let (mut deep, mut used) = (0, 0);
    for held in held {
        let row = height(text, theme, at, held);
        if deep > 0 && used + row > high {
            break;
        }
        used += row;
        deep += 1;
    }
    deep.max(1)
}

/// What one row's three parts wrap to, and where its passage opens.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
struct Wrapped {
    /// The book's name on its own lines, empty where the list does not state it.
    name: Vec<String>,
    /// The lines of the passage the row shows, from [`Wrapped::opens`].
    said: Vec<String>,
    noted: Vec<String>,
    /// The line of the whole passage the row opens on, 0 at its head.
    opens: usize,
}

/// How the words of one row fall against `at`.
fn wrapped(text: &mut TextRenderer, theme: &Theme, at: &Layout, held: &Row) -> Wrapped {
    let script = held.script();
    let name = match at.named {
        false => Vec::new(),
        true => {
            text.set_px(theme.small_px);
            text.wrap_and_clamp_in(script, &held.title, at.width.max(1) as u32, NAME_LINES)
        }
    };
    let room = (at.width - indent(theme)).max(1) as u32;
    text.set_px(theme.body_px);
    // With no query the passage wraps as far as the row shows; with one it
    // wraps whole, and [`opens_at`] reads every line of it.
    let (said, opens) = match at.needle {
        None => (
            text.wrap_and_clamp_in(script, &held.mark.body, room, at.lines),
            0,
        ),
        Some(needle) => {
            let every = text.wrap_and_clamp_in(script, &held.mark.body, room, usize::MAX);
            let opens = opens_at(&every, needle);
            (window(text, script, every, opens, at.lines, room), opens)
        }
    };
    // `note` is set beside the rule, not past it, and takes `at.width` whole.
    let noted = match &held.note {
        Some(note) => text.wrap_and_clamp_in(script, &note.body, at.width.max(1) as u32, at.lines),
        None => Vec::new(),
    };
    Wrapped {
        name,
        said,
        noted,
        opens,
    }
}

/// The lines of `every` a row shows: `lines` of them from `opens`, the last
/// marked where the passage runs on past the window.
fn window(
    text: &mut TextRenderer,
    script: Script,
    every: Vec<String>,
    opens: usize,
    lines: usize,
    room: u32,
) -> Vec<String> {
    let more = opens + lines < every.len();
    let mut said: Vec<String> = every.into_iter().skip(opens).take(lines).collect();
    if more && let Some(last) = said.last_mut() {
        mark_more(last, room, |s| text.measure_width_in(script, s));
    }
    said
}

/// `ch` as a query matches it: [`crate::hanfold::folded`], lowercased.
fn keyed(ch: char) -> char {
    let ch = crate::hanfold::folded(ch);
    ch.to_lowercase().next().unwrap_or(ch)
}

/// Every run of `needle` in `line`, as byte ranges, matched by [`keyed`] a
/// character at a time.
fn runs_in(line: &str, needle: &str) -> Vec<(usize, usize)> {
    let want: Vec<char> = needle.chars().map(keyed).collect();
    if want.is_empty() {
        return Vec::new();
    }
    let held: Vec<(usize, char)> = line
        .char_indices()
        .map(|(at, ch)| (at, keyed(ch)))
        .collect();
    let mut out = Vec::new();
    let mut at = 0;
    while at + want.len() <= held.len() {
        let same = held[at..at + want.len()]
            .iter()
            .map(|(_, ch)| *ch)
            .eq(want.iter().copied());
        if !same {
            at += 1;
            continue;
        }
        let to = match held.get(at + want.len()) {
            Some((byte, _)) => *byte,
            None => line.len(),
        };
        out.push((held[at].0, to));
        at += want.len();
    }
    out
}

/// `Palette::wash` behind every run of `needle` in `line`, which is set from
/// `x` on `baseline` at [`Theme::body_px`].
fn wash(cx: &mut Ctx, script: Script, x: i32, baseline: i32, line: &str, needle: &str) {
    let cap = cx.text.cap_height() as i32;
    // The band clears the ascenders and takes in the descenders under it.
    let drop = ((cx.text.line_height() as i32 - cap) / 2).max(1);
    let over = (drop / 2).max(1);
    let ink = cx.palette.wash();
    for (from, to) in runs_in(line, needle) {
        let before = cx.text.measure_width_in(script, &line[..from]) as i32;
        let wide = cx.text.measure_width_in(script, &line[from..to]) as i32;
        let box_ = Rect::new(x + before, baseline - cap - over, wide, cap + over + drop);
        paint::fill_rgb(cx.fb, box_, ink);
    }
}

/// The line a passage's window opens on: one above the first line
/// [`runs_in`] finds `needle` in, and 0 where no line holds it.
fn opens_at(every: &[String], needle: &str) -> usize {
    every
        .iter()
        .position(|line| !runs_in(line, needle).is_empty())
        .unwrap_or(0)
        .saturating_sub(1)
}

/// The height one row takes, the words wrapped to see how many lines they
/// actually run to.
pub(super) fn height(text: &mut TextRenderer, theme: &Theme, at: &Layout, held: &Row) -> i32 {
    let w = wrapped(text, theme, at, held);
    at.m.row(theme, w.name.len(), w.said.len(), w.noted.len())
}

/// One mark: `title` where `at.named`, `mark.body` behind a rule in
/// `mark.colour`, the `note` under that, and [`label`] with `Mark::day` below
/// both. The rule stands on `area`'s left margin and the words end at its right.
pub(super) fn row(cx: &mut Ctx, area: Rect, held: &Row, at: &Layout) {
    let theme: &Theme = cx.theme;
    let s = cx.s();
    let m = at.m;
    let script = held.script();
    let w = wrapped(cx.text, theme, at, held);
    let x = area.x + indent(theme);

    // `title`, over the words and the whole width of `area`.
    cx.text.set_px(theme.small_px);
    let mut baseline = area.y + cx.text.cap_height() as i32;
    for line in &w.name {
        cx.text
            .draw_in(script, cx.fb, area.x, baseline, line, false);
        baseline += m.small;
    }
    let top = area.y + w.name.len() as i32 * m.small;

    // An empty `mark.colour` takes `paint::mark_colour`'s neutral.
    let coloured = paint::is_coloured(&cx.palette);
    let ink = paint::mark_colour(&held.mark.colour, coloured);
    let quoted = m.quoted(theme, w.said.len());
    let (from, deep) = m.ruled(theme, w.said.len());
    paint::fill_rgb(
        cx.fb,
        Rect::new(area.x, top + from, rule_width(theme), deep),
        ink,
    );

    // The passage, set in the book's own script.
    cx.text.set_px(theme.body_px);
    let mut y = top + m.baseline(theme);
    for line in &w.said {
        if let Some(needle) = at.needle {
            wash(cx, script, x, y, line, needle);
        }
        cx.text.draw_in(script, cx.fb, x, y, line, false);
        y += m.line;
    }
    // [`MORE`] stands over a passage opening past `w.opens` 0.
    if w.opens > 0 {
        cx.text
            .draw_in(script, cx.fb, x, top + theme.gap, MORE, false);
    }

    // `note` stands at `area.x`, past the rule and clear of [`indent`].
    let mut baseline = top + quoted + theme.gap + m.body_cap;
    for line in &w.noted {
        if let Some(needle) = at.needle {
            wash(cx, script, area.x, baseline, line, needle);
        }
        cx.text
            .draw_in(script, cx.fb, area.x, baseline, line, false);
        baseline += m.line;
    }

    // The label last, under the words and the note.
    cx.text.set_px(theme.small_px);
    let ui = cx.ui_script();
    let baseline = top + quoted + m.noted(theme, w.noted.len()) + theme.gap * 2 + m.cap;
    let said = label(&held.mark, held.extent, s);
    cx.text.draw_in(ui, cx.fb, area.x, baseline, &said, false);
    // The day it was made, against the right edge.
    if let Some(day) = held.mark.day() {
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
fn label(mark: &Mark, extent: i64, s: &Strings) -> String {
    format!("{}{BAR}{}", place(mark, extent, s), kind_name(mark.kind, s))
}

/// `mark.page`, `mark.location` and `mark.through(extent)`, [`DOT`]-joined in
/// that order, each standing only where its own source stated it. Three axes,
/// and none converts to another; a mark stating none takes [`DASH`].
fn place(mark: &Mark, extent: i64, s: &Strings) -> String {
    let mut said: Vec<String> = Vec::with_capacity(3);
    if !mark.page.is_empty() {
        said.push(format!("{} {}", s.at_page, mark.page));
    }
    if mark.location >= 0 {
        said.push(format!("{} {}", s.at_location, mark.location));
    }
    if let Some(through) = mark.through(extent) {
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

/// What `kind` is called. The five kinds a `.sdr` alone carries take
/// `s.kind_other` between them.
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

    #[test]
    fn a_row_states_every_place_its_sources_named() {
        // The page and the location come off the clipping, the percentage
        // off the sidecar's own position: 330 of 1000.
        let mut held = mark(State::Live, 330, 111);
        held.page = "15".into();
        assert_eq!(place(&held, 1_000, en()), "page 15 · Location 111 · 33%");
    }

    #[test]
    fn a_place_no_source_named_is_never_derived_from_another() {
        // `mark.location` is on another axis than `mark.start`.
        let held = mark(State::Unconfirmed, -1, 111);
        assert_eq!(place(&held, 1_000, en()), "Location 111");
        // A sidecar record no clipping speaks for has the fraction alone.
        let mut held = mark(State::Live, 330, -1);
        assert_eq!(place(&held, 1_000, en()), "33%");
        // And a book whose extent nothing states says nothing at all.
        held.location = -1;
        assert_eq!(place(&held, 0, en()), DASH);
    }

    #[test]
    fn a_window_opens_on_the_line_before_the_one_that_holds_the_query() {
        let every: Vec<String> = ["a first line", "a second", "a third", "a fourth"]
            .iter()
            .map(|line| line.to_string())
            .collect();
        assert_eq!(opens_at(&every, "third"), 1);
        assert_eq!(opens_at(&every, "fourth"), 2);
        // A match in the first two lines opens the passage at its head.
        assert_eq!(opens_at(&every, "first"), 0);
        assert_eq!(opens_at(&every, "SECOND"), 0, "case folded");
        // And a query the note alone matched leaves the passage where it is.
        assert_eq!(opens_at(&every, "nowhere"), 0);
    }

    #[test]
    fn a_query_is_found_in_a_line_wherever_it_stands_in_it() {
        let line = "The bigger the company";
        let runs = runs_in(line, "the");
        assert_eq!(runs, [(0, 3), (11, 14)]);
        assert_eq!(&line[runs[0].0..runs[0].1], "The");
        assert_eq!(&line[runs[1].0..runs[1].1], "the");
        // A run at the end of the line reaches the end of it.
        assert_eq!(runs_in(line, "company"), [(15, 22)]);
        // A word no line holds, and a query with nothing in it.
        assert!(runs_in(line, "zzz").is_empty());
        assert!(runs_in(line, "").is_empty());
        // Runs never overlap: two of three take the head of the line.
        assert_eq!(runs_in("aaaa", "aa"), [(0, 2), (2, 4)]);
    }

    #[test]
    fn a_query_in_one_script_is_found_in_a_line_written_in_the_other() {
        let line = "下列觀念是中國現代政治思想";
        // 观 is what a pinyin keyboard commits, and 觀 is what the line holds.
        let runs = runs_in(line, "观念");
        assert_eq!(runs.len(), 1);
        assert_eq!(&line[runs[0].0..runs[0].1], "觀念");
        assert_eq!(runs_in(line, "觀念"), runs);
        // And the window opens on the line a fold found, not on the head.
        let every: Vec<String> = ["a first line", "a second", line.to_string().as_str()]
            .iter()
            .map(|line| line.to_string())
            .collect();
        assert_eq!(opens_at(&every, "观念"), 1);
    }

    #[test]
    fn a_result_row_gives_up_a_line_rather_than_take_the_whole_page() {
        let m = Metrics {
            cap: 14,
            body_cap: 18,
            small: 20,
            line: 26,
        };
        for (w, h) in PANELS {
            let theme = Theme::for_screen(w, h);
            // A page deep enough for two of the deepest rows keeps them.
            let deep = m.row(&theme, 1, WINDOW_LINES, 0);
            assert_eq!(window_lines(m, &theme, deep * 2), WINDOW_LINES, "{w}x{h}");
            // One that is not gives the window up a line at a time, and never
            // gives up the last one.
            assert!(
                window_lines(m, &theme, deep * 2 - 1) < WINDOW_LINES,
                "{w}x{h}"
            );
            assert_eq!(window_lines(m, &theme, 1), 1, "{w}x{h}");
            // Whatever it answers, two rows that deep stand in the box.
            let lines = window_lines(m, &theme, deep + 10);
            assert!((1..=WINDOW_LINES).contains(&lines), "{w}x{h}");
        }
    }

    #[test]
    fn a_query_names_the_passages_of_one_book_and_no_other() {
        let stats = crate::stats::tests::marked_shelf();
        let record = stats.books[0].clone();
        // The whole book with no query, and with one that holds nothing.
        assert_eq!(listing(&stats, 0, &record, None).len(), 1);
        assert_eq!(listing(&stats, 0, &record, Some("")).len(), 1);
        // The passage's own words, and the note written on it.
        assert_eq!(listing(&stats, 0, &record, Some("SKY")).len(), 1);
        assert_eq!(listing(&stats, 0, &record, Some("borrowed")).len(), 1);
        // A word the other book's passage holds names nothing here.
        assert!(listing(&stats, 0, &record, Some("港")).is_empty());
        // And the row states the place its own book's list opens at.
        let held = listing(&stats, 0, &record, Some("sky"));
        assert_eq!((held[0].book, held[0].at), (0, 0));
    }

    #[test]
    fn the_keyboard_floors_the_list_while_it_stands() {
        for (w, h) in PANELS {
            let theme = Theme::for_screen(w, h);
            let area = chrome::content_box(&theme);
            let down = page_box(&theme, area, false);
            let up = page_box(&theme, area, true);
            assert_eq!(down, area, "{w}x{h}");
            assert!(up.h < down.h, "{w}x{h}: {} against {}", up.h, down.h);
            assert!(up.h > 0, "{w}x{h}");
            let over = theme.screen.bottom() - crate::keyboard::height(theme.screen.h);
            assert!(up.bottom() <= over, "{w}x{h}: the list reaches the keys");
        }
    }

    #[test]
    fn every_kind_is_named() {
        for kind in Kind::ALL {
            assert!(!kind_name(kind, en()).is_empty(), "{kind:?}");
        }
    }

    #[test]
    fn the_rule_holds_the_same_air_over_the_words_as_under_them() {
        for (w, h) in PANELS {
            let theme = Theme::for_screen(w, h);
            // The heights a face reads at this panel's own sizes: a cap of
            // `font::CAP` of the em, over a line a quarter taller than the em.
            let cap_of = |px: f32| (px * crate::font::CAP) as i32;
            let m = Metrics {
                cap: cap_of(theme.small_px),
                body_cap: cap_of(theme.body_px),
                small: theme.small_px as i32,
                line: (theme.body_px * 1.25) as i32 + leading(&theme),
            };
            for lines in 1..=WINDOW_LINES {
                let (from, deep) = m.ruled(&theme, lines);
                // The first line's cap and the last line's baseline, both
                // where `row` sets them.
                let first = m.baseline(&theme) - m.body_cap;
                let last = m.baseline(&theme) + (lines as i32 - 1) * m.line;
                assert_eq!(first - from, from + deep - last, "{w}x{h}, {lines}");
                assert_eq!(first - from, air(&theme), "{w}x{h}, {lines}");
                // And the rule stays inside the block the note stands under.
                assert!(from + deep <= m.quoted(&theme, lines), "{w}x{h}");
            }
        }
    }

    #[test]
    fn a_row_stands_as_tall_as_the_words_it_holds() {
        // The heights `Metrics::of` reads off a face.
        let m = Metrics {
            cap: 14,
            body_cap: 18,
            small: 20,
            line: 26,
        };
        for (w, h) in PANELS {
            let theme = Theme::for_screen(w, h);
            // A one-line passage takes a shorter row than a two-line one.
            assert!(m.row(&theme, 0, 2, 0) > m.row(&theme, 0, 1, 0));
            assert_eq!(
                m.row(&theme, 0, 0, 0),
                m.row(&theme, 0, 1, 0),
                "never no lines"
            );
            // A row carrying a note stands taller than the same row without,
            // and one stating its book taller again.
            assert!(m.row(&theme, 0, 2, 2) > m.row(&theme, 0, 2, 0));
            assert!(m.row(&theme, 1, 2, 0) > m.row(&theme, 0, 2, 0));
            // A box of 1 gives a passage one line, and `lines_in` counts
            // every line a taller box holds.
            assert_eq!(lines_in(m, &theme, 1), 1);
            let deep = lines_in(m, &theme, m.row(&theme, 0, 7, 0));
            assert_eq!(deep, 7, "{w}x{h}");
            assert!(m.row(&theme, 0, deep + 1, 0) > m.row(&theme, 0, 7, 0));
        }
    }
}
