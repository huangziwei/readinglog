//! [`Stats`] is built once from a [`Record`]. A sitting's `end_position`,
//! through `Record::extent_of`, is the library's `p_contentSize`, which a
//! [`record::Book`] is keyed by.

use std::collections::HashMap;

use crate::clippings::Kind;
use crate::date;
use crate::highlights::Highlight;
use crate::log::sitting::{self, Measure, Stretch};
use crate::record::{self, FINISHED_PERCENT, Record};
use crate::settings::{SittingFloor, StatsFrom, WeekStart};

/// The day a book with no reading stands on, absent from `days`.
pub const NO_DAY: i64 = i64::MIN;

/// One book, with everything ever read in it.
#[derive(Debug, Default, Clone, PartialEq)]
pub struct BookStat {
    /// The library's number for this book, and the key the views address it by.
    pub extent: i64,
    /// `Book::cde_key`, which `Record::set_finished` addresses.
    pub cde_key: String,
    /// `Book::cde_type`, which `mark::set` names the book by.
    pub cde_type: String,
    pub title: String,
    pub author: String,
    /// The cover to draw, empty where none will — see [`cover`].
    pub thumbnail: String,
    /// The library's own progress figure, 0 through 100, or negative.
    pub percent: f64,
    /// Whether the library names a file for this book.
    pub on_kindle: bool,
    /// The file holding this book, which `open::uri` names.
    pub location: String,
    /// The library language tag, which picks the face a CJK title is set in.
    pub language: String,
    /// `Book::finished`.
    pub finished: bool,
    /// `Book::restarted_on`, which the reading standing is not in.
    pub restarted_on: Vec<i64>,
    pub seconds: i64,
    /// The seconds `Sitting` counted under any [`StatsFrom`]: `words`'s span under `Kindle`.
    pub counted_seconds: i64,
    /// The parts of `seconds` carrying `Measure::Paged` and `Measure::Awake`.
    pub paged_seconds: i64,
    pub awake_seconds: i64,
    pub sittings: i64,
    pub page_turns: i64,
    pub words: i64,
    /// Distinct days this book was read on.
    pub days: i64,
    pub first_day: i64,
    pub last_day: i64,
    /// Seconds into `last_day` at which the last sitting ended.
    pub last_secs: i64,
    /// The share of the book this record's sittings cover, summed: [`Self::time_left`]'s divisor.
    pub covered: f64,
    /// The seconds those pages were open: `covered`'s numerator, off the same page metrics.
    pub covered_seconds: i64,
    /// `Book::timer_ms` in seconds, reaching reading before [`Record::reset_at`].
    pub timer_seconds: i64,
    /// `Book::timer_words`, over the same reading as `timer_seconds`.
    pub timer_words: i64,
    /// `Book::timer_covered`, the share of the book `timer_seconds` covers.
    pub timer_covered: f64,
    /// `Sitting::time_left` as the newest sitting closed.
    pub stated_time_left: Option<i64>,
    /// The rate it stated at the same moment, off `Sitting::stated_wpm`.
    pub stated_wpm: Option<i64>,
    /// Where this book's highlights sit in [`Stats::highlights`], in the order they were made.
    pub highlights: Vec<usize>,
}

impl BookStat {
    /// Whether a cover can be drawn for this book: `thumbnail` names one.
    pub fn has_cover(&self) -> bool {
        !self.thumbnail.is_empty()
    }

    /// Whether the library states a progress figure for this book.
    pub fn has_percent(&self) -> bool {
        self.percent >= 0.0
    }

    /// The progress every figure for this book states, through
    /// `whole_per_cent`. Below zero outside [`Self::has_percent`], and
    /// `finished` does not carry it.
    pub fn percent_shown(&self) -> i64 {
        whole_per_cent(self.percent)
    }

    /// How full a bar for this book draws: 100 under `finished`, else
    /// [`Self::percent_shown`].
    pub fn bar_percent(&self) -> i64 {
        match self.finished {
            true => 100,
            false => self.percent_shown(),
        }
    }

    /// Whether this book can be opened: `on_kindle`, and a `location` the
    /// library stated.
    pub fn can_open(&self) -> bool {
        self.on_kindle && !self.location.is_empty()
    }

    /// `finished`, which `Book::stand_at` sets at [`FINISHED_PERCENT`]
    /// and a falling `percent` leaves on.
    pub fn is_finished(&self) -> bool {
        self.finished
    }

    /// How many times this book has been read end to end: the readings
    /// `Record::restart` banked, and the one standing where it is finished.
    pub fn times_finished(&self) -> i64 {
        self.restarted_on.len() as i64 + i64::from(self.is_finished())
    }

    /// Whether `percent` alone reaches [`FINISHED_PERCENT`].
    pub fn read_through(&self) -> bool {
        self.percent >= FINISHED_PERCENT
    }

    /// Whether `finished` can be set: `percent` states neither way.
    pub fn can_mark(&self) -> bool {
        !self.read_through()
    }

    /// Whether `Record::restart` has anything to clear: `finished`, or progress.
    pub fn can_restart(&self) -> bool {
        self.finished || self.percent_shown() > 0
    }

    /// Days from the first this book was read on to the last, both counted.
    pub fn span_days(&self) -> i64 {
        (self.last_day - self.first_day + 1).max(1)
    }

    /// Over `days`, the days with reading on them.
    pub fn per_day(&self, from: StatsFrom) -> i64 {
        match self.days {
            0 => 0,
            d => self.read_seconds(from) / d,
        }
    }

    pub fn per_sitting(&self, from: StatsFrom) -> i64 {
        match self.sittings {
            0 => 0,
            s => self.read_seconds(from) / s,
        }
    }

    /// This book's reading: `seconds`, the sum `Stats::build` credits each
    /// sitting through `sitting_seconds`.
    pub fn read_seconds(&self, _from: StatsFrom) -> i64 {
        self.seconds
    }

    /// [`Self::read_seconds`] for the word counter.
    pub fn words_read(&self, _from: StatsFrom) -> i64 {
        self.words
    }

    /// Seconds left to read: `stated_time_left`, else the seconds a book's pages
    /// were open over the share they cover, times the share ahead.
    /// `OldTimeLeft` states the same figure.
    pub fn time_left(&self, from: StatsFrom) -> Option<i64> {
        if self.is_finished() {
            return Some(0);
        }
        if let (StatsFrom::Kindle, Some(stated)) = (from, self.stated_time_left) {
            return Some(stated);
        }
        if self.percent < 0.5 || self.percent >= FINISHED_PERCENT {
            return None;
        }
        let ahead = (100.0 - self.percent) / 100.0;
        let (read, covered) = match from {
            StatsFrom::Kindle => (self.timer_seconds, self.timer_covered),
            StatsFrom::App => (self.covered_seconds, self.covered),
        };
        // `covered` or `read` at zero leaves nothing to project from.
        if covered <= 0.0 || read <= 0 {
            return None;
        }
        Some((read as f64 * ahead / covered) as i64)
    }

    /// Words a minute: `stated_wpm` under [`StatsFrom::Kindle`], else this
    /// mode's own words over this mode's own seconds, both off one source and
    /// never the pages over the counter.
    pub fn wpm(&self, from: StatsFrom) -> Option<i64> {
        if let (StatsFrom::Kindle, Some(stated)) = (from, self.stated_wpm) {
            return Some(stated);
        }
        let (words, read) = match from {
            StatsFrom::App => (self.words, self.seconds),
            // `timer.model`'s own figures before this record's, which reach
            // reading before [`Record::reset_at`] as `Self::time_left` does.
            StatsFrom::Kindle if self.timer_words > 0 && self.timer_seconds > 0 => {
                (self.timer_words, self.timer_seconds)
            }
            StatsFrom::Kindle => (self.words, self.counted_seconds),
        };
        (words > 0 && read > 0).then(|| words * 60 / read)
    }
}

/// `value`, 0 through 100, as whole per cent, through `round_ties_even`.
fn whole_per_cent(value: f64) -> i64 {
    value.round_ties_even() as i64
}

/// One sitting, placed on the calendar and pointed at its book.
#[derive(Debug, Clone, PartialEq)]
pub struct Sitting {
    pub day: i64,
    pub from_secs: i64,
    pub to_secs: i64,
    pub seconds: i64,
    /// Index into [`Stats::books`]. `None` where the book is unidentified.
    pub book: Option<usize>,
    /// The library number this sitting was keyed by, identified or not.
    pub key: i64,
    pub measure: Measure,
    pub page_turns: i64,
    /// [`Sitting::hours`], the seconds read in each clock hour of `day`.
    pub hours: Vec<(u8, i64)>,
    /// [`Sitting::progress`], where the book stood as this sitting ended.
    pub progress: Option<f64>,
    /// [`Sitting::stretches`], the stretches this sitting covered.
    pub stretches: Vec<Stretch>,
}

/// The width of one band of the sitting histogram; [`Stats::first_band`] names the first.
pub const SITTING_STEP_SECS: i64 = 5 * 60;
/// The bands of the sitting histogram, the last holding every longer sitting.
pub const SITTING_BANDS: usize = 25;

/// What a span of days came to, from [`Stats::tally`].
pub struct Tally {
    /// Seconds read over the span.
    pub read: i64,
    /// Days of it with any reading on them.
    pub days_read: i64,
    /// What one of those days averaged.
    pub a_day: i64,
    /// Books the library states read through, finished inside the span.
    pub finished: i64,
}

/// The record folded onto one cycle, from [`Stats::average_day`] and its kin.
pub struct Fold {
    /// One entry per bucket of the cycle, in the order they are drawn.
    pub values: Vec<i64>,
    /// What one turn of the cycle comes to, over however many have gone by.
    pub each: i64,
    /// The fullest bucket, where any of them holds anything.
    pub busiest: Option<usize>,
}

/// The whole picture, built once per launch.
#[derive(Debug, Default, Clone, PartialEq)]
pub struct Stats {
    /// Most recently read first.
    pub books: Vec<BookStat>,
    /// `(day, seconds)`, ascending, one entry per day with any reading.
    pub days: Vec<(i64, i64)>,
    /// Ascending by day then by start.
    pub sittings: Vec<Sitting>,
    pub total_seconds: i64,
    pub total_turns: i64,
    pub total_words: i64,
    pub longest_streak: i64,
    pub current_streak: i64,
    /// Seconds stated against a file `Book::is_book` is false on.
    pub skipped_seconds: i64,
    /// Seconds on unidentified books. In every total, drawn as no row.
    pub unidentified_seconds: i64,
    /// The floor [`Self::build`] took, which [`Self::first_band`] reads.
    pub sitting_floor: SittingFloor,
    /// Every highlight the record holds, ascending by when it was made.
    pub highlights: Vec<Highlight>,
    /// Each highlight's `body` lowercased and Han-folded, `None` where no book holds it.
    pub folded: Vec<Option<String>>,
}

/// One row of [`Stats::book_highlights`] and where in the record it came from.
#[derive(Debug, Clone, Copy)]
pub struct Across<'a> {
    /// The book, by its index in [`Stats::books`].
    pub book: usize,
    /// The row's place in its book's [`Stats::book_highlights`], where the tab opens.
    pub at: usize,
    /// Where the highlight sits in [`Stats::highlights`], and in [`Stats::folded`].
    pub held: usize,
    pub highlight: &'a Highlight,
    /// The note written on it, as [`crate::highlights::Paired`] pairs them.
    pub note: Option<&'a Highlight>,
}

impl Stats {
    /// Everything the record holds, totalled, with no library read. `today`
    /// places the current streak, `unidentified` and `floor` settle which
    /// sittings count, and `from` picks the seconds, per [`sitting_seconds`].
    pub fn build(
        record: &Record,
        today: i64,
        unidentified: bool,
        from: StatsFrom,
        floor: SittingFloor,
    ) -> Self {
        let mut out = Self {
            sitting_floor: floor,
            ..Self::default()
        };
        // One slot per book, in first-seen order, until `sort_books` orders them.
        let mut index: Vec<(i64, usize)> = Vec::new();
        for s in &record.sittings {
            let Some(day) = date::parse_day(date::day_of(&s.started_at)) else {
                continue;
            };
            let secs = sitting_seconds(s, from);
            let words = sitting_words(s, from);
            let counted = secs >= floor.seconds();
            // `found.extent` where the record holds the book, else `raw`.
            let raw = record.extent_of(s.end_position);
            let found = record.book_for(raw, s.asin.as_deref());
            // `is_book` is false on a scriptlet and on `My Clippings.txt`.
            if found.is_some_and(|b| !b.is_book()) {
                out.skipped_seconds += secs;
                continue;
            }
            // A [`record::Book`] reached by its key alone states an extent of zero.
            let at = found.map(|stored| {
                let extent = match stored.extent {
                    0 => raw,
                    stated => stated,
                };
                match index.binary_search_by_key(&extent, |(e, _)| *e) {
                    Ok(i) => index[i].1,
                    Err(i) => {
                        let slot = out.books.len();
                        // [`NO_DAY`] until `credit` sets the days.
                        let opened = match counted {
                            true => day,
                            false => NO_DAY,
                        };
                        out.books.push(fresh(extent, stored, opened));
                        index.insert(i, (extent, slot));
                        slot
                    }
                }
            });
            if !counted {
                continue;
            }
            match at {
                Some(slot) => credit(&mut out.books[slot], s, day, secs, words),
                None if !unidentified => continue,
                None => out.unidentified_seconds += secs,
            }
            let (from_secs, to_secs) = (date::secs_of(&s.started_at), date::secs_of(&s.ended_at));
            out.sittings.push(Sitting {
                day,
                from_secs,
                to_secs,
                seconds: secs,
                book: at,
                key: raw,
                measure: s.measure,
                page_turns: s.page_turns,
                hours: hours_at(&s.hours, s.seconds, secs, from_secs, to_secs),
                progress: s.progress,
                stretches: s.stretches.clone(),
            });
            out.total_seconds += secs;
            out.total_turns += s.page_turns;
            out.total_words += words;
            match out.days.binary_search_by_key(&day, |(d, _)| *d) {
                Ok(i) => out.days[i].1 += secs,
                Err(i) => out.days.insert(i, (day, secs)),
            }
        }

        // `record.books` lists at zero a book `Record::clear_book` emptied of
        // every sitting.
        for stored in record.books.iter().filter(|b| b.kept && b.is_book()) {
            let extent = match stored.extent {
                0 => continue,
                stated => stated,
            };
            if out.books.iter().any(|b| b.extent == extent) {
                continue;
            }
            out.books.push(fresh(extent, stored, NO_DAY));
        }

        out.hold_days();
        out.sittings.sort_by_key(|s| (s.day, s.from_secs));
        let (longest, current) = streaks(&out.days, today);
        out.longest_streak = longest;
        out.current_streak = current;
        out.sort_books();
        out.hold_highlights(record);
        out
    }

    /// Each book's `days`: the distinct days it was read on, in one pass over
    /// the sittings.
    fn hold_days(&mut self) {
        let mut seen: Vec<(usize, i64)> = self
            .sittings
            .iter()
            .filter_map(|s| Some((s.book?, s.day)))
            .collect();
        seen.sort_unstable();
        seen.dedup();
        for book in &mut self.books {
            book.days = 0;
        }
        for (slot, _) in seen {
            self.books[slot].days += 1;
        }
    }

    /// Every `a` row handed to the book it names, after `sort_books`, at that
    /// book's index in [`Self::books`]; [`Self::highlights`] holds one naming no
    /// book there all the same.
    fn hold_highlights(&mut self, record: &Record) {
        self.highlights = record.highlights.clone();
        // Both ways in, built once, each holding the *first* slot under a key.
        let mut by_extent: HashMap<i64, usize> = HashMap::new();
        let mut by_title: HashMap<String, usize> = HashMap::new();
        for (at, book) in self.books.iter().enumerate() {
            by_extent.entry(book.extent).or_insert(at);
            by_title
                .entry(crate::identify::normalise(&book.title))
                .or_insert(at);
        }
        // `self.folded` holds `None` for a highlight no book holds.
        self.folded = Vec::with_capacity(self.highlights.len());
        let mut held: Vec<(usize, usize)> = Vec::new();
        for (at, highlight) in self.highlights.iter().enumerate() {
            let slot = match highlight.extent != 0 {
                true => by_extent.get(&highlight.extent).copied(),
                false => None,
            }
            .or_else(|| {
                by_title
                    .get(&crate::identify::normalise(&highlight.title))
                    .copied()
            });
            self.folded.push(
                slot.map(|_| crate::hanfold::fold(&highlight.body.to_lowercase()).into_owned()),
            );
            if let Some(slot) = slot {
                held.push((slot, at));
            }
        }
        for (slot, at) in held {
            self.books[slot].highlights.push(at);
        }
    }

    /// The highlights one book carries, in the order they were made.
    pub fn highlights_of(&self, book: usize) -> impl Iterator<Item = &Highlight> {
        self.books.get(book).into_iter().flat_map(|b| {
            b.highlights
                .iter()
                .filter_map(|at| self.highlights.get(*at))
        })
    }

    /// One book's highlights: `Kind::spans_a_passage`, most recently made first,
    /// each with the note `highlights::paired` folds under it. `Kind::Bookmark`
    /// and `Kind::Pin` carry no words and are not here.
    pub fn book_highlights(&self, book: usize) -> Vec<crate::highlights::Paired<'_>> {
        let mut held: Vec<&Highlight> = self
            .highlights_of(book)
            .filter(|m| m.kind.spans_a_passage())
            .collect();
        held.reverse();
        crate::highlights::paired(&held)
    }

    /// Every book's [`Self::book_highlights`] rows as one list, `Highlight::at`
    /// descending and an empty one last, each carrying its `book` and its `at`
    /// in that book's own list.
    pub fn highlights_across(&self) -> Vec<Across<'_>> {
        let mut out: Vec<Across<'_>> = Vec::new();
        for book in 0..self.books.len() {
            // `row.at` is a place in the list `paired` built; `held` is the
            // place in [`Self::highlights`], off `BookStat::highlights`.
            let slots = &self.books[book].highlights;
            for (at, row) in self.book_highlights(book).into_iter().enumerate() {
                let held = slots
                    .iter()
                    .copied()
                    .find(|&i| {
                        self.highlights
                            .get(i)
                            .is_some_and(|m| std::ptr::eq(m, row.highlight))
                    })
                    .unwrap_or(usize::MAX);
                out.push(Across {
                    book,
                    at,
                    held,
                    highlight: row.highlight,
                    note: row.note,
                });
            }
        }
        out.sort_by(|a, b| b.highlight.at.cmp(&a.highlight.at));
        out
    }

    /// One book's highlights as a row states them: passages highlighted, and
    /// notes written, a note under the second and its passage under the
    /// first, no row counted twice.
    pub fn highlights_counted(&self, book: usize) -> (usize, usize) {
        Self::counted(
            self.book_highlights(book)
                .iter()
                .collect::<Vec<_>>()
                .as_slice(),
        )
    }

    /// [`Self::highlights_counted`] over the highlights made on `day` alone, which is
    /// what a row listing that book's reading on that day states.
    pub fn highlights_counted_on(&self, book: usize, day: i64) -> (usize, usize) {
        self.highlights_counted_over(book, day..=day)
    }

    /// [`Self::highlights_counted`] over the highlights made inside `days`, a
    /// row counting where its passage or its note was made there.
    pub fn highlights_counted_over(
        &self,
        book: usize,
        days: std::ops::RangeInclusive<i64>,
    ) -> (usize, usize) {
        let made = |m: &Highlight| m.day().is_some_and(|day| days.contains(&day));
        let held = self.book_highlights(book);
        let mine: Vec<&crate::highlights::Paired<'_>> = held
            .iter()
            .filter(|m| made(m.highlight) || m.note.is_some_and(made))
            .collect();
        Self::counted(&mine)
    }

    /// The two figures [`Self::highlights_counted`] states, over `rows`.
    fn counted(rows: &[&crate::highlights::Paired<'_>]) -> (usize, usize) {
        let notes = rows
            .iter()
            .filter(|m| m.note.is_some() || m.highlight.kind == Kind::Note)
            .count();
        let book_highlights = rows
            .iter()
            .filter(|m| m.highlight.kind != Kind::Note)
            .count();
        (book_highlights, notes)
    }

    /// Highlights made on one day, whatever book they belong to.
    pub fn highlights_on(&self, day: i64) -> impl Iterator<Item = &Highlight> {
        self.highlights.iter().filter(move |m| m.day() == Some(day))
    }

    /// How many rows [`Self::book_highlights`] lists for one book, which is the count
    /// its tab carries. `highlights::fold` writes no row for a deleted highlight.
    pub fn highlights_held(&self, book: usize) -> usize {
        self.book_highlights(book).len()
    }

    /// One book's highlights by kind, in [`Kind::ALL`]'s order, leaving out the
    /// kinds it carries none of.
    pub fn highlights_by_kind(&self, book: usize) -> Vec<(Kind, usize)> {
        Kind::ALL
            .into_iter()
            .filter_map(|kind| {
                let n = self.highlights_of(book).filter(|m| m.kind == kind).count();
                (n > 0).then_some((kind, n))
            })
            .collect()
    }

    /// One book's highlights by the `colour` the sidecar names, most first, an
    /// empty `colour` left out.
    pub fn highlights_by_colour(&self, book: usize) -> Vec<(String, usize)> {
        let mut out: Vec<(String, usize)> = Vec::new();
        for highlight in self.highlights_of(book).filter(|m| !m.colour.is_empty()) {
            match out.iter_mut().find(|(name, _)| *name == highlight.colour) {
                Some(held) => held.1 += 1,
                None => out.push((highlight.colour.clone(), 1)),
            }
        }
        out.sort_by(|a, b| b.1.cmp(&a.1).then(a.0.cmp(&b.0)));
        out
    }

    /// The longest streak over the days `book` does not stand alone on: those
    /// whose seconds run past that book's own in [`Self::book_days`], through
    /// [`streaks`].
    pub fn streak_without(&self, book: usize) -> i64 {
        let mine = self.book_days(book);
        let left: Vec<(i64, i64)> = self
            .days
            .iter()
            .filter(|(day, secs)| {
                let taken = mine
                    .binary_search_by_key(day, |(d, _)| *d)
                    .map_or(0, |i| mine[i].1);
                *secs > taken
            })
            .copied()
            .collect();
        streaks(&left, self.last_day()).0
    }

    pub fn day_seconds(&self, day: i64) -> i64 {
        self.days
            .binary_search_by_key(&day, |(d, _)| *d)
            .map_or(0, |i| self.days[i].1)
    }

    /// Where a day's reading fell, as spans of seconds into the day: a block in
    /// each clock hour `Sitting::hours` states, as many seconds long as were
    /// read there.
    pub fn day_blocks(&self, day: i64) -> Vec<(i64, i64)> {
        self.day_blocks_of(day, None)
    }

    /// [`Stats::day_blocks`] narrowed to one book, or all of them where `book`
    /// is `None`.
    pub fn day_blocks_of(&self, day: i64, book: Option<usize>) -> Vec<(i64, i64)> {
        let mut out: Vec<(i64, i64)> = Vec::new();
        let mine = |s: &Sitting| book.is_none() || s.book == book;
        for sitting in self.sittings_on(day).filter(|s| mine(s)) {
            let closed = sitting.to_secs.max(sitting.from_secs);
            for (hour, secs) in &sitting.hours {
                let (lo, hi) = (*hour as i64 * 3600, (*hour as i64 + 1) * 3600);
                // The window's overlap with this hour, or the whole hour where
                // the two do not meet.
                let (from, to) = match (lo.max(sitting.from_secs), hi.min(closed)) {
                    (a, b) if a < b => (a, b),
                    _ => (lo, hi),
                };
                out.push((from, (from + secs).min(to)));
            }
        }
        out.sort_unstable();
        out
    }

    /// The sittings of one day, in the order they happened.
    pub fn sittings_on(&self, day: i64) -> impl Iterator<Item = &Sitting> {
        self.sittings.iter().filter(move |s| s.day == day)
    }

    /// The longest of [`Self::sittings_on`] `day`, 0 where it holds none.
    pub fn longest_on(&self, day: i64) -> i64 {
        self.sittings_on(day).map(|s| s.seconds).max().unwrap_or(0)
    }

    /// Pages turned on one day, over [`Self::sittings_on`].
    pub fn turns_on(&self, day: i64) -> i64 {
        self.sittings_on(day).map(|s| s.page_turns).sum()
    }

    /// The sittings falling inside `days`, ascending. The one population every
    /// figure over the sittings of a span is drawn from.
    pub fn sittings_over(
        &self,
        days: std::ops::RangeInclusive<i64>,
    ) -> impl Iterator<Item = &Sitting> {
        self.sittings.iter().filter(move |s| days.contains(&s.day))
    }

    /// `(day, seconds)` for every day inside `days` with reading on it. The one
    /// population every figure over a span of days is drawn from.
    pub fn days_in(
        &self,
        days: std::ops::RangeInclusive<i64>,
    ) -> impl Iterator<Item = &(i64, i64)> {
        self.days.iter().filter(move |(d, _)| days.contains(d))
    }

    /// One book's sittings, ascending, every one the [`SittingFloor`] kept.
    /// The one population any figure counting this book's sittings is drawn
    /// from, `BookStat::sittings` included.
    pub fn book_sittings(&self, book: usize) -> impl Iterator<Item = &Sitting> {
        self.sittings.iter().filter(move |s| s.book == Some(book))
    }

    /// `(day, seconds)` for every day one book was read on, ascending.
    pub fn book_days(&self, book: usize) -> Vec<(i64, i64)> {
        let mut out: Vec<(i64, i64)> = Vec::new();
        for s in self.book_sittings(book) {
            match out.binary_search_by_key(&s.day, |(d, _)| *d) {
                Ok(i) => out[i].1 += s.seconds,
                Err(i) => out.insert(i, (s.day, s.seconds)),
            }
        }
        out
    }

    /// Where one book stood as each of its sittings ended: one entry per
    /// [`Self::book_sittings`], ascending, the day and the progress as a whole per
    /// cent, `None` where `Sitting::progress` states none.
    pub fn book_progress(&self, book: usize) -> Vec<(i64, Option<i64>)> {
        let mut out: Vec<((i64, i64), Option<f64>)> = self
            .book_sittings(book)
            .map(|s| ((s.day, s.to_secs), s.progress))
            .collect();
        out.sort_by_key(|(at, _)| *at);
        out.into_iter()
            .map(|((day, _), p)| {
                (
                    day,
                    p.map(|p| whole_per_cent((p * 100.0).clamp(0.0, 100.0))),
                )
            })
            .collect()
    }

    /// One entry per [`Self::book_sittings`], ascending and in step with
    /// [`Self::book_progress`], each stretch a pair of whole per cents, an
    /// empty list where a sitting states no stretch.
    pub fn book_stretches(&self, book: usize) -> Vec<Vec<(i64, i64)>> {
        /// One sitting's stretches, keyed by its close as [`Stats::book_progress`] keys.
        type Closed = ((i64, i64), Vec<(i64, i64)>);
        let mut out: Vec<Closed> = self
            .book_sittings(book)
            .map(|s| {
                let at = |p: f64| whole_per_cent((p * 100.0).clamp(0.0, 100.0));
                let stretches = s.stretches.iter().map(|r| (at(r.from), at(r.to))).collect();
                ((s.day, s.to_secs), stretches)
            })
            .collect();
        out.sort_by_key(|(at, _)| *at);
        out.into_iter().map(|(_, stretches)| stretches).collect()
    }

    /// Each reading of one book: the first and last day it was read on, each
    /// day in `BookStat::restarted_on` opening one and closing the one before,
    /// the last entry alone naming a reading left open.
    pub fn book_readings(&self, book: usize) -> Vec<(i64, i64)> {
        let Some(stat) = self.books.get(book) else {
            return Vec::new();
        };
        let days: Vec<i64> = self
            .book_days(book)
            .into_iter()
            .map(|(day, _)| day)
            .collect();
        let mut out = Vec::new();
        let mut from = 0usize;
        for began in &stat.restarted_on {
            let cut = from + days[from..].partition_point(|day| day < began);
            if cut > from {
                out.push((days[from], days[cut - 1]));
            }
            from = cut;
        }
        if from < days.len() {
            out.push((days[from], days[days.len() - 1]));
        }
        out
    }

    /// One book's reading over `days` cut by the clock hour it happened in, as
    /// [`Stats::hours_over`] cuts a span of days. `days` is one reading of the
    /// book, off [`Stats::book_readings`].
    pub fn book_hours(&self, book: usize, days: std::ops::RangeInclusive<i64>) -> [i64; 24] {
        let mut out = [0i64; 24];
        for sitting in self.book_sittings(book).filter(|s| days.contains(&s.day)) {
            for (hour, secs) in &sitting.hours {
                out[(*hour as usize).min(23)] += secs;
            }
        }
        out
    }

    /// Seconds read of one book over `days`, off [`Self::book_sittings`]. Over
    /// one of [`Self::book_readings`], what that reading took.
    pub fn book_seconds_over(&self, book: usize, days: std::ops::RangeInclusive<i64>) -> i64 {
        self.book_sittings(book)
            .filter(|s| days.contains(&s.day))
            .map(|s| s.seconds)
            .sum()
    }

    /// Seconds read of each book over a span of days, longest first, a sitting
    /// on an unidentified book left out: [`Stats::day_seconds`] over the same
    /// days can come to more.
    pub fn book_totals(&self, days: std::ops::RangeInclusive<i64>) -> Vec<(usize, i64)> {
        let mut out: Vec<(usize, i64)> = Vec::new();
        for sitting in self.sittings_over(days.clone()) {
            let Some(book) = sitting.book else { continue };
            match out.iter_mut().find(|(b, _)| *b == book) {
                Some((_, secs)) => *secs += sitting.seconds,
                None => out.push((book, sitting.seconds)),
            }
        }
        out.sort_by_key(|(_, secs)| -secs);
        out
    }

    /// The same, ordered by the last sitting each book had inside `days`:
    /// what was picked up most recently, first.
    pub fn book_totals_recent(&self, days: std::ops::RangeInclusive<i64>) -> Vec<(usize, i64)> {
        // Seconds, and the instant the book was last put down inside `days`.
        let mut out: Vec<(usize, i64, (i64, i64))> = Vec::new();
        for sitting in self.sittings_over(days.clone()) {
            let Some(book) = sitting.book else { continue };
            let at = (sitting.day, sitting.to_secs);
            match out.iter_mut().find(|(b, _, _)| *b == book) {
                Some((_, secs, last)) => {
                    *secs += sitting.seconds;
                    *last = (*last).max(at);
                }
                None => out.push((book, sitting.seconds, at)),
            }
        }
        out.sort_by_key(|(_, _, last)| (-last.0, -last.1));
        out.into_iter()
            .map(|(book, secs, _)| (book, secs))
            .collect()
    }

    /// The progress `book` stood at as its last sitting inside `days` ended, or
    /// [`BookStat::percent_shown`] where `days` holds the last day of a book
    /// read through: `Sitting::progress` stops a page short of the end.
    pub fn percent_over(&self, book: usize, days: std::ops::RangeInclusive<i64>) -> Option<i64> {
        let stat = self.books.get(book)?;
        if stat.is_finished() && stat.has_percent() && days.contains(&stat.last_day) {
            return Some(stat.percent_shown());
        }
        self.book_sittings(book)
            .filter(|s| days.contains(&s.day))
            .filter_map(|s| Some(((s.day, s.to_secs), s.progress?)))
            .max_by_key(|(at, _)| *at)
            .map(|(_, p)| whole_per_cent((p * 100.0).clamp(0.0, 100.0)))
    }

    /// Unidentified books read over `days`, and the seconds on them, a book to
    /// each [`Sitting::key`]: two sittings no line stated a library number for,
    /// a word position apiece, counting as two.
    pub fn unidentified_over(&self, days: std::ops::RangeInclusive<i64>) -> (usize, i64) {
        let mut keys: Vec<i64> = Vec::new();
        let mut seconds = 0;
        for sitting in self.sittings_over(days.clone()) {
            if sitting.book.is_some() {
                continue;
            }
            seconds += sitting.seconds;
            if let Err(at) = keys.binary_search(&sitting.key) {
                keys.insert(at, sitting.key);
            }
        }
        (keys.len(), seconds)
    }

    /// Unidentified books the whole record holds.
    pub fn unidentified_books(&self) -> usize {
        match self.days.first() {
            Some((first, _)) => self.unidentified_over(*first..=self.last_day()).0,
            None => 0,
        }
    }

    /// `books` and [`Self::unidentified_books`] together.
    pub fn book_count(&self) -> usize {
        self.books.len() + self.unidentified_books()
    }

    /// The last day the record holds.
    fn last_day(&self) -> i64 {
        self.days.last().map(|(d, _)| *d).unwrap_or(0)
    }

    /// Everything read over `days`, cut by the clock hour it happened in, off
    /// each sitting's own `hours`: a sitting past midnight counted where it
    /// was read, never where it started.
    pub fn hours_over(&self, days: std::ops::RangeInclusive<i64>) -> [i64; 24] {
        let mut out = [0i64; 24];
        for sitting in self.sittings_over(days.clone()) {
            for (hour, secs) in &sitting.hours {
                out[(*hour as usize).min(23)] += secs;
            }
        }
        out
    }

    /// Everything read over `days`, cut by the weekday it fell on, Monday
    /// first.
    pub fn weekdays_over(&self, days: std::ops::RangeInclusive<i64>) -> [i64; 7] {
        let mut out = [0i64; 7];
        for (day, secs) in self.days_in(days.clone()) {
            out[date::weekday(*day)] += secs;
        }
        out
    }

    /// The same, cut by the month of the year it fell in.
    pub fn months_over(&self, days: std::ops::RangeInclusive<i64>) -> [i64; 12] {
        let mut out = [0i64; 12];
        for (day, secs) in self.days_in(days.clone()) {
            let (_, month, _) = date::civil_from_days(*day);
            out[(month - 1).clamp(0, 11) as usize] += secs;
        }
        out
    }

    /// One fold of the record onto a cycle: `values`, each bucket's figure;
    /// `each`, what one turn averages; and the fullest bucket.
    pub fn fold(&self, values: Vec<i64>, each: i64) -> Fold {
        let busiest = values
            .iter()
            .enumerate()
            .max_by_key(|(_, secs)| **secs)
            .filter(|(_, secs)| **secs > 0)
            .map(|(at, _)| at);
        Fold {
            values,
            each,
            busiest,
        }
    }

    /// The first day the record holds, or `today` where it holds none.
    pub fn opened(&self, today: i64) -> i64 {
        self.days.first().map(|(d, _)| *d).unwrap_or(today)
    }

    /// Days from the first the record holds to `today`, both counted.
    pub fn covered(&self, today: i64) -> i64 {
        (today - self.opened(today) + 1).max(1)
    }

    /// The record folded onto one day: the seconds each of the twenty-four
    /// hours holds in an average day read on, every hour over the one divisor.
    pub fn average_day(&self, today: i64) -> Fold {
        let over = self.opened(today)..=today;
        // `days_over` counts the days with reading on them, the `a_day` divisor.
        let days = self.days_over(over.clone()).max(1);
        let hours = self.hours_over(over.clone());
        let values = hours.iter().map(|secs| secs / days).collect();
        self.fold(values, self.tally(over).a_day)
    }

    /// The record folded onto one week, from whichever day `week` starts on:
    /// the seconds each weekday holds in an average week, and their sum.
    pub fn average_week(&self, today: i64, week: WeekStart) -> Fold {
        let over = self.opened(today)..=today;
        let counted = self.weekdays_over(over.clone());
        let mut seen = [0i64; 7];
        for day in over.clone() {
            seen[date::weekday(day)] += 1;
        }
        let values: Vec<i64> = (0..7)
            .map(|column| {
                let day = week.day_in(column);
                counted[day] / seen[day].max(1)
            })
            .collect();
        let each = values.iter().sum();
        self.fold(values, each)
    }

    /// A [`Fold`] by month of the year, summed over every occurrence and
    /// undivided.
    pub fn by_month(&self, today: i64) -> Fold {
        let counted = self.months_over(self.opened(today)..=today);
        let total = counted.iter().sum();
        self.fold(counted.to_vec(), total)
    }

    /// The band [`Self::sitting_floor`] falls in, where the scale opens.
    pub fn first_band(&self) -> usize {
        (self.sitting_floor.seconds() / SITTING_STEP_SECS) as usize
    }

    /// How many sittings ran each length, one count per band of
    /// [`SITTING_STEP_SECS`] from [`Self::first_band`] up, the last holding
    /// everything past the top of the scale.
    pub fn sitting_bands(&self) -> Vec<i64> {
        let first = self.first_band();
        let mut out = vec![0i64; SITTING_BANDS - first];
        for sitting in &self.sittings {
            let at =
                (sitting.seconds / SITTING_STEP_SECS).clamp(first as i64, SITTING_BANDS as i64 - 1);
            out[at as usize - first] += 1;
        }
        out
    }

    /// Seconds read over `days`.
    pub fn span_seconds(&self, days: std::ops::RangeInclusive<i64>) -> i64 {
        self.days_in(days).map(|(_, secs)| secs).sum()
    }

    /// The fullest day inside `days` and what was read on it, the level every
    /// bar over a span of days is banded against. `None` over a span
    /// with no reading in it.
    pub fn busiest_day(&self, days: std::ops::RangeInclusive<i64>) -> Option<(i64, i64)> {
        self.days_in(days).max_by_key(|(_, secs)| *secs).copied()
    }

    /// The longest single sitting of the record: the day it fell on and its
    /// seconds. Of two as long, the later. `None` where the record holds none.
    pub fn longest_sitting(&self) -> Option<(i64, i64)> {
        self.sittings
            .iter()
            .max_by_key(|s| s.seconds)
            .map(|s| (s.day, s.seconds))
    }

    /// What a sitting of the record averaged: [`Self::total_seconds`] over
    /// [`Self::sittings`].
    pub fn a_sitting(&self) -> i64 {
        self.total_seconds / (self.sittings.len() as i64).max(1)
    }

    /// What a span of days came to, stated the same way over the whole
    /// record and over one span.
    pub fn tally(&self, days: std::ops::RangeInclusive<i64>) -> Tally {
        let read = self.span_seconds(days.clone());
        let days_read = self.days_over(days.clone());
        Tally {
            read,
            days_read,
            // `read` divided by `days_read` is `a_day`.
            a_day: read / days_read.max(1),
            finished: self.finished_over(days),
        }
    }

    /// Days of `days` with any reading on them.
    pub fn days_over(&self, days: std::ops::RangeInclusive<i64>) -> i64 {
        self.days_in(days).count() as i64
    }

    /// Books the library states read through whose last sitting falls inside
    /// `days`: the books finished over that span.
    pub fn finished_over(&self, days: std::ops::RangeInclusive<i64>) -> i64 {
        self.finished_in(days).count() as i64
    }

    /// The books read through inside `days`, by their index in [`Self::books`].
    /// The one population every figure over books read through is drawn from.
    pub fn finished_in(&self, days: std::ops::RangeInclusive<i64>) -> impl Iterator<Item = usize> {
        self.books
            .iter()
            .enumerate()
            .filter(move |(_, b)| b.is_finished() && days.contains(&b.last_day))
            .map(|(at, _)| at)
    }

    /// What a book read through took, averaged over every book
    /// [`Self::finished_in`] answers across the record. `None` where none is.
    pub fn a_book(&self) -> Option<i64> {
        let finished: Vec<usize> = self.finished_in(i64::MIN..=i64::MAX).collect();
        let spent: i64 = finished.iter().map(|at| self.books[*at].seconds).sum();
        (!finished.is_empty()).then(|| spent / finished.len() as i64)
    }

    /// Whether the book at `book` counts as finished on `day`: read through,
    /// and `day` is the last it was read on. The rule
    /// [`Self::finished_over`] counts by.
    pub fn finished_on(&self, book: usize, day: i64) -> bool {
        self.books
            .get(book)
            .is_some_and(|b| b.is_finished() && b.last_day == day)
    }

    /// Days with any reading at all.
    pub fn days_read(&self) -> i64 {
        self.days.len() as i64
    }

    /// Weeks back from the one holding `today` with a day read in each, every
    /// week opening on the day `week` starts on, a week with nothing read in it
    /// yet counted from the week before it.
    pub fn weeks_running(&self, today: i64, week: WeekStart) -> i64 {
        let opens = |day: i64| day - week.column_of(date::weekday(day)) as i64;
        let any = |from: i64| (from..from + 7).any(|d| self.day_seconds(d) > 0);
        let mut at = opens(today);
        if !any(at) {
            at -= 7;
        }
        let mut running = 0;
        while any(at) {
            running += 1;
            at -= 7;
        }
        running
    }

    /// Most recently read first, and within a day the one put down last.
    fn sort_books(&mut self) {
        let mut order: Vec<usize> = (0..self.books.len()).collect();
        // Reversed, never negated: [`NO_DAY`] has no negation to take.
        order.sort_by_key(|&i| {
            let b = &self.books[i];
            std::cmp::Reverse((b.last_day, b.last_secs, b.seconds))
        });
        // `Sitting::book` names a slot, which `moved` carries across the sort.
        let mut moved: Vec<usize> = vec![0; order.len()];
        for (to, &from) in order.iter().enumerate() {
            moved[from] = to;
        }
        for s in &mut self.sittings {
            s.book = s.book.map(|b| moved[b]);
        }
        let mut books: Vec<Option<BookStat>> = self.books.drain(..).map(Some).collect();
        self.books = order
            .iter()
            .map(|&from| books[from].take().expect("each book moves once"))
            .collect();
    }
}

/// The cover `found` names, where `covers::drawable` answers true of it, and
/// an empty path where it does not. `Book::cover_file` outlives the file, a
/// retry count and a placeholder alike.
fn cover(found: &record::Book) -> String {
    let picture = found.cover_file();
    match crate::covers::drawable(std::path::Path::new(picture)) {
        true => picture.to_string(),
        false => String::new(),
    }
}

/// A `BookStat` with no sitting credited to it.
fn fresh(extent: i64, found: &record::Book, day: i64) -> BookStat {
    BookStat {
        extent,
        cde_key: found.cde_key.clone(),
        cde_type: found.cde_type.clone(),
        finished: found.finished,
        restarted_on: found.restarted_on.clone(),
        title: found.title.clone(),
        author: found.author.clone(),
        thumbnail: cover(found),
        percent: found.percent,
        on_kindle: found.on_kindle,
        location: found.location.clone(),
        language: found.language.clone(),
        seconds: 0,
        counted_seconds: 0,
        paged_seconds: 0,
        awake_seconds: 0,
        sittings: 0,
        page_turns: 0,
        words: 0,
        days: 0,
        first_day: day,
        last_day: day,
        last_secs: 0,
        covered: 0.0,
        covered_seconds: 0,
        timer_seconds: found.timer_ms / 1000,
        timer_words: found.timer_words,
        timer_covered: found.timer_covered,
        stated_time_left: None,
        stated_wpm: None,
        highlights: Vec::new(),
    }
}

/// Whether `measured` states at least the seconds `counted` states.
fn reaches(measured: i64, counted: i64) -> bool {
    measured > 0 && measured >= counted
}

/// `s.seconds` under [`StatsFrom::Kindle`]. Under [`StatsFrom::App`], the first of
/// `timed_seconds` and `paged_seconds` that [`reaches`] it, else `s.seconds`.
fn sitting_seconds(s: &sitting::Sitting, from: StatsFrom) -> i64 {
    match from {
        StatsFrom::App if reaches(s.timed_seconds, s.seconds) => s.timed_seconds,
        StatsFrom::App if reaches(s.paged_seconds, s.seconds) => s.paged_seconds,
        _ => s.seconds,
    }
}

/// `s.words` under [`StatsFrom::Kindle`]. Under [`StatsFrom::App`], the words of
/// whichever source [`sitting_seconds`] reads, and `s.words` at the least.
fn sitting_words(s: &sitting::Sitting, from: StatsFrom) -> i64 {
    match from {
        StatsFrom::App if reaches(s.timed_seconds, s.seconds) => s.timed_words.max(s.words),
        StatsFrom::App if reaches(s.paged_seconds, s.seconds) => s.paged_words.max(s.words),
        _ => s.words,
    }
}

/// `hours`, which sum to `sum`, rescaled to sum to `target`: each entry in
/// proportion, and the division's remainder to the busiest. An empty `hours`
/// takes [`spread_over`] between `from` and `to`, seconds into the day.
fn hours_at(hours: &[(u8, i64)], sum: i64, target: i64, from: i64, to: i64) -> Vec<(u8, i64)> {
    if hours.is_empty() {
        return spread_over(target, from, to);
    }
    if sum <= 0 || target == sum {
        return hours.to_vec();
    }
    let mut out: Vec<(u8, i64)> = hours
        .iter()
        .map(|(h, secs)| (*h, secs * target / sum))
        .filter(|(_, secs)| *secs > 0)
        .collect();
    let placed: i64 = out.iter().map(|(_, secs)| secs).sum();
    match out.iter_mut().max_by_key(|(_, secs)| *secs) {
        Some(busiest) => busiest.1 += target - placed,
        None => out.push((hours[0].0, target)),
    }
    out
}

/// `secs` across the clock hours between `from` and `to`, seconds into the day:
/// `held` gives each hour its share of the span and the hour `from` falls in
/// the remainder, as `log::sitting` books a page turn.
fn spread_over(secs: i64, from: i64, to: i64) -> Vec<(u8, i64)> {
    let hour = |at: i64| (at / 3600).clamp(0, 23) as usize;
    if secs <= 0 {
        return Vec::new();
    }
    let span = to - from;
    if span <= 0 {
        return vec![(hour(from) as u8, secs)];
    }
    let mut held = [0i64; 24];
    let mut placed = 0;
    for h in (from / 3600)..=((to - 1) / 3600) {
        let overlap = to.min((h + 1) * 3600) - from.max(h * 3600);
        if overlap <= 0 {
            continue;
        }
        let share = secs * overlap / span;
        held[hour(h * 3600)] += share;
        placed += share;
    }
    held[hour(from)] += secs - placed;
    held.iter()
        .enumerate()
        .filter(|(_, secs)| **secs > 0)
        .map(|(at, secs)| (at as u8, *secs))
        .collect()
}

fn credit(book: &mut BookStat, s: &sitting::Sitting, day: i64, secs: i64, words: i64) {
    book.seconds += secs;
    book.counted_seconds += s.seconds;
    match s.measure {
        Measure::Counted | Measure::Timed => {}
        Measure::Paged => book.paged_seconds += secs,
        Measure::Awake => book.awake_seconds += secs,
    }
    book.sittings += 1;
    book.page_turns += s.page_turns;
    book.words += words;
    // `s.covered` and `s.paged_seconds` come off the same pages, and add together.
    if s.covered > 0.0 {
        book.covered += s.covered;
        book.covered_seconds += s.paged_seconds;
    }
    // [`NO_DAY`] takes `day` whole.
    book.first_day = match book.first_day {
        NO_DAY => day,
        held => held.min(day),
    };
    let ended = date::secs_of(&s.ended_at);
    if (day, ended) >= (book.last_day, book.last_secs) {
        book.last_day = day;
        book.last_secs = ended;
        book.stated_time_left = s.time_left.or(book.stated_time_left);
        book.stated_wpm = s.stated_wpm.or(book.stated_wpm);
    }
}

/// The longest streak of consecutive days with reading, and the streak ending at
/// `today`, which accepts a last day of `today` or `today - 1`.
fn streaks(days: &[(i64, i64)], today: i64) -> (i64, i64) {
    let (mut longest, mut streak) = (0, 0);
    let mut previous: Option<i64> = None;
    for (day, _) in days {
        streak = match previous {
            Some(p) if *day == p + 1 => streak + 1,
            _ => 1,
        };
        longest = longest.max(streak);
        previous = Some(*day);
    }
    let current = match previous {
        Some(last) if last == today || last == today - 1 => streak,
        _ => 0,
    };
    (longest, current)
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use crate::clock::Clock;
    use crate::library::Book;

    /// The opening of a JPEG, which is all [`cover`] reads of one.
    const JPEG: &[u8] = b"\xff\xd8\xff\xe0\x00\x10JFIF\0";

    fn day(y: i64, m: i64, d: i64) -> i64 {
        date::days_from_civil(y, m, d)
    }

    fn sitting(at: &str, ended: &str, book: i64, secs: i64, hour: u8) -> sitting::Sitting {
        sitting::Sitting {
            started_at: at.into(),
            ended_at: ended.into(),
            end_position: book,
            seconds: secs,
            page_turns: 4,
            words: 600,
            hours: vec![(hour, secs)],
            measure: Measure::Counted,
            asin: None,
            progress: None,
            ..sitting::Sitting::default()
        }
    }

    fn library() -> Vec<Book> {
        vec![Book {
            extent: 148_209,
            cde_key: "B00OKPCRLG".into(),
            cde_type: "EBOK".into(),
            title: "The Jewish Study Bible".into(),
            author: "Adele Berlin".into(),
            percent: 25.0,
            thumbnail: "/mnt/us/system/thumbnails/t.jpg".into(),
            last_access: 0,
            language: "en".into(),
            location: "/mnt/us/documents/bible.kfx".into(),
            on_kindle: true,
            read_state: -1,
        }]
    }

    /// A day count for a civil date, for the fold assertions below.
    fn at(y: i64, m: i64, d: i64) -> i64 {
        date::days_from_civil(y, m, d)
    }

    /// A record holding one sitting of `secs` on each day named.
    fn on_days(days: &[i64], secs: i64) -> Record {
        let mut record = Record::default();
        for day in days {
            let (y, m, d) = date::civil_from_days(*day);
            record.sittings.push(sitting::Sitting {
                started_at: format!("{y:04}-{m:02}-{d:02}T09:00:00"),
                ended_at: format!("{y:04}-{m:02}-{d:02}T09:{:02}:00", secs / 60),
                end_position: 100,
                seconds: secs,
                page_turns: 1,
                words: 100,
                hours: vec![(9, secs)],
                measure: Measure::Counted,
                asin: None,
                progress: None,
                ..sitting::Sitting::default()
            });
        }
        record
    }
    #[test]
    fn a_marked_book_holds_the_progress_the_library_states() {
        let mut book = fresh(1, &record::Book::default(), 0);
        book.percent = 55.4;
        assert_eq!((book.percent_shown(), book.bar_percent()), (55, 55));
        assert!(!book.is_finished());

        // `finished` fills `bar_percent` and leaves `percent_shown` standing.
        book.finished = true;
        assert_eq!((book.percent_shown(), book.bar_percent()), (55, 100));
        assert!(book.is_finished());

        book.percent = 100.0;
        assert_eq!((book.percent_shown(), book.bar_percent()), (100, 100));
        // `percent` -1 states no figure, and `finished` alone fills the bar.
        book.percent = -1.0;
        assert!(!book.has_percent());
        assert_eq!(book.bar_percent(), 100);
    }

    #[test]
    fn the_readings_counted_are_the_banked_ones_and_the_one_standing() {
        let mut book = fresh(1, &record::Book::default(), 0);
        // Never read through, and read through once without a restart.
        assert_eq!(book.times_finished(), 0);
        book.finished = true;
        assert_eq!(book.times_finished(), 1);
        // `restarted_on` banks that reading, and `finished` clears.
        book.finished = false;
        book.restarted_on = vec![0];
        assert_eq!(
            book.times_finished(),
            1,
            "the reading that ended still counts"
        );
        // And through the end of the second.
        book.finished = true;
        assert_eq!(book.times_finished(), 2);
    }

    #[test]
    fn the_mark_is_the_readers_only_short_of_the_threshold() {
        let mut book = fresh(1, &record::Book::default(), 0);
        for (percent, highlights, restarts) in [
            (-1.0, true, false),
            (0.0, true, false),
            (55.4, true, true),
            (99.4, true, true),
            (99.5, false, true),
            (100.0, false, true),
        ] {
            book.percent = percent;
            book.finished = percent >= FINISHED_PERCENT;
            assert_eq!(
                book.can_mark(),
                highlights,
                "{percent}% is the reader's to mark or not"
            );
            assert_eq!(
                book.can_restart(),
                restarts,
                "{percent}% has progress or not"
            );
        }
        // `finished` set by hand holds `can_mark` and `can_restart` true.
        book.percent = -1.0;
        book.finished = true;
        assert!(book.can_mark() && book.can_restart());
    }

    #[test]
    fn a_bar_and_the_figure_beside_it_state_one_progress() {
        let stored = record::Book::default();
        let mut book = fresh(1, &stored, 0);
        // `raw` straddles the rounding in each pair, where truncating and
        // rounding part company.
        for (raw, shown) in [(99.6, 100), (99.4, 99), (0.4, 0), (100.0, 100)] {
            book.percent = raw;
            assert_eq!(book.percent_shown(), shown, "{raw}");
        }
    }

    #[test]
    fn every_month_the_record_touches_states_what_was_read_in_it() {
        // `days` from 27 July to 4 September: five days of July, all of
        // August, four of September, each month stating its own reading
        // whatever share of it the record holds.
        let days: Vec<i64> = (at(2026, 7, 27)..=at(2026, 9, 4)).collect();
        let today = at(2026, 9, 5);
        let stats = Stats::build(
            &on_days(&days, 3600),
            today,
            true,
            StatsFrom::Kindle,
            SittingFloor::All,
        );

        let fold = stats.by_month(today);
        assert_eq!(fold.values[6], 5 * 3600, "five days of July");
        assert_eq!(fold.values[7], 31 * 3600, "August whole");
        assert_eq!(fold.values[8], 4 * 3600, "four days of September");
        assert_eq!(fold.busiest, Some(7));
        assert_eq!(fold.each, 40 * 3600, "the twelve months are the record");
    }

    #[test]
    fn the_board_and_a_span_state_the_same_days_the_same_way() {
        // `days`, ten of an hour with three empty, holds days read apart from
        // days covered, two figures that cannot agree by accident.
        let days: Vec<i64> = (0..10)
            .filter(|i| i % 4 != 0)
            .map(|i| at(2026, 3, 1) + i)
            .collect();
        let today = at(2026, 3, 11);
        let stats = Stats::build(
            &on_days(&days, 3600),
            today,
            true,
            StatsFrom::Kindle,
            SittingFloor::All,
        );

        let whole = stats.tally(stats.opened(today)..=today);
        let march = stats.tally(at(2026, 3, 1)..=at(2026, 3, 31));
        assert_eq!(whole.read, march.read);
        assert_eq!(whole.days_read, march.days_read);
        assert_eq!(whole.a_day, march.a_day);
        assert_eq!(whole.days_read, days.len() as i64);
        // The figure beside it: what was read over the days it was read on.
        assert_eq!(whole.a_day, whole.read / whole.days_read);
    }

    #[test]
    fn a_week_not_yet_read_in_leaves_the_weeks_running_on_the_one_before() {
        // 2026-03-02 is a Monday: three weeks read in, one day apiece.
        let days = [at(2026, 3, 3), at(2026, 3, 11), at(2026, 3, 20)];
        let stats = Stats::build(
            &on_days(&days, 600),
            at(2026, 3, 20),
            true,
            StatsFrom::Kindle,
            SittingFloor::All,
        );
        let running = |today: i64| stats.weeks_running(today, WeekStart::Monday);
        assert_eq!(running(at(2026, 3, 20)), 3);
        // The Monday after, with nothing read in its week yet.
        assert_eq!(running(at(2026, 3, 23)), 3);
        // `at(2026, 3, 30)` follows a whole week with nothing read: the run ends.
        assert_eq!(running(at(2026, 3, 30)), 0);
    }

    #[test]
    fn a_day_states_the_progress_its_own_last_sitting_reached() {
        let mut record = Record::default();
        record.books.push(record::Book {
            extent: 100,
            cde_key: "B01".into(),
            percent: 62.0,
            ..record::Book::default()
        });
        // Two days, two sittings each, climbing 20 → 31 → 44 → 62.
        for (day, at, progress) in [
            ("2026-08-07", "09:00:00", 0.20),
            ("2026-08-07", "21:00:00", 0.31),
            ("2026-08-08", "09:00:00", 0.44),
            ("2026-08-08", "21:00:00", 0.62),
        ] {
            record.sittings.push(sitting::Sitting {
                started_at: format!("{day}T{at}"),
                ended_at: format!("{day}T{at}"),
                end_position: 100,
                seconds: 600,
                page_turns: 1,
                words: 100,
                hours: vec![(9, 600)],
                measure: Measure::Counted,
                asin: None,
                progress: Some(progress),
                ..sitting::Sitting::default()
            });
        }
        let stats = Stats::build(
            &record,
            day(2026, 8, 8),
            true,
            StatsFrom::Kindle,
            SittingFloor::All,
        );
        let (first, last) = (day(2026, 8, 7), day(2026, 8, 8));
        assert_eq!(stats.percent_over(0, first..=first), Some(31));
        assert_eq!(stats.percent_over(0, last..=last), Some(62));
        assert_eq!(stats.percent_over(0, first..=last), Some(62));
        // `first - 1` holds no sitting of this book, and no per cent.
        assert_eq!(stats.percent_over(0, first - 1..=first - 1), None);

        // `progress` `None` on a sitting leaves the one before it standing.
        record.sittings[3].progress = None;
        let stats = Stats::build(
            &record,
            day(2026, 8, 8),
            true,
            StatsFrom::Kindle,
            SittingFloor::All,
        );
        assert_eq!(stats.percent_over(0, last..=last), Some(44));
    }

    #[test]
    fn a_book_read_through_states_the_librarys_figure_on_the_day_it_ended() {
        // A book read through on 5 September, `progress` last at 0.994690.
        let mut record = Record::default();
        record.books.push(record::Book {
            extent: 142_294,
            cde_key: "J5LSDTFS".into(),
            percent: 100.0,
            finished: true,
            ..record::Book::default()
        });
        for (at, progress) in [("08:54:34", 0.603540), ("22:36:42", 0.994690)] {
            record.sittings.push(sitting::Sitting {
                started_at: format!("2026-09-05T{at}"),
                ended_at: format!("2026-09-05T{at}"),
                end_position: 142_294,
                seconds: 600,
                page_turns: 1,
                words: 100,
                hours: vec![(8, 600)],
                measure: Measure::Counted,
                asin: None,
                progress: Some(progress),
                ..sitting::Sitting::default()
            });
        }
        let ended = day(2026, 9, 5);
        let stats = Stats::build(&record, ended, true, StatsFrom::Kindle, SittingFloor::All);
        assert_eq!(stats.books[0].percent_shown(), 100);
        assert_eq!(stats.percent_over(0, ended..=ended), Some(100));
        // `ended + 1` as `today` leaves `ended` the last day, at the library's 100.
        let stats = Stats::build(
            &record,
            ended + 1,
            true,
            StatsFrom::Kindle,
            SittingFloor::All,
        );
        assert_eq!(stats.percent_over(0, ended..=ended), Some(100));
    }

    #[test]
    fn a_half_per_cent_rounds_to_even_the_way_the_kindle_rounds_it() {
        // `whole_per_cent` rounds halves to even.
        for (raw, shown) in [
            (46.5, 46),
            (47.5, 48),
            (0.5, 0),
            (99.5, 100),
            (99.469, 99),
            (46.4, 46),
            (46.6, 47),
        ] {
            assert_eq!(whole_per_cent(raw), shown, "{raw}");
        }
    }

    #[test]
    fn what_was_read_can_be_ordered_by_what_was_put_down_last() {
        // Two books: one read longer, the other read later.
        let mut record = Record::default();
        let long = at(2026, 3, 1);
        let late = at(2026, 3, 5);
        for (pos, name) in [(100, "The Long One"), (200, "The Late One")] {
            record.books.push(record::Book {
                extent: pos,
                cde_key: format!("KEY{pos}"),
                title: name.into(),
                ..record::Book::default()
            });
        }
        for (day, secs, pos) in [(long, 7200, 100), (late, 600, 200)] {
            let (y, m, d) = date::civil_from_days(day);
            record.sittings.push(sitting::Sitting {
                started_at: format!("{y:04}-{m:02}-{d:02}T09:00:00"),
                ended_at: format!("{y:04}-{m:02}-{d:02}T10:00:00"),
                end_position: pos,
                seconds: secs,
                page_turns: 1,
                words: 100,
                hours: vec![(9, secs)],
                measure: Measure::Counted,
                asin: None,
                progress: None,
                ..sitting::Sitting::default()
            });
        }
        let today = at(2026, 3, 6);
        let stats = Stats::build(&record, today, true, StatsFrom::Kindle, SittingFloor::All);
        let over = long..=today;

        let longest = stats.book_totals(over.clone());
        let recent = stats.book_totals_recent(over);
        assert_eq!(longest.len(), 2);
        assert_eq!(longest[0].1, 7200, "longest first states the long one");
        assert_eq!(recent[0].1, 600, "most recent first states the late one");
        assert_ne!(longest[0].0, recent[0].0);
        assert_eq!(stats.books[recent[0].0].title, "The Late One");
    }

    #[test]
    fn a_book_opened_and_shut_contributes_nothing_the_floor_excludes() {
        let today = at(2026, 3, 2);
        let day = at(2026, 3, 1);
        // A thirty-second skim and a two-minute sitting, one day, one book.
        let mut record = on_days(&[day], 30);
        record.sittings.extend(on_days(&[day], 120).sittings);
        let counted = |floor| Stats::build(&record, today, true, StatsFrom::Kindle, floor);

        let all = counted(SittingFloor::All);
        assert_eq!(all.sittings.len(), 2);
        assert_eq!(all.total_seconds, 150);
        assert_eq!(all.day_seconds(day), 150);

        let minute = counted(SittingFloor::OneMinute);
        assert_eq!(minute.sittings.len(), 1, "the skim is not a sitting");
        assert_eq!(minute.total_seconds, 120, "nor is it in the total");
        assert_eq!(minute.day_seconds(day), 120);
        assert_eq!(minute.total_turns, 1, "nor are its page turns");
        assert_eq!(minute.total_words, 100, "nor its words");
        assert_eq!(minute.sitting_bands()[0], 1);

        // `SittingFloor::FifteenMinutes`, above every sitting, leaves `day` unread.
        let quarter = counted(SittingFloor::FifteenMinutes);
        assert!(quarter.sittings.is_empty());
        assert_eq!(quarter.total_seconds, 0);
        assert_eq!(quarter.day_seconds(day), 0);
        assert_eq!(quarter.days_read(), 0);
        assert_eq!(quarter.longest_streak, 0);
    }

    #[test]
    fn the_floor_reads_the_clock_the_stats_are_taken_from() {
        // 40 counted seconds and 180 `timed_seconds`: a skim under
        // [`StatsFrom::Kindle`], a sitting under [`StatsFrom::App`].
        let day = at(2026, 3, 1);
        let mut record = on_days(&[day], 40);
        record.sittings[0].timed_seconds = 180;
        let counted =
            |from| Stats::build(&record, at(2026, 3, 2), true, from, SittingFloor::OneMinute);
        assert!(counted(StatsFrom::Kindle).sittings.is_empty());
        assert_eq!(counted(StatsFrom::App).sittings.len(), 1);
        assert_eq!(counted(StatsFrom::App).total_seconds, 180);
    }

    /// [`StatsFrom::App`] stands on `timed_seconds`, and on `paged_seconds`
    /// without them.
    #[test]
    fn app_stats_take_the_turn_lines_over_the_pages() {
        let day = at(2026, 3, 1);
        let mut record = on_days(&[day], 40);
        record.sittings[0].timed_seconds = 300;
        record.sittings[0].paged_seconds = 900;
        let counted = |record: &Record, from| {
            Stats::build(record, at(2026, 3, 2), true, from, SittingFloor::OneMinute).total_seconds
        };
        assert_eq!(counted(&record, StatsFrom::Kindle), 0, "40 s is a skim");
        assert_eq!(counted(&record, StatsFrom::App), 300);

        // A sitting whose stack writes no turn line.
        record.sittings[0].timed_seconds = 0;
        assert_eq!(counted(&record, StatsFrom::App), 900);

        // And one neither source states anything for.
        record.sittings[0].paged_seconds = 0;
        assert_eq!(counted(&record, StatsFrom::App), 0);
    }

    /// `timed_seconds` under `seconds` hands the sitting to `paged_seconds`,
    /// and `paged_seconds` under `seconds` hands it to `seconds`.
    #[test]
    fn a_measure_answers_for_app_stats_only_where_it_reaches_the_counter() {
        let day = at(2026, 3, 1);
        let mut record = on_days(&[day], 455);
        record.books.push(crate::record::Book {
            extent: 100,
            title: "A Book".into(),
            ..Default::default()
        });
        record.sittings[0].words = 1_819;
        record.sittings[0].timed_seconds = 5;
        record.sittings[0].timed_words = 283;
        let built = |record: &Record, from| {
            Stats::build(record, at(2026, 3, 2), true, from, SittingFloor::OneMinute)
        };

        // 455 is past both sources: `seconds` answers under both.
        let app = built(&record, StatsFrom::App);
        assert_eq!((app.total_seconds, app.total_words), (455, 1_819));
        assert_eq!(built(&record, StatsFrom::Kindle).total_seconds, 455);

        // `paged_seconds` reaches 455 and answers with its own 600.
        record.sittings[0].paged_seconds = 600;
        record.sittings[0].paged_words = 2_000;
        let app = built(&record, StatsFrom::App);
        assert_eq!((app.total_seconds, app.total_words), (600, 2_000));
        assert_eq!(
            app.books[0].wpm(StatsFrom::App),
            Some(200),
            "one source, both sides"
        );

        // `paged_seconds` at 448 falls under `seconds`.
        record.sittings[0].paged_seconds = 448;
        let app = built(&record, StatsFrom::App);
        assert_eq!((app.total_seconds, app.total_words), (455, 1_819));

        // `timed_seconds` at 700 reaches `seconds` and heads the chain.
        record.sittings[0].timed_seconds = 700;
        record.sittings[0].timed_words = 2_400;
        record.sittings[0].paged_seconds = 600;
        let app = built(&record, StatsFrom::App);
        assert_eq!((app.total_seconds, app.total_words), (700, 2_400));
    }

    #[test]
    fn the_app_words_come_off_the_pages_and_the_rate_divides_by_app_seconds() {
        let day = at(2026, 3, 1);
        let mut record = on_days(&[day], 600);
        record.books.push(crate::record::Book {
            extent: 100,
            title: "A Book".into(),
            ..Default::default()
        });
        record.sittings[0].words = 1_000;
        record.sittings[0].paged_words = 3_000;
        record.sittings[0].paged_seconds = 600;
        let built = |record: &Record, from| {
            Stats::build(record, at(2026, 3, 2), true, from, SittingFloor::OneMinute)
        };
        let kindle = built(&record, StatsFrom::Kindle);
        let app = built(&record, StatsFrom::App);
        assert_eq!(kindle.total_words, 1_000);
        assert_eq!(app.total_words, 3_000, "the pages, not the counter");
        assert_eq!(kindle.books[0].wpm(StatsFrom::Kindle), Some(100));
        assert_eq!(app.books[0].wpm(StatsFrom::App), Some(300));

        // `paged_words` 0 keeps the counter's words.
        record.sittings[0].paged_words = 0;
        assert_eq!(built(&record, StatsFrom::App).total_words, 1_000);
    }

    #[test]
    fn a_book_reached_by_nothing_but_skims_is_listed_at_zero() {
        // `148_207`'s sittings cut under a minute, and the unidentified book's
        // left long.
        let mut record = record();
        for s in &mut record.sittings {
            if s.end_position == 148_207 {
                s.seconds = 30;
                s.hours = vec![(9, 30)];
            }
        }
        let today = day(2026, 8, 7);
        let stats = Stats::build(
            &record,
            today,
            true,
            StatsFrom::Kindle,
            SittingFloor::OneMinute,
        );

        let book = stats
            .books
            .iter()
            .find(|b| b.extent == 148_209)
            .expect("a book skimmed is still a book");
        assert_eq!(
            (book.sittings, book.seconds, book.days, book.page_turns),
            (0, 0, 0, 0),
            "and it is listed at zero"
        );
        assert_eq!(book.title, "The Jewish Study Bible");
        assert_eq!(book.last_day, NO_DAY, "it stands on no day");
        assert!(
            stats.sittings.iter().all(|s| s.book.is_none()),
            "and no sitting of it survived"
        );
        // `1_800`, the one long sitting, is the whole of the record's reading.
        assert_eq!(stats.total_seconds, 1_800);
    }

    #[test]
    fn a_skim_before_the_reading_does_not_take_the_books_first_day() {
        // `record.sittings[0]`, a skim, opens the row on `NO_DAY`, and the
        // sitting after it gives the book a real first day, not `i64::MIN`.
        let mut record = record();
        record.sittings[0].seconds = 30;
        record.sittings[0].hours = vec![(9, 30)];
        let stats = Stats::build(
            &record,
            day(2026, 8, 7),
            true,
            StatsFrom::Kindle,
            SittingFloor::OneMinute,
        );
        let book = &stats.books[0];
        assert_eq!(book.first_day, day(2026, 8, 6));
        assert_eq!(book.last_day, day(2026, 8, 7));
        assert_eq!(book.sittings, 2);
    }

    #[test]
    fn the_scale_opens_on_the_band_the_floor_stands_in() {
        let day = at(2026, 3, 1);
        // 30 s, 3 min, 8 min, 20 min.
        let mut record = on_days(&[day], 30);
        for secs in [180, 480, 1_200] {
            record.sittings.extend(on_days(&[day], secs).sittings);
        }
        let bands = |floor| {
            Stats::build(&record, at(2026, 3, 2), true, StatsFrom::Kindle, floor).sitting_bands()
        };

        // `bands` under each floor drops the sittings under it and none above.
        let all = bands(SittingFloor::All);
        assert_eq!(all.len(), SITTING_BANDS);
        assert_eq!(
            (all[0], all[1], all[4]),
            (2, 1, 1),
            "30 s and 3 min share a band"
        );

        let minute = bands(SittingFloor::OneMinute);
        assert_eq!(minute.len(), SITTING_BANDS, "a minute is inside band 0");
        assert_eq!((minute[0], minute[1], minute[4]), (1, 1, 1));

        let five = bands(SittingFloor::FiveMinutes);
        assert_eq!(
            five.len(),
            SITTING_BANDS - 1,
            "the band under it is not drawn"
        );
        assert_eq!((five[0], five[3]), (1, 1));

        let quarter = bands(SittingFloor::FifteenMinutes);
        assert_eq!(quarter.len(), SITTING_BANDS - 3);
        assert_eq!(quarter.iter().sum::<i64>(), 1, "only the twenty-minute one");
        assert_eq!(quarter[1], 1);
    }

    #[test]
    fn an_average_day_is_the_figure_the_board_states_beside_it() {
        // Ten days of an hour, three of them empty: `a day` is what was read
        // over the days it was read on, and one number wherever it is stated.
        let days: Vec<i64> = (0..10)
            .filter(|i| i % 4 != 0)
            .map(|i| at(2026, 3, 1) + i)
            .collect();
        let today = at(2026, 3, 10);
        let stats = Stats::build(
            &on_days(&days, 3600),
            today,
            true,
            StatsFrom::Kindle,
            SittingFloor::All,
        );

        let board = stats.tally(stats.opened(today)..=today);
        let fold = stats.average_day(today);
        assert_eq!(fold.each, board.a_day);
        assert_eq!(fold.each, 3600, "an hour on a day read on");
        assert_eq!(
            fold.values.iter().sum::<i64>(),
            fold.each,
            "the hours of the fold are the day it states"
        );
    }

    /// `BookStat::sittings` and [`Stats::book_sittings`] count one population,
    /// whatever floor [`Stats::build`] took, the one every figure over a
    /// book's sittings reads.
    #[test]
    fn a_book_states_one_count_of_its_sittings() {
        for floor in SittingFloor::ALL {
            let stats = Stats::build(&record(), at(2026, 3, 9), true, StatsFrom::Kindle, floor);
            for (at, book) in stats.books.iter().enumerate() {
                assert_eq!(
                    stats.book_sittings(at).count() as i64,
                    book.sittings,
                    "{floor:?} book {at}"
                );
                assert_eq!(stats.book_progress(at).len() as i64, book.sittings);
                // `book_stretches` walks in step with `book_progress`, a
                // band's bars and its marks each drawn off one sitting.
                assert_eq!(stats.book_stretches(at).len() as i64, book.sittings);
                assert_eq!(stats.book_days(at).len() as i64, book.days);
                let whole = i64::MIN..=i64::MAX;
                assert_eq!(
                    stats.book_hours(at, whole).iter().sum::<i64>(),
                    book.seconds
                );
                assert_eq!(
                    stats.book_days(at).iter().map(|(_, s)| s).sum::<i64>(),
                    book.seconds
                );
            }
        }
    }

    /// `hours_over`, `weekdays_over` and `months_over` each hold the whole of
    /// [`Stats::span_seconds`] over the same days.
    #[test]
    fn every_cut_of_a_span_holds_the_span_whole() {
        let today = at(2026, 3, 9);
        for floor in SittingFloor::ALL {
            let stats = Stats::build(&record(), today, true, StatsFrom::Kindle, floor);
            for span in [
                i64::MIN..=i64::MAX,
                stats.opened(today)..=today,
                today - 6..=today,
                today..=today,
            ] {
                let whole = stats.span_seconds(span.clone());
                let cuts = [
                    ("hours", stats.hours_over(span.clone()).iter().sum::<i64>()),
                    ("weekdays", stats.weekdays_over(span.clone()).iter().sum()),
                    ("months", stats.months_over(span.clone()).iter().sum()),
                ];
                for (name, cut) in cuts {
                    assert_eq!(cut, whole, "{floor:?} {name} over {span:?}");
                }
            }
        }
    }

    /// `hours_over` holds every second of the day it cuts, a `Sitting` whose
    /// `hours` the record leaves empty included.
    #[test]
    fn a_sitting_stating_no_hour_reaches_the_clock() {
        let day = at(2026, 3, 4);
        let (y, m, d) = date::civil_from_days(day);
        let mut record = on_days(&[day], 3600);
        for (from, to, secs) in [
            ("00:00:00", "21:44:19", 615),
            ("09:11:08", "19:28:58", 1516),
        ] {
            record.sittings.push(sitting::Sitting {
                started_at: format!("{y:04}-{m:02}-{d:02}T{from}"),
                ended_at: format!("{y:04}-{m:02}-{d:02}T{to}"),
                end_position: 100,
                seconds: secs,
                page_turns: 1,
                words: 100,
                hours: Vec::new(),
                measure: Measure::Counted,
                asin: None,
                progress: None,
                ..sitting::Sitting::default()
            });
        }
        let stats = Stats::build(&record, day, true, StatsFrom::Kindle, SittingFloor::All);

        let clock = stats.hours_over(day..=day);
        assert_eq!(clock.iter().sum::<i64>(), 3600 + 615 + 1516);
        assert_eq!(
            clock.iter().sum::<i64>(),
            stats.days[0].1,
            "the clock holds the day it decomposes"
        );
        // `clock` stands inside the windows the rows ran: 22 and 23 stand empty.
        assert!(clock[22] == 0 && clock[23] == 0);
        assert!(clock[9] > 0 && clock[19] > 0);
    }

    #[test]
    fn spread_over_places_every_second_inside_the_span() {
        // `hour` gives seconds into the day.
        let hour = |at: i64| at * 3600;
        assert_eq!(spread_over(600, hour(9), hour(10)), vec![(9, 600)]);
        assert_eq!(
            spread_over(600, hour(9), hour(11)),
            vec![(9, 300), (10, 300)]
        );
        // `spread_over` holds a span of nothing at `from`.
        assert_eq!(spread_over(600, hour(9), hour(9)), vec![(9, 600)]);
        assert_eq!(spread_over(0, hour(9), hour(11)), Vec::new());

        // `whole` adds back up across a day of span over a prime.
        let whole = spread_over(3607, 0, 86_400);
        assert_eq!(whole.iter().map(|(_, secs)| secs).sum::<i64>(), 3607);
        assert_eq!(whole.len(), 24);
    }

    #[test]
    fn an_average_week_is_the_weekdays_the_band_under_it_draws() {
        // An hour on one weekday, twice, over a nine-day record: `seen` counts
        // that weekday twice and every other column stands empty.
        let days = [at(2026, 3, 1), at(2026, 3, 8)];
        let today = at(2026, 3, 9);
        let stats = Stats::build(
            &on_days(&days, 3600),
            today,
            true,
            StatsFrom::Kindle,
            SittingFloor::All,
        );

        for week in [WeekStart::Monday, WeekStart::Sunday] {
            let fold = stats.average_week(today, week);
            assert_eq!(fold.values.len(), 7);
            assert_eq!(
                fold.values.iter().filter(|secs| **secs > 0).count(),
                1,
                "{week:?}: one weekday was read on"
            );
            assert_eq!(
                fold.values.iter().sum::<i64>(),
                fold.each,
                "{week:?}: the weekdays of the fold are the week it states"
            );
            assert_eq!(fold.each, 3600, "{week:?}: an hour on that weekday");
        }
    }

    #[test]
    fn a_month_of_the_year_sums_every_occurrence_the_record_holds() {
        // `days` holds two Augusts of an hour each against one October of an
        // hour: two hours against one.
        let days = [
            at(2023, 7, 1),
            at(2023, 8, 10),
            at(2024, 8, 10),
            at(2023, 10, 10),
        ];
        let today = at(2024, 9, 1);
        let stats = Stats::build(
            &on_days(&days, 3600),
            today,
            true,
            StatsFrom::Kindle,
            SittingFloor::All,
        );
        let fold = stats.by_month(today);
        assert_eq!(fold.values[7], 7200, "two Augusts");
        assert_eq!(fold.values[9], 3600, "one October");
    }

    #[test]
    fn every_hour_of_an_average_day_divides_by_the_days_covered() {
        // `days`, ten of an hour read in the ninth hour: an average day of
        // that hour and nothing else.
        let days: Vec<i64> = (0..10).map(|i| at(2026, 3, 1) + i).collect();
        let today = at(2026, 3, 10);
        let stats = Stats::build(
            &on_days(&days, 3600),
            today,
            true,
            StatsFrom::Kindle,
            SittingFloor::All,
        );
        let fold = stats.average_day(today);
        assert_eq!(fold.values[9], 3600, "an hour a day in the ninth hour");
        assert_eq!(fold.values.iter().sum::<i64>(), 3600);
        assert_eq!(fold.each, 3600, "an average day is that one hour");
        assert_eq!(fold.busiest, Some(9));
    }

    #[test]
    fn a_sitting_past_the_top_of_the_scale_lands_in_the_last_band() {
        let today = at(2026, 3, 2);
        let long = SITTING_STEP_SECS * SITTING_BANDS as i64 * 3;
        let stats = Stats::build(
            &on_days(&[at(2026, 3, 1)], long),
            today,
            true,
            StatsFrom::Kindle,
            SittingFloor::All,
        );
        let bands = stats.sitting_bands();
        assert_eq!(bands.len(), SITTING_BANDS);
        assert_eq!(bands[SITTING_BANDS - 1], 1, "the overflow band holds it");
        assert_eq!(bands.iter().sum::<i64>(), 1, "and holds it only once");
    }

    #[test]
    fn a_fold_over_an_empty_record_names_no_fullest_bucket() {
        let today = at(2026, 3, 2);
        let stats = Stats::build(
            &Record::default(),
            today,
            true,
            StatsFrom::Kindle,
            SittingFloor::All,
        );
        for fold in [
            stats.average_day(today),
            stats.average_week(today, WeekStart::Monday),
            stats.by_month(today),
        ] {
            assert!(fold.busiest.is_none());
            assert_eq!(fold.each, 0);
        }
    }

    /// Two books over three days, one of which the library names.
    fn record() -> Record {
        let mut out = bare_record();
        out.remember(&library());
        out
    }

    /// The same sittings, with nothing ever remembered about them.
    fn bare_record() -> Record {
        Record {
            sittings: vec![
                sitting(
                    "2026-08-05T09:00:00",
                    "2026-08-05T09:30:00",
                    148_207,
                    1_800,
                    9,
                ),
                sitting(
                    "2026-08-06T21:00:00",
                    "2026-08-06T21:20:00",
                    148_207,
                    1_200,
                    21,
                ),
                sitting("2026-08-06T22:00:00", "2026-08-06T22:30:00", 555, 1_800, 22),
                sitting(
                    "2026-08-07T09:00:00",
                    "2026-08-07T09:10:00",
                    148_207,
                    600,
                    9,
                ),
            ],
            // `ends` takes the page events' 148207 to the library's 148209.
            ends: vec![(148_207, 148_209)],
            keys: Vec::new(),
            counters: Vec::new(),
            pairs: Vec::new(),
            books: Vec::new(),
            watermark: "260807:091000".into(),
            watermark_offset: None,
            reset_at: String::new(),
            cleared: Vec::new(),
            highlights: Vec::new(),
            gate: None,
            sources: None,
            clock: Clock::default(),
        }
    }

    #[test]
    fn a_book_cleared_and_kept_is_listed_at_zero() {
        let mut record = record();
        let before = Stats::build(
            &record,
            day(2026, 8, 7),
            true,
            StatsFrom::Kindle,
            SittingFloor::All,
        );
        assert_eq!(before.books[0].sittings, 3);

        record.clear_book(148_209, "B00OKPCRLG");
        let after = Stats::build(
            &record,
            day(2026, 8, 7),
            true,
            StatsFrom::Kindle,
            SittingFloor::All,
        );
        let book = after
            .books
            .iter()
            .find(|b| b.extent == 148_209)
            .expect("the book, kept");
        assert_eq!(book.title, "The Jewish Study Bible");
        assert_eq!(
            (book.sittings, book.seconds, book.days, book.page_turns),
            (0, 0, 0, 0)
        );
        assert_eq!(book.first_day, NO_DAY, "no day to draw");
        assert_eq!(
            after.total_seconds,
            before.total_seconds - before.books[0].seconds,
            "the reading went out of every total"
        );
    }

    #[test]
    fn a_book_forgotten_is_listed_no_longer() {
        let mut record = record();
        record.forget_book(148_209, "B00OKPCRLG");
        let after = Stats::build(
            &record,
            day(2026, 8, 7),
            true,
            StatsFrom::Kindle,
            SittingFloor::All,
        );
        assert!(after.books.is_empty(), "the book went with the reading");
    }

    #[test]
    fn a_book_cleared_and_kept_sorts_under_one_with_reading() {
        let mut record = record();
        record.books.push(record::Book {
            extent: 900_001,
            cde_key: "B00KEPT".into(),
            title: "Kept".into(),
            kept: true,
            ..record::Book::default()
        });
        let stats = Stats::build(
            &record,
            day(2026, 8, 7),
            true,
            StatsFrom::Kindle,
            SittingFloor::All,
        );
        assert_eq!(stats.books.len(), 2);
        assert_eq!(stats.books[0].extent, 148_209, "the one with reading first");
        assert_eq!(stats.books[1].title, "Kept");
    }

    #[test]
    fn a_sitting_is_joined_to_the_book_the_library_names() {
        let stats = Stats::build(
            &record(),
            day(2026, 8, 7),
            true,
            StatsFrom::Kindle,
            SittingFloor::All,
        );
        // `stats.books` holds one of the two books the sittings name.
        assert_eq!(stats.books.len(), 1);
        let bible = stats
            .books
            .iter()
            .find(|b| b.extent == 148_209)
            .expect("the library's book");
        assert_eq!(bible.title, "The Jewish Study Bible");
        assert_eq!(bible.seconds, 1_800 + 1_200 + 600);
        assert_eq!(bible.sittings, 3);
        assert_eq!(bible.days, 3);
        assert_eq!(bible.page_turns, 12);
    }

    #[test]
    fn the_books_nothing_names_are_counted_and_timed() {
        let stats = Stats::build(
            &record(),
            day(2026, 8, 7),
            true,
            StatsFrom::Kindle,
            SittingFloor::All,
        );
        let sixth = day(2026, 8, 6);
        // 555 is the one key no book carries, read once on the sixth.
        assert_eq!(stats.unidentified_over(sixth..=sixth), (1, 1_800));
        assert_eq!(stats.unidentified_books(), 1);
        // `seventh` holds identified reading alone, and counts none.
        let seventh = day(2026, 8, 7);
        assert_eq!(stats.unidentified_over(seventh..=seventh), (0, 0));
    }

    #[test]
    fn two_sittings_on_one_unidentified_key_count_as_one_book() {
        let mut s = record();
        // A second sitting on 555, the day after the first.
        s.sittings.push(sitting::Sitting {
            started_at: "2026-08-07T09:00:00".into(),
            ended_at: "2026-08-07T09:30:00".into(),
            end_position: 555,
            seconds: 900,
            page_turns: 3,
            words: 700,
            hours: vec![(9, 900)],
            measure: Measure::Counted,
            asin: None,
            progress: None,
            ..sitting::Sitting::default()
        });
        let stats = Stats::build(
            &s,
            day(2026, 8, 7),
            true,
            StatsFrom::Kindle,
            SittingFloor::All,
        );
        assert_eq!(stats.unidentified_books(), 1);
        assert_eq!(stats.unidentified_seconds, 1_800 + 900);
    }

    #[test]
    fn dropping_the_unidentified_leaves_every_total_over_the_books_listed() {
        let sixth = day(2026, 8, 6);
        let kept = Stats::build(
            &record(),
            day(2026, 8, 7),
            true,
            StatsFrom::Kindle,
            SittingFloor::All,
        );
        let dropped = Stats::build(
            &record(),
            day(2026, 8, 7),
            false,
            StatsFrom::Kindle,
            SittingFloor::All,
        );

        assert_eq!(dropped.unidentified_seconds, 0);
        assert_eq!(dropped.unidentified_books(), 0);
        assert_eq!(kept.total_seconds - dropped.total_seconds, 1_800);
        assert_eq!(kept.day_seconds(sixth) - dropped.day_seconds(sixth), 1_800);
        assert_eq!(dropped.sittings.len(), kept.sittings.len() - 1);
        assert!(dropped.sittings.iter().all(|s| s.book.is_some()));
        // `books` stands untouched by the drop.
        assert_eq!(dropped.books, kept.books);
        // `book_count` holds the unidentified key; `books` holds none of it.
        assert_eq!(kept.book_count(), dropped.book_count() + 1);
        assert_eq!(dropped.book_count(), dropped.books.len());
    }

    #[test]
    fn a_book_nothing_names_gets_no_row_and_keeps_its_time() {
        let stats = Stats::build(
            &record(),
            day(2026, 8, 7),
            true,
            StatsFrom::Kindle,
            SittingFloor::All,
        );
        assert!(!stats.books.iter().any(|b| b.extent == 555));
        assert!(!stats.books.iter().any(|b| b.title.starts_with("Book at")));
        assert_eq!(stats.unidentified_seconds, 1_800);
        // `unidentified_seconds` reaches the day and the all-time total.
        assert_eq!(stats.total_seconds, 1_800 + 1_200 + 1_800 + 600);
        assert_eq!(stats.day_seconds(day(2026, 8, 6)), 1_200 + 1_800);
        // `sittings_on` holds its sitting, pointing at no book.
        let unidentified = stats
            .sittings_on(day(2026, 8, 6))
            .find(|s| s.book.is_none())
            .expect("the sitting on the unidentified book");
        assert_eq!(unidentified.seconds, 1_800);
    }

    #[test]
    fn a_day_counts_only_identified_books() {
        // 2026-08-06 holds one sitting on the identified book and one on neither.
        let sixth = day(2026, 8, 6);
        assert_eq!(
            Stats::build(
                &record(),
                day(2026, 8, 7),
                true,
                StatsFrom::Kindle,
                SittingFloor::All
            )
            .book_totals(sixth..=sixth)
            .len(),
            1
        );
        assert_eq!(
            Stats::build(
                &record_both_identified(),
                day(2026, 8, 7),
                true,
                StatsFrom::Kindle,
                SittingFloor::All
            )
            .book_totals(sixth..=sixth)
            .len(),
            2
        );
    }

    #[test]
    fn a_book_taken_off_the_kindle_keeps_its_title_and_its_cover() {
        let dir = std::env::temp_dir().join("readinglog-stats-cover");
        let _ = std::fs::create_dir_all(&dir);
        let picture = dir.join("t.jpg");
        std::fs::write(&picture, JPEG).expect("a written cover");

        // A record remembered, and a library without it.
        let mut s = record();
        s.books[0].thumbnail = picture.to_string_lossy().into_owned();
        s.remember(&[]);
        assert_eq!(s.books.len(), 1, "an empty library removes nothing");

        let stats = Stats::build(
            &s,
            day(2026, 8, 7),
            true,
            StatsFrom::Kindle,
            SittingFloor::All,
        );
        let bible = stats
            .books
            .iter()
            .find(|b| b.extent == 148_209)
            .expect("the removed book");
        assert_eq!(bible.title, "The Jewish Study Bible");
        assert_eq!(bible.author, "Adele Berlin");
        assert_eq!(bible.thumbnail, picture.to_string_lossy());
        assert!(bible.has_cover());
        assert_eq!(bible.percent, 25.0);
        assert_eq!(bible.seconds, 1_800 + 1_200 + 600);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_stated_cover_that_is_no_picture_leaves_the_book_naming_none() {
        let dir = std::env::temp_dir().join("readinglog-stats-nojacket");
        let _ = std::fs::create_dir_all(&dir);
        // A GIF under the `.jpg` name `Book::cover_file` states.
        let placeholder = dir.join("none.jpg");
        std::fs::write(&placeholder, b"GIF89a\x3c\x00\x28\x00\x80\x00\x00").expect("written");

        for stated in [
            placeholder.to_string_lossy().into_owned(),
            dir.join("deleted.jpg").to_string_lossy().into_owned(),
            // `p_thumbnail` holding a count of extraction attempts.
            "0".to_string(),
        ] {
            let mut s = record();
            s.books[0].thumbnail = stated.clone();
            let stats = Stats::build(
                &s,
                day(2026, 8, 7),
                true,
                StatsFrom::Kindle,
                SittingFloor::All,
            );
            let bible = stats.books.first().expect("the book");
            assert!(bible.thumbnail.is_empty(), "{stated} was taken for a cover");
            assert!(!bible.has_cover(), "{stated}");
        }
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_record_that_has_never_seen_the_library_draws_no_books_at_all() {
        let bare = Stats::build(
            &bare_record(),
            day(2026, 8, 7),
            true,
            StatsFrom::Kindle,
            SittingFloor::All,
        );
        assert!(bare.books.is_empty());
        // `bare` holds every second and every sitting.
        assert_eq!(bare.total_seconds, 1_800 + 1_200 + 1_800 + 600);
        assert_eq!(bare.unidentified_seconds, bare.total_seconds);
        assert_eq!(bare.sittings.len(), 4);
        // The same record, with the library remembered.
        let after = Stats::build(
            &record(),
            day(2026, 8, 7),
            true,
            StatsFrom::Kindle,
            SittingFloor::All,
        );
        assert!(
            after
                .books
                .iter()
                .any(|b| b.title == "The Jewish Study Bible")
        );
    }

    #[test]
    fn a_sitting_on_a_loose_file_is_left_out_of_the_log() {
        let mut listed = library();
        listed.push(Book {
            extent: 777,
            cde_key: "*aa11bb22".into(),
            cde_type: "PDOC".into(),
            title: "Reading Log".into(),
            author: String::new(),
            percent: -1.0,
            thumbnail: String::new(),
            last_access: 0,
            language: String::new(),
            location: "/mnt/us/documents/ReadingLog.sh".into(),
            on_kindle: true,
            read_state: -1,
        });
        let mut s = record();
        s.remember(&listed);
        s.sittings.push(sitting(
            "2026-08-07T12:00:00",
            "2026-08-07T12:12:00",
            777,
            720,
            12,
        ));

        let stats = Stats::build(
            &s,
            day(2026, 8, 7),
            true,
            StatsFrom::Kindle,
            SittingFloor::All,
        );
        assert!(!stats.books.iter().any(|b| b.title == "Reading Log"));
        assert_eq!(stats.skipped_seconds, 720);
        // `total_seconds` and `day_seconds` are `record()`'s alone.
        assert_eq!(stats.total_seconds, 1_800 + 1_200 + 1_800 + 600);
        assert_eq!(stats.day_seconds(day(2026, 8, 7)), 600);
    }

    #[test]
    fn a_sideload_carrying_the_same_key_shape_is_a_book_and_is_listed() {
        let mut listed = library();
        listed.push(Book {
            extent: 888,
            cde_key: "*cc33dd44".into(),
            cde_type: "EBOK".into(),
            title: "A Sideloaded Book".into(),
            author: "A Writer".into(),
            percent: 4.0,
            thumbnail: String::new(),
            last_access: 0,
            language: String::new(),
            location: "/mnt/us/documents/sideload.epub".into(),
            on_kindle: true,
            read_state: -1,
        });
        let mut s = record();
        s.remember(&listed);
        s.sittings.push(sitting(
            "2026-08-07T12:00:00",
            "2026-08-07T12:12:00",
            888,
            720,
            12,
        ));

        let stats = Stats::build(
            &s,
            day(2026, 8, 7),
            true,
            StatsFrom::Kindle,
            SittingFloor::All,
        );
        let book = stats
            .books
            .iter()
            .find(|b| b.title == "A Sideloaded Book")
            .expect("a row of its own");
        assert_eq!(book.seconds, 720);
        assert_eq!(stats.skipped_seconds, 0);
        assert_eq!(stats.day_seconds(day(2026, 8, 7)), 600 + 720);
    }

    #[test]
    fn a_book_reached_by_extent_and_by_key_is_one_book() {
        let mut unmapped = sitting(
            "2026-08-08T09:00:00",
            "2026-08-08T09:20:00",
            148_207,
            1_200,
            9,
        );
        // `ends` holds no 999999; `extent_of` returns it unchanged.
        unmapped.end_position = 999_999;
        unmapped.asin = Some("B00OKPCRLG".into());
        let mut s = record();
        s.sittings.push(unmapped);

        let stats = Stats::build(
            &s,
            day(2026, 8, 8),
            true,
            StatsFrom::Kindle,
            SittingFloor::All,
        );
        let bible: Vec<&BookStat> = stats.books.iter().filter(|b| b.extent == 148_209).collect();
        assert_eq!(bible.len(), 1, "one row, not two: {:#?}", stats.books);
        assert_eq!(bible[0].seconds, 1_800 + 1_200 + 600 + 1_200);
        assert_eq!(bible[0].sittings, 4);
    }

    /// [`record`] holding the second book too.
    fn record_both_identified() -> Record {
        let mut out = bare_record();
        let mut listed = library();
        listed.push(Book {
            extent: 555,
            cde_key: "B01OTHER".into(),
            cde_type: "EBOK".into(),
            title: "The Other Book".into(),
            author: "Someone".into(),
            percent: -1.0,
            thumbnail: String::new(),
            last_access: 0,
            language: String::new(),
            location: "/mnt/us/documents/other.kfx".into(),
            on_kindle: true,
            read_state: -1,
        });
        out.remember(&listed);
        out
    }

    #[test]
    fn books_are_ordered_by_what_was_read_last() {
        let stats = Stats::build(
            &record_both_identified(),
            day(2026, 8, 7),
            true,
            StatsFrom::Kindle,
            SittingFloor::All,
        );
        assert_eq!(stats.books[0].extent, 148_209, "read on the 7th");
        assert_eq!(stats.books[1].extent, 555, "last read on the 6th");
    }

    #[test]
    fn a_sitting_points_at_its_book_after_the_reorder() {
        let stats = Stats::build(
            &record_both_identified(),
            day(2026, 8, 7),
            true,
            StatsFrom::Kindle,
            SittingFloor::All,
        );
        for s in &stats.sittings {
            let book = &stats.books[s.book.expect("every book identified")];
            // `22 * 3600` opens the second book's one sitting.
            let expected = if s.from_secs == 22 * 3600 {
                555
            } else {
                148_209
            };
            assert_eq!(book.extent, expected, "{s:?}");
        }
    }

    #[test]
    fn a_window_over_a_sleep_draws_only_the_hours_the_counter_ran_in() {
        let mut s = Record::default();
        // A window from 02:00 to 11:50, 40 seconds read at either end.
        s.sittings.push(sitting::Sitting {
            started_at: "2026-08-07T02:00:00".into(),
            ended_at: "2026-08-07T11:50:40".into(),
            end_position: 148_207,
            seconds: 80,
            page_turns: 4,
            words: 600,
            hours: vec![(2, 40), (11, 40)],
            measure: Measure::Counted,
            asin: None,
            progress: None,
            ..sitting::Sitting::default()
        });
        let stats = Stats::build(
            &s,
            day(2026, 8, 7),
            true,
            StatsFrom::Kindle,
            SittingFloor::All,
        );

        let blocks = stats.day_blocks(day(2026, 8, 7));
        assert_eq!(
            blocks,
            vec![(2 * 3600, 2 * 3600 + 40), (11 * 3600, 11 * 3600 + 40)]
        );
        let inked: i64 = blocks.iter().map(|(a, b)| b - a).sum();
        assert_eq!(
            inked,
            stats.day_seconds(day(2026, 8, 7)),
            "the strip states the day"
        );
        for (from, to) in &blocks {
            assert!(
                *to <= 3 * 3600 || *from >= 11 * 3600,
                "({from}, {to}) inks a sleep nothing was read through"
            );
        }
    }

    #[test]
    fn a_block_never_leaves_the_hour_or_the_window_it_belongs_to() {
        let stats = Stats::build(
            &record(),
            day(2026, 8, 7),
            true,
            StatsFrom::Kindle,
            SittingFloor::All,
        );
        for (from, to) in stats.day_blocks(day(2026, 8, 7)) {
            assert!(from <= to, "({from}, {to})");
            assert!(to - from <= 3600, "({from}, {to}) outgrew its hour");
            assert!(
                (0..=86_400).contains(&from) && to <= 86_400,
                "({from}, {to})"
            );
        }
    }

    #[test]
    fn a_span_cuts_its_own_reading_by_the_hour_of_it() {
        let stats = Stats::build(
            &record(),
            day(2026, 8, 7),
            true,
            StatsFrom::Kindle,
            SittingFloor::All,
        );
        let all = stats.hours_over(i64::MIN..=i64::MAX);
        assert_eq!(all.iter().sum::<i64>(), stats.total_seconds);
        assert_eq!(all[9], 1_800 + 600);
        assert_eq!(all[22], 1_800);

        // `seventh` alone holds the one sitting read on it.
        let seventh = day(2026, 8, 7)..=day(2026, 8, 7);
        let one = stats.hours_over(seventh.clone());
        assert_eq!(one.iter().sum::<i64>(), 600);
        assert_eq!(one[9], 600);
        assert_eq!(stats.span_seconds(seventh), 600);
        assert_eq!(
            stats.span_seconds(day(2026, 8, 1)..=day(2026, 8, 31)),
            stats.total_seconds
        );
        assert_eq!(stats.span_seconds(day(2026, 9, 1)..=day(2026, 9, 30)), 0);

        // The same total, cut two more ways. August 2026, a Saturday first.
        let august = day(2026, 8, 1)..=day(2026, 8, 31);
        let weekdays = stats.weekdays_over(august.clone());
        assert_eq!(weekdays.iter().sum::<i64>(), stats.total_seconds);
        let months = stats.months_over(august);
        assert_eq!(months.iter().sum::<i64>(), stats.total_seconds);
        assert_eq!(months[7], stats.total_seconds, "all of it fell in August");
        assert_eq!(months[0], 0);
    }

    #[test]
    fn a_streak_runs_while_the_days_are_consecutive() {
        let stats = Stats::build(
            &record(),
            day(2026, 8, 7),
            true,
            StatsFrom::Kindle,
            SittingFloor::All,
        );
        assert_eq!(stats.days_read(), 3);
        assert_eq!(stats.longest_streak, 3);
        assert_eq!(stats.current_streak, 3);
    }

    #[test]
    fn a_day_still_in_progress_keeps_the_streak() {
        let stats = Stats::build(
            &record(),
            day(2026, 8, 8),
            true,
            StatsFrom::Kindle,
            SittingFloor::All,
        );
        assert_eq!(stats.current_streak, 3, "yesterday still counts");
        let stale = Stats::build(
            &record(),
            day(2026, 8, 9),
            true,
            StatsFrom::Kindle,
            SittingFloor::All,
        );
        assert_eq!(stale.current_streak, 0);
        assert_eq!(stale.longest_streak, 3, "the record stands");
    }

    #[test]
    fn a_gap_ends_one_streak_and_starts_another() {
        let mut s = record();
        s.sittings.push(sitting(
            "2026-08-20T09:00:00",
            "2026-08-20T09:30:00",
            148_207,
            1_800,
            9,
        ));
        let stats = Stats::build(
            &s,
            day(2026, 8, 20),
            true,
            StatsFrom::Kindle,
            SittingFloor::All,
        );
        assert_eq!(stats.longest_streak, 3);
        assert_eq!(stats.current_streak, 1);
    }

    #[test]
    fn a_span_totals_each_book_over_it_longest_first() {
        let stats = Stats::build(
            &record_both_identified(),
            day(2026, 8, 7),
            true,
            StatsFrom::Kindle,
            SittingFloor::All,
        );
        let week = stats.book_totals(day(2026, 8, 1)..=day(2026, 8, 7));
        assert_eq!(week.len(), 2);
        assert!(week[0].1 >= week[1].1, "out of order: {week:?}",);
        // `sixth`, one day of it, and `empty`, a day outside it holding nothing.
        let sixth = day(2026, 8, 6);
        assert_eq!(stats.book_totals(sixth..=sixth).len(), 2);
        let empty = day(2026, 8, 4);
        assert!(stats.book_totals(empty..=empty).is_empty());
    }

    #[test]
    fn a_day_gives_up_its_sittings_and_its_books() {
        let stats = Stats::build(
            &record(),
            day(2026, 8, 7),
            true,
            StatsFrom::Kindle,
            SittingFloor::All,
        );
        let sixth: Vec<&Sitting> = stats.sittings_on(day(2026, 8, 6)).collect();
        assert_eq!(sixth.len(), 2);
        assert_eq!(sixth[0].from_secs, 21 * 3600, "in the order they happened");
        assert_eq!(sixth[1].from_secs, 22 * 3600);
        // `22 * 3600` opens a sitting on an unidentified book.
        let sixth = day(2026, 8, 6);
        let fifth = day(2026, 8, 5);
        assert_eq!(stats.book_totals(sixth..=sixth).len(), 1);
        assert_eq!(stats.book_totals(fifth..=fifth).len(), 1);
    }

    #[test]
    fn a_books_own_days_are_its_own() {
        let stats = Stats::build(
            &record(),
            day(2026, 8, 7),
            true,
            StatsFrom::Kindle,
            SittingFloor::All,
        );
        let bible = stats
            .books
            .iter()
            .position(|b| b.extent == 148_209)
            .unwrap();
        assert_eq!(
            stats.book_days(bible),
            vec![
                (day(2026, 8, 5), 1_800),
                (day(2026, 8, 6), 1_200),
                (day(2026, 8, 7), 600),
            ]
        );
    }

    /// The sittings on one book, each carrying its own share of `whole` over
    /// pages open for its own seconds.
    fn carrying(record: &mut Record, book: i64, whole: f64) {
        let secs: i64 = sittings_on(record, book).map(|s| s.seconds).sum();
        for s in sittings_on(record, book) {
            s.covered = whole * s.seconds as f64 / secs as f64;
            s.paged_seconds = s.seconds;
        }
    }

    fn sittings_on(record: &mut Record, book: i64) -> impl Iterator<Item = &mut sitting::Sitting> {
        record
            .sittings
            .iter_mut()
            .filter(move |s| s.end_position == book)
    }

    #[test]
    fn what_is_left_is_projected_from_the_share_the_sittings_carried() {
        let mut record = record();
        // `carrying` gives the three sittings the whole quarter the library states.
        carrying(&mut record, 148_207, 0.25);
        let stats = Stats::build(
            &record,
            day(2026, 8, 7),
            true,
            StatsFrom::Kindle,
            SittingFloor::All,
        );
        let bible = stats.books.iter().find(|b| b.extent == 148_209).unwrap();
        // 3600 s carried 25%: the 75% ahead is three times that.
        assert_eq!(bible.time_left(StatsFrom::App), Some(10_800));
        assert_eq!(bible.per_day(StatsFrom::Kindle), 1_200);
        assert_eq!(bible.per_sitting(StatsFrom::Kindle), 1_200);
        // `gone`, a book the library states no progress for, projects nothing.
        let other = Stats::build(
            &record_both_identified(),
            day(2026, 8, 7),
            true,
            StatsFrom::Kindle,
            SittingFloor::All,
        );
        let gone = other.books.iter().find(|b| b.extent == 555).unwrap();
        assert_eq!(gone.time_left(StatsFrom::App), None);
    }

    /// `percent` counts reading before [`Record::reset_at`] and `covered` counts
    /// none of it, `percent` never the divisor.
    #[test]
    fn a_book_read_before_the_record_is_projected_off_the_share_watched() {
        let mut record = record();
        // `carrying` gives a fiftieth of a book the library states a quarter done.
        carrying(&mut record, 148_207, 0.02);
        let stats = Stats::build(
            &record,
            day(2026, 8, 7),
            true,
            StatsFrom::Kindle,
            SittingFloor::All,
        );
        let bible = stats.books.iter().find(|b| b.extent == 148_209).unwrap();
        // 3600 s against the 2% watched: 0.75 / 0.02 is 37.5 of it.
        assert_eq!(bible.time_left(StatsFrom::App), Some(135_000));
    }

    /// `timer_covered` answers under [`StatsFrom::Kindle`], and it reaches reading
    /// no sitting here covers.
    #[test]
    fn the_kindle_projects_off_the_counter_in_its_own_sidecar() {
        let mut record = record();
        // `covered` holds a fiftieth; `timer_covered` holds the whole quarter
        // the library states, over two hours of `timer_ms`.
        carrying(&mut record, 148_207, 0.02);
        let slot = record
            .books
            .iter()
            .position(|b| b.extent == 148_209)
            .expect("the book");
        record.books[slot].timer_ms = 7_200_000;
        record.books[slot].timer_covered = 0.25;
        let stats = Stats::build(
            &record,
            day(2026, 8, 7),
            true,
            StatsFrom::Kindle,
            SittingFloor::All,
        );
        let bible = stats.books.iter().find(|b| b.extent == 148_209).unwrap();
        // 7200 s carried 25%: the 75% ahead is three times that.
        assert_eq!(bible.time_left(StatsFrom::Kindle), Some(21_600));
        // `covered` and `covered_seconds` answer the same beside it.
        assert_eq!(bible.time_left(StatsFrom::App), Some(135_000));
    }

    /// `covered` 0 off a row an older build wrote, or a sitting whose pages
    /// never reached the log.
    #[test]
    fn a_book_no_sitting_states_a_share_for_is_projected_from_nothing() {
        let stats = Stats::build(
            &record(),
            day(2026, 8, 7),
            true,
            StatsFrom::Kindle,
            SittingFloor::All,
        );
        let bible = stats.books.iter().find(|b| b.extent == 148_209).unwrap();
        assert_eq!(bible.covered, 0.0);
        assert_eq!(bible.time_left(StatsFrom::App), None);
    }

    /// The `t` row, a running total in the book's own sidecar lost whenever
    /// that sidecar is written anew, never stands in for the sum of the
    /// sittings, every one made before the reset dropped from it.
    #[test]
    fn a_reset_kindle_counter_never_shrinks_the_book() {
        let mut record = record();
        // A third of the sittings' 3600 s: a counter reset in the middle of the book.
        record.counters = vec![(148_207, 1_200_000, 500)];
        for s in record.sittings.iter_mut() {
            s.stated_wpm = Some(240);
        }
        let stats = Stats::build(
            &record,
            day(2026, 8, 7),
            true,
            StatsFrom::Kindle,
            SittingFloor::All,
        );
        let bible = stats.books.iter().find(|b| b.extent == 148_209).unwrap();

        // 3600 s over three sittings of 600 words each, whatever the `t` row.
        assert_eq!(bible.read_seconds(StatsFrom::Kindle), 3_600);
        assert_eq!(bible.words_read(StatsFrom::Kindle), 1_800);
        assert_eq!(bible.per_sitting(StatsFrom::Kindle), 1_200);
        assert_eq!(bible.read_seconds(StatsFrom::App), 3_600);
        assert_eq!(bible.words_read(StatsFrom::App), 1_800);

        // `stated_wpm` off the newest sitting, never a total.
        assert_eq!(bible.wpm(StatsFrom::Kindle), Some(240));
        assert_eq!(bible.wpm(StatsFrom::App), Some(30));
    }

    #[test]
    fn under_app_a_sitting_states_what_its_turn_lines_credit() {
        let mut record = record();
        // 1800 s of `seconds` under 2700 s of `timed_seconds`.
        for s in record.sittings.iter_mut() {
            s.timed_seconds = s.seconds * 3 / 2;
        }
        let kindle = Stats::build(
            &record,
            day(2026, 8, 7),
            true,
            StatsFrom::Kindle,
            SittingFloor::All,
        );
        let app = Stats::build(
            &record,
            day(2026, 8, 7),
            true,
            StatsFrom::App,
            SittingFloor::All,
        );
        assert_eq!(app.total_seconds, kindle.total_seconds * 3 / 2);

        let at = |s: &Stats| {
            s.books
                .iter()
                .find(|b| b.extent == 148_209)
                .unwrap()
                .seconds
        };
        assert_eq!(at(&app), at(&kindle) * 3 / 2);
        // `wpm` divides each mode's own words by its own seconds, and with no
        // page words [`StatsFrom::App`] takes the counter's words over a span
        // half again as long, at two thirds the rate.
        let rate = |s: &Stats, from| {
            s.books
                .iter()
                .find(|b| b.extent == 148_209)
                .unwrap()
                .wpm(from)
        };
        assert_eq!(rate(&kindle, StatsFrom::Kindle), Some(30));
        assert_eq!(rate(&app, StatsFrom::App), Some(20));
        // `sitting.hours` keeps the shape of the sitting and sums to the new figure.
        for sitting in &app.sittings {
            let summed: i64 = sitting.hours.iter().map(|(_, s)| s).sum();
            assert_eq!(summed, sitting.seconds);
        }
        // `timed_seconds` 0 keeps `Sitting::seconds`.
        for s in record.sittings.iter_mut() {
            s.timed_seconds = 0;
        }
        let bare = Stats::build(
            &record,
            day(2026, 8, 7),
            true,
            StatsFrom::App,
            SittingFloor::All,
        );
        assert_eq!(bare.total_seconds, kindle.total_seconds);
    }

    #[test]
    fn a_book_the_kindle_states_no_counter_for_falls_back_to_the_sittings() {
        let stats = Stats::build(
            &record(),
            day(2026, 8, 7),
            true,
            StatsFrom::Kindle,
            SittingFloor::All,
        );
        let bible = stats.books.iter().find(|b| b.extent == 148_209).unwrap();
        assert_eq!(bible.read_seconds(StatsFrom::Kindle), bible.seconds);
    }

    #[test]
    fn the_time_left_the_kindle_stated_stands_over_the_projection() {
        let mut record = record();
        // The figure the book's own footer carries, on the newest sitting.
        let newest = record
            .sittings
            .iter_mut()
            .filter(|s| s.end_position == 148_207)
            .max_by(|a, b| a.ended_at.cmp(&b.ended_at))
            .expect("a sitting on the bible");
        newest.time_left = Some(41_400);
        let stats = Stats::build(
            &record,
            day(2026, 8, 7),
            true,
            StatsFrom::Kindle,
            SittingFloor::All,
        );
        let bible = stats.books.iter().find(|b| b.extent == 148_209).unwrap();
        assert_eq!(bible.stated_time_left, Some(41_400));
        assert_eq!(
            bible.time_left(StatsFrom::Kindle),
            Some(41_400),
            "the projection stood"
        );
    }

    #[test]
    fn an_empty_record_is_an_empty_picture() {
        let stats = Stats::build(
            &Record::default(),
            day(2026, 8, 7),
            true,
            StatsFrom::Kindle,
            SittingFloor::All,
        );
        assert_eq!(stats.total_seconds, 0);
        assert_eq!(stats.days_read(), 0);
        assert_eq!(stats.current_streak, 0);
        assert!(stats.books.is_empty());
        let today = day(2026, 8, 7);
        assert!(stats.book_totals(today..=today).is_empty());
    }

    /// Two books carrying highlights: the first an English passage with a note
    /// written on it, the second a passage and a `Kind::Bookmark`, both
    /// passages holding `port` and the second book's the later `at`.
    pub(crate) fn highlighted_books() -> Stats {
        let highlight =
            |extent: i64, kind: Kind, at: &str, body: &str, span: (i64, i64)| Highlight {
                extent,
                kind,
                at: at.into(),
                body: body.into(),
                start: span.0,
                end: span.1,
                ..Highlight::default()
            };
        let winter = BookStat {
            title: "The Ninth Winter".into(),
            author: "Cordelia Nash".into(),
            thumbnail: "a".into(),
            extent: 1000,
            highlights: vec![0, 1],
            ..BookStat::default()
        };
        let dream = BookStat {
            title: "夢遊症候群".into(),
            author: "林素".into(),
            thumbnail: "b".into(),
            extent: 2000,
            language: "ja".into(),
            highlights: vec![2, 3],
            ..BookStat::default()
        };
        Stats {
            books: vec![winter, dream],
            highlights: vec![
                highlight(
                    1000,
                    Kind::Highlight,
                    "2026-01-02T10:00:00",
                    "the sky above the port",
                    (100, 200),
                ),
                highlight(
                    1000,
                    Kind::Note,
                    "2026-01-02T10:05:00",
                    "a borrowed line",
                    (150, 150),
                ),
                highlight(
                    2000,
                    Kind::Highlight,
                    "2026-03-04T09:00:00",
                    "港 is the word for port",
                    (10, 20),
                ),
                highlight(2000, Kind::Bookmark, "2026-03-05T09:00:00", "", (30, 30)),
            ],
            ..Stats::default()
        }
    }

    #[test]
    fn a_span_counts_the_highlights_made_inside_it() {
        let stats = highlighted_books();
        let made = day(2026, 1, 2);
        assert_eq!(stats.highlights_counted_over(0, made..=made), (1, 1));
        assert_eq!(stats.highlights_counted_on(0, made), (1, 1));
        assert_eq!(
            stats.highlights_counted_over(0, day(2026, 1, 1)..=day(2026, 1, 31)),
            stats.highlights_counted(0)
        );
        assert_eq!(
            stats.highlights_counted_over(0, day(2026, 1, 3)..=day(2026, 3, 31)),
            (0, 0)
        );
    }

    #[test]
    fn the_whole_record_lists_its_highlights_newest_first() {
        let stats = highlighted_books();
        let across = stats.highlights_across();
        // `across` holds two passages, the note folded into the first book's
        // row and the bookmark, spanning no passage, left out.
        assert_eq!(across.len(), 2);
        assert_eq!((across[0].book, across[0].at), (1, 0));
        assert!(across[0].note.is_none());
        assert_eq!((across[1].book, across[1].at), (0, 0));
        assert_eq!(
            across[1].note.map(|n| n.body.as_str()),
            Some("a borrowed line")
        );
        // `row.at` names the place its own book's list holds it, where the
        // Highlights tab opens.
        for row in &across {
            assert_eq!(
                stats.book_highlights(row.book)[row.at].highlight,
                row.highlight
            );
        }
    }
}
