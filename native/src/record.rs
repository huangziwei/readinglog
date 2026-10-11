//! The record of what was read, at [`RECORD_DIR`]: [`Record::update`] folds
//! `log::source` and `library` into `RECORD_FILE`, writing each sitting once
//! but the one in progress, re-measured from its own start.

use std::cmp::Reverse;
use std::fmt::Write as _;
use std::io::Write as _;
use std::path::{Path, PathBuf};

use crate::clippings::Kind;
use crate::clock::Clock;
use crate::covers;
use crate::highlights::{Gate, Highlight, State};
use crate::library;
use crate::log::line::{line_stamp, log_stamp};
use crate::log::sitting::{Measure, SITTING_GAP_SECS, Sitting, Stretch};
use crate::log::source;
use crate::sidecar;

/// The directory the record's file sits in.
pub const RECORD_DIR: &str = "/mnt/us/extensions/readinglog";

/// The file inside it, holding the sittings, the books and the watermark.
const RECORD_FILE: &str = "sessions.tsv";

/// What the first line reads, `2` naming the parse below it.
pub(crate) const HEADER: &str = "#readinglog\t2";

/// The progress `Book::stand_at` sets [`Book::finished`] at.
pub const FINISHED_PERCENT: f64 = 99.5;

/// The shortest sitting [`Record::identify_from`] lets a witness identify a book over.
pub const CLAIM_MIN_SECS: i64 = 60;

/// What identified a book, ranked strongest first, in the order
/// [`crate::identify::from_sources`] asks the sources: each variant takes a book a
/// weaker one identified, and none one [`IdentifiedBy::Library`] identified.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum IdentifiedBy {
    /// `cc.db`: the title, the author, the cover, the content key, the progress.
    #[default]
    Library,
    /// `vocab.db`: a title, an author and a content key, per word looked up.
    Vocab,
    /// `My Clippings.txt`: a title and an author, per clipping.
    Clippings,
    /// A `.sdr` directory: the book's own file name and nothing else.
    Sidecar,
}

impl IdentifiedBy {
    /// The letter a `b` row carries.
    fn as_str(self) -> &'static str {
        match self {
            Self::Library => "c",
            Self::Vocab => "v",
            Self::Clippings => "l",
            Self::Sidecar => "s",
        }
    }

    /// What a `b` row's letter names, and `None` for a row carrying none or
    /// any other text.
    fn from_stored(text: &str) -> Option<Self> {
        match text.trim() {
            "c" => Some(Self::Library),
            "v" => Some(Self::Vocab),
            "l" => Some(Self::Clippings),
            "s" => Some(Self::Sidecar),
            _ => None,
        }
    }
}

/// What `library` stated about one book, on the last pass that listed it.
#[derive(Debug, Default, Clone, PartialEq)]
pub struct Book {
    /// `p_contentSize`, the number a sitting is keyed by.
    pub extent: i64,
    pub cde_key: String,
    /// `p_cdeType`, which `mark::set` names the book by alongside `cde_key`.
    pub cde_type: String,
    pub title: String,
    pub author: String,
    pub thumbnail: String,
    pub language: String,
    /// The library's `p_percentFinished`, 0 through 100, negative where unstated.
    pub percent: f64,
    /// Whether `library` stated a `p_location` for this book on the last pass.
    pub on_kindle: bool,
    /// The record's own copy of the cover, under `covers::COVERS_DIR`.
    pub cover: String,
    /// The `p_location` `library` last stated, empty where it stated none.
    pub location: String,
    /// Whether this book is read through, set by [`Record::set_finished`].
    pub finished: bool,
    /// The progress [`Record::restart`] was called at.
    pub restart: Option<f64>,
    /// The day each reading carried through to the end was restarted on.
    pub restarted_on: Vec<i64>,
    /// The library's `p_readState`, negative where it states none.
    pub read_state: i64,
    /// `timer.model`'s `TotalTime` in milliseconds, zero until a sidecar of this book is read.
    pub timer_ms: i64,
    /// `timer.model`'s `TotalWords` beside `timer_ms`.
    pub timer_words: i64,
    /// The share of the book `timer_ms` and `timer_words` were gathered over.
    pub timer_covered: f64,
    /// Whether [`Record::clear_book`] took this book's reading, `Stats::build` listing it at zero.
    pub kept: bool,
    /// What identified this book, free to a stronger [`IdentifiedBy`].
    pub identified_by: IdentifiedBy,
}

impl Book {
    /// Whether this names a book. [`Self::cde_key`] carries a `*` for
    /// a file with no content key, and [`crate::library::is_reading`] answers
    /// for those from [`Self::location`].
    pub fn is_book(&self) -> bool {
        !self.cde_key.starts_with('*') || crate::library::is_reading(&self.location)
    }

    /// Whether the progress this book holds calls it read through.
    fn read_through(&self) -> bool {
        self.percent >= FINISHED_PERCENT
    }

    /// Take `percent` as this book's progress. At or past [`FINISHED_PERCENT`] it
    /// carries [`Self::finished`]; at or past [`Self::restart`] it is the
    /// reading that ended, and `percent` holds 0.
    fn stand_at(&mut self, percent: f64) {
        let percent = stored_percent(percent);
        if let Some(from) = self.restart {
            if percent >= from {
                return;
            }
            self.restart = None;
        }
        self.percent = percent;
        self.finished |= self.read_through();
    }

    /// Take `state` as [`Self::read_state`]: `state` differing from the one held
    /// carries [`Self::finished`], an unchanged one leaves `finished` standing,
    /// and [`Self::read_through`] outranks both.
    fn take_mark(&mut self, state: i64) {
        if state == self.read_state {
            return;
        }
        self.read_state = state;
        let Some(read) = crate::library::read_state_says(state) else {
            return;
        };
        self.finished = read || self.read_through();
        if read {
            self.restart = None;
        }
    }

    /// [`Self::cover`] where one is held, else [`Self::thumbnail`].
    pub fn cover_file(&self) -> &str {
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
    /// The book's `p_contentSize`, 0 where the library never stated one.
    pub extent: i64,
    /// The book's `cde_key`, which reaches a book carrying no extent.
    pub key: String,
    /// The instant it was cleared, as `YYMMDD:HHMMSS`.
    pub at: String,
}

/// The record, loaded whole.
#[derive(Debug, Default, Clone, PartialEq)]
pub struct Record {
    /// Ascending by `started_at`, with `end_position` a sitting's identity.
    pub sittings: Vec<Sitting>,
    /// The offsets `Clock` holds, placing an instant a source states as a true epoch.
    pub clock: Clock,
    /// `EndPos → BookEndPosition.FromBook`, ascending by key.
    pub ends: Vec<(i64, i64)>,
    /// Every book `library` has listed, ascending by `extent` then `cde_key`.
    pub books: Vec<Book>,
    /// `extent → cde_key`, every pairing any pass has seen, ascending.
    pub keys: Vec<(i64, String)>,
    /// `(end position, TotalTime, TotalWords)` per end position, ascending.
    pub counters: Vec<(i64, i64, i64)>,
    /// `extent → the book's file`, from a sidecar of the same counter.
    pub pairs: Vec<(i64, String)>,
    /// The newest log line any pass has read, as `YYMMDD:HHMMSS`.
    pub watermark: String,
    /// Seconds ahead of UTC at [`Self::watermark`], read by [`Self::follow_clock`].
    pub watermark_offset: Option<i64>,
    /// Where the record was last reset, `YYMMDD:HHMMSS`, a floor to every pass.
    pub reset_at: String,
    /// Books put back to zero, ascending by `extent` then `key`.
    pub cleared: Vec<Cleared>,
    /// Every highlight the two sources state, by stamp, written whole by
    /// [`crate::highlights::fold`].
    pub highlights: Vec<Highlight>,
    /// What the pass that wrote [`Self::highlights`] saw, `None` where no pass has.
    pub gate: Option<Gate>,
    /// What the last pass over the identifying sources stood at, `None` where none has.
    pub sources: Option<Sources>,
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
    /// Seconds [`Record::follow_clock`] pulled the watermark back by, else 0.
    pub rewound: i64,
}

impl Record {
    /// The record's file under `dir`.
    pub fn file(dir: &Path) -> PathBuf {
        dir.join(RECORD_FILE)
    }

    /// Read the record: empty where there is none or it will not parse, whole
    /// under any other `HEADER`.
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

    /// [`Self::from_text`], for what `backup::restore` folds in.
    pub fn from_backup(text: &str) -> Self {
        Self::parse(text)
    }

    /// The rows of `text`, one arm per row type.
    fn parse(text: &str) -> Self {
        let mut out = Self::default();
        // `Clock` sorts and collapses `seen`, folded in whole past the loop.
        let mut seen: Vec<(i64, i64)> = Vec::new();
        for line in text.lines() {
            let mut f = line.split('\t');
            match f.next() {
                Some("#readinglog") => {}
                Some("m") => {
                    out.watermark = f.next().unwrap_or_default().to_string();
                    out.watermark_offset = f.next().and_then(|o| o.parse().ok());
                }
                Some("z") => {
                    if let (Some(Ok(epoch)), Some(Ok(offset))) = (
                        f.next().map(str::parse::<i64>),
                        f.next().map(str::parse::<i64>),
                    ) {
                        seen.push((epoch, offset));
                    }
                }
                Some("f") => out.reset_at = f.next().unwrap_or_default().to_string(),
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
                Some("s") => out.sittings.extend(read_sitting(&mut f)),
                Some("b") => out.books.extend(read_book(&mut f)),
                Some("c") => out.cleared.extend(read_cleared(&mut f)),
                Some("a") => out.highlights.extend(read_highlight(&mut f)),
                Some("n") => out.gate = read_gate(&mut f),
                Some("g") => out.sources = read_sources(&mut f),
                _ => {}
            }
        }
        out.clock.observe(seen);
        out.sort();
        out.migrate();
        out
    }

    /// Set [`IdentifiedBy::Sidecar`] on each [`IdentifiedBy::Library`] book whose
    /// `(extent, cde_key)` sits in [`Self::pairs`] — the pairing
    /// [`Self::recover`] writes, and no other record carries.
    fn migrate(&mut self) {
        for i in 0..self.books.len() {
            if self.books[i].identified_by != IdentifiedBy::Library {
                continue;
            }
            let paired = (self.books[i].extent, self.books[i].cde_key.clone());
            if self.pairs.binary_search(&paired).is_ok() {
                self.books[i].identified_by = IdentifiedBy::Sidecar;
            }
        }
    }

    /// Write the record to `dir`, replacing what is there.
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

    /// The record as the file holds it, which is what a backup carries and
    /// what [`Self::load`] reads back.
    pub fn text(&self) -> String {
        // `out` takes every row through `write!`, a `String` for none.
        let mut out = String::new();
        out.push_str(HEADER);
        out.push('\n');
        if !self.watermark.is_empty() {
            match self.watermark_offset {
                Some(offset) => row(&mut out, format_args!("m\t{}\t{offset}", self.watermark)),
                None => row(&mut out, format_args!("m\t{}", self.watermark)),
            }
        }
        for (epoch, offset) in self.clock.rows() {
            row(&mut out, format_args!("z\t{epoch}\t{offset}"));
        }
        if !self.reset_at.is_empty() {
            row(&mut out, format_args!("f\t{}", self.reset_at));
        }
        for c in &self.cleared {
            row(
                &mut out,
                format_args!("c\t{}\t{}\t{}", c.extent, flat(&c.key), c.at),
            );
        }
        for (k, v) in &self.ends {
            row(&mut out, format_args!("e\t{k}\t{v}"));
        }
        for (extent, key) in &self.keys {
            row(&mut out, format_args!("k\t{extent}\t{}", flat(key)));
        }
        for (ep, ms, words) in &self.counters {
            row(&mut out, format_args!("t\t{ep}\t{ms}\t{words}"));
        }
        for (extent, file) in &self.pairs {
            row(&mut out, format_args!("ep\t{extent}\t{}", flat(file)));
        }
        for b in &self.books {
            write_book(&mut out, b);
        }
        for s in &self.sittings {
            write_sitting(&mut out, s);
        }
        if let Some(gate) = self.gate {
            row(
                &mut out,
                format_args!(
                    "n\t{}\t{}\t{}\t{}\t{}",
                    gate.len, gate.mtime, gate.sidecars, gate.dirs, gate.rules
                ),
            );
        }
        for m in &self.highlights {
            write_highlight(&mut out, m);
        }
        if let Some(g) = self.sources {
            row(
                &mut out,
                format_args!(
                    "g\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}",
                    g.vocab_len,
                    g.vocab_mtime,
                    g.clips_len,
                    g.clips_mtime,
                    g.sidecars,
                    g.dirs,
                    g.record,
                    g.rules
                ),
            );
        }
        out
    }

    /// Take `highlights` as the whole of [`Self::highlights`], with the gate the pass
    /// that read them stood at. Answers whether anything moved.
    pub fn take_highlights(&mut self, mut highlights: Vec<Highlight>, gate: Gate) -> bool {
        // `held` lends each fresh highlight the `start`, `end`, `colour` and
        // `State::Live` of the held one sharing its `identity`.
        let held: std::collections::HashMap<_, &Highlight> =
            self.highlights.iter().map(|m| (m.identity(), m)).collect();
        for highlight in &mut highlights {
            if let Some(stored) = held.get(&highlight.identity()).copied() {
                highlight.keep_from(stored);
            }
        }
        let moved = self.highlights != highlights || self.gate != Some(gate);
        self.highlights = highlights;
        self.gate = Some(gate);
        self.sort_highlights();
        moved
    }

    /// The highlights one book carries, in the order they were made.
    pub fn highlights_of(&self, extent: i64) -> impl Iterator<Item = &Highlight> {
        self.highlights.iter().filter(move |m| m.extent == extent)
    }

    /// Move [`Self::watermark`] and [`Self::reset_at`] onto the clock `offset` names at
    /// `at`, answering the seconds they moved, and write that clock into
    /// [`Self::clock`]. `offset` stepping forward moves neither.
    pub fn follow_clock(&mut self, at: i64, offset: Option<i64>) -> i64 {
        let Some(now) = offset else {
            return 0;
        };
        self.clock.learn(at, now);
        // `watermark_offset` unset compares against nothing, and takes `now` as the watermark's.
        let Some(held) = self.watermark_offset.replace(now) else {
            return 0;
        };
        let back = held - now;
        if back < CLOCK_STEP_SECS {
            return 0;
        }
        self.watermark = moved(&self.watermark, -back);
        self.reset_at = moved(&self.reset_at, -back);
        back
    }

    /// The instant a pass must start reading the log at: [`Self::watermark`], or under
    /// `Self::open_at_watermark` the newest sitting's own start, re-measuring that
    /// sitting whole. Never under [`Self::reset_at`].
    pub fn read_from(&self) -> String {
        self.starts_at().max(self.reset_at.clone())
    }

    /// [`Self::read_from`] before [`Self::reset_at`] is applied. An empty `sittings`
    /// answers the empty string, and [`Self::watermark`] bounds nothing.
    fn starts_at(&self) -> String {
        let Some(newest) = self.sittings.last() else {
            return String::new();
        };
        let start = log_stamp(&self.continued_from(newest).started_at).unwrap_or_default();
        if self.watermark.is_empty() || self.open_at_watermark(newest) {
            return start;
        }
        self.watermark.clone().max(start)
    }

    /// The sitting `sitting` continues across a midnight cut: the same book's
    /// sitting closing at `23:59:59` the day before, else `sitting` itself.
    fn continued_from<'a>(&'a self, sitting: &'a Sitting) -> &'a Sitting {
        let mut begun = sitting;
        while begun.started_at.ends_with("T00:00:00")
            && let Some(at) = instant(&begun.started_at)
        {
            let eve = crate::date::stamp((at - 1).div_euclid(86_400), (at - 1).rem_euclid(86_400));
            match self
                .sittings
                .iter()
                .rev()
                .find(|s| s.end_position == begun.end_position && s.ended_at == eve)
            {
                Some(before) => begun = before,
                None => break,
            }
        }
        begun
    }

    /// Whether [`Self::watermark`] falls within [`SITTING_GAP_SECS`] of `sitting`
    /// ending, true where a stamp will not parse.
    fn open_at_watermark(&self, sitting: &Sitting) -> bool {
        let (Some(ended), Some(watermark)) = (
            instant(&sitting.ended_at),
            instant(&iso_stamp(&self.watermark)),
        ) else {
            return true;
        };
        watermark - ended < SITTING_GAP_SECS
    }

    /// Fold `lines` into the record: what `lines` measured replaces every sitting
    /// at or after `from`, less those [`Self::barred`] answers for.
    pub fn absorb(&mut self, lines: &[String], from: &str) -> (usize, usize) {
        let refs: Vec<&str> = lines.iter().map(String::as_str).collect();
        let cut = match from.is_empty() {
            true => String::new(),
            false => iso_stamp(from),
        };
        // The sittings `parsed` replaces, which state the counters a sitting
        // opened at where the lines before `from` did.
        let restated = match cut.is_empty() {
            true => &[][..],
            false => {
                let at = self.sittings.partition_point(|s| s.started_at < cut);
                &self.sittings[at..]
            }
        };
        let parsed =
            crate::log::sitting::parse_restating(refs.iter().copied(), &self.counters, restated);
        // Ahead of the sittings: `Self::barred` places one through `ends`.
        for (k, v) in crate::log::line::frombook_map(refs.iter().copied()) {
            self.learn_end(k, v);
        }
        self.sort_ends();
        for (ep, ms, words) in crate::log::line::counter_map(refs.iter().copied()) {
            self.learn_counter(ep, ms, words);
        }
        let mut parsed: Vec<Sitting> = parsed.into_iter().filter(|s| !self.barred(s)).collect();
        // The clock this pass ran under, on the sittings it caught up with, an
        // older one keeping its own pass's.
        if let Some(offset) = self.watermark_offset {
            let (today, _) = crate::date::now();
            for sitting in &mut parsed {
                let day = crate::date::parse_day(crate::date::day_of(&sitting.started_at));
                if day.is_some_and(|day| (today - day).abs() <= 1) {
                    sitting.tz_offset_s = Some(offset);
                }
            }
        }

        let before = self.sittings.len();
        // `parsed` states everything from `cut` on, and an empty `cut` nothing.
        if !cut.is_empty() {
            self.sittings.retain(|s| s.started_at < cut);
        }
        let dropped = before - self.sittings.len();
        let found = parsed.len();
        self.sittings.extend(parsed);

        if let Some(newest) = refs.iter().filter_map(|l| line_stamp(l)).max()
            && newest > self.watermark.as_str()
        {
            self.watermark = newest.to_string();
        }
        self.sort();
        (found.saturating_sub(dropped), dropped.min(found))
    }

    /// Whether `sitting` starts under [`Self::reset_at`], or under the stamp of
    /// the `c` row identifying its book.
    fn barred(&self, sitting: &Sitting) -> bool {
        if !self.reset_at.is_empty() && sitting.started_at < iso_stamp(&self.reset_at) {
            return true;
        }
        if self.cleared.is_empty() {
            return false;
        }
        let extent = self.extent_of(sitting.end_position);
        match self.cleared_at(extent, sitting.asin.as_deref()) {
            Some(at) => sitting.started_at < iso_stamp(at),
            None => false,
        }
    }

    /// When a book was last cleared, found the way [`Self::slot_for`] finds the
    /// book: under `extent` where one is stated, else under `key`.
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
        let now = crate::date::epoch_now();
        let rewound = self.follow_clock(now, crate::zone::offset_at(now));
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

    /// Parse the whole log into a [`Record`] of its own and fold it in through
    /// [`Self::merge`]. Clears [`Self::reset_at`], keeps the `c` rows. Answers
    /// the sittings [`Self::merge`] added.
    pub fn rebuild(&mut self, on: &mut dyn FnMut(usize, usize)) -> usize {
        self.rebuild_from(
            Path::new(source::LIVE_LOG),
            Path::new(source::LOG_DIR),
            Path::new(source::DUMP_DIR),
            on,
        )
    }

    /// Re-measure every sitting [`source::collect_from`] restates. Answers
    /// the stored rows whose figures moved.
    pub fn heal(&mut self, on: &mut dyn FnMut(usize, usize)) -> usize {
        self.heal_from(
            Path::new(source::LIVE_LOG),
            Path::new(source::LOG_DIR),
            Path::new(source::DUMP_DIR),
            on,
        )
    }

    /// Every end position each counter pair was stated for, all pairs at
    /// once: the sittings carrying their own last reading, and the `t` rows
    /// standing for the end positions no sitting counter reaches.
    fn end_positions_by_counter(&self) -> std::collections::HashMap<(i64, i64), Vec<i64>> {
        let mut out: std::collections::HashMap<(i64, i64), Vec<i64>> =
            std::collections::HashMap::new();
        let stated = self
            .sittings
            .iter()
            .filter_map(|s| Some((s.end_counter_ms?, s.end_words?, s.end_position)))
            .chain(
                self.counters
                    .iter()
                    .map(|(ep, ms, words)| (*ms, *words, *ep)),
            );
        for (ms, words, ep) in stated {
            out.entry((ms, words)).or_default().push(ep);
        }
        for held in out.values_mut() {
            held.sort_unstable();
            held.dedup();
        }
        out
    }

    /// Whether an end position no book names holds no counter, on the sitting or in
    /// [`Self::counters`] — what a pass over the logs can add.
    pub fn wants_the_logs(&self) -> bool {
        self.sittings.iter().any(|s| {
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

    /// Read the whole log for what each end position states — its book end
    /// in [`Self::ends`], its counter in [`Self::counters`] — folding in no
    /// sitting. Answers the end positions that gained a counter.
    pub fn relearn_end_positions(&mut self, on: &mut dyn FnMut(usize, usize)) -> usize {
        self.end_positions_from(
            Path::new(source::LIVE_LOG),
            Path::new(source::LOG_DIR),
            Path::new(source::DUMP_DIR),
            on,
        )
    }

    /// [`Self::relearn_end_positions`] over the three log sources named.
    fn end_positions_from(
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
    pub fn rebuild_from(
        &mut self,
        live: &Path,
        chunks: &Path,
        dumps: &Path,
        on: &mut dyn FnMut(usize, usize),
    ) -> usize {
        let got = source::collect_from(live, chunks, dumps, "", on);
        let mut whole = Record {
            ends: self.ends.clone(),
            counters: self.counters.clone(),
            cleared: self.cleared.clone(),
            ..Record::default()
        };
        whole.absorb(&got.lines, "");
        // `opened`: the start the log states for a stored sitting closing on the
        // same second, taken as `Sitting::remeasure` takes it.
        let opened: std::collections::HashMap<(i64, &str), &str> = whole
            .sittings
            .iter()
            .map(|s| ((s.end_position, s.ended_at.as_str()), s.started_at.as_str()))
            .collect();
        for held in &mut self.sittings {
            if let Some(&start) = opened.get(&(held.end_position, held.ended_at.as_str()))
                && start < held.started_at.as_str()
            {
                held.started_at = start.to_string();
            }
        }
        // `restated`: the sittings the log restates, given up for the parse that read them.
        let mut restated: Vec<(&str, i64, &str)> = whole
            .sittings
            .iter()
            .map(|s| (s.started_at.as_str(), s.end_position, s.ended_at.as_str()))
            .collect();
        restated.sort_unstable();
        self.sittings.retain(|s| {
            restated
                .binary_search(&(s.started_at.as_str(), s.end_position, s.ended_at.as_str()))
                .is_err()
        });
        let added = self.merge(&whole);
        self.watermark = self.watermark.clone().max(whole.watermark);
        self.reset_at.clear();
        added
    }

    /// [`Self::heal`] over the three log sources named: [`Sitting::remeasure`]
    /// gives a stored row the fresh parse restates its figures, and
    /// [`Self::merge`] adds a sitting the record does not hold.
    pub fn heal_from(
        &mut self,
        live: &Path,
        chunks: &Path,
        dumps: &Path,
        on: &mut dyn FnMut(usize, usize),
    ) -> usize {
        let got = source::collect_from(live, chunks, dumps, "", on);
        // `whole` parses under no `reset_at` and no `c` row: `Self::barred` holds
        // over what merges, below, and not over what this corrects.
        let mut whole = Record {
            ends: self.ends.clone(),
            counters: self.counters.clone(),
            ..Record::default()
        };
        whole.absorb(&got.lines, "");
        let mut healed = 0;
        // `added` holds the fresh rows that corrected none.
        let mut added: Vec<Sitting> = Vec::new();
        for fresh in &whole.sittings {
            // `started_at` and `ended_at` each name one sitting of a book, and
            // `ended_at` the one a midnight cut starts at the boundary.
            let at = self
                .sittings
                .iter()
                .position(|s| {
                    s.end_position == fresh.end_position && s.started_at == fresh.started_at
                })
                .or_else(|| {
                    self.sittings.iter().position(|s| {
                        s.end_position == fresh.end_position && s.ended_at == fresh.ended_at
                    })
                });
            match at {
                Some(at) if self.sittings[at].remeasure(fresh) => healed += 1,
                Some(_) => {}
                None => added.push(fresh.clone()),
            }
        }
        // `merge` takes no row [`Self::reset_at`] or a `c` row holds back.
        whole.sittings = added;
        whole.sittings.retain(|s| !self.barred(s));
        self.merge(&whole);
        self.watermark = self.watermark.clone().max(whole.watermark);
        healed
    }

    /// Fold what `library` states into [`Self::books`], answering how many
    /// books changed: `library` merges into each book it lists, and leaves
    /// a book it stops listing what it holds.
    pub fn remember(&mut self, library: &[library::Book]) -> usize {
        let was_on: Vec<bool> = self.books.iter().map(|r| r.on_kindle).collect();
        // `moved` opens on `was_on`, moving each book the library stops listing.
        let mut moved = was_on.clone();
        for book in &mut self.books {
            book.on_kindle = false;
        }
        // Every slot a `cde_key` reaches, in slot order.
        let mut by_key: std::collections::HashMap<String, Vec<usize>> =
            std::collections::HashMap::new();
        for (at, book) in self.books.iter().enumerate() {
            by_key.entry(book.cde_key.clone()).or_default().push(at);
        }
        // The slots `library` stated a `p_percentFinished` for on this pass.
        let mut stated: std::collections::HashSet<usize> = std::collections::HashSet::new();
        for listed in library {
            let slot = match slot_in(&by_key, &self.books, listed) {
                Some(i) => {
                    let changed = merge(&mut self.books[i], listed);
                    // `None`: a slot past `was_on`, pushed by an earlier row of this pass.
                    moved[i] = match was_on.get(i) {
                        Some(&was) => changed || was != self.books[i].on_kindle,
                        None => true,
                    };
                    i
                }
                // `book` unsized under a key `by_key` holds names no second book.
                None if listed.extent == 0 && by_key.contains_key(&listed.cde_key) => continue,
                None => {
                    let at = self.books.len();
                    by_key.entry(listed.cde_key.clone()).or_default().push(at);
                    self.books.push(taken(listed));
                    moved.push(true);
                    at
                }
            };
            if listed.percent >= 0.0 {
                stated.insert(slot);
            }
            if listed.extent != 0 && !listed.cde_key.is_empty() {
                self.learn_key(listed.extent, &listed.cde_key);
            }
        }
        self.note_progress(&stated);
        self.sort_books();
        moved.into_iter().filter(|&m| m).count()
    }

    /// Take each book's `timer.model` counter off the sidecar of its own file,
    /// through [`Self::pairs`]. Answers how many books moved.
    pub fn note_timers(&mut self, sidecars: &[sidecar::Counter]) -> usize {
        let mut moved = 0;
        for card in sidecars {
            let Some((extent, _)) = self.pairs.iter().find(|(_, file)| *file == card.file) else {
                continue;
            };
            let Some(slot) = self.slot_for(*extent, None) else {
                continue;
            };
            let book = &mut self.books[slot];
            if (book.timer_ms, book.timer_words, book.timer_covered)
                == (card.total_ms, card.words, card.covered)
            {
                continue;
            }
            book.timer_ms = card.total_ms;
            book.timer_words = card.words;
            book.timer_covered = card.covered;
            moved += 1;
        }
        moved
    }

    /// Identify reading the library cannot: `sidecars` against [`Self::end_positions_by_counter`].
    pub fn recover(&mut self, sidecars: &[sidecar::Counter]) -> usize {
        // Drop a pairing whose extent a book carries under another key.
        if !self.pairs.is_empty() {
            let mut keyed: std::collections::HashMap<i64, Vec<&str>> =
                std::collections::HashMap::new();
            for book in &self.books {
                keyed
                    .entry(book.extent)
                    .or_default()
                    .push(book.cde_key.as_str());
            }
            let drop: Vec<(i64, String)> = self
                .pairs
                .iter()
                .filter(|(extent, file)| {
                    keyed
                        .get(extent)
                        .is_some_and(|keys| keys.iter().any(|k| k != file))
                })
                .cloned()
                .collect();
            self.pairs.retain(|held| !drop.contains(held));
        }
        // Nothing to identify and nothing left to drop.
        if sidecars.is_empty() {
            return 0;
        }
        // The end positions each counter pair was ever stated for.
        let end_positions = self.end_positions_by_counter();
        let mut claims: Vec<(i64, &str)> = Vec::new();
        for card in sidecars {
            if card.total_ms == 0 {
                continue;
            }
            let [end_position] = end_positions
                .get(&(card.total_ms, card.words))
                .map_or(&[][..], Vec::as_slice)
            else {
                continue;
            };
            claims.push((self.extent_of(*end_position), &card.file));
        }
        let mut identified = 0;
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
            identified += 1;
        }
        self.sort_books();
        identified
    }

    /// Every end position with sittings that [`Self::wants`] answers `by`
    /// for: one no book names, and one whose `identified_by` ranks under `by`.
    /// [`crate::identify::from_sources`] reads no source an empty answer names.
    pub fn end_positions_wanting(&self, by: IdentifiedBy) -> Vec<i64> {
        let mut out: Vec<i64> = self
            .sittings
            .iter()
            .map(|s| (self.extent_of(s.end_position), s.asin.as_deref()))
            .filter(|(extent, key)| self.wants(*extent, *key, by))
            .map(|(extent, _)| extent)
            .collect();
        out.sort_unstable();
        out.dedup();
        out
    }

    /// The record as the identifying pass reads it, in one number: the sittings'
    /// brackets and counters, the end position each states, and the books a
    /// claim could land on.
    pub fn identify_stamp(&self) -> u64 {
        let mut stamp = crate::stamp::Stamp::default();
        for s in &self.sittings {
            stamp.text(&s.started_at);
            stamp.text(&s.ended_at);
            stamp.num(s.end_position);
            stamp.num(s.seconds);
            stamp.text(s.asin.as_deref().unwrap_or_default());
            stamp.num(s.end_counter_ms.unwrap_or(-1));
            stamp.num(s.end_words.unwrap_or(-1));
        }
        for b in &self.books {
            stamp.num(b.extent);
            stamp.text(&b.cde_key);
            stamp.text(&b.title);
            stamp.num(b.identified_by as i64);
        }
        // `ends` is what `extent_of` reads an end position through, and `keys` and
        // `pairs` are what a claim is landed by.
        for (k, v) in &self.ends {
            stamp.num(*k);
            stamp.num(*v);
        }
        for (k, v) in &self.keys {
            stamp.num(*k);
            stamp.text(v);
        }
        for (k, v) in &self.pairs {
            stamp.num(*k);
            stamp.text(v);
        }
        for (ep, ms, words) in &self.counters {
            stamp.num(*ep);
            stamp.num(*ms);
            stamp.num(*words);
        }
        stamp.done()
    }

    /// Whether [`Self::wants`] answers `by` for any end position in
    /// [`Self::sittings`].
    pub fn wants_identifying(&self, by: IdentifiedBy) -> bool {
        self.sittings
            .iter()
            .any(|s| self.wants(self.extent_of(s.end_position), s.asin.as_deref(), by))
    }

    /// Whether a source ranked `by` may speak for `extent`.
    fn wants(&self, extent: i64, key: Option<&str>, by: IdentifiedBy) -> bool {
        match self.book_for(extent, key) {
            Some(book) => book.identified_by > by,
            None => true,
        }
    }

    /// Identify reading the library cannot, from what `witnesses` saw: each names the
    /// one end position bracketing its instant, over a sitting of at least
    /// [`CLAIM_MIN_SECS`]; `contested` holds the rest.
    pub fn identify_from(
        &mut self,
        witnesses: &[crate::identify::Witness],
        by: IdentifiedBy,
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
            // `witness.pos` past the book's own end was not made in that book.
            if witness.pos >= 0 && extent > 0 && witness.pos > extent {
                continue;
            }
            if seconds < CLAIM_MIN_SECS
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

        let mut identified = 0;
        let mut from = 0;
        while from < claims.len() {
            let extent = claims[from].extent;
            let to = from + claims[from..].partition_point(|c| c.extent == extent);
            let said = &claims[from..to];
            from = to;
            // `said` naming two titles for one end position: `contested` holds it
            // from every source after it.
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
                    // `self.books[slot]` at extent 0 takes `extent`; one carrying its
                    // own keeps it, the `k` row reaching it.
                    if self.books[slot].extent == 0 {
                        self.books[slot].extent = extent;
                    }
                    self.books[slot].identified_by = self.books[slot].identified_by.min(by);
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
            identified += 1;
        }
        self.sort_books();
        identified
    }

    /// The book a claim names: the one `key` names, else the only one
    /// carrying `name`. `None` where two books share `name`.
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

    /// Drop the book a source weaker than `by` made for `extent`, and the
    /// `pairs` and `keys` rows reaching it.
    fn give_up(&mut self, extent: i64, by: IdentifiedBy) -> bool {
        let Some(slot) = self
            .books
            .iter()
            .position(|b| b.extent == extent && b.identified_by > by)
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
        self.sittings
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

    /// The book whose `cde_key` `file` contains, where exactly one does and it
    /// states no extent.
    fn keyed_by_name(&self, file: &str) -> Option<usize> {
        let mut found = self.books.iter().enumerate().filter(|(_, b)| {
            b.extent == 0 && b.cde_key.len() >= KEY_IN_NAME && file.contains(&b.cde_key)
        });
        let (slot, _) = found.next()?;
        found.next().is_none().then_some(slot)
    }

    /// Give the book at `slot` the progress its newest sitting states.
    /// `sittings` ascends by `started_at`.
    fn stand_where_read(&mut self, slot: usize) {
        let extent = self.books[slot].extent;
        for i in 0..self.sittings.len() {
            let Some(progress) = self.sittings[i].progress else {
                continue;
            };
            if self.extent_of(self.sittings[i].end_position) != extent {
                continue;
            }
            self.books[slot].stand_at((progress * 100.0).clamp(0.0, 100.0));
        }
    }

    /// Give each book outside `stated` the furthest its sittings reached,
    /// where that stands past the progress it holds. `%Left` fills the gap a
    /// deletion leaves and stops a page short of the end.
    fn note_progress(&mut self, stated: &std::collections::HashSet<usize>) {
        for i in 0..self.sittings.len() {
            let Some(progress) = self.sittings[i].progress else {
                continue;
            };
            let extent = self.extent_of(self.sittings[i].end_position);
            let key = self.sittings[i].asin.clone();
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
    /// is credited to, and one [`Self::clear_book`] set `kept`, less those
    /// [`Book::is_book`] refuses.
    fn shown_slots(&self) -> std::collections::HashSet<usize> {
        let mut out: std::collections::HashSet<usize> = self
            .sittings
            .iter()
            .filter_map(|s| self.slot_for(self.extent_of(s.end_position), s.asin.as_deref()))
            .collect();
        out.extend((0..self.books.len()).filter(|&slot| self.books[slot].kept));
        out.retain(|&slot| self.books[slot].is_book());
        out
    }

    /// The record in one short line: what it holds, and how far the pass has
    /// read.
    pub fn said(&self) -> String {
        format!(
            "{} s, {} b, {} a, watermark {}",
            self.sittings.len(),
            self.books.len(),
            self.highlights.len(),
            match self.watermark.is_empty() {
                true => "none",
                false => self.watermark.as_str(),
            },
        )
    }

    /// Copy the cover of each slot [`Self::shown_slots`] answers into `dir`,
    /// point its `cover` at the copy, and delete every other file there,
    /// leaving an empty `cover` on a slot left out.
    pub fn keep_covers(&mut self, dir: &Path) -> CoverSweep {
        self.keep_covers_from(dir, Path::new(covers::THUMBNAILS_DIR))
    }

    /// [`Self::keep_covers`] against a `thumbnails` directory: `thumbnail` where
    /// it names a file, else what `covers::cached` answers under `cde_key`.
    pub fn keep_covers_from(&mut self, dir: &Path, thumbnails: &Path) -> CoverSweep {
        let shown = self.shown_slots();
        // `shown` empty over a non-empty `books` changes no book and drops no file.
        if shown.is_empty() && !self.books.is_empty() {
            return CoverSweep::default();
        }
        let mut cached: Option<std::collections::HashMap<String, PathBuf>> = None;
        let mut kept = 0;
        let mut uncovered = 0;
        // Titles named on one line by `CoverSweep::said`.
        let mut lost: Vec<String> = Vec::new();
        for (slot, book) in self.books.iter_mut().enumerate() {
            if !shown.contains(&slot) {
                kept += usize::from(!std::mem::take(&mut book.cover).is_empty());
                continue;
            }
            let at = covers::path(dir, &book.cde_key);
            if !covers::held(dir, &book.cde_key) {
                let stated = Path::new(book.thumbnail.as_str());
                let picture = match stated.is_file() {
                    true => Some(stated.to_path_buf()),
                    false => cached
                        .get_or_insert_with(|| covers::cached(thumbnails))
                        .get(&book.cde_key)
                        .cloned(),
                };
                let taken = match picture {
                    Some(picture) => match covers::keep(dir, &book.cde_key, &picture) {
                        Ok(_) => true,
                        // `covers::drawable` refused `picture`. `uncovered`
                        // counts these, and one line states the count.
                        Err(err) if err.kind() == std::io::ErrorKind::InvalidData => {
                            uncovered += 1;
                            false
                        }
                        Err(err) => {
                            lost.push(format!("{} ({err})", book.title));
                            false
                        }
                    },
                    None => {
                        if !book.thumbnail.is_empty() {
                            lost.push(book.title.clone());
                        }
                        false
                    }
                };
                if !taken {
                    // `at` goes with `cover`, `Book::cover_file` falling to `thumbnail`.
                    let _ = std::fs::remove_file(&at);
                    kept += usize::from(!std::mem::take(&mut book.cover).is_empty());
                    continue;
                }
            }
            let at = at.to_string_lossy();
            if book.cover != at {
                book.cover = at.into_owned();
                kept += 1;
            }
        }
        let held: Vec<&str> = shown
            .iter()
            .map(|&slot| self.books[slot].cde_key.as_str())
            .collect();
        let swept = covers::sweep(dir, &held);
        CoverSweep {
            kept,
            held: held.len(),
            swept,
            uncovered,
            lost,
        }
    }

    /// Where `book` sits in [`Self::books`]: under its `extent`, else its key.
    /// A `book` stating no extent reaches a book carrying one.
    #[cfg(test)]
    fn slot_of(&self, book: &library::Book) -> Option<usize> {
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

    /// The [`Book`] a sitting reaches: by `extent` first, then by `key`.
    ///
    /// `key` reaches a book whose `p_contentSize` `library` never stated.
    pub fn book_for(&self, extent: i64, key: Option<&str>) -> Option<&Book> {
        self.slot_for(extent, key).map(|i| &self.books[i])
    }

    /// Where [`Self::book_for`]'s answer sits in [`Self::books`]. `extent`
    /// answers, and `key` takes it where `key` reaches the lower [`IdentifiedBy`].
    fn slot_for(&self, extent: i64, key: Option<&str>) -> Option<usize> {
        let by_extent = match extent {
            0 => None,
            e => self.slot_at(e),
        };
        let by_key = key
            .filter(|k| !k.is_empty())
            .and_then(|k| self.books.iter().position(|b| b.cde_key == k));
        let identified = |slot: usize| self.books[slot].identified_by;
        match (by_extent, by_key) {
            (Some(e), Some(k)) if identified(k) < identified(e) => return Some(k),
            (Some(e), _) => return Some(e),
            (None, Some(k)) => return Some(k),
            (None, None) => {}
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

    /// The first book carrying `extent`, over the key [`Self::sort_books`]
    /// orders on. `partition_point` answers the **first** of a run of books
    /// sharing an extent, never `binary_search`.
    fn slot_at(&self, extent: i64) -> Option<usize> {
        let at = self.books.partition_point(|b| b.extent < extent);
        self.books
            .get(at)
            .filter(|b| b.extent == extent)
            .map(|_| at)
    }

    /// The book file a sidecar paired `extent` with, where exactly one did.
    fn file_at(&self, extent: i64) -> Option<&str> {
        let mut found = self.pairs.iter().filter(|(e, _)| *e == extent);
        let (_, only) = found.next()?;
        found.next().is_none().then_some(only.as_str())
    }

    /// The `cde_key` some pass paired `extent` with, where exactly one did.
    fn key_at(&self, extent: i64) -> Option<&str> {
        let mut found = self.keys.iter().filter(|(e, _)| *e == extent);
        let (_, only) = found.next()?;
        found.next().is_none().then_some(only.as_str())
    }

    /// Set [`Book::finished`] on the book [`Self::book_for`] answers
    /// for `extent` and `key`, answering whether the value changed; `finished`
    /// clears [`Book::restart`].
    pub fn set_finished(&mut self, extent: i64, key: &str, finished: bool) -> bool {
        let Some(slot) = self.slot_for(extent, Some(key)) else {
            return false;
        };
        let book = &mut self.books[slot];
        let changed = book.finished != finished;
        book.finished = finished;
        if finished {
            book.restart = None;
        }
        changed
    }

    /// Write `state` down as [`Book::read_state`], leaving
    /// [`Book::finished`] where it stands.
    pub fn note_mark(&mut self, extent: i64, key: &str, state: i64) {
        if let Some(slot) = self.slot_for(extent, Some(key)) {
            self.books[slot].read_state = state;
        }
    }

    /// Restart the book [`Self::book_for`] answers for, on `day`: `finished`
    /// banks on [`Book::restarted_on`] and comes off, `percent` becomes
    /// [`Book::restart`] and reads 0. Answers whether anything changed.
    pub fn restart(&mut self, extent: i64, key: &str, day: i64) -> bool {
        let Some(slot) = self.slot_for(extent, Some(key)) else {
            return false;
        };
        let book = &mut self.books[slot];
        let changed = book.finished || book.percent > 0.0;
        if book.finished {
            book.restarted_on.push(day);
        }
        book.finished = false;
        if book.percent > 0.0 {
            book.restart = Some(book.percent);
            book.percent = 0.0;
        }
        changed
    }

    /// Clear `sittings`, `books` and `ends`, and set [`Self::reset_at`] from
    /// [`Self::watermark`]. `watermark` and the `c` rows stand. Answers false where
    /// `watermark` is empty.
    pub fn reset(&mut self) -> bool {
        if self.watermark.is_empty() {
            return false;
        }
        self.sittings.clear();
        self.books.clear();
        self.ends.clear();
        self.reset_at = self.watermark.clone();
        true
    }

    /// [`Self::merge`] `other`, taking the stronger claim on every book both
    /// records hold into the book `Self::sort_books` keeps.
    pub fn fold_in(&mut self, other: &Record) -> usize {
        let mut at: std::collections::HashMap<(i64, &str), usize> =
            std::collections::HashMap::with_capacity(self.books.len());
        for (slot, book) in self.books.iter().enumerate() {
            at.insert((book.extent, book.cde_key.as_str()), slot);
        }
        let both: Vec<(usize, &Book)> = other
            .books
            .iter()
            .filter_map(|incoming| {
                let slot = *at.get(&(incoming.extent, incoming.cde_key.as_str()))?;
                Some((slot, incoming))
            })
            .collect();
        for (slot, incoming) in both {
            take_stronger(&mut self.books[slot], incoming);
        }
        self.merge(other)
    }

    /// Fold `other`'s sittings, ends and books into this one, through
    /// [`Self::sort`]. [`Self::reset_at`] and the `c` rows stand. Answers the
    /// sittings added.
    pub fn merge(&mut self, other: &Record) -> usize {
        let before = self.sittings.len();
        self.take_sittings(&other.sittings);
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
        let held = self.highlights.len();
        // `keep_from` takes the sidecar fields of a highlight `other` names too, and
        // `push` adds one it does not.
        for fresh in &other.highlights {
            match self
                .highlights
                .iter_mut()
                .find(|m| m.identity() == fresh.identity())
            {
                Some(mine) => mine.keep_from(fresh),
                None => self.highlights.push(fresh.clone()),
            }
        }
        self.sort_highlights();
        // `sort_highlights` puts two highlights equal in every field side by side.
        self.highlights.dedup_by(|a, b| a == b);
        if self.highlights.len() != held {
            self.gate = None;
        }
        self.sort();
        self.sittings.len() - before
    }

    /// Fold `fresh` into `sittings`. A `row` closing where a held row of its book
    /// closes hands that row [`Sitting::keep_close`] from a start no earlier, and
    /// [`Sitting::remeasure`] from an earlier one.
    fn take_sittings(&mut self, fresh: &[Sitting]) {
        // `closes`: the held row of the earliest start closing on each second.
        let mut closes: std::collections::HashMap<(i64, String), usize> =
            std::collections::HashMap::with_capacity(self.sittings.len());
        for (at, s) in self.sittings.iter().enumerate() {
            let held = closes
                .entry((s.end_position, s.ended_at.clone()))
                .or_insert(at);
            if s.started_at < self.sittings[*held].started_at {
                *held = at;
            }
        }
        for row in fresh {
            let close = (row.end_position, row.ended_at.clone());
            match closes.get(&close) {
                Some(&at) if self.sittings[at].started_at <= row.started_at => {
                    self.sittings[at].keep_close(row);
                }
                Some(&at) => {
                    self.sittings[at].remeasure(row);
                }
                None => {
                    closes.insert(close, self.sittings.len());
                    self.sittings.push(row.clone());
                }
            }
        }
    }

    /// One book's rows as a [`Record`]: its `b` row, the sittings
    /// [`Self::slot_for`] places on it, and the `e` rows keying them. Empty
    /// where `extent` and `key` reach no book.
    pub fn one_book(&self, extent: i64, key: &str) -> Record {
        let Some(slot) = self.slot_for(extent, Some(key)) else {
            return Record::default();
        };
        let sittings: Vec<Sitting> = self
            .sittings
            .iter()
            .filter(|s| {
                self.slot_for(self.extent_of(s.end_position), s.asin.as_deref()) == Some(slot)
            })
            .cloned()
            .collect();
        let ends = self
            .ends
            .iter()
            .filter(|(k, _)| sittings.iter().any(|s| s.end_position == *k))
            .copied()
            .collect();
        let key = self.books[slot].cde_key.clone();
        let counters = self
            .counters
            .iter()
            .filter(|(ep, _, _)| sittings.iter().any(|s| s.end_position == *ep))
            .copied()
            .collect();
        Record {
            sittings,
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
            highlights: self
                .highlights_of(self.books[slot].extent)
                .cloned()
                .collect(),
            // `clock` is the record's, and `highlights` state instants placed on it.
            clock: self.clock.clone(),
            // `watermark`, `reset_at`, the `c` rows and the two gates key the record, not one book.
            watermark: String::new(),
            watermark_offset: None,
            reset_at: String::new(),
            cleared: Vec::new(),
            gate: None,
            sources: None,
        }
    }

    /// Put one book back to zero: [`Self::drop_reading`] takes its sittings,
    /// [`Self::restart`] its progress, and [`Book::kept`] keeps it listed.
    /// Answers the sittings dropped.
    pub fn clear_book(&mut self, extent: i64, key: &str) -> usize {
        let went = self.drop_reading(extent, key);
        self.restart(extent, key, 0);
        if let Some(slot) = self.slot_for(extent, Some(key)) {
            self.books[slot].kept = true;
            self.books[slot].restarted_on.clear();
        }
        went
    }

    /// [`Self::clear_book`], and the `b` row with it. Answers the sittings
    /// dropped. [`Self::remember`] writes a fresh `b` row for a book the
    /// library lists.
    pub fn forget_book(&mut self, extent: i64, key: &str) -> usize {
        let went = self.drop_reading(extent, key);
        if let Some(slot) = self.slot_for(extent, Some(key)) {
            self.books.remove(slot);
        }
        self.pairs.retain(|(_, f)| f != key);
        went
    }

    /// The highlights of one book, dropped, and `gate` with them.
    fn drop_highlights(&mut self, extent: i64) {
        let before = self.highlights.len();
        self.highlights.retain(|m| m.extent != extent);
        if self.highlights.len() != before {
            self.gate = None;
        }
    }

    /// The sittings of one book, dropped and held back. Answers how many went.
    fn drop_reading(&mut self, extent: i64, key: &str) -> usize {
        let Some(slot) = self.slot_for(extent, Some(key)) else {
            return 0;
        };
        self.drop_highlights(self.books[slot].extent);
        let kept: Vec<Sitting> = {
            // A sitting landing on this book, as `Stats::build` places one.
            let mine = |s: &Sitting| {
                self.slot_for(self.extent_of(s.end_position), s.asin.as_deref()) == Some(slot)
            };
            self.sittings.iter().filter(|s| !mine(s)).cloned().collect()
        };
        let went = self.sittings.len() - kept.len();
        self.sittings = kept;
        // An empty `watermark` names no instant to hold a parse below.
        if !self.watermark.is_empty() {
            self.cleared.push(Cleared {
                extent: self.books[slot].extent,
                key: self.books[slot].cde_key.clone(),
                at: self.watermark.clone(),
            });
            self.sort_cleared();
        }
        went
    }

    /// The library number for a sitting's own key, which is the key itself
    /// where no line ever stated the mapping.
    pub fn extent_of(&self, end_position: i64) -> i64 {
        self.ends
            .binary_search_by_key(&end_position, |(k, _)| *k)
            .map_or(end_position, |i| self.ends[i].1)
    }

    /// Orders `sittings` on `started_at`, `end_position` and `ended_at`, keeping
    /// one row per `started_at` and `end_position`.
    fn sort(&mut self) {
        // The latest `ended_at` sorts first and stands, `ended_at` moving between passes.
        self.sittings.sort_by(|a, b| {
            (&a.started_at, a.end_position, Reverse(&a.ended_at)).cmp(&(
                &b.started_at,
                b.end_position,
                Reverse(&b.ended_at),
            ))
        });
        self.sittings
            .dedup_by(|a, b| a.started_at == b.started_at && a.end_position == b.end_position);
        self.sort_ends();
        self.keys.sort();
        self.keys.dedup();
        self.counters.sort();
        self.counters.dedup();
        self.pairs.sort();
        self.pairs.dedup();
        self.sort_books();
        self.sort_cleared();
        self.sort_highlights();
    }

    /// How many sittings close on the second another sitting of their book closes
    /// on, from a later start: the shorter of two spans, one nested in the other.
    pub fn nested(&self) -> usize {
        let mut closes: Vec<(i64, &str)> = self
            .sittings
            .iter()
            .map(|s| (s.end_position, s.ended_at.as_str()))
            .collect();
        let rows = closes.len();
        closes.sort_unstable();
        closes.dedup();
        rows - closes.len()
    }

    /// Folds each sitting [`Self::nested`] counts into the row of the earliest
    /// `started_at` closing on its second, through [`Sitting::keep_close`].
    /// Answers how many went.
    pub fn drop_nested(&mut self) -> usize {
        let rows = self.sittings.len();
        // The earliest `started_at` sorts first among the rows closing on one second.
        self.sittings.sort_by(|a, b| {
            (a.end_position, &a.ended_at, &a.started_at).cmp(&(
                b.end_position,
                &b.ended_at,
                &b.started_at,
            ))
        });
        self.sittings.dedup_by(|gone, kept| {
            let one = gone.end_position == kept.end_position && gone.ended_at == kept.ended_at;
            if one {
                kept.keep_close(gone);
            }
            one
        });
        self.sort();
        rows - self.sittings.len()
    }

    /// Orders `highlights` by when they were made, then by the book and the position.
    fn sort_highlights(&mut self) {
        self.highlights.sort_by(|a, b| {
            (&a.at, a.extent, &a.title, a.kind, a.start, a.location)
                .cmp(&(&b.at, b.extent, &b.title, b.kind, b.start, b.location))
        });
    }

    /// Hold `extent` against `key`, in order, keeping a pairing held.
    fn learn_key(&mut self, extent: i64, key: &str) {
        let at = (extent, key.to_string());
        if let Err(i) = self.keys.binary_search(&at) {
            self.keys.insert(i, at);
        }
    }

    /// Hold `from_book` as the book end `end_position` names, the
    /// last statement standing.
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

    /// Hold `file` against `extent`, in order, keeping a pairing held.
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

    /// Orders and de-duplicates `books` on `extent` and `cde_key` together, neither unique alone.
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

/// The shortest `cde_key` [`Record::keyed_by_name`] matches inside a name.
const KEY_IN_NAME: usize = 10;

/// One sitting placed on the clock, for bracketing a witness.
struct Span {
    from: i64,
    to: i64,
    /// The extent a book is keyed by.
    extent: i64,
    /// `Sitting::asin`, which reaches a book the library never sized.
    key: Option<String>,
    seconds: i64,
}

/// What one witness said about one end position.
struct Claim {
    extent: i64,
    /// [`crate::identify::normalise`] over `title`, what two claims agree on.
    name: String,
    title: String,
    author: String,
    key: String,
}

/// The end position every sitting spanning `at` states, the key one of them
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

/// The [`Book`] a witness identified, keyed by `key` where stated, else
/// by `title`, as [`from_sidecar`] keys one by the file name.
fn from_witness(extent: i64, title: &str, author: &str, key: &str, by: IdentifiedBy) -> Book {
    Book {
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
        on_kindle: false,
        cover: String::new(),
        location: String::new(),
        finished: false,
        restart: None,
        restarted_on: Vec::new(),
        read_state: -1,
        timer_ms: 0,
        timer_words: 0,
        timer_covered: 0.0,
        kept: false,
        identified_by: by,
    }
}

/// A field with nothing in it that could be read as a separator.
fn flat(text: &str) -> String {
    text.replace(['\t', '\n', '\r'], " ")
}

/// The [`Book`] its sidecar alone identifies. `file` is the `.sdr`
/// directory's name without the suffix, standing as both `cde_key` and `title`.
fn from_sidecar(extent: i64, file: &str) -> Book {
    Book {
        extent,
        cde_key: flat(file),
        cde_type: String::new(),
        title: flat(file),
        author: String::new(),
        thumbnail: String::new(),
        language: String::new(),
        percent: -1.0,
        on_kindle: false,
        cover: String::new(),
        location: String::new(),
        finished: false,
        restart: None,
        restarted_on: Vec::new(),
        read_state: -1,
        timer_ms: 0,
        timer_words: 0,
        timer_covered: 0.0,
        kept: false,
        identified_by: IdentifiedBy::Sidecar,
    }
}

/// A `Book` holding everything `book` states.
fn taken(listed: &library::Book) -> Book {
    let mut book = Book {
        extent: listed.extent,
        cde_key: flat(&listed.cde_key),
        cde_type: flat(&listed.cde_type),
        title: flat(&listed.title),
        author: flat(&listed.author),
        thumbnail: flat(&listed.thumbnail),
        language: flat(&listed.language),
        percent: stored_percent(listed.percent),
        on_kindle: listed.on_kindle,
        cover: String::new(),
        location: flat(&listed.location),
        finished: listed.percent >= FINISHED_PERCENT,
        restart: None,
        restarted_on: Vec::new(),
        // What `take_mark` reads to answer whether `book` states a new mark.
        read_state: -1,
        timer_ms: 0,
        timer_words: 0,
        timer_covered: 0.0,
        kept: false,
        identified_by: IdentifiedBy::Library,
    };
    book.take_mark(listed.read_state);
    book
}

/// What one pass of [`Record::keep_covers`] came to.
#[derive(Default)]
pub struct CoverSweep {
    /// Books whose `cover` changed.
    pub kept: usize,
    /// Covers standing under the covers directory after the sweep.
    pub held: usize,
    /// Files the sweep dropped, naming no shown book.
    pub swept: usize,
    /// Books the record holds no cover for at all.
    pub uncovered: usize,
    /// Books whose `Book::cover` names a file `keep_covers_from` could not read.
    pub lost: Vec<String>,
}

impl CoverSweep {
    /// [`Self::held`], [`Self::lost`] and [`Self::uncovered`] on one line.
    pub fn said(&self) -> String {
        let mut out = format!("cov={}", self.held);
        if !self.lost.is_empty() {
            let _ = write!(out, "-{}", self.lost.len());
        }
        if self.uncovered > 0 {
            let _ = write!(out, "~{}", self.uncovered);
        }
        out
    }

    /// The books whose cover is gone, named on one line.
    pub fn lost_said(&self) -> Option<String> {
        (!self.lost.is_empty()).then(|| first_few(&self.lost))
    }
}

/// `names` on one line: the first few, then how many more there were.
fn first_few(names: &[String]) -> String {
    const SHOWN: usize = 5;
    let head = names[..names.len().min(SHOWN)].join(", ");
    match names.len() > SHOWN {
        true => format!("{head}, and {} more", names.len() - SHOWN),
        false => head,
    }
}

/// Where `book` sits: one under this key carrying this extent, else
/// the first under this key whatever its extent.
fn slot_in(
    by_key: &std::collections::HashMap<String, Vec<usize>>,
    books: &[Book],
    book: &library::Book,
) -> Option<usize> {
    let slots = by_key.get(&book.cde_key)?;
    if let Some(&at) = slots.iter().find(|&&at| books[at].extent == book.extent) {
        return Some(at);
    }
    // `book` unsized, a purchase row beside the file's, takes no slot.
    if book.extent == 0 {
        return None;
    }
    // `slots.first()`: a book made before the library stated a size, taking it.
    slots.first().copied()
}

/// Take what `incoming` claims about a book `held` names too, where the claim
/// is the stronger one. `on_kindle`, `location` and `read_state` stand, and a
/// `held` carrying a `restart` keeps its progress and its mark.
fn take_stronger(held: &mut Book, incoming: &Book) {
    for (field, stated) in [
        (&mut held.cde_type, &incoming.cde_type),
        (&mut held.title, &incoming.title),
        (&mut held.author, &incoming.author),
        (&mut held.thumbnail, &incoming.thumbnail),
        (&mut held.language, &incoming.language),
        (&mut held.cover, &incoming.cover),
    ] {
        if field.is_empty() {
            *field = stated.clone();
        }
    }
    held.kept |= incoming.kept;
    if incoming.restarted_on.len() > held.restarted_on.len() {
        held.restarted_on = incoming.restarted_on.clone();
    }
    if held.restart.is_some() {
        return;
    }
    if incoming.percent > held.percent {
        held.stand_at(incoming.percent);
    }
    held.finished |= incoming.finished;
}

/// Fold what the library states into `record`, answering whether anything
/// moved. `on_kindle` is [`Record::remember`]'s to judge.
fn merge(book: &mut Book, listed: &library::Book) -> bool {
    let mut moved = false;
    for (field, stated) in [
        (&mut book.cde_key, &listed.cde_key),
        (&mut book.cde_type, &listed.cde_type),
        (&mut book.title, &listed.title),
        (&mut book.author, &listed.author),
        (&mut book.thumbnail, &listed.thumbnail),
        (&mut book.language, &listed.language),
        (&mut book.location, &listed.location),
    ] {
        if !stated.is_empty() {
            let said = flat(stated);
            moved |= *field != said;
            *field = said;
        }
    }
    if listed.extent != 0 {
        moved |= book.extent != listed.extent;
        book.extent = listed.extent;
    }
    let before = (book.finished, book.restart, book.percent);
    book.take_mark(listed.read_state);
    if listed.percent >= 0.0 {
        book.stand_at(listed.percent);
    }
    book.on_kindle |= listed.on_kindle;
    moved |= before != (book.finished, book.restart, book.percent);
    // `book` comes from the library, which outranks every other `IdentifiedBy`.
    moved |= book.identified_by != IdentifiedBy::Library;
    book.identified_by = IdentifiedBy::Library;
    moved
}

/// Append one row and its line ending.
fn row(out: &mut String, args: std::fmt::Arguments) {
    use std::fmt::Write as _;
    let _ = out.write_fmt(args);
    out.push('\n');
}

/// Decimal places a `b` row carries for a book's progress, all a book holds.
const PERCENT_PLACES: usize = 6;

/// `percent` as a `b` row can hold it.
fn stored_percent(percent: f64) -> f64 {
    let scale = 10f64.powi(PERCENT_PLACES as i32);
    (percent * scale).round() / scale
}

fn write_book(out: &mut String, b: &Book) {
    row(
        out,
        format_args!(
            "b\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}",
            b.extent,
            flat(&b.cde_key),
            flat(&b.title),
            flat(&b.author),
            flat(&b.thumbnail),
            flat(&b.language),
            format_args!("{:.*}", PERCENT_PLACES, b.percent),
            u8::from(b.on_kindle),
            flat(&b.cover),
            flat(&b.location),
            u8::from(b.finished),
            b.restart
                .map(|p| format!("{p:.*}", PERCENT_PLACES))
                .unwrap_or_default(),
            b.read_state,
            flat(&b.cde_type),
            u8::from(b.kept),
            b.identified_by.as_str(),
            b.timer_ms,
            b.timer_words,
            format_args!("{:.*}", PERCENT_PLACES, b.timer_covered),
            b.restarted_on
                .iter()
                .map(i64::to_string)
                .collect::<Vec<String>>()
                .join(";"),
        ),
    )
}

/// A `b` row as a [`Book`]. `extent` is the one field a row has to carry: `next`
/// reads every field past the row's last as empty, which each parse below takes
/// for its default.
fn read_book<'a>(f: &mut impl Iterator<Item = &'a str>) -> Option<Book> {
    let mut next = || f.next().unwrap_or_default();
    Some(Book {
        extent: next().parse().ok()?,
        cde_key: next().to_string(),
        title: next().to_string(),
        author: next().to_string(),
        thumbnail: next().to_string(),
        language: next().to_string(),
        percent: next().parse().unwrap_or(-1.0),
        on_kindle: next().trim() == "1",
        cover: next().to_string(),
        location: next().to_string(),
        finished: next().trim() == "1",
        restart: next().trim().parse().ok(),
        // -1, which `take_mark` reads as a value the next library pass renews.
        read_state: next().trim().parse().unwrap_or(-1),
        cde_type: next().to_string(),
        kept: next().trim() == "1",
        // `IdentifiedBy::Library` where the row states none, `Record::migrate`
        // picking out a sidecar's.
        identified_by: IdentifiedBy::from_stored(next()).unwrap_or_default(),
        // 0 where the row states none, until a pass over the sidecars fills them.
        timer_ms: next().trim().parse().unwrap_or(0),
        timer_words: next().trim().parse().unwrap_or(0),
        timer_covered: next().trim().parse().unwrap_or(0.0),
        restarted_on: next()
            .split(';')
            .filter_map(|d| d.trim().parse().ok())
            .collect(),
    })
    // `read_through` marks a book the row left unmarked.
    .map(|mut book: Book| {
        book.finished |= book.read_through();
        book
    })
}

/// An `a` row: one highlight, as [`crate::highlights`] merged it. `m.at` is
/// the key. `m.start`/`m.end` are on the book's `p_contentSize` axis and
/// `m.location` is the display location the clipping stated.
fn write_highlight(out: &mut String, m: &Highlight) {
    row(
        out,
        format_args!(
            "a\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}",
            m.extent,
            flat(&m.title),
            m.kind.as_str(),
            m.at,
            m.state.as_str(),
            m.start,
            m.end,
            m.location,
            flat(&m.page),
            flat(&m.colour),
            flat(&m.body),
        ),
    )
}

/// An `a` row as a [`Highlight`]: `None` for a kind unknown here and for
/// `State::Retired`, neither of which [`crate::highlights::fold`] writes.
fn read_highlight<'a>(f: &mut impl Iterator<Item = &'a str>) -> Option<Highlight> {
    let mut next = || f.next().unwrap_or_default();
    let out = Highlight {
        extent: next().parse().unwrap_or(0),
        title: next().to_string(),
        kind: Kind::from_stored(next())?,
        at: next().trim().to_string(),
        state: State::from_stored(next()),
        start: next().trim().parse().unwrap_or(-1),
        end: next().trim().parse().unwrap_or(-1),
        location: next().trim().parse().unwrap_or(-1),
        page: next().to_string(),
        colour: next().to_string(),
        body: next().to_string(),
    };
    out.state.is_in_the_book().then_some(out)
}

/// What a pass over the identifying sources stood at, the next pass skipped whole
/// where it finds the same.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct Sources {
    /// `vocab.db`'s length and modification time.
    pub vocab_len: u64,
    pub vocab_mtime: i64,
    /// `My Clippings.txt`'s length and modification time.
    pub clips_len: u64,
    pub clips_mtime: i64,
    /// [`crate::sidecar::Survey::stamp`], and the directories it counted.
    pub sidecars: u64,
    pub dirs: usize,
    /// [`Record::identify_stamp`].
    pub record: u64,
    /// [`crate::identify::IDENTIFY_RULES`] as the pass that wrote this read them.
    pub rules: u32,
}

/// An `n` row: what the pass that wrote the `a` rows above it had seen,
/// `rules` 0 or `None` where a field is missing.
fn read_gate<'a>(f: &mut impl Iterator<Item = &'a str>) -> Option<Gate> {
    let mut next = || f.next().unwrap_or_default();
    Some(Gate {
        len: next().trim().parse().ok()?,
        mtime: next().trim().parse().ok()?,
        sidecars: next().trim().parse().ok()?,
        dirs: next().trim().parse().ok()?,
        rules: next().trim().parse().unwrap_or(0),
    })
}

/// A `g` row as a [`Sources`], every field it does not state zero.
fn read_sources<'a>(f: &mut impl Iterator<Item = &'a str>) -> Option<Sources> {
    let mut next = || f.next().unwrap_or_default().trim().to_string();
    Some(Sources {
        vocab_len: next().parse().ok()?,
        vocab_mtime: next().parse().ok()?,
        clips_len: next().parse().ok()?,
        clips_mtime: next().parse().ok()?,
        sidecars: next().parse().ok()?,
        dirs: next().parse().ok()?,
        record: next().parse().ok()?,
        rules: next().parse().unwrap_or(0),
    })
}

/// A `c` row as a [`Cleared`], `None` where it states no stamp.
fn read_cleared<'a>(f: &mut impl Iterator<Item = &'a str>) -> Option<Cleared> {
    let mut next = || f.next().unwrap_or_default();
    let out = Cleared {
        extent: next().parse().ok()?,
        key: next().to_string(),
        at: next().trim().to_string(),
    };
    (!out.at.is_empty()).then_some(out)
}

/// How far the offset must step back before a pass follows it as a zone change.
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

/// A stored `YYYY-MM-DDTHH:MM:SS` as seconds, for taking one from another.
fn instant(at: &str) -> Option<i64> {
    Some(crate::date::parse_day(crate::date::day_of(at))? * 86_400 + crate::date::secs_of(at))
}

/// `YYMMDD:HHMMSS` to the `YYYY-MM-DDTHH:MM:SS` a sitting is stored under.
fn iso_stamp(stamp: &str) -> String {
    match crate::log::line::stamp(&format!("{stamp} x")) {
        Some(m) => m.at,
        None => String::new(),
    }
}

/// The hours of a sitting as `h:s;h:s`, the one field carrying a list.
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

fn write_sitting(out: &mut String, s: &Sitting) {
    let num = |n: Option<i64>| n.map(|n| n.to_string()).unwrap_or_default();
    row(
        out,
        format_args!(
            "s\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}",
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
            s.paged_seconds,
            s.paged_words,
            s.timed_seconds,
            s.timed_words,
            format_args!("{:.*}", PERCENT_PLACES, s.covered),
            write_stretches(&s.stretches),
        ),
    )
}

fn read_sitting<'a>(f: &mut impl Iterator<Item = &'a str>) -> Option<Sitting> {
    let mut next = || f.next().unwrap_or_default();
    let started_at = next().to_string();
    let ended_at = next().to_string();
    if started_at.is_empty() || ended_at.is_empty() {
        return None;
    }
    Some(Sitting {
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
        // `None` for all four where the row carries no counters.
        start_counter_ms: next().parse().ok(),
        end_counter_ms: next().parse().ok(),
        start_words: next().parse().ok(),
        end_words: next().parse().ok(),
        tz_offset_s: next().parse().ok(),
        time_left: next().parse().ok(),
        stated_wpm: next().parse().ok(),
        awake_seconds: next().parse().unwrap_or(0),
        // 0 where the row states none, `StatsFrom::App` falling back until a heal.
        paged_seconds: next().parse().unwrap_or(0),
        paged_words: next().parse().unwrap_or(0),
        timed_seconds: next().parse().unwrap_or(0),
        timed_words: next().parse().unwrap_or(0),
        // 0.0 where the row states no share.
        covered: next().parse().unwrap_or(0.0),
        // No stretch where the row states no field here.
        stretches: read_stretches(next()),
    })
}

/// The stretches a sitting covered, as `from-to` pairs in hundredths of
/// a per cent: `500-800,1000-1200` is 5 %–8 % and then 10 %–12 %.
fn write_stretches(stretches: &[Stretch]) -> String {
    stretches
        .iter()
        .map(|r| {
            let at = |p: f64| (p * 10_000.0).round() as i64;
            format!("{}-{}", at(r.from), at(r.to))
        })
        .collect::<Vec<_>>()
        .join(",")
}

fn read_stretches(said: &str) -> Vec<Stretch> {
    said.split(',')
        .filter_map(|stretch| {
            // `split_once` on '-': every value written is positive.
            let (from, to) = stretch.split_once('-')?;
            let at = |s: &str| s.trim().parse::<f64>().ok().map(|n| n / 10_000.0);
            Some(Stretch {
                from: at(from)?,
                to: at(to)?,
            })
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// `remember` reaches a book through an index of `cde_key` to slot. It
    /// must pick the same book a pass over `books` in order picks, including
    /// which of several under one key.
    #[test]
    fn the_key_index_answers_what_the_scan_answered() {
        let mut record = Record::default();
        for (extent, key) in [
            (0, "shared"),
            (900, "shared"),
            (1500, "shared"),
            (0, "alone"),
            (77, "other"),
        ] {
            record.books.push(from_witness(
                extent,
                key,
                "An Author",
                key,
                IdentifiedBy::Library,
            ));
        }
        record.sort_books();

        let mut by_key: std::collections::HashMap<String, Vec<usize>> =
            std::collections::HashMap::new();
        for (at, held) in record.books.iter().enumerate() {
            by_key.entry(held.cde_key.clone()).or_default().push(at);
        }

        for (extent, key) in [
            (900, "shared"),  // one of several under the key, by extent
            (0, "shared"),    // the zero-extent one under the same key
            (4242, "shared"), // an extent none of them carries
            (0, "alone"),     // a key one record holds
            (77, "other"),
            (5, "nobody"), // a key no record holds
        ] {
            let book = listed(extent, key, key, -1.0);
            assert_eq!(
                slot_in(&by_key, &record.books, &book),
                record.slot_of(&book),
                "{key} at {extent}",
            );
        }
    }

    /// `bought` and the file row under one key fold onto one book, which a
    /// second `remember` leaves standing.
    #[test]
    fn a_purchase_row_never_takes_the_record_the_file_row_holds() {
        let bought = crate::library::Book {
            extent: 0,
            cde_key: "B00771M8JQ".into(),
            cde_type: "EBOK".into(),
            title: "Daijisen (Japanese Edition)".into(),
            author: "Shogakukan".into(),
            percent: -1.0,
            thumbnail: String::new(),
            language: "ja".into(),
            location: String::new(),
            on_kindle: false,
            read_state: -1,
            last_access: 1_406_465_985,
        };
        let downloaded = crate::library::Book {
            extent: 75_349_739,
            title: "Daijisen".into(),
            percent: 0.0,
            location: "/mnt/us/documents/Daijisen_B00771M8JQ.azw".into(),
            on_kindle: true,
            last_access: 1_782_381_327,
            ..bought.clone()
        };

        // Either order: one book, the file's own title.
        for library in [
            vec![bought.clone(), downloaded.clone()],
            vec![downloaded.clone(), bought.clone()],
        ] {
            let mut record = Record::default();
            record.remember(&library);
            assert_eq!(record.books.len(), 1, "{library:?}");
            assert_eq!(record.books[0].title, "Daijisen");
            assert_eq!(record.books[0].extent, 75_349_739);

            // `remember` again over the same library moves nothing.
            let text = record.text();
            let mut again = Record::from_text(&text);
            assert_eq!(again.remember(&library), 0, "a pass that changed nothing");
            assert_eq!(again.text(), text);
        }
    }

    /// `library` names one book twice, against a record holding no books, its
    /// second row reaching a slot the same `remember` pushed.
    #[test]
    fn a_row_restating_a_book_this_pass_added_reaches_no_snapshot() {
        let library = [
            listed(1, "B01", "One", -1.0),
            listed(938_018, "B00OKPCRLG", "Bible", 55.0),
            listed(938_018, "B00OKPCRLG", "Bible", 55.0),
        ];
        let mut record = Record::default();
        assert_eq!(record.remember(&library), 2, "both books are new");
        assert_eq!(record.books.len(), 2);
        // `remember` over the same `library` again moves nothing.
        assert_eq!(record.remember(&library), 0);
    }

    /// `PERCENT_PLACES` bounds what a book holds, and `remember` over the same
    /// `book` moves nothing on the pass after.
    #[test]
    fn progress_the_row_cannot_write_is_not_progress_the_record_keeps() {
        let book = crate::library::Book {
            extent: 500_100,
            cde_key: "B00OKPCRLG".into(),
            cde_type: "EBOK".into(),
            title: "A Book".into(),
            author: String::new(),
            percent: 6.391_478_5,
            thumbnail: String::new(),
            language: String::new(),
            location: "/mnt/us/documents/a.azw".into(),
            on_kindle: true,
            read_state: -1,
            last_access: 0,
        };
        let mut record = Record::default();
        record.remember(std::slice::from_ref(&book));
        // `text` read back states the same number.
        let text = record.text();
        let written = Record::from_text(&text);
        assert_eq!(written.books[0].percent, record.books[0].percent);

        let mut again = Record::from_text(&text);
        assert_eq!(again.remember(std::slice::from_ref(&book)), 0);
        assert_eq!(again.text(), text);
    }

    /// One book, at the progress and the mark each record gives it.
    fn one_book_at(percent: f64, finished: bool, title: &str) -> Record {
        let mut record = Record::default();
        let mut book = from_witness(
            938_018,
            title,
            "An Author",
            "B00OKPCRLG",
            IdentifiedBy::Library,
        );
        book.percent = percent;
        book.finished = finished;
        record.books.push(book);
        record
    }

    /// `sort_books` keeps the book standing here over the one a backup
    /// carries. `fold_in` takes the further progress and the standing mark out of
    /// the one it drops.
    #[test]
    fn a_book_both_records_hold_takes_the_further_progress() {
        let mut here = one_book_at(40.0, false, "A Book");
        here.fold_in(&one_book_at(80.0, true, "A Book"));
        assert_eq!(here.books.len(), 1);
        assert_eq!(here.books[0].percent, 80.0);
        assert!(here.books[0].finished);
    }

    /// `fold_in` an older backup of this record's own books, the newer progress
    /// standing.
    #[test]
    fn an_older_backup_of_this_records_own_books_moves_nothing() {
        let mut here = one_book_at(80.0, true, "A Book");
        let text = here.text();
        here.fold_in(&one_book_at(40.0, false, "A Book"));
        assert_eq!(here.text(), text);
    }

    /// `fold_in` takes a title, an author and a cover `here` has none of, and
    /// keeps the progress it holds.
    #[test]
    fn a_field_the_record_states_none_of_is_taken_from_the_backup() {
        let mut here = one_book_at(40.0, false, "");
        here.books[0].author = String::new();
        let mut backup = one_book_at(10.0, false, "A Book");
        backup.books[0].cover = "B00OKPCRLG.jpg".into();
        here.fold_in(&backup);
        assert_eq!(here.books[0].title, "A Book");
        assert_eq!(here.books[0].author, "An Author");
        assert_eq!(here.books[0].cover, "B00OKPCRLG.jpg");
        assert_eq!(here.books[0].percent, 40.0, "the progress moved backwards");
    }

    #[test]
    fn a_book_begun_again_part_way_through_counts_no_reading() {
        let mut record = one_book_at(46.0, false, "A Book");
        let (extent, key) = (record.books[0].extent, record.books[0].cde_key.clone());
        assert!(
            record.restart(extent, &key, 0),
            "progress is something to clear"
        );
        assert_eq!(record.books[0].restarted_on.len(), 0);
        // And again from the head of that reading, which clears nothing.
        assert!(!record.restart(extent, &key, 0));
        assert_eq!(record.books[0].restarted_on.len(), 0);
    }

    #[test]
    fn a_book_read_end_to_end_and_begun_again_counts_the_reading_that_ended() {
        let mut record = one_book_at(100.0, true, "A Book");
        let (extent, key) = (record.books[0].extent, record.books[0].cde_key.clone());
        assert_eq!(record.books[0].restarted_on.len(), 0);

        record.restart(extent, &key, 0);
        assert_eq!(record.books[0].restarted_on.len(), 1);
        assert!(!record.books[0].finished, "the mark comes off");

        // Part way through the second reading: `restarted_on` stands where it is.
        record.books[0].percent = 40.0;
        record.restart(extent, &key, 0);
        assert_eq!(record.books[0].restarted_on.len(), 1);

        // And through the end of a third, which the mark carries.
        record.books[0].finished = true;
        record.restart(extent, &key, 0);
        assert_eq!(record.books[0].restarted_on.len(), 2);
    }

    #[test]
    fn clearing_a_book_takes_the_readings_it_was_counted_for() {
        let mut record = one_book_at(100.0, true, "A Book");
        let (extent, key) = (record.books[0].extent, record.books[0].cde_key.clone());
        record.restart(extent, &key, 0);
        assert_eq!(record.books[0].restarted_on.len(), 1);
        record.clear_book(extent, &key);
        assert_eq!(record.books[0].restarted_on.len(), 0);
    }

    #[test]
    fn the_tally_rides_the_row_and_a_row_without_it_reads_zero() {
        let mut record = one_book_at(100.0, true, "A Book");
        let (extent, key) = (record.books[0].extent, record.books[0].cde_key.clone());
        record.restart(extent, &key, 0);
        record.restart(extent, &key, 0);
        let again = Record::from_text(&record.text());
        assert_eq!(again.books[0].restarted_on, record.books[0].restarted_on);

        // The row as an older build wrote it: every column but the last.
        let text = record.text();
        let cut: String = text
            .lines()
            .map(|line| match line.starts_with("b\t") {
                true => line.rsplit_once('\t').expect("a column").0.to_string(),
                false => line.to_string(),
            })
            .collect::<Vec<String>>()
            .join("\n");
        assert_eq!(Record::from_text(&cut).books[0].restarted_on.len(), 0);
    }

    /// `restart` is the floor a book's progress is read against, a book
    /// carrying one keeping the progress and the mark it set.
    #[test]
    fn a_restart_here_outranks_progress_and_a_mark_from_elsewhere() {
        let mut here = one_book_at(0.0, false, "A Book");
        here.books[0].restart = Some(80.0);
        here.fold_in(&one_book_at(90.0, true, "A Book"));
        assert_eq!(here.books[0].percent, 0.0);
        assert!(!here.books[0].finished);
        assert_eq!(here.books[0].restart, Some(80.0));
    }

    /// `fold_in` adds whole a book `backup` names and `here` does not.
    #[test]
    fn a_book_only_the_backup_names_is_added() {
        let mut here = one_book_at(40.0, false, "A Book");
        let mut backup = Record::default();
        backup.books.push(from_witness(
            555,
            "Another",
            "Someone",
            "B0OTHER0001",
            IdentifiedBy::Library,
        ));
        here.fold_in(&backup);
        assert_eq!(here.books.len(), 2);
    }

    /// `slot_at` answers the **first** of a run of books under one extent,
    /// zero for most.
    #[test]
    fn a_run_of_books_under_one_extent_answers_its_first() {
        let mut record = Record::default();
        for (extent, key) in [
            (0, "zeta"),
            (0, "alpha"),
            (0, "mid"),
            (900, "nine"),
            (900, "another"),
            (1500, "one"),
        ] {
            record.books.push(from_witness(
                extent,
                key,
                "An Author",
                key,
                IdentifiedBy::Library,
            ));
        }
        record.sort_books();

        for extent in [0, 900, 1500] {
            let at = record.slot_at(extent).expect("a book under {extent}");
            assert_eq!(record.books[at].extent, extent);
            assert!(
                at == 0 || record.books[at - 1].extent < extent,
                "the first of the run, not one inside it",
            );
            let scanned = record.books.iter().position(|b| b.extent == extent);
            assert_eq!(Some(at), scanned, "the same slot a scan answered");
        }
        assert_eq!(record.slot_at(42), None, "an extent no book carries");
        assert_eq!(record.slot_at(2000), None, "past the end");
    }

    /// [`first_few`] names its entries once and holds to one line at any
    /// length.
    #[test]
    fn lost_covers_are_named_on_one_line_however_many() {
        let none: Vec<String> = Vec::new();
        assert_eq!(first_few(&none), "");

        let few: Vec<String> = ["A", "B"].iter().map(|s| s.to_string()).collect();
        assert_eq!(first_few(&few), "A, B");

        let five: Vec<String> = ["A", "B", "C", "D", "E"]
            .iter()
            .map(|s| s.to_string())
            .collect();
        assert_eq!(first_few(&five), "A, B, C, D, E", "five still fit");

        let twenty: Vec<String> = (0..20).map(|n| format!("book {n}")).collect();
        let said = first_few(&twenty);
        assert_eq!(said, "book 0, book 1, book 2, book 3, book 4, and 15 more");
        assert!(!said.contains('\n'), "one line: {said}");
    }

    fn sitting(started: &str, ended: &str, book: i64, secs: i64) -> Sitting {
        Sitting {
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
            ..Sitting::default()
        }
    }

    fn scratch(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("readinglog-record-{name}"));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("a scratch directory");
        dir
    }

    #[test]
    fn a_book_the_library_states_no_thumbnail_for_is_given_the_cached_one() {
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
        let identified = |extent: i64, key: &str, thumbnail: &str| Book {
            extent,
            cde_key: key.into(),
            title: key.into(),
            thumbnail: thumbnail.into(),
            ..Book::default()
        };
        let read =
            |extent: i64| sitting("2026-08-07T10:15:01", "2026-08-07T10:55:43", extent, 2_400);
        let mut record = Record {
            sittings: vec![read(148_207), read(148_301), read(148_402)],
            books: vec![
                // `thumbnail` names `stated`, and `cache` holds the key too.
                identified(148_207, "B00OKPCRLG", &stated.to_string_lossy()),
                // An empty `thumbnail`, under a key `cache` holds.
                identified(148_301, "B00RESCUED", ""),
                // A `thumbnail` naming no file, under a key `cache` lacks.
                identified(
                    148_402,
                    "B00SIDELOAD",
                    &dir.join("thumbnail_gone.jpg").to_string_lossy(),
                ),
            ],
            ..Record::default()
        };

        assert_eq!(record.keep_covers_from(&dir, &cache).kept, 2);
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
        let picture = cache.join("thumbnail_B0053VMNY2_EBOK_portrait.jpg");
        std::fs::write(&picture, b"GIF89a\x3c\x00\x28\x00\x80\x00\x00")
            .expect("a written thumbnail");
        // The same bytes under `covers::COVERS_DIR`.
        std::fs::create_dir_all(dir.join(covers::COVERS_DIR)).expect("the covers directory");
        std::fs::write(
            covers::path(&dir, "B0053VMNY2"),
            b"GIF89a\x3c\x00\x28\x00\x80\x00\x00",
        )
        .expect("a copied placeholder");
        let mut record = Record {
            sittings: vec![sitting(
                "2026-08-07T10:15:01",
                "2026-08-07T10:55:43",
                148_207,
                2_400,
            )],
            books: vec![Book {
                extent: 148_207,
                cde_key: "B0053VMNY2".into(),
                title: "The New Oxford American Dictionary".into(),
                thumbnail: picture.to_string_lossy().into_owned(),
                cover: covers::path(&dir, "B0053VMNY2")
                    .to_string_lossy()
                    .into_owned(),
                ..Book::default()
            }],
            ..Record::default()
        };

        assert_eq!(
            record.keep_covers_from(&dir, &cache).kept,
            1,
            "the book changed"
        );
        assert!(record.books[0].cover.is_empty(), "and names no cover");
        assert!(!covers::path(&dir, "B0053VMNY2").exists(), "the copy went");
        assert!(picture.is_file(), "the Kindle's own cache is left alone");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_cover_is_kept_only_for_a_book_shown_slots_answers() {
        let dir = scratch("covers");
        let picture = dir.join("thumbnail.jpg");
        std::fs::write(&picture, b"\xff\xd8\xff\xe0\x00\x10JFIF\0").expect("a written thumbnail");
        let identified = |extent: i64, key: &str| Book {
            extent,
            cde_key: key.into(),
            title: key.into(),
            thumbnail: picture.to_string_lossy().into_owned(),
            ..Book::default()
        };
        let mut record = Record {
            sittings: vec![sitting(
                "2026-08-07T10:15:01",
                "2026-08-07T10:55:43",
                148_207,
                2_400,
            )],
            books: vec![
                identified(148_207, "B00OKPCRLG"),
                identified(938_016, "B00NEVERRD"),
                // `kept`, which `shown_slots` answers with no sitting.
                Book {
                    kept: true,
                    ..identified(511_402, "B00CLEARED")
                },
            ],
            ..Record::default()
        };
        // `B00NEVERRD` holds a cover, and `B01.partial` sits beside it.
        covers::keep(&dir, "B00NEVERRD", &picture).expect("a copied cover");
        record.books[1].cover = covers::path(&dir, "B00NEVERRD")
            .to_string_lossy()
            .into_owned();
        std::fs::write(dir.join(covers::COVERS_DIR).join("B01.partial"), b"x").unwrap();

        assert_eq!(record.keep_covers(&dir).kept, 3, "two taken, one given up");
        assert!(covers::held(&dir, "B00OKPCRLG"), "the book a sitting names");
        assert!(covers::held(&dir, "B00CLEARED"), "the book `kept` flags");
        assert!(!covers::held(&dir, "B00NEVERRD"), "the book neither names");
        assert!(record.books[1].cover.is_empty(), "a cover no file backs");
        // `B01.partial` is not among them.
        let left = std::fs::read_dir(dir.join(covers::COVERS_DIR))
            .expect("the covers directory")
            .count();
        assert_eq!(left, 2, "one cover per slot `shown_slots` answers");

        // `keep_covers` over the same record takes nothing and drops nothing.
        assert_eq!(record.keep_covers(&dir).kept, 0);
        assert!(covers::held(&dir, "B00OKPCRLG"));
    }

    /// An `end_position` no `e` row raises lands on the [`IdentifiedBy::Sidecar`]
    /// book. `asin` reaches the [`IdentifiedBy::Library`] one.
    #[test]
    fn a_sidecar_book_gives_up_its_extent_to_the_key_the_library_holds() {
        let dir = scratch("shadowed-sidecar");
        let picture = dir.join("thumbnail_usuoi3.jpg");
        std::fs::write(&picture, b"\xff\xd8\xff\xe0\x00\x10JFIF\0").expect("a written thumbnail");
        let key = "OFMMGEJ7P4N5P4GMMECAHYOBBRN5MNAO";
        let mut record = Record {
            sittings: vec![Sitting {
                asin: Some(key.into()),
                ..sitting("2026-09-13T15:40:58", "2026-09-13T15:48:02", 166_929, 399)
            }],
            books: vec![
                Book {
                    extent: 166_929,
                    cde_key: "charles-dickens_a-christmas-carol".into(),
                    title: "charles-dickens_a-christmas-carol".into(),
                    identified_by: IdentifiedBy::Sidecar,
                    ..Book::default()
                },
                Book {
                    extent: 166_933,
                    cde_key: key.into(),
                    title: "A Christmas Carol".into(),
                    author: "Charles Dickens".into(),
                    thumbnail: picture.to_string_lossy().into_owned(),
                    identified_by: IdentifiedBy::Library,
                    ..Book::default()
                },
            ],
            ..Record::default()
        };
        let found = record
            .book_for(record.extent_of(166_929), Some(key))
            .expect("a book");
        assert_eq!(
            (found.title.as_str(), found.author.as_str()),
            ("A Christmas Carol", "Charles Dickens")
        );

        // `shown_slots` answers the slot, and `keep_covers` copies its cover.
        assert_eq!(record.keep_covers(&dir).kept, 1);
        assert!(covers::held(&dir, key));

        // `ends` raising the end position answers the same book on `extent` alone.
        record.ends = vec![(166_929, 166_933)];
        let raised = record
            .book_for(record.extent_of(166_929), None)
            .expect("a book");
        assert_eq!(raised.title, "A Christmas Carol");
    }

    /// A highlight of a known shape, for the `a` row below.
    fn one_highlight() -> Highlight {
        Highlight {
            extent: 148_207,
            title: "The Big Sleep".into(),
            kind: Kind::Highlight,
            at: "2026-09-07T09:41:55".into(),
            state: State::Live,
            start: 11_521,
            end: 11_601,
            location: 111,
            page: "xii".into(),
            colour: "orange".into(),
            body: "\u{201C}I was fired. For insubordination.\u{201D}".into(),
        }
    }

    #[test]
    fn an_a_row_states_a_highlight_and_reads_back_as_the_same_one() {
        let mut record = Record::default();
        let gate = crate::highlights::gate(
            Path::new("/nonexistent/My Clippings.txt"),
            &Default::default(),
        );
        assert!(record.take_highlights(vec![one_highlight()], gate));
        let back = Record::from_text(&record.text());
        assert_eq!(back.highlights, vec![one_highlight()]);
        assert_eq!(back.gate, Some(gate));
        // `start` and `location` stand on different axes, kept apart by the row.
        assert_eq!(back.highlights[0].start, 11_521);
        assert_eq!(back.highlights[0].location, 111);
        // `take_highlights` again with the same sources writes nothing new.
        assert!(!record.take_highlights(vec![one_highlight()], gate));
    }

    #[test]
    fn an_a_row_stating_a_deleted_highlight_is_dropped() {
        // `State::Retired` names a highlight the record draws no row for.
        let mut retired = one_highlight();
        retired.state = State::Retired;
        let held = Record {
            highlights: vec![one_highlight(), retired],
            ..Record::default()
        };
        let back = Record::from_text(&held.text());
        assert_eq!(back.highlights, vec![one_highlight()]);
    }

    #[test]
    fn a_row_naming_a_kind_this_build_does_not_know_costs_only_itself() {
        let text = format!(
            "{HEADER}\na\t1\tA Book\tscribble\t2026-09-07T09:41:55\tlive\t1\t2\t3\t\t\t\n{}\n",
            {
                let mut out = String::new();
                write_highlight(&mut out, &one_highlight());
                out.trim_end().to_string()
            }
        );
        let back = Record::from_text(&text);
        assert_eq!(back.highlights, vec![one_highlight()]);
    }

    #[test]
    fn taking_a_books_reading_takes_its_highlights_and_the_gate_with_them() {
        let mut record = with_unidentified();
        let extent = record.books[0].extent;
        let key = record.books[0].cde_key.clone();
        let mut mine = one_highlight();
        mine.extent = extent;
        record.take_highlights(
            vec![mine],
            crate::highlights::gate(
                Path::new("/nonexistent/My Clippings.txt"),
                &Default::default(),
            ),
        );
        record.clear_book(extent, &key);
        assert!(
            record.highlights.is_empty(),
            "the highlights go with the reading"
        );
        assert!(
            record.gate.is_none(),
            "and the next pass reads the sources again rather than standing on a count"
        );
    }

    /// `books` holds a book and `sittings` is empty: `keep_covers` answers
    /// 0 and leaves `covers::COVERS_DIR` standing.
    #[test]
    fn a_record_holding_books_and_no_sittings_keeps_every_cover() {
        let dir = scratch("short-covers");
        let picture = dir.join("thumbnail.jpg");
        std::fs::write(&picture, b"\xff\xd8\xff\xe0\x00\x10JFIF\0").expect("a written thumbnail");
        covers::keep(&dir, "B00OKPCRLG", &picture).expect("a copied cover");
        let mut record = Record {
            books: vec![Book {
                extent: 148_207,
                cde_key: "B00OKPCRLG".into(),
                title: "B00OKPCRLG".into(),
                thumbnail: picture.to_string_lossy().into_owned(),
                cover: covers::path(&dir, "B00OKPCRLG")
                    .to_string_lossy()
                    .into_owned(),
                ..Book::default()
            }],
            ..Record::default()
        };

        assert_eq!(record.keep_covers(&dir).kept, 0, "no book changed");
        assert!(covers::held(&dir, "B00OKPCRLG"), "the cover stands");
        assert!(!record.books[0].cover.is_empty(), "and the book names it");
    }

    #[test]
    fn a_record_round_trips_through_its_file() {
        let dir = scratch("roundtrip");
        let mut record = Record {
            sittings: vec![
                sitting("2026-08-07T10:15:01", "2026-08-07T10:55:43", 148_207, 2_400),
                sitting("2026-08-08T21:00:00", "2026-08-08T21:30:00", 938_016, 1_800),
            ],
            ends: vec![(938_016, 938_018)],
            keys: Vec::new(),
            counters: Vec::new(),
            pairs: Vec::new(),
            books: Vec::new(),
            watermark: "260808:213000".into(),
            watermark_offset: None,
            reset_at: String::new(),
            highlights: Vec::new(),
            gate: None,
            sources: None,
            cleared: vec![Cleared {
                extent: 304_517,
                key: "B00OKPCRLG".into(),
                at: "260808:120000".into(),
            }],
            clock: Clock::default(),
        };
        record.sittings[0].time_left = Some(41_400);
        record.sittings[1].asin = None;
        record.sittings[1].progress = None;
        record.sittings[1].measure = Measure::Paged;
        record.save(&dir).expect("a written record");

        assert_eq!(Record::load(&dir), record);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_pass_with_no_stamp_to_cut_at_leaves_every_sitting_standing() {
        // A stored `started_at` no `log_stamp` reads leaves `read_from` empty.
        let mut record = Record {
            sittings: vec![sitting("nonsense", "nonsense", 148_207, 2_400)],
            ..Record::default()
        };
        assert!(record.read_from().is_empty());
        record.absorb(&[], "");
        assert_eq!(record.sittings.len(), 1, "an empty cut emptied the record");
    }

    #[test]
    fn a_record_from_an_older_parse_keeps_every_row_it_holds() {
        let dir = scratch("stamped");
        let record = Record {
            sittings: vec![sitting(
                "2026-08-07T10:15:01",
                "2026-08-07T10:55:43",
                148_207,
                2_400,
            )],
            ends: vec![(938_016, 938_018)],
            keys: Vec::new(),
            counters: Vec::new(),
            pairs: Vec::new(),
            books: vec![Book {
                extent: 148_207,
                title: "A Book".into(),
                ..Book::default()
            }],
            watermark: "260808:213000".into(),
            watermark_offset: None,
            reset_at: String::new(),
            highlights: Vec::new(),
            gate: None,
            sources: None,
            cleared: vec![Cleared {
                extent: 148_207,
                key: "B00OKPCRLG".into(),
                at: "260808:120000".into(),
            }],
            clock: Clock::default(),
        };
        record.save(&dir).expect("a written record");
        let text = std::fs::read_to_string(Record::file(&dir)).expect("a record to stamp");
        std::fs::write(
            Record::file(&dir),
            text.replacen(HEADER, "#readinglog\t1", 1),
        )
        .expect("an older record");

        let read = Record::load(&dir);
        assert_eq!(read.sittings, record.sittings, "a load gave sittings up");
        assert_eq!(
            read.watermark, record.watermark,
            "a load gave the watermark up"
        );
        assert_eq!(read.books, record.books, "the names were not the parse's");
        assert_eq!(read.ends, record.ends);
        assert_eq!(
            read.cleared, record.cleared,
            "a cleared book would come back with the whole log"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// `record`, written to `dir` and then stamped with a header this build
    /// does not know.
    fn superseded(dir: &Path, record: &Record) {
        record.save(dir).expect("a written record");
        let text = std::fs::read_to_string(Record::file(dir)).expect("a record to stamp");
        std::fs::write(
            Record::file(dir),
            text.replacen(HEADER, "#readinglog\t1", 1),
        )
        .expect("an older record");
    }

    #[test]
    fn a_superseded_record_opens_whole_and_is_not_backed_up() {
        let dir = scratch("superseded");
        let record = two_books();
        superseded(&dir, &record);

        // `Record::open` under an unknown header takes nothing and writes nothing beside it.
        let read = Record::open(&dir);
        assert_eq!(read.sittings, record.sittings, "the sittings were given up");
        assert_eq!(read.books, record.books);
        assert!(
            crate::backup::list(&dir).is_empty(),
            "a backup nobody asked for"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_record_this_build_reads_whole_is_not_backed_up() {
        let dir = scratch("current");
        two_books().save(&dir).expect("a written record");
        assert_eq!(Record::open(&dir).sittings.len(), 3);
        assert!(crate::backup::list(&dir).is_empty(), "a backup for nothing");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_record_that_is_not_there_is_opened_as_an_empty_one() {
        let dir = scratch("open-missing");
        assert_eq!(Record::open(&dir), Record::default());
        assert!(crate::backup::list(&dir).is_empty());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_record_that_is_not_there_reads_as_empty() {
        let dir = scratch("missing");
        assert_eq!(Record::load(&dir.join("nothing")), Record::default());
        let _ = std::fs::remove_dir_all(&dir);
    }

    fn listed(extent: i64, key: &str, title: &str, percent: f64) -> library::Book {
        library::Book {
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
            on_kindle: true,
            read_state: -1,
        }
    }

    #[test]
    fn a_highlight_survives_the_file_and_every_pass_over_it() {
        let dir = scratch("finished");
        let mut record = Record::default();
        let library = [listed(938_018, "B00OKPCRLG", "Bible", 55.0)];
        record.remember(&library);
        assert!(!record.books[0].finished, "a library row states no mark");

        assert!(record.set_finished(938_018, "B00OKPCRLG", true));
        // `set_finished` with the same value is no change and no write.
        assert!(!record.set_finished(938_018, "B00OKPCRLG", true));
        // `remember` states nothing about the mark and takes nothing off.
        record.remember(&library);
        assert!(record.books[0].finished);

        record.save(&dir).expect("a written record");
        assert!(Record::load(&dir).books[0].finished);

        assert!(record.set_finished(938_018, "B00OKPCRLG", false));
        record.save(&dir).expect("a written record");
        assert!(!Record::load(&dir).books[0].finished);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_row_written_before_the_mark_reads_as_unmarked() {
        let dir = scratch("premark");
        std::fs::create_dir_all(&dir).expect("a directory to write in");
        // Ten fields: a `b` row without the `finished` field.
        let row = "b\t938018\tB00OKPCRLG\tBible\tBerlin\t\ten\t55.000000\t1\t\t/mnt/us/a.kfx";
        std::fs::write(Record::file(&dir), format!("{HEADER}\n{row}\n")).expect("a record");
        let record = Record::load(&dir);
        assert_eq!(record.books.len(), 1);
        assert!(!record.books[0].finished);
        assert_eq!(record.books[0].location, "/mnt/us/a.kfx");
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// [`listed`] carrying `read_state`.
    fn marked(percent: f64, read_state: i64) -> library::Book {
        library::Book {
            read_state,
            ..listed(938_018, "B00OKPCRLG", "Bible", percent)
        }
    }

    #[test]
    fn the_librarys_own_mark_carries_the_books_and_a_tap_holds_against_it() {
        let mut record = Record::default();
        record.remember(&[marked(40.0, -1)]);
        assert!(!record.books[0].finished, "a NULL column states nothing");

        // 1, which `library::read_state_says` reads as read.
        record.remember(&[marked(40.0, 1)]);
        assert!(record.books[0].finished);

        // `set_finished` disagrees, and an unchanged `read_state` leaves it standing.
        assert!(record.set_finished(938_018, "B00OKPCRLG", false));
        record.remember(&[marked(40.0, 1)]);
        record.remember(&[marked(40.0, 1)]);
        assert!(
            !record.books[0].finished,
            "an unchanged column overrode a tap"
        );

        // 3 states unread, 4 read.
        record.remember(&[marked(40.0, 3)]);
        assert!(!record.books[0].finished);
        record.remember(&[marked(40.0, 4)]);
        assert!(record.books[0].finished);

        // 0 states neither way and takes nothing off.
        record.remember(&[marked(40.0, 0)]);
        assert!(record.books[0].finished);
    }

    #[test]
    fn a_mark_handed_to_the_library_reads_back_as_no_change() {
        let dir = scratch("notemark");
        let mut record = Record::default();
        record.remember(&[marked(40.0, -1)]);

        // `set_finished` and `note_mark`, the pair one tap makes.
        assert!(record.set_finished(938_018, "B00OKPCRLG", true));
        record.note_mark(938_018, "B00OKPCRLG", crate::library::read_state_for(true));
        assert_eq!(record.books[0].read_state, 1);
        assert!(record.books[0].finished);

        // `remember` states the same value back.
        record.remember(&[marked(40.0, 1)]);
        assert!(record.books[0].finished);

        // `cde_type` reaches the record and the row.
        assert_eq!(record.books[0].cde_type, "EBOK");
        record.save(&dir).expect("a written record");
        let back = Record::load(&dir);
        assert_eq!(back.books[0].cde_type, "EBOK");
        assert_eq!(back.books[0].read_state, 1);

        // `note_mark` under a key no book carries marks nothing.
        record.note_mark(0, "NOSUCHKEY", 3);
        assert_eq!(record.books[0].read_state, 1);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn progress_read_through_outranks_the_librarys_mark() {
        let mut record = Record::default();
        record.remember(&[marked(100.0, 3)]);
        assert!(
            record.books[0].finished,
            "the progress states this one read through"
        );

        // `restart` clears `finished`, with `read_state` unchanged through it.
        assert!(record.restart(938_018, "B00OKPCRLG", 0));
        assert!(!record.books[0].finished);
        record.remember(&[marked(100.0, 3)]);
        assert_eq!(record.books[0].percent, 0.0);
        assert!(!record.books[0].finished);
    }

    #[test]
    fn the_librarys_mark_round_trips_and_an_older_row_takes_the_next_one() {
        let dir = scratch("readstate");
        let mut record = Record::default();
        record.remember(&[marked(40.0, 1)]);
        record.save(&dir).expect("a written record");
        let back = Record::load(&dir);
        assert_eq!(back.books[0].read_state, 1);
        assert!(back.books[0].finished);

        // Twelve fields: a `b` row without `read_state`, -1 against what `remember` takes.
        std::fs::create_dir_all(&dir).expect("a directory to write in");
        let row = "b\t938018\tB00OKPCRLG\tBible\tBerlin\t\ten\t40.000000\t1\t\t/mnt/us/a.kfx\t0\t";
        std::fs::write(Record::file(&dir), format!("{HEADER}\n{row}\n")).expect("a record");
        let mut record = Record::load(&dir);
        assert_eq!(record.books[0].read_state, -1);
        record.remember(&[marked(40.0, 2)]);
        assert!(record.books[0].finished, "READ_AUTOMATIC reached no book");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn progress_read_through_marks_its_own_book() {
        // `set_finished` is not called: `remember` marks this one.
        let mut record = Record::default();
        record.remember(&[listed(938_018, "B00OKPCRLG", "Bible", 99.8)]);
        assert!(record.books[0].finished);

        // `finished` holds when `percent` turns back.
        record.remember(&[listed(938_018, "B00OKPCRLG", "Bible", 71.0)]);
        assert_eq!(record.books[0].percent, 71.0);
        assert!(record.books[0].finished);

        // A `b` row carrying `finished` at 0 reads back marked.
        let dir = scratch("through");
        std::fs::create_dir_all(&dir).expect("a directory to write in");
        let row = "b\t938018\tB00OKPCRLG\tBible\tBerlin\t\ten\t100.000000\t1\t\t/mnt/us/a.kfx\t0";
        std::fs::write(Record::file(&dir), format!("{HEADER}\n{row}\n")).expect("a record");
        assert!(Record::load(&dir).books[0].finished);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_restart_gives_up_the_progress_and_waits_for_one_before_it() {
        let dir = scratch("restart");
        let mut record = Record::default();
        let library = [listed(938_018, "B00OKPCRLG", "Bible", 100.0)];
        record.remember(&library);
        assert!(record.books[0].finished);

        assert!(record.restart(938_018, "B00OKPCRLG", 0));
        assert_eq!(record.books[0].percent, 0.0);
        assert_eq!(record.books[0].restart, Some(100.0));
        assert!(!record.books[0].finished);

        // `remember` at or past `restart` leaves `percent` at 0.
        record.remember(&library);
        assert_eq!(record.books[0].percent, 0.0);
        assert!(!record.books[0].finished);
        record.save(&dir).expect("a written record");
        assert_eq!(Record::load(&dir).books[0].restart, Some(100.0));

        // A `percent` under `restart` clears it, and `remember` takes over.
        record.remember(&[listed(938_018, "B00OKPCRLG", "Bible", 4.0)]);
        assert_eq!(
            (record.books[0].percent, record.books[0].restart),
            (4.0, None)
        );
        record.remember(&[listed(938_018, "B00OKPCRLG", "Bible", 100.0)]);
        assert_eq!(record.books[0].percent, 100.0);
        assert!(
            record.books[0].finished,
            "and the second pass marks it again"
        );

        // `restart` gives up nothing on no book, nor on one at its beginning.
        assert!(!record.restart(0, "NOSUCHKEY", 0));
        record.remember(&[listed(555, "B00OTHER", "Another", 0.0)]);
        assert!(!record.restart(555, "B00OTHER", 0));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_book_round_trips_through_the_file() {
        let dir = scratch("books");
        let mut record = Record::default();
        record.remember(&[listed(
            938_018,
            "B00OKPCRLG",
            "The Jewish\tStudy Bible",
            76.125,
        )]);
        record.save(&dir).expect("a written record");

        let back = Record::load(&dir);
        assert_eq!(back, record);
        let book = &back.books[0];
        // `title` written with a tab reads back as no field break.
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
    fn a_book_gone_from_the_kindle_keeps_the_file_it_was_read_from() {
        let mut record = Record::default();
        record.remember(&[listed(1, "B01", "A Book", 10.0)]);
        // `library`: the row a deletion leaves behind, stating no location.
        let mut library = listed(0, "B01", "A Book", -1.0);
        library.location = String::new();
        library.on_kindle = false;
        record.remember(&[library]);
        let book = &record.books[0];
        assert!(!book.on_kindle, "the Kindle holds it");
        assert_eq!(book.location, "/mnt/us/documents/A Book.kfx");
    }

    #[test]
    fn the_library_refreshes_a_book_and_never_removes_one() {
        let mut record = Record::default();
        assert_eq!(record.remember(&[listed(1, "B01", "A Book", 10.0)]), 1);
        // `remember` at the same reading again changes nothing.
        assert_eq!(record.remember(&[listed(1, "B01", "A Book", 10.0)]), 0);
        // Read further, and the library's new figure is taken.
        assert_eq!(record.remember(&[listed(1, "B01", "A Book", 40.0)]), 1);
        assert_eq!(record.books[0].percent, 40.0);

        // `remember` listing none leaves the book standing, flagged for where it sits.
        assert_eq!(record.remember(&[]), 1);
        assert_eq!(record.books.len(), 1);
        assert!(!record.books[0].on_kindle);
        assert_eq!(record.books[0].title, "A Book");
        assert_eq!(record.books[0].percent, 40.0);
        // `remember` empty again has nothing left to change.
        assert_eq!(record.remember(&[]), 0);
    }

    /// A record read to 88%, then to `progress`, then with its library row
    /// deleted.
    fn read_on_to(progress: Option<f64>) -> Record {
        let mut record = Record {
            sittings: vec![sitting(
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
            watermark: String::new(),
            watermark_offset: None,
            reset_at: String::new(),
            cleared: Vec::new(),
            highlights: Vec::new(),
            gate: None,
            sources: None,
            clock: Clock::default(),
        };
        record.sittings[0].progress = progress;
        record.remember(&[listed(938_018, "B00OKPCRLG", "A Book", 88.0)]);
        assert_eq!(record.books[0].percent, 88.0);
        let mut cloud = listed(938_018, "B00OKPCRLG", "A Book", -1.0);
        cloud.on_kindle = false;
        record.remember(&[cloud]);
        record
    }

    #[test]
    fn a_book_takes_the_progress_its_newest_sitting_states() {
        // `%Left` of 0 on the last turn, past the 88% the library stated.
        assert_eq!(read_on_to(Some(1.0)).books[0].percent, 100.0);
        // `%Left` of 0.6 on the last turn, short of it: 88 stands.
        assert_eq!(read_on_to(Some(0.4)).books[0].percent, 88.0);
    }

    #[test]
    fn a_deleted_book_keeps_the_progress_the_library_stated_for_it() {
        // The shape the two disagree in: `p_percentFinished` reads 100 and the
        // last turn's `%Left` is a page short of it.
        let mut record = Record {
            sittings: vec![sitting(
                "2026-08-27T10:34:40",
                "2026-08-27T11:03:20",
                938_018,
                1_720,
            )],
            ..Record::default()
        };
        record.sittings[0].progress = Some(0.994_69);
        record.remember(&[listed(938_018, "B00OKPCRLG", "A Book", 100.0)]);
        assert_eq!(record.books[0].percent, 100.0);
        assert!(record.books[0].finished);

        // `cloud`: the downloaded row deleted, and `p_percentFinished` with it.
        let mut cloud = listed(938_018, "B00OKPCRLG", "A Book", -1.0);
        cloud.on_kindle = false;
        record.remember(&[cloud.clone()]);
        assert_eq!(
            record.books[0].percent, 100.0,
            "the progress was walked back"
        );
        // `remember` again leaves it where it stands.
        record.remember(&[cloud]);
        assert_eq!(record.books[0].percent, 100.0);
    }

    #[test]
    fn a_book_the_library_states_no_progress_for_takes_the_progress_its_sittings_state() {
        let mut record = Record {
            sittings: vec![sitting(
                "2026-08-27T10:34:40",
                "2026-08-27T11:03:20",
                938_018,
                1_720,
            )],
            ..Record::default()
        };
        record.sittings[0].progress = Some(0.4);
        record.remember(&[listed(938_018, "B00OKPCRLG", "A Book", -1.0)]);
        assert_eq!(record.books[0].percent, 40.0);
    }

    #[test]
    fn a_re_download_hands_the_progress_back_to_the_library() {
        let mut record = read_on_to(Some(1.0));
        record.remember(&[listed(938_018, "B00OKPCRLG", "A Book", 99.5)]);
        assert_eq!(record.books[0].percent, 99.5);
    }

    #[test]
    fn a_sitting_stating_no_progress_leaves_the_record_alone() {
        assert_eq!(read_on_to(None).books[0].percent, 88.0);
    }

    #[test]
    fn a_library_row_names_a_book_without_unseating_its_extent() {
        let mut record = Record::default();
        record.remember(&[listed(
            938_018,
            "B00OKPCRLG",
            "The Jewish Study Bible",
            76.0,
        )]);
        // The same book with no extent and no progress, a title only.
        let mut cloud = listed(0, "B00OKPCRLG", "The Jewish Study Bible", -1.0);
        cloud.on_kindle = false;
        record.remember(&[cloud]);

        assert_eq!(record.books.len(), 1);
        let book = &record.books[0];
        assert_eq!(book.extent, 938_018, "the log keys sittings by this");
        assert_eq!(book.percent, 76.0);
        assert!(!book.on_kindle);
    }

    #[test]
    fn a_book_is_reached_by_its_key_when_the_extent_misses() {
        let mut record = Record::default();
        record.remember(&[listed(
            938_018,
            "B00OKPCRLG",
            "The Jewish Study Bible",
            76.0,
        )]);
        assert_eq!(
            record.book_for(938_018, None).map(|b| &b.title[..]),
            Some("The Jewish Study Bible")
        );
        // `book_for` a position no `BookEndPosition` line mapped reaches the book by key.
        assert_eq!(
            record.book_for(4_242, Some("B00OKPCRLG")).map(|b| b.extent),
            Some(938_018)
        );
        assert!(record.book_for(4_242, Some("")).is_none());
        assert!(record.book_for(4_242, None).is_none());
    }

    /// A record holding one sitting, with the log read as far as `watermark`.
    fn one_sitting(watermark: &str) -> Record {
        Record {
            sittings: vec![sitting(
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
            watermark: watermark.into(),
            watermark_offset: None,
            reset_at: String::new(),
            cleared: Vec::new(),
            highlights: Vec::new(),
            gate: None,
            sources: None,
            clock: Clock::default(),
        }
    }

    /// The instant a pass ran at.
    const NOW: i64 = 1_788_951_701;

    #[test]
    fn the_clocks_the_kindle_stood_on_round_trip_through_the_file() {
        let dir = scratch("clocks");
        let mut record = on_clock("260911:115340", Some(7200));
        // A fortnight four hours east, between two spans at home.
        record.clock.observe(vec![
            (1_786_000_000, 7200),
            (1_787_600_000, 21_600),
            (1_788_900_000, 7200),
        ]);
        record.save(&dir).expect("a record");
        let read = Record::load(&dir);
        assert_eq!(read.clock, record.clock);
        assert_eq!(read.clock.offsets(), vec![7200, 21_600]);
        assert_eq!(read.clock.offset_at(1_787_700_000), Some(21_600));
        // `older`: a record carrying no `z` row.
        let older = Record::from_text(
            "#readinglog
m	260911:115340	7200
",
        );
        assert!(older.clock.is_empty());
    }

    /// A record at `watermark` under `offset`, its `reset_at` the same
    /// wall-clock stamp.
    fn on_clock(watermark: &str, offset: Option<i64>) -> Record {
        Record {
            watermark: watermark.into(),
            watermark_offset: offset,
            reset_at: watermark.into(),
            ..Record::default()
        }
    }

    #[test]
    fn a_clock_stepping_back_pulls_the_watermark_and_the_reset_stamp_with_it() {
        // Berlin +2 to +1, 02:00 to 03:00 written twice: `watermark` bars the second.
        let mut record = on_clock("261025:023000", Some(7200));
        assert_eq!(record.follow_clock(NOW, Some(3600)), 3600);
        assert_eq!(record.watermark, "261025:013000");
        assert_eq!(record.reset_at, "261025:013000");
        assert_eq!(record.watermark_offset, Some(3600));

        // +8 home to +1, across a day boundary.
        let mut far = on_clock("261102:043000", Some(8 * 3600));
        assert_eq!(far.follow_clock(NOW, Some(3600)), 7 * 3600);
        assert_eq!(far.watermark, "261101:213000");
    }

    #[test]
    fn a_clock_that_has_not_stepped_back_moves_nothing() {
        // Forward: `watermark` falls further behind, skipping no line.
        let mut ahead = on_clock("260909:130922", Some(7200));
        assert_eq!(ahead.follow_clock(NOW, Some(8 * 3600)), 0);
        assert_eq!(ahead.watermark, "260909:130922");
        assert_eq!(ahead.watermark_offset, Some(8 * 3600));

        // The same offset, and a step under `CLOCK_STEP_SECS`, change no zone.
        let mut same = on_clock("260909:130922", Some(7200));
        assert_eq!(same.follow_clock(NOW, Some(7200)), 0);
        assert_eq!(same.follow_clock(NOW, Some(7170)), 0);
        assert_eq!(same.watermark, "260909:130922");
    }

    #[test]
    fn a_record_with_no_offset_takes_one_and_stands_still() {
        // Nothing to compare against.
        let mut fresh = on_clock("260909:130922", None);
        assert_eq!(fresh.follow_clock(NOW, Some(7260)), 0);
        assert_eq!(fresh.watermark, "260909:130922");
        assert_eq!(fresh.watermark_offset, Some(7260));

        // `follow_clock` with no offset leaves the record as it stands.
        let mut blind = on_clock("260909:130922", Some(7200));
        assert_eq!(blind.follow_clock(NOW, None), 0);
        assert_eq!(blind.watermark_offset, Some(7200));
    }

    #[test]
    fn the_watermark_carries_the_clock_it_was_written_under() {
        let record = on_clock("260909:130922", Some(7260));
        assert!(record.text().contains("m\t260909:130922\t7260\n"));
        let read = Record::parse(&record.text());
        assert_eq!(read.watermark_offset, Some(7260));
        // `m` with no offset: `watermark_offset` unset, `watermark` standing.
        let old = Record::parse("#readinglog\t2\nm\t260909:130922\n");
        assert_eq!(old.watermark, "260909:130922");
        assert_eq!(old.watermark_offset, None);
    }

    #[test]
    fn the_pass_rewinds_to_a_sitting_that_could_still_be_open() {
        // `watermark` four minutes past the sitting's end.
        assert_eq!(one_sitting("260807:105943").read_from(), "260807:101501");
    }

    #[test]
    fn the_pass_does_not_rewind_to_a_sitting_the_log_has_closed() {
        // A whole day past the sitting's end.
        assert_eq!(one_sitting("260808:213000").read_from(), "260808:213000");
        // 10:55:43 ending: 29 minutes on is open, 30 closed.
        assert_eq!(one_sitting("260807:112443").read_from(), "260807:101501");
        assert_eq!(one_sitting("260807:112543").read_from(), "260807:112543");
    }

    #[test]
    fn a_record_holding_no_sitting_reads_the_whole_log() {
        let bare = Record {
            watermark: "260808:213000".into(),
            watermark_offset: None,
            ..Record::default()
        };
        assert_eq!(bare.read_from(), "");
        assert_eq!(Record::default().read_from(), "");
        // `reset_at` bounds the pass.
        let was_reset = Record {
            watermark: "260808:213000".into(),
            watermark_offset: None,
            reset_at: "260808:213000".into(),
            ..Record::default()
        };
        assert_eq!(was_reset.read_from(), "260808:213000");
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
            covered: 0.0,
        }
    }

    /// A record holding one book's reading, and nothing that names it: the
    /// state a first parse is in when the book was deleted before it ran.
    fn read_but_unidentified() -> Record {
        let mut record = Record::default();
        record.absorb(
            &[
                turn("260807:101501", 148_207, 7_390_020, 49_583),
                turn("260807:101543", 148_207, 7_431_463, 49_712),
            ],
            "",
        );
        assert!(record.book_for(148_207, None).is_none());
        record
    }

    #[test]
    fn a_sidecar_names_the_reading_of_a_book_the_library_has_forgotten() {
        let mut record = read_but_unidentified();
        // `timer.model` holds the highest counter the log showed.
        assert_eq!(
            record.recover(&[card("a deleted book", 7_431_463, 49_712)]),
            1
        );
        let book = record.book_for(148_207, None).expect("the book identified");
        assert_eq!(book.title, "a deleted book");
        assert_eq!(book.extent, 148_207);
        assert!(!book.on_kindle);
        assert!(book.is_book(), "it draws a row");
        // `percent` off the sitting, the library stating none.
        assert!((book.percent - 35.5).abs() < 0.1, "{}", book.percent);
        assert_eq!(record.pairs, vec![(148_207, "a deleted book".to_string())]);
    }

    #[test]
    fn a_counter_short_of_the_last_one_logged_names_nothing() {
        let mut record = read_but_unidentified();
        // A counter below the end position's highest.
        assert_eq!(record.recover(&[card("some book", 7_390_020, 49_583)]), 0);
        assert!(record.book_for(148_207, None).is_none());
        assert!(record.pairs.is_empty());
    }

    #[test]
    fn a_book_opened_and_never_read_names_nothing() {
        let mut record = read_but_unidentified();
        // `card` at zero, the counter every untimed end position shares, names none.
        assert_eq!(record.recover(&[card("never read", 0, 0)]), 0);
        assert!(record.pairs.is_empty());
    }

    #[test]
    fn an_end_position_two_sidecars_both_claim_is_left_alone() {
        let mut record = read_but_unidentified();
        let identified = record.recover(&[
            card("one book", 7_431_463, 49_712),
            card("another book", 7_431_463, 49_712),
        ]);
        assert_eq!(identified, 0, "one candidate or nothing, both ways");
        assert!(record.pairs.is_empty());
    }

    #[test]
    fn a_book_the_library_still_names_is_never_inferred_over() {
        let mut record = read_but_unidentified();
        record.remember(&[listed(148_207, "B01", "The Library's Own Title", 40.0)]);
        assert_eq!(
            record.recover(&[card("a deleted book", 7_431_463, 49_712)]),
            0
        );
        let book = record.book_for(148_207, None).expect("the library's book");
        assert_eq!(book.title, "The Library's Own Title");
        assert!(record.pairs.is_empty());
    }

    #[test]
    fn a_library_row_arriving_later_takes_the_book_back() {
        let mut record = read_but_unidentified();
        assert_eq!(
            record.recover(&[card("a deleted book", 7_431_463, 49_712)]),
            1
        );
        // `remember` states the book again.
        record.remember(&[listed(148_207, "B01", "The Library's Own Title", 40.0)]);
        record.recover(&[card("a deleted book", 7_431_463, 49_712)]);
        assert!(record.pairs.is_empty(), "the inference stands down");
        assert_eq!(
            record.book_for(148_207, None).expect("a book").title,
            "The Library's Own Title"
        );
    }

    #[test]
    fn the_counter_and_the_pairing_both_survive_the_file() {
        let dir = scratch("sidecar-rows");
        let mut record = read_but_unidentified();
        record.recover(&[card("a deleted book", 7_431_463, 49_712)]);
        record.save(&dir).expect("a written record");
        let back = Record::load(&dir);
        assert_eq!(back, record);
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
    fn a_mobi8_sidecar_names_a_mobi8_end_position() {
        let mut record = Record::default();
        record.absorb(
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
        assert_eq!(record.extent_of(1_543_288), 1_543_289);
        assert!(record.book_for(1_543_289, None).is_none());
        // The counters an `.azw3f` states, in `timer.model`'s own order.
        assert_eq!(record.recover(&[card("lovecraft", 1_003_773, 1_834)]), 1);
        let book = record
            .book_for(1_543_289, None)
            .expect("the book identified");
        assert_eq!(book.title, "lovecraft");
        assert_eq!(book.extent, 1_543_289);
    }

    #[test]
    fn a_sidecar_named_for_a_key_reaches_the_row_holding_that_key() {
        let mut record = read_but_unidentified();
        // The row a deletion leaves: a key and a title, no location, no extent.
        let mut cloud = listed(0, "B00OKPCRLG", "The Jewish Study Bible", -1.0);
        cloud.location = String::new();
        cloud.on_kindle = false;
        record.remember(&[cloud]);
        let identified =
            record.recover(&[card("The Jewish Study Bible_B00OKPCRLG", 7_431_463, 49_712)]);
        assert_eq!(identified, 1);
        let book = record.book_for(148_207, None).expect("the book identified");
        assert_eq!(book.title, "The Jewish Study Bible");
        assert_eq!(book.author, "Adele Berlin");
        assert!(book.thumbnail.ends_with("t.jpg"), "the cover comes with it");
        assert_eq!(record.books.len(), 1, "one book, not two");
    }

    #[test]
    fn a_name_carrying_no_key_is_titled_by_the_file() {
        let mut record = read_but_unidentified();
        let mut cloud = listed(0, "B00OKPCRLG", "The Jewish Study Bible", -1.0);
        cloud.location = String::new();
        record.remember(&[cloud]);
        record.recover(&[card("some sideload", 7_431_463, 49_712)]);
        let book = record.book_for(148_207, None).expect("the book identified");
        assert_eq!(book.title, "some sideload");
    }

    #[test]
    fn a_book_already_holding_an_extent_is_never_taken_by_a_name() {
        let mut record = read_but_unidentified();
        record.remember(&[listed(999_999, "B00OKPCRLG", "Another Book", 10.0)]);
        record.recover(&[card("Another Book_B00OKPCRLG", 7_431_463, 49_712)]);
        let other = record
            .books
            .iter()
            .find(|b| b.cde_key == "B00OKPCRLG")
            .expect("the live book");
        assert_eq!(other.extent, 999_999, "its own extent stands");
        assert_eq!(
            record
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
        let mut record = Record::default();
        let (added, extended) = record.absorb(&lines, "");
        assert_eq!((added, extended), (1, 0));
        assert_eq!(record.sittings.len(), 1);
        assert_eq!(record.sittings[0].seconds, 41);
        assert_eq!(record.watermark, "260807:101543");

        let again = record.clone();
        let from = record.read_from();
        let (added, extended) = record.absorb(&lines, &from);
        assert_eq!((added, extended), (0, 1), "the same sitting, re-measured");
        assert_eq!(record, again);
    }

    #[test]
    fn a_sitting_still_open_grows_instead_of_splitting() {
        let first = vec![
            page("260807:101501", 7_390_020),
            page("260807:101543", 7_431_463),
        ];
        let mut record = Record::default();
        record.absorb(&first, "");
        assert_eq!(record.sittings[0].seconds, 41);
        assert_eq!(record.sittings[0].ended_at, "2026-08-07T10:15:43");

        // `read_from` re-reads from the sitting's own start, two more turns of it.
        let from = record.read_from();
        let mut second = first.clone();
        second.push(page("260807:101625", 7_473_000));
        second.push(page("260807:101710", 7_518_000));
        let (added, extended) = record.absorb(&second, &from);

        assert_eq!((added, extended), (0, 1));
        assert_eq!(record.sittings.len(), 1, "one sitting, not two");
        assert_eq!(record.sittings[0].started_at, "2026-08-07T10:15:01");
        assert_eq!(record.sittings[0].ended_at, "2026-08-07T10:17:10");
        assert_eq!(record.sittings[0].seconds, 127);
    }

    #[test]
    fn a_sitting_older_than_the_pass_is_left_alone() {
        let mut record = Record {
            sittings: vec![sitting(
                "2026-08-01T09:00:00",
                "2026-08-01T09:30:00",
                999,
                1_800,
            )],
            ..Record::default()
        };
        let lines = vec![
            page("260807:101501", 7_390_020),
            page("260807:101543", 7_431_463),
        ];
        let (added, _) = record.absorb(&lines, "260807:000000");
        assert_eq!(added, 1);
        assert_eq!(record.sittings.len(), 2);
        assert_eq!(record.sittings[0].end_position, 999, "the old one survives");
    }

    /// Two books' reading, with a book for each and the log read to `watermark`.
    fn two_books() -> Record {
        let mut record = Record {
            sittings: vec![
                sitting("2026-08-07T10:15:01", "2026-08-07T10:55:43", 148_207, 2_400),
                sitting("2026-08-08T09:00:00", "2026-08-08T09:30:00", 148_207, 1_800),
                sitting("2026-08-09T21:00:00", "2026-08-09T21:30:00", 938_018, 1_800),
            ],
            books: vec![
                Book {
                    extent: 148_207,
                    cde_key: "B00OKPCRLG".into(),
                    title: "A Book".into(),
                    finished: true,
                    restart: Some(62.0),
                    ..Book::default()
                },
                Book {
                    extent: 938_018,
                    cde_key: "B00SECOND".into(),
                    title: "Another".into(),
                    ..Book::default()
                },
            ],
            watermark: "260810:120000".into(),
            watermark_offset: None,
            ..Record::default()
        };
        for s in &mut record.sittings {
            s.asin = None;
        }
        record.sort();
        record
    }

    #[test]
    fn clearing_a_book_takes_its_sittings_and_keeps_the_book() {
        let mut record = two_books();
        assert_eq!(record.clear_book(148_207, "B00OKPCRLG"), 2);
        assert_eq!(record.sittings.len(), 1, "the other book reads on");
        assert_eq!(record.sittings[0].end_position, 938_018);
        assert_eq!(record.books.len(), 2, "the book stands");
        let held = &record.books[0];
        assert_eq!(held.title, "A Book", "the book stands, and names itself");
        assert!(held.kept, "and stands on the lists at zero");
        // `restart` took `finished` and `percent` with the sittings.
        assert!(!held.finished);
        assert_eq!(held.percent, 0.0);
        assert_eq!(held.restart, Some(62.0), "the progress it gave up");
        assert_eq!(
            record.cleared,
            vec![Cleared {
                extent: 148_207,
                key: "B00OKPCRLG".into(),
                at: "260810:120000".into(),
            }]
        );
    }

    #[test]
    fn forgetting_a_book_takes_the_book_with_the_reading() {
        let mut record = two_books();
        assert_eq!(record.forget_book(148_207, "B00OKPCRLG"), 2);
        assert_eq!(record.sittings.len(), 1);
        assert_eq!(record.books.len(), 1, "the book went too");
        assert_eq!(record.books[0].extent, 938_018);
        assert_eq!(record.cleared.len(), 1, "and is still held back");
        assert_eq!(record.cleared[0].extent, 148_207);
    }

    #[test]
    fn clearing_a_book_the_record_does_not_hold_does_nothing() {
        let mut record = two_books();
        let before = record.clone();
        assert_eq!(record.clear_book(1, "B00NOTHERE"), 0);
        assert_eq!(record, before);
    }

    #[test]
    fn a_cleared_book_is_not_re_derived_by_the_whole_log() {
        let mut record = two_books();
        record.clear_book(148_207, "B00OKPCRLG");
        // Every line the book was measured from.
        let lines = vec![
            page("260807:101501", 7_390_020),
            page("260807:101543", 7_431_463),
        ];
        record.absorb(&lines, "");
        assert!(
            record.sittings.iter().all(|s| s.end_position != 148_207),
            "the log handed the book back"
        );
    }

    #[test]
    fn a_reset_empties_the_record_and_floors_the_pass() {
        let mut record = two_books();
        record.cleared.push(Cleared {
            extent: 555,
            key: "B00GONE".into(),
            at: "260809:090000".into(),
        });
        assert!(record.reset());
        assert!(record.sittings.is_empty());
        assert!(record.books.is_empty());
        assert!(record.ends.is_empty());
        assert_eq!(
            record.watermark, "260810:120000",
            "the pass knows where it read"
        );
        assert_eq!(record.reset_at, "260810:120000");
        assert_eq!(record.cleared.len(), 1, "a book taken off stays off");
        assert_eq!(record.read_from(), "260810:120000");
    }

    #[test]
    fn a_record_that_has_read_nothing_has_nothing_to_reset() {
        let mut record = Record::default();
        assert!(!record.reset());
        assert_eq!(record, Record::default());
    }

    #[test]
    fn a_stale_header_takes_neither_the_watermark_nor_the_reset_stamp() {
        let dir = scratch("stale-header");
        let mut record = two_books();
        record.reset();
        superseded(&dir, &record);

        let read = Record::load(&dir);
        assert_eq!(
            read.watermark, record.watermark,
            "the stamp took the watermark"
        );
        assert_eq!(
            read.reset_at, "260810:120000",
            "the stamp took the reset stamp"
        );
        assert_eq!(read.read_from(), "260810:120000");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_pass_after_a_reset_folds_nothing_from_before_it() {
        let mut record = Record::default();
        let lines = vec![
            page("260807:101501", 7_390_020),
            page("260807:101543", 7_431_463),
        ];
        record.absorb(&lines, "");
        assert_eq!(record.sittings.len(), 1);
        record.reset();

        let from = record.read_from();
        record.absorb(&lines, &from);
        assert!(record.sittings.is_empty(), "the log handed the era back");
    }

    #[test]
    fn a_reset_record_holds_its_stamps_and_nothing_else() {
        let dir = scratch("reset");
        let mut record = two_books();
        record.cleared.push(Cleared {
            extent: 555,
            key: "B00GONE".into(),
            at: "260809:090000".into(),
        });
        record.reset();
        record.save(&dir).expect("a written record");

        let text = std::fs::read_to_string(Record::file(&dir)).expect("the record");
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

        let mut record = Record {
            // Older than the lines `live` holds.
            sittings: vec![sitting(
                "2026-07-01T09:00:00",
                "2026-07-01T09:30:00",
                999,
                1_800,
            )],
            watermark: "260807:101543".into(),
            watermark_offset: None,
            reset_at: "260807:120000".into(),
            ..Record::default()
        };
        let added =
            record.rebuild_from(&live, &dir.join("none"), &dir.join("none"), &mut |_, _| {});

        assert_eq!(added, 1, "the log's own sitting did not come back");
        assert_eq!(record.sittings.len(), 2, "the older sitting was lost");
        assert!(record.sittings.iter().any(|s| s.end_position == 999));
        assert!(record.reset_at.is_empty(), "the reset stamp stayed up");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_sitting_carrying_its_own_counter_reaches_its_sidecar_with_no_log() {
        // One sitting holding the counter it was last seen at, with no `t`
        // row beside it.
        let mut record = Record {
            sittings: vec![Sitting {
                end_counter_ms: Some(900_000),
                end_words: Some(4_100),
                ..sitting("2026-08-07T10:15:01", "2026-08-07T10:20:00", 148_207, 299)
            }],
            ..Record::default()
        };
        assert!(record.counters.is_empty());
        assert!(!record.wants_the_logs(), "the logs hold nothing this needs");

        assert_eq!(record.recover(&[card("Vom Kriege", 900_000, 4_100)]), 1);
        assert_eq!(record.books.len(), 1);
        // `card` at a counter no sitting states names nothing.
        assert_eq!(record.recover(&[card("Anderes Buch", 1, 2)]), 0);
    }

    #[test]
    fn a_sitting_written_before_the_counters_were_held_still_wants_the_logs() {
        let record = Record {
            sittings: vec![sitting(
                "2026-08-07T10:15:01",
                "2026-08-07T10:20:00",
                148_207,
                299,
            )],
            ..Record::default()
        };
        assert!(record.wants_the_logs(), "an old row was taken as complete");
    }

    #[test]
    fn reading_the_end_positions_again_reaches_a_sitting_recorded_without_one() {
        let dir = scratch("end-positions");
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

        // One sitting, no counter for a sidecar to match, and a watermark past
        // everything the log holds.
        let mut record = Record {
            sittings: vec![sitting(
                "2026-08-07T10:15:01",
                "2026-08-07T10:20:00",
                148_207,
                299,
            )],
            watermark: "260807:101543".into(),
            watermark_offset: None,
            ..Record::default()
        };
        assert!(record.counters.is_empty());
        let learned =
            record.end_positions_from(&live, &dir.join("none"), &dir.join("none"), &mut |_, _| {});

        assert_eq!(
            learned, 2,
            "the log's end positions did not reach the record"
        );
        assert_eq!(record.sittings.len(), 1, "a sitting came in with them");
        // `card` states the counter the sitting holds.
        let identified = record.recover(&[card("Vom Kriege", 900_000, 4_100)]);
        assert_eq!(identified, 1, "the counter reached no sidecar");
        assert_eq!(record.books.len(), 1);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn reading_the_end_positions_again_hands_back_no_reading_the_reader_emptied() {
        let dir = scratch("end-positions-reset");
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
        let mut record = having_cleared("260807:120000");
        record.watermark = "260807:101543".into();
        record.reset_at = "260807:120000".into();
        record.end_positions_from(&live, &dir.join("none"), &dir.join("none"), &mut |_, _| {});

        assert!(record.sittings.is_empty(), "emptied reading came back");
        assert_eq!(
            record.reset_at, "260807:120000",
            "the reset stamp came down"
        );
        assert_eq!(record.cleared.len(), 1, "a book's stamp came off");
        assert!(
            !record.counters.is_empty(),
            "no end position reached the record"
        );
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
        let mut record = Record {
            sittings: vec![Sitting {
                words: 0,
                ..sitting("2026-08-07T10:15:01", "2026-08-07T10:15:43", 148_207, 42)
            }],
            watermark: "260807:101543".into(),
            watermark_offset: None,
            ..Record::default()
        };
        record.rebuild_from(&live, &dir.join("none"), &dir.join("none"), &mut |_, _| {});

        assert_eq!(record.sittings.len(), 1, "the sitting was stated twice");
        assert_eq!(record.sittings[0].words, 800, "the stale measurement stood");
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// A page, a sleep of forty minutes, and two pages after the wake, the
    /// first measured from it.
    fn log_with_a_wake() -> String {
        [
            page("260807:100000", 7_390_020),
            "260807:100030 cvm[6144]: I ReadingTimerController:Information::\
             Reading_Interrupted,Reason:1;"
                .to_string(),
            "260807:104000 cvm[6144]: I ReadingTimerController:Information::\
             Reading_Resumed,Reason:2;"
                .to_string(),
            page("260807:104040", 7_429_910),
            page("260807:104120", 7_469_800),
        ]
        .join("\n")
            + "\n"
    }

    /// A stored row opening at the first page after a wake, and the log opening
    /// its sitting at the wake, ending on the same second: one sitting, once.
    #[test]
    fn a_rebuild_restates_once_a_sitting_the_log_now_opens_at_its_wake() {
        let dir = scratch("rebuild-wake");
        let live = dir.join("messages");
        std::fs::write(&live, log_with_a_wake()).expect("a log to read");
        let mut record = Record {
            sittings: vec![sitting(
                "2026-08-07T10:40:40",
                "2026-08-07T10:41:20",
                148_207,
                39,
            )],
            watermark: "260807:104120".into(),
            ..Record::default()
        };
        record.rebuild_from(&live, &dir.join("none"), &dir.join("none"), &mut |_, _| {});

        let ending: Vec<&Sitting> = record
            .sittings
            .iter()
            .filter(|s| s.ended_at == "2026-08-07T10:41:20")
            .collect();
        assert_eq!(ending.len(), 1, "the sitting was stated twice");
        assert_eq!(ending[0].started_at, "2026-08-07T10:40:00");
        // Both pages after the wake, the first one included.
        assert_eq!(ending[0].seconds, 79);
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// `read_from` the half of a sitting past midnight answers the half before it,
    /// where the counter at the boundary comes from.
    #[test]
    fn a_pass_reading_a_sitting_cut_at_midnight_reads_it_from_the_day_before() {
        let lines: Vec<String> = [
            ("260807:235900", 7_000_000),
            ("260807:235940", 7_039_890),
            ("260808:000020", 7_079_780),
            ("260808:000100", 7_119_670),
        ]
        .iter()
        .map(|(stamp, total_ms)| page(stamp, *total_ms))
        .collect();
        let mut record = Record::default();
        record.absorb(&lines, "");
        let after: Vec<(String, i64)> = record
            .sittings
            .iter()
            .map(|s| (s.started_at.clone(), s.seconds))
            .collect();
        assert_eq!(after[1].0, "2026-08-08T00:00:00");

        let from = record.read_from();
        assert_eq!(
            from, "260807:235900",
            "the half past midnight was read alone"
        );
        let batch: Vec<String> = lines
            .iter()
            .filter(|l| l.as_str() >= from.as_str())
            .cloned()
            .collect();
        record.absorb(&batch, &from);
        let again: Vec<(String, i64)> = record
            .sittings
            .iter()
            .map(|s| (s.started_at.clone(), s.seconds))
            .collect();
        assert_eq!(again, after);
    }

    /// `absorb` over a `batch` from the wake opening a sitting of page metrics
    /// alone keeps the sitting, on the book its stored row names.
    #[test]
    fn a_pass_reading_from_a_wake_keeps_a_sitting_of_pages_alone() {
        let worded = |stamp: &str| {
            format!(
                "{stamp} fastmetrics[9842]: D fastmetrics: Emitting a new record. \
                 SchemaName[ereader_book_consume_content], Fields[{{ \"words_count\" : 300 }} ]. :"
            )
        };
        let lines: Vec<String> = vec![
            page("260807:100000", 7_390_020),
            "260807:100030 cvm[6144]: I ReadingTimerController:Information::\
             Reading_Interrupted,Reason:1;"
                .to_string(),
            "260807:120000 cvm[6144]: I ReadingTimerController:Information::\
             Reading_Resumed,Reason:2;"
                .to_string(),
            worded("260807:120010"),
            worded("260807:120100"),
            worded("260807:120200"),
        ];
        let mut record = Record::default();
        record.absorb(&lines, "");
        let pages = |record: &Record| {
            record
                .sittings
                .iter()
                .find(|s| s.started_at == "2026-08-07T12:00:00")
                .map(|s| s.paged_seconds)
        };
        assert_eq!(pages(&record), Some(110));

        let from = record.read_from();
        assert_eq!(from, "260807:120000");
        let batch: Vec<String> = lines
            .iter()
            .filter(|l| l.as_str() >= from.as_str())
            .cloned()
            .collect();
        record.absorb(&batch, &from);
        assert_eq!(
            pages(&record),
            Some(110),
            "the sitting was lost to the pass"
        );
    }

    /// `absorb` over a batch from the wake, none of the lines before it held,
    /// keeps the counter the stored row opened at.
    #[test]
    fn a_pass_reading_from_a_wake_keeps_the_counter_the_sitting_opened_at() {
        let kpp = |stamp: &str, event: &str, total_ms: i64| {
            format!(
                "{stamp} java[5878]: I ReadingTimerController:Information::{event}\
                 PageStartPos:YJPosition: AeUAAAAwAAAA:10671,IntervalTime:91293,\
                 IntervalWords:187,SkipAvgReason:RTC_PrevPage,TotalTime:{total_ms},\
                 TotalWords:1322,CurrentPos:YJPosition: AeUAAAAwAAAA:10671,\
                 EndPos:YJPosition: AbIWAADQAAAA:289095,PosLeft:278424,%Left:0.9622;"
            )
        };
        let lines: Vec<String> = vec![
            kpp("260807:223127", "", 18_963_538),
            "260807:223131 powerd[1]: I lipc:evts:name=goingToScreenSaver, \
             origin=com.lab126.powerd"
                .to_string(),
            "260807:233443 cvm[6397]: I ReadingTimerController:Information::\
             Reading_Resumed,Reason:2;"
                .to_string(),
            "260807:233443 powerd[1]: I lipc:evts:name=outOfScreenSaver, \
             origin=com.lab126.powerd"
                .to_string(),
            kpp("260807:234505", "CloseBook,", 19_285_414),
        ];
        let mut record = Record::default();
        record.absorb(&lines, "");
        let after_wake = record
            .sittings
            .iter()
            .find(|s| s.started_at == "2026-08-07T23:34:43")
            .cloned()
            .expect("the sitting after the wake");
        assert_eq!(after_wake.start_counter_ms, Some(18_963_538));

        // `read_from`: the newest sitting's start.
        let from = record.read_from();
        assert_eq!(from, "260807:233443");
        let batch: Vec<String> = lines
            .iter()
            .filter(|l| l.as_str() >= from.as_str())
            .cloned()
            .collect();
        record.absorb(&batch, &from);
        let again = record
            .sittings
            .iter()
            .find(|s| s.started_at == "2026-08-07T23:34:43")
            .expect("the sitting after the wake, read again");
        assert_eq!(again.start_counter_ms, after_wake.start_counter_ms);
        assert_eq!(again.seconds, after_wake.seconds);
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

        let mut record = having_cleared("260807:120000");
        record.watermark = "260807:101543".into();
        record.reset_at = "260807:120000".into();
        let added =
            record.rebuild_from(&live, &dir.join("none"), &dir.join("none"), &mut |_, _| {});

        assert_eq!(added, 0);
        assert!(record.sittings.is_empty(), "a cleared book came back");
        assert_eq!(record.cleared.len(), 1, "the stamp came off");
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// [`crate::settings::StatsFrom::App`] reads the page figure, and the `s`
    /// row carries it, a row stating none falling back to `awake_seconds`.
    #[test]
    fn an_s_row_carries_the_page_figure_beside_the_counter_and_the_awake_span() {
        let record = Record {
            sittings: vec![Sitting {
                started_at: "2026-09-08T21:51:10".into(),
                ended_at: "2026-09-08T23:41:17".into(),
                end_position: 1_911_890,
                seconds: 2390,
                awake_seconds: 6607,
                paged_seconds: 5964,
                ..Sitting::default()
            }],
            ..Record::default()
        };
        let back = Record::from_text(&record.text());
        assert_eq!(back.sittings, record.sittings);

        let older: String = record
            .text()
            .lines()
            .map(|l| match l.strip_prefix("s\t") {
                Some(rest) => {
                    // The four fields a build before v0.3.4 states none of, the
                    // share of the book after them, and the stretches of the book
                    // after that.
                    let cut = (0..6).fold(rest, |r, _| {
                        r.rsplit_once('\t').expect("a page or interval field").0
                    });
                    format!("s\t{cut}\n")
                }
                None => format!("{l}\n"),
            })
            .collect();
        let read = Record::from_text(&older);
        assert_eq!(read.sittings[0].paged_seconds, 0);
        assert_eq!(read.sittings[0].paged_words, 0);
        assert_eq!(read.sittings[0].timed_seconds, 0);
        assert_eq!(read.sittings[0].timed_words, 0);
        assert_eq!(read.sittings[0].covered, 0.0);
        assert!(read.sittings[0].stretches.is_empty());
        assert_eq!(read.sittings[0].awake_seconds, 6607);
        assert_eq!(read.sittings[0].seconds, 2390);
    }

    /// `Sitting::stretches` survives [`Record::text`] and [`Record::from_text`],
    /// and an `s` row whose field is empty reads as an empty list.
    #[test]
    fn the_stretches_survive_the_record() {
        let stretches = vec![
            Stretch {
                from: 0.05,
                to: 0.08,
            },
            Stretch {
                from: 0.1,
                to: 0.1234,
            },
            // One read backwards, which the record states as it stands.
            Stretch {
                from: 0.9,
                to: 0.87,
            },
        ];
        let record = Record {
            sittings: vec![Sitting {
                started_at: "2026-09-15T20:00:00".into(),
                ended_at: "2026-09-15T20:30:00".into(),
                end_position: 148_207,
                seconds: 1800,
                stretches: stretches.clone(),
                ..Sitting::default()
            }],
            ..Record::default()
        };
        let back = Record::from_text(&record.text());
        assert_eq!(back.sittings[0].stretches, stretches);
        // `stretches` stands last, past every field an older row holds.
        assert!(record.text().contains("\t500-800,1000-1234,9000-8700"));
        assert!(read_stretches("").is_empty());
        assert!(read_stretches("nonsense").is_empty());
    }

    /// [`Record::absorb`] replaces a row opening at or after `cut` and retains
    /// one opening before it. `Sitting::stretches` holds the progress of its own
    /// first turn under both.
    #[test]
    fn a_sitting_extended_by_a_later_pass_keeps_the_progress_it_opened_at() {
        let page_at = |stamp: &str, total_ms: i64, left: f64| {
            format!(
                "{stamp} cvm[6144]: I ReadingTimerController:Information::NextPage,\
                 Verdict:Processed,IntervalTime:39890,TotalTime:{total_ms},TotalWords:49583,\
                 CurrentPos:YJPosition: AfQJAAAAAAAA:54205,\
                 EndPos:YJPosition: AbcVAAAPAAAA:148207,PosLeft:94002,%Left:{left};"
            )
        };
        // Two turns, at 10 % and at 20 %.
        let first = [
            page_at("260807:101501", 7_390_020, 0.9),
            page_at("260807:101543", 7_431_463, 0.8),
        ];
        let mut record = Record::default();
        record.absorb(&first, "");
        assert_eq!(record.sittings.len(), 1);
        assert_eq!(write_stretches(&record.sittings[0].stretches), "1000-2000");

        // `read_from`: this sitting's own start, the watermark within `SITTING_GAP_SECS` of it.
        let from = record.read_from();
        assert_eq!(from, "260807:101501", "the batch opens at the sitting");
        let whole = [
            first[0].clone(),
            first[1].clone(),
            page_at("260807:101643", 7_531_463, 0.7),
        ];
        record.absorb(&whole, &from);
        assert_eq!(
            record.sittings.len(),
            1,
            "the row was replaced, not doubled"
        );
        assert_eq!(
            write_stretches(&record.sittings[0].stretches),
            "1000-3000",
            "the sitting still opens where the reader opened it",
        );

        // Past `SITTING_GAP_SECS`: `read_from` answers `watermark`, `absorb` keeping the row.
        record.watermark = "260807:120000".into();
        let from = record.read_from();
        assert!(
            from.as_str() > "260807:101501",
            "the batch opens past the sitting"
        );
        record.absorb(&[page_at("260807:120001", 9_000_000, 0.5)], &from);
        let held = record
            .sittings
            .iter()
            .find(|s| s.started_at == "2026-08-07T10:15:01")
            .expect("the sitting stands");
        assert_eq!(write_stretches(&held.stretches), "1000-3000");
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

        // One row `live` restates, at other figures, and one it does not.
        let mut record = Record {
            sittings: vec![
                Sitting {
                    started_at: "2026-08-07T10:15:01".into(),
                    ended_at: "2026-08-07T10:15:43".into(),
                    end_position: 148_207,
                    seconds: 9,
                    awake_seconds: 0,
                    ..Sitting::default()
                },
                Sitting {
                    started_at: "2020-01-01T00:00:00".into(),
                    ended_at: "2020-01-01T00:30:00".into(),
                    end_position: 148_207,
                    seconds: 1800,
                    ..Sitting::default()
                },
            ],
            watermark: "260807:101543".into(),
            watermark_offset: None,
            ..Record::default()
        };

        let healed = record.heal_from(&live, &dir.join("none"), &dir.join("none"), &mut |_, _| {});
        assert_eq!(healed, 1, "the row the log restates kept its old figures");
        assert_eq!(record.sittings.len(), 2, "a row was given up");
        let fresh = &record.sittings[1];
        assert_eq!(fresh.started_at, "2026-08-07T10:15:01");
        assert_eq!(fresh.seconds, 41, "the counter's own span");
        let older = &record.sittings[0];
        assert_eq!(older.started_at, "2020-01-01T00:00:00");
        assert_eq!(older.seconds, 1800, "a row older than the logs was touched");
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// [`Record::heal_from`] sets `stretches` on a row holding none, over the
    /// span the log restates, and leaves a row the log does not restate whole.
    #[test]
    fn a_heal_fills_the_stretches_on_a_row_that_was_written_without_them() {
        let dir = scratch("heal-sittings");
        let live = dir.join("messages");
        let page_at = |stamp: &str, total_ms: i64, left: f64| {
            format!(
                "{stamp} cvm[6144]: I ReadingTimerController:Information::NextPage,\
                 Verdict:Processed,IntervalTime:39890,TotalTime:{total_ms},TotalWords:49583,\
                 CurrentPos:YJPosition: AfQJAAAAAAAA:54205,\
                 EndPos:YJPosition: AbcVAAAPAAAA:148207,PosLeft:94002,%Left:{left};"
            )
        };
        std::fs::write(
            &live,
            format!(
                "{}\n{}\n",
                page_at("260807:101501", 7_390_020, 0.9),
                page_at("260807:101543", 7_431_463, 0.75),
            ),
        )
        .expect("a log to read");

        let mut record = Record {
            sittings: vec![
                Sitting {
                    started_at: "2026-08-07T10:15:01".into(),
                    ended_at: "2026-08-07T10:15:43".into(),
                    end_position: 148_207,
                    seconds: 41,
                    progress: Some(0.25),
                    stretches: Vec::new(),
                    ..Sitting::default()
                },
                Sitting {
                    started_at: "2020-01-01T00:00:00".into(),
                    ended_at: "2020-01-01T00:30:00".into(),
                    end_position: 148_207,
                    seconds: 1800,
                    progress: Some(0.5),
                    ..Sitting::default()
                },
            ],
            watermark: "260807:101543".into(),
            watermark_offset: None,
            ..Record::default()
        };
        record.heal_from(&live, &dir.join("none"), &dir.join("none"), &mut |_, _| {});

        let reached = record
            .sittings
            .iter()
            .find(|s| s.started_at == "2026-08-07T10:15:01")
            .expect("the row the log reaches");
        assert_eq!(
            write_stretches(&reached.stretches),
            "1000-2500",
            "the stretches the log restates",
        );
        let older = record
            .sittings
            .iter()
            .find(|s| s.started_at == "2020-01-01T00:00:00")
            .expect("the row older than the logs");
        assert!(
            older.stretches.is_empty(),
            "a row the log does not restate holds no stretch",
        );
        assert_eq!(
            older.progress,
            Some(0.5),
            "and it keeps the progress it had"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// A stored row closing where its pass stopped reading, the rest of its sitting
    /// in the log: [`Record::heal_from`] states the whole sitting on the one row.
    #[test]
    fn a_heal_holds_one_row_for_a_sitting_whose_close_moved() {
        let dir = scratch("heal-moved-close");
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

        let mut record = Record {
            sittings: vec![Sitting {
                started_at: "2026-08-07T10:15:01".into(),
                ended_at: "2026-08-07T10:15:20".into(),
                end_position: 148_207,
                seconds: 9,
                ..Sitting::default()
            }],
            watermark: "260807:101543".into(),
            watermark_offset: None,
            ..Record::default()
        };
        record.heal_from(&live, &dir.join("none"), &dir.join("none"), &mut |_, _| {});

        assert_eq!(record.sittings.len(), 1, "one sitting, one row");
        assert_eq!(record.sittings[0].seconds, 41);
        assert_eq!(record.sittings[0].ended_at, "2026-08-07T10:15:43");
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// [`Record::take_highlights`] holds the `start`, `end`, `colour` and
    /// [`State::Live`] of the highlight sharing its `identity`, against a fresh highlight stating
    /// none of them.
    #[test]
    fn a_highlight_keeps_the_sidecar_fields_a_pass_without_one_states_none_of() {
        let held = Highlight {
            extent: 350_188,
            title: "Do Androids Dream of Electric Sheep?".into(),
            kind: Kind::Highlight,
            at: "2026-09-09T21:39:55".into(),
            state: State::Live,
            start: 246_506,
            end: 246_594,
            location: 1479,
            page: "159".into(),
            colour: "orange".into(),
            body: "See, now we have nothing to hide".into(),
        };
        let mut record = Record {
            highlights: vec![held.clone()],
            ..Record::default()
        };
        // The same highlight as the clippings file alone can state it.
        let thin = Highlight {
            state: State::Unconfirmed,
            start: -1,
            end: -1,
            colour: String::new(),
            ..held.clone()
        };
        record.take_highlights(vec![thin], Gate::default());

        assert_eq!(record.highlights.len(), 1);
        let kept = &record.highlights[0];
        assert_eq!(kept.start, 246_506, "the sidecar position stands");
        assert_eq!(kept.end, 246_594);
        assert_eq!(kept.colour, "orange");
        assert_eq!(kept.state, State::Live);
    }

    /// [`Record::merge`] folds `other`'s highlights into the ones held by `identity`,
    /// taking the sidecar fields and adding no second row.
    #[test]
    fn merging_a_backup_restores_a_highlight_and_doubles_none() {
        let good = Highlight {
            extent: 350_188,
            title: "Do Androids Dream of Electric Sheep?".into(),
            kind: Kind::Note,
            at: "2026-09-09T21:41:28".into(),
            state: State::Live,
            start: 246_588,
            end: 246_594,
            location: 1479,
            page: "159".into(),
            colour: String::new(),
            body: "Happiness is being respected".into(),
        };
        let thin = Highlight {
            state: State::Unconfirmed,
            start: -1,
            end: -1,
            ..good.clone()
        };
        let mut record = Record {
            highlights: vec![thin],
            ..Record::default()
        };
        let backup = Record {
            highlights: vec![good.clone()],
            ..Record::default()
        };
        record.merge(&backup);

        assert_eq!(record.highlights.len(), 1, "one highlight, not two");
        assert_eq!(record.highlights[0].start, 246_588);
        assert_eq!(record.highlights[0].state, State::Live);
    }

    /// [`Record::heal_from`] corrects a row held under [`Record::reset_at`], the
    /// shape [`Record::reset`] and a restore leave, and adds none.
    #[test]
    fn a_heal_corrects_a_held_row_from_before_the_reset() {
        let dir = scratch("heal-before-reset");
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

        let mut record = Record {
            sittings: vec![Sitting {
                started_at: "2026-08-07T10:15:01".into(),
                ended_at: "2026-08-07T10:15:43".into(),
                end_position: 148_207,
                seconds: 9,
                ..Sitting::default()
            }],
            watermark: "260807:101543".into(),
            watermark_offset: None,
            // Past every line `live` holds.
            reset_at: "260810:120000".into(),
            ..Record::default()
        };
        let healed = record.heal_from(&live, &dir.join("none"), &dir.join("none"), &mut |_, _| {});

        assert_eq!(healed, 1, "the held row was corrected");
        assert_eq!(record.sittings.len(), 1, "and no row was added under it");
        assert_eq!(record.sittings[0].seconds, 41);
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// `ended_at` names the sitting a held row and the parse disagree about
    /// `started_at` on, the parse opening it at midnight.
    #[test]
    fn a_heal_reaches_a_sitting_whose_open_a_midnight_cut_moved() {
        let dir = scratch("heal-midnight");
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

        let mut record = Record {
            sittings: vec![Sitting {
                // The open a watermark pass states.
                started_at: "2026-08-07T10:15:30".into(),
                ended_at: "2026-08-07T10:15:43".into(),
                end_position: 148_207,
                seconds: 9,
                ..Sitting::default()
            }],
            watermark: "260807:101543".into(),
            watermark_offset: None,
            ..Record::default()
        };
        record.heal_from(&live, &dir.join("none"), &dir.join("none"), &mut |_, _| {});

        assert_eq!(record.sittings.len(), 1, "one sitting, one row");
        assert_eq!(record.sittings[0].seconds, 41);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_heal_leaves_reset_at_and_the_era_before_it_alone() {
        let dir = scratch("heal-reset");
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

        let mut record = Record {
            watermark: "260807:101543".into(),
            watermark_offset: None,
            reset_at: "260807:120000".into(),
            ..Record::default()
        };
        let healed = record.heal_from(&live, &dir.join("none"), &dir.join("none"), &mut |_, _| {});

        assert_eq!(healed, 0);
        assert!(
            record.sittings.is_empty(),
            "the era before the reset came back"
        );
        assert_eq!(
            record.reset_at, "260807:120000",
            "the reset stamp was lifted"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_rebuild_states_sittings_and_leaves_every_book_to_the_library() {
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
        let library = [listed(938_018, "B00OKPCRLG", "Bible", 55.0)];

        let mut record = Record::default();
        record.remember(&library);
        record.watermark = "260807:101543".into();
        record.reset();
        assert!(record.books.is_empty(), "a reset left a book standing");

        let added =
            record.rebuild_from(&live, &dir.join("none"), &dir.join("none"), &mut |_, _| {});
        assert_eq!(added, 1, "the log's sitting did not come back");
        assert!(
            record.books.is_empty(),
            "a parse identified a book — only the library can, and a reset that \
             skips it hands back sittings against nothing"
        );

        // The pass `App::relearn` makes once a reset is over.
        record.remember(&library);
        assert_eq!(record.books.len(), 1, "the library left the books empty");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn merging_one_record_twice_is_merging_it_once() {
        let mut record = two_books();
        let copy = record.clone();
        assert_eq!(record.merge(&copy), 0);
        assert_eq!(record, copy);
    }

    #[test]
    fn two_eras_merge_to_their_union() {
        let whole = two_books();
        let (older, newer): (Vec<Sitting>, Vec<Sitting>) = whole
            .sittings
            .iter()
            .cloned()
            .partition(|s| s.started_at.as_str() < "2026-08-09");
        assert_eq!((older.len(), newer.len()), (2, 1), "a split worth making");

        let mut first = Record {
            sittings: older,
            ..whole.clone()
        };
        let second = Record {
            sittings: newer,
            ..whole.clone()
        };
        assert_eq!(first.merge(&second), 1);
        assert_eq!(first.sittings, whole.sittings);
    }

    #[test]
    fn a_merge_leaves_reset_at_and_the_cleared_books_standing() {
        let mut record = two_books();
        record.clear_book(148_207, "B00OKPCRLG");
        record.reset_at = "260810:120000".into();
        let held = record.cleared.clone();

        // What a backup of the book holds.
        let mut coming_back = Record::default();
        coming_back.sittings.push(sitting(
            "2026-08-07T10:15:01",
            "2026-08-07T10:55:43",
            148_207,
            2_400,
        ));
        assert_eq!(record.merge(&coming_back), 1, "a backup is not a parse");
        assert_eq!(record.reset_at, "260810:120000");
        assert_eq!(record.cleared, held);
    }

    #[test]
    fn one_books_rows_carry_its_sittings_and_the_book_alone() {
        let record = two_books();
        let one = record.one_book(148_207, "B00OKPCRLG");
        assert_eq!(one.sittings.len(), 2);
        assert!(one.sittings.iter().all(|s| s.end_position == 148_207));
        assert_eq!(one.books.len(), 1);
        assert_eq!(one.books[0].title, "A Book");
        assert!(
            one.watermark.is_empty(),
            "a stamp belongs to the whole record"
        );
        assert_eq!(Record::from_text(&one.text()).sittings.len(), 2);
    }

    #[test]
    fn one_book_of_a_record_that_holds_none_is_empty() {
        assert_eq!(two_books().one_book(1, "B00NOTHERE"), Record::default());
    }

    /// A record that gave up book 148207 at `at`, which is what `page` writes.
    fn having_cleared(at: &str) -> Record {
        Record {
            reset_at: String::new(),
            highlights: Vec::new(),
            gate: None,
            sources: None,
            cleared: vec![Cleared {
                extent: 148_207,
                key: "B00OKPCRLG".into(),
                at: at.into(),
            }],
            ..Record::default()
        }
    }

    #[test]
    fn the_whole_log_re_derives_nothing_of_a_cleared_book() {
        let lines = vec![
            page("260807:101501", 7_390_020),
            page("260807:101543", 7_431_463),
        ];
        // A `c` row stamped past the sitting, as `drop_reading` writes one.
        let mut record = having_cleared("260807:120000");
        let (added, extended) = record.absorb(&lines, "");
        assert_eq!((added, extended), (0, 0));
        assert!(record.sittings.is_empty(), "the log handed it back");
        assert_eq!(
            record.watermark, "260807:101543",
            "the lines were still read"
        );
    }

    #[test]
    fn reading_after_a_clear_is_folded_as_any_other() {
        let lines = vec![
            page("260807:101501", 7_390_020),
            page("260807:101543", 7_431_463),
        ];
        let mut record = having_cleared("260807:100000");
        let (added, _) = record.absorb(&lines, "");
        assert_eq!(added, 1, "the sitting starts past the stamp");
        assert_eq!(record.sittings.len(), 1);
    }

    #[test]
    fn a_clear_reaches_the_book_through_the_ends_map() {
        let lines = vec![
            page("260807:101501", 7_390_020),
            page("260807:101543", 7_431_463),
        ];
        // `ends` raises the same book to 148209, and the `c` row names that.
        let mut record = Record {
            ends: vec![(148_207, 148_209)],
            reset_at: String::new(),
            highlights: Vec::new(),
            gate: None,
            sources: None,
            cleared: vec![Cleared {
                extent: 148_209,
                key: String::new(),
                at: "260807:120000".into(),
            }],
            ..Record::default()
        };
        record.absorb(&lines, "");
        assert!(
            record.sittings.is_empty(),
            "the sitting was placed by extent"
        );
    }

    #[test]
    fn one_book_cleared_leaves_another_books_sittings_alone() {
        let mut record = having_cleared("260807:120000");
        record.sittings.push(sitting(
            "2026-08-01T09:00:00",
            "2026-08-01T09:30:00",
            999,
            1800,
        ));
        let lines = vec![
            page("260807:101501", 7_390_020),
            page("260807:101543", 7_431_463),
        ];
        record.absorb(&lines, "260807:000000");
        assert_eq!(record.sittings.len(), 1);
        assert_eq!(record.sittings[0].end_position, 999);
    }

    #[test]
    fn the_newest_clear_of_one_book_is_the_one_kept() {
        let mut record = Record {
            reset_at: String::new(),
            highlights: Vec::new(),
            gate: None,
            sources: None,
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
            ..Record::default()
        };
        record.sort();
        assert_eq!(record.cleared.len(), 1);
        assert_eq!(record.cleared[0].at, "260807:120000");
    }

    /// [`Record::sort`] keeps the longer of two rows sharing `started_at` and
    /// `end_position`, their spans nesting.
    #[test]
    fn two_rows_sharing_a_start_second_are_one_sitting() {
        let mut record = Record {
            sittings: vec![
                sitting("2026-08-07T21:45:36", "2026-08-07T21:45:37", 304_517, 1),
                sitting("2026-08-07T21:45:36", "2026-08-07T22:12:47", 304_517, 1_631),
            ],
            ..Record::default()
        };
        record.sort();
        assert_eq!(record.sittings.len(), 1);
        assert_eq!(record.sittings[0].seconds, 1_631, "the sitting seen whole");
        assert_eq!(record.sittings[0].ended_at, "2026-08-07T22:12:47");

        let again = record.sittings.clone();
        record.sittings.extend(again);
        record.sort();
        assert_eq!(record.sittings.len(), 1);
    }

    /// [`Record::drop_nested`] keeps the earliest start of two rows sharing
    /// `ended_at` and `end_position`, and the close `short` states.
    #[test]
    fn two_rows_sharing_a_close_second_are_one_sitting() {
        let short = Sitting {
            time_left: Some(57_000),
            stated_wpm: Some(279),
            paged_seconds: 147,
            start_counter_ms: Some(2_165_201),
            end_counter_ms: Some(2_296_253),
            ..sitting("2026-09-05T18:15:02", "2026-09-05T18:21:43", 226_229, 131)
        };
        let mut record = Record {
            sittings: vec![
                sitting("2026-09-05T18:03:38", "2026-09-05T18:21:43", 226_229, 776),
                sitting("2026-09-05T18:10:00", "2026-09-05T18:21:43", 148_207, 600),
                short,
                sitting("2026-09-05T18:15:02", "2026-09-05T18:30:00", 148_207, 500),
            ],
            ..Record::default()
        };
        assert_eq!(record.nested(), 1);
        assert_eq!(record.drop_nested(), 1);
        assert_eq!(
            record
                .sittings
                .iter()
                .map(|s| (s.started_at.as_str(), s.end_position, s.seconds))
                .collect::<Vec<_>>(),
            [
                ("2026-09-05T18:03:38", 226_229, 776),
                ("2026-09-05T18:10:00", 148_207, 600),
                ("2026-09-05T18:15:02", 148_207, 500),
            ]
        );
        let kept = &record.sittings[0];
        assert_eq!((kept.time_left, kept.stated_wpm), (Some(57_000), Some(279)));
        // `paged_seconds` and the counters measure the shorter span alone.
        assert_eq!((kept.paged_seconds, kept.start_counter_ms), (0, None));
        assert_eq!((record.nested(), record.drop_nested()), (0, 0));
    }

    /// [`Record::merge`] takes `short` nested in a held `long` as its close alone,
    /// and gives a held `short` the start of an incoming `long`.
    #[test]
    fn a_merge_holds_one_row_per_close() {
        let long = sitting("2026-08-30T02:09:32", "2026-08-30T02:37:01", 933_987, 1_218);
        let short = Sitting {
            time_left: Some(83_820),
            ..sitting("2026-08-30T02:15:02", "2026-08-30T02:37:01", 933_987, 922)
        };
        let mut record = Record {
            sittings: vec![long.clone()],
            ..Record::default()
        };
        let incoming = Record {
            sittings: vec![long.clone(), short.clone()],
            ..Record::default()
        };
        assert_eq!(record.merge(&incoming), 0);
        assert_eq!(record.sittings.len(), 1);
        assert_eq!(record.sittings[0].seconds, 1_218);
        assert_eq!(record.sittings[0].time_left, Some(83_820));

        let mut record = Record {
            sittings: vec![short],
            ..Record::default()
        };
        assert_eq!(record.merge(&incoming), 0);
        assert_eq!(record.sittings.len(), 1);
        assert_eq!(record.sittings[0].started_at, long.started_at);
        assert_eq!(record.sittings[0].seconds, 1_218);
    }

    #[test]
    fn a_sitting_is_keyed_by_the_number_the_library_uses() {
        let record = Record {
            ends: vec![(938_016, 938_018)],
            keys: Vec::new(),
            counters: Vec::new(),
            pairs: Vec::new(),
            ..Record::default()
        };
        assert_eq!(record.extent_of(938_016), 938_018);
        // `extent_of` a book no line ever mapped: its own key.
        assert_eq!(record.extent_of(148_207), 148_207);
    }

    #[test]
    fn a_stored_mapping_gives_way_to_what_the_log_states_again() {
        let mut record = Record {
            // 148207 against another book's end.
            ends: vec![(148_207, 938_018)],
            ..Record::default()
        };
        let lines = [
            "260807:100000 java[1]: I ReadingTimerController:Information::OpenBook,StoredBookData:null;".to_string(),
            "260807:100001 java[1]: I ReadingTimerController:Information::BookEndPosition.FromBook:YJPosition: AZI/AAAAAAAA:148213,CurrentPos:YJPosition: AWUDAAAAAAAA:2,EndPos:YJPosition: AbcVAAAPAAAA:148207,PosLeft:6;".to_string(),
        ];
        record.absorb(&lines, "");
        assert_eq!(record.extent_of(148_207), 148_213);

        // `merge` takes it too.

        let mut held = Record {
            ends: vec![(148_207, 938_018)],
            ..Record::default()
        };
        held.merge(&record);
        assert_eq!(held.extent_of(148_207), 148_213);
    }

    #[test]
    fn a_mobi8_sitting_reaches_the_book_the_library_names() {
        let mut record = Record::default();
        let lines = [
            "260906:192401 java[1]: I ReadingTimerController:Information::OpenBook,StoredBookData:TimeRead:329 sec. WPM:0. Version:0,Title:<private>;".to_string(),
            "260906:192402 java[1]: I ReadingTimerController:Information::BookEndPosition.FromBook:HTMLPosition:19886521,CurrentPos:HTMLPosition:7731097,EndPos:HTMLPosition:19886489,PosLeft:12155392;".to_string(),
            "260906:192404 cvm[6144]: I ReadingTimerController:Information::NextPage,Verdict:Processed,PageStartPos:HTMLPosition:7731097,IntervalTime:785,IntervalWords:12,TotalTime:329785,TotalWords:1905,CurrentPos:HTMLPosition:7731097,EndPos:HTMLPosition:19886489,PosLeft:12155392,%Left:0.6112;".to_string(),
            "260906:192425 cvm[6144]: I ReadingTimerController:Information::NextPage,Verdict:Processed,PageStartPos:HTMLPosition:7731725,IntervalTime:21217,IntervalWords:172,TotalTime:351002,TotalWords:2077,CurrentPos:HTMLPosition:7731725,EndPos:HTMLPosition:19886489,PosLeft:12154764,%Left:0.6111;".to_string(),
        ];
        assert_eq!(record.absorb(&lines, ""), (1, 0));

        // `p_contentSize` 19886522 against `FromBook` 19886521.
        record.remember(&[listed(19_886_522, "*8F3C", "A Sideload", -1.0)]);
        let extent = record.extent_of(record.sittings[0].end_position);
        assert_eq!(extent, 19_886_522);
        assert_eq!(
            record.book_for(extent, None).map(|b| b.title.as_str()),
            Some("A Sideload")
        );
    }

    #[test]
    fn a_sitting_reaches_its_book_at_an_extent_the_record_no_longer_carries() {
        let mut record = Record {
            books: vec![Book {
                // The copy the library names today.
                extent: 148_199,
                cde_key: "BTIFIHP3JYKFKDNZLSD7CODVHTDUEOIM".into(),
                title: "A Volume".into(),
                ..Book::default()
            }],
            ends: vec![(148_207, 148_213)],
            ..Record::default()
        };
        assert!(record.book_for(148_213, None).is_none());

        // `remember` at 148213, under the key the record carries.
        record.remember(&[listed(
            148_213,
            "BTIFIHP3JYKFKDNZLSD7CODVHTDUEOIM",
            "A Volume",
            -1.0,
        )]);
        // `remember` again at 148199, which the record takes.
        record.remember(&[listed(
            148_199,
            "BTIFIHP3JYKFKDNZLSD7CODVHTDUEOIM",
            "A Volume",
            -1.0,
        )]);
        assert_eq!(record.books[0].extent, 148_199, "the record follows");

        assert_eq!(
            record.book_for(148_213, None).map(|b| &b.title[..]),
            Some("A Volume")
        );
        // Written down and read back.
        let read = Record::from_text(&record.text());
        assert_eq!(read.keys, record.keys);
        assert_eq!(
            read.book_for(148_213, None).map(|b| &b.title[..]),
            Some("A Volume")
        );
    }

    #[test]
    fn an_extent_two_books_have_carried_names_neither() {
        let mut record = Record {
            books: vec![
                Book {
                    extent: 1,
                    cde_key: "ONE".into(),
                    ..Book::default()
                },
                Book {
                    extent: 2,
                    cde_key: "TWO".into(),
                    ..Book::default()
                },
            ],
            ..Record::default()
        };
        record.learn_key(999, "ONE");
        assert!(record.book_for(999, None).is_some());
        record.learn_key(999, "TWO");
        assert!(record.book_for(999, None).is_none());
    }

    // ---- identifying an end position no book reaches ---------------

    /// A record of three days' reading: one book the library names and two
    /// end positions it does not.
    fn with_unidentified() -> Record {
        Record {
            sittings: vec![
                sitting("2026-08-07T10:00:00", "2026-08-07T11:00:00", 148_207, 3_600),
                sitting("2026-08-08T10:00:00", "2026-08-08T11:00:00", 500_100, 3_600),
                sitting("2026-08-09T10:00:00", "2026-08-09T11:00:00", 700_200, 3_600),
            ],
            books: vec![Book {
                extent: 148_207,
                cde_key: "B00OKPCRLG".into(),
                title: "A Named Book".into(),
                ..Book::default()
            }],
            ..Record::default()
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

    /// [`Record::identify_from`] with no end position held back.
    fn identify_from(
        record: &mut Record,
        said: &[crate::identify::Witness],
        by: IdentifiedBy,
    ) -> usize {
        record.identify_from(said, by, &mut Vec::new())
    }

    #[test]
    fn a_witness_bracketed_by_a_sitting_names_that_sittings_end_position() {
        let mut record = with_unidentified();
        // `sitting` sets `asin` on every sitting; an empty one leaves
        // `book_for` to reach the book through `end_position` alone.
        for s in &mut record.sittings {
            s.asin = None;
        }
        let identified = identify_from(
            &mut record,
            &[witness("2026-08-08T10:30:00", "The Orphan", "B00ORPHANED")],
            IdentifiedBy::Vocab,
        );
        assert_eq!(identified, 1);
        let found = record
            .book_for(500_100, None)
            .expect("the end position was identified");
        assert_eq!(found.title, "The Orphan");
        assert_eq!(found.cde_key, "B00ORPHANED", "the source stated a key");
        assert_eq!(found.identified_by, IdentifiedBy::Vocab);
        assert!(
            record.book_for(700_200, None).is_none(),
            "an unclaimed end position"
        );
    }

    #[test]
    fn an_end_position_whose_book_the_record_already_holds_is_linked_and_not_copied() {
        // One book under two end positions, filed under the older.
        let mut record = with_unidentified();
        for s in &mut record.sittings {
            s.asin = None;
        }
        let before = record.books.len();
        let identified = identify_from(
            &mut record,
            &[witness("2026-08-08T10:30:00", "a named   BOOK", "")],
            IdentifiedBy::Clippings,
        );
        assert_eq!(identified, 1);
        assert_eq!(record.books.len(), before, "a second row for one book");
        let found = record
            .book_for(500_100, None)
            .expect("the end position was identified");
        assert_eq!(found.cde_key, "B00OKPCRLG");
        assert_eq!(
            found.identified_by,
            IdentifiedBy::Library,
            "the library still holds it"
        );
        // Through the `k` row `slot_for`'s third arm consults.
        assert_eq!(record.key_at(500_100), Some("B00OKPCRLG"));
    }

    #[test]
    fn an_end_position_two_titles_claim_is_left_alone_by_every_later_source() {
        let mut record = with_unidentified();
        for s in &mut record.sittings {
            s.asin = None;
        }
        let mut contested = Vec::new();
        let identified = record.identify_from(
            &[
                witness("2026-08-08T10:10:00", "One Book", ""),
                witness("2026-08-08T10:20:00", "Another Book", ""),
            ],
            IdentifiedBy::Vocab,
            &mut contested,
        );
        assert_eq!(
            identified, 0,
            "an end position holding two books was identified"
        );
        assert_eq!(contested, vec![500_100]);
        // `IdentifiedBy::Clippings` seeing one of the two settles nothing.
        let identified = record.identify_from(
            &[witness("2026-08-08T10:30:00", "One Book", "")],
            IdentifiedBy::Clippings,
            &mut contested,
        );
        assert_eq!(
            identified, 0,
            "a contested end position was identified by a weaker source"
        );
        assert!(record.book_for(500_100, None).is_none());
    }

    #[test]
    fn a_witness_two_end_positions_bracket_names_neither() {
        let mut record = with_unidentified();
        for s in &mut record.sittings {
            s.asin = None;
        }
        // A second sitting of another end position over the same hour.
        record.sittings.push(sitting(
            "2026-08-08T10:00:00",
            "2026-08-08T11:00:00",
            700_200,
            3_600,
        ));
        record.sittings.last_mut().unwrap().asin = None;
        record.sort();
        let identified = identify_from(
            &mut record,
            &[witness("2026-08-08T10:30:00", "The Orphan", "")],
            IdentifiedBy::Vocab,
        );
        assert_eq!(identified, 0);
        assert!(record.book_for(500_100, None).is_none());
        assert!(record.book_for(700_200, None).is_none());
    }

    #[test]
    fn a_sitting_too_brief_to_identify_carries_no_claim() {
        let mut record = with_unidentified();
        for s in &mut record.sittings {
            s.asin = None;
        }
        let least = CLAIM_MIN_SECS;
        record.sittings[1].ended_at = "2026-08-08T10:00:59".into();
        record.sittings[1].seconds = least - 1;
        assert_eq!(
            identify_from(
                &mut record,
                &[witness("2026-08-08T10:00:30", "The Orphan", "")],
                IdentifiedBy::Vocab,
            ),
            0,
        );
        record.sittings[1].seconds = least;
        assert_eq!(
            identify_from(
                &mut record,
                &[witness("2026-08-08T10:00:30", "The Orphan", "")],
                IdentifiedBy::Vocab,
            ),
            1,
        );
    }

    #[test]
    fn a_lookup_past_the_books_own_end_names_nothing() {
        let mut record = with_unidentified();
        for s in &mut record.sittings {
            s.asin = None;
        }
        let mut past = witness("2026-08-08T10:30:00", "The Orphan", "");
        past.pos = 500_101;
        assert_eq!(identify_from(&mut record, &[past], IdentifiedBy::Vocab), 0);
        let mut inside = witness("2026-08-08T10:30:00", "The Orphan", "");
        inside.pos = 400_000;
        assert_eq!(
            identify_from(&mut record, &[inside], IdentifiedBy::Vocab),
            1
        );
    }

    #[test]
    fn a_book_that_arrived_as_a_file_name_gets_its_real_title() {
        // A book the sidecar arm identified from the `.sdr` directory, nothing better spoken.
        let mut record = with_unidentified();
        for s in &mut record.sittings {
            s.asin = None;
        }
        record.learn_pair(500_100, "Some_Book_File");
        record.books.push(from_sidecar(500_100, "Some_Book_File"));
        record.sort_books();
        assert_eq!(
            record.book_for(500_100, None).map(|b| b.title.as_str()),
            Some("Some_Book_File")
        );

        let identified = identify_from(
            &mut record,
            &[witness(
                "2026-08-08T10:30:00",
                "The Real Title",
                "B00REALKEY",
            )],
            IdentifiedBy::Vocab,
        );
        assert_eq!(identified, 1);
        let found = record
            .book_for(500_100, None)
            .expect("the end position is still identified");
        assert_eq!(found.title, "The Real Title");
        assert_eq!(found.identified_by, IdentifiedBy::Vocab);
        assert_eq!(
            record.books.iter().filter(|b| b.extent == 500_100).count(),
            1,
            "the file-name book was left standing beside the real one",
        );
        assert!(record.pairs.is_empty(), "the pairing still reaches it");
        // `end_positions_wanting` leaves the end position out for anything weaker.
        assert!(
            !record
                .end_positions_wanting(IdentifiedBy::Clippings)
                .contains(&500_100)
        );
        assert!(
            record
                .end_positions_wanting(IdentifiedBy::Vocab)
                .contains(&700_200)
        );
    }

    #[test]
    fn a_book_states_what_identified_it_and_a_row_written_before_that_reads_back() {
        let dir = scratch("identified-by");
        let mut record = with_unidentified();
        record.learn_pair(500_100, "Some_Book_File");
        record.books.push(from_sidecar(500_100, "Some_Book_File"));
        record.sort_books();
        record.save(&dir).expect("a written record");
        let read = Record::load(&dir);
        assert_eq!(
            read.books, record.books,
            "a `b` row lost what identified it"
        );

        // The same file with every `b` row one column short, its last field
        // gone, `pairs` picking the stem out.
        let older: String = std::fs::read_to_string(Record::file(&dir))
            .expect("a record to shorten")
            .lines()
            .map(|line| match line.starts_with("b\t") {
                true => line.rsplit_once('\t').expect("a `b` row's last field").0,
                false => line,
            })
            .collect::<Vec<_>>()
            .join("\n");
        let read = Record::from_text(&older);
        let identified: Vec<IdentifiedBy> = read.books.iter().map(|b| b.identified_by).collect();
        assert_eq!(identified, [IdentifiedBy::Library, IdentifiedBy::Sidecar]);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_source_is_asked_only_while_an_end_position_still_wants_what_it_says() {
        let mut record = with_unidentified();
        for s in &mut record.sittings {
            s.asin = None;
        }
        assert_eq!(
            record.end_positions_wanting(IdentifiedBy::Vocab),
            [500_100, 700_200]
        );
        assert_eq!(
            record.end_positions_wanting(IdentifiedBy::Sidecar),
            [500_100, 700_200]
        );
        identify_from(
            &mut record,
            &[
                witness("2026-08-08T10:30:00", "One", ""),
                witness("2026-08-09T10:30:00", "Two", ""),
            ],
            IdentifiedBy::Vocab,
        );
        // `end_positions_wanting` leaves nothing for a weaker source to walk.
        assert!(
            record
                .end_positions_wanting(IdentifiedBy::Clippings)
                .is_empty()
        );
        assert!(
            record
                .end_positions_wanting(IdentifiedBy::Sidecar)
                .is_empty()
        );
        // `IdentifiedBy::Library` outranks them all.
        assert_eq!(record.end_positions_wanting(IdentifiedBy::Library).len(), 2);
    }
}
