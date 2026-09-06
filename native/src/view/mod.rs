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
pub mod rhythm;

use crate::date;
use crate::eink::fb::Framebuffer;
use crate::lang::{Lang, Strings};
use crate::settings::WeekStart;
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
    /// Hand a book on the device back to the Kindle's reader, by its index in
    /// [`Stats::books`]. Leaves the app, as [`Hit::Exit`] does.
    Open(usize),
    /// Ask to set `BookRecord::finished` on a book, by its index in
    /// [`Stats::books`] and the value a tap would set. Puts the question up
    /// rather than answering it: [`Hit::Answer`] answers.
    Finished(usize, bool),
    /// Ask to read a book again, by its index in [`Stats::books`]. Puts the
    /// question up rather than answering it: [`Hit::Answer`] answers.
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
    /// The colours the charts are drawn in.
    ColorScheme(crate::settings::ColorScheme),
    /// Go looking for a newer release of this app.
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
    /// The order the Books screen lists in.
    Sorted(Sort),
}

/// A question standing over a book's own screen, which [`Hit::Answer`] carries
/// out and [`Hit::Dismiss`] takes down.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Ask {
    /// Give up the book's place and its mark, and hand it back at its start.
    Restart,
    /// Set `BookRecord::finished` to the value carried.
    Mark(bool),
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
    /// The two chips the row draws: every book, then the shelf this one
    /// narrows to.
    pub fn chips(self) -> [Shelf; 2] {
        match self {
            Shelf::Unfinished => [Shelf::All, Shelf::Unfinished],
            _ => [Shelf::All, Shelf::Finished],
        }
    }

    /// The shelf a tap on the second chip lands on.
    pub fn cycled(self) -> Shelf {
        match self {
            Shelf::All => Shelf::Finished,
            Shelf::Finished => Shelf::Unfinished,
            Shelf::Unfinished => Shelf::All,
        }
    }

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
    Longest,
    /// Furthest through first.
    Furthest,
}

impl Sort {
    pub const ALL: [Sort; 3] = [Sort::Recent, Sort::Longest, Sort::Furthest];

    /// What this order is called, in the interface's own language.
    pub fn label(self, lang: Lang) -> &'static str {
        let s = lang.strings();
        match self {
            Sort::Recent => s.by_recent,
            Sort::Longest => s.by_longest,
            Sort::Furthest => s.by_furthest,
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
            Span::Week => {
                let days = self.days(week);
                let (from, to) = (
                    date::short_day(*days.start(), s),
                    date::short_day(*days.end(), s),
                );
                format!("{from} – {to}")
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
    /// The question standing over the open book's screen, and the book it
    /// names.
    pub asked: Option<(usize, Ask)>,
    /// How far down the book list has been paged.
    pub books_from: usize,
    /// Which books the Books screen lists.
    pub shelf: Shelf,
    /// The stretch that list is narrowed to, which a span's own Finished
    /// figure opens it under.
    pub window: Option<Window>,
    /// The order it lists them in, which a tab change keeps.
    pub sort: Sort,
    /// Whether the day picked off the grid was asked to open as its own page.
    /// A span whose books are listed under its grid narrows that list to the
    /// picked day, and opens the day only on this.
    pub opened_day: bool,
    /// Which page of All Time is showing, of [`alltime::PAGES`].
    pub alltime_page: usize,
    /// How far down Rhythm's own book list has been paged.
    pub list_from: usize,
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
            books_from: 0,
            shelf: Shelf::default(),
            window: None,
            sort: Sort::default(),
            opened_day: false,
            alltime_page: 0,
            list_from: 0,
        }
    }

    /// Go to `tab`, closing any book, day or shelf open over it. Answers
    /// whether that moved anywhere: a tap on the tab showing, with nothing
    /// open over it, is no navigation and costs no redraw.
    pub fn go(&mut self, tab: Tab) -> bool {
        if self.tab == tab
            && self.book.is_none()
            && self.asked.is_none()
            && !self.picked
            && self.shelf == Shelf::All
            && self.window.is_none()
        {
            return false;
        }
        self.tab = tab;
        self.book = None;
        self.asked = None;
        self.picked = false;
        self.opened_day = false;
        self.shelf = Shelf::All;
        self.window = None;
        self.alltime_page = 0;
        self.list_from = 0;
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
    /// What the charts draw in, from `crate::ui::paint::Palette::for_panel`.
    pub palette: crate::ui::paint::Palette,
    pub stats: &'a Stats,
    /// The device's own local day, and the second of it.
    pub today: i64,
    pub now: i64,
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
        assert_eq!(named(Span::Week, en), "Aug 31 – Sep 6");
        assert_eq!(named(Span::Year, ja), "2026年");
        assert_eq!(named(Span::Month, ja), "2026年9月");
        // The stretch a window covers is the span's own.
        for span in Span::CALENDAR {
            let window = Window { span, day };
            assert_eq!(window.days(week), span.days(day, week));
            assert!(window.days(week).contains(&day));
        }
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
}
