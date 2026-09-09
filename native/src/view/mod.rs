//! The screens. Each takes the box left above the tab strip and draws into it,
//! recording a hit box for anything touchable. [`State`] holds the day, the
//! span and the open book; a redraw after a tap is the same call.

pub mod alltime;
pub mod band;
pub mod book;
pub mod books;
pub mod config;
pub mod daybooks;
pub mod home;
pub mod marks;
pub mod pager;
pub mod rhythm;
pub mod search;

use crate::date;
use crate::eink::fb::Framebuffer;
use crate::lang::{Lang, Strings};
use crate::settings::{Scope, WeekStart};
use crate::stats::Stats;
use crate::ui::chrome::Tab;
use crate::ui::cover::Covers;
use crate::ui::paint::Rect;
use crate::ui::text::TextRenderer;
use crate::ui::theme::Theme;

/// Something a touch lands on.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Hit {
    Tab(Tab),
    /// A day of the Rhythm grid, as a day count.
    Day(i64),
    /// A book, by its index in [`Stats::books`].
    Book(usize),
    /// One of the open book's own two tabs.
    BookTab(BookTab),
    /// Where the open book's list of marks opens, as an index into it.
    MarksPage(usize),
    /// A passage the search found: the book it was marked in, and the row's
    /// place in that book's own list of marks.
    Mark(usize, usize),
    /// Open a book through `open::uri`, by its index in [`Stats::books`].
    /// Leaves the app, as [`Hit::Exit`] does.
    Open(usize),
    /// Ask to set `BookRecord::finished` on a book, by its index in
    /// [`Stats::books`] and the value a tap sets. [`Hit::Answer`] answers it.
    Finished(usize, bool),
    /// Ask to read a book again, by its index in [`Stats::books`].
    /// [`Hit::Answer`] answers it.
    Restart(usize),
    /// Carry out the question [`State::asked`] holds. A restart leaves the app,
    /// as [`Hit::Open`] does.
    Answer,
    /// Take the question down, leaving the book as it stands.
    Dismiss,
    /// Leave the app. The only way out: a book is closed by tapping a tab.
    Exit,
    /// A chip on the config page.
    Language(Lang),
    WeekStart(WeekStart),
    TextSize(crate::settings::TextSize),
    /// Whether a total counts reading on books the catalog names none of.
    ShowUnnamed(bool),
    /// Whether a book no jacket can be drawn for is listed at all.
    ShowUncovered(bool),
    /// Ask before reading the logs, the catalog and the sidecars again, to
    /// name the books nothing on the record names yet.
    Retry,
    /// The answer to that question.
    Retried,
    /// Ask before reading every log again and re-measuring each sitting.
    Heal,
    /// The answer to that question.
    Healed,
    /// The colours the charts are drawn in.
    ColorScheme(crate::settings::ColorScheme),
    /// Where a book's own figures come from.
    Figures(crate::settings::Figures),
    /// Go looking for a newer release.
    Update,
    Prev,
    Next,
    /// One span of the Rhythm screen.
    Span(Span),
    /// The span of the width showing that holds today.
    Now,
    /// The day picked off the grid, opened as its own page.
    OpenDay,
    /// Where a book list opens, as an index into it. The screen drawing the
    /// list holds the index inside the list: a step past either end is no step.
    ListPage(usize),
    /// The books tab, narrowed to one shelf and one stretch of days.
    Shelved(Shelf, Option<Window>),
    /// Where the Books list opens, as an index into it.
    BooksPage(usize),
    /// Which page of the config screen is showing, counted from zero.
    ConfigPage(usize),
    /// The order the Books screen lists in.
    Sorted(Sort),
    /// Which list the search names, off the chips in its head row.
    Scoped(Scope),
    /// Open the search over the Books tab, with the keyboard up.
    Search,
    /// A tap on the search field, which raises the keyboard.
    SearchField,
    /// The mark in the field's right end: it takes the query off, and closes
    /// the search where there is none to take.
    SearchClear,
    /// Where the results open, as an index into them.
    SearchPage(usize),
    /// Ask what should become of one book's reading, by its index in
    /// [`Stats::books`]. Answered by [`Hit::ClearBook`] or [`Hit::ForgetBook`].
    Clear(usize),
    /// Take one book's reading, keeping its record.
    ClearBook(usize),
    /// Take one book's reading and its record together.
    ForgetBook(usize),
    /// Ask to empty the whole record. The flag is whether an archive is
    /// written first.
    Wipe(bool),
    /// Empty it, as [`Hit::Wipe`] asked.
    Wiped(bool),
    /// Ask to take an archive back, by its place in `backup::list`.
    Restore(usize),
    /// Take it back.
    Restored(usize),
    /// Ask to read every log again, and read them.
    Rebuild,
    Rebuilt,
}

/// A question standing over the config page, with the figures it states
/// gathered when it went up: nothing is walked while the dialog is drawn.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Confirm {
    pub about: About,
    /// What the question is about, counted when it went up.
    pub sittings: usize,
    pub books: usize,
    /// The archive to be written, or the space a wipe gives back.
    pub bytes: u64,
    /// The archive's name, where the question names one.
    pub named: String,
}

/// What a [`Confirm`] asks about: a reset, or one of the two passes over the
/// device's logs.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum About {
    Reset(Reset),
    /// Measure every sitting the logs reach again.
    Heal,
    /// Read every source of identity again.
    Retry,
}

/// Which reset a [`Confirm`] asks about.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Reset {
    /// Empty the record; the flag is whether an archive is kept.
    Wipe(bool),
    /// Take the archive at this place in `backup::list` back.
    Restore(usize),
    /// Read every log again.
    Rebuild,
}

impl Reset {
    /// The headline and the note a banner over this reset draws. The headline
    /// names the act: `reset_row`, `restore_do`, `rebuild_head`.
    pub fn doing(self, s: &Strings) -> (String, Vec<String>) {
        let (headline, said) = match self {
            Reset::Wipe(_) => (s.reset_row, s.wipe_doing),
            Reset::Restore(_) => (s.restore_do, s.restore_doing),
            Reset::Rebuild => (s.rebuild_head, s.rebuild_doing),
        };
        (headline.into(), vec![said.into()])
    }
}

/// What the banner over a retry is saying.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Retrying {
    /// Running, with every log to read before the books' own files.
    Logs,
    /// Running over those files alone, the logs holding no counter the record
    /// is missing.
    Files,
    /// Over, having named this many books.
    Named(usize),
}

impl Retrying {
    /// The headline naming the act, and what stands under it: the pass, with
    /// how long it runs where that is the long one, else what it came to.
    pub fn banner(self, s: &Strings) -> (&'static str, Vec<String>) {
        let said = match self {
            Retrying::Logs => vec![s.retry_logs.to_string(), s.retry_minutes.to_string()],
            Retrying::Files => vec![s.retry_files.to_string()],
            Retrying::Named(0) => vec![s.retry_none.to_string()],
            Retrying::Named(named) => vec![crate::lang::counted(s.retry_named, named as i64)],
        };
        (s.retry_head, said)
    }
}

/// The pass [`Hit::Heal`] runs, as its banner states it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Healing {
    /// Running, with every log left to read.
    Logs,
    /// Over, having moved the figures of this many stored sittings.
    Done(usize),
}

impl Healing {
    /// The headline naming the act, and what stands under it: the pass, with
    /// how long it runs, else what it came to.
    pub fn banner(self, s: &Strings) -> (&'static str, Vec<String>) {
        let said = match self {
            Healing::Logs => vec![s.heal_doing.to_string(), s.retry_minutes.to_string()],
            Healing::Done(0) => vec![s.heal_none.to_string()],
            Healing::Done(healed) => vec![crate::lang::counted(s.heal_done, healed as i64)],
        };
        (s.heal_head, said)
    }
}

/// A question standing over a book's own screen, which [`Hit::Answer`] carries
/// out and [`Hit::Dismiss`] takes down.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Ask {
    /// Give up the book's place and its mark, and hand it back at its start.
    Restart,
    /// Set `BookRecord::finished` to the value carried.
    Mark(bool),
    /// Put this book's reading back to zero, keeping the record or not. Its
    /// two answers are their own hits, being two different acts.
    Clear,
}

/// The two pages a book's own screen holds, as its picker states them.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum BookTab {
    /// What the reading came to: the figures and the journey.
    #[default]
    Statistics,
    /// The book's marked passages, most recent first.
    Marks,
}

impl BookTab {
    pub const ALL: [BookTab; 2] = [BookTab::Statistics, BookTab::Marks];

    /// What this page is called, in the interface's own language.
    pub fn label(self, lang: Lang) -> &'static str {
        let s = lang.strings();
        match self {
            BookTab::Statistics => s.statistics,
            BookTab::Marks => s.marks_tab,
        }
    }
}

/// The search standing over the Books tab.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Search {
    pub query: String,
    /// What an IME is composing, drawn after `query` and matched on by
    /// nothing.
    pub preedit: String,
    pub from: usize,
    /// Whether the on-screen keyboard stands over the foot of the screen.
    pub keyboard: bool,
}

impl Search {
    /// Takes `said` onto `query`, and puts `from` back to the head.
    pub fn typed(&mut self, said: char) {
        self.query.push(said);
        self.from = 0;
    }

    /// Takes the last character off `query`, answering whether there was one,
    /// and puts `from` back to the head.
    pub fn backspace(&mut self) -> bool {
        self.from = 0;
        self.query.pop().is_some()
    }

    /// Takes `said` onto `query`, clearing `preedit`.
    pub fn commit(&mut self, said: &str) {
        self.preedit.clear();
        self.query.push_str(said);
        self.from = 0;
    }

    /// Takes `count` characters off the end of `query`.
    pub fn delete(&mut self, count: usize) {
        for _ in 0..count {
            self.query.pop();
        }
        self.from = 0;
    }

    /// `keyboardCommit` carries the text; `keyboardSetPreeditString`
    /// `position:str`; `keyboardDelete` `before:after`; `keyboardReplace`
    /// `before:after:str`.
    pub fn set(&mut self, property: &str, value: &str) -> bool {
        match property {
            "keyboardCommit" => self.commit(value),
            "keyboardSetPreeditString" => {
                let (_, said) = value.split_once(':').unwrap_or(("", value));
                said.clone_into(&mut self.preedit);
            }
            "keyboardDelete" => {
                let (before, _) = value.split_once(':').unwrap_or((value, ""));
                self.delete(before.parse().unwrap_or(0));
            }
            "keyboardReplace" => {
                let mut parts = value.splitn(3, ':');
                let before = parts.next().unwrap_or_default().parse().unwrap_or(0);
                let said = parts.nth(1).unwrap_or_default().to_string();
                self.delete(before);
                self.commit(&said);
            }
            _ => return false,
        }
        true
    }
}

/// Which books the Books screen lists.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Shelf {
    #[default]
    All,
    /// Books the catalog states read through.
    Finished,
    /// Books the catalog states short of read through.
    Unfinished,
}

impl Shelf {
    /// What this shelf is called, in the interface's own language.
    pub fn label(self, lang: Lang) -> &'static str {
        let s = lang.strings();
        match self {
            Shelf::All => s.shelf_every,
            Shelf::Finished => s.shelf_finished,
            Shelf::Unfinished => s.shelf_unfinished,
        }
    }
}

/// The order the Books screen lists in.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Sort {
    /// Last put down first.
    #[default]
    Recent,
    /// Most time read first.
    Time,
    /// Furthest through first.
    Progress,
}

impl Sort {
    pub const ALL: [Sort; 3] = [Sort::Recent, Sort::Time, Sort::Progress];

    /// What this order is called, in the interface's own language.
    pub fn label(self, lang: Lang) -> &'static str {
        let s = lang.strings();
        match self {
            Sort::Recent => s.by_recent,
            Sort::Time => s.by_time,
            Sort::Progress => s.by_progress,
        }
    }

    /// The order the chip stating this one opens.
    pub fn next(self) -> Sort {
        let at = Sort::ALL.iter().position(|o| *o == self).unwrap_or(0);
        Sort::ALL[(at + 1) % Sort::ALL.len()]
    }
}

/// How wide a stretch of days the Rhythm screen draws around the one showing.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Span {
    /// Everything the record holds, which a board of figures states.
    AllTime,
    Year,
    Month,
    Week,
}

impl Span {
    pub const ALL: [Span; 4] = [Span::AllTime, Span::Year, Span::Month, Span::Week];

    /// The spans drawn as a grid of days over the page.
    pub const CALENDAR: [Span; 3] = [Span::Year, Span::Month, Span::Week];

    /// What this span is called, in the interface's own language.
    pub fn label(self, lang: Lang) -> &'static str {
        let s = lang.strings();
        match self {
            Span::AllTime => s.all_time,
            Span::Week => s.week,
            Span::Month => s.month,
            Span::Year => s.year,
        }
    }

    /// The days this span covers, `day` among them.
    pub fn days(self, day: i64, week: WeekStart) -> std::ops::RangeInclusive<i64> {
        let (year, month, _) = date::civil_from_days(day);
        match self {
            // Every day the store can hold: the epoch opens it.
            Span::AllTime => 0..=day,
            Span::Week => {
                let first = day - week.column_of(date::weekday(day)) as i64;
                first..=first + 6
            }
            Span::Month => {
                let first = date::days_from_civil(year, month, 1);
                first..=first + date::days_in_month(year, month) - 1
            }
            Span::Year => date::days_from_civil(year, 1, 1)..=date::days_from_civil(year, 12, 31),
        }
    }

    /// `day` moved `by` spans.
    pub fn step(self, day: i64, by: i64) -> i64 {
        match self {
            // The whole record has nothing on either side of it.
            Span::AllTime => day,
            Span::Week => day + by * 7,
            Span::Month => date::shift_months(day, by),
            Span::Year => date::shift_months(day, by * 12),
        }
    }

    /// What the span holding `day` is called.
    pub fn name(self, day: i64, week: WeekStart, s: &Strings) -> String {
        let (year, month, _) = date::civil_from_days(day);
        match self {
            Span::AllTime => s.all_time.to_string(),
            Span::Week => {
                let days = self.days(day, week);
                let (of, no) = date::week_of_year(day, week);
                let (from, to) = (
                    date::short_day(*days.start(), s),
                    date::short_day(*days.end(), s),
                );
                let numbered = format!("{}{no}{}", s.week_no, s.week_no_after);
                // Two dates alone name no year, and a week in a record of
                // several is read by its number as often as by its dates.
                match s.date_ymd {
                    true => format!("{of}年 {from} – {to} · {numbered}"),
                    false => format!("{from} – {to}, {of} · {numbered}"),
                }
            }
            Span::Month => date::month_name(year, month, s),
            Span::Year => match s.date_ymd {
                true => format!("{year}年"),
                false => year.to_string(),
            },
        }
    }
}

/// The stretch a book list is narrowed to: a span, and a day inside it. A book
/// belongs to the window where the day it was last put down falls in
/// [`Self::days`], which is the rule `Stats::finished_over` counts by.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Window {
    pub span: Span,
    pub day: i64,
}

impl Window {
    /// The days this window covers.
    pub fn days(self, week: WeekStart) -> std::ops::RangeInclusive<i64> {
        self.span.days(self.day, week)
    }

    /// What the chip stating this window reads, which is short enough for a
    /// chip and names the year wherever the stretch is smaller than one.
    pub fn name(self, week: WeekStart, s: &Strings) -> String {
        let (year, month, _) = date::civil_from_days(self.day);
        let at = (month - 1).clamp(0, 11) as usize;
        match self.span {
            Span::AllTime => s.all_time.to_string(),
            Span::Year => match s.date_ymd {
                true => format!("{year}年"),
                false => year.to_string(),
            },
            Span::Month => match s.date_ymd {
                true => format!("{year}年{}", s.months_short[at]),
                false => format!("{} {year}", s.months_short[at]),
            },
            // A week is named by its number, not by the two dates it runs
            // between: those are long, name no year, and read alike.
            Span::Week => {
                let (of, no) = date::week_of_year(self.day, week);
                let numbered = format!("{}{no}{}", s.week_no, s.week_no_after);
                match s.date_ymd {
                    true => format!("{of}年{numbered}"),
                    false => format!("{of} {numbered}"),
                }
            }
        }
    }
}

/// What the screens are drawn at: the tab, the day, the span, the open book.
#[derive(Debug, Clone, PartialEq)]
pub struct State {
    pub tab: Tab,
    /// The day Rhythm looks at. `span` holds it, and the grid draws that span.
    pub day: i64,
    pub span: Span,
    /// Whether `day` is picked off the grid, which a month and the board open whole.
    pub picked: bool,
    /// The book whose own screen is open, over whichever tab opened it.
    pub book: Option<usize>,
    /// The question over the open book's screen, and the book it names.
    pub asked: Option<(usize, Ask)>,
    /// The question over the config page, with the figures it states.
    pub confirm: Option<Confirm>,
    /// Which page of the config screen is showing.
    pub config_page: usize,
    /// How far down the book list has been paged.
    pub books_from: usize,
    /// The search open over the Books tab.
    pub search: Option<Search>,
    /// The search open over the book on screen, held apart from `search`.
    pub book_search: Option<Search>,
    /// Which books the Books screen lists.
    pub shelf: Shelf,
    /// The stretch that list is narrowed to.
    pub window: Option<Window>,
    /// The order it lists them in, which a tab change keeps.
    pub sort: Sort,
    /// Which list the search names, which a tab change keeps as well.
    /// `Settings::scope` holds it between launches.
    pub scope: Scope,
    /// Whether the day picked off the grid opens as its own page.
    pub opened_day: bool,
    /// Which page of All Time is showing, of [`alltime::PAGES`].
    pub alltime_page: usize,
    /// How far down Rhythm's own book list has been paged.
    pub list_from: usize,
    /// Which of the open book's two pages is showing.
    pub book_tab: BookTab,
    /// How far down that book's list of marks has been paged.
    pub marks_from: usize,
}

impl State {
    pub fn new(today: i64) -> Self {
        Self {
            tab: Tab::Home,
            day: today,
            span: Span::AllTime,
            picked: false,
            book: None,
            asked: None,
            confirm: None,
            config_page: 0,
            books_from: 0,
            search: None,
            book_search: None,
            shelf: Shelf::default(),
            window: None,
            sort: Sort::default(),
            scope: Scope::default(),
            opened_day: false,
            alltime_page: 0,
            list_from: 0,
            book_tab: BookTab::default(),
            marks_from: 0,
        }
    }

    /// Open `book`'s own screen, at the page a book always opens on, with
    /// `Search::keyboard` cleared.
    pub fn open_book(&mut self, book: usize) {
        self.book = Some(book);
        self.book_tab = BookTab::default();
        self.marks_from = 0;
        self.book_search = None;
        if let Some(search) = self.search.as_mut() {
            search.keyboard = false;
        }
    }

    /// Where the search in play is held: the open book's own where a book is
    /// open, and the Books tab's where none is.
    pub fn searching_slot(&mut self) -> &mut Option<Search> {
        match self.book {
            Some(_) => &mut self.book_search,
            None => &mut self.search,
        }
    }

    /// The search in play, where one stands.
    pub fn searching(&self) -> Option<&Search> {
        match self.book {
            Some(_) => self.book_search.as_ref(),
            None => self.search.as_ref(),
        }
    }

    /// The same search, to type into.
    pub fn searching_mut(&mut self) -> Option<&mut Search> {
        self.searching_slot().as_mut()
    }

    /// Open `book` at the passage standing `at` in its own list of marks,
    /// which is where a search result leads.
    pub fn open_mark(&mut self, book: usize, at: usize) {
        self.open_book(book);
        self.book_tab = BookTab::Marks;
        self.marks_from = at;
    }

    /// Show `tab` of the open book. Answers whether that moved anywhere.
    pub fn go_in_book(&mut self, tab: BookTab) -> bool {
        if self.book_tab == tab {
            return false;
        }
        self.book_tab = tab;
        self.marks_from = 0;
        true
    }

    /// Go to `tab`, closing any book, day or shelf open over it. Answers
    /// whether that moved anywhere: a tap on the tab showing, with nothing
    /// open over it, is no navigation and costs no redraw.
    pub fn go(&mut self, tab: Tab) -> bool {
        if self.tab == tab
            && self.book.is_none()
            && self.asked.is_none()
            && self.confirm.is_none()
            && !self.picked
            && self.search.is_none()
            && self.shelf == Shelf::All
            && self.window.is_none()
        {
            return false;
        }
        self.tab = tab;
        self.book = None;
        self.asked = None;
        self.confirm = None;
        self.picked = false;
        self.opened_day = false;
        self.search = None;
        self.book_search = None;
        self.shelf = Shelf::All;
        self.window = None;
        self.alltime_page = 0;
        self.list_from = 0;
        self.book_tab = BookTab::default();
        self.marks_from = 0;
        true
    }

    /// Step Rhythm on: a day at a time where one is open, a page of All Time
    /// where that is the span, else a whole span, answering whether anything
    /// moved. All Time has no span either side; its pages are what step.
    pub fn shift(&mut self, by: i64) -> bool {
        if self.picked {
            self.day += by;
            return true;
        }
        if self.span == Span::AllTime {
            let last = alltime::PAGES as i64 - 1;
            let page = (self.alltime_page as i64 + by).clamp(0, last) as usize;
            let moved = page != self.alltime_page;
            self.alltime_page = page;
            return moved;
        }
        let was = self.day;
        self.day = self.span.step(self.day, by);
        self.day != was
    }
}

/// `read` with every book no jacket can be drawn for left out, where the
/// config page hides them. Nothing is taken off a total: the reading stands,
/// and only the row naming it goes.
pub fn covered(stats: &Stats, uncovered: bool, read: &mut Vec<(usize, i64)>) {
    if !uncovered {
        read.retain(|(book, _)| stats.books[*book].has_cover());
    }
}

/// `s.n_highlights` and `s.n_notes` counted and joined by ` · `, each dropped
/// where its count is zero. Empty where both are.
pub fn marks_said(s: &Strings, (marked, notes): (usize, usize)) -> String {
    let said = [(marked, s.n_highlights), (notes, s.n_notes)];
    said.iter()
        .filter(|(n, _)| *n > 0)
        .map(|(n, one)| crate::lang::counted(one, *n as i64))
        .collect::<Vec<String>>()
        .join(" · ")
}

/// `author` at the left of `area` and `marked` at its right, both set at
/// [`Theme::small_px`]. `author` is cut to the width `marked` leaves.
pub fn under_title(
    cx: &mut Ctx,
    area: Rect,
    baseline: i32,
    script: crate::font::Script,
    author: &str,
    marked: &str,
) {
    let theme = cx.theme;
    cx.text.set_px(theme.small_px);
    let counts = match marked.is_empty() {
        true => 0,
        false => cx.text.measure_width(marked) as i32,
    };
    let room = (area.w - counts - theme.gap * 2).max(1);
    let said = cx.text.wrap_and_clamp_in(script, author, room as u32, 1);
    cx.text.draw_in(
        script,
        cx.fb,
        area.x,
        baseline,
        said.first().map(String::as_str).unwrap_or_default(),
        false,
    );
    if counts > 0 {
        cx.text
            .draw(cx.fb, area.right() - counts, baseline, marked, false);
    }
}

/// `area` floored a gap clear of the keyboard's top edge.
pub(crate) fn over_keyboard(theme: &Theme, area: Rect) -> Rect {
    let over = theme.screen.bottom() - crate::keyboard::height(theme.screen.h);
    Rect::new(area.x, area.y, area.w, (over - theme.gap - area.y).max(0))
}

/// The index the last page of `count` rows opens at, `deep` rows to a page.
/// The pages tile the list, and the last one is the short one.
pub fn last_page_at(count: usize, deep: usize) -> usize {
    let deep = deep.max(1);
    count.saturating_sub(1) / deep * deep
}

/// What a screen draws with.
pub struct Ctx<'a> {
    pub fb: &'a mut Framebuffer,
    pub text: &'a mut TextRenderer,
    pub covers: &'a mut Covers,
    pub theme: &'a Theme,
    pub lang: Lang,
    pub week: WeekStart,
    /// Where a book's own figures come from.
    pub figures: crate::settings::Figures,
    /// Whether a book no jacket can be drawn for is listed at all.
    pub uncovered: bool,
    /// What the charts draw in, from `crate::ui::paint::Palette::for_panel`.
    pub palette: crate::ui::paint::Palette,
    pub stats: &'a Stats,
    /// The local day, and the second of it.
    pub today: i64,
    pub now: i64,
    /// Whether the record stands on a floor. The one thing a screen asks it:
    /// an empty list is a first run or a reset, and the two read differently.
    pub floored: bool,
    pub hits: Vec<(Hit, Rect)>,
}

impl Ctx<'_> {
    pub fn hit(&mut self, what: Hit, area: Rect) {
        self.hits.push((what, area));
    }

    /// The words this screen is written in.
    pub fn s(&self) -> &'static Strings {
        self.lang.strings()
    }

    /// The convention this screen's own labels are set in — a book's title
    /// keeps the convention its catalog entry names, whatever this is.
    pub fn ui_script(&self) -> crate::font::Script {
        crate::font::Script::of_language(self.lang.language_tag())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A day the assertions below are written against: Thursday 3 September
    /// 2026, in a month of thirty days.
    fn third() -> i64 {
        date::days_from_civil(2026, 9, 3)
    }

    #[test]
    fn a_span_covers_the_days_around_the_one_showing() {
        let day = third();
        let week = Span::Week.days(day, WeekStart::Monday);
        assert_eq!(date::civil_from_days(*week.start()), (2026, 8, 31));
        assert_eq!(date::civil_from_days(*week.end()), (2026, 9, 6));
        // A Sunday-first week holds the same day, a column over.
        let sunday = Span::Week.days(day, WeekStart::Sunday);
        assert_eq!(date::civil_from_days(*sunday.start()), (2026, 8, 30));
        assert!(sunday.contains(&day));

        let month = Span::Month.days(day, WeekStart::Monday);
        assert_eq!(date::civil_from_days(*month.start()), (2026, 9, 1));
        assert_eq!(date::civil_from_days(*month.end()), (2026, 9, 30));

        let year = Span::Year.days(day, WeekStart::Monday);
        assert_eq!(date::civil_from_days(*year.start()), (2026, 1, 1));
        assert_eq!(date::civil_from_days(*year.end()), (2026, 12, 31));
        assert_eq!(year.count(), 365);
    }

    #[test]
    fn a_step_moves_by_the_span_showing_and_comes_back() {
        for (span, count) in [(Span::Week, 7), (Span::Month, 12), (Span::Year, 3)] {
            let mut day = third();
            for _ in 0..count {
                day = span.step(day, 1);
            }
            for _ in 0..count {
                day = span.step(day, -1);
            }
            assert_eq!(day, third(), "{span:?} lost its place");
        }
        assert_eq!(
            Span::Week.step(third(), -1),
            date::days_from_civil(2026, 8, 27)
        );
        assert_eq!(
            Span::Month.step(third(), 4),
            date::days_from_civil(2027, 1, 3)
        );
        assert_eq!(
            Span::Year.step(third(), -2),
            date::days_from_civil(2024, 9, 3)
        );
    }

    #[test]
    fn a_step_leaves_the_day_inside_the_span_it_names() {
        // Every step lands on a day whose own span holds it, month ends
        // included: 31 March steps back to 28 February, not into March.
        for span in Span::ALL {
            let mut day = date::days_from_civil(2026, 3, 31);
            for _ in 0..30 {
                day = span.step(day, -1);
                assert!(span.days(day, WeekStart::Monday).contains(&day), "{span:?}");
            }
        }
        assert_eq!(
            Span::Month.step(date::days_from_civil(2026, 3, 31), -1),
            date::days_from_civil(2026, 2, 28)
        );
    }

    #[test]
    fn a_tab_tap_lands_on_that_tab() {
        let mut s = State::new(third());
        assert!(s.go(Tab::Books));
        assert_eq!(s.tab, Tab::Books);
        assert!(s.go(Tab::Rhythm));
        assert_eq!(s.tab, Tab::Rhythm);
    }

    #[test]
    fn a_book_opens_on_its_statistics_and_steps_onto_its_marks() {
        // The book screen is one track: the statistics page, then each page of
        // its marks. Stepping is what walks it, and stepping off either end
        // does nothing — the screen is left by a tab.
        let mut s = State::new(third());
        s.open_book(3);
        assert_eq!(s.book_tab, BookTab::Statistics);
        assert_eq!(s.marks_from, 0);

        assert!(s.go_in_book(BookTab::Marks));
        assert!(!s.go_in_book(BookTab::Marks), "already there");
        s.marks_from = 5;
        // Coming back to the statistics opens the marks at their head again.
        assert!(s.go_in_book(BookTab::Statistics));
        assert_eq!(s.marks_from, 0);

        // And another book opens on its own statistics, whatever the last one
        // was left showing.
        s.go_in_book(BookTab::Marks);
        s.marks_from = 5;
        s.open_book(4);
        assert_eq!(s.book_tab, BookTab::Statistics);
        assert_eq!(s.marks_from, 0);
    }

    #[test]
    fn a_tab_change_keeps_the_order_and_the_scope_and_takes_the_search_off() {
        let mut s = State::new(third());
        s.sort = Sort::Time;
        s.scope = Scope::Marks;
        s.search = Some(Search::default());
        assert!(s.go(Tab::Rhythm));
        assert!(s.search.is_none(), "the search is over the Books tab");
        assert_eq!(s.sort, Sort::Time, "the order is the reader's own answer");
        assert_eq!(s.scope, Scope::Marks, "and so is the scope");
    }

    #[test]
    fn a_passage_the_search_found_opens_its_book_at_that_passage() {
        let mut s = State::new(third());
        s.search = Some(Search {
            query: "port".into(),
            keyboard: true,
            ..Search::default()
        });
        s.open_mark(4, 7);
        assert_eq!(s.book, Some(4));
        assert_eq!(s.book_tab, BookTab::Marks);
        assert_eq!(s.marks_from, 7, "the book opens on the passage tapped");
        // The keyboard comes down under the book, as it does for any book
        // opened out of the results.
        assert_eq!(s.search.map(|search| search.keyboard), Some(false));
    }

    #[test]
    fn a_tab_tap_closes_the_book_open_over_it() {
        // The only way out of a book. Tapping the tab it was opened from
        // returns to that tab's screen and stays there.
        let mut s = State::new(third());
        s.go(Tab::Books);
        s.book = Some(3);
        assert!(s.go(Tab::Books), "the tab under a book still navigates");
        assert_eq!(s.tab, Tab::Books);
        assert!(s.book.is_none());

        // A book opened from Today is closed by any tab, landing on that one.
        s.book = Some(3);
        assert!(s.go(Tab::Rhythm));
        assert_eq!(s.tab, Tab::Rhythm);
        assert!(s.book.is_none());
    }

    #[test]
    fn the_tab_already_showing_is_not_a_navigation() {
        let mut s = State::new(third());
        assert_eq!(s.tab, Tab::Home);
        assert!(
            !s.go(Tab::Home),
            "a redraw is owed only where something moved"
        );
        assert_eq!(s.tab, Tab::Home);
    }

    #[test]
    fn a_state_opens_on_the_day_the_device_is_in() {
        let s = State::new(third());
        assert_eq!(s.day, third());
        assert_eq!(s.span, Span::AllTime);
        assert_eq!(s.tab, Tab::Home);
        assert!(!s.picked);
        assert!(s.book.is_none());
    }

    #[test]
    fn a_tab_tap_closes_the_day_open_over_the_calendar() {
        let mut s = State::new(third());
        s.go(Tab::Rhythm);
        s.picked = true;
        assert!(s.go(Tab::Rhythm), "the tab under a day still navigates");
        assert!(!s.picked);
        assert!(!s.go(Tab::Rhythm), "and the calendar itself stays put");
    }

    #[test]
    fn all_time_steps_through_its_pages_and_stops_at_either_end() {
        let mut s = State::new(third());
        assert_eq!(s.span, Span::AllTime);
        assert_eq!(s.alltime_page, 0);
        assert!(!s.shift(-1), "the first page has nothing before it");
        assert_eq!(s.alltime_page, 0);

        for page in 1..alltime::PAGES {
            assert!(s.shift(1), "page {page} is a step");
            assert_eq!(s.alltime_page, page);
            assert_eq!(s.day, third(), "paging never moves the day");
        }
        assert!(!s.shift(1), "the last page has nothing after it");
        assert_eq!(s.alltime_page, alltime::PAGES - 1);

        assert!(s.shift(-1));
        assert_eq!(s.alltime_page, alltime::PAGES - 2);
    }

    #[test]
    fn leaving_all_time_comes_back_to_its_first_page() {
        let mut s = State::new(third());
        s.shift(1);
        assert_ne!(s.alltime_page, 0);
        s.go(Tab::Books);
        s.go(Tab::Rhythm);
        assert_eq!(s.alltime_page, 0, "a tab tap opens the board again");
    }

    #[test]
    fn a_calendar_span_steps_a_span_and_leaves_the_page_alone() {
        let mut s = State::new(third());
        s.span = Span::Month;
        assert!(s.shift(1));
        assert_eq!(s.day, date::days_from_civil(2026, 10, 3));
        assert_eq!(s.alltime_page, 0);
    }

    #[test]
    fn a_picked_day_steps_a_day_at_a_time() {
        let mut s = State::new(third());
        s.shift(1);
        assert_eq!(s.day, third(), "paging All Time moves no day");
        s.span = Span::Month;
        s.shift(1);
        assert_eq!(
            s.day,
            date::days_from_civil(2026, 10, 3),
            "a month at a time"
        );
        s.day = third();
        s.picked = true;
        s.shift(1);
        assert_eq!(s.day, third() + 1);
        s.shift(-3);
        assert_eq!(s.day, third() - 2);
    }

    #[test]
    fn a_window_names_its_stretch_and_the_year_it_falls_in() {
        let day = third();
        let (en, ja) = (Lang::English.strings(), Lang::Japanese.strings());
        let week = WeekStart::Monday;
        let named = |span, s| Window { span, day }.name(week, s);
        assert_eq!(named(Span::Year, en), "2026");
        assert_eq!(named(Span::Month, en), "Sep 2026");
        assert_eq!(named(Span::Week, en), "2026 W36");
        assert_eq!(named(Span::Year, ja), "2026年");
        assert_eq!(named(Span::Month, ja), "2026年9月");
        assert_eq!(named(Span::Week, ja), "2026年第36週");
        // A week is numbered off the year holding most of it: the last days
        // of December are named by the year they run into.
        let turn = Window {
            span: Span::Week,
            day: date::days_from_civil(2025, 12, 30),
        };
        assert_eq!(turn.name(week, en), "2026 W1");
        // The stretch a window covers is the span's own.
        for span in Span::CALENDAR {
            let window = Window { span, day };
            assert_eq!(window.days(week), span.days(day, week));
            assert!(window.days(week).contains(&day));
        }
    }

    #[test]
    fn a_tab_tap_takes_down_the_question_standing_over_the_page() {
        let mut s = State::new(third());
        s.go(Tab::Config);
        s.confirm = Some(Confirm {
            about: About::Reset(Reset::Wipe(true)),
            sittings: 12,
            books: 3,
            bytes: 0,
            named: "readinglog-260906-010231.zip".into(),
        });
        assert!(
            s.go(Tab::Config),
            "the tab under a question still navigates"
        );
        assert!(s.confirm.is_none());
        assert!(!s.go(Tab::Config), "and the bare page stays put");
    }

    #[test]
    fn a_tab_tap_drops_the_window_the_list_was_under() {
        let mut s = State::new(third());
        s.go(Tab::Books);
        s.shelf = Shelf::Finished;
        s.window = Some(Window {
            span: Span::Year,
            day: third(),
        });
        assert!(s.go(Tab::Books), "the tab under a window still navigates");
        assert_eq!(s.shelf, Shelf::All);
        assert!(s.window.is_none());
        assert!(!s.go(Tab::Books), "and the whole shelf stays put");
    }

    #[test]
    fn a_commit_takes_the_preedit_off_and_the_text_on() {
        let mut search = Search::default();
        assert!(search.set("keyboardSetPreeditString", "8:youzheng"));
        assert_eq!(search.preedit, "youzheng");
        assert!(search.set("keyboardCommit", "夢遊"));
        assert_eq!(search.query, "夢遊");
        assert!(search.preedit.is_empty());
    }

    #[test]
    fn a_preedit_carries_its_cursor_before_the_first_colon() {
        let mut search = Search::default();
        search.set("keyboardSetPreeditString", "2:a:b");
        assert_eq!(search.preedit, "a:b");
        search.set("keyboardSetPreeditString", "0:");
        assert!(search.preedit.is_empty());
        search.set("keyboardSetPreeditString", "");
        assert!(search.preedit.is_empty());
    }

    /// A delete counts characters, never bytes.
    #[test]
    fn a_delete_counts_characters() {
        let mut search = Search::default();
        search.set("keyboardCommit", "夢遊症");
        assert!(search.set("keyboardDelete", "2:0"));
        assert_eq!(search.query, "夢");
    }

    /// A replace deletes and inserts in one, and its text may hold colons.
    #[test]
    fn a_replace_deletes_then_inserts() {
        let mut search = Search::default();
        search.set("keyboardCommit", "abc");
        assert!(search.set("keyboardReplace", "2:0:XY:Z"));
        assert_eq!(search.query, "aXY:Z");
    }

    /// Every set opens the results at their head, and a property this screen
    /// does not answer moves nothing.
    #[test]
    fn a_property_this_screen_does_not_answer_moves_nothing() {
        let mut search = Search {
            from: 8,
            ..Search::default()
        };
        assert!(!search.set("keyboardGetSurround", ""));
        assert_eq!(search.from, 8);
        assert!(search.set("keyboardCommit", "a"));
        assert_eq!(search.from, 0);
    }
}
