//! The app's own record of what was read, at [`STORE_DIR`]. Every pass folds
//! `log::source` and `catalog` into [`STORE_FILE`]. A sitting is written once,
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

/// What `catalog` stated about one book, on the last pass that named it.
#[derive(Debug, Default, Clone, PartialEq)]
pub struct BookRecord {
    /// `p_contentSize`, the number a sitting is keyed by.
    pub extent: i64,
    pub cde_key: String,
    pub title: String,
    pub author: String,
    pub thumbnail: String,
    pub language: String,
    /// The catalog's `p_percentFinished`, 0 through 100. Negative where
    /// unstated.
    pub percent: f64,
    /// Whether `catalog` stated a `p_location` for this book on the last pass.
    pub on_device: bool,
    /// The store's own copy of the cover, under `covers::COVERS_DIR`.
    pub cover: String,
    /// The `p_location` `catalog` last stated, which the reader is handed to
    /// open the book. Empty for a book the catalog has only ever named in the
    /// library.
    pub location: String,
}

impl BookRecord {
    /// False on a `*`-prefixed `cde_key`, which names a file and not a book.
    pub fn is_book(&self) -> bool {
        !self.cde_key.starts_with('*')
    }

    /// [`Self::cover`] where one is held, else [`Self::thumbnail`].
    pub fn art(&self) -> &str {
        match self.cover.is_empty() {
            true => &self.thumbnail,
            false => &self.cover,
        }
    }
}

/// Everything the app knows, loaded whole.
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
    /// that will not parse reads as empty; one stamped with an older [`HEADER`]
    /// keeps `books` and `ends` and gives up `sessions` and `mark`.
    pub fn load(dir: &Path) -> Self {
        let Ok(text) = std::fs::read_to_string(Self::file(dir)) else {
            return Self::default();
        };
        let mut out = Self::default();
        let mut stamped = false;
        for line in text.lines() {
            let mut f = line.split('\t');
            match f.next() {
                Some("#readinglog") => stamped = line == HEADER,
                Some("m") => out.mark = f.next().unwrap_or_default().to_string(),
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
            writeln!(out, "{HEADER}")?;
            if !self.mark.is_empty() {
                writeln!(out, "m\t{}", self.mark)?;
            }
            for (k, v) in &self.ends {
                writeln!(out, "e\t{k}\t{v}")?;
            }
            for b in &self.books {
                writeln!(out, "{}", write_book(b))?;
            }
            for s in &self.sessions {
                writeln!(out, "{}", write_session(s))?;
            }
            out.sync_all()?;
        }
        std::fs::rename(&partial, &target)
    }

    /// The instant a pass must start reading the log at: [`Self::mark`], except
    /// under [`Self::open_at_mark`], where it is the newest sitting's own start
    /// so that sitting is re-measured whole.
    pub fn read_from(&self) -> String {
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
    /// `from` is dropped and replaced by what `lines` measured, so a sitting in
    /// progress is re-measured whole on each pass.
    pub fn absorb(&mut self, lines: &[String], from: &str) -> (usize, usize) {
        let refs: Vec<&str> = lines.iter().map(String::as_str).collect();
        let parsed = crate::log::parse_sessions(refs.iter().copied());
        let cut = match from.is_empty() {
            true => String::new(),
            false => iso_stamp(from),
        };
        let before = self.sessions.len();
        // Everything from `cut` on is `parsed`'s to state. An empty `cut`
        // covers the whole log.
        self.sessions
            .retain(|s| !cut.is_empty() && s.started_at < cut);
        let dropped = before - self.sessions.len();
        let found = parsed.len();
        self.sessions.extend(parsed);

        for (k, v) in crate::log::line::frombook_map(refs.iter().copied()) {
            if !self.ends.iter().any(|(key, _)| *key == k) {
                self.ends.push((k, v));
            }
        }
        if let Some(newest) = refs.iter().filter_map(|l| line_stamp(l)).max()
            && newest > self.mark.as_str()
        {
            self.mark = newest.to_string();
        }
        self.sort();
        (found.saturating_sub(dropped), dropped.min(found))
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
                self.books[slot].percent = (progress * 100.0).clamp(0.0, 100.0);
            }
        }
    }

    /// Copy each book's `thumbnail` into `dir` and point its `cover` at the
    /// copy, answering how many records changed. A record whose copy is on disk
    /// keeps it, `thumbnail` unread.
    pub fn keep_covers(&mut self, dir: &Path) -> usize {
        let mut kept = 0;
        for record in &mut self.books {
            if !record.is_book() {
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
        self.ends.sort_unstable();
        self.ends.dedup_by_key(|(k, _)| *k);
        self.sort_books();
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
}

/// A field with nothing in it that could be read as a separator.
fn flat(text: &str) -> String {
    text.replace(['\t', '\n', '\r'], " ")
}

/// A `BookRecord` holding everything `book` states.
fn taken(book: &Book) -> BookRecord {
    BookRecord {
        extent: book.extent,
        cde_key: flat(&book.cde_key),
        title: flat(&book.title),
        author: flat(&book.author),
        thumbnail: flat(&book.thumbnail),
        language: flat(&book.language),
        percent: book.percent,
        on_device: book.on_device,
        cover: String::new(),
        location: flat(&book.location),
    }
}

/// Take what `book` states over what `record` holds, field by field. A cloud
/// row states no extent and no percentage, and a record carrying either from an
/// earlier pass keeps it.
fn merge(record: &mut BookRecord, book: &Book) {
    for (field, stated) in [
        (&mut record.cde_key, &book.cde_key),
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
    if book.percent >= 0.0 {
        record.percent = book.percent;
    }
    record.on_device |= book.on_device;
}

fn write_book(b: &BookRecord) -> String {
    format!(
        "b\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}",
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
    )
}

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
        // A row written before the location was kept states none, and the next
        // catalog pass fills it.
        location: next().to_string(),
    })
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
        }
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
