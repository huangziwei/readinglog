//! The record of what was read, at [`STORE_DIR`]. Every pass folds
//! `log::source` and `catalog` into `STORE_FILE`. A sitting is written once,
//! except one a pass finds in progress and re-measures from its own start.

use std::io::Write as _;
use std::path::{Path, PathBuf};

use crate::catalog::Book;
use crate::covers;
use crate::log::line::{line_stamp, log_stamp};
use crate::log::session::{Measure, SESSION_GAP_SECS, Session};
use crate::log::source;
use crate::sidecar;

/// The directory the store's file sits in.
pub const STORE_DIR: &str = "/mnt/us/extensions/readinglog";

/// The file inside it, holding the sittings, the book records and the mark.
const STORE_FILE: &str = "sessions.tsv";

/// What the first line reads. The number names the parse below it.
pub(crate) const HEADER: &str = "#readinglog\t2";

/// The percentage `BookRecord::stand_at` sets [`BookRecord::finished`] at.
pub const FINISHED_PERCENT: f64 = 99.5;

/// What named a book record, ranked strongest first, in the order
/// [`crate::identify::rescue`] asks the sources. A source takes a class a
/// weaker one named; nothing takes one [`Named::Catalog`] named.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Named {
    /// `cc.db`: the title, the author, the jacket, the content key, the place.
    /// Everything a screen draws comes from here.
    #[default]
    Catalog,
    /// `vocab.db`: a title, an author and a content key, per word looked up.
    Vocab,
    /// `My Clippings.txt`: a title and an author, per annotation.
    Clippings,
    /// A `.sdr` directory: the book's own file name and nothing else.
    Sidecar,
}

impl Named {
    /// The letter a `b` row carries.
    fn as_str(self) -> &'static str {
        match self {
            Self::Catalog => "c",
            Self::Vocab => "v",
            Self::Clippings => "l",
            Self::Sidecar => "s",
        }
    }

    /// What a `b` row's letter names, and `None` for a row carrying none or
    /// any other text.
    fn from_stored(text: &str) -> Option<Self> {
        match text.trim() {
            "c" => Some(Self::Catalog),
            "v" => Some(Self::Vocab),
            "l" => Some(Self::Clippings),
            "s" => Some(Self::Sidecar),
            _ => None,
        }
    }
}

/// What `catalog` stated about one book, on the last pass that named it.
#[derive(Debug, Default, Clone, PartialEq)]
pub struct BookRecord {
    /// `p_contentSize`, the number a sitting is keyed by.
    pub extent: i64,
    pub cde_key: String,
    /// `p_cdeType`, which `mark::set` names the book by alongside `cde_key`.
    pub cde_type: String,
    pub title: String,
    pub author: String,
    pub thumbnail: String,
    pub language: String,
    /// The catalog's `p_percentFinished`, 0 through 100, negative where unstated.
    pub percent: f64,
    /// Whether `catalog` stated a `p_location` for this book on the last pass.
    pub on_device: bool,
    /// The store's own copy of the cover, under `covers::COVERS_DIR`.
    pub cover: String,
    /// The `p_location` `catalog` last stated, empty where it named none.
    pub location: String,
    /// Whether this book is read through, set by [`Store::set_finished`].
    pub finished: bool,
    /// The place [`Store::restart`] was called at.
    pub restart: Option<f64>,
    /// The catalog's `p_readState`, negative where it states none.
    pub read_state: i64,
    /// Whether [`Store::clear_book`] took this book's reading. `Stats::build`
    /// lists the book at zero while it stands.
    pub kept: bool,
    /// What named this book, which is what says whether a stronger source may
    /// take it.
    pub named_by: Named,
}

impl BookRecord {
    /// Whether this record names a book. [`Self::cde_key`] carries a `*` for
    /// a file with no content key, and [`crate::catalog::is_reading`] answers
    /// for those from [`Self::location`].
    pub fn is_book(&self) -> bool {
        !self.cde_key.starts_with('*') || crate::catalog::is_reading(&self.location)
    }

    /// Whether the place this record holds calls the book read through.
    fn read_through(&self) -> bool {
        self.percent >= FINISHED_PERCENT
    }

    /// Take `percent` as this book's place. At or past [`FINISHED_PERCENT`] it
    /// carries [`Self::finished`]; at or past [`Self::restart`] it is the
    /// reading that ended, and `percent` holds 0.
    fn stand_at(&mut self, percent: f64) {
        if let Some(from) = self.restart {
            if percent >= from {
                return;
            }
            self.restart = None;
        }
        self.percent = percent;
        self.finished |= self.read_through();
    }

    /// Take `state` as [`Self::read_state`]. A value differing from the one
    /// held carries [`Self::finished`]; an unchanged one leaves `finished` as
    /// it stands. [`Self::read_through`] outranks both.
    fn take_mark(&mut self, state: i64) {
        if state == self.read_state {
            return;
        }
        self.read_state = state;
        let Some(read) = crate::catalog::read_state_says(state) else {
            return;
        };
        self.finished = read || self.read_through();
        if read {
            self.restart = None;
        }
    }

    /// [`Self::cover`] where one is held, else [`Self::thumbnail`].
    pub fn art(&self) -> &str {
        match self.cover.is_empty() {
            true => &self.thumbnail,
            false => &self.cover,
        }
    }
}

/// A book whose reading was put back to zero, and when. Outlives the sittings
/// it holds back.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct Cleared {
    /// The record's `p_contentSize`, 0 where the catalog never stated one.
    pub extent: i64,
    /// The record's `cde_key`, which reaches a book carrying no extent.
    pub key: String,
    /// The instant it was cleared, as `YYMMDD:HHMMSS`.
    pub at: String,
}

/// The record, loaded whole.
#[derive(Debug, Default, Clone, PartialEq)]
pub struct Store {
    /// Ascending by `started_at`, with `end_position` a sitting's identity.
    pub sessions: Vec<Session>,
    /// `EndPos → BookEndPosition.FromBook`, ascending by key.
    pub ends: Vec<(i64, i64)>,
    /// Every book `catalog` has named, ascending by `extent` then `cde_key`.
    pub books: Vec<BookRecord>,
    /// `extent → cde_key`, every pairing any pass has seen, ascending.
    pub keys: Vec<(i64, String)>,
    /// `(end position, TotalTime, TotalWords)` per class, ascending.
    pub counters: Vec<(i64, i64, i64)>,
    /// `extent → the book's file`, from a sidecar of the same counter.
    pub pairs: Vec<(i64, String)>,
    /// The newest log line any pass has read, as `YYMMDD:HHMMSS`.
    pub mark: String,
    /// Seconds the device's clock stood ahead of UTC when [`Self::mark`] was
    /// last written. [`Self::follow_clock`] reads it to tell a clock that has
    /// stepped back from one that has not, and a row carrying none states no
    /// clock to compare against.
    pub mark_offset: Option<i64>,
    /// Where the record was last emptied, `YYMMDD:HHMMSS`. No pass reads under it.
    pub floor: String,
    /// Books put back to zero, ascending by `extent` then `key`. No parse
    /// folds a sitting of one that starts below its stamp.
    pub cleared: Vec<Cleared>,
}

/// What one pass did.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct Pass {
    pub lines: usize,
    /// Sittings this pass added, past the ones it re-measured.
    pub added: usize,
    /// Sittings re-measured, having been in progress.
    pub extended: usize,
    pub from: source::Sources,
    /// Seconds [`Store::follow_clock`] pulled the mark back by, and 0 where
    /// the clock had not stepped back.
    pub rewound: i64,
}

impl Store {
    /// The store's file under `dir`.
    pub fn file(dir: &Path) -> PathBuf {
        dir.join(STORE_FILE)
    }

    /// Read the store, or an empty one where there is none to read. A file
    /// that will not parse reads as empty; one under any other `HEADER` is
    /// read whole.
    pub fn load(dir: &Path) -> Self {
        let Ok(text) = std::fs::read_to_string(Self::file(dir)) else {
            return Self::default();
        };
        Self::from_text(&text)
    }

    /// [`Self::load`] over the file in `dir`, surrendering no row it holds.
    pub fn open(dir: &Path) -> Self {
        let Ok(text) = std::fs::read_to_string(Self::file(dir)) else {
            return Self::default();
        };
        Self::from_text(&text)
    }

    /// Every row `text` holds, whatever stamp it carries.
    pub fn from_text(text: &str) -> Self {
        Self::parse(text)
    }

    /// [`Self::from_text`], for what `backup::take` folds in.
    pub fn from_archive(text: &str) -> Self {
        Self::parse(text)
    }

    /// The rows of `text`, one arm per row type.
    fn parse(text: &str) -> Self {
        let mut out = Self::default();
        for line in text.lines() {
            let mut f = line.split('\t');
            match f.next() {
                Some("#readinglog") => {}
                Some("m") => {
                    out.mark = f.next().unwrap_or_default().to_string();
                    out.mark_offset = f.next().and_then(|o| o.parse().ok());
                }
                Some("f") => out.floor = f.next().unwrap_or_default().to_string(),
                Some("e") => {
                    if let (Some(Ok(k)), Some(Ok(v))) = (
                        f.next().map(str::parse::<i64>),
                        f.next().map(str::parse::<i64>),
                    ) {
                        out.ends.push((k, v));
                    }
                }
                Some("t") => {
                    if let (Some(Ok(ep)), Some(Ok(ms)), Some(Ok(w))) = (
                        f.next().map(str::parse::<i64>),
                        f.next().map(str::parse::<i64>),
                        f.next().map(str::parse::<i64>),
                    ) {
                        out.counters.push((ep, ms, w));
                    }
                }
                Some("ep") => {
                    if let (Some(Ok(k)), Some(v)) = (f.next().map(str::parse::<i64>), f.next())
                        && !v.is_empty()
                    {
                        out.pairs.push((k, v.to_string()));
                    }
                }
                Some("k") => {
                    if let (Some(Ok(k)), Some(v)) = (f.next().map(str::parse::<i64>), f.next())
                        && !v.is_empty()
                    {
                        out.keys.push((k, v.to_string()));
                    }
                }
                Some("s") => out.sessions.extend(read_session(&mut f)),
                Some("b") => out.books.extend(read_book(&mut f)),
                Some("c") => out.cleared.extend(read_cleared(&mut f)),
                _ => {}
            }
        }
        out.sort();
        out.migrate();
        out
    }

    /// Set [`Named::Sidecar`] on each [`Named::Catalog`] record whose
    /// `(extent, cde_key)` sits in [`Self::pairs`] — the pairing
    /// [`Self::recover`] writes, and no other record carries.
    fn migrate(&mut self) {
        for i in 0..self.books.len() {
            if self.books[i].named_by != Named::Catalog {
                continue;
            }
            let paired = (self.books[i].extent, self.books[i].cde_key.clone());
            if self.pairs.binary_search(&paired).is_ok() {
                self.books[i].named_by = Named::Sidecar;
            }
        }
    }

    /// Write the store to `dir`, replacing what is there.
    ///
    /// Through a `.partial` sibling and a rename.
    pub fn save(&self, dir: &Path) -> std::io::Result<()> {
        std::fs::create_dir_all(dir)?;
        let target = Self::file(dir);
        let partial = target.with_extension("partial");
        {
            let mut out = std::fs::File::create(&partial)?;
            out.write_all(self.text().as_bytes())?;
            out.sync_all()?;
        }
        std::fs::rename(&partial, &target)
    }

    /// The record as the file holds it, which is what an archive carries and
    /// what [`Self::load`] reads back.
    pub fn text(&self) -> String {
        let mut out = String::new();
        out.push_str(HEADER);
        out.push('\n');
        if !self.mark.is_empty() {
            match self.mark_offset {
                Some(offset) => out.push_str(&format!("m\t{}\t{offset}\n", self.mark)),
                None => out.push_str(&format!("m\t{}\n", self.mark)),
            }
        }
        if !self.floor.is_empty() {
            out.push_str(&format!("f\t{}\n", self.floor));
        }
        for c in &self.cleared {
            out.push_str(&format!("c\t{}\t{}\t{}\n", c.extent, flat(&c.key), c.at));
        }
        for (k, v) in &self.ends {
            out.push_str(&format!("e\t{k}\t{v}\n"));
        }
        for (extent, key) in &self.keys {
            out.push_str(&format!("k\t{extent}\t{}\n", flat(key)));
        }
        for (ep, ms, words) in &self.counters {
            out.push_str(&format!("t\t{ep}\t{ms}\t{words}\n"));
        }
        for (extent, file) in &self.pairs {
            out.push_str(&format!("ep\t{extent}\t{}\n", flat(file)));
        }
        for b in &self.books {
            out.push_str(&write_book(b));
            out.push('\n');
        }
        for s in &self.sessions {
            out.push_str(&write_session(s));
            out.push('\n');
        }
        out
    }

    /// Move [`Self::mark`] and [`Self::floor`] onto the clock `offset` names,
    /// answering the seconds they moved.
    ///
    /// Both are wall-clock stamps and no pass reads under them, so a clock
    /// that steps back — a zone set west, or daylight saving ending — would
    /// leave every line written inside the step below the mark and unread.
    /// Pulling both back by as much as the clock moved reads that stretch
    /// again; `absorb` replaces whatever the last pass made of it.
    ///
    /// A clock stepping *forward* needs nothing: the mark falls further behind
    /// and no line is skipped.
    pub fn follow_clock(&mut self, offset: Option<i64>) -> i64 {
        let Some(now) = offset else {
            return 0;
        };
        // A record holding no offset states no clock to compare against; from
        // here it is read as the one the mark stands on.
        let Some(held) = self.mark_offset.replace(now) else {
            return 0;
        };
        let back = held - now;
        if back < CLOCK_STEP_SECS {
            return 0;
        }
        self.mark = moved(&self.mark, -back);
        self.floor = moved(&self.floor, -back);
        back
    }

    /// The instant a pass must start reading the log at: [`Self::mark`], except
    /// under `Self::open_at_mark`, where it is the newest sitting's own start
    /// and that sitting is re-measured whole. Never under [`Self::floor`].
    pub fn read_from(&self) -> String {
        self.starts_at().max(self.floor.clone())
    }

    /// [`Self::read_from`] before the floor is applied. An empty `sessions`
    /// answers the empty string, and [`Self::mark`] bounds nothing.
    fn starts_at(&self) -> String {
        let Some(newest) = self.sessions.last() else {
            return String::new();
        };
        let start = log_stamp(&newest.started_at).unwrap_or_default();
        if self.mark.is_empty() || self.open_at_mark(newest) {
            return start;
        }
        self.mark.clone().max(start)
    }

    /// Whether [`Self::mark`] falls within [`SESSION_GAP_SECS`] of `session`
    /// ending. An unparsable stamp reads as true.
    fn open_at_mark(&self, session: &Session) -> bool {
        let (Some(ended), Some(mark)) =
            (instant(&session.ended_at), instant(&iso_stamp(&self.mark)))
        else {
            return true;
        };
        mark - ended < SESSION_GAP_SECS
    }

    /// Fold `lines` into the store. Every sitting at or after `from` is
    /// dropped and replaced by what `lines` measured; a sitting [`Self::barred`]
    /// answers for is dropped.
    pub fn absorb(&mut self, lines: &[String], from: &str) -> (usize, usize) {
        let refs: Vec<&str> = lines.iter().map(String::as_str).collect();
        let parsed = crate::log::parse_sessions(refs.iter().copied(), &self.counters);
        let cut = match from.is_empty() {
            true => String::new(),
            false => iso_stamp(from),
        };
        // Ahead of the sittings: `Self::barred` places one through `ends`.
        for (k, v) in crate::log::line::frombook_map(refs.iter().copied()) {
            self.learn_end(k, v);
        }
        self.sort_ends();
        for (ep, ms, words) in crate::log::line::counter_map(refs.iter().copied()) {
            self.learn_counter(ep, ms, words);
        }
        let mut parsed: Vec<Session> = parsed.into_iter().filter(|s| !self.barred(s)).collect();
        // The clock this pass ran under, on the sittings it caught up with. An
        // older one was read on some other clock and states none rather than
        // this one — a rebuild re-reads years of them.
        if let Some(offset) = self.mark_offset {
            let (today, _) = crate::date::now();
            for session in &mut parsed {
                let day = crate::date::parse_day(crate::date::day_of(&session.started_at));
                if day.is_some_and(|day| (today - day).abs() <= 1) {
                    session.tz_offset_s = Some(offset);
                }
            }
        }

        let before = self.sessions.len();
        // Everything from `cut` on is `parsed`'s to state. A `cut` no sitting
        // stated leaves every row standing.
        if !cut.is_empty() {
            self.sessions.retain(|s| s.started_at < cut);
        }
        let dropped = before - self.sessions.len();
        let found = parsed.len();
        self.sessions.extend(parsed);

        if let Some(newest) = refs.iter().filter_map(|l| line_stamp(l)).max()
            && newest > self.mark.as_str()
        {
            self.mark = newest.to_string();
        }
        self.sort();
        (found.saturating_sub(dropped), dropped.min(found))
    }

    /// Whether `session` starts under [`Self::floor`], or under the stamp of
    /// the `c` row naming its book.
    fn barred(&self, session: &Session) -> bool {
        if !self.floor.is_empty() && session.started_at < iso_stamp(&self.floor) {
            return true;
        }
        if self.cleared.is_empty() {
            return false;
        }
        let extent = self.extent_of(session.end_position);
        match self.cleared_at(extent, session.asin.as_deref()) {
            Some(at) => session.started_at < iso_stamp(at),
            None => false,
        }
    }

    /// When a book was last cleared, found the way [`Self::slot_for`] finds its
    /// record: under `extent` where one is stated, else under `key`.
    fn cleared_at(&self, extent: i64, key: Option<&str>) -> Option<&str> {
        if extent != 0
            && let Some(c) = self.cleared.iter().find(|c| c.extent == extent)
        {
            return Some(&c.at);
        }
        let key = key.filter(|k| !k.is_empty())?;
        self.cleared
            .iter()
            .find(|c| c.key == key)
            .map(|c| c.at.as_str())
    }

    /// Read the log and fold it in, reporting files opened and files to open.
    pub fn update(&mut self, on: &mut dyn FnMut(usize, usize)) -> Pass {
        let rewound = self.follow_clock(crate::zone::offset_at(crate::date::epoch_now()));
        let from = self.read_from();
        let got = source::collect_from(
            Path::new(source::LIVE_LOG),
            Path::new(source::LOG_DIR),
            Path::new(source::DUMP_DIR),
            &from,
            on,
        );
        let (added, extended) = self.absorb(&got.lines, &from);
        Pass {
            lines: got.lines.len(),
            added,
            extended,
            from: got.from,
            rewound,
        }
    }

    /// Parse the whole log into a [`Store`] of its own and fold it in through
    /// [`Self::merge`]. Clears [`Self::floor`], keeps the `c` rows. Answers
    /// the sittings [`Self::merge`] added.
    pub fn rebuild(&mut self, on: &mut dyn FnMut(usize, usize)) -> usize {
        self.rebuild_from(
            Path::new(source::LIVE_LOG),
            Path::new(source::LOG_DIR),
            Path::new(source::DUMP_DIR),
            on,
        )
    }

    /// Re-measure every sitting the device's logs still reach. Answers the
    /// stored rows whose figures moved.
    pub fn heal(&mut self, on: &mut dyn FnMut(usize, usize)) -> usize {
        self.heal_from(
            Path::new(source::LIVE_LOG),
            Path::new(source::LOG_DIR),
            Path::new(source::DUMP_DIR),
            on,
        )
    }

    /// Every `EndPos` class this counter pair was ever stated for: the sittings
    /// carrying their own last reading of it, and the `t` rows standing for
    /// those a record holds no sitting counter for.
    fn classes_at(&self, total_ms: i64, words: i64) -> Vec<i64> {
        let mut out: Vec<i64> = self
            .sessions
            .iter()
            .filter(|s| s.end_counter_ms == Some(total_ms) && s.end_words == Some(words))
            .map(|s| s.end_position)
            .chain(
                self.counters
                    .iter()
                    .filter(|(_, ms, words_)| *ms == total_ms && *words_ == words)
                    .map(|(ep, _, _)| *ep),
            )
            .collect();
        out.sort_unstable();
        out.dedup();
        out
    }

    /// Whether a class no record names holds no counter, on the sitting or in
    /// [`Self::counters`] — what a pass over the logs can add.
    pub fn wants_the_logs(&self) -> bool {
        self.sessions.iter().any(|s| {
            let held = s.end_counter_ms.is_some()
                || self
                    .counters
                    .binary_search_by(|(ep, _, _)| ep.cmp(&s.end_position))
                    .is_ok();
            !held
                && self
                    .book_for(self.extent_of(s.end_position), s.asin.as_deref())
                    .is_none()
        })
    }

    /// Read the whole log for what each `EndPos` class states — its book end
    /// in [`Self::ends`], its counter in [`Self::counters`] — folding in no
    /// sitting. Answers the classes that gained a counter.
    pub fn relearn_classes(&mut self, on: &mut dyn FnMut(usize, usize)) -> usize {
        self.classes_from(
            Path::new(source::LIVE_LOG),
            Path::new(source::LOG_DIR),
            Path::new(source::DUMP_DIR),
            on,
        )
    }

    /// [`Self::relearn_classes`] over the three log sources named.
    fn classes_from(
        &mut self,
        live: &Path,
        chunks: &Path,
        dumps: &Path,
        on: &mut dyn FnMut(usize, usize),
    ) -> usize {
        let got = source::collect_from(live, chunks, dumps, "", on);
        let lines: Vec<&str> = got.lines.iter().map(String::as_str).collect();
        let before = self.counters.len();
        for (k, v) in crate::log::line::frombook_map(lines.iter().copied()) {
            self.learn_end(k, v);
        }
        for (ep, ms, words) in crate::log::line::counter_map(lines.iter().copied()) {
            self.learn_counter(ep, ms, words);
        }
        self.sort_ends();
        self.counters.sort();
        self.counters.dedup();
        self.counters.len() - before
    }

    /// [`Self::rebuild`] over the three log sources named.
    fn rebuild_from(
        &mut self,
        live: &Path,
        chunks: &Path,
        dumps: &Path,
        on: &mut dyn FnMut(usize, usize),
    ) -> usize {
        let got = source::collect_from(live, chunks, dumps, "", on);
        let mut whole = Store {
            ends: self.ends.clone(),
            counters: self.counters.clone(),
            cleared: self.cleared.clone(),
            ..Store::default()
        };
        whole.absorb(&got.lines, "");
        // A sitting the log restates is given up for the parse that read it.
        let mut restated: Vec<(&str, i64, &str)> = whole
            .sessions
            .iter()
            .map(|s| (s.started_at.as_str(), s.end_position, s.ended_at.as_str()))
            .collect();
        restated.sort_unstable();
        self.sessions.retain(|s| {
            restated
                .binary_search(&(s.started_at.as_str(), s.end_position, s.ended_at.as_str()))
                .is_err()
        });
        let added = self.merge(&whole);
        self.mark = self.mark.clone().max(whole.mark);
        self.floor.clear();
        added
    }

    /// [`Self::heal`] over the three log sources named. A stored row the fresh
    /// parse restates takes its figures through [`Session::remeasure`]; a
    /// sitting the record does not hold is added by [`Self::merge`].
    fn heal_from(
        &mut self,
        live: &Path,
        chunks: &Path,
        dumps: &Path,
        on: &mut dyn FnMut(usize, usize),
    ) -> usize {
        let got = source::collect_from(live, chunks, dumps, "", on);
        let mut whole = Store {
            ends: self.ends.clone(),
            counters: self.counters.clone(),
            cleared: self.cleared.clone(),
            floor: self.floor.clone(),
            ..Store::default()
        };
        whole.absorb(&got.lines, "");
        let mut healed = 0;
        for fresh in &whole.sessions {
            let held = self.sessions.iter_mut().find(|s| {
                s.started_at == fresh.started_at
                    && s.end_position == fresh.end_position
                    && s.ended_at == fresh.ended_at
            });
            if let Some(held) = held
                && held.remeasure(fresh)
            {
                healed += 1;
            }
        }
        // `merge` keeps the stored copy of a row it holds twice, the remeasured one.
        self.merge(&whole);
        self.mark = self.mark.clone().max(whole.mark);
        healed
    }

    /// Fold what `catalog` states into [`Self::books`], answering how many
    /// records changed. A book `catalog` names has its record merged; one it
    /// stops naming keeps what it holds.
    pub fn remember(&mut self, catalog: &[Book]) -> usize {
        let before = self.books.clone();
        for record in &mut self.books {
            record.on_device = false;
        }
        // The slots `catalog` stated a `p_percentFinished` for on this pass.
        let mut stated: Vec<usize> = Vec::new();
        for book in catalog {
            let slot = match self.slot_of(book) {
                Some(i) => {
                    merge(&mut self.books[i], book);
                    i
                }
                None => {
                    self.books.push(taken(book));
                    self.books.len() - 1
                }
            };
            if book.percent >= 0.0 {
                stated.push(slot);
            }
            if book.extent != 0 && !book.cde_key.is_empty() {
                self.learn_key(book.extent, &book.cde_key);
            }
        }
        self.note_progress(&stated);
        self.sort_books();
        self.books.iter().filter(|r| !before.contains(r)).count()
    }

    /// Name reading the catalog cannot: `sidecars` against [`Self::classes_at`].
    pub fn recover(&mut self, sidecars: &[sidecar::Counter]) -> usize {
        // Drop a pairing whose extent a record carries under another key.
        let books = self.books.clone();
        self.pairs.retain(|(extent, file)| {
            !books
                .iter()
                .any(|b| b.extent == *extent && b.cde_key != *file)
        });
        let mut claims: Vec<(i64, &str)> = Vec::new();
        for card in sidecars {
            if card.total_ms == 0 {
                continue;
            }
            let [end_position] = self.classes_at(card.total_ms, card.words)[..] else {
                continue;
            };
            claims.push((self.extent_of(end_position), &card.file));
        }
        let mut named = 0;
        for (i, &(extent, file)) in claims.iter().enumerate() {
            let contested = claims
                .iter()
                .enumerate()
                .any(|(j, &(e, f))| j != i && (e == extent || f == file));
            if contested || self.slot_for(extent, None).is_some() {
                continue;
            }
            self.learn_pair(extent, file);
            let slot = match self.keyed_by_name(file) {
                Some(i) => {
                    self.books[i].extent = extent;
                    i
                }
                None => {
                    self.books.push(from_sidecar(extent, file));
                    self.books.len() - 1
                }
            };
            self.stand_where_read(slot);
            named += 1;
        }
        self.sort_books();
        named
    }

    /// Every `EndPos` class with sittings that [`Self::wants`] answers `by`
    /// for: one no record names, and one whose `named_by` ranks under `by`.
    /// [`crate::identify::rescue`] reads no source an empty answer names.
    pub fn classes_wanting(&self, by: Named) -> Vec<i64> {
        let mut out: Vec<i64> = self
            .sessions
            .iter()
            .map(|s| (self.extent_of(s.end_position), s.asin.as_deref()))
            .filter(|(extent, key)| self.wants(*extent, *key, by))
            .map(|(extent, _)| extent)
            .collect();
        out.sort_unstable();
        out.dedup();
        out
    }

    /// Whether [`Self::wants`] answers `by` for any class in
    /// [`Self::sessions`].
    pub fn wants_naming(&self, by: Named) -> bool {
        self.sessions
            .iter()
            .any(|s| self.wants(self.extent_of(s.end_position), s.asin.as_deref(), by))
    }

    /// Whether a source ranked `by` may speak for `extent`.
    fn wants(&self, extent: i64, key: Option<&str>, by: Named) -> bool {
        match self.book_for(extent, key) {
            Some(record) => record.named_by > by,
            None => true,
        }
    }

    /// Name reading the catalog cannot, from what one source witnessed. A
    /// witness names the one class bracketing its instant, over a sitting of
    /// at least [`crate::stats::SITTING_FLOOR_SECS`]; `contested` holds the rest.
    pub fn name_from(
        &mut self,
        witnesses: &[crate::identify::Witness],
        by: Named,
        contested: &mut Vec<i64>,
    ) -> usize {
        let spans = self.spans();
        let longest = spans.iter().map(|s| s.to - s.from).max().unwrap_or(0);
        let mut claims: Vec<Claim> = Vec::new();
        for witness in witnesses {
            let Some(at) = instant(&witness.at) else {
                continue;
            };
            let Some((extent, key, seconds)) = around(&spans, at, longest) else {
                continue;
            };
            // A lookup past the class's own end was not made in that book.
            if witness.pos >= 0 && extent > 0 && witness.pos > extent {
                continue;
            }
            if seconds < crate::stats::SITTING_FLOOR_SECS
                || contested.contains(&extent)
                || !self.wants(extent, key, by)
            {
                continue;
            }
            claims.push(Claim {
                extent,
                name: crate::identify::normalise(&witness.title),
                title: witness.title.clone(),
                author: witness.author.clone(),
                key: witness.key.clone(),
            });
        }
        claims.sort_by(|a, b| (a.extent, &a.name).cmp(&(b.extent, &b.name)));

        let mut named = 0;
        let mut from = 0;
        while from < claims.len() {
            let extent = claims[from].extent;
            let to = from + claims[from..].partition_point(|c| c.extent == extent);
            let said = &claims[from..to];
            from = to;
            // Two titles for one class: the class holds two books and no
            // source can say which reading was which. A weaker source that
            // saw only one of them must not name it either.
            if said.iter().any(|c| c.name != said[0].name) {
                contested.push(extent);
                continue;
            }
            let key = said
                .iter()
                .map(|c| c.key.as_str())
                .find(|k| !k.is_empty())
                .unwrap_or_default();
            self.give_up(extent, by);
            match self.titled(&said[0].name, key) {
                Some(slot) => {
                    // A record whose `extent` reads 0 takes `extent`; one
                    // carrying its own keeps it, and the `k` row reaches it.
                    if self.books[slot].extent == 0 {
                        self.books[slot].extent = extent;
                    }
                    self.books[slot].named_by = self.books[slot].named_by.min(by);
                    let found = self.books[slot].cde_key.clone();
                    self.learn_key(extent, &found);
                }
                None => {
                    self.books.push(from_witness(
                        extent,
                        &said[0].title,
                        &said[0].author,
                        key,
                        by,
                    ));
                    let slot = self.books.len() - 1;
                    self.stand_where_read(slot);
                }
            }
            named += 1;
        }
        self.sort_books();
        named
    }

    /// The record a claim names: the one `key` names, else the only one
    /// carrying `name`. `None` where two records share `name`.
    fn titled(&self, name: &str, key: &str) -> Option<usize> {
        if !key.is_empty()
            && let Some(slot) = self.books.iter().position(|b| b.cde_key == key)
        {
            return Some(slot);
        }
        let mut found = self
            .books
            .iter()
            .enumerate()
            .filter(|(_, b)| crate::identify::normalise(&b.title) == name);
        let (slot, _) = found.next()?;
        found.next().is_none().then_some(slot)
    }

    /// Drop the record a source weaker than `by` made for `extent`, and the
    /// rows reaching it, so a stronger claim can stand in its place. This is
    /// what gives a book that arrived as a file name its real title.
    fn give_up(&mut self, extent: i64, by: Named) -> bool {
        let Some(slot) = self
            .books
            .iter()
            .position(|b| b.extent == extent && b.named_by > by)
        else {
            return false;
        };
        let gone = self.books.remove(slot);
        self.pairs
            .retain(|(e, file)| !(*e == extent && *file == gone.cde_key));
        self.keys
            .retain(|(e, key)| !(*e == extent && *key == gone.cde_key));
        true
    }

    /// Every sitting placed on the clock, ascending by start, for bracketing
    /// a witness.
    fn spans(&self) -> Vec<Span> {
        self.sessions
            .iter()
            .filter_map(|s| {
                Some(Span {
                    from: instant(&s.started_at)?,
                    to: instant(&s.ended_at)?,
                    extent: self.extent_of(s.end_position),
                    key: s.asin.clone(),
                    seconds: s.seconds,
                })
            })
            .collect()
    }

    /// The record whose `cde_key` `file` contains, where exactly one does and it
    /// states no extent.
    fn keyed_by_name(&self, file: &str) -> Option<usize> {
        let mut found = self.books.iter().enumerate().filter(|(_, b)| {
            b.extent == 0 && b.cde_key.len() >= KEY_IN_NAME && file.contains(&b.cde_key)
        });
        let (slot, _) = found.next()?;
        found.next().is_none().then_some(slot)
    }

    /// Give the record at `slot` the percentage its newest sitting states.
    /// `sessions` ascends by `started_at`.
    fn stand_where_read(&mut self, slot: usize) {
        let extent = self.books[slot].extent;
        for i in 0..self.sessions.len() {
            let Some(progress) = self.sessions[i].progress else {
                continue;
            };
            if self.extent_of(self.sessions[i].end_position) != extent {
                continue;
            }
            self.books[slot].stand_at((progress * 100.0).clamp(0.0, 100.0));
        }
    }

    /// Give each record outside `stated` the furthest its sittings reached,
    /// where that stands past the place it holds. `%Left` fills the gap a
    /// deletion leaves and stops a page short of the end.
    fn note_progress(&mut self, stated: &[usize]) {
        for i in 0..self.sessions.len() {
            let Some(progress) = self.sessions[i].progress else {
                continue;
            };
            let extent = self.extent_of(self.sessions[i].end_position);
            let key = self.sessions[i].asin.clone();
            let Some(slot) = self.slot_for(extent, key.as_deref()) else {
                continue;
            };
            let at = (progress * 100.0).clamp(0.0, 100.0);
            if !stated.contains(&slot) && at > self.books[slot].percent {
                self.books[slot].stand_at(at);
            }
        }
    }

    /// The slots in [`Self::books`] `Stats::build` lists: the slot a sitting
    /// is credited to, and one [`Self::clear_book`] marked `kept`. A slot
    /// [`BookRecord::is_book`] refuses is left out.
    fn shown_slots(&self) -> std::collections::HashSet<usize> {
        let mut out: std::collections::HashSet<usize> = self
            .sessions
            .iter()
            .filter_map(|s| self.slot_for(self.extent_of(s.end_position), s.asin.as_deref()))
            .collect();
        out.extend((0..self.books.len()).filter(|&slot| self.books[slot].kept));
        out.retain(|&slot| self.books[slot].is_book());
        out
    }

    /// Copy the jacket of each slot [`Self::shown_slots`] answers into `dir`,
    /// point its `cover` at the copy, and delete every other file there. A
    /// slot left out keeps an empty `cover`.
    pub fn keep_covers(&mut self, dir: &Path) -> usize {
        self.keep_covers_from(dir, Path::new(covers::THUMBNAILS_DIR))
    }

    /// [`Self::keep_covers`] against a `thumbnails` directory. A record's
    /// `thumbnail` is taken where it names a file; `covers::cached` answers
    /// the rest under `cde_key`, read once. Neither leaves an empty `cover`.
    pub fn keep_covers_from(&mut self, dir: &Path, thumbnails: &Path) -> usize {
        let shown = self.shown_slots();
        // A non-empty `books` under an empty `shown`: no record changes and
        // no file under `dir` is dropped.
        if shown.is_empty() && !self.books.is_empty() {
            return 0;
        }
        let mut cached: Option<std::collections::HashMap<String, PathBuf>> = None;
        let mut kept = 0;
        let mut placeholders = 0;
        for (slot, record) in self.books.iter_mut().enumerate() {
            if !shown.contains(&slot) {
                kept += usize::from(!std::mem::take(&mut record.cover).is_empty());
                continue;
            }
            let at = covers::path(dir, &record.cde_key);
            if !covers::held(dir, &record.cde_key) {
                let stated = Path::new(record.thumbnail.as_str());
                let art = match stated.is_file() {
                    true => Some(stated.to_path_buf()),
                    false => cached
                        .get_or_insert_with(|| covers::cached(thumbnails))
                        .get(&record.cde_key)
                        .cloned(),
                };
                let taken = match art {
                    Some(art) => match covers::keep(dir, &record.cde_key, &art) {
                        Ok(_) => true,
                        // `covers::drawable` refused `art`. `placeholders`
                        // counts these, and one line states the count.
                        Err(err) if err.kind() == std::io::ErrorKind::InvalidData => {
                            placeholders += 1;
                            false
                        }
                        Err(err) => {
                            eprintln!("covers: {} — {err}", record.title);
                            false
                        }
                    },
                    None => {
                        if !record.thumbnail.is_empty() {
                            eprintln!(
                                "covers: {} — nothing at {}, and the cache holds none under {}",
                                record.title, record.thumbnail, record.cde_key
                            );
                        }
                        false
                    }
                };
                if !taken {
                    // `at` and `cover` go together: an empty `cover` leaves
                    // `BookRecord::art` on `thumbnail`.
                    let _ = std::fs::remove_file(&at);
                    kept += usize::from(!std::mem::take(&mut record.cover).is_empty());
                    continue;
                }
            }
            let at = at.to_string_lossy();
            if record.cover != at {
                record.cover = at.into_owned();
                kept += 1;
            }
        }
        if placeholders > 0 {
            eprintln!("covers: {placeholders} books the store holds no artwork for");
        }
        let held: Vec<&str> = shown
            .iter()
            .map(|&slot| self.books[slot].cde_key.as_str())
            .collect();
        let swept = covers::sweep(dir, &held);
        if swept > 0 {
            eprintln!("covers: {swept} dropped, holding {}", held.len());
        }
        kept
    }

    /// Where `book` sits in [`Self::books`]: under its `extent`, else its key.
    /// A `book` stating no extent reaches a record carrying one.
    fn slot_of(&self, book: &Book) -> Option<usize> {
        if book.extent != 0
            && let Some(i) = self
                .books
                .iter()
                .position(|r| r.extent == book.extent && r.cde_key == book.cde_key)
        {
            return Some(i);
        }
        self.books.iter().position(|r| r.cde_key == book.cde_key)
    }

    /// The record for a sitting: by `extent` first, then by `key`.
    ///
    /// `key` reaches a book whose `p_contentSize` `catalog` never stated.
    pub fn book_for(&self, extent: i64, key: Option<&str>) -> Option<&BookRecord> {
        self.slot_for(extent, key).map(|i| &self.books[i])
    }

    /// Where [`Self::book_for`]'s answer sits in [`Self::books`].
    fn slot_for(&self, extent: i64, key: Option<&str>) -> Option<usize> {
        if extent != 0
            && let Some(i) = self.books.iter().position(|b| b.extent == extent)
        {
            return Some(i);
        }
        if let Some(k) = key.filter(|k| !k.is_empty())
            && let Some(i) = self.books.iter().position(|b| b.cde_key == k)
        {
            return Some(i);
        }
        if let Some(i) = self
            .key_at(extent)
            .and_then(|k| self.books.iter().position(|b| b.cde_key == k))
        {
            return Some(i);
        }
        self.file_at(extent)
            .and_then(|f| self.books.iter().position(|b| b.cde_key == f))
    }

    /// The book file a sidecar paired `extent` with, where exactly one did.
    /// Two files claiming one extent name neither.
    fn file_at(&self, extent: i64) -> Option<&str> {
        let mut found = self.pairs.iter().filter(|(e, _)| *e == extent);
        let (_, only) = found.next()?;
        found.next().is_none().then_some(only.as_str())
    }

    /// The `cde_key` some pass paired `extent` with, where exactly one did.
    /// Two books sharing an extent name neither.
    fn key_at(&self, extent: i64) -> Option<&str> {
        let mut found = self.keys.iter().filter(|(e, _)| *e == extent);
        let (_, only) = found.next()?;
        found.next().is_none().then_some(only.as_str())
    }

    /// Set [`BookRecord::finished`] on the record [`Self::book_for`] answers
    /// for `extent` and `key`, answering whether the value changed. A book
    /// declared read through carries no [`BookRecord::restart`].
    pub fn set_finished(&mut self, extent: i64, key: &str, finished: bool) -> bool {
        let Some(slot) = self.slot_for(extent, Some(key)) else {
            return false;
        };
        let record = &mut self.books[slot];
        let changed = record.finished != finished;
        record.finished = finished;
        if finished {
            record.restart = None;
        }
        changed
    }

    /// Write `state` down as [`BookRecord::read_state`], leaving
    /// [`BookRecord::finished`] where it stands. A pass reading the same value
    /// back takes it for no change.
    pub fn note_mark(&mut self, extent: i64, key: &str, state: i64) {
        if let Some(slot) = self.slot_for(extent, Some(key)) {
            self.books[slot].read_state = state;
        }
    }

    /// Declare a restart of the record [`Self::book_for`] answers for:
    /// `finished` comes off, `percent` becomes [`BookRecord::restart`] and
    /// reads 0. Answers whether anything changed.
    pub fn restart(&mut self, extent: i64, key: &str) -> bool {
        let Some(slot) = self.slot_for(extent, Some(key)) else {
            return false;
        };
        let record = &mut self.books[slot];
        let changed = record.finished || record.percent > 0.0;
        record.finished = false;
        if record.percent > 0.0 {
            record.restart = Some(record.percent);
            record.percent = 0.0;
        }
        changed
    }

    /// Clear `sessions`, `books` and `ends`, and set [`Self::floor`] from
    /// [`Self::mark`]. `mark` and the `c` rows stand. Answers false where
    /// `mark` is empty.
    pub fn wipe(&mut self) -> bool {
        if self.mark.is_empty() {
            return false;
        }
        self.sessions.clear();
        self.books.clear();
        self.ends.clear();
        self.floor = self.mark.clone();
        true
    }

    /// Fold `other`'s sittings, ends and books into this one, through
    /// [`Self::sort`]. [`Self::floor`] and the `c` rows stand. Answers the
    /// sittings added.
    pub fn merge(&mut self, other: &Store) -> usize {
        let before = self.sessions.len();
        self.sessions.extend(other.sessions.iter().cloned());
        for &(k, v) in &other.ends {
            self.learn_end(k, v);
        }
        self.books.extend(other.books.iter().cloned());
        for (extent, key) in &other.keys {
            self.learn_key(*extent, key);
        }
        for &(ep, ms, words) in &other.counters {
            self.learn_counter(ep, ms, words);
        }
        for (extent, file) in &other.pairs {
            self.learn_pair(*extent, file);
        }
        self.sort();
        self.sessions.len() - before
    }

    /// One book's rows as a [`Store`]: its `b` row, the sittings
    /// [`Self::slot_for`] places on it, and the `e` rows keying them. Empty
    /// where `extent` and `key` reach no record.
    pub fn one_book(&self, extent: i64, key: &str) -> Store {
        let Some(slot) = self.slot_for(extent, Some(key)) else {
            return Store::default();
        };
        let sessions: Vec<Session> = self
            .sessions
            .iter()
            .filter(|s| {
                self.slot_for(self.extent_of(s.end_position), s.asin.as_deref()) == Some(slot)
            })
            .cloned()
            .collect();
        let ends = self
            .ends
            .iter()
            .filter(|(k, _)| sessions.iter().any(|s| s.end_position == *k))
            .copied()
            .collect();
        let key = self.books[slot].cde_key.clone();
        let counters = self
            .counters
            .iter()
            .filter(|(ep, _, _)| sessions.iter().any(|s| s.end_position == *ep))
            .copied()
            .collect();
        Store {
            sessions,
            ends,
            counters,
            keys: self
                .keys
                .iter()
                .filter(|(_, k)| *k == key)
                .cloned()
                .collect(),
            pairs: self
                .pairs
                .iter()
                .filter(|(_, f)| *f == key)
                .cloned()
                .collect(),
            books: vec![self.books[slot].clone()],
            // `mark`, `floor` and the `c` rows key the record, not one book.
            mark: String::new(),
            mark_offset: None,
            floor: String::new(),
            cleared: Vec::new(),
        }
    }

    /// Put one book back to zero: its sittings go, a `c` row holds a later
    /// parse off them, [`Self::restart`] takes the place, and
    /// [`BookRecord::kept`] keeps it listed. Answers the sittings dropped.
    pub fn clear_book(&mut self, extent: i64, key: &str) -> usize {
        let went = self.drop_reading(extent, key);
        self.restart(extent, key);
        if let Some(slot) = self.slot_for(extent, Some(key)) {
            self.books[slot].kept = true;
        }
        went
    }

    /// [`Self::clear_book`], and the `b` row with it. Answers the sittings
    /// dropped. [`Self::remember`] writes a fresh record for a book the
    /// catalog names.
    pub fn forget_book(&mut self, extent: i64, key: &str) -> usize {
        let went = self.drop_reading(extent, key);
        if let Some(slot) = self.slot_for(extent, Some(key)) {
            self.books.remove(slot);
        }
        self.pairs.retain(|(_, f)| f != key);
        went
    }

    /// The sittings of one book, dropped and held back. Answers how many went.
    fn drop_reading(&mut self, extent: i64, key: &str) -> usize {
        let Some(slot) = self.slot_for(extent, Some(key)) else {
            return 0;
        };
        let kept: Vec<Session> = {
            // A sitting landing on this record, as `Stats::build` places one.
            let mine = |s: &Session| {
                self.slot_for(self.extent_of(s.end_position), s.asin.as_deref()) == Some(slot)
            };
            self.sessions.iter().filter(|s| !mine(s)).cloned().collect()
        };
        let went = self.sessions.len() - kept.len();
        self.sessions = kept;
        // An empty `mark` names no instant to hold a parse below.
        if !self.mark.is_empty() {
            self.cleared.push(Cleared {
                extent: self.books[slot].extent,
                key: self.books[slot].cde_key.clone(),
                at: self.mark.clone(),
            });
            self.sort_cleared();
        }
        went
    }

    /// The catalog number for a sitting's own key, which is the key itself
    /// where no line ever stated the mapping.
    pub fn extent_of(&self, end_position: i64) -> i64 {
        self.ends
            .binary_search_by_key(&end_position, |(k, _)| *k)
            .map_or(end_position, |i| self.ends[i].1)
    }

    /// Orders and de-duplicates `sessions` on `started_at`, `end_position` and
    /// `ended_at` together. Two sittings can share the first two.
    fn sort(&mut self) {
        self.sessions.sort_by(|a, b| {
            (&a.started_at, a.end_position, &a.ended_at).cmp(&(
                &b.started_at,
                b.end_position,
                &b.ended_at,
            ))
        });
        self.sessions.dedup_by(|a, b| {
            a.started_at == b.started_at
                && a.end_position == b.end_position
                && a.ended_at == b.ended_at
        });
        self.sort_ends();
        self.keys.sort();
        self.keys.dedup();
        self.counters.sort();
        self.counters.dedup();
        self.pairs.sort();
        self.pairs.dedup();
        self.sort_books();
        self.sort_cleared();
    }

    /// Hold `extent` against `key`, in order. A pairing held stands.
    fn learn_key(&mut self, extent: i64, key: &str) {
        let at = (extent, key.to_string());
        if let Err(i) = self.keys.binary_search(&at) {
            self.keys.insert(i, at);
        }
    }

    /// Hold `from_book` as the book end of the class `end_position` names.
    /// The last statement stands: the timer logs a book's end once and writes
    /// over it with the place the reading stopped at.
    fn learn_end(&mut self, end_position: i64, from_book: i64) {
        match self.ends.iter_mut().find(|(key, _)| *key == end_position) {
            Some(held) => held.1 = from_book,
            None => self.ends.push((end_position, from_book)),
        }
    }

    /// Hold the counter `ep` was logged with, where it is the highest yet seen.
    fn learn_counter(&mut self, ep: i64, total_ms: i64, words: i64) {
        match self.counters.iter_mut().find(|(k, _, _)| *k == ep) {
            Some(held) if held.1 < total_ms => *held = (ep, total_ms, words),
            Some(_) => {}
            None => self.counters.push((ep, total_ms, words)),
        }
    }

    /// Hold `file` against `extent`, in order. A pairing held stands.
    fn learn_pair(&mut self, extent: i64, file: &str) {
        let at = (extent, file.to_string());
        if let Err(i) = self.pairs.binary_search(&at) {
            self.pairs.insert(i, at);
        }
    }

    /// Orders and de-duplicates `ends` on their key, which is what
    /// [`Self::extent_of`] searches.
    fn sort_ends(&mut self) {
        self.ends.sort_unstable();
        self.ends.dedup_by_key(|(k, _)| *k);
    }

    /// Orders and de-duplicates `books` on `extent` and `cde_key` together:
    /// every cloud record carries an `extent` of zero, and a periodical's
    /// issues share one `cde_key`.
    fn sort_books(&mut self) {
        self.books
            .sort_by(|a, b| (a.extent, &a.cde_key).cmp(&(b.extent, &b.cde_key)));
        self.books
            .dedup_by(|a, b| a.extent == b.extent && a.cde_key == b.cde_key);
    }

    /// Orders `cleared` on `extent` and `key`, keeping the newest stamp where
    /// one book was cleared more than once.
    fn sort_cleared(&mut self) {
        self.cleared.sort_by(|a, b| {
            (a.extent, &a.key)
                .cmp(&(b.extent, &b.key))
                .then(b.at.cmp(&a.at))
        });
        self.cleared
            .dedup_by(|a, b| a.extent == b.extent && a.key == b.key);
    }
}

/// The shortest `cde_key` [`Store::keyed_by_name`] matches inside a name.
const KEY_IN_NAME: usize = 10;

/// One sitting placed on the clock, for bracketing a witness.
struct Span {
    from: i64,
    to: i64,
    /// The class's own end, which is what a record is keyed by.
    extent: i64,
    /// `Session::asin`, which reaches a book the catalog never sized.
    key: Option<String>,
    seconds: i64,
}

/// What one witness said about one class.
struct Claim {
    extent: i64,
    /// [`crate::identify::normalise`] over `title`, which is what two claims
    /// have to agree on.
    name: String,
    title: String,
    author: String,
    key: String,
}

/// The class every sitting spanning `at` belongs to, the key one of them
/// carried, and the longest of them in seconds. `None` where no sitting spans
/// `at`, and where two do. `longest` bounds the walk back.
fn around(spans: &[Span], at: i64, longest: i64) -> Option<(i64, Option<&str>, i64)> {
    let over = spans.partition_point(|s| s.from <= at);
    let mut extent: Option<i64> = None;
    let mut key = None;
    let mut seconds = 0;
    for span in spans[..over].iter().rev() {
        if span.from < at - longest {
            break;
        }
        if span.to < at {
            continue;
        }
        match extent {
            Some(held) if held != span.extent => return None,
            Some(_) => {}
            None => extent = Some(span.extent),
        }
        seconds = seconds.max(span.seconds);
        key = key.or(span.key.as_deref());
    }
    extent.map(|extent| (extent, key, seconds))
}

/// The record for a book a witness named. A source stating a content key keys
/// the record by it, the way the catalog does; one that states none keys it by
/// the title, the way [`from_sidecar`] keys a record by the file name.
fn from_witness(extent: i64, title: &str, author: &str, key: &str, by: Named) -> BookRecord {
    BookRecord {
        extent,
        cde_key: match key.is_empty() {
            true => flat(title),
            false => flat(key),
        },
        cde_type: String::new(),
        title: flat(title),
        author: flat(author),
        thumbnail: String::new(),
        language: String::new(),
        percent: -1.0,
        on_device: false,
        cover: String::new(),
        location: String::new(),
        finished: false,
        restart: None,
        read_state: -1,
        kept: false,
        named_by: by,
    }
}

/// A field with nothing in it that could be read as a separator.
fn flat(text: &str) -> String {
    text.replace(['\t', '\n', '\r'], " ")
}

/// The record for a book named by its sidecar alone. `file` is the `.sdr`
/// directory's name without the suffix, standing as both `cde_key` and `title`.
fn from_sidecar(extent: i64, file: &str) -> BookRecord {
    BookRecord {
        extent,
        cde_key: flat(file),
        cde_type: String::new(),
        title: flat(file),
        author: String::new(),
        thumbnail: String::new(),
        language: String::new(),
        percent: -1.0,
        on_device: false,
        cover: String::new(),
        location: String::new(),
        finished: false,
        restart: None,
        read_state: -1,
        kept: false,
        named_by: Named::Sidecar,
    }
}

/// A `BookRecord` holding everything `book` states.
fn taken(book: &Book) -> BookRecord {
    let mut record = BookRecord {
        extent: book.extent,
        cde_key: flat(&book.cde_key),
        cde_type: flat(&book.cde_type),
        title: flat(&book.title),
        author: flat(&book.author),
        thumbnail: flat(&book.thumbnail),
        language: flat(&book.language),
        percent: book.percent,
        on_device: book.on_device,
        cover: String::new(),
        location: flat(&book.location),
        finished: book.percent >= FINISHED_PERCENT,
        restart: None,
        // What `take_mark` reads to answer whether `book` states a new mark.
        read_state: -1,
        kept: false,
        named_by: Named::Catalog,
    };
    record.take_mark(book.read_state);
    record
}

/// Take what `book` states over what `record` holds, field by field. A cloud
/// row states no extent and no percentage, and a record carrying either from an
/// earlier pass keeps it.
fn merge(record: &mut BookRecord, book: &Book) {
    for (field, stated) in [
        (&mut record.cde_key, &book.cde_key),
        (&mut record.cde_type, &book.cde_type),
        (&mut record.title, &book.title),
        (&mut record.author, &book.author),
        (&mut record.thumbnail, &book.thumbnail),
        (&mut record.language, &book.language),
        (&mut record.location, &book.location),
    ] {
        if !stated.is_empty() {
            *field = flat(stated);
        }
    }
    if book.extent != 0 {
        record.extent = book.extent;
    }
    record.take_mark(book.read_state);
    if book.percent >= 0.0 {
        record.stand_at(book.percent);
    }
    record.on_device |= book.on_device;
    // `book` comes from the catalog, which outranks every other `Named`.
    record.named_by = Named::Catalog;
}

fn write_book(b: &BookRecord) -> String {
    format!(
        "b\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}",
        b.extent,
        flat(&b.cde_key),
        flat(&b.title),
        flat(&b.author),
        flat(&b.thumbnail),
        flat(&b.language),
        format_args!("{:.6}", b.percent),
        u8::from(b.on_device),
        flat(&b.cover),
        flat(&b.location),
        u8::from(b.finished),
        b.restart.map(|p| format!("{p:.6}")).unwrap_or_default(),
        b.read_state,
        flat(&b.cde_type),
        u8::from(b.kept),
        b.named_by.as_str(),
    )
}

/// A `b` row as a record. `extent` is the one field a row has to carry: `next`
/// reads every field past the row's last as empty, which each parse below takes
/// for its default.
fn read_book<'a>(f: &mut impl Iterator<Item = &'a str>) -> Option<BookRecord> {
    let mut next = || f.next().unwrap_or_default();
    Some(BookRecord {
        extent: next().parse().ok()?,
        cde_key: next().to_string(),
        title: next().to_string(),
        author: next().to_string(),
        thumbnail: next().to_string(),
        language: next().to_string(),
        percent: next().parse().unwrap_or(-1.0),
        on_device: next().trim() == "1",
        cover: next().to_string(),
        location: next().to_string(),
        finished: next().trim() == "1",
        restart: next().trim().parse().ok(),
        // -1, which `take_mark` reads as a value the next catalog pass renews.
        read_state: next().trim().parse().unwrap_or(-1),
        cde_type: next().to_string(),
        kept: next().trim() == "1",
        // A row stating none reads as the catalog's, which is what no source
        // may take. `Store::migrate` picks out the records a sidecar made.
        named_by: Named::from_stored(next()).unwrap_or_default(),
    })
    // `read_through` marks a record the row left unmarked.
    .map(|mut record: BookRecord| {
        record.finished |= record.read_through();
        record
    })
}

/// A `c` row as a [`Cleared`]. A row stating no stamp holds nothing back and
/// is dropped.
fn read_cleared<'a>(f: &mut impl Iterator<Item = &'a str>) -> Option<Cleared> {
    let mut next = || f.next().unwrap_or_default();
    let out = Cleared {
        extent: next().parse().ok()?,
        key: next().to_string(),
        at: next().trim().to_string(),
    };
    (!out.at.is_empty()).then_some(out)
}

/// A stored `YYYY-MM-DDTHH:MM:SS` as seconds, for taking one from another.
/// The epoch is the date module's own and only differences are ever read.
/// How far the offset must step back before a pass follows it. Under a minute
/// is a clock being nudged, not a zone changing.
const CLOCK_STEP_SECS: i64 = 60;

/// A `YYMMDD:HHMMSS` stamp moved `by` seconds, and left alone where it names
/// no instant.
fn moved(stamp: &str, by: i64) -> String {
    let Some(at) = instant(&iso_stamp(stamp)).map(|at| at + by) else {
        return stamp.to_string();
    };
    let iso = crate::date::stamp(at.div_euclid(86_400), at.rem_euclid(86_400));
    log_stamp(&iso).unwrap_or_else(|| stamp.to_string())
}

fn instant(at: &str) -> Option<i64> {
    Some(crate::date::parse_day(crate::date::day_of(at))? * 86_400 + crate::date::secs_of(at))
}

/// `YYMMDD:HHMMSS` to the `YYYY-MM-DDTHH:MM:SS` a session is stored under.
fn iso_stamp(stamp: &str) -> String {
    match crate::log::line::stamp(&format!("{stamp} x")) {
        Some(m) => m.at,
        None => String::new(),
    }
}

/// The hours of a session as `h:s;h:s`, the one field carrying a list.
fn write_hours(hours: &[(u8, i64)]) -> String {
    hours
        .iter()
        .map(|(h, s)| format!("{h}:{s}"))
        .collect::<Vec<_>>()
        .join(";")
}

fn read_hours(text: &str) -> Vec<(u8, i64)> {
    text.split(';')
        .filter_map(|pair| {
            let (h, s) = pair.split_once(':')?;
            Some((h.parse().ok()?, s.parse().ok()?))
        })
        .collect()
}

fn write_session(s: &Session) -> String {
    let num = |n: Option<i64>| n.map(|n| n.to_string()).unwrap_or_default();
    format!(
        "s\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}",
        s.started_at,
        s.ended_at,
        s.end_position,
        s.seconds,
        s.page_turns,
        s.words,
        s.measure.as_str(),
        s.asin.as_deref().unwrap_or(""),
        s.progress.map(|p| format!("{p:.6}")).unwrap_or_default(),
        write_hours(&s.hours),
        num(s.start_counter_ms),
        num(s.end_counter_ms),
        num(s.start_words),
        num(s.end_words),
        num(s.tz_offset_s),
        num(s.time_left),
        num(s.stated_wpm),
        s.awake_seconds,
    )
}

fn read_session<'a>(f: &mut impl Iterator<Item = &'a str>) -> Option<Session> {
    let mut next = || f.next().unwrap_or_default();
    let started_at = next().to_string();
    let ended_at = next().to_string();
    if started_at.is_empty() || ended_at.is_empty() {
        return None;
    }
    Some(Session {
        started_at,
        ended_at,
        end_position: next().parse().ok()?,
        seconds: next().parse().ok()?,
        page_turns: next().parse().unwrap_or(0),
        words: next().parse().unwrap_or(0),
        measure: Measure::from_stored(next()),
        asin: Some(next().to_string()).filter(|a| !a.is_empty()),
        progress: next().parse().ok(),
        hours: read_hours(next()),
        // A row carrying no counters leaves all four unstated.
        start_counter_ms: next().parse().ok(),
        end_counter_ms: next().parse().ok(),
        start_words: next().parse().ok(),
        end_words: next().parse().ok(),
        tz_offset_s: next().parse().ok(),
        time_left: next().parse().ok(),
        stated_wpm: next().parse().ok(),
        awake_seconds: next().parse().unwrap_or(0),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn session(started: &str, ended: &str, book: i64, secs: i64) -> Session {
        Session {
            started_at: started.into(),
            ended_at: ended.into(),
            end_position: book,
            seconds: secs,
            page_turns: 3,
            words: 900,
            hours: vec![(10, secs)],
            measure: Measure::Counted,
            asin: Some("B00OKPCRLG".into()),
            progress: Some(0.355),
            ..Session::default()
        }
    }

    fn scratch(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("readinglog-store-{name}"));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("a scratch directory");
        dir
    }

    #[test]
    fn a_book_the_catalog_states_no_thumbnail_for_is_given_the_cached_one() {
        let dir = scratch("cached-covers");
        let cache = dir.join("thumbnails");
        std::fs::create_dir_all(&cache).expect("a thumbnail cache");
        let write = |at: &Path, bytes: &[u8]| {
            std::fs::write(at, bytes).expect("a written thumbnail");
            at.to_path_buf()
        };
        let stated = write(
            &dir.join("thumbnail_XH01Es.jpg"),
            b"\xff\xd8\xffthe stated one",
        );
        write(
            &cache.join("thumbnail_B00OKPCRLG_EBOK_portrait.jpg"),
            b"\xff\xd8\xffthe cached one",
        );
        write(
            &cache.join("thumbnail_B00RESCUED_EBOK_portrait.jpg"),
            b"\xff\xd8\xffthe rescued one",
        );
        let named = |extent: i64, key: &str, thumbnail: &str| BookRecord {
            extent,
            cde_key: key.into(),
            title: key.into(),
            thumbnail: thumbnail.into(),
            ..BookRecord::default()
        };
        let read =
            |extent: i64| session("2026-08-07T10:15:01", "2026-08-07T10:55:43", extent, 2_400);
        let mut store = Store {
            sessions: vec![read(148_207), read(148_301), read(148_402)],
            books: vec![
                // `thumbnail` names `stated`, and `cache` holds the key too.
                named(148_207, "B00OKPCRLG", &stated.to_string_lossy()),
                // An empty `thumbnail`, under a key `cache` holds.
                named(148_301, "B00RESCUED", ""),
                // A `thumbnail` naming no file, under a key `cache` lacks.
                named(
                    148_402,
                    "B00SIDELOAD",
                    &dir.join("thumbnail_gone.jpg").to_string_lossy(),
                ),
            ],
            ..Store::default()
        };

        assert_eq!(store.keep_covers_from(&dir, &cache), 2);
        let held = |key: &str| std::fs::read(covers::path(&dir, key)).expect("a copied cover");
        assert_eq!(
            held("B00OKPCRLG"),
            b"\xff\xd8\xffthe stated one",
            "the cache outranked it"
        );
        assert_eq!(
            held("B00RESCUED"),
            b"\xff\xd8\xffthe rescued one",
            "no path was stated"
        );
        assert!(!covers::held(&dir, "B00SIDELOAD"), "nothing names one");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_placeholder_already_copied_is_dropped_and_named_by_nothing() {
        let dir = scratch("placeholder-covers");
        let cache = dir.join("thumbnails");
        std::fs::create_dir_all(&cache).expect("a thumbnail cache");
        // A 60x40 GIF under a `.jpg` name, which `covers::drawable` refuses.
        let art = cache.join("thumbnail_B0053VMNY2_EBOK_portrait.jpg");
        std::fs::write(&art, b"GIF89a\x3c\x00\x28\x00\x80\x00\x00").expect("a written thumbnail");
        // The same bytes under `covers::COVERS_DIR`.
        std::fs::create_dir_all(dir.join(covers::COVERS_DIR)).expect("the covers directory");
        std::fs::write(
            covers::path(&dir, "B0053VMNY2"),
            b"GIF89a\x3c\x00\x28\x00\x80\x00\x00",
        )
        .expect("a copied placeholder");
        let mut store = Store {
            sessions: vec![session(
                "2026-08-07T10:15:01",
                "2026-08-07T10:55:43",
                148_207,
                2_400,
            )],
            books: vec![BookRecord {
                extent: 148_207,
                cde_key: "B0053VMNY2".into(),
                title: "The New Oxford American Dictionary".into(),
                thumbnail: art.to_string_lossy().into_owned(),
                cover: covers::path(&dir, "B0053VMNY2")
                    .to_string_lossy()
                    .into_owned(),
                ..BookRecord::default()
            }],
            ..Store::default()
        };

        assert_eq!(
            store.keep_covers_from(&dir, &cache),
            1,
            "the record changed"
        );
        assert!(store.books[0].cover.is_empty(), "and names no jacket");
        assert!(!covers::path(&dir, "B0053VMNY2").exists(), "the copy went");
        assert!(art.is_file(), "the device's own cache is left alone");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_cover_is_kept_only_for_a_book_shown_slots_answers() {
        let dir = scratch("covers");
        let art = dir.join("thumbnail.jpg");
        std::fs::write(&art, b"\xff\xd8\xff\xe0\x00\x10JFIF\0").expect("a written thumbnail");
        let named = |extent: i64, key: &str| BookRecord {
            extent,
            cde_key: key.into(),
            title: key.into(),
            thumbnail: art.to_string_lossy().into_owned(),
            ..BookRecord::default()
        };
        let mut store = Store {
            sessions: vec![session(
                "2026-08-07T10:15:01",
                "2026-08-07T10:55:43",
                148_207,
                2_400,
            )],
            books: vec![
                named(148_207, "B00OKPCRLG"),
                named(938_016, "B00NEVERRD"),
                // `kept`, which `shown_slots` answers with no sitting.
                BookRecord {
                    kept: true,
                    ..named(511_402, "B00CLEARED")
                },
            ],
            ..Store::default()
        };
        // `B00NEVERRD` holds a jacket, and `B01.partial` sits beside it.
        covers::keep(&dir, "B00NEVERRD", &art).expect("a copied cover");
        store.books[1].cover = covers::path(&dir, "B00NEVERRD")
            .to_string_lossy()
            .into_owned();
        std::fs::write(dir.join(covers::COVERS_DIR).join("B01.partial"), b"x").unwrap();

        assert_eq!(store.keep_covers(&dir), 3, "two taken, one given up");
        assert!(covers::held(&dir, "B00OKPCRLG"), "the book a sitting names");
        assert!(covers::held(&dir, "B00CLEARED"), "the book `kept` marks");
        assert!(!covers::held(&dir, "B00NEVERRD"), "the book neither names");
        assert!(store.books[1].cover.is_empty(), "a cover no file backs");
        // `B01.partial` is not among them.
        let left = std::fs::read_dir(dir.join(covers::COVERS_DIR))
            .expect("the covers directory")
            .count();
        assert_eq!(left, 2, "one jacket per slot `shown_slots` answers");

        // `keep_covers` over the same store takes nothing and drops nothing.
        assert_eq!(store.keep_covers(&dir), 0);
        assert!(covers::held(&dir, "B00OKPCRLG"));
    }

    /// `books` holds a record and `sessions` is empty: `keep_covers` answers
    /// 0 and leaves `covers::COVERS_DIR` standing.
    #[test]
    fn a_record_holding_books_and_no_sittings_keeps_every_jacket() {
        let dir = scratch("short-covers");
        let art = dir.join("thumbnail.jpg");
        std::fs::write(&art, b"\xff\xd8\xff\xe0\x00\x10JFIF\0").expect("a written thumbnail");
        covers::keep(&dir, "B00OKPCRLG", &art).expect("a copied cover");
        let mut store = Store {
            books: vec![BookRecord {
                extent: 148_207,
                cde_key: "B00OKPCRLG".into(),
                title: "B00OKPCRLG".into(),
                thumbnail: art.to_string_lossy().into_owned(),
                cover: covers::path(&dir, "B00OKPCRLG")
                    .to_string_lossy()
                    .into_owned(),
                ..BookRecord::default()
            }],
            ..Store::default()
        };

        assert_eq!(store.keep_covers(&dir), 0, "no record changed");
        assert!(covers::held(&dir, "B00OKPCRLG"), "the jacket stands");
        assert!(!store.books[0].cover.is_empty(), "and the record names it");
    }

    #[test]
    fn a_store_round_trips_through_its_file() {
        let dir = scratch("roundtrip");
        let mut store = Store {
            sessions: vec![
                session("2026-08-07T10:15:01", "2026-08-07T10:55:43", 148_207, 2_400),
                session("2026-08-08T21:00:00", "2026-08-08T21:30:00", 938_016, 1_800),
            ],
            ends: vec![(938_016, 938_018)],
            keys: Vec::new(),
            counters: Vec::new(),
            pairs: Vec::new(),
            books: Vec::new(),
            mark: "260808:213000".into(),
            mark_offset: None,
            floor: String::new(),
            cleared: vec![Cleared {
                extent: 304_517,
                key: "B00OKPCRLG".into(),
                at: "260808:120000".into(),
            }],
        };
        store.sessions[0].time_left = Some(41_400);
        store.sessions[1].asin = None;
        store.sessions[1].progress = None;
        store.sessions[1].measure = Measure::Dwell;
        store.save(&dir).expect("a written store");

        assert_eq!(Store::load(&dir), store);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_pass_with_no_stamp_to_cut_at_leaves_every_sitting_standing() {
        // A stored `started_at` no `log_stamp` reads leaves `read_from` empty.
        let mut store = Store {
            sessions: vec![session("nonsense", "nonsense", 148_207, 2_400)],
            ..Store::default()
        };
        assert!(store.read_from().is_empty());
        store.absorb(&[], "");
        assert_eq!(store.sessions.len(), 1, "an empty cut emptied the record");
    }

    #[test]
    fn a_store_from_an_older_parse_keeps_every_row_it_holds() {
        let dir = scratch("stamped");
        let store = Store {
            sessions: vec![session(
                "2026-08-07T10:15:01",
                "2026-08-07T10:55:43",
                148_207,
                2_400,
            )],
            ends: vec![(938_016, 938_018)],
            keys: Vec::new(),
            counters: Vec::new(),
            pairs: Vec::new(),
            books: vec![BookRecord {
                extent: 148_207,
                title: "A Book".into(),
                ..BookRecord::default()
            }],
            mark: "260808:213000".into(),
            mark_offset: None,
            floor: String::new(),
            cleared: vec![Cleared {
                extent: 148_207,
                key: "B00OKPCRLG".into(),
                at: "260808:120000".into(),
            }],
        };
        store.save(&dir).expect("a written store");
        let text = std::fs::read_to_string(Store::file(&dir)).expect("a store to stamp");
        std::fs::write(
            Store::file(&dir),
            text.replacen(HEADER, "#readinglog\t1", 1),
        )
        .expect("an older store");

        let read = Store::load(&dir);
        assert_eq!(read.sessions, store.sessions, "a load gave sittings up");
        assert_eq!(read.mark, store.mark, "a load gave the mark up");
        assert_eq!(read.books, store.books, "the names were not the parse's");
        assert_eq!(read.ends, store.ends);
        assert_eq!(
            read.cleared, store.cleared,
            "a cleared book would come back with the whole log"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// `store`, written to `dir` and then stamped with a header this build
    /// does not know.
    fn superseded(dir: &Path, store: &Store) {
        store.save(dir).expect("a written store");
        let text = std::fs::read_to_string(Store::file(dir)).expect("a store to stamp");
        std::fs::write(Store::file(dir), text.replacen(HEADER, "#readinglog\t1", 1))
            .expect("an older store");
    }

    #[test]
    fn a_superseded_record_opens_whole_and_is_not_archived() {
        let dir = scratch("superseded");
        let store = two_books();
        superseded(&dir, &store);

        // Opening an updated build takes nothing and writes nothing beside it.
        let read = Store::open(&dir);
        assert_eq!(read.sessions, store.sessions, "the sittings were given up");
        assert_eq!(read.books, store.books);
        assert!(
            crate::backup::list(&dir).is_empty(),
            "an archive nobody asked for"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_record_this_build_reads_whole_is_not_archived() {
        let dir = scratch("current");
        two_books().save(&dir).expect("a written store");
        assert_eq!(Store::open(&dir).sessions.len(), 3);
        assert!(
            crate::backup::list(&dir).is_empty(),
            "an archive for nothing"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_record_that_is_not_there_is_opened_as_an_empty_one() {
        let dir = scratch("open-missing");
        assert_eq!(Store::open(&dir), Store::default());
        assert!(crate::backup::list(&dir).is_empty());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_store_that_is_not_there_reads_as_empty() {
        let dir = scratch("missing");
        assert_eq!(Store::load(&dir.join("nothing")), Store::default());
        let _ = std::fs::remove_dir_all(&dir);
    }

    fn shelved(extent: i64, key: &str, title: &str, percent: f64) -> Book {
        Book {
            extent,
            cde_key: key.into(),
            cde_type: "EBOK".into(),
            title: title.into(),
            author: "Adele Berlin".into(),
            percent,
            thumbnail: "/mnt/us/system/thumbnails/t.jpg".into(),
            last_access: 0,
            language: "en".into(),
            location: format!("/mnt/us/documents/{title}.kfx"),
            on_device: true,
            read_state: -1,
        }
    }

    #[test]
    fn a_mark_survives_the_file_and_every_pass_over_it() {
        let dir = scratch("finished");
        let mut store = Store::default();
        let shelf = [shelved(938_018, "B00OKPCRLG", "Bible", 55.0)];
        store.remember(&shelf);
        assert!(!store.books[0].finished, "a catalog row states no mark");

        assert!(store.set_finished(938_018, "B00OKPCRLG", true));
        // The same value twice is no change and no write.
        assert!(!store.set_finished(938_018, "B00OKPCRLG", true));
        // A catalog pass states nothing about the mark and takes nothing off.
        store.remember(&shelf);
        assert!(store.books[0].finished);

        store.save(&dir).expect("a written store");
        assert!(Store::load(&dir).books[0].finished);

        assert!(store.set_finished(938_018, "B00OKPCRLG", false));
        store.save(&dir).expect("a written store");
        assert!(!Store::load(&dir).books[0].finished);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_row_written_before_the_mark_reads_as_unmarked() {
        let dir = scratch("premark");
        std::fs::create_dir_all(&dir).expect("a directory to write in");
        // Ten fields: a `b` row without the `finished` field.
        let row = "b\t938018\tB00OKPCRLG\tBible\tBerlin\t\ten\t55.000000\t1\t\t/mnt/us/a.kfx";
        std::fs::write(Store::file(&dir), format!("{HEADER}\n{row}\n")).expect("a store");
        let store = Store::load(&dir);
        assert_eq!(store.books.len(), 1);
        assert!(!store.books[0].finished);
        assert_eq!(store.books[0].location, "/mnt/us/a.kfx");
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// [`shelved`] carrying `read_state`.
    fn marked(percent: f64, read_state: i64) -> Book {
        Book {
            read_state,
            ..shelved(938_018, "B00OKPCRLG", "Bible", percent)
        }
    }

    #[test]
    fn the_librarys_own_mark_carries_the_records_and_a_tap_holds_against_it() {
        let mut store = Store::default();
        store.remember(&[marked(40.0, -1)]);
        assert!(!store.books[0].finished, "a NULL column states nothing");

        // 1, which `catalog::read_state_says` reads as read.
        store.remember(&[marked(40.0, 1)]);
        assert!(store.books[0].finished);

        // `set_finished` disagrees, and an unchanged `read_state` on every
        // pass after it leaves `finished` standing.
        assert!(store.set_finished(938_018, "B00OKPCRLG", false));
        store.remember(&[marked(40.0, 1)]);
        store.remember(&[marked(40.0, 1)]);
        assert!(
            !store.books[0].finished,
            "an unchanged column overrode a tap"
        );

        // 3 states unread, 4 read.
        store.remember(&[marked(40.0, 3)]);
        assert!(!store.books[0].finished);
        store.remember(&[marked(40.0, 4)]);
        assert!(store.books[0].finished);

        // 0 states neither way and takes nothing off.
        store.remember(&[marked(40.0, 0)]);
        assert!(store.books[0].finished);
    }

    #[test]
    fn a_mark_handed_to_the_library_reads_back_as_no_change() {
        let dir = scratch("notemark");
        let mut store = Store::default();
        store.remember(&[marked(40.0, -1)]);

        // `set_finished` and `note_mark`, the pair one tap makes.
        assert!(store.set_finished(938_018, "B00OKPCRLG", true));
        store.note_mark(938_018, "B00OKPCRLG", crate::catalog::read_state_for(true));
        assert_eq!(store.books[0].read_state, 1);
        assert!(store.books[0].finished);

        // The catalog states the same value back.
        store.remember(&[marked(40.0, 1)]);
        assert!(store.books[0].finished);

        // `cde_type` reaches the record and the row.
        assert_eq!(store.books[0].cde_type, "EBOK");
        store.save(&dir).expect("a written store");
        let back = Store::load(&dir);
        assert_eq!(back.books[0].cde_type, "EBOK");
        assert_eq!(back.books[0].read_state, 1);

        // A record no key names takes no mark.
        store.note_mark(0, "NOSUCHKEY", 3);
        assert_eq!(store.books[0].read_state, 1);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_place_read_through_outranks_the_librarys_mark() {
        let mut store = Store::default();
        store.remember(&[marked(100.0, 3)]);
        assert!(
            store.books[0].finished,
            "the place states this one read through"
        );

        // `restart` clears `finished`, with `read_state` unchanged through
        // it.
        assert!(store.restart(938_018, "B00OKPCRLG"));
        assert!(!store.books[0].finished);
        store.remember(&[marked(100.0, 3)]);
        assert_eq!(store.books[0].percent, 0.0);
        assert!(!store.books[0].finished);
    }

    #[test]
    fn the_librarys_mark_round_trips_and_an_older_row_takes_the_next_one() {
        let dir = scratch("readstate");
        let mut store = Store::default();
        store.remember(&[marked(40.0, 1)]);
        store.save(&dir).expect("a written store");
        let back = Store::load(&dir);
        assert_eq!(back.books[0].read_state, 1);
        assert!(back.books[0].finished);

        // Twelve fields: a `b` row without `read_state`. The next value
        // `remember` takes is new to it.
        std::fs::create_dir_all(&dir).expect("a directory to write in");
        let row = "b\t938018\tB00OKPCRLG\tBible\tBerlin\t\ten\t40.000000\t1\t\t/mnt/us/a.kfx\t0\t";
        std::fs::write(Store::file(&dir), format!("{HEADER}\n{row}\n")).expect("a store");
        let mut store = Store::load(&dir);
        assert_eq!(store.books[0].read_state, -1);
        store.remember(&[marked(40.0, 2)]);
        assert!(store.books[0].finished, "READ_AUTOMATIC reached no record");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_place_read_through_marks_its_own_record() {
        // `set_finished` is not called: `remember` marks this one.
        let mut store = Store::default();
        store.remember(&[shelved(938_018, "B00OKPCRLG", "Bible", 99.8)]);
        assert!(store.books[0].finished);

        // `finished` holds when `percent` turns back.
        store.remember(&[shelved(938_018, "B00OKPCRLG", "Bible", 71.0)]);
        assert_eq!(store.books[0].percent, 71.0);
        assert!(store.books[0].finished);

        // A `b` row carrying `finished` at 0 reads back marked.
        let dir = scratch("through");
        std::fs::create_dir_all(&dir).expect("a directory to write in");
        let row = "b\t938018\tB00OKPCRLG\tBible\tBerlin\t\ten\t100.000000\t1\t\t/mnt/us/a.kfx\t0";
        std::fs::write(Store::file(&dir), format!("{HEADER}\n{row}\n")).expect("a store");
        assert!(Store::load(&dir).books[0].finished);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_restart_gives_up_the_place_and_waits_for_one_before_it() {
        let dir = scratch("restart");
        let mut store = Store::default();
        let shelf = [shelved(938_018, "B00OKPCRLG", "Bible", 100.0)];
        store.remember(&shelf);
        assert!(store.books[0].finished);

        assert!(store.restart(938_018, "B00OKPCRLG"));
        assert_eq!(store.books[0].percent, 0.0);
        assert_eq!(store.books[0].restart, Some(100.0));
        assert!(!store.books[0].finished);

        // `remember` at or past `restart` leaves `percent` at 0.
        store.remember(&shelf);
        assert_eq!(store.books[0].percent, 0.0);
        assert!(!store.books[0].finished);
        store.save(&dir).expect("a written store");
        assert_eq!(Store::load(&dir).books[0].restart, Some(100.0));

        // A `percent` under `restart` clears it, and `remember` takes over.
        store.remember(&[shelved(938_018, "B00OKPCRLG", "Bible", 4.0)]);
        assert_eq!(
            (store.books[0].percent, store.books[0].restart),
            (4.0, None)
        );
        store.remember(&[shelved(938_018, "B00OKPCRLG", "Bible", 100.0)]);
        assert_eq!(store.books[0].percent, 100.0);
        assert!(
            store.books[0].finished,
            "and the second pass marks it again"
        );

        // No record, and a record at its beginning, have nothing to
        // give up.
        assert!(!store.restart(0, "NOSUCHKEY"));
        store.remember(&[shelved(555, "B00OTHER", "Another", 0.0)]);
        assert!(!store.restart(555, "B00OTHER"));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_book_record_round_trips_through_the_file() {
        let dir = scratch("books");
        let mut store = Store::default();
        store.remember(&[shelved(
            938_018,
            "B00OKPCRLG",
            "The Jewish\tStudy Bible",
            76.125,
        )]);
        store.save(&dir).expect("a written store");

        let back = Store::load(&dir);
        assert_eq!(back, store);
        let book = &back.books[0];
        // A tab in a title reads as no field break.
        assert_eq!(book.title, "The Jewish Study Bible");
        assert!((book.percent - 76.125).abs() < 1e-6);
        assert!(book.is_book());
        assert_eq!(
            book.location,
            "/mnt/us/documents/The Jewish Study Bible.kfx"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_book_gone_from_the_device_keeps_the_file_it_was_read_from() {
        let mut store = Store::default();
        store.remember(&[shelved(1, "B01", "A Book", 10.0)]);
        // The library row a deletion leaves behind states no location.
        let mut library = shelved(0, "B01", "A Book", -1.0);
        library.location = String::new();
        library.on_device = false;
        store.remember(&[library]);
        let book = &store.books[0];
        assert!(!book.on_device, "the device holds it");
        assert_eq!(book.location, "/mnt/us/documents/A Book.kfx");
    }

    #[test]
    fn the_catalog_refreshes_a_record_and_never_removes_one() {
        let mut store = Store::default();
        assert_eq!(store.remember(&[shelved(1, "B01", "A Book", 10.0)]), 1);
        // The same reading again changes nothing.
        assert_eq!(store.remember(&[shelved(1, "B01", "A Book", 10.0)]), 0);
        // Read further, and the catalog's new figure is taken.
        assert_eq!(store.remember(&[shelved(1, "B01", "A Book", 40.0)]), 1);
        assert_eq!(store.books[0].percent, 40.0);

        // The catalog stops naming the book. Its record stands, marked for
        // where the book sits.
        assert_eq!(store.remember(&[]), 1);
        assert_eq!(store.books.len(), 1);
        assert!(!store.books[0].on_device);
        assert_eq!(store.books[0].title, "A Book");
        assert_eq!(store.books[0].percent, 40.0);
        // A second empty pass has nothing left to change.
        assert_eq!(store.remember(&[]), 0);
    }

    /// A record read to 88%, then to `progress`, then with its catalog row
    /// deleted.
    fn read_on_to(progress: Option<f64>) -> Store {
        let mut store = Store {
            sessions: vec![session(
                "2026-08-27T10:34:40",
                "2026-08-27T11:03:20",
                938_018,
                1_720,
            )],
            ends: Vec::new(),
            keys: Vec::new(),
            counters: Vec::new(),
            pairs: Vec::new(),
            books: Vec::new(),
            mark: String::new(),
            mark_offset: None,
            floor: String::new(),
            cleared: Vec::new(),
        };
        store.sessions[0].progress = progress;
        store.remember(&[shelved(938_018, "B00OKPCRLG", "A Book", 88.0)]);
        assert_eq!(store.books[0].percent, 88.0);
        let mut archived = shelved(938_018, "B00OKPCRLG", "A Book", -1.0);
        archived.on_device = false;
        store.remember(&[archived]);
        store
    }

    #[test]
    fn a_record_takes_the_percentage_its_newest_sitting_states() {
        // `%Left` of 0 on the last turn, past the 88% the catalog stated.
        assert_eq!(read_on_to(Some(1.0)).books[0].percent, 100.0);
        // `%Left` of 0.6 on the last turn, short of it: the catalog's stands.
        assert_eq!(read_on_to(Some(0.4)).books[0].percent, 88.0);
    }

    #[test]
    fn a_deleted_book_keeps_the_place_the_catalog_stated_for_it() {
        // The shape the two disagree in: `p_percentFinished` reads 100 and the
        // last turn's `%Left` is a page short of it.
        let mut store = Store {
            sessions: vec![session(
                "2026-08-27T10:34:40",
                "2026-08-27T11:03:20",
                938_018,
                1_720,
            )],
            ..Store::default()
        };
        store.sessions[0].progress = Some(0.994_69);
        store.remember(&[shelved(938_018, "B00OKPCRLG", "A Book", 100.0)]);
        assert_eq!(store.books[0].percent, 100.0);
        assert!(store.books[0].finished);

        // Deleted: the downloaded row goes and with it `p_percentFinished`.
        let mut archived = shelved(938_018, "B00OKPCRLG", "A Book", -1.0);
        archived.on_device = false;
        store.remember(&[archived.clone()]);
        assert_eq!(store.books[0].percent, 100.0, "the place was walked back");
        // And every later pass leaves it where it stands.
        store.remember(&[archived]);
        assert_eq!(store.books[0].percent, 100.0);
    }

    #[test]
    fn a_record_the_catalog_never_placed_takes_the_place_its_sittings_state() {
        let mut store = Store {
            sessions: vec![session(
                "2026-08-27T10:34:40",
                "2026-08-27T11:03:20",
                938_018,
                1_720,
            )],
            ..Store::default()
        };
        store.sessions[0].progress = Some(0.4);
        store.remember(&[shelved(938_018, "B00OKPCRLG", "A Book", -1.0)]);
        assert_eq!(store.books[0].percent, 40.0);
    }

    #[test]
    fn a_re_download_hands_the_percentage_back_to_the_catalog() {
        let mut store = read_on_to(Some(1.0));
        store.remember(&[shelved(938_018, "B00OKPCRLG", "A Book", 99.5)]);
        assert_eq!(store.books[0].percent, 99.5);
    }

    #[test]
    fn a_sitting_stating_no_percentage_leaves_the_record_alone() {
        assert_eq!(read_on_to(None).books[0].percent, 88.0);
    }

    #[test]
    fn a_library_row_names_a_record_without_unseating_its_extent() {
        let mut store = Store::default();
        store.remember(&[shelved(
            938_018,
            "B00OKPCRLG",
            "The Jewish Study Bible",
            76.0,
        )]);
        // The same book with no extent and no percentage, a title only.
        let mut cloud = shelved(0, "B00OKPCRLG", "The Jewish Study Bible", -1.0);
        cloud.on_device = false;
        store.remember(&[cloud]);

        assert_eq!(store.books.len(), 1);
        let record = &store.books[0];
        assert_eq!(record.extent, 938_018, "the log keys sittings by this");
        assert_eq!(record.percent, 76.0);
        assert!(!record.on_device);
    }

    #[test]
    fn a_record_is_reached_by_its_key_when_the_extent_misses() {
        let mut store = Store::default();
        store.remember(&[shelved(
            938_018,
            "B00OKPCRLG",
            "The Jewish Study Bible",
            76.0,
        )]);
        assert_eq!(
            store.book_for(938_018, None).map(|b| &b.title[..]),
            Some("The Jewish Study Bible")
        );
        // A position no `BookEndPosition` line mapped reaches the book by key.
        assert_eq!(
            store.book_for(4_242, Some("B00OKPCRLG")).map(|b| b.extent),
            Some(938_018)
        );
        assert!(store.book_for(4_242, Some("")).is_none());
        assert!(store.book_for(4_242, None).is_none());
    }

    /// A store holding one sitting, with the log read as far as `mark`.
    fn one_sitting(mark: &str) -> Store {
        Store {
            sessions: vec![session(
                "2026-08-07T10:15:01",
                "2026-08-07T10:55:43",
                148_207,
                2_400,
            )],
            ends: Vec::new(),
            keys: Vec::new(),
            counters: Vec::new(),
            pairs: Vec::new(),
            books: Vec::new(),
            mark: mark.into(),
            mark_offset: None,
            floor: String::new(),
            cleared: Vec::new(),
        }
    }

    /// A record marked at `mark` under `offset`, with the same stamp as its
    /// floor: both are wall-clock and both must follow the clock.
    fn on_clock(mark: &str, offset: Option<i64>) -> Store {
        Store {
            mark: mark.into(),
            mark_offset: offset,
            floor: mark.into(),
            ..Store::default()
        }
    }

    #[test]
    fn a_clock_stepping_back_pulls_the_mark_and_the_floor_with_it() {
        // Berlin at +2 to Berlin at +1: the hour between 02:00 and 03:00 is
        // written twice, and the mark would bar the second one.
        let mut store = on_clock("261025:023000", Some(7200));
        assert_eq!(store.follow_clock(Some(3600)), 3600);
        assert_eq!(store.mark, "261025:013000");
        assert_eq!(store.floor, "261025:013000");
        assert_eq!(store.mark_offset, Some(3600));

        // +8 home to +1, across a day boundary.
        let mut far = on_clock("261102:043000", Some(8 * 3600));
        assert_eq!(far.follow_clock(Some(3600)), 7 * 3600);
        assert_eq!(far.mark, "261101:213000");
    }

    #[test]
    fn a_clock_that_has_not_stepped_back_moves_nothing() {
        // Forward: the mark falls further behind, which skips no line.
        let mut ahead = on_clock("260909:130922", Some(7200));
        assert_eq!(ahead.follow_clock(Some(8 * 3600)), 0);
        assert_eq!(ahead.mark, "260909:130922");
        assert_eq!(ahead.mark_offset, Some(8 * 3600));

        // The same offset, and a step under a minute, are not a zone changing.
        let mut same = on_clock("260909:130922", Some(7200));
        assert_eq!(same.follow_clock(Some(7200)), 0);
        assert_eq!(same.follow_clock(Some(7170)), 0);
        assert_eq!(same.mark, "260909:130922");
    }

    #[test]
    fn a_record_with_no_offset_takes_one_and_stands_still() {
        // Nothing to compare against.
        let mut fresh = on_clock("260909:130922", None);
        assert_eq!(fresh.follow_clock(Some(7260)), 0);
        assert_eq!(fresh.mark, "260909:130922");
        assert_eq!(fresh.mark_offset, Some(7260));

        // A device stating no zone at all leaves the record as it stands.
        let mut blind = on_clock("260909:130922", Some(7200));
        assert_eq!(blind.follow_clock(None), 0);
        assert_eq!(blind.mark_offset, Some(7200));
    }

    #[test]
    fn the_mark_carries_the_clock_it_was_written_under() {
        let store = on_clock("260909:130922", Some(7260));
        assert!(store.text().contains("m\t260909:130922\t7260\n"));
        let read = Store::parse(&store.text());
        assert_eq!(read.mark_offset, Some(7260));
        // A row carrying no offset states none, and the mark stands.
        let old = Store::parse("#readinglog\t2\nm\t260909:130922\n");
        assert_eq!(old.mark, "260909:130922");
        assert_eq!(old.mark_offset, None);
    }

    #[test]
    fn the_pass_rewinds_to_a_sitting_that_could_still_be_running() {
        // The log stops four minutes past the sitting's end.
        assert_eq!(one_sitting("260807:105943").read_from(), "260807:101501");
    }

    #[test]
    fn the_pass_does_not_rewind_to_a_sitting_the_log_has_closed() {
        // A whole day past the sitting's end.
        assert_eq!(one_sitting("260808:213000").read_from(), "260808:213000");
        // The sitting ends at 10:55:43: 29 minutes on is open, 30 is closed.
        assert_eq!(one_sitting("260807:112443").read_from(), "260807:101501");
        assert_eq!(one_sitting("260807:112543").read_from(), "260807:112543");
    }

    #[test]
    fn a_record_holding_no_sitting_reads_the_whole_log() {
        let bare = Store {
            mark: "260808:213000".into(),
            mark_offset: None,
            ..Store::default()
        };
        assert_eq!(bare.read_from(), "");
        assert_eq!(Store::default().read_from(), "");
        // `floor` bounds the pass.
        let floored = Store {
            mark: "260808:213000".into(),
            mark_offset: None,
            floor: "260808:213000".into(),
            ..Store::default()
        };
        assert_eq!(floored.read_from(), "260808:213000");
        assert_eq!(one_sitting("").read_from(), "260807:101501");
    }

    /// The two lines a sitting is measured from, at `stamp` with the counter at
    /// `total_ms`.
    fn page(stamp: &str, total_ms: i64) -> String {
        format!(
            "{stamp} cvm[6144]: I ReadingTimerController:Information::NextPage,Verdict:Processed,\
             IntervalTime:39890,TotalTime:{total_ms},TotalWords:49583,\
             CurrentPos:YJPosition: AfQJAAAAAAAA:54205,EndPos:YJPosition: AbcVAAAPAAAA:148207,\
             PosLeft:94002,%Left:0.645;"
        )
    }

    /// A page turn of the book whose `EndPos` is `at`, carrying `total_ms` and
    /// `words`.
    fn turn(stamp: &str, at: i64, total_ms: i64, words: i64) -> String {
        format!(
            "{stamp} cvm[6144]: I ReadingTimerController:Information::NextPage,Verdict:Processed,\
             IntervalTime:39890,TotalTime:{total_ms},TotalWords:{words},\
             CurrentPos:YJPosition: AfQJAAAAAAAA:54205,EndPos:YJPosition: AbcVAAAPAAAA:{at},\
             PosLeft:94002,%Left:0.645;"
        )
    }

    fn card(file: &str, total_ms: i64, words: i64) -> sidecar::Counter {
        sidecar::Counter {
            file: file.into(),
            total_ms,
            words,
        }
    }

    /// A store holding one book's reading, and nothing that names it: the
    /// state a first parse is in when the book was deleted before it ran.
    fn read_but_unnamed() -> Store {
        let mut store = Store::default();
        store.absorb(
            &[
                turn("260807:101501", 148_207, 7_390_020, 49_583),
                turn("260807:101543", 148_207, 7_431_463, 49_712),
            ],
            "",
        );
        assert!(store.book_for(148_207, None).is_none());
        store
    }

    #[test]
    fn a_sidecar_names_the_reading_of_a_book_the_catalog_has_forgotten() {
        let mut store = read_but_unnamed();
        // `timer.model` holds the highest counter the log showed.
        assert_eq!(
            store.recover(&[card("a deleted book", 7_431_463, 49_712)]),
            1
        );
        let book = store.book_for(148_207, None).expect("the book named");
        assert_eq!(book.title, "a deleted book");
        assert_eq!(book.extent, 148_207);
        assert!(!book.on_device);
        assert!(book.is_book(), "it draws a row");
        // The place comes off the sitting, the catalog having none to state.
        assert!((book.percent - 35.5).abs() < 0.1, "{}", book.percent);
        assert_eq!(store.pairs, vec![(148_207, "a deleted book".to_string())]);
    }

    #[test]
    fn a_counter_short_of_the_last_one_logged_names_nothing() {
        let mut store = read_but_unnamed();
        // A counter below the class's highest.
        assert_eq!(store.recover(&[card("some book", 7_390_020, 49_583)]), 0);
        assert!(store.book_for(148_207, None).is_none());
        assert!(store.pairs.is_empty());
    }

    #[test]
    fn a_book_opened_and_never_read_names_nothing() {
        let mut store = read_but_unnamed();
        // Every untimed class shares a counter of zero; none of them is named.
        assert_eq!(store.recover(&[card("never read", 0, 0)]), 0);
        assert!(store.pairs.is_empty());
    }

    #[test]
    fn a_class_two_sidecars_both_claim_is_left_alone() {
        let mut store = read_but_unnamed();
        let named = store.recover(&[
            card("one book", 7_431_463, 49_712),
            card("another book", 7_431_463, 49_712),
        ]);
        assert_eq!(named, 0, "one candidate or nothing, both ways");
        assert!(store.pairs.is_empty());
    }

    #[test]
    fn a_book_the_catalog_still_names_is_never_inferred_over() {
        let mut store = read_but_unnamed();
        store.remember(&[shelved(148_207, "B01", "The Catalog's Own Title", 40.0)]);
        assert_eq!(
            store.recover(&[card("a deleted book", 7_431_463, 49_712)]),
            0
        );
        let book = store.book_for(148_207, None).expect("the catalog's record");
        assert_eq!(book.title, "The Catalog's Own Title");
        assert!(store.pairs.is_empty());
    }

    #[test]
    fn a_catalog_row_arriving_later_takes_the_book_back() {
        let mut store = read_but_unnamed();
        assert_eq!(
            store.recover(&[card("a deleted book", 7_431_463, 49_712)]),
            1
        );
        // `remember` states the book again.
        store.remember(&[shelved(148_207, "B01", "The Catalog's Own Title", 40.0)]);
        store.recover(&[card("a deleted book", 7_431_463, 49_712)]);
        assert!(store.pairs.is_empty(), "the inference stands down");
        assert_eq!(
            store.book_for(148_207, None).expect("a record").title,
            "The Catalog's Own Title"
        );
    }

    #[test]
    fn the_counter_and_the_pairing_both_survive_the_file() {
        let dir = scratch("sidecar-rows");
        let mut store = read_but_unnamed();
        store.recover(&[card("a deleted book", 7_431_463, 49_712)]);
        store.save(&dir).expect("a written store");
        let back = Store::load(&dir);
        assert_eq!(back, store);
        assert_eq!(back.counters, vec![(148_207, 7_431_463, 49_712)]);
        assert_eq!(back.pairs, vec![(148_207, "a deleted book".to_string())]);
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// A page turn of a mobi8 book, whose positions are `HTMLPosition` and
    /// whose `EndPos` is one under `p_contentSize`.
    fn mobi8_turn(stamp: &str, at: i64, total_ms: i64, words: i64) -> String {
        format!(
            "{stamp} cvm[6144]: I ReadingTimerController:Information::NextPage,Verdict:Processed,\
             IntervalTime:785,TotalTime:{total_ms},TotalWords:{words},\
             CurrentPos:HTMLPosition:7731097,EndPos:HTMLPosition:{at},PosLeft:12155392,\
             %Left:0.6112;"
        )
    }

    #[test]
    fn a_mobi8_sidecar_names_a_mobi8_class() {
        let mut store = Store::default();
        store.absorb(
            &[
                "260906:192400 cvm[6144]: I ReadingTimerController:Information::OpenBook,\
                 BookEndPosition.FromBook:HTMLPosition:1543288;"
                    .to_string(),
                mobi8_turn("260906:192404", 1_543_288, 329_785, 1_905),
                mobi8_turn("260906:192504", 1_543_288, 1_003_773, 1_834),
            ],
            "",
        );
        // `from_book` puts the mobi8 book end back at `p_contentSize`.
        assert_eq!(store.extent_of(1_543_288), 1_543_289);
        assert!(store.book_for(1_543_289, None).is_none());
        // The counters an `.azw3f` states, in `timer.model`'s own order.
        assert_eq!(store.recover(&[card("lovecraft", 1_003_773, 1_834)]), 1);
        let book = store.book_for(1_543_289, None).expect("the book named");
        assert_eq!(book.title, "lovecraft");
        assert_eq!(book.extent, 1_543_289);
    }

    #[test]
    fn a_sidecar_named_for_a_key_reaches_the_row_holding_that_key() {
        let mut store = read_but_unnamed();
        // The row a deletion leaves: a key and a title, no location, no extent.
        let mut archived = shelved(0, "B00OKPCRLG", "The Jewish Study Bible", -1.0);
        archived.location = String::new();
        archived.on_device = false;
        store.remember(&[archived]);
        let named = store.recover(&[card("The Jewish Study Bible_B00OKPCRLG", 7_431_463, 49_712)]);
        assert_eq!(named, 1);
        let book = store.book_for(148_207, None).expect("the book named");
        assert_eq!(book.title, "The Jewish Study Bible");
        assert_eq!(book.author, "Adele Berlin");
        assert!(
            book.thumbnail.ends_with("t.jpg"),
            "the jacket comes with it"
        );
        assert_eq!(store.books.len(), 1, "one record, not two");
    }

    #[test]
    fn a_name_carrying_no_key_is_titled_by_the_file() {
        let mut store = read_but_unnamed();
        let mut archived = shelved(0, "B00OKPCRLG", "The Jewish Study Bible", -1.0);
        archived.location = String::new();
        store.remember(&[archived]);
        store.recover(&[card("some sideload", 7_431_463, 49_712)]);
        let book = store.book_for(148_207, None).expect("the book named");
        assert_eq!(book.title, "some sideload");
    }

    #[test]
    fn a_record_already_holding_an_extent_is_never_taken_by_a_name() {
        let mut store = read_but_unnamed();
        store.remember(&[shelved(999_999, "B00OKPCRLG", "Another Book", 10.0)]);
        store.recover(&[card("Another Book_B00OKPCRLG", 7_431_463, 49_712)]);
        let other = store
            .books
            .iter()
            .find(|b| b.cde_key == "B00OKPCRLG")
            .expect("the live book");
        assert_eq!(other.extent, 999_999, "its own extent stands");
        assert_eq!(
            store
                .book_for(148_207, None)
                .expect("the recovered book")
                .title,
            "Another Book_B00OKPCRLG"
        );
    }

    #[test]
    fn a_second_pass_over_the_same_log_changes_nothing() {
        let lines = vec![
            page("260807:101501", 7_390_020),
            page("260807:101543", 7_431_463),
        ];
        let mut store = Store::default();
        let (added, extended) = store.absorb(&lines, "");
        assert_eq!((added, extended), (1, 0));
        assert_eq!(store.sessions.len(), 1);
        assert_eq!(store.sessions[0].seconds, 41);
        assert_eq!(store.mark, "260807:101543");

        let again = store.clone();
        let from = store.read_from();
        let (added, extended) = store.absorb(&lines, &from);
        assert_eq!((added, extended), (0, 1), "the same sitting, re-measured");
        assert_eq!(store, again);
    }

    #[test]
    fn a_sitting_still_running_grows_instead_of_splitting() {
        let first = vec![
            page("260807:101501", 7_390_020),
            page("260807:101543", 7_431_463),
        ];
        let mut store = Store::default();
        store.absorb(&first, "");
        assert_eq!(store.sessions[0].seconds, 41);
        assert_eq!(store.sessions[0].ended_at, "2026-08-07T10:15:43");

        // The next pass re-reads from the sitting's own start and sees two more
        // turns of the same run.
        let from = store.read_from();
        let mut second = first.clone();
        second.push(page("260807:101625", 7_473_000));
        second.push(page("260807:101710", 7_518_000));
        let (added, extended) = store.absorb(&second, &from);

        assert_eq!((added, extended), (0, 1));
        assert_eq!(store.sessions.len(), 1, "one sitting, not two");
        assert_eq!(store.sessions[0].started_at, "2026-08-07T10:15:01");
        assert_eq!(store.sessions[0].ended_at, "2026-08-07T10:17:10");
        assert_eq!(store.sessions[0].seconds, 127);
    }

    #[test]
    fn a_sitting_older_than_the_pass_is_left_alone() {
        let mut store = Store {
            sessions: vec![session(
                "2026-08-01T09:00:00",
                "2026-08-01T09:30:00",
                999,
                1_800,
            )],
            ..Store::default()
        };
        let lines = vec![
            page("260807:101501", 7_390_020),
            page("260807:101543", 7_431_463),
        ];
        let (added, _) = store.absorb(&lines, "260807:000000");
        assert_eq!(added, 1);
        assert_eq!(store.sessions.len(), 2);
        assert_eq!(store.sessions[0].end_position, 999, "the old one survives");
    }

    /// Two books' reading, with a record for each and the log read to `mark`.
    fn two_books() -> Store {
        let mut store = Store {
            sessions: vec![
                session("2026-08-07T10:15:01", "2026-08-07T10:55:43", 148_207, 2_400),
                session("2026-08-08T09:00:00", "2026-08-08T09:30:00", 148_207, 1_800),
                session("2026-08-09T21:00:00", "2026-08-09T21:30:00", 938_018, 1_800),
            ],
            books: vec![
                BookRecord {
                    extent: 148_207,
                    cde_key: "B00OKPCRLG".into(),
                    title: "A Book".into(),
                    finished: true,
                    restart: Some(62.0),
                    ..BookRecord::default()
                },
                BookRecord {
                    extent: 938_018,
                    cde_key: "B00SECOND".into(),
                    title: "Another".into(),
                    ..BookRecord::default()
                },
            ],
            mark: "260810:120000".into(),
            mark_offset: None,
            ..Store::default()
        };
        for s in &mut store.sessions {
            s.asin = None;
        }
        store.sort();
        store
    }

    #[test]
    fn clearing_a_book_takes_its_sittings_and_keeps_its_record() {
        let mut store = two_books();
        assert_eq!(store.clear_book(148_207, "B00OKPCRLG"), 2);
        assert_eq!(store.sessions.len(), 1, "the other book reads on");
        assert_eq!(store.sessions[0].end_position, 938_018);
        assert_eq!(store.books.len(), 2, "the record stands");
        let held = &store.books[0];
        assert_eq!(held.title, "A Book", "the record stands, and names itself");
        assert!(held.kept, "and stands on the lists at zero");
        // `restart` took `finished` and `percent` with the sittings.
        assert!(!held.finished);
        assert_eq!(held.percent, 0.0);
        assert_eq!(held.restart, Some(62.0), "the place it gave up");
        assert_eq!(
            store.cleared,
            vec![Cleared {
                extent: 148_207,
                key: "B00OKPCRLG".into(),
                at: "260810:120000".into(),
            }]
        );
    }

    #[test]
    fn forgetting_a_book_takes_the_record_with_the_reading() {
        let mut store = two_books();
        assert_eq!(store.forget_book(148_207, "B00OKPCRLG"), 2);
        assert_eq!(store.sessions.len(), 1);
        assert_eq!(store.books.len(), 1, "the record went too");
        assert_eq!(store.books[0].extent, 938_018);
        assert_eq!(store.cleared.len(), 1, "and is still held back");
        assert_eq!(store.cleared[0].extent, 148_207);
    }

    #[test]
    fn clearing_a_book_the_record_does_not_hold_does_nothing() {
        let mut store = two_books();
        let before = store.clone();
        assert_eq!(store.clear_book(1, "B00NOTHERE"), 0);
        assert_eq!(store, before);
    }

    #[test]
    fn a_cleared_book_is_not_re_derived_by_the_whole_log() {
        let mut store = two_books();
        store.clear_book(148_207, "B00OKPCRLG");
        // Every line the book was measured from.
        let lines = vec![
            page("260807:101501", 7_390_020),
            page("260807:101543", 7_431_463),
        ];
        store.absorb(&lines, "");
        assert!(
            store.sessions.iter().all(|s| s.end_position != 148_207),
            "the log handed the book back"
        );
    }

    #[test]
    fn a_wipe_empties_the_record_and_floors_the_pass() {
        let mut store = two_books();
        store.cleared.push(Cleared {
            extent: 555,
            key: "B00GONE".into(),
            at: "260809:090000".into(),
        });
        assert!(store.wipe());
        assert!(store.sessions.is_empty());
        assert!(store.books.is_empty());
        assert!(store.ends.is_empty());
        assert_eq!(store.mark, "260810:120000", "the pass knows where it read");
        assert_eq!(store.floor, "260810:120000");
        assert_eq!(store.cleared.len(), 1, "a book taken off stays off");
        assert_eq!(store.read_from(), "260810:120000");
    }

    #[test]
    fn a_record_that_has_read_nothing_has_nothing_to_wipe() {
        let mut store = Store::default();
        assert!(!store.wipe());
        assert_eq!(store, Store::default());
    }

    #[test]
    fn a_stale_header_takes_neither_the_mark_nor_the_floor() {
        let dir = scratch("floored");
        let mut store = two_books();
        store.wipe();
        superseded(&dir, &store);

        let read = Store::load(&dir);
        assert_eq!(read.mark, store.mark, "the stamp took the mark");
        assert_eq!(read.floor, "260810:120000", "the stamp took the floor");
        assert_eq!(read.read_from(), "260810:120000");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_pass_after_a_wipe_folds_nothing_from_before_it() {
        let mut store = Store::default();
        let lines = vec![
            page("260807:101501", 7_390_020),
            page("260807:101543", 7_431_463),
        ];
        store.absorb(&lines, "");
        assert_eq!(store.sessions.len(), 1);
        store.wipe();

        let from = store.read_from();
        store.absorb(&lines, &from);
        assert!(store.sessions.is_empty(), "the log handed the era back");
    }

    #[test]
    fn a_wiped_record_holds_its_stamps_and_nothing_else() {
        let dir = scratch("wiped");
        let mut store = two_books();
        store.cleared.push(Cleared {
            extent: 555,
            key: "B00GONE".into(),
            at: "260809:090000".into(),
        });
        store.wipe();
        store.save(&dir).expect("a written store");

        let text = std::fs::read_to_string(Store::file(&dir)).expect("the record");
        let rows: Vec<&str> = text.lines().collect();
        assert_eq!(
            rows,
            [
                HEADER,
                "m\t260810:120000",
                "f\t260810:120000",
                "c\t555\tB00GONE\t260809:090000",
            ]
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_rebuild_from_the_log_keeps_what_the_log_is_too_young_to_know() {
        let dir = scratch("rebuild");
        let live = dir.join("messages");
        std::fs::write(
            &live,
            format!(
                "{}\n{}\n",
                page("260807:101501", 7_390_020),
                page("260807:101543", 7_431_463)
            ),
        )
        .expect("a log to read");

        let mut store = Store {
            // Older than the lines `live` holds.
            sessions: vec![session(
                "2026-07-01T09:00:00",
                "2026-07-01T09:30:00",
                999,
                1_800,
            )],
            mark: "260807:101543".into(),
            mark_offset: None,
            floor: "260807:120000".into(),
            ..Store::default()
        };
        let added = store.rebuild_from(&live, &dir.join("none"), &dir.join("none"), &mut |_, _| {});

        assert_eq!(added, 1, "the log's own sitting did not come back");
        assert_eq!(store.sessions.len(), 2, "the older sitting was lost");
        assert!(store.sessions.iter().any(|s| s.end_position == 999));
        assert!(store.floor.is_empty(), "the floor stayed up");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_sitting_carrying_its_own_counter_reaches_its_sidecar_with_no_log() {
        // One sitting holding the counter it was last seen at, with no `t`
        // row beside it.
        let mut store = Store {
            sessions: vec![Session {
                end_counter_ms: Some(900_000),
                end_words: Some(4_100),
                ..session("2026-08-07T10:15:01", "2026-08-07T10:20:00", 148_207, 299)
            }],
            ..Store::default()
        };
        assert!(store.counters.is_empty());
        assert!(!store.wants_the_logs(), "the logs hold nothing this needs");

        assert_eq!(store.recover(&[card("Vom Kriege", 900_000, 4_100)]), 1);
        assert_eq!(store.books.len(), 1);
        // A counter no sitting states names nothing.
        assert_eq!(store.recover(&[card("Anderes Buch", 1, 2)]), 0);
    }

    #[test]
    fn a_sitting_written_before_the_counters_were_held_still_wants_the_logs() {
        let store = Store {
            sessions: vec![session(
                "2026-08-07T10:15:01",
                "2026-08-07T10:20:00",
                148_207,
                299,
            )],
            ..Store::default()
        };
        assert!(store.wants_the_logs(), "an old row was taken as complete");
    }

    #[test]
    fn reading_the_classes_again_reaches_a_sitting_recorded_without_one() {
        let dir = scratch("classes");
        let live = dir.join("messages");
        std::fs::write(
            &live,
            format!(
                "{}\n{}\n",
                turn("260807:101501", 148_207, 900_000, 4_100),
                turn("260807:101543", 990_111, 1_500_000, 7_000)
            ),
        )
        .expect("a log to read");

        // One sitting, no counter for a sidecar to match, and a mark past
        // everything the log holds.
        let mut store = Store {
            sessions: vec![session(
                "2026-08-07T10:15:01",
                "2026-08-07T10:20:00",
                148_207,
                299,
            )],
            mark: "260807:101543".into(),
            mark_offset: None,
            ..Store::default()
        };
        assert!(store.counters.is_empty());
        let learned =
            store.classes_from(&live, &dir.join("none"), &dir.join("none"), &mut |_, _| {});

        assert_eq!(learned, 2, "the log's classes did not reach the record");
        assert_eq!(store.sessions.len(), 1, "a sitting came in with them");
        // `card` states the counter the sitting holds.
        let named = store.recover(&[card("Vom Kriege", 900_000, 4_100)]);
        assert_eq!(named, 1, "the counter reached no sidecar");
        assert_eq!(store.books.len(), 1);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn reading_the_classes_again_hands_back_no_reading_the_reader_emptied() {
        let dir = scratch("classes-floored");
        let live = dir.join("messages");
        std::fs::write(
            &live,
            format!(
                "{}\n{}\n",
                page("260807:101501", 7_390_020),
                page("260807:101543", 7_431_463)
            ),
        )
        .expect("a log to read");

        // The two stamps that hold the parser back, both above the log.
        let mut store = having_cleared("260807:120000");
        store.mark = "260807:101543".into();
        store.floor = "260807:120000".into();
        store.classes_from(&live, &dir.join("none"), &dir.join("none"), &mut |_, _| {});

        assert!(store.sessions.is_empty(), "emptied reading came back");
        assert_eq!(store.floor, "260807:120000", "the floor came down");
        assert_eq!(store.cleared.len(), 1, "a book's stamp came off");
        assert!(!store.counters.is_empty(), "no class reached the record");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_rebuild_restates_a_sitting_the_log_still_holds() {
        let dir = scratch("rebuild-measure");
        let live = dir.join("messages");
        std::fs::write(
            &live,
            format!(
                "{}\n{}\n",
                turn("260807:101501", 148_207, 900_000, 4_100),
                turn("260807:101543", 148_207, 940_000, 4_900)
            ),
        )
        .expect("a log to read");

        // The same sitting the log states, measured by an older parse at zero.
        let mut store = Store {
            sessions: vec![Session {
                words: 0,
                ..session("2026-08-07T10:15:01", "2026-08-07T10:15:43", 148_207, 42)
            }],
            mark: "260807:101543".into(),
            mark_offset: None,
            ..Store::default()
        };
        store.rebuild_from(&live, &dir.join("none"), &dir.join("none"), &mut |_, _| {});

        assert_eq!(store.sessions.len(), 1, "the sitting was stated twice");
        assert_eq!(store.sessions[0].words, 800, "the stale measurement stood");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_rebuild_leaves_a_book_taken_off_the_record_off_it() {
        let dir = scratch("rebuild-cleared");
        let live = dir.join("messages");
        std::fs::write(
            &live,
            format!(
                "{}\n{}\n",
                page("260807:101501", 7_390_020),
                page("260807:101543", 7_431_463)
            ),
        )
        .expect("a log to read");

        let mut store = having_cleared("260807:120000");
        store.mark = "260807:101543".into();
        store.floor = "260807:120000".into();
        let added = store.rebuild_from(&live, &dir.join("none"), &dir.join("none"), &mut |_, _| {});

        assert_eq!(added, 0);
        assert!(store.sessions.is_empty(), "a cleared book came back");
        assert_eq!(store.cleared.len(), 1, "the stamp came off");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_heal_corrects_the_row_the_logs_reach_and_keeps_the_one_they_do_not() {
        let dir = scratch("heal-rows");
        let live = dir.join("messages");
        std::fs::write(
            &live,
            format!(
                "{}\n{}\n",
                page("260807:101501", 7_390_020),
                page("260807:101543", 7_431_463)
            ),
        )
        .expect("a log to read");

        // The row the log restates, holding an older parse's figures, and one
        // from before the log the device still keeps.
        let mut store = Store {
            sessions: vec![
                Session {
                    started_at: "2026-08-07T10:15:01".into(),
                    ended_at: "2026-08-07T10:15:43".into(),
                    end_position: 148_207,
                    seconds: 9,
                    awake_seconds: 0,
                    ..Session::default()
                },
                Session {
                    started_at: "2020-01-01T00:00:00".into(),
                    ended_at: "2020-01-01T00:30:00".into(),
                    end_position: 148_207,
                    seconds: 1800,
                    ..Session::default()
                },
            ],
            mark: "260807:101543".into(),
            mark_offset: None,
            ..Store::default()
        };

        let healed = store.heal_from(&live, &dir.join("none"), &dir.join("none"), &mut |_, _| {});
        assert_eq!(healed, 1, "the row the log restates kept its old figures");
        assert_eq!(store.sessions.len(), 2, "a row was given up");
        let fresh = &store.sessions[1];
        assert_eq!(fresh.started_at, "2026-08-07T10:15:01");
        assert_eq!(fresh.seconds, 41, "the counter's own span");
        let older = &store.sessions[0];
        assert_eq!(older.started_at, "2020-01-01T00:00:00");
        assert_eq!(older.seconds, 1800, "a row older than the logs was touched");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_heal_leaves_the_floor_and_the_era_under_it_alone() {
        let dir = scratch("heal-floor");
        let live = dir.join("messages");
        std::fs::write(
            &live,
            format!(
                "{}\n{}\n",
                page("260807:101501", 7_390_020),
                page("260807:101543", 7_431_463)
            ),
        )
        .expect("a log to read");

        let mut store = Store {
            mark: "260807:101543".into(),
            mark_offset: None,
            floor: "260807:120000".into(),
            ..Store::default()
        };
        let healed = store.heal_from(&live, &dir.join("none"), &dir.join("none"), &mut |_, _| {});

        assert_eq!(healed, 0);
        assert!(
            store.sessions.is_empty(),
            "the era under the floor came back"
        );
        assert_eq!(store.floor, "260807:120000", "the floor was lifted");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_rebuild_states_sittings_and_leaves_every_book_to_the_catalog() {
        let dir = scratch("rebuild-books");
        let live = dir.join("messages");
        std::fs::write(
            &live,
            format!(
                "{}\n{}\n",
                page("260807:101501", 7_390_020),
                page("260807:101543", 7_431_463)
            ),
        )
        .expect("a log to read");
        let shelf = [shelved(938_018, "B00OKPCRLG", "Bible", 55.0)];

        let mut store = Store::default();
        store.remember(&shelf);
        store.mark = "260807:101543".into();
        store.wipe();
        assert!(store.books.is_empty(), "a wipe left a book record standing");

        let added = store.rebuild_from(&live, &dir.join("none"), &dir.join("none"), &mut |_, _| {});
        assert_eq!(added, 1, "the log's sitting did not come back");
        assert!(
            store.books.is_empty(),
            "a parse named a book — only the catalog can, and a reset that \
             skips it hands back sittings against nothing"
        );

        // The pass `App::relearn` makes once a reset is over.
        store.remember(&shelf);
        assert_eq!(store.books.len(), 1, "the catalog left the shelf empty");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn merging_one_record_twice_is_merging_it_once() {
        let mut store = two_books();
        let copy = store.clone();
        assert_eq!(store.merge(&copy), 0);
        assert_eq!(store, copy);
    }

    #[test]
    fn two_eras_merge_to_their_union() {
        let whole = two_books();
        let (older, newer): (Vec<Session>, Vec<Session>) = whole
            .sessions
            .iter()
            .cloned()
            .partition(|s| s.started_at.as_str() < "2026-08-09");
        assert_eq!((older.len(), newer.len()), (2, 1), "a split worth making");

        let mut first = Store {
            sessions: older,
            ..whole.clone()
        };
        let second = Store {
            sessions: newer,
            ..whole.clone()
        };
        assert_eq!(first.merge(&second), 1);
        assert_eq!(first.sessions, whole.sessions);
    }

    #[test]
    fn a_merge_leaves_the_floor_and_the_cleared_books_standing() {
        let mut store = two_books();
        store.clear_book(148_207, "B00OKPCRLG");
        store.floor = "260810:120000".into();
        let held = store.cleared.clone();

        // What an archive of the book holds.
        let mut coming_back = Store::default();
        coming_back.sessions.push(session(
            "2026-08-07T10:15:01",
            "2026-08-07T10:55:43",
            148_207,
            2_400,
        ));
        assert_eq!(store.merge(&coming_back), 1, "an archive is not a parse");
        assert_eq!(store.floor, "260810:120000");
        assert_eq!(store.cleared, held);
    }

    #[test]
    fn one_books_rows_carry_its_sittings_and_its_record_alone() {
        let store = two_books();
        let one = store.one_book(148_207, "B00OKPCRLG");
        assert_eq!(one.sessions.len(), 2);
        assert!(one.sessions.iter().all(|s| s.end_position == 148_207));
        assert_eq!(one.books.len(), 1);
        assert_eq!(one.books[0].title, "A Book");
        assert!(one.mark.is_empty(), "a stamp belongs to the whole record");
        assert_eq!(Store::from_text(&one.text()).sessions.len(), 2);
    }

    #[test]
    fn one_book_of_a_record_that_holds_none_is_empty() {
        assert_eq!(two_books().one_book(1, "B00NOTHERE"), Store::default());
    }

    /// A store that gave up book 148207 at `at`, which is what `page` writes.
    fn having_cleared(at: &str) -> Store {
        Store {
            floor: String::new(),
            cleared: vec![Cleared {
                extent: 148_207,
                key: "B00OKPCRLG".into(),
                at: at.into(),
            }],
            ..Store::default()
        }
    }

    #[test]
    fn the_whole_log_re_derives_nothing_of_a_cleared_book() {
        let lines = vec![
            page("260807:101501", 7_390_020),
            page("260807:101543", 7_431_463),
        ];
        // A `c` row stamped past the sitting, as `drop_reading` writes one.
        let mut store = having_cleared("260807:120000");
        let (added, extended) = store.absorb(&lines, "");
        assert_eq!((added, extended), (0, 0));
        assert!(store.sessions.is_empty(), "the log handed it back");
        assert_eq!(store.mark, "260807:101543", "the lines were still read");
    }

    #[test]
    fn reading_after_a_clear_is_folded_as_any_other() {
        let lines = vec![
            page("260807:101501", 7_390_020),
            page("260807:101543", 7_431_463),
        ];
        let mut store = having_cleared("260807:100000");
        let (added, _) = store.absorb(&lines, "");
        assert_eq!(added, 1, "the sitting starts past the stamp");
        assert_eq!(store.sessions.len(), 1);
    }

    #[test]
    fn a_clear_reaches_the_book_through_the_ends_map() {
        let lines = vec![
            page("260807:101501", 7_390_020),
            page("260807:101543", 7_431_463),
        ];
        // The catalog calls the same book 148209, and the `c` row names that.
        let mut store = Store {
            ends: vec![(148_207, 148_209)],
            floor: String::new(),
            cleared: vec![Cleared {
                extent: 148_209,
                key: String::new(),
                at: "260807:120000".into(),
            }],
            ..Store::default()
        };
        store.absorb(&lines, "");
        assert!(
            store.sessions.is_empty(),
            "the sitting was placed by extent"
        );
    }

    #[test]
    fn one_book_cleared_leaves_another_books_sittings_alone() {
        let mut store = having_cleared("260807:120000");
        store.sessions.push(session(
            "2026-08-01T09:00:00",
            "2026-08-01T09:30:00",
            999,
            1800,
        ));
        let lines = vec![
            page("260807:101501", 7_390_020),
            page("260807:101543", 7_431_463),
        ];
        store.absorb(&lines, "260807:000000");
        assert_eq!(store.sessions.len(), 1);
        assert_eq!(store.sessions[0].end_position, 999);
    }

    #[test]
    fn the_newest_clear_of_one_book_is_the_one_kept() {
        let mut store = Store {
            floor: String::new(),
            cleared: vec![
                Cleared {
                    extent: 148_207,
                    key: "B00OKPCRLG".into(),
                    at: "260801:090000".into(),
                },
                Cleared {
                    extent: 148_207,
                    key: "B00OKPCRLG".into(),
                    at: "260807:120000".into(),
                },
            ],
            ..Store::default()
        };
        store.sort();
        assert_eq!(store.cleared.len(), 1);
        assert_eq!(store.cleared[0].at, "260807:120000");
    }

    #[test]
    fn two_sittings_sharing_a_start_second_are_both_held() {
        let mut store = Store {
            sessions: vec![
                session("2026-08-07T21:45:36", "2026-08-07T21:45:37", 304_517, 1),
                session("2026-08-07T21:45:36", "2026-08-07T22:12:47", 304_517, 1_631),
            ],
            ..Store::default()
        };
        store.sort();
        assert_eq!(store.sessions.len(), 2);
        assert_eq!(store.sessions.iter().map(|s| s.seconds).sum::<i64>(), 1_632);

        let again = store.sessions.clone();
        store.sessions.extend(again);
        store.sort();
        assert_eq!(store.sessions.len(), 2);
    }

    #[test]
    fn a_sitting_is_keyed_by_the_number_the_catalog_uses() {
        let store = Store {
            ends: vec![(938_016, 938_018)],
            keys: Vec::new(),
            counters: Vec::new(),
            pairs: Vec::new(),
            ..Store::default()
        };
        assert_eq!(store.extent_of(938_016), 938_018);
        // A book no line ever mapped keeps its own key.
        assert_eq!(store.extent_of(148_207), 148_207);
    }

    #[test]
    fn a_stored_mapping_gives_way_to_what_the_log_states_again() {
        let mut store = Store {
            // 148207 against another book's end.
            ends: vec![(148_207, 938_018)],
            ..Store::default()
        };
        let lines = [
            "260807:100000 java[1]: I ReadingTimerController:Information::OpenBook,StoredBookData:null;".to_string(),
            "260807:100001 java[1]: I ReadingTimerController:Information::BookEndPosition.FromBook:YJPosition: AZI/AAAAAAAA:148213,CurrentPos:YJPosition: AWUDAAAAAAAA:2,EndPos:YJPosition: AbcVAAAPAAAA:148207,PosLeft:6;".to_string(),
        ];
        store.absorb(&lines, "");
        assert_eq!(store.extent_of(148_207), 148_213);

        // `merge` takes it too.

        let mut held = Store {
            ends: vec![(148_207, 938_018)],
            ..Store::default()
        };
        held.merge(&store);
        assert_eq!(held.extent_of(148_207), 148_213);
    }

    #[test]
    fn a_mobi8_sitting_reaches_the_book_the_catalog_names() {
        let mut store = Store::default();
        let lines = [
            "260906:192401 java[1]: I ReadingTimerController:Information::OpenBook,StoredBookData:TimeRead:329 sec. WPM:0. Version:0,Title:<private>;".to_string(),
            "260906:192402 java[1]: I ReadingTimerController:Information::BookEndPosition.FromBook:HTMLPosition:19886521,CurrentPos:HTMLPosition:7731097,EndPos:HTMLPosition:19886489,PosLeft:12155392;".to_string(),
            "260906:192404 cvm[6144]: I ReadingTimerController:Information::NextPage,Verdict:Processed,PageStartPos:HTMLPosition:7731097,IntervalTime:785,IntervalWords:12,TotalTime:329785,TotalWords:1905,CurrentPos:HTMLPosition:7731097,EndPos:HTMLPosition:19886489,PosLeft:12155392,%Left:0.6112;".to_string(),
            "260906:192425 cvm[6144]: I ReadingTimerController:Information::NextPage,Verdict:Processed,PageStartPos:HTMLPosition:7731725,IntervalTime:21217,IntervalWords:172,TotalTime:351002,TotalWords:2077,CurrentPos:HTMLPosition:7731725,EndPos:HTMLPosition:19886489,PosLeft:12154764,%Left:0.6111;".to_string(),
        ];
        assert_eq!(store.absorb(&lines, ""), (1, 0));

        // `p_contentSize` 19886522 against `FromBook` 19886521.
        store.remember(&[shelved(19_886_522, "*8F3C", "A Sideload", -1.0)]);
        let extent = store.extent_of(store.sessions[0].end_position);
        assert_eq!(extent, 19_886_522);
        assert_eq!(
            store.book_for(extent, None).map(|b| b.title.as_str()),
            Some("A Sideload")
        );
    }

    #[test]
    fn a_sitting_reaches_its_book_at_an_extent_the_record_no_longer_carries() {
        let mut store = Store {
            books: vec![BookRecord {
                // The copy the catalog names today.
                extent: 148_199,
                cde_key: "BTIFIHP3JYKFKDNZLSD7CODVHTDUEOIM".into(),
                title: "A Volume".into(),
                ..BookRecord::default()
            }],
            ends: vec![(148_207, 148_213)],
            ..Store::default()
        };
        assert!(store.book_for(148_213, None).is_none());

        // `remember` at 148213, under the key the record carries.
        store.remember(&[shelved(
            148_213,
            "BTIFIHP3JYKFKDNZLSD7CODVHTDUEOIM",
            "A Volume",
            -1.0,
        )]);
        // `remember` again at 148199, which the record takes.
        store.remember(&[shelved(
            148_199,
            "BTIFIHP3JYKFKDNZLSD7CODVHTDUEOIM",
            "A Volume",
            -1.0,
        )]);
        assert_eq!(store.books[0].extent, 148_199, "the record follows");

        assert_eq!(
            store.book_for(148_213, None).map(|b| &b.title[..]),
            Some("A Volume")
        );
        // Written down and read back.
        let read = Store::from_text(&store.text());
        assert_eq!(read.keys, store.keys);
        assert_eq!(
            read.book_for(148_213, None).map(|b| &b.title[..]),
            Some("A Volume")
        );
    }

    #[test]
    fn an_extent_two_books_have_carried_names_neither() {
        let mut store = Store {
            books: vec![
                BookRecord {
                    extent: 1,
                    cde_key: "ONE".into(),
                    ..BookRecord::default()
                },
                BookRecord {
                    extent: 2,
                    cde_key: "TWO".into(),
                    ..BookRecord::default()
                },
            ],
            ..Store::default()
        };
        store.learn_key(999, "ONE");
        assert!(store.book_for(999, None).is_some());
        store.learn_key(999, "TWO");
        assert!(store.book_for(999, None).is_none());
    }

    // ---- naming a class no record reaches -----------------------------

    /// A store of three days' reading: one book the catalog names and two
    /// classes it does not.
    fn orphaned() -> Store {
        Store {
            sessions: vec![
                session("2026-08-07T10:00:00", "2026-08-07T11:00:00", 148_207, 3_600),
                session("2026-08-08T10:00:00", "2026-08-08T11:00:00", 500_100, 3_600),
                session("2026-08-09T10:00:00", "2026-08-09T11:00:00", 700_200, 3_600),
            ],
            books: vec![BookRecord {
                extent: 148_207,
                cde_key: "B00OKPCRLG".into(),
                title: "A Named Book".into(),
                ..BookRecord::default()
            }],
            ..Store::default()
        }
    }

    fn witness(at: &str, title: &str, key: &str) -> crate::identify::Witness {
        crate::identify::Witness {
            at: at.into(),
            title: title.into(),
            author: "An Author".into(),
            key: key.into(),
            pos: -1,
        }
    }

    /// [`Store::name_from`] with no class held back.
    fn name_from(store: &mut Store, said: &[crate::identify::Witness], by: Named) -> usize {
        store.name_from(said, by, &mut Vec::new())
    }

    #[test]
    fn a_witness_bracketed_by_a_sitting_names_that_sittings_class() {
        let mut store = orphaned();
        // `session` sets `asin` on every sitting; an empty one leaves
        // `book_for` to reach the record through `end_position` alone.
        for s in &mut store.sessions {
            s.asin = None;
        }
        let named = name_from(
            &mut store,
            &[witness("2026-08-08T10:30:00", "The Orphan", "B00ORPHANED")],
            Named::Vocab,
        );
        assert_eq!(named, 1);
        let found = store.book_for(500_100, None).expect("the class was named");
        assert_eq!(found.title, "The Orphan");
        assert_eq!(found.cde_key, "B00ORPHANED", "the source stated a key");
        assert_eq!(found.named_by, Named::Vocab);
        assert!(
            store.book_for(700_200, None).is_none(),
            "an unclaimed class"
        );
    }

    #[test]
    fn a_class_whose_book_the_record_already_holds_is_linked_and_not_copied() {
        // One book under two `EndPos` classes, its record under the older:
        // the device logs a new class when the file changes under the book.
        let mut store = orphaned();
        for s in &mut store.sessions {
            s.asin = None;
        }
        let before = store.books.len();
        let named = name_from(
            &mut store,
            &[witness("2026-08-08T10:30:00", "a named   BOOK", "")],
            Named::Clippings,
        );
        assert_eq!(named, 1);
        assert_eq!(store.books.len(), before, "a second record for one book");
        let found = store.book_for(500_100, None).expect("the class was named");
        assert_eq!(found.cde_key, "B00OKPCRLG");
        assert_eq!(found.named_by, Named::Catalog, "the catalog still holds it");
        // Through the `k` row `slot_for`'s third arm consults.
        assert_eq!(store.key_at(500_100), Some("B00OKPCRLG"));
    }

    #[test]
    fn a_class_two_titles_claim_is_left_alone_by_every_later_source() {
        let mut store = orphaned();
        for s in &mut store.sessions {
            s.asin = None;
        }
        let mut contested = Vec::new();
        let named = store.name_from(
            &[
                witness("2026-08-08T10:10:00", "One Book", ""),
                witness("2026-08-08T10:20:00", "Another Book", ""),
            ],
            Named::Vocab,
            &mut contested,
        );
        assert_eq!(named, 0, "a class holding two books was named");
        assert_eq!(contested, vec![500_100]);
        // A weaker source seeing only one of the two does not settle it.
        let named = store.name_from(
            &[witness("2026-08-08T10:30:00", "One Book", "")],
            Named::Clippings,
            &mut contested,
        );
        assert_eq!(named, 0, "a contested class was named by a weaker source");
        assert!(store.book_for(500_100, None).is_none());
    }

    #[test]
    fn a_witness_two_classes_bracket_names_neither() {
        let mut store = orphaned();
        for s in &mut store.sessions {
            s.asin = None;
        }
        // A second run of another class over the same hour.
        store.sessions.push(session(
            "2026-08-08T10:00:00",
            "2026-08-08T11:00:00",
            700_200,
            3_600,
        ));
        store.sessions.last_mut().unwrap().asin = None;
        store.sort();
        let named = name_from(
            &mut store,
            &[witness("2026-08-08T10:30:00", "The Orphan", "")],
            Named::Vocab,
        );
        assert_eq!(named, 0);
        assert!(store.book_for(500_100, None).is_none());
        assert!(store.book_for(700_200, None).is_none());
    }

    #[test]
    fn a_run_too_short_to_count_as_a_sitting_carries_no_claim() {
        let mut store = orphaned();
        for s in &mut store.sessions {
            s.asin = None;
        }
        let floor = crate::stats::SITTING_FLOOR_SECS;
        store.sessions[1].ended_at = "2026-08-08T10:00:59".into();
        store.sessions[1].seconds = floor - 1;
        assert_eq!(
            name_from(
                &mut store,
                &[witness("2026-08-08T10:00:30", "The Orphan", "")],
                Named::Vocab,
            ),
            0,
        );
        store.sessions[1].seconds = floor;
        assert_eq!(
            name_from(
                &mut store,
                &[witness("2026-08-08T10:00:30", "The Orphan", "")],
                Named::Vocab,
            ),
            1,
        );
    }

    #[test]
    fn a_lookup_past_the_classs_own_end_names_nothing() {
        let mut store = orphaned();
        for s in &mut store.sessions {
            s.asin = None;
        }
        let mut past = witness("2026-08-08T10:30:00", "The Orphan", "");
        past.pos = 500_101;
        assert_eq!(name_from(&mut store, &[past], Named::Vocab), 0);
        let mut inside = witness("2026-08-08T10:30:00", "The Orphan", "");
        inside.pos = 400_000;
        assert_eq!(name_from(&mut store, &[inside], Named::Vocab), 1);
    }

    #[test]
    fn a_book_that_arrived_as_a_file_name_gets_its_real_title() {
        // What an existing record looks like: the sidecar arm named the class
        // from the `.sdr` directory and nothing better had spoken.
        let mut store = orphaned();
        for s in &mut store.sessions {
            s.asin = None;
        }
        store.learn_pair(500_100, "Some_Book_File");
        store.books.push(from_sidecar(500_100, "Some_Book_File"));
        store.sort_books();
        assert_eq!(
            store.book_for(500_100, None).map(|b| b.title.as_str()),
            Some("Some_Book_File")
        );

        let named = name_from(
            &mut store,
            &[witness(
                "2026-08-08T10:30:00",
                "The Real Title",
                "B00REALKEY",
            )],
            Named::Vocab,
        );
        assert_eq!(named, 1);
        let found = store
            .book_for(500_100, None)
            .expect("the class is still named");
        assert_eq!(found.title, "The Real Title");
        assert_eq!(found.named_by, Named::Vocab);
        assert_eq!(
            store.books.iter().filter(|b| b.extent == 500_100).count(),
            1,
            "the file-name record was left standing beside the real one",
        );
        assert!(store.pairs.is_empty(), "the pairing still reaches it");
        // `classes_wanting` leaves the class out for anything weaker.
        assert!(!store.classes_wanting(Named::Clippings).contains(&500_100));
        assert!(store.classes_wanting(Named::Vocab).contains(&700_200));
    }

    #[test]
    fn a_record_states_what_named_it_and_a_row_written_before_that_reads_back() {
        let dir = scratch("named-by");
        let mut store = orphaned();
        store.learn_pair(500_100, "Some_Book_File");
        store.books.push(from_sidecar(500_100, "Some_Book_File"));
        store.sort_books();
        store.save(&dir).expect("a written store");
        let read = Store::load(&dir);
        assert_eq!(read.books, store.books, "a `b` row lost what named it");

        // The same file with every `b` row one column short, its last field
        // gone. The pairing is what picks the stem out.
        let older: String = std::fs::read_to_string(Store::file(&dir))
            .expect("a store to shorten")
            .lines()
            .map(|line| match line.starts_with("b\t") {
                true => line.rsplit_once('\t').expect("a `b` row's last field").0,
                false => line,
            })
            .collect::<Vec<_>>()
            .join("\n");
        let read = Store::from_text(&older);
        let named: Vec<Named> = read.books.iter().map(|b| b.named_by).collect();
        assert_eq!(named, [Named::Catalog, Named::Sidecar]);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_source_is_asked_only_while_a_class_still_wants_what_it_says() {
        let mut store = orphaned();
        for s in &mut store.sessions {
            s.asin = None;
        }
        assert_eq!(store.classes_wanting(Named::Vocab), [500_100, 700_200]);
        assert_eq!(store.classes_wanting(Named::Sidecar), [500_100, 700_200]);
        name_from(
            &mut store,
            &[
                witness("2026-08-08T10:30:00", "One", ""),
                witness("2026-08-09T10:30:00", "Two", ""),
            ],
            Named::Vocab,
        );
        // Nothing weaker has anything left to say, and the walk never happens.
        assert!(store.classes_wanting(Named::Clippings).is_empty());
        assert!(store.classes_wanting(Named::Sidecar).is_empty());
        // `Named::Catalog` outranks them all.
        assert_eq!(store.classes_wanting(Named::Catalog).len(), 2);
    }
}
