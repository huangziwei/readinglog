//! One book's highlights and notes, at whichever end [`super::Way`]
//! opens the list. `Highlight::body` comes from `crate::clippings` and `Highlight::start`
//! from `crate::sidecar`; [`positions`] states each on its own axis.

use crate::clippings::Kind;
use crate::font::Script;
use crate::highlights::Highlight;
use crate::lang::Strings;
use crate::markdown::{self, Block};
use crate::stats::BookStat;
use crate::stats::Stats;
use crate::ui::chrome;
use crate::ui::paint::{self, DARK, INK, LIGHT, PALE, Rect};
use crate::ui::text::TextRenderer;
use crate::ui::theme::Theme;
use crate::wrap::{MORE, mark_more};
use crate::written::Laid;

use super::{Ctx, Hit, Search, Way, pager, search};

/// What separates the parts of a row's own label: `Location 608 | Highlight`.
const BAR: &str = " | ";
const DOT: &str = " · ";

/// What a highlight with no position states for its position.
const DASH: &str = "—";

/// What follows `Strings::more` on a cut row a tap opens.
const OPENS: &str = "›";

/// The most lines of its passage, and of its notes, a collapsed row shows.
const WINDOW_LINES: usize = 4;

/// The most lines a row's `title` takes, wrapped to the row's own width.
const NAME_LINES: usize = 3;

/// One row of a list of highlights: `highlight`, the `notes` written on it, and
/// the `title`, `language` and `extent` of the book it was made in.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Row {
    /// The book, by its index in `Stats::books`.
    pub book: usize,
    /// The row's place in `Stats::book_highlights`, where its Journal tab opens.
    pub at: usize,
    pub highlight: Highlight,
    /// Notes on the passage `Stats::written` does not hold; [`row`] draws the first.
    pub notes: Vec<Highlight>,
    /// Notes on the passage `Stats::written` holds, in the order they were written.
    pub written: Vec<Highlight>,
    /// What `Record::drawn` laid `highlight` as, where `Stats::written` holds it.
    pub laid: Option<Laid>,
    /// The book's title, standing over the row in a list running across books.
    pub title: String,
    /// The book's library language, which picks the face the words are set in.
    pub language: String,
    /// The book's extent, which the progress is measured against.
    pub extent: i64,
}

impl Row {
    /// `highlight` and `notes` as `stats` holds them, each note in
    /// `Stats::written` among [`Row::written`].
    pub(super) fn new(
        book: usize,
        at: usize,
        record: &BookStat,
        highlight: &Highlight,
        notes: &[&Highlight],
        stats: &Stats,
    ) -> Self {
        let (mut written, notes): (Vec<Highlight>, Vec<Highlight>) = notes
            .iter()
            .map(|note| (*note).clone())
            .partition(|note| stats.laid(note).is_some());
        written.sort_by(|a, b| a.at.cmp(&b.at));
        Self {
            book,
            at,
            highlight: highlight.clone(),
            notes,
            written,
            laid: stats.laid(highlight).cloned(),
            title: record.title.clone(),
            language: record.language.clone(),
            extent: record.extent,
        }
    }

    fn script(&self) -> Script {
        Script::of_language(&self.language)
    }

    /// `true` where [`Row::written`] or [`Row::laid`] holds anything: the rows
    /// whose Kindle note states its own day.
    fn writes(&self) -> bool {
        !self.written.is_empty() || self.laid.is_some()
    }
}

/// A row of one book's journal open on a page of its own: `row`, by its place
/// in the list, and `page`, which of the pages it runs over shows.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Entry {
    pub row: usize,
    pub page: usize,
}

/// How wide the rule beside a passage is.
fn rule_width(theme: &Theme) -> i32 {
    (theme.gap / 2).max(3)
}

fn indent(theme: &Theme) -> i32 {
    theme.gap * 3
}

/// How far a row is set: the most lines of its passage, and of its notes
/// together.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
struct Depth {
    lines: usize,
    notes: usize,
}

impl Depth {
    /// Every line the row holds.
    const WHOLE: Self = Self {
        lines: usize::MAX,
        notes: usize::MAX,
    };
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

/// How far over a row's foot the rule between two rows stands.
pub(super) fn rule_over(theme: &Theme) -> i32 {
    theme.gap * 3 / 2
}

/// The white over a row's first ink, from the rule over it, and under its last
/// baseline, to the rule under it.
fn pad(theme: &Theme) -> i32 {
    theme.gap * 3
}

/// Where a row's first ink stands under the row's head: [`pad`] less the
/// [`rule_over`] the row before keeps under its own rule.
fn head(theme: &Theme) -> i32 {
    pad(theme) - rule_over(theme)
}

/// Where a row's words open under its head, its passage's rule or its note's
/// first cap: at [`head`], or a gap under `name` lines of its book's name.
fn opened(m: Metrics, theme: &Theme, name: usize) -> i32 {
    match name {
        0 => head(theme),
        n => head(theme) + n as i32 * m.small + theme.gap,
    }
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

    /// Where the last of `lines` of passage sets its baseline, from the rule's
    /// head: [`air`] over the first line's cap.
    fn last(self, theme: &Theme, lines: usize) -> i32 {
        air(theme) + self.body_cap + (lines.max(1) as i32 - 1) * self.line
    }

    /// The height of the rule `paint::highlight_colour` draws beside `lines` of
    /// passage: [`air`] under the last line's baseline.
    fn ruled(self, theme: &Theme, lines: usize) -> i32 {
        self.last(theme, lines) + air(theme)
    }

    /// The white between the last baseline of one part of a row (its passage, a
    /// note or a note's day) and the cap of the part under it: a line's own white
    /// and two gaps.
    fn apart(self, theme: &Theme) -> i32 {
        self.line - self.body_cap + theme.gap * 2
    }

    /// How far `note` lines of the note under a passage reach past the passage's
    /// last baseline, to the note's own; 0 for none.
    fn noted(self, theme: &Theme, note: usize) -> i32 {
        match note {
            0 => 0,
            n => self.apart(theme) + self.body_cap + (n as i32 - 1) * self.line,
        }
    }

    /// Where [`label`]'s line sets its baseline, from the row's head:
    /// [`Metrics::apart`] under the passage's `lines` and the `note` under those.
    fn label(self, theme: &Theme, name: usize, lines: usize, note: usize) -> i32 {
        opened(self, theme, name)
            + self.last(theme, lines)
            + self.noted(theme, note)
            + self.apart(theme)
            + self.cap
    }

    /// The height a row takes: [`Metrics::label`], [`pad`] under that and the
    /// [`rule_over`] closing the row.
    fn row(self, theme: &Theme, name: usize, lines: usize, note: usize) -> i32 {
        self.label(theme, name, lines, note) + pad(theme) + rule_over(theme)
    }
}

/// The passage lines a [`Layout::found`] row shows in a box `high` tall:
/// [`WINDOW_LINES`], less wherever two rows that deep overflow `high`; `m.row`
/// measures one line of `title` and no `note`.
fn window_lines(m: Metrics, theme: &Theme, high: i32) -> usize {
    let mut lines = WINDOW_LINES;
    while lines > 1 && m.row(theme, 1, lines, 0) * 2 > high {
        lines -= 1;
    }
    lines
}

/// The most passage lines one row can take in a box `h` tall. A `highlight.body`
/// running past it is ellipsized there.
fn lines_in(m: Metrics, theme: &Theme, h: i32) -> usize {
    let mut lines = 1;
    while m.row(theme, 0, lines + 1, 0) <= h {
        lines += 1;
    }
    lines
}

/// `book`'s page openings in `area`, ascending from 0, and empty for a book
/// carrying none: the list [`draw`] and the page step read, `search` naming its
/// rows and `ui` the script notes are set in.
#[allow(clippy::too_many_arguments)]
pub fn pages(
    text: &mut TextRenderer,
    theme: &Theme,
    stats: &Stats,
    book: usize,
    area: Rect,
    order: Way,
    search: Option<&Search>,
    ui: Script,
) -> Vec<usize> {
    let Some(record) = stats.books.get(book) else {
        return Vec::new();
    };
    let query = search.map(|search| search.query.as_str());
    let held = listing(stats, book, record, order, query);
    if held.is_empty() {
        return Vec::new();
    }
    let page = page_box(theme, area, search.is_some_and(|search| search.keyboard));
    let inner = list_box(text, theme, page);
    let at = layout(text, theme, inner, query, ui);
    paged(text, theme, &held, &at)
        .iter()
        .map(Page::opening)
        .collect()
}

/// How many pages `book`'s row `row` runs over open on its own in `area`, the
/// list in `order`; 0 where it holds no such row.
#[allow(clippy::too_many_arguments)]
pub fn entry_pages(
    text: &mut TextRenderer,
    theme: &Theme,
    stats: &Stats,
    book: usize,
    area: Rect,
    order: Way,
    row: usize,
    ui: Script,
) -> usize {
    let Some(record) = stats.books.get(book) else {
        return 0;
    };
    let held = listing(stats, book, record, order, None);
    let Some(one) = held.get(row) else {
        return 0;
    };
    let inner = list_box(text, theme, area);
    let at = layout(text, theme, inner, None, ui);
    let whole = plan_in(text, theme, &at, one, Depth::WHOLE);
    parts(text, theme, &whole, inner.h).len()
}

/// One book's own rows, most recently made first, as its list holds them.
fn rows(stats: &Stats, book: usize, record: &BookStat) -> Vec<Row> {
    stats
        .book_highlights(book)
        .iter()
        .enumerate()
        .map(|(at, m)| Row::new(book, at, record, m.highlight, &m.notes, stats))
        .collect()
}

/// The rows a list holds, in `order`: every one of `book`'s, or those a `query`
/// names by the words of the passage or of a note on it. `Way::Up` leaves
/// every `Row::at` alone.
pub fn listing(
    stats: &Stats,
    book: usize,
    record: &BookStat,
    order: Way,
    query: Option<&str>,
) -> Vec<Row> {
    let mut held = rows(stats, book, record);
    if let Some(needle) = query.map(search::Needle::of) {
        held.retain(|row| {
            needle.holds(&row.highlight.body)
                || row
                    .notes
                    .iter()
                    .chain(&row.written)
                    .any(|note| needle.holds(&note.body))
                || row
                    .laid
                    .as_ref()
                    .and_then(|laid| laid.gone.as_deref())
                    .is_some_and(|words| needle.holds(words))
        });
    }
    if order == Way::Up {
        held.reverse();
    }
    held
}

/// What the rows are measured against: [`Layout::found`], each row collapsed
/// and, under a query, its passage opening where the query stands.
fn layout<'a>(
    text: &mut TextRenderer,
    theme: &Theme,
    inner: Rect,
    query: Option<&'a str>,
    ui: Script,
) -> Layout<'a> {
    Layout::of(text, theme, inner, false, ui).found(theme, inner.h, query.unwrap_or(""))
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

/// One row as a page sets it: `row`, by its place in the list, its `depth`,
/// and `opens`, `true` where it stands cut and a tap opens it on its own page.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Piece {
    row: usize,
    depth: Depth,
    opens: bool,
}

/// What one page of a list holds, top down; never nothing.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
struct Page {
    pieces: Vec<Piece>,
}

impl Page {
    fn opening(&self) -> usize {
        self.pieces.first().map_or(0, |piece| piece.row)
    }
}

/// Every page of `held` in a box `at.high` tall, each row collapsed, the one a
/// page cannot hold cut to the room left where `at.needle` is `None`.
fn paged(text: &mut TextRenderer, theme: &Theme, held: &[Row], at: &Layout) -> Vec<Page> {
    let mut out = Vec::new();
    let mut page = Page::default();
    let mut used = 0;
    for (row, one) in held.iter().enumerate() {
        let collapsed = plan(text, theme, at, one);
        if used == 0 || used + collapsed.high <= at.high {
            page.pieces.push(Piece {
                row,
                depth: collapsed.depth,
                opens: collapsed.cut,
            });
            used += collapsed.high;
            continue;
        }
        let cut = match at.needle {
            None => squeezed(text, theme, at, one, &collapsed, at.high - used),
            Some(_) => None,
        };
        if let Some(depth) = cut {
            page.pieces.push(Piece {
                row,
                depth,
                opens: true,
            });
        }
        out.push(std::mem::take(&mut page));
        used = 0;
        if cut.is_none() {
            page.pieces.push(Piece {
                row,
                depth: collapsed.depth,
                opens: collapsed.cut,
            });
            used = collapsed.high;
        }
    }
    if !page.pieces.is_empty() {
        out.push(page);
    }
    out
}

/// How far `held` is set cut to the most of its lines that stand in `room`,
/// its passage's kept before its notes', `collapsed` being how it stands
/// collapsed; `None` where not one line does.
fn squeezed(
    text: &mut TextRenderer,
    theme: &Theme,
    at: &Layout,
    held: &Row,
    collapsed: &Plan,
    room: i32,
) -> Option<Depth> {
    let said = collapsed.said;
    let depth = |lines: usize| Depth {
        lines: lines.min(said),
        notes: lines - lines.min(said),
    };
    let most = said + collapsed.depth.notes;
    let fits = |text: &mut TextRenderer, lines: usize| {
        plan_in(text, theme, at, held, depth(lines)).high <= room
    };
    if most < 2 || !fits(text, 1) {
        return None;
    }
    // The most lines that fit, of fewer than `most`, which does not.
    let (mut fit, mut over) = (1, most);
    while over - fit > 1 {
        let mid = (fit + over) / 2;
        match fits(text, mid) {
            true => fit = mid,
            false => over = mid,
        }
    }
    Some(depth(fit))
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
    let under = rest.split_top(chrome::section_height(text, theme)).1;
    // `chrome::section`'s rule stands a gap over `under`, the first row `rule_over` under it.
    under.split_top(rule_over(theme) - theme.gap).1
}

/// One book's highlights under their heading, in `order` and opened at `from`,
/// or `entry` on a page of its own; `search` names the rows listed, opens the
/// list at its own `from`, and floors the box while the keyboard stands.
pub fn draw(
    cx: &mut Ctx,
    area: Rect,
    book: usize,
    from: usize,
    entry: Option<Entry>,
    order: Way,
    search: Option<&Search>,
) {
    let theme: &Theme = cx.theme;
    let s = cx.s();
    // `Stats::book_highlights` sets the kinds that are here and which note belongs
    // under which passage; `order` sets which end the list opens at.
    let Some(record) = cx.stats.books.get(book).cloned() else {
        return;
    };
    let query = search.map(|search| search.query.as_str());
    let held = listing(cx.stats, book, &record, order, query);
    let named = heading(cx, book);
    let page = page_box(theme, area, search.is_some_and(|search| search.keyboard));
    let inner = list_box(cx.text, theme, page);
    chrome::section(cx.fb, cx.text, theme, area, &named);

    if held.is_empty() {
        cx.text.set_px(theme.body_px);
        let script = cx.ui_script();
        let baseline = inner.y + theme.gap + cx.text.cap_height() as i32;
        // `no_highlights_in_book` where no query narrows `held`.
        let said = match query.is_some_and(|query| !query.is_empty()) {
            true => s.no_highlights_here,
            false => s.no_highlights_in_book,
        };
        cx.text
            .draw_in(script, cx.fb, inner.x, baseline, said, false);
        return;
    }

    let at = layout(cx.text, theme, inner, query, cx.ui_script());
    if let (None, Some(entry)) = (search, entry) {
        return entry_page(cx, page, inner, &held, &at, entry);
    }
    let from = search.map_or(from, |search| search.from);
    let pages = paged(cx.text, theme, &held, &at);
    // `page_at`: the last page opening at or before `from`.
    let page_at = pages
        .iter()
        .rposition(|page| page.opening() <= from)
        .unwrap_or(0);
    let Some(shown) = pages.get(page_at) else {
        return;
    };
    let first = shown.pieces.first().map_or(0, |piece| piece.row);
    let last = shown.pieces.last().map_or(0, |piece| piece.row);
    if pages.len() > 1 {
        // `pager::draw` sets the strip along the foot.
        let label = format!("{}–{} {} {}", first + 1, last + 1, s.of, held.len());
        let end = pages.last().map_or(0, Page::opening);
        let steps = match search {
            Some(_) => [Hit::SearchPage(0), Hit::SearchPage(end)],
            None => [Hit::JournalPage(0), Hit::JournalPage(end)],
        };
        pager::draw(
            cx,
            pager::foot(theme, page),
            &label,
            [page_at > 0, page_at + 1 < pages.len()],
            steps,
        );
    }

    let mut y = inner.y;
    for (n, piece) in shown.pieces.iter().enumerate() {
        let held = &held[piece.row];
        let drawn = plan_in(cx.text, theme, &at, held, piece.depth);
        let box_of = Rect::new(inner.x, y, inner.w, drawn.high);
        let opens = search.is_none() && piece.opens;
        planned(cx, box_of, held, &at, &drawn, Part::whole(&drawn), opens);
        match search {
            // `Hit::Highlight` opens the book's own list at a passage a search found.
            Some(_) => cx.hit(Hit::Highlight(held.book, held.at), box_of),
            None if opens => cx.hit(Hit::Entry(piece.row), box_of),
            None => {}
        }
        // A rule between rows, and none under the last.
        if n + 1 < shown.pieces.len() {
            paint::hline(
                cx.fb,
                inner.x,
                y + drawn.high - rule_over(theme),
                inner.w,
                PALE,
                1,
            );
        }
        y += drawn.high;
    }
}

/// `entry`'s row of `held` whole on a page of its own, at its page
/// `entry.page` in `inner`, and along `page`'s foot `‹ Journal` back to the
/// list beside the row's own pager.
fn entry_page(cx: &mut Ctx, page: Rect, inner: Rect, held: &[Row], at: &Layout, entry: Entry) {
    let theme: &Theme = cx.theme;
    let s = cx.s();
    let Some(row) = held.get(entry.row) else {
        return;
    };
    let whole = plan_in(cx.text, theme, at, row, Depth::WHOLE);
    let parts = parts(cx.text, theme, &whole, inner.h);
    let shown = entry.page.min(parts.len().saturating_sub(1));
    let part = parts.get(shown).copied().unwrap_or(Part::whole(&whole));
    let box_of = Rect::new(inner.x, inner.y, inner.w, part.high);
    planned(cx, box_of, row, at, &whole, part, false);
    let label = match parts.len() {
        0 | 1 => String::new(),
        n => format!("{} {} {n}", shown + 1, s.of),
    };
    pager::draw_back(
        cx,
        pager::foot(theme, page),
        (s.journal_tab, Hit::EntryClose),
        &label,
        [shown > 0, shown + 1 < parts.len()],
    );
}

/// What every row of one list is measured and drawn against.
#[derive(Clone, Copy)]
pub(super) struct Layout<'a> {
    m: Metrics,
    /// The most lines of its passage, and of its notes, a collapsed row shows.
    lines: usize,
    /// The width a row draws into.
    width: i32,
    /// The height of the box the rows are drawn into.
    high: i32,
    /// `true` where each row states the book it was made in, over its own words.
    named: bool,
    /// The query a list is drawn against, where one names its rows.
    needle: Option<&'a str>,
    /// [`Ctx::ui_script`]'s script, the one the notes of `Stats::written` are set in.
    ui: Script,
}

impl<'a> Layout<'a> {
    /// What a list drawn into `box_` is measured against: `named`, whether each
    /// row states its book, and `ui`, [`Ctx::ui_script`]'s script.
    pub(super) fn of(
        text: &mut TextRenderer,
        theme: &Theme,
        box_: Rect,
        named: bool,
        ui: Script,
    ) -> Self {
        let m = Metrics::of(text, theme);
        Self {
            m,
            lines: lines_in(m, theme, box_.h),
            width: box_.w,
            high: box_.h,
            named,
            needle: None,
            ui,
        }
    }

    /// The same list in a box `high` tall, each row collapsed to [`window_lines`]
    /// of its passage and of its notes, its passage opening on the line holding
    /// `query`; an empty `query` opens each at its head.
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

/// The book's name on its own lines, empty where `at` does not state it.
fn name_lines(text: &mut TextRenderer, theme: &Theme, at: &Layout, held: &Row) -> Vec<String> {
    match at.named {
        false => Vec::new(),
        true => {
            text.set_px(theme.small_px);
            text.wrap_and_clamp_in(
                held.script(),
                &held.title,
                at.width.max(1) as u32,
                NAME_LINES,
            )
        }
    }
}

/// `body`'s lines behind the rule, `lines` of them at the most, the line of the
/// whole passage they open on, and whether more of it follows them.
fn passage_lines(
    text: &mut TextRenderer,
    theme: &Theme,
    at: &Layout,
    script: Script,
    body: &str,
    lines: usize,
) -> (Vec<String>, usize, bool) {
    let room = (at.width - indent(theme)).max(1) as u32;
    text.set_px(theme.body_px);
    // `at.needle` `None` wraps the passage as far as the row shows; `Some`
    // wraps it whole for [`opens_at`].
    match at.needle {
        None => {
            let (said, cut) = clamped(text, script, body, room, lines);
            (said, 0, cut)
        }
        Some(needle) => {
            let every = text.wrap_and_clamp_in(script, body, room, usize::MAX);
            let opens = opens_at(&every, needle);
            let cut = opens.saturating_add(lines) < every.len();
            (window(text, script, every, opens, lines, room), opens, cut)
        }
    }
}

/// `words` wrapped to `room` in `script`, `lines` of them at the most, the
/// last marked [`MORE`] where they run on past it; answers whether they do.
fn clamped(
    text: &mut TextRenderer,
    script: Script,
    words: &str,
    room: u32,
    lines: usize,
) -> (Vec<String>, bool) {
    let mut out = crate::wrap::wrap_to_width(words, room, |s| text.measure_width_in(script, s));
    let cut = out.len() > lines;
    if cut {
        out.truncate(lines);
        if let Some(last) = out.last_mut() {
            mark_more(last, room, |s| text.measure_width_in(script, s));
        }
    }
    (out, cut)
}

/// One line [`plan`] sets: from `x` past the row's left edge, on `baseline`
/// under its top, at `px` in `script` and `ink`; `washed` where a search's runs
/// are washed in it.
#[derive(Clone, Debug, PartialEq)]
struct Set {
    x: i32,
    baseline: i32,
    px: f32,
    script: Script,
    ink: u8,
    washed: bool,
    words: String,
}

/// What [`planned`] draws of a row, from its top.
#[derive(Clone, Debug, Default, PartialEq)]
struct Plan {
    sets: Vec<Set>,
    /// The rule beside the passage, its top and height.
    rule: Option<(i32, i32)>,
    /// The rule beside each quotation inside a note, its top and height.
    quotes: Vec<(i32, i32)>,
    /// Where [`MORE`] stands over a passage opening past its head.
    more: Option<i32>,
    /// Each note's day under it, as the baseline it is set on.
    days: Vec<(i32, i64)>,
    /// The label's baseline, and whether it states the row's place and kind.
    label: i32,
    place: bool,
    high: i32,
    /// How far the row is set.
    depth: Depth,
    /// How many lines of its passage it sets.
    said: usize,
    /// `true` where it leaves out a line the row holds.
    cut: bool,
    /// The place in `sets` of the line the last cut closes on with [`MORE`].
    marked: Option<usize>,
}

/// One line a [`Plan`] sets, as [`parts`] reads it: its cap's top, its
/// baseline, and whether it is a heading's or one that closes the part over it,
/// a note's day or the label.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Line {
    top: i32,
    baseline: i32,
    heading: bool,
    closes: bool,
}

impl Plan {
    /// Every line the plan sets, top down, what shares a baseline as one line.
    fn lines(&self, text: &mut TextRenderer, theme: &Theme) -> Vec<Line> {
        let mut out = Vec::with_capacity(self.sets.len() + self.days.len() + 2);
        for set in &self.sets {
            text.set_px(set.px);
            out.push(Line {
                top: set.baseline - text.cap_height() as i32,
                baseline: set.baseline,
                heading: set.px == theme.head_px,
                closes: false,
            });
        }
        if let Some(over) = self.more {
            text.set_px(theme.body_px);
            out.push(Line {
                top: over - text.cap_height() as i32,
                baseline: over,
                heading: false,
                closes: false,
            });
        }
        text.set_px(theme.small_px);
        let cap = text.cap_height() as i32;
        let closing = self.days.iter().map(|(day, _)| *day).chain([self.label]);
        out.extend(closing.map(|baseline| Line {
            top: baseline - cap,
            baseline,
            heading: false,
            closes: true,
        }));
        out.sort_by_key(|line| line.baseline);
        // `dedup_by` hands over the later of two lines first.
        out.dedup_by(|line, over| {
            let one = line.baseline == over.baseline;
            if one {
                over.top = over.top.min(line.top);
                over.heading |= line.heading;
                over.closes &= line.closes;
            }
            one
        });
        out
    }
}

/// The stretch of a [`Plan`] one page shows: the lines whose baselines fall
/// past `after` and at `last` or over, standing `from` higher than the plan
/// sets them, and the height it takes on the page.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Part {
    from: i32,
    after: i32,
    last: i32,
    high: i32,
}

impl Part {
    /// All of `plan`, on one page.
    fn whole(plan: &Plan) -> Self {
        Self {
            from: 0,
            after: i32::MIN,
            last: i32::MAX,
            high: plan.high,
        }
    }

    fn shows(self, baseline: i32) -> bool {
        self.after < baseline && baseline <= self.last
    }
}

/// `plan` cut into the pages a box `high` tall holds it on: each page's last
/// baseline [`pad`] and [`rule_over`] over its foot, the next page opening
/// [`head`] over its first cap, never on a `closes` line or under a heading.
fn parts(text: &mut TextRenderer, theme: &Theme, plan: &Plan, high: i32) -> Vec<Part> {
    if plan.high <= high {
        return vec![Part::whole(plan)];
    }
    let room = high - pad(theme) - rule_over(theme);
    let lines = plan.lines(text, theme);
    let mut out = Vec::new();
    let mut part = Part {
        high,
        ..Part::whole(plan)
    };
    // The place in `lines` of the first line `part` holds, and of the line in hand.
    let (mut first, mut at) = (0, 0);
    while at < lines.len() {
        if at == first || lines[at].baseline - part.from <= room {
            at += 1;
            continue;
        }
        // `cut`, the line the next page opens on, steps back over every line
        // that keeps with the one over it, the part keeping its first.
        let mut cut = at;
        while cut - 1 > first && (lines[cut].closes || lines[cut - 1].heading) {
            cut -= 1;
        }
        let after = lines[cut - 1].baseline;
        out.push(Part {
            last: after,
            ..part
        });
        part = Part {
            from: lines[cut].top - head(theme),
            after,
            last: i32::MAX,
            high,
        };
        (first, at) = (cut, cut);
    }
    out.push(Part {
        high: plan.high - part.from,
        ..part
    });
    out
}

/// [`plan_in`] collapsed: `at.lines` of its passage and of its notes, fewer of
/// its notes where it stands taller than `at.high`.
fn plan(text: &mut TextRenderer, theme: &Theme, at: &Layout, held: &Row) -> Plan {
    let mut depth = Depth {
        lines: at.lines,
        notes: at.lines,
    };
    loop {
        let out = plan_in(text, theme, at, held, depth);
        if out.high <= at.high || depth.notes <= 1 {
            return out;
        }
        depth.notes -= 1;
    }
}

/// `held` set top down to `depth`: its passage behind a rule (a gone one's
/// words for a note written on it), its notes each over its day and sharing
/// `depth.notes` lines; a note alone with no rule and no place.
fn plan_in(text: &mut TextRenderer, theme: &Theme, at: &Layout, held: &Row, depth: Depth) -> Plan {
    let m = at.m;
    let gap = theme.gap;
    let script = held.script();
    let mut budget = depth.notes;
    let mut out = Plan {
        depth,
        ..Plan::default()
    };
    let names = name_lines(text, theme, at, held);
    let top = opened(m, theme, names.len());
    let mut baseline = head(theme) + m.cap;
    for line in names {
        out.sets.push(Set {
            x: 0,
            baseline,
            px: theme.small_px,
            script,
            ink: INK,
            washed: false,
            words: line,
        });
        baseline += m.small;
    }
    let passage = match &held.laid {
        Some(laid) => laid.gone.as_deref(),
        None => Some(held.highlight.body.as_str()),
    };
    let Some(words) = passage else {
        let (last, _) = note(
            &mut out,
            text,
            theme,
            at,
            &held.highlight.body,
            top,
            &mut budget,
        );
        footed(&mut out, theme, last + gap * 2 + m.cap);
        return out;
    };
    let (said, opens, cut) = passage_lines(text, theme, at, script, words, depth.lines);
    out.said = said.len();
    out.cut = cut || opens > 0;
    out.rule = Some((top, m.ruled(theme, said.len())));
    let first = top + m.last(theme, said.len()) + m.apart(theme);
    let mut baseline = top + air(theme) + m.body_cap;
    for line in said {
        out.sets.push(Set {
            x: indent(theme),
            baseline,
            px: theme.body_px,
            script,
            ink: INK,
            washed: true,
            words: line,
        });
        baseline += m.line;
    }
    if cut {
        out.marked = out.sets.len().checked_sub(1);
    }
    if opens > 0 {
        out.more = Some(top);
    }
    out.place = true;
    if held.laid.is_some() {
        let (last, under) = note(
            &mut out,
            text,
            theme,
            at,
            &held.highlight.body,
            first,
            &mut budget,
        );
        footed(&mut out, theme, last + under + gap * 2 + m.cap);
        return out;
    }
    // `ends`: the baseline the note set before ends on, its day's where it states one.
    let mut ends = None;
    if let Some(kindle) = held.notes.first() {
        if budget == 0 {
            ran_on(&mut out, text, at);
            out.cut = true;
        } else {
            text.set_px(theme.body_px);
            let room = at.width.max(1) as u32;
            let (lines, cut) = clamped(text, script, &kindle.body, room, budget);
            out.cut |= cut;
            budget = budget.saturating_sub(lines.len());
            let mut last = first + m.body_cap - m.line;
            for line in lines {
                last += m.line;
                out.sets.push(Set {
                    x: 0,
                    baseline: last,
                    px: theme.body_px,
                    script,
                    ink: INK,
                    washed: true,
                    words: line,
                });
            }
            if cut {
                out.marked = out.sets.len().checked_sub(1);
            }
            // `kindle` states its day only where `held.writes()`.
            ends = Some(match held.writes() {
                true => dated(&mut out, theme, m, last, kindle),
                false => last,
            });
        }
    }
    for written in &held.written {
        if budget == 0 {
            ran_on(&mut out, text, at);
            out.cut = true;
            break;
        }
        let y = ends.map_or(first, |end| end + m.apart(theme));
        let (last, _) = note(&mut out, text, theme, at, &written.body, y, &mut budget);
        ends = Some(dated(&mut out, theme, m, last, written));
    }
    let label = match ends {
        Some(end) => end + m.apart(theme) + m.cap,
        None => first + m.cap,
    };
    footed(&mut out, theme, label);
    out
}

/// `label` as the baseline `out` sets its label on, and the row's height under
/// it: [`pad`] and the [`rule_over`] closing the row.
fn footed(out: &mut Plan, theme: &Theme, label: i32) {
    out.label = label;
    out.high = label + pad(theme) + rule_over(theme);
}

/// `note`'s day a gap under the line set on `last`, where it states one;
/// answers the baseline the note ends on, the day's or `last`.
fn dated(out: &mut Plan, theme: &Theme, m: Metrics, last: i32, note: &Highlight) -> i32 {
    let Some(on) = note.day() else {
        return last;
    };
    let day = last + theme.gap + m.cap;
    out.days.push((day, on));
    day
}

/// `words`, a desktop note's Markdown, set from `y` in `at.ui` as
/// `markdown::blocks` reads it, `budget` lines at most and [`MORE`] where it runs
/// on; answers the last baseline and the room its line keeps under it.
fn note(
    out: &mut Plan,
    text: &mut TextRenderer,
    theme: &Theme,
    at: &Layout,
    words: &str,
    y: i32,
    budget: &mut usize,
) -> (i32, i32) {
    let m = at.m;
    let gap = theme.gap;
    let blocks = markdown::blocks(words);
    // The block set before: its last baseline, advance, cap, and whether an item.
    let mut before: Option<(i32, i32, i32, bool)> = None;
    for block in &blocks {
        if *budget == 0 {
            ran_on(out, text, at);
            out.cut = true;
            break;
        }
        let head = block.kind == markdown::Kind::Heading;
        let px = match head {
            true => theme.head_px,
            false => theme.body_px,
        };
        text.set_px(px);
        let (cap, advance) = match head {
            true => (text.cap_height() as i32, text.line_height() as i32),
            false => (m.body_cap, m.line),
        };
        let (lead, x) = placed(text, theme, at.ui, block);
        text.set_px(px);
        let room = (at.width - x).max(1) as u32;
        let (lines, cut) = clamped(text, at.ui, &block.words, room, *budget);
        out.cut |= cut;
        *budget = budget.saturating_sub(lines.len());
        let item = block.kind == markdown::Kind::Item;
        let first = match before {
            None => y + cap,
            Some((last, step, was_cap, was_item)) => {
                let air = match item && was_item {
                    true => 0,
                    false => gap,
                };
                last + step + air + (cap - was_cap).max(0)
            }
        };
        let mut baseline = first - advance;
        for line in lines {
            baseline += advance;
            out.sets.push(Set {
                x,
                baseline,
                px,
                script: at.ui,
                ink: INK,
                washed: true,
                words: line,
            });
        }
        if cut {
            out.marked = out.sets.len().checked_sub(1);
        }
        if let Some((lead, at_x)) = lead {
            out.sets.push(Set {
                x: at_x,
                baseline: first,
                px,
                script: at.ui,
                ink: INK,
                washed: false,
                words: lead,
            });
        }
        if block.kind == markdown::Kind::Quote {
            out.quotes
                .push((first - cap - gap / 2, baseline - first + cap + gap));
        }
        before = Some((baseline, advance, cap, item));
    }
    match before {
        Some((last, step, cap, _)) => (last, step - cap),
        None => (y, m.line - m.body_cap),
    }
}

/// [`MORE`] at the end of the last line of words `out` sets, where it carries
/// none, and that line `out.marked`.
fn ran_on(out: &mut Plan, text: &mut TextRenderer, at: &Layout) {
    let Some(at_set) = out.sets.iter().rposition(|set| set.washed) else {
        return;
    };
    out.marked = Some(at_set);
    let last = &mut out.sets[at_set];
    if !last.words.ends_with(MORE) {
        text.set_px(last.px);
        let (room, script) = ((at.width - last.x).max(1) as u32, last.script);
        mark_more(&mut last.words, room, |s| text.measure_width_in(script, s));
    }
}

/// Where `block` sets from: a quotation past [`indent`], a list item past its
/// bullet or number, each list deeper a further [`indent`]; answers the lead
/// and where it stands, where the block carries one.
fn placed(
    text: &mut TextRenderer,
    theme: &Theme,
    script: Script,
    block: &Block,
) -> (Option<(String, i32)>, i32) {
    match block.kind {
        markdown::Kind::Quote => (None, indent(theme)),
        markdown::Kind::Item => {
            let from = block.depth.saturating_sub(1) as i32 * indent(theme);
            text.set_px(theme.body_px);
            let lead = match block.lead.is_empty() {
                true => "•",
                false => block.lead.as_str(),
            };
            let x = from + text.measure_width_in(script, lead) as i32 + theme.gap;
            let lead = (!block.lead.is_empty()).then(|| (block.lead.clone(), from));
            (lead, x)
        }
        _ => (None, 0),
    }
}

/// The lines of `every` a row shows: `lines` of them from `opens`, the last
/// ending in [`MORE`] where the passage runs on past the window.
fn window(
    text: &mut TextRenderer,
    script: Script,
    every: Vec<String>,
    opens: usize,
    lines: usize,
    room: u32,
) -> Vec<String> {
    let more = opens.saturating_add(lines) < every.len();
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
pub fn runs_in(line: &str, needle: &str) -> Vec<(usize, usize)> {
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
/// `x` on `baseline` at the size `cx.text` stands at.
fn wash(cx: &mut Ctx, script: Script, x: i32, baseline: i32, line: &str, needle: &str) {
    let cap = cx.text.cap_height() as i32;
    // `drop` and `over` clear the ascenders and take in the descenders.
    let drop = ((cx.text.line_height() as i32 - cap) / 2).max(1);
    let over = (drop / 2).max(1);
    let ink = cx.palette.wash();
    for (from, to) in runs_in(line, needle) {
        // Where the pen stands at the run, and where its last glyph ends.
        let opens = cx.text.measure_upto_in(script, line, from) as i32;
        let closes = cx.text.measure_width_in(script, &line[..to]) as i32;
        let box_ = Rect::new(
            x + opens,
            baseline - cap - over,
            closes - opens,
            cap + over + drop,
        );
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

/// The height one row takes collapsed, its words wrapped to the lines they run to.
pub(super) fn height(text: &mut TextRenderer, theme: &Theme, at: &Layout, held: &Row) -> i32 {
    plan(text, theme, at, held).high
}

/// The stretch `part` shows of `held` as `plan` sets it, into `area`: its rules
/// kept to the lines `part` shows, its words, each note's day, its label with
/// `Highlight::day` at the far end, and the cut line [`closed`] where `opens`.
fn planned(
    cx: &mut Ctx,
    area: Rect,
    held: &Row,
    at: &Layout,
    plan: &Plan,
    part: Part,
    opens: bool,
) {
    let theme: &Theme = cx.theme;
    let s = cx.s();
    let y = |baseline: i32| area.y + baseline - part.from;
    // The first cap and the last baseline `part` shows, where it leaves lines out.
    let shown = match part == Part::whole(plan) {
        true => None,
        false => {
            let lines = plan.lines(cx.text, theme);
            let shown = || lines.iter().filter(|line| part.shows(line.baseline));
            let first = shown().map(|line| line.top).min();
            first.zip(shown().map(|line| line.baseline).max())
        }
    };
    let kept = |(top, deep): (i32, i32), air: i32| match shown {
        None => Some((top, deep)),
        Some((first, last)) => {
            let (from, to) = (top.max(first - air), (top + deep).min(last + air));
            (to > from).then_some((from, to - from))
        }
    };
    if let Some((top, deep)) = plan.rule.and_then(|rule| kept(rule, air(theme))) {
        let coloured = paint::is_coloured(&cx.palette);
        let colour = match held.laid.is_some() {
            true => "",
            false => held.highlight.colour.as_str(),
        };
        let ink = paint::highlight_colour(colour, coloured);
        let rule = Rect::new(area.x, y(top), rule_width(theme), deep);
        paint::fill_rgb(cx.fb, rule, ink);
    }
    for quote in &plan.quotes {
        if let Some((top, deep)) = kept(*quote, theme.gap / 2) {
            let rule = Rect::new(area.x, y(top), rule_width(theme), deep);
            paint::fill(cx.fb, rule, LIGHT);
        }
    }
    for (n, set) in plan.sets.iter().enumerate() {
        if !part.shows(set.baseline) {
            continue;
        }
        cx.text.set_px(set.px);
        let (x, baseline) = (area.x + set.x, y(set.baseline));
        if opens && plan.marked == Some(n) {
            closed(cx, set, x, baseline, area.right());
            continue;
        }
        if set.washed
            && let Some(needle) = at.needle
        {
            wash(cx, set.script, x, baseline, &set.words, needle);
        }
        cx.text
            .draw_inked(set.script, cx.fb, x, baseline, &set.words, set.ink);
    }
    if let Some(over) = plan.more.filter(|over| part.shows(*over)) {
        cx.text.set_px(theme.body_px);
        cx.text.draw_in(
            held.script(),
            cx.fb,
            area.x + indent(theme),
            y(over),
            MORE,
            false,
        );
    }
    cx.text.set_px(theme.small_px);
    let ui = cx.ui_script();
    for (baseline, day) in plan.days.iter().filter(|(day, _)| part.shows(*day)) {
        let when = crate::date::year_day(*day, s);
        cx.text
            .draw_inked(ui, cx.fb, area.x, y(*baseline), &when, DARK);
    }
    if !part.shows(plan.label) {
        return;
    }
    let baseline = y(plan.label);
    if plan.place {
        let said = label(&held.highlight, held.extent, s);
        cx.text.draw_in(ui, cx.fb, area.x, baseline, &said, false);
    }
    if let Some(day) = held.highlight.day() {
        let when = crate::date::year_day(day, s);
        let w = cx.text.measure_width_in(ui, &when) as i32;
        cx.text
            .draw_in(ui, cx.fb, area.right() - w, baseline, &when, false);
    }
}

/// `set`, a cut line, from `x` on `baseline`: its words cut whole to the room
/// [`MORE`] and `Strings::more` with [`OPENS`] take after them by `right`,
/// [`MORE`] in their ink and the rest in `DARK`.
fn closed(cx: &mut Ctx, set: &Set, x: i32, baseline: i32, right: i32) {
    let ui = cx.ui_script();
    let sign = format!("{} {OPENS}", cx.s().more);
    let gap = cx.text.measure_width_in(ui, " ") as i32;
    let reach = gap
        + cx.text.measure_width_in(set.script, MORE) as i32
        + gap
        + cx.text.measure_width_in(ui, &sign) as i32;
    let room = right - x - reach;
    let full = set.words.strip_suffix(MORE).unwrap_or(&set.words);
    let mut words = full.to_string();
    while !words.is_empty() && cx.text.measure_width_in(set.script, &words) as i32 > room {
        words.pop();
    }
    // `words` gives up a word the cut breaks, where the line breaks at spaces.
    if words.len() < full.len()
        && !full[words.len()..].starts_with(' ')
        && let Some(space) = words.rfind(' ')
    {
        words.truncate(space);
    }
    let words = words.trim_end();
    cx.text
        .draw_inked(set.script, cx.fb, x, baseline, words, set.ink);
    let mut pen = x + cx.text.measure_width_in(set.script, words) as i32;
    if words.chars().last().is_some_and(spaced) {
        pen += gap;
    }
    cx.text
        .draw_inked(set.script, cx.fb, pen, baseline, MORE, set.ink);
    pen += cx.text.measure_width_in(set.script, MORE) as i32 + gap;
    cx.text.draw_inked(ui, cx.fb, pen, baseline, &sign, DARK);
}

/// Whether a word space stands after `c`: in every script short of the CJK
/// blocks [`crate::wrap::classify`] sets solid.
fn spaced(c: char) -> bool {
    (c as u32) < 0x2E80
}

/// One highlight collapsed: `title` where `at.named`, `highlight.body` behind a
/// rule in `highlight.colour`, its notes under that and [`label`] with
/// `Highlight::day` below, the rule on `area`'s left and the words to its right.
pub(super) fn row(cx: &mut Ctx, area: Rect, held: &Row, at: &Layout) {
    let plan = plan(cx.text, cx.theme, at, held);
    planned(cx, area, held, at, &plan, Part::whole(&plan), false);
}

/// What stands over the list: the book it belongs to, its title cut to two
/// thirds of the strip and the pager taking the rest.
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

/// What a row's label reads: where the highlight falls and what kind it is —
/// `Location 608 | Highlight`; [`row`] sets the day at the line's other end.
pub fn label(highlight: &Highlight, extent: i64, s: &Strings) -> String {
    format!(
        "{}{BAR}{}",
        positions(highlight, extent, s),
        kind_name(highlight.kind, s)
    )
}

/// `highlight.page`, `highlight.location` and `highlight.through(extent)`, [`DOT`]-joined in
/// that order, each standing only where its own source stated it, none derived
/// from another; [`DASH`] where none is stated.
fn positions(highlight: &Highlight, extent: i64, s: &Strings) -> String {
    let mut said: Vec<String> = Vec::with_capacity(3);
    if !highlight.page.is_empty() {
        said.push(format!("{} {}", s.at_page, highlight.page));
    }
    if highlight.location >= 0 {
        said.push(format!("{} {}", s.at_location, highlight.location));
    }
    if let Some(through) = highlight.through(extent) {
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

/// What `kind` is called, `s.kind_other` where no arm names it.
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
    use crate::highlights::State;
    use crate::lang::Lang;
    use crate::ui::theme::tests::PANELS;

    fn en() -> &'static Strings {
        Lang::English.strings()
    }

    fn highlight(state: State, start: i64, location: i64) -> Highlight {
        Highlight {
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
    fn a_row_states_every_position_its_sources_named() {
        // `page` and `location` off the clipping, the 33% off `start`: 330 of 1000.
        let mut held = highlight(State::Live, 330, 111);
        held.page = "15".into();
        assert_eq!(
            positions(&held, 1_000, en()),
            "page 15 · Location 111 · 33%"
        );
    }

    #[test]
    fn a_position_no_source_named_is_never_derived_from_another() {
        // `highlight.location` is on another axis than `highlight.start`.
        let held = highlight(State::Unconfirmed, -1, 111);
        assert_eq!(positions(&held, 1_000, en()), "Location 111");
        // `location` -1, no clipping beside `start`: the fraction alone.
        let mut held = highlight(State::Live, 330, -1);
        assert_eq!(positions(&held, 1_000, en()), "33%");
        // `0` for the extent: nothing at all.
        held.location = -1;
        assert_eq!(positions(&held, 0, en()), DASH);
    }

    #[test]
    fn a_window_opens_on_the_line_before_the_one_that_holds_the_query() {
        let every: Vec<String> = ["a first line", "a second", "a third", "a fourth"]
            .iter()
            .map(|line| line.to_string())
            .collect();
        assert_eq!(opens_at(&every, "third"), 1);
        assert_eq!(opens_at(&every, "fourth"), 2);
        // `"first"` and `"SECOND"`, in the first two lines, open at the head.
        assert_eq!(opens_at(&every, "first"), 0);
        assert_eq!(opens_at(&every, "SECOND"), 0, "case folded");
        // `"nowhere"`, a note's match alone, leaves the passage at its head.
        assert_eq!(opens_at(&every, "nowhere"), 0);
    }

    #[test]
    fn a_query_is_found_in_a_line_wherever_it_stands_in_it() {
        let line = "The bigger the company";
        let runs = runs_in(line, "the");
        assert_eq!(runs, [(0, 3), (11, 14)]);
        assert_eq!(&line[runs[0].0..runs[0].1], "The");
        assert_eq!(&line[runs[1].0..runs[1].1], "the");
        // `"company"` at the end of the line reaches the end of it.
        assert_eq!(runs_in(line, "company"), [(15, 22)]);
        // `"zzz"`, which no line holds, and `""`.
        assert!(runs_in(line, "zzz").is_empty());
        assert!(runs_in(line, "").is_empty());
        // `runs_in` never overlaps: `"aa"` in `"aaaa"` twice.
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
        // `opens_at` opens on the line a fold found.
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
            // `deep * 2` keeps `WINDOW_LINES`.
            let deep = m.row(&theme, 1, WINDOW_LINES, 0);
            assert_eq!(window_lines(m, &theme, deep * 2), WINDOW_LINES, "{w}x{h}");
            // `deep * 2 - 1` gives up a line at a time, never the last one.
            assert!(
                window_lines(m, &theme, deep * 2 - 1) < WINDOW_LINES,
                "{w}x{h}"
            );
            assert_eq!(window_lines(m, &theme, 1), 1, "{w}x{h}");
            // `lines` stays inside `1..=WINDOW_LINES`.
            let lines = window_lines(m, &theme, deep + 10);
            assert!((1..=WINDOW_LINES).contains(&lines), "{w}x{h}");
        }
    }

    #[test]
    fn a_query_names_the_passages_of_one_book_and_no_other() {
        let stats = crate::stats::tests::highlighted_books();
        let record = stats.books[0].clone();
        let recent = Way::Down;
        // `None` and `""` list the whole book.
        assert_eq!(listing(&stats, 0, &record, recent, None).len(), 1);
        assert_eq!(listing(&stats, 0, &record, recent, Some("")).len(), 1);
        // `"SKY"` in the passage, `"borrowed"` in its note.
        assert_eq!(listing(&stats, 0, &record, recent, Some("SKY")).len(), 1);
        assert_eq!(
            listing(&stats, 0, &record, recent, Some("borrowed")).len(),
            1
        );
        // `"港"`, the other book's word, lists nothing here.
        assert!(listing(&stats, 0, &record, recent, Some("港")).is_empty());
        // `(book, at)` names where its own book's list opens.
        let held = listing(&stats, 0, &record, recent, Some("sky"));
        assert_eq!((held[0].book, held[0].at), (0, 0));
    }

    #[test]
    fn a_note_written_on_the_desktop_stands_apart_from_the_kindles_and_is_found() {
        use crate::written::{HighlightId, Laid};
        let mut stats = crate::stats::tests::highlighted_books();
        let laid = |at: &str, on: Option<HighlightId>| Laid {
            written: at.into(),
            on,
            gone: None,
        };
        // `under`, the book's note, laid as one written on the desktop on
        // `passage`; `alone`, one on the book alone.
        let passage = HighlightId::of(&stats.highlights[0]);
        let under = HighlightId::of(&stats.highlights[1]);
        stats
            .written
            .insert(under, laid("2026-01-02T10:05:00", Some(passage)));
        let alone = Highlight {
            extent: 1000,
            kind: Kind::Note,
            at: "2026-01-05T08:00:00".into(),
            body: "# A heading\n\nIts words".into(),
            ..Highlight::default()
        };
        stats
            .written
            .insert(HighlightId::of(&alone), laid("2026-01-05T08:00:00", None));
        stats.highlights.push(alone);
        stats.books[0].highlights.push(4);
        let record = stats.books[0].clone();
        let held = listing(&stats, 0, &record, Way::Down, None);
        assert_eq!(held.len(), 2);
        let (alone, passage) = (&held[0], &held[1]);
        assert!(alone.writes() && alone.laid.is_some() && alone.written.is_empty());
        assert!(passage.notes.is_empty(), "no Kindle note under it");
        assert_eq!(passage.written.len(), 1);
        assert!(passage.writes() && passage.laid.is_none());
        let found = listing(&stats, 0, &record, Way::Down, Some("borrowed"));
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].highlight.body, "the sky above the port");
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
                let deep = m.ruled(&theme, lines);
                // The first line's cap and the last line's baseline, both
                // where `row` sets them under the rule's head.
                let first = air(&theme);
                let last = m.last(&theme, lines);
                assert_eq!(first, deep - last, "{w}x{h}, {lines}");
                // `last + m.apart(&theme)`, the note's cap, stands past the rule's foot.
                assert!(last + m.apart(&theme) > deep, "{w}x{h}");
            }
        }
    }

    #[test]
    fn a_rows_parts_stand_apart_and_its_edges_hold_one_white() {
        for (w, h) in PANELS {
            let theme = Theme::for_screen(w, h);
            let Ok(mut text) = TextRenderer::load(theme.body_px) else {
                return;
            };
            let at = Layout::of(
                &mut text,
                &theme,
                Rect::new(0, 0, w as i32, h as i32),
                false,
                Script::of_language("en"),
            );
            let m = at.m;
            let note = |at: &str, body: &str| Highlight {
                extent: 1_000,
                kind: Kind::Note,
                at: at.into(),
                body: body.into(),
                ..Highlight::default()
            };
            // `passage` with two notes written on the desktop under it.
            let passage = Row {
                highlight: highlight(State::Live, 100, 10),
                written: vec![
                    note("2026-02-08T10:00:00", "The first."),
                    note("2026-02-12T10:00:00", "The second."),
                ],
                language: "en".into(),
                extent: 1_000,
                ..Row::default()
            };
            let out = plan(&mut text, &theme, &at, &passage);
            let (top, _) = out.rule.expect("a rule");
            let edge = |plan: &Plan, first: i32| {
                (
                    rule_over(&theme) + first,
                    plan.high - rule_over(&theme) - plan.label,
                )
            };
            assert_eq!(edge(&out, top), (pad(&theme), pad(&theme)), "{w}x{h}");
            let lines: Vec<i32> = out
                .sets
                .iter()
                .filter(|set| set.x == 0)
                .map(|set| set.baseline)
                .collect();
            let days: Vec<i32> = out.days.iter().map(|(day, _)| *day).collect();
            assert_eq!((lines.len(), days.len()), (2, 2), "{w}x{h}");
            // Each note's day a gap under it, the next part `apart` under that day.
            assert_eq!(
                lines[0] - m.body_cap - (top + m.last(&theme, 1)),
                m.apart(&theme)
            );
            assert_eq!(days[0] - m.cap - lines[0], theme.gap);
            assert_eq!(lines[1] - m.body_cap - days[0], m.apart(&theme));
            assert_eq!(out.label - m.cap - days[1], m.apart(&theme));
            assert!(m.apart(&theme) > m.line - m.body_cap + theme.gap, "{w}x{h}");
            // `alone`, a note on the book alone, holds the same white at both edges.
            let alone = Row {
                highlight: note("2026-01-25T10:00:00", "Halfway."),
                laid: Some(Laid {
                    written: "2026-01-25T10:00:00".into(),
                    on: None,
                    gone: None,
                }),
                language: "en".into(),
                ..Row::default()
            };
            let out = plan(&mut text, &theme, &at, &alone);
            let first = out.sets[0].baseline - m.body_cap;
            assert_eq!(edge(&out, first), (pad(&theme), pad(&theme)), "{w}x{h}");
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
            // `m.row` with one passage line stands shorter than with two.
            assert!(m.row(&theme, 0, 2, 0) > m.row(&theme, 0, 1, 0));
            assert_eq!(
                m.row(&theme, 0, 0, 0),
                m.row(&theme, 0, 1, 0),
                "never no lines"
            );
            // `m.row` with a note stands taller, and with its book's title
            // taller again.
            assert!(m.row(&theme, 0, 2, 2) > m.row(&theme, 0, 2, 0));
            assert!(m.row(&theme, 1, 2, 0) > m.row(&theme, 0, 2, 0));
            // `lines_in` gives a box of 1 one line, and counts every line a
            // taller box holds.
            assert_eq!(lines_in(m, &theme, 1), 1);
            let deep = lines_in(m, &theme, m.row(&theme, 0, 7, 0));
            assert_eq!(deep, 7, "{w}x{h}");
            assert!(m.row(&theme, 0, deep + 1, 0) > m.row(&theme, 0, 7, 0));
        }
    }

    /// A passage `words` words long, its book's in English.
    fn passage(words: usize) -> Row {
        Row {
            highlight: Highlight {
                body: vec!["word"; words].join(" "),
                ..highlight(State::Live, 100, 10)
            },
            language: "en".into(),
            extent: 1_000,
            ..Row::default()
        }
    }

    #[test]
    fn a_page_closes_on_the_row_it_cannot_hold_cut_to_the_room_left() {
        for (w, h) in PANELS {
            let theme = Theme::for_screen(w, h);
            let Ok(mut text) = TextRenderer::load(theme.body_px) else {
                return;
            };
            let area = Rect::new(0, 0, w as i32, h as i32 / 2);
            let at = Layout::of(&mut text, &theme, area, false, Script::of_language("en"))
                .found(&theme, area.h, "");
            let held: Vec<Row> = [3, 40, 12, 80, 5, 25, 60, 8, 30, 2, 90, 14]
                .into_iter()
                .map(passage)
                .collect();
            let pages = paged(&mut text, &theme, &held, &at);
            let rows: Vec<usize> = pages
                .iter()
                .flat_map(|page| page.pieces.iter().map(|piece| piece.row))
                .collect();
            assert_eq!(rows, (0..held.len()).collect::<Vec<_>>(), "{w}x{h}");
            for (n, page) in pages.iter().enumerate() {
                let high: i32 = page
                    .pieces
                    .iter()
                    .map(|piece| {
                        plan_in(&mut text, &theme, &at, &held[piece.row], piece.depth).high
                    })
                    .sum();
                assert!(high <= at.high, "{w}x{h}: page {n} stands {high}");
                // What a page leaves holds not one line of the row opening the next.
                let Some(next) = pages.get(n + 1).map(|page| page.pieces[0].row) else {
                    continue;
                };
                let collapsed = plan(&mut text, &theme, &at, &held[next]);
                let room = at.high - high;
                let cut = squeezed(&mut text, &theme, &at, &held[next], &collapsed, room);
                assert_eq!(cut, None, "{w}x{h}: page {n} leaves {room}");
            }
            // `long`, a passage past the fold, stands cut, and `opens` its own page.
            let long = &pages[0].pieces[1];
            assert!(long.opens && long.depth.lines < Depth::WHOLE.lines);
        }
    }

    #[test]
    fn an_entry_runs_over_pages_showing_each_line_once() {
        for (w, h) in PANELS {
            let theme = Theme::for_screen(w, h);
            let Ok(mut text) = TextRenderer::load(theme.body_px) else {
                return;
            };
            let area = Rect::new(0, 0, w as i32, h as i32 / 2);
            let at = Layout::of(&mut text, &theme, area, false, Script::of_language("en"))
                .found(&theme, area.h, "");
            let note = |at: &str| Highlight {
                extent: 1_000,
                kind: Kind::Note,
                at: at.into(),
                body: "# A heading\n\nA paragraph of its own.\n\n".repeat(12),
                ..Highlight::default()
            };
            let mut long = passage(300);
            long.written = vec![note("2026-02-08T10:00:00"), note("2026-02-12T10:00:00")];
            let whole = plan_in(&mut text, &theme, &at, &long, Depth::WHOLE);
            let parts = parts(&mut text, &theme, &whole, at.high);
            assert!(parts.len() > 2, "{w}x{h}");
            let lines = whole.lines(&mut text, &theme);
            for line in &lines {
                let shown = parts
                    .iter()
                    .filter(|part| part.shows(line.baseline))
                    .count();
                assert_eq!(shown, 1, "{w}x{h}: the line on {}", line.baseline);
            }
            for (k, part) in parts.iter().enumerate() {
                let shown: Vec<&Line> = lines
                    .iter()
                    .filter(|line| part.shows(line.baseline))
                    .collect();
                let (first, last) = (shown[0], shown[shown.len() - 1]);
                assert!(
                    last.baseline - part.from <= at.high - pad(&theme) - rule_over(&theme),
                    "{w}x{h}: part {k}"
                );
                if k > 0 {
                    assert_eq!(first.top - part.from, head(&theme), "{w}x{h}: part {k}");
                    assert!(!first.closes, "{w}x{h}: part {k} opens on a day");
                }
            }
        }
    }
}
