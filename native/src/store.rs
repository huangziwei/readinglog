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

/// Where the store lives on the device.
pub const STORE_DIR: &str = "/mnt/us/extensions/readinglog";

/// The file inside it, holding the sittings, the book records and the mark.
const STORE_FILE: &str = "sessions.tsv";

/// What the first line reads. The number names the parse below it.
const HEADER: &str = "#readinglog\t2";

/// The percentage `BookRecord::stand_at` sets [`BookRecord::finished`] at.
pub const FINISHED_PERCENT: f64 = 99.5;

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
}

impl BookRecord {
    /// False on a `*`-prefixed `cde_key`, which names a file and not a book.
    pub fn is_book(&self) -> bool {
        !self.cde_key.starts_with('*')
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
/// it holds back: [`Store::load`] keeps these rows where it clears `sessions`.
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
    /// The newest log line any pass has read, as `YYMMDD:HHMMSS`.
    pub mark: String,
    /// Where the record was last emptied, as `YYMMDD:HHMMSS`. No ordinary pass
    /// reads under it, and unlike [`Self::mark`] a stale `HEADER` leaves it
    /// standing. Empty until the first reset asks for one.
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
}

impl Store {
    /// The store's file under `dir`.
    pub fn file(dir: &Path) -> PathBuf {
        dir.join(STORE_FILE)
    }

    /// Read the store, or an empty one where there is none to read. A file
    /// that will not parse reads as empty; one stamped with an older `HEADER`
    /// keeps `books`, `ends` and `cleared` and gives up `sessions` and `mark`.
    pub fn load(dir: &Path) -> Self {
        let Ok(text) = std::fs::read_to_string(Self::file(dir)) else {
            return Self::default();
        };
        Self::from_text(&text)
    }

    /// Read the record, keeping a copy of one this build will not read whole.
    ///
    /// [`Self::load`] gives up `sessions` and `mark` under a stamp it does not
    /// know, on the contract that the next pass measures them again. The
    /// record outgrew that contract: it holds more days than the device keeps
    /// log for, and under a floor the pass may not look at all. So the file
    /// goes into an archive first, whole and unaltered.
    ///
    /// Restoring that archive takes back its books and its ends, never its
    /// sittings: those were measured by the parser this build supersedes, and
    /// `from_text` refuses them there for the same reason it refuses them
    /// here. What the archive is, is the file itself, kept where the reader
    /// can copy it off.
    pub fn open(dir: &Path) -> Self {
        let Ok(text) = std::fs::read_to_string(Self::file(dir)) else {
            return Self::default();
        };
        if text.lines().next() != Some(HEADER) && !text.trim().is_empty() {
            match crate::backup::keep_text(dir, &text, &stamp_in(&text)) {
                Ok(at) => eprintln!(
                    "store: the record this build supersedes is at {}",
                    at.display()
                ),
                Err(err) => eprintln!("store: the superseded record was not kept — {err}"),
            }
        }
        Self::from_text(&text)
    }

    /// [`Self::load`] over text already in hand, which is how an archive's own
    /// copy of the record is read.
    pub fn from_text(text: &str) -> Self {
        let mut out = Self::default();
        let mut stamped = false;
        for line in text.lines() {
            let mut f = line.split('\t');
            match f.next() {
                Some("#readinglog") => stamped = line == HEADER,
                Some("m") => out.mark = f.next().unwrap_or_default().to_string(),
                Some("f") => out.floor = f.next().unwrap_or_default().to_string(),
                Some("e") => {
                    if let (Some(Ok(k)), Some(Ok(v))) = (
                        f.next().map(str::parse::<i64>),
                        f.next().map(str::parse::<i64>),
                    ) {
                        out.ends.push((k, v));
                    }
                }
                Some("s") => out.sessions.extend(read_session(&mut f)),
                Some("b") => out.books.extend(read_book(&mut f)),
                Some("c") => out.cleared.extend(read_cleared(&mut f)),
                _ => {}
            }
        }
        if !stamped {
            out.sessions.clear();
            out.mark.clear();
        }
        out.sort();
        out
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
            out.push_str(&format!("m\t{}\n", self.mark));
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

    /// The instant a pass must start reading the log at: [`Self::mark`], except
    /// under `Self::open_at_mark`, where it is the newest sitting's own start
    /// and that sitting is re-measured whole. Never under [`Self::floor`].
    pub fn read_from(&self) -> String {
        self.starts_at().max(self.floor.clone())
    }

    /// [`Self::read_from`] before the floor is applied.
    fn starts_at(&self) -> String {
        let Some(newest) = self.sessions.last() else {
            return self.mark.clone();
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

    /// Fold a batch of log lines into the store. Every sitting at or after
    /// `from` is dropped and replaced by what `lines` measured; a sitting in
    /// progress is re-measured whole on each pass. A sitting under the floor,
    /// or under the stamp of a `c` row, is measured and then let go.
    pub fn absorb(&mut self, lines: &[String], from: &str) -> (usize, usize) {
        let refs: Vec<&str> = lines.iter().map(String::as_str).collect();
        let parsed = crate::log::parse_sessions(refs.iter().copied());
        let cut = match from.is_empty() {
            true => String::new(),
            false => iso_stamp(from),
        };
        // Ahead of the sittings: `Self::barred` places one through `ends`.
        for (k, v) in crate::log::line::frombook_map(refs.iter().copied()) {
            if !self.ends.iter().any(|(key, _)| *key == k) {
                self.ends.push((k, v));
            }
        }
        self.sort_ends();
        let parsed: Vec<Session> = parsed.into_iter().filter(|s| !self.barred(s)).collect();

        let before = self.sessions.len();
        // Everything from `cut` on is `parsed`'s to state. An empty `cut`
        // covers the whole log.
        self.sessions
            .retain(|s| !cut.is_empty() && s.started_at < cut);
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

    /// Whether this record refuses `session`: it starts under the floor, or
    /// under the stamp of the `c` row naming its book.
    ///
    /// The floor is checked here and not only against the log's watermark, so
    /// the guarantee holds however the lines reached this pass.
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

    /// Read the device's log and fold it in, reporting files opened and
    /// files to open.
    pub fn update(&mut self, on: &mut dyn FnMut(usize, usize)) -> Pass {
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
        }
    }

    /// Read the device's whole log into a record of its own and fold it in,
    /// answering how many sittings this record did not already hold.
    ///
    /// The parse goes into a store of its own and comes back through
    /// [`Self::merge`], so nothing already held can be lost: run against this
    /// record, [`Self::absorb`] with no watermark would drop every sitting and
    /// let the log restate the lot — and the log is younger than the record.
    ///
    /// The floor comes off, the reader having just asked for what stood under
    /// it. The `c` rows do not: a book taken off the record was taken off on
    /// purpose, one book at a time.
    pub fn rebuild(&mut self, on: &mut dyn FnMut(usize, usize)) -> usize {
        self.rebuild_from(
            Path::new(source::LIVE_LOG),
            Path::new(source::LOG_DIR),
            Path::new(source::DUMP_DIR),
            on,
        )
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
            cleared: self.cleared.clone(),
            ..Store::default()
        };
        whole.absorb(&got.lines, "");
        let added = self.merge(&whole);
        self.mark = self.mark.clone().max(whole.mark);
        self.floor.clear();
        added
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
        }
        self.note_progress(&stated);
        self.sort_books();
        self.books.iter().filter(|r| !before.contains(r)).count()
    }

    /// Give each record outside `stated` the percentage its newest sitting
    /// states. `p_percentFinished` sits on the row a deletion drops; `%Left`
    /// states the same figure, and `sessions` ascends by `started_at`.
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
            if !stated.contains(&slot) {
                self.books[slot].stand_at((progress * 100.0).clamp(0.0, 100.0));
            }
        }
    }

    /// The slots in [`Self::books`] a sitting is credited to.
    fn read_slots(&self) -> std::collections::HashSet<usize> {
        self.sessions
            .iter()
            .filter_map(|s| self.slot_for(self.extent_of(s.end_position), s.asin.as_deref()))
            .filter(|&slot| self.books[slot].is_book())
            .collect()
    }

    /// Copy the `thumbnail` of each slot `Self::read_slots` answers into
    /// `dir`, point its `cover` at the copy, and delete every other file there.
    /// Answers how many records changed.
    pub fn keep_covers(&mut self, dir: &Path) -> usize {
        let read = self.read_slots();
        let mut kept = 0;
        for (slot, record) in self.books.iter_mut().enumerate() {
            if !read.contains(&slot) {
                kept += usize::from(!std::mem::take(&mut record.cover).is_empty());
                continue;
            }
            let at = covers::path(dir, &record.cde_key);
            let at = at.to_string_lossy();
            if !covers::held(dir, &record.cde_key) {
                if record.thumbnail.is_empty() {
                    continue;
                }
                if let Err(err) = covers::keep(dir, &record.cde_key, Path::new(&record.thumbnail)) {
                    eprintln!("covers: {} — {err}", record.title);
                    continue;
                }
            }
            if record.cover != at {
                record.cover = at.into_owned();
                kept += 1;
            }
        }
        // An empty `sessions` leaves every file under `dir` standing.
        if self.sessions.is_empty() {
            return kept;
        }
        let held: Vec<&str> = read
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
    /// A cloud row states no extent, and the record it belongs to may carry one
    /// from a pass that ran while the book was on the device.
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
        let key = key.filter(|k| !k.is_empty())?;
        self.books.iter().position(|b| b.cde_key == key)
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

    /// Empty the record: the sittings, the books and the ends all go, and
    /// [`Self::floor`] takes the mark so no later pass reads under it.
    ///
    /// [`Self::mark`] stands, and so do the `c` rows — every one of them is at
    /// or under the new floor, and they are what keeps a book taken off the
    /// record from returning when the log is read whole again. Answers whether
    /// anything was there to wipe.
    ///
    /// A record with no mark has read nothing, and has no instant to floor.
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

    /// Fold another record's sittings, ends and books into this one, answering
    /// how many sittings it did not already hold.
    ///
    /// [`Self::sort`] de-duplicates, and its sorts are stable, so folding the
    /// same record in twice is folding it in once, and where both hold a book
    /// this record's own row stands. Neither [`Self::floor`] nor the `c` rows
    /// move: they hold back the parser, and a record is not a parse.
    pub fn merge(&mut self, other: &Store) -> usize {
        let before = self.sessions.len();
        self.sessions.extend(other.sessions.iter().cloned());
        self.ends.extend(other.ends.iter().copied());
        self.books.extend(other.books.iter().cloned());
        self.sort();
        self.sessions.len() - before
    }

    /// One book's rows as a record of its own: its own `b` row, the sittings
    /// credited to it, and the `e` rows that place them. Empty where the
    /// record holds no such book.
    ///
    /// What an archive carries before that book is cleared or forgotten.
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
        Store {
            sessions,
            ends,
            books: vec![self.books[slot].clone()],
            // The stamps belong to the record as a whole and never to a book.
            mark: String::new(),
            floor: String::new(),
            cleared: Vec::new(),
        }
    }

    /// Put one book's reading back to zero: its sittings go, and a `c` row
    /// stops a later parse handing them back. The record stands, holding the
    /// title, the cover and the place. Answers how many sittings went.
    pub fn clear_book(&mut self, extent: i64, key: &str) -> usize {
        self.drop_reading(extent, key)
    }

    /// Take one book off the record altogether: its reading goes the way
    /// [`Self::clear_book`] takes it, and its record goes with it. Answers how
    /// many sittings went.
    ///
    /// The catalog writes a fresh record on the next pass for a book still in
    /// the library; for one that has left it, this is the last copy of the
    /// title, the author and the cover on the machine.
    pub fn forget_book(&mut self, extent: i64, key: &str) -> usize {
        let went = self.drop_reading(extent, key);
        if let Some(slot) = self.slot_for(extent, Some(key)) {
            self.books.remove(slot);
        }
        went
    }

    /// The sittings of one book, dropped and held back. Answers how many went.
    fn drop_reading(&mut self, extent: i64, key: &str) -> usize {
        let Some(slot) = self.slot_for(extent, Some(key)) else {
            return 0;
        };
        let kept: Vec<Session> = {
            // A sitting is this book's where it lands on this book's record,
            // which is the placement `Stats::build` draws it under.
            let mine = |s: &Session| {
                self.slot_for(self.extent_of(s.end_position), s.asin.as_deref()) == Some(slot)
            };
            self.sessions.iter().filter(|s| !mine(s)).cloned().collect()
        };
        let went = self.sessions.len() - kept.len();
        self.sessions = kept;
        // An empty mark means no pass has read anything, so there is no
        // instant to hold a later parse below.
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
        self.sort_books();
        self.sort_cleared();
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

/// A field with nothing in it that could be read as a separator.
fn flat(text: &str) -> String {
    text.replace(['\t', '\n', '\r'], " ")
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
}

fn write_book(b: &BookRecord) -> String {
    format!(
        "b\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}",
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
    })
    // `read_through` marks a record the row left unmarked.
    .map(|mut record: BookRecord| {
        record.finished |= record.read_through();
        record
    })
}

/// What to name an archive of `text` after: the `m` row it states, else the
/// newest sitting it holds, else the local clock. Only the ordering of the
/// name depends on it.
fn stamp_in(text: &str) -> String {
    let row = |tag: &str| {
        text.lines()
            .filter_map(|l| l.strip_prefix(tag))
            .filter_map(|rest| rest.split('\t').next())
            .map(str::to_string)
            .max()
    };
    if let Some(mark) = row("m\t").filter(|m| !m.is_empty()) {
        return mark;
    }
    if let Some(newest) = row("s\t").and_then(|at| log_stamp(&at)) {
        return newest;
    }
    let (day, secs) = crate::date::now();
    let (y, m, d) = crate::date::civil_from_days(day);
    format!(
        "{:02}{:02}{:02}:{:02}{:02}{:02}",
        y % 100,
        m,
        d,
        secs / 3600,
        secs / 60 % 60,
        secs % 60
    )
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
    format!(
        "s\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}",
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
        }
    }

    fn scratch(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("readinglog-store-{name}"));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("a scratch directory");
        dir
    }

    #[test]
    fn a_cover_is_kept_only_for_a_book_the_record_has_reading_against() {
        let dir = scratch("covers");
        let art = dir.join("thumbnail.jpg");
        std::fs::write(&art, b"jpegbytes").expect("a written thumbnail");
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
            books: vec![named(148_207, "B00OKPCRLG"), named(938_016, "B00NEVERRD")],
            ..Store::default()
        };
        // A cover for the book with no reading, and a `.partial` beside it.
        covers::keep(&dir, "B00NEVERRD", &art).expect("a copied cover");
        store.books[1].cover = covers::path(&dir, "B00NEVERRD")
            .to_string_lossy()
            .into_owned();
        std::fs::write(dir.join(covers::COVERS_DIR).join("B01.partial"), b"x").unwrap();

        assert_eq!(store.keep_covers(&dir), 2, "one taken, one given up");
        assert!(covers::held(&dir, "B00OKPCRLG"), "the book that was read");
        assert!(!covers::held(&dir, "B00NEVERRD"), "the book that was not");
        assert!(store.books[1].cover.is_empty(), "a cover no file backs");
        let left = std::fs::read_dir(dir.join(covers::COVERS_DIR))
            .expect("the covers directory")
            .count();
        assert_eq!(left, 1, "one book carries reading");

        // `keep_covers` over the same store takes nothing and drops nothing.
        assert_eq!(store.keep_covers(&dir), 0);
        assert!(covers::held(&dir, "B00OKPCRLG"));

        // An empty `sessions` holds every file under `dir`.
        store.sessions.clear();
        store.keep_covers(&dir);
        assert!(covers::held(&dir, "B00OKPCRLG"), "the cache is not emptied");
        let _ = std::fs::remove_dir_all(&dir);
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
            books: Vec::new(),
            mark: "260808:213000".into(),
            floor: String::new(),
            cleared: vec![Cleared {
                extent: 304_517,
                key: "B00OKPCRLG".into(),
                at: "260808:120000".into(),
            }],
        };
        store.sessions[1].asin = None;
        store.sessions[1].progress = None;
        store.sessions[1].measure = Measure::Dwell;
        store.save(&dir).expect("a written store");

        assert_eq!(Store::load(&dir), store);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_store_from_an_older_parse_gives_up_its_sittings_and_keeps_its_books() {
        let dir = scratch("stamped");
        let store = Store {
            sessions: vec![session(
                "2026-08-07T10:15:01",
                "2026-08-07T10:55:43",
                148_207,
                2_400,
            )],
            ends: vec![(938_016, 938_018)],
            books: vec![BookRecord {
                extent: 148_207,
                title: "A Book".into(),
                ..BookRecord::default()
            }],
            mark: "260808:213000".into(),
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
        assert!(read.sessions.is_empty(), "an older parse's sittings held");
        assert!(read.mark.is_empty(), "the pass would not read them again");
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
    fn a_superseded_record_is_kept_before_it_is_given_up() {
        let dir = scratch("superseded");
        let store = two_books();
        superseded(&dir, &store);

        let read = Store::open(&dir);
        assert!(read.sessions.is_empty(), "the sittings were still given up");

        let held = crate::backup::list(&dir);
        assert_eq!(held.len(), 1, "the record was not kept");
        assert_eq!(held[0].stamp, "260810-120000", "named for the mark");

        // The file is in there whole, which is what a reader copying it off
        // over USB has.
        let mut open =
            crate::update::archive::Archive::open(&held[0].path).expect("a readable archive");
        let entry = open.entries()[0].clone();
        let bytes = open.read(&entry).expect("the record inside");
        let text = String::from_utf8_lossy(&bytes);
        assert_eq!(text.lines().filter(|l| l.starts_with("s\t")).count(), 3);

        // What this build will take back from it is what its own parser never
        // measured: the names, and not the sittings.
        let inside = crate::backup::peek(&held[0].path).expect("a readable archive");
        assert!(inside.sessions.is_empty(), "another parser's measurements");
        assert_eq!(inside.books.len(), 2, "the names were not the parser's");
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
            is_book: !key.starts_with('*'),
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

    /// A record read on the device to 88%, then to `progress`, then left with
    /// the row `p_percentFinished` sits on deleted.
    fn read_on_to(progress: Option<f64>) -> Store {
        let mut store = Store {
            sessions: vec![session(
                "2026-08-27T10:34:40",
                "2026-08-27T11:03:20",
                938_018,
                1_720,
            )],
            ends: Vec::new(),
            books: Vec::new(),
            mark: String::new(),
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
        // `%Left` of 0 on the last turn.
        assert_eq!(read_on_to(Some(1.0)).books[0].percent, 100.0);
        // `%Left` of 0.6 on the last turn: a book put down at 40% reads 40%.
        assert_eq!(read_on_to(Some(0.4)).books[0].percent, 40.0);
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
        // The same book off the device: no extent, no percentage, a title.
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
            books: Vec::new(),
            mark: mark.into(),
            floor: String::new(),
            cleared: Vec::new(),
        }
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
    fn the_pass_starts_at_the_mark_with_no_sitting_and_at_nothing_with_neither() {
        let bare = Store {
            mark: "260808:213000".into(),
            ..Store::default()
        };
        assert_eq!(bare.read_from(), "260808:213000");
        assert_eq!(Store::default().read_from(), "");
        // A first run over a store that never recorded a mark reads everything.
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
        assert_eq!(held.title, "A Book");
        assert!(held.finished, "the place is not the reading");
        assert_eq!(held.restart, Some(62.0));
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
        // The log still holds every line the book was measured from.
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
    fn the_floor_outlives_the_mark_a_stale_header_clears() {
        let dir = scratch("floored");
        let mut store = two_books();
        store.wipe();
        superseded(&dir, &store);

        let read = Store::load(&dir);
        assert!(read.mark.is_empty(), "the stamp did not clear the mark");
        assert_eq!(read.floor, "260810:120000", "the floor went with it");
        assert_eq!(
            read.read_from(),
            "260810:120000",
            "the pass would read the whole log"
        );
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
                "#readinglog\t2",
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
            // Older than anything the log still holds.
            sessions: vec![session(
                "2026-07-01T09:00:00",
                "2026-07-01T09:30:00",
                999,
                1_800,
            )],
            mark: "260807:101543".into(),
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

        // The archive still holds what the book had.
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
        // Cleared after the sitting ran, which is every clear: the stamp is the
        // mark, and the mark stands past everything already read.
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
            ..Store::default()
        };
        assert_eq!(store.extent_of(938_016), 938_018);
        // A book no line ever mapped keeps its own key.
        assert_eq!(store.extent_of(148_207), 148_207);
    }
}
