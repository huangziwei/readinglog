//! Everything the screens draw, computed once from the store. A sitting's
//! `end_position`, through `Store::extent_of`, is the catalog's `p_contentSize`,
//! which is what a [`BookRecord`] is keyed by.

use crate::date;
use crate::log::session::{Measure, Session};
use crate::settings::WeekStart;
use crate::store::{BookRecord, Store};

/// One book, with everything ever read in it.
#[derive(Debug, Clone, PartialEq)]
pub struct BookStat {
    /// The catalog's number for this book, and the key the views address it by.
    pub extent: i64,
    pub title: String,
    pub author: String,
    pub thumbnail: String,
    /// The catalog's own progress figure, 0 through 100, or negative.
    pub percent: f64,
    /// Whether the catalog names this book as one the device holds.
    pub on_device: bool,
    /// The catalog language tag, which picks the face a CJK title is set in.
    pub language: String,
    pub seconds: i64,
    /// The parts of `seconds` carrying `Measure::Dwell` and `Measure::Awake`.
    pub dwell_seconds: i64,
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
}

/// How far through a book counts as read through.
const FINISHED_PERCENT: f64 = 99.0;

impl BookStat {
    /// Whether the catalog states a progress figure for this book.
    pub fn has_percent(&self) -> bool {
        self.percent >= 0.0
    }

    /// Whether the catalog states this book read through.
    pub fn is_finished(&self) -> bool {
        self.has_percent() && self.percent >= FINISHED_PERCENT
    }

    /// Over `days`, the days with reading on them.
    pub fn per_day(&self) -> i64 {
        match self.days {
            0 => 0,
            d => self.seconds / d,
        }
    }

    pub fn per_sitting(&self) -> i64 {
        match self.sittings {
            0 => 0,
            s => self.seconds / s,
        }
    }

    /// What is left to read, at this book's own rate.
    ///
    /// `None` where `percent` falls outside `0.5..99.5`, or `seconds` is zero.
    pub fn time_left(&self) -> Option<i64> {
        if !(0.5..99.5).contains(&self.percent) || self.seconds <= 0 {
            return None;
        }
        Some((self.seconds as f64 * (100.0 - self.percent) / self.percent) as i64)
    }

    /// Words a minute over everything counted in this book.
    pub fn wpm(&self) -> Option<i64> {
        (self.words > 0 && self.seconds > 0).then(|| self.words * 60 / self.seconds)
    }
}

/// One sitting, placed on the calendar and pointed at its book.
#[derive(Debug, Clone, PartialEq)]
pub struct Sitting {
    pub day: i64,
    pub from_secs: i64,
    pub to_secs: i64,
    pub seconds: i64,
    /// Index into [`Stats::books`]. `None` where no record names the book.
    pub book: Option<usize>,
    /// The catalog number this sitting was keyed by, named or not. Two unnamed
    /// sittings sharing it were read in the same book.
    pub key: i64,
    pub measure: Measure,
    pub page_turns: i64,
    /// [`Session::hours`], the seconds read in each clock hour of `day`.
    pub hours: Vec<(u8, i64)>,
}

/// The sitting histogram: five minutes a band to two hours, and one more band
/// holding everything above, so the axis closes on a round `2h+` rather than
/// mid-step.
pub const SITTING_STEP_SECS: i64 = 5 * 60;
pub const SITTING_BANDS: usize = 25;

/// The shortest run the histogram counts as reading at all.
///
/// A book opened and shut again leaves a run of seconds, and there are enough
/// of them to stand over every real sitting on the chart.
pub const SITTING_FLOOR_SECS: i64 = 60;

/// What a stretch of days came to, from [`Stats::tally`].
pub struct Tally {
    /// Seconds read over the stretch.
    pub read: i64,
    /// Days of it with any reading on them.
    pub days_read: i64,
    /// What one of those days averaged.
    pub a_day: i64,
    /// Books the catalog states read through, finished inside the stretch.
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
    /// Seconds stated against a file `BookRecord::is_book` is false on.
    pub skipped_seconds: i64,
    /// Seconds on a book no record names. In every total, drawn as no row.
    pub unnamed_seconds: i64,
}

impl Stats {
    /// Total up everything the store holds. `store.books` names each book and
    /// `catalog` is not read here; `today` places the current streak. Dropping
    /// `unnamed` leaves every total over the books the screens can list.
    pub fn build(store: &Store, today: i64, unnamed: bool) -> Self {
        let mut out = Self::default();
        // One slot per book, in first-seen order; the sort comes last.
        let mut index: Vec<(i64, usize)> = Vec::new();
        for s in &store.sessions {
            let Some(day) = date::parse_day(date::day_of(&s.started_at)) else {
                continue;
            };
            // `found.extent` where the store has a record, else `raw`.
            let raw = store.extent_of(s.end_position);
            let found = store.book_for(raw, s.asin.as_deref());
            // `is_book` is false on a scriptlet and on `My Clippings.txt`.
            if found.is_some_and(|b| !b.is_book()) {
                out.skipped_seconds += s.seconds;
                continue;
            }
            // A record reached by its key alone states an extent of zero.
            let at = found.map(|record| {
                let extent = match record.extent {
                    0 => raw,
                    stated => stated,
                };
                match index.binary_search_by_key(&extent, |(e, _)| *e) {
                    Ok(i) => index[i].1,
                    Err(i) => {
                        let slot = out.books.len();
                        out.books.push(fresh(extent, record, day));
                        index.insert(i, (extent, slot));
                        slot
                    }
                }
            });
            match at {
                Some(slot) => credit(&mut out.books[slot], s, day),
                None if !unnamed => continue,
                None => out.unnamed_seconds += s.seconds,
            }
            out.sittings.push(Sitting {
                day,
                from_secs: date::secs_of(&s.started_at),
                to_secs: date::secs_of(&s.ended_at),
                seconds: s.seconds,
                book: at,
                key: raw,
                measure: s.measure,
                page_turns: s.page_turns,
                hours: s.hours.clone(),
            });
            out.total_seconds += s.seconds;
            out.total_turns += s.page_turns;
            out.total_words += s.words;
            match out.days.binary_search_by_key(&day, |(d, _)| *d) {
                Ok(i) => out.days[i].1 += s.seconds,
                Err(i) => out.days.insert(i, (day, s.seconds)),
            }
        }

        // A book's day count needs every sitting seen before it can be stated.
        for (slot, book) in out.books.iter_mut().enumerate() {
            book.days = distinct_days(&out.sittings, slot);
        }
        out.sittings.sort_by_key(|s| (s.day, s.from_secs));
        let (longest, current) = streaks(&out.days, today);
        out.longest_streak = longest;
        out.current_streak = current;
        out.sort_books();
        out
    }

    /// Seconds read on one day.
    pub fn day_seconds(&self, day: i64) -> i64 {
        self.days
            .binary_search_by_key(&day, |(d, _)| *d)
            .map_or(0, |i| self.days[i].1)
    }

    /// Where a day's reading fell, as spans of seconds into the day.
    /// `Sitting::hours` states the seconds read in each clock hour, and each
    /// block stands in that hour, as many seconds long as were read there.
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

    /// `(day, seconds)` for every day one book was read on, ascending.
    pub fn book_days(&self, book: usize) -> Vec<(i64, i64)> {
        let mut out: Vec<(i64, i64)> = Vec::new();
        for s in self.sittings.iter().filter(|s| s.book == Some(book)) {
            match out.binary_search_by_key(&s.day, |(d, _)| *d) {
                Ok(i) => out[i].1 += s.seconds,
                Err(i) => out.insert(i, (s.day, s.seconds)),
            }
        }
        out
    }

    /// Seconds read of each book over a span of days, longest first. A sitting
    /// no record names carries no book and is left out: these need not add up
    /// to [`Stats::day_seconds`] over the same days.
    pub fn book_totals(&self, days: std::ops::RangeInclusive<i64>) -> Vec<(usize, i64)> {
        let mut out: Vec<(usize, i64)> = Vec::new();
        for sitting in self.sittings.iter().filter(|s| days.contains(&s.day)) {
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
        for sitting in self.sittings.iter().filter(|s| days.contains(&s.day)) {
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

    /// Books read over `days` that no record names, and the seconds on them.
    /// A book is one [`Sitting::key`]: two sittings whose catalog number no
    /// line ever stated carry a word position apiece and count as two.
    pub fn unnamed_over(&self, days: std::ops::RangeInclusive<i64>) -> (usize, i64) {
        let mut keys: Vec<i64> = Vec::new();
        let mut seconds = 0;
        for sitting in self.sittings.iter().filter(|s| days.contains(&s.day)) {
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

    /// Books the whole record holds that no record names.
    pub fn unnamed_books(&self) -> usize {
        match self.days.first() {
            Some((first, _)) => self.unnamed_over(*first..=self.last_day()).0,
            None => 0,
        }
    }

    /// The last day the record holds.
    fn last_day(&self) -> i64 {
        self.days.last().map(|(d, _)| *d).unwrap_or(0)
    }

    /// Everything read over `days`, cut by the clock hour it happened in. A
    /// sitting states its own seconds per hour: one running past midnight is
    /// counted where it was read, never where it started.
    pub fn hours_over(&self, days: std::ops::RangeInclusive<i64>) -> [i64; 24] {
        let mut out = [0i64; 24];
        for sitting in self.sittings.iter().filter(|s| days.contains(&s.day)) {
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
        for (day, secs) in self.days.iter().filter(|(d, _)| days.contains(d)) {
            out[date::weekday(*day)] += secs;
        }
        out
    }

    /// The same, cut by the month of the year it fell in.
    pub fn months_over(&self, days: std::ops::RangeInclusive<i64>) -> [i64; 12] {
        let mut out = [0i64; 12];
        for (day, secs) in self.days.iter().filter(|(d, _)| days.contains(d)) {
            let (_, month, _) = date::civil_from_days(*day);
            out[(month - 1).clamp(0, 11) as usize] += secs;
        }
        out
    }

    /// One fold of the record onto a cycle: what each bucket of the cycle
    /// holds, what one turn of it averages, and the fullest bucket.
    ///
    /// Every bucket is divided by however many of it the record covers, which
    /// is what makes the buckets comparable: a record opening in July holds
    /// four Augusts and three Februaries by its fourth spring.
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
    /// hours holds in an average day read on.
    ///
    /// Every such day holds all twenty-four hours, so every bucket takes the
    /// same divisor.
    pub fn average_day(&self, today: i64) -> Fold {
        let over = self.opened(today)..=today;
        // Over the days there was reading on, which is what `a day` means
        // wherever the app states one, so the hours sum to the figure over
        // them and to [`Tally::a_day`].
        let days = self.days_over(over.clone()).max(1);
        let hours = self.hours_over(over.clone());
        let values = hours.iter().map(|secs| secs / days).collect();
        self.fold(values, self.tally(over).a_day)
    }

    /// The record folded onto one week, from whichever day `week` starts on:
    /// the seconds each weekday holds in an average week.
    pub fn average_week(&self, today: i64, week: WeekStart) -> Fold {
        let over = self.opened(today)..=today;
        let counted = self.weekdays_over(over.clone());
        let mut seen = [0i64; 7];
        for day in over.clone() {
            seen[date::weekday(day)] += 1;
        }
        let values = (0..7)
            .map(|column| {
                let day = week.day_in(column);
                counted[day] / seen[day].max(1)
            })
            .collect();
        let days = over.end() - over.start() + 1;
        self.fold(values, self.span_seconds(over) * 7 / days.max(1))
    }

    /// The record laid out by month of the year: what was read in each,
    /// summed over every occurrence of that month the record holds.
    ///
    /// A total, not an average. A month the record covers four times stands
    /// against one it covers three, which is what a total of that month says.
    pub fn by_month(&self, today: i64) -> Fold {
        let counted = self.months_over(self.opened(today)..=today);
        let total = counted.iter().sum();
        self.fold(counted.to_vec(), total)
    }

    /// How many sittings of the record ran each length    /// How many sittings of the record ran each length, one count per band of
    /// [`SITTING_STEP_SECS`], the last holding every sitting past the top of
    /// the scale. A run under [`SITTING_FLOOR_SECS`] is not counted at all.
    pub fn sitting_bands(&self) -> Vec<i64> {
        let mut out = vec![0i64; SITTING_BANDS];
        for sitting in &self.sittings {
            if sitting.seconds < SITTING_FLOOR_SECS {
                continue;
            }
            let at = (sitting.seconds / SITTING_STEP_SECS).clamp(0, SITTING_BANDS as i64 - 1);
            out[at as usize] += 1;
        }
        out
    }

    /// Seconds read over `days`.
    pub fn span_seconds(&self, days: std::ops::RangeInclusive<i64>) -> i64 {
        self.days
            .iter()
            .filter(|(d, _)| days.contains(d))
            .map(|(_, secs)| secs)
            .sum()
    }

    /// What a stretch of days came to. Every screen stating these states them
    /// the same way: the board over the whole record, a span page over its own
    /// days.
    pub fn tally(&self, days: std::ops::RangeInclusive<i64>) -> Tally {
        let read = self.span_seconds(days.clone());
        let days_read = self.days_over(days.clone());
        Tally {
            read,
            days_read,
            // Over the days there was reading on, which is the figure beside
            // it: `read` divided by `days_read` is `a_day`.
            a_day: read / days_read.max(1),
            finished: self.finished_over(days),
        }
    }

    /// Days of `days` with any reading on them.
    pub fn days_over(&self, days: std::ops::RangeInclusive<i64>) -> i64 {
        self.days.iter().filter(|(d, _)| days.contains(d)).count() as i64
    }

    /// Books the catalog states read through whose last sitting falls inside
    /// `days`: the books finished over that stretch.
    pub fn finished_over(&self, days: std::ops::RangeInclusive<i64>) -> i64 {
        self.books
            .iter()
            .filter(|b| b.is_finished() && days.contains(&b.last_day))
            .count() as i64
    }

    /// Days with any reading at all.
    pub fn days_read(&self) -> i64 {
        self.days.len() as i64
    }

    /// Most recently read first, and within a day the one put down last.
    fn sort_books(&mut self) {
        let mut order: Vec<usize> = (0..self.books.len()).collect();
        order.sort_by_key(|&i| {
            let b = &self.books[i];
            (-b.last_day, -b.last_secs, -b.seconds)
        });
        // Every sitting names its book by slot.
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

/// A `BookStat` with no sitting credited to it.
fn fresh(extent: i64, found: &BookRecord, day: i64) -> BookStat {
    BookStat {
        extent,
        title: found.title.clone(),
        author: found.author.clone(),
        thumbnail: found.art().to_string(),
        percent: found.percent,
        on_device: found.on_device,
        language: found.language.clone(),
        seconds: 0,
        dwell_seconds: 0,
        awake_seconds: 0,
        sittings: 0,
        page_turns: 0,
        words: 0,
        days: 0,
        first_day: day,
        last_day: day,
        last_secs: 0,
    }
}

fn credit(book: &mut BookStat, s: &Session, day: i64) {
    book.seconds += s.seconds;
    match s.measure {
        Measure::Counted => {}
        Measure::Dwell => book.dwell_seconds += s.seconds,
        Measure::Awake => book.awake_seconds += s.seconds,
    }
    book.sittings += 1;
    book.page_turns += s.page_turns;
    book.words += s.words;
    book.first_day = book.first_day.min(day);
    let ended = date::secs_of(&s.ended_at);
    if (day, ended) >= (book.last_day, book.last_secs) {
        book.last_day = day;
        book.last_secs = ended;
    }
}

fn distinct_days(sittings: &[Sitting], book: usize) -> i64 {
    let mut days: Vec<i64> = sittings
        .iter()
        .filter(|s| s.book == Some(book))
        .map(|s| s.day)
        .collect();
    days.sort_unstable();
    days.dedup();
    days.len() as i64
}

/// The longest run of consecutive days with reading, and the run ending at `today`.
///
/// `current` accepts a last day of `today` or `today - 1`.
fn streaks(days: &[(i64, i64)], today: i64) -> (i64, i64) {
    let (mut longest, mut run) = (0, 0);
    let mut previous: Option<i64> = None;
    for (day, _) in days {
        run = match previous {
            Some(p) if *day == p + 1 => run + 1,
            _ => 1,
        };
        longest = longest.max(run);
        previous = Some(*day);
    }
    let current = match previous {
        Some(last) if last == today || last == today - 1 => run,
        _ => 0,
    };
    (longest, current)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::catalog::Book;

    fn day(y: i64, m: i64, d: i64) -> i64 {
        date::days_from_civil(y, m, d)
    }

    fn sitting(at: &str, ended: &str, book: i64, secs: i64, hour: u8) -> Session {
        Session {
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
        }
    }

    fn catalog() -> Vec<Book> {
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
            is_book: true,
            on_device: true,
        }]
    }

    /// A day count for a civil date, for the fold assertions below.
    fn at(y: i64, m: i64, d: i64) -> i64 {
        date::days_from_civil(y, m, d)
    }

    /// A store holding one sitting of `secs` on each day named.
    fn on_days(days: &[i64], secs: i64) -> Store {
        let mut store = Store::default();
        for day in days {
            let (y, m, d) = date::civil_from_days(*day);
            store.sessions.push(Session {
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
            });
        }
        store
    }

    #[test]
    fn every_month_the_record_touches_states_what_was_read_in_it() {
        // A record opening on 27 July and running to 4 September: five days of
        // July, all of August, four of September. Each month states its own
        // reading, whatever share of that month the record holds.
        let days: Vec<i64> = (at(2026, 7, 27)..=at(2026, 9, 4)).collect();
        let today = at(2026, 9, 5);
        let stats = Stats::build(&on_days(&days, 3600), today, true);

        let fold = stats.by_month(today);
        assert_eq!(fold.values[6], 5 * 3600, "five days of July");
        assert_eq!(fold.values[7], 31 * 3600, "August whole");
        assert_eq!(fold.values[8], 4 * 3600, "four days of September");
        assert_eq!(fold.busiest, Some(7));
        assert_eq!(fold.each, 40 * 3600, "the twelve months are the record");
    }

    #[test]
    fn the_board_and_a_span_state_the_same_stretch_the_same_way() {
        // Ten days of an hour, three of them empty, so days read and days
        // covered differ and the two figures cannot agree by accident.
        let days: Vec<i64> = (0..10)
            .filter(|i| i % 4 != 0)
            .map(|i| at(2026, 3, 1) + i)
            .collect();
        let today = at(2026, 3, 11);
        let stats = Stats::build(&on_days(&days, 3600), today, true);

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
    fn what_was_read_can_be_ordered_by_what_was_put_down_last() {
        // Two books: one read longer, the other read later.
        let mut store = Store::default();
        let long = at(2026, 3, 1);
        let late = at(2026, 3, 5);
        for (pos, name) in [(100, "The Long One"), (200, "The Late One")] {
            store.books.push(BookRecord {
                extent: pos,
                cde_key: format!("KEY{pos}"),
                title: name.into(),
                ..BookRecord::default()
            });
        }
        for (day, secs, pos) in [(long, 7200, 100), (late, 600, 200)] {
            let (y, m, d) = date::civil_from_days(day);
            store.sessions.push(Session {
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
            });
        }
        let today = at(2026, 3, 6);
        let stats = Stats::build(&store, today, true);
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
    fn a_book_opened_and_shut_is_not_a_sitting() {
        let today = at(2026, 3, 2);
        let day = at(2026, 3, 1);
        let mut store = on_days(&[day], SITTING_FLOOR_SECS - 1);
        let long = on_days(&[day], SITTING_FLOOR_SECS + 60);
        store.sessions.extend(long.sessions);
        let stats = Stats::build(&store, today, true);

        let bands = stats.sitting_bands();
        assert_eq!(
            bands.iter().sum::<i64>(),
            1,
            "the run of seconds is dropped"
        );
        assert_eq!(
            bands[0], 1,
            "the two-minute one is a short sitting, not none"
        );
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
        let stats = Stats::build(&on_days(&days, 3600), today, true);

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

    #[test]
    fn a_month_of_the_year_sums_every_occurrence_the_record_holds() {
        // Two Augusts of an hour each against one October of an hour: a total
        // states two hours against one.
        let days = [
            at(2023, 7, 1),
            at(2023, 8, 10),
            at(2024, 8, 10),
            at(2023, 10, 10),
        ];
        let today = at(2024, 9, 1);
        let stats = Stats::build(&on_days(&days, 3600), today, true);
        let fold = stats.by_month(today);
        assert_eq!(fold.values[7], 7200, "two Augusts");
        assert_eq!(fold.values[9], 3600, "one October");
    }

    #[test]
    fn every_hour_of_an_average_day_divides_by_the_days_covered() {
        // Ten days, an hour read in the ninth hour of each: an average day
        // holds that hour and nothing else.
        let days: Vec<i64> = (0..10).map(|i| at(2026, 3, 1) + i).collect();
        let today = at(2026, 3, 10);
        let stats = Stats::build(&on_days(&days, 3600), today, true);
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
        let stats = Stats::build(&on_days(&[at(2026, 3, 1)], long), today, true);
        let bands = stats.sitting_bands();
        assert_eq!(bands.len(), SITTING_BANDS);
        assert_eq!(bands[SITTING_BANDS - 1], 1, "the overflow band holds it");
        assert_eq!(bands.iter().sum::<i64>(), 1, "and holds it only once");
    }

    #[test]
    fn a_fold_over_an_empty_record_names_no_fullest_bucket() {
        let today = at(2026, 3, 2);
        let stats = Stats::build(&Store::default(), today, true);
        for fold in [
            stats.average_day(today),
            stats.average_week(today, WeekStart::Monday),
            stats.by_month(today),
        ] {
            assert!(fold.busiest.is_none());
            assert_eq!(fold.each, 0);
        }
    }

    /// Two books over three days, one of which the catalog names.
    fn store() -> Store {
        let mut out = bare_store();
        out.remember(&catalog());
        out
    }

    /// The same sittings, with nothing ever remembered about them.
    fn bare_store() -> Store {
        Store {
            sessions: vec![
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
            // The first book's page events key it 148207; the catalog calls the
            // same book 148209.
            ends: vec![(148_207, 148_209)],
            books: Vec::new(),
            mark: "260807:091000".into(),
        }
    }

    #[test]
    fn a_sitting_is_joined_to_the_book_the_catalog_names() {
        let stats = Stats::build(&store(), day(2026, 8, 7), true);
        // The second book of the two the sittings name has no record.
        assert_eq!(stats.books.len(), 1);
        let bible = stats
            .books
            .iter()
            .find(|b| b.extent == 148_209)
            .expect("the catalog's book");
        assert_eq!(bible.title, "The Jewish Study Bible");
        assert_eq!(bible.seconds, 1_800 + 1_200 + 600);
        assert_eq!(bible.sittings, 3);
        assert_eq!(bible.days, 3);
        assert_eq!(bible.page_turns, 12);
    }

    #[test]
    fn the_books_nothing_names_are_counted_and_timed() {
        let stats = Stats::build(&store(), day(2026, 8, 7), true);
        let sixth = day(2026, 8, 6);
        // 555 is the one key no record names, read once on the sixth.
        assert_eq!(stats.unnamed_over(sixth..=sixth), (1, 1_800));
        assert_eq!(stats.unnamed_books(), 1);
        // A day of named reading alone counts none.
        let seventh = day(2026, 8, 7);
        assert_eq!(stats.unnamed_over(seventh..=seventh), (0, 0));
    }

    #[test]
    fn two_sittings_on_one_unnamed_key_count_as_one_book() {
        let mut s = store();
        // A second sitting on 555, the day after the first.
        s.sessions.push(Session {
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
        });
        let stats = Stats::build(&s, day(2026, 8, 7), true);
        assert_eq!(stats.unnamed_books(), 1);
        assert_eq!(stats.unnamed_seconds, 1_800 + 900);
    }

    #[test]
    fn dropping_the_unnamed_leaves_every_total_over_the_books_listed() {
        let sixth = day(2026, 8, 6);
        let kept = Stats::build(&store(), day(2026, 8, 7), true);
        let dropped = Stats::build(&store(), day(2026, 8, 7), false);

        assert_eq!(dropped.unnamed_seconds, 0);
        assert_eq!(dropped.unnamed_books(), 0);
        assert_eq!(kept.total_seconds - dropped.total_seconds, 1_800);
        assert_eq!(kept.day_seconds(sixth) - dropped.day_seconds(sixth), 1_800);
        assert_eq!(dropped.sittings.len(), kept.sittings.len() - 1);
        assert!(dropped.sittings.iter().all(|s| s.book.is_some()));
        // The books themselves are untouched.
        assert_eq!(dropped.books, kept.books);
    }

    #[test]
    fn a_book_nothing_names_gets_no_row_and_keeps_its_time() {
        let stats = Stats::build(&store(), day(2026, 8, 7), true);
        assert!(!stats.books.iter().any(|b| b.extent == 555));
        assert!(!stats.books.iter().any(|b| b.title.starts_with("Book at")));
        assert_eq!(stats.unnamed_seconds, 1_800);
        // Its time is in the day and in the all-time total.
        assert_eq!(stats.total_seconds, 1_800 + 1_200 + 1_800 + 600);
        assert_eq!(stats.day_seconds(day(2026, 8, 6)), 1_200 + 1_800);
        // Its sitting is on the calendar, pointing at no book.
        let orphan = stats
            .sittings_on(day(2026, 8, 6))
            .find(|s| s.book.is_none())
            .expect("the sitting on the unnamed book");
        assert_eq!(orphan.seconds, 1_800);
    }

    #[test]
    fn a_day_counts_only_the_books_it_can_name() {
        // 2026-08-06 holds one sitting on the named book and one on neither.
        let sixth = day(2026, 8, 6);
        assert_eq!(
            Stats::build(&store(), day(2026, 8, 7), true)
                .book_totals(sixth..=sixth)
                .len(),
            1
        );
        assert_eq!(
            Stats::build(&store_both_named(), day(2026, 8, 7), true)
                .book_totals(sixth..=sixth)
                .len(),
            2
        );
    }

    #[test]
    fn a_book_taken_off_the_device_keeps_its_title_and_its_cover() {
        // A record remembered, and a catalog without it.
        let mut s = store();
        s.remember(&[]);
        assert_eq!(s.books.len(), 1, "an empty catalog removes nothing");

        let stats = Stats::build(&s, day(2026, 8, 7), true);
        let bible = stats
            .books
            .iter()
            .find(|b| b.extent == 148_209)
            .expect("the removed book");
        assert_eq!(bible.title, "The Jewish Study Bible");
        assert_eq!(bible.author, "Adele Berlin");
        assert_eq!(bible.thumbnail, "/mnt/us/system/thumbnails/t.jpg");
        assert_eq!(bible.percent, 25.0);
        assert_eq!(bible.seconds, 1_800 + 1_200 + 600);
    }

    #[test]
    fn a_store_that_has_never_seen_the_catalog_draws_no_books_at_all() {
        let bare = Stats::build(&bare_store(), day(2026, 8, 7), true);
        assert!(bare.books.is_empty());
        // Every second is held, and every sitting is on the calendar.
        assert_eq!(bare.total_seconds, 1_800 + 1_200 + 1_800 + 600);
        assert_eq!(bare.unnamed_seconds, bare.total_seconds);
        assert_eq!(bare.sittings.len(), 4);
        // The same store, with the catalog remembered.
        let after = Stats::build(&store(), day(2026, 8, 7), true);
        assert!(
            after
                .books
                .iter()
                .any(|b| b.title == "The Jewish Study Bible")
        );
    }

    #[test]
    fn a_sitting_on_a_loose_file_is_left_out_of_the_log() {
        let mut shelf = catalog();
        shelf.push(Book {
            extent: 777,
            cde_key: "*aa11bb22".into(),
            cde_type: "PDOC".into(),
            title: "Reading Log".into(),
            author: String::new(),
            percent: -1.0,
            thumbnail: String::new(),
            last_access: 0,
            language: String::new(),
            is_book: false,
            on_device: true,
        });
        let mut s = store();
        s.remember(&shelf);
        s.sessions.push(sitting(
            "2026-08-07T12:00:00",
            "2026-08-07T12:12:00",
            777,
            720,
            12,
        ));

        let stats = Stats::build(&s, day(2026, 8, 7), true);
        assert!(!stats.books.iter().any(|b| b.title == "Reading Log"));
        assert_eq!(stats.skipped_seconds, 720);
        // The totals are the ones from `store()` alone.
        assert_eq!(stats.total_seconds, 1_800 + 1_200 + 1_800 + 600);
        assert_eq!(stats.day_seconds(day(2026, 8, 7)), 600);
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
        let mut s = store();
        s.sessions.push(unmapped);

        let stats = Stats::build(&s, day(2026, 8, 8), true);
        let bible: Vec<&BookStat> = stats.books.iter().filter(|b| b.extent == 148_209).collect();
        assert_eq!(bible.len(), 1, "one row, not two: {:#?}", stats.books);
        assert_eq!(bible[0].seconds, 1_800 + 1_200 + 600 + 1_200);
        assert_eq!(bible[0].sittings, 4);
    }

    /// [`store`] with a record for the second book too.
    fn store_both_named() -> Store {
        let mut out = bare_store();
        let mut shelf = catalog();
        shelf.push(Book {
            extent: 555,
            cde_key: "B01OTHER".into(),
            cde_type: "EBOK".into(),
            title: "The Other Book".into(),
            author: "Someone".into(),
            percent: -1.0,
            thumbnail: String::new(),
            last_access: 0,
            language: String::new(),
            is_book: true,
            on_device: true,
        });
        out.remember(&shelf);
        out
    }

    #[test]
    fn books_are_ordered_by_what_was_read_last() {
        let stats = Stats::build(&store_both_named(), day(2026, 8, 7), true);
        assert_eq!(stats.books[0].extent, 148_209, "read on the 7th");
        assert_eq!(stats.books[1].extent, 555, "last read on the 6th");
    }

    #[test]
    fn a_sitting_points_at_its_book_after_the_reorder() {
        let stats = Stats::build(&store_both_named(), day(2026, 8, 7), true);
        for s in &stats.sittings {
            let book = &stats.books[s.book.expect("every book named")];
            // The 22:00 sitting is the only one on the second book.
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
        let mut s = Store::default();
        // A window from 02:00 to 11:50, 40 seconds read at either end.
        s.sessions.push(Session {
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
        });
        let stats = Stats::build(&s, day(2026, 8, 7), true);

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
        let stats = Stats::build(&store(), day(2026, 8, 7), true);
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
        let stats = Stats::build(&store(), day(2026, 8, 7), true);
        let all = stats.hours_over(i64::MIN..=i64::MAX);
        assert_eq!(all.iter().sum::<i64>(), stats.total_seconds);
        assert_eq!(all[9], 1_800 + 600);
        assert_eq!(all[22], 1_800);

        // The 7th alone holds the one sitting read on it.
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
        let stats = Stats::build(&store(), day(2026, 8, 7), true);
        assert_eq!(stats.days_read(), 3);
        assert_eq!(stats.longest_streak, 3);
        assert_eq!(stats.current_streak, 3);
    }

    #[test]
    fn a_day_still_in_progress_keeps_the_streak() {
        let stats = Stats::build(&store(), day(2026, 8, 8), true);
        assert_eq!(stats.current_streak, 3, "yesterday still counts");
        let stale = Stats::build(&store(), day(2026, 8, 9), true);
        assert_eq!(stale.current_streak, 0);
        assert_eq!(stale.longest_streak, 3, "the record stands");
    }

    #[test]
    fn a_gap_ends_one_streak_and_starts_another() {
        let mut s = store();
        s.sessions.push(sitting(
            "2026-08-20T09:00:00",
            "2026-08-20T09:30:00",
            148_207,
            1_800,
            9,
        ));
        let stats = Stats::build(&s, day(2026, 8, 20), true);
        assert_eq!(stats.longest_streak, 3);
        assert_eq!(stats.current_streak, 1);
    }

    #[test]
    fn a_span_totals_each_book_over_it_longest_first() {
        let stats = Stats::build(&store_both_named(), day(2026, 8, 7), true);
        let week = stats.book_totals(day(2026, 8, 1)..=day(2026, 8, 7));
        assert_eq!(week.len(), 2);
        assert!(week[0].1 >= week[1].1, "out of order: {week:?}",);
        // One day of it, and a day outside the span holds nothing.
        let sixth = day(2026, 8, 6);
        assert_eq!(stats.book_totals(sixth..=sixth).len(), 2);
        let empty = day(2026, 8, 4);
        assert!(stats.book_totals(empty..=empty).is_empty());
    }

    #[test]
    fn a_day_gives_up_its_sittings_and_its_books() {
        let stats = Stats::build(&store(), day(2026, 8, 7), true);
        let sixth: Vec<&Sitting> = stats.sittings_on(day(2026, 8, 6)).collect();
        assert_eq!(sixth.len(), 2);
        assert_eq!(sixth[0].from_secs, 21 * 3600, "in the order they happened");
        assert_eq!(sixth[1].from_secs, 22 * 3600);
        // The 22:00 sitting is on a book no record names.
        let sixth = day(2026, 8, 6);
        let fifth = day(2026, 8, 5);
        assert_eq!(stats.book_totals(sixth..=sixth).len(), 1);
        assert_eq!(stats.book_totals(fifth..=fifth).len(), 1);
    }

    #[test]
    fn a_books_own_days_are_its_own() {
        let stats = Stats::build(&store(), day(2026, 8, 7), true);
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

    #[test]
    fn what_is_left_is_projected_from_what_the_catalog_says_is_done() {
        let stats = Stats::build(&store(), day(2026, 8, 7), true);
        let bible = stats.books.iter().find(|b| b.extent == 148_209).unwrap();
        // 3600 s at 25%: 75% is three times that.
        assert_eq!(bible.time_left(), Some(10_800));
        assert_eq!(bible.per_day(), 1_200);
        assert_eq!(bible.per_sitting(), 1_200);
        // A book the catalog states no progress for is projected from nothing.
        let other = Stats::build(&store_both_named(), day(2026, 8, 7), true);
        let gone = other.books.iter().find(|b| b.extent == 555).unwrap();
        assert_eq!(gone.time_left(), None);
    }

    #[test]
    fn an_empty_store_is_an_empty_picture() {
        let stats = Stats::build(&Store::default(), day(2026, 8, 7), true);
        assert_eq!(stats.total_seconds, 0);
        assert_eq!(stats.days_read(), 0);
        assert_eq!(stats.current_streak, 0);
        assert!(stats.books.is_empty());
        let today = day(2026, 8, 7);
        assert!(stats.book_totals(today..=today).is_empty());
    }
}
