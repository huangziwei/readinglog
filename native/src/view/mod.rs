//! The screens.
//!
//! Each takes the box left above the tab strip and draws into it, recording a
//! hit box for anything the reader can touch. A screen
//! holds no state of its own: what month is showing, which book is open and
//! which day is selected all live in [`State`], so a redraw after a tap is the
//! same call with a different state.

pub mod book;
pub mod books;
pub mod calendar;
pub mod clock;
pub mod config;
pub mod home;

use crate::eink::fb::Framebuffer;
use crate::lang::{Lang, Strings};
use crate::stats::Stats;
use crate::ui::chrome::Tab;
use crate::ui::cover::Covers;
use crate::ui::paint::Rect;
use crate::ui::text::TextRenderer;
use crate::ui::theme::Theme;

/// Something the reader can touch.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Hit {
    Tab(Tab),
    /// A day of the calendar, as a day count.
    Day(i64),
    /// A book, by its index in [`Stats::books`].
    Book(usize),
    /// Leave the app. The only way out: a book is closed by tapping a tab.
    Exit,
    /// A chip on the config page.
    Language(Lang),
    WeekStart(crate::settings::WeekStart),
    TextSize(crate::settings::TextSize),
    Prev,
    Next,
    /// One cut of the clock.
    Cut(Cut),
}

/// Which way the clock screen slices the same total.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Cut {
    Hour,
    Weekday,
    Month,
}

impl Cut {
    pub const ALL: [Cut; 3] = [Cut::Hour, Cut::Weekday, Cut::Month];

    /// What this cut is called, in the interface's own language.
    pub fn label(self, lang: Lang) -> &'static str {
        let s = lang.strings();
        match self {
            Cut::Hour => s.hour_of_day,
            Cut::Weekday => s.weekday,
            Cut::Month => s.month,
        }
    }
}

/// Where the reader has navigated to.
#[derive(Debug, Clone, PartialEq)]
pub struct State {
    pub tab: Tab,
    /// The month the calendar is showing, as `(year, month)`.
    pub month: (i64, i64),
    /// The day a tap selected, which the calendar lists below the grid.
    pub day: Option<i64>,
    /// The book whose own screen is open, over whichever tab opened it.
    pub book: Option<usize>,
    pub cut: Cut,
    /// How far down the book list has been paged.
    pub books_from: usize,
}

impl State {
    pub fn new(today: i64) -> Self {
        let (year, month, _) = crate::date::civil_from_days(today);
        Self {
            tab: Tab::Home,
            month: (year, month),
            day: None,
            book: None,
            cut: Cut::Hour,
            books_from: 0,
        }
    }

    /// Go to `tab`, closing any book open over it. Answers whether that moved
    /// anywhere: a tap on the tab already showing, with no book over it, is
    /// not a navigation and costs no redraw.
    ///
    /// This is the only way out of a book — there is no back control, and the
    /// tab a book was opened from stays lit while it is open, so tapping it
    /// returns to that tab's own screen.
    pub fn go(&mut self, tab: Tab) -> bool {
        if self.tab == tab && self.book.is_none() {
            return false;
        }
        self.tab = tab;
        self.book = None;
        true
    }

    /// Step the calendar a month either way.
    pub fn shift_month(&mut self, by: i64) {
        let (mut y, mut m) = self.month;
        m += by;
        while m > 12 {
            m -= 12;
            y += 1;
        }
        while m < 1 {
            m += 12;
            y -= 1;
        }
        self.month = (y, m);
    }
}

/// What a screen draws with.
pub struct Ctx<'a> {
    pub fb: &'a mut Framebuffer,
    pub text: &'a mut TextRenderer,
    pub covers: &'a mut Covers,
    pub theme: &'a Theme,
    pub lang: Lang,
    pub week: crate::settings::WeekStart,
    pub stats: &'a Stats,
    /// The device's own local day, and the second of it now.
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
    use crate::date;

    #[test]
    fn a_month_steps_across_the_year_boundary() {
        let mut s = State::new(date::days_from_civil(2026, 1, 15));
        assert_eq!(s.month, (2026, 1));
        s.shift_month(-1);
        assert_eq!(s.month, (2025, 12));
        s.shift_month(1);
        assert_eq!(s.month, (2026, 1));
        s.shift_month(12);
        assert_eq!(s.month, (2027, 1));
        s.shift_month(-24);
        assert_eq!(s.month, (2025, 1));
    }

    #[test]
    fn a_tab_tap_lands_on_that_tab() {
        let mut s = State::new(date::days_from_civil(2026, 9, 3));
        assert!(s.go(Tab::Books));
        assert_eq!(s.tab, Tab::Books);
        assert!(s.go(Tab::Clock));
        assert_eq!(s.tab, Tab::Clock);
    }

    #[test]
    fn a_tab_tap_closes_the_book_open_over_it() {
        // The only way out of a book. Tapping the tab it was opened from
        // returns to that tab's screen and stays there.
        let mut s = State::new(date::days_from_civil(2026, 9, 3));
        s.go(Tab::Books);
        s.book = Some(3);
        assert!(s.go(Tab::Books), "the tab under a book still navigates");
        assert_eq!(s.tab, Tab::Books);
        assert!(s.book.is_none());

        // A book opened from Today is closed by any tab, landing on that one.
        s.book = Some(3);
        assert!(s.go(Tab::Calendar));
        assert_eq!(s.tab, Tab::Calendar);
        assert!(s.book.is_none());
    }

    #[test]
    fn the_tab_already_showing_is_not_a_navigation() {
        let mut s = State::new(date::days_from_civil(2026, 9, 3));
        assert_eq!(s.tab, Tab::Home);
        assert!(
            !s.go(Tab::Home),
            "a redraw is owed only where something moved"
        );
        assert_eq!(s.tab, Tab::Home);
    }

    #[test]
    fn a_state_opens_on_the_month_the_device_is_in() {
        let s = State::new(date::days_from_civil(2026, 8, 29));
        assert_eq!(s.month, (2026, 8));
        assert_eq!(s.tab, Tab::Home);
        assert!(s.day.is_none());
        assert!(s.book.is_none());
    }
}
