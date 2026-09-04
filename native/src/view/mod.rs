//! The screens.
//!
//! Each takes the box left above the tab strip and draws into it, recording a
//! hit box for anything the reader can touch. A screen
//! holds no state of its own: which day is showing, how wide a span is drawn
//! around it and which book is open all live in [`State`], so a redraw after a
//! tap is the same call with a different state.

pub mod alltime;
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

/// Something the reader can touch.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Hit {
    Tab(Tab),
    /// A day of the Rhythm grid, as a day count.
    Day(i64),
    /// A book, by its index in [`Stats::books`].
    Book(usize),
    /// Leave the app. The only way out: a book is closed by tapping a tab.
    Exit,
    /// A chip on the config page.
    Language(Lang),
    WeekStart(WeekStart),
    TextSize(crate::settings::TextSize),
    Prev,
    Next,
    /// One span of the Rhythm screen.
    Span(Span),
    /// The average day drawn over every span of its width, or over the one
    /// showing.
    Average(bool),
    /// A page of the book list, forward or back.
    ListPage(i64),
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
                format!(
                    "{} – {}",
                    date::short_day(*days.start(), s),
                    date::short_day(*days.end(), s)
                )
            }
            Span::Month => date::month_name(year, month, s),
            Span::Year => match s.date_ymd {
                true => format!("{year}年"),
                false => year.to_string(),
            },
        }
    }
}

/// Where the reader has navigated to.
#[derive(Debug, Clone, PartialEq)]
pub struct State {
    pub tab: Tab,
    /// The day the Rhythm screen is looking at. The span holding it is what
    /// the grid draws, and its own books are what the page lists.
    pub day: i64,
    pub span: Span,
    /// Whether the reader has picked `day` off the grid. A week and a year
    /// narrow their book list to it; a month draws it whole.
    pub picked: bool,
    /// The book whose own screen is open, over whichever tab opened it.
    pub book: Option<usize>,
    /// How far down the book list has been paged.
    pub books_from: usize,
    /// Whether the average day covers every span of its width.
    pub average_all: bool,
    /// How far down Rhythm's own book list has been paged.
    pub list_from: usize,
}

impl State {
    pub fn new(today: i64) -> Self {
        Self {
            tab: Tab::Home,
            day: today,
            span: Span::Month,
            picked: false,
            book: None,
            books_from: 0,
            average_all: false,
            list_from: 0,
        }
    }

    /// Go to `tab`, closing any book or day open over it. Answers whether that
    /// moved anywhere: a tap on the tab already showing, with nothing open
    /// over it, is not a navigation and costs no redraw.
    ///
    /// This is the only way out of a book — there is no back control, and the
    /// tab a book was opened from stays lit while it is open, so tapping it
    /// returns to that tab's own screen.
    pub fn go(&mut self, tab: Tab) -> bool {
        if self.tab == tab && self.book.is_none() && !self.picked {
            return false;
        }
        self.tab = tab;
        self.book = None;
        self.picked = false;
        true
    }

    /// Step Rhythm on: a day at a time where one is open, else a whole span.
    pub fn shift(&mut self, by: i64) {
        self.day = match self.picked {
            true => self.day + by,
            false => self.span.step(self.day, by),
        };
    }
}

/// What a screen draws with.
pub struct Ctx<'a> {
    pub fb: &'a mut Framebuffer,
    pub text: &'a mut TextRenderer,
    pub covers: &'a mut Covers,
    pub theme: &'a Theme,
    pub lang: Lang,
    pub week: WeekStart,
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
        assert_eq!(s.span, Span::Month);
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
    fn a_picked_day_steps_a_day_at_a_time() {
        let mut s = State::new(third());
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
}
