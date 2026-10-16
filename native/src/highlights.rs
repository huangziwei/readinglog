//! The two highlight sources joined: the `.sdr` rare sidecar is the roster of
//! the highlights a book carries, `My Clippings.txt` the journal of every write ever.
//! Neither is a superset, and their union holds deleted highlights.
use std::collections::{HashMap, HashSet};
use std::path::Path;

use crate::clippings::{Clipping, Kind};
use crate::clock::{self, Clock};
use crate::date;
use crate::identify::normalise;
use crate::record::{self, Record};
use crate::sidecar::{Roster, Sidecars, Survey};

/// Characters kept of a passage's words; a note's are kept whole.
pub const EXCERPT: usize = 600;

/// What the ellipsis on a cut body is.
const CUT: char = '…';

/// What the two sources together say about one highlight.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum State {
    /// A sidecar annotation carries this stamp: the highlight is in the book.
    Live,
    /// A sidecar in a position to carry this highlight does not.
    Retired,
    /// No sidecar could say either way.
    #[default]
    Unconfirmed,
}

impl State {
    /// The word a stored row carries.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Live => "live",
            Self::Retired => "retired",
            Self::Unconfirmed => "unconfirmed",
        }
    }

    /// What [`Self::as_str`] wrote. Any other word reads as
    /// [`State::Unconfirmed`], which is what claims the least.
    pub fn from_stored(text: &str) -> Self {
        match text.trim() {
            "live" => Self::Live,
            "retired" => Self::Retired,
            _ => Self::Unconfirmed,
        }
    }

    /// Whether the book carries this highlight, which is what [`fold`] stores.
    pub fn is_in_the_book(self) -> bool {
        self != Self::Retired
    }
}

/// One highlight in one book, as the record holds it.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct Highlight {
    /// The book's `p_contentSize`, 0 until a pass identifies the book.
    pub extent: i64,
    /// The title as the source stated it: what a re-read joins on, outliving `extent`.
    pub title: String,
    pub kind: Kind,
    /// `YYYY-MM-DDTHH:MM:SS`, Kindle-local, as a sitting stores its own.
    pub at: String,
    pub state: State,
    /// The sidecar's start on the `p_contentSize` axis, -1 where none; never a location.
    pub start: i64,
    /// The sidecar's end position, -1 where none.
    pub end: i64,
    /// The display location the clipping stated, -1 where none, on no axis of `start`'s.
    pub location: i64,
    /// The publisher's page label the clipping carried, empty where none.
    pub page: String,
    /// One of the eleven colour names a sidecar states, empty where none.
    pub colour: String,
    /// A note's whole text, or [`EXCERPT`] characters of a passage's.
    pub body: String,
}

impl Highlight {
    /// Where this highlight falls through the book. `None` without a sidecar
    /// position or a stated extent: a display location does not convert.
    pub fn through(&self, extent: i64) -> Option<f64> {
        (self.start >= 0 && extent > 0).then(|| self.start as f64 / extent as f64)
    }

    /// The day this highlight was made, as a day count.
    pub fn day(&self) -> Option<i64> {
        date::parse_day(date::day_of(&self.at))
    }

    /// What identifies this highlight across passes: the fields a clipping states,
    /// and, where no clipping placed it, the sidecar's position: highlights a
    /// sidecar alone states can share a stamp, a kind and a title.
    pub fn identity(&self) -> (&str, Kind, &str, i64, i64) {
        let start = match self.location < 0 {
            true => self.start,
            false => -1,
        };
        (&self.at, self.kind, &self.title, self.location, start)
    }

    /// Take from `held` every sidecar field this highlight states none of:
    /// `start`, `end`, `colour`, `page`, `extent` and a state over
    /// [`State::Unconfirmed`].
    pub fn keep_from(&mut self, held: &Highlight) {
        if self.start < 0 && held.start >= 0 {
            self.start = held.start;
            self.end = held.end;
        }
        if self.colour.is_empty() {
            self.colour.clone_from(&held.colour);
        }
        if self.page.is_empty() {
            self.page.clone_from(&held.page);
        }
        if self.extent == 0 {
            self.extent = held.extent;
        }
        if self.state == State::Unconfirmed {
            self.state = held.state;
        }
    }

    /// [`Self::keep_from`] the row at this highlight's sidecar place, taking
    /// too the stamp, title and location a clipping gave that row where none
    /// placed this one, and its words where this states none.
    fn keep_place_from(&mut self, held: &Highlight) {
        self.keep_from(held);
        if self.location < 0 && held.location >= 0 {
            self.at.clone_from(&held.at);
            self.title.clone_from(&held.title);
            self.location = held.location;
        }
        if self.body.is_empty() {
            self.body.clone_from(&held.body);
        }
    }

    /// Whether a sidecar stated these words: a [`State::Live`] note.
    fn states_its_own_words(&self) -> bool {
        self.state == State::Live && self.kind == Kind::Note
    }

    /// How far apart in stamp this highlight and `other` are, for choosing between
    /// two passages that cover one note equally well.
    fn created_near(&self, other: &Highlight) -> usize {
        self.at
            .bytes()
            .zip(other.at.bytes())
            .take_while(|(a, b)| a == b)
            .count()
            .wrapping_neg()
    }
}

/// One row of a book's highlights: a passage and the note written on it. A
/// note carries no link to its passage: an overlap of their ranges is the
/// only link there is, and a note with no position stands alone.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Paired<'a> {
    pub highlight: &'a Highlight,
    pub note: Option<&'a Highlight>,
}

/// `highlights` as the screens list them, in the order given: every note whose
/// range a passage covers folded into that passage's own row.
pub fn paired<'a>(highlights: &[&'a Highlight]) -> Vec<Paired<'a>> {
    // Which passage each note belongs under, and `None` for one that belongs
    // under nothing.
    let under: Vec<Option<usize>> = highlights
        .iter()
        .map(|note| match note.kind == Kind::Note {
            true => covering(highlights, note),
            false => None,
        })
        .collect();
    highlights
        .iter()
        .enumerate()
        .filter(|(at, _)| under[*at].is_none())
        .map(|(at, highlight)| Paired {
            highlight,
            note: under
                .iter()
                .position(|held| *held == Some(at))
                .map(|i| highlights[i]),
        })
        .collect()
}

/// The passage `note` sits inside, as an index into `highlights`. The narrowest
/// range that covers it wins: a note inside two nested passages goes under the
/// closer one; a tie goes to the passage made nearest in time.
fn covering(highlights: &[&Highlight], note: &Highlight) -> Option<usize> {
    if note.start < 0 {
        return None;
    }
    highlights
        .iter()
        .enumerate()
        .filter(|(_, m)| {
            m.kind != Kind::Note
                && m.start >= 0
                && m.start <= note.start
                && m.end >= note.start
                && m.extent == note.extent
        })
        .min_by_key(|(_, m)| (m.end - m.start, (m.created_near(note))))
        .map(|(at, _)| at)
}

/// What one pass over the two sources came to.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct Merge {
    /// Whether the pass read anything at all, or [`gate`] held it back.
    pub read: bool,
    /// Clippings in `My Clippings.txt`.
    pub clippings: usize,
    /// Annotations across every rare sidecar.
    pub sidecars: usize,
    /// Rare sidecars parsed by the walk, the kept rosters answering for the rest.
    pub parsed: usize,
    /// Books whose rare sidecar was in a position to say what exists.
    pub trusted: usize,
    pub live: usize,
    pub retired: usize,
    pub unconfirmed: usize,
    /// Rows the record holds after the pass.
    pub held: usize,
}

/// The join's version: a [`Gate`] stating a lower one joins the sources again.
const RULES: u32 = 7;

/// What a pass must have seen for the rows it wrote to stand: figures a walk
/// states with no sidecar parsed.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct Gate {
    /// `My Clippings.txt`'s length in bytes.
    pub len: u64,
    /// Its modification time, epoch seconds.
    pub mtime: i64,
    /// [`crate::sidecar::Survey::rare`], which is what says a highlight was made,
    /// edited or deleted since, and [`crate::sidecar::Survey::dirs`] beside it.
    pub sidecars: u64,
    pub dirs: usize,
    /// [`RULES`] at the pass that wrote the rows; a lower number merges them again.
    pub rules: u32,
}

/// The gate `clips` and `survey` stand at.
pub fn gate(clips: &Path, survey: &Survey) -> Gate {
    let stated = std::fs::metadata(clips).ok();
    Gate {
        len: stated.as_ref().map_or(0, std::fs::Metadata::len),
        mtime: stated
            .as_ref()
            .and_then(|m| m.modified().ok())
            .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
            .map_or(0, |d| d.as_secs() as i64),
        sidecars: survey.rare,
        dirs: survey.dirs,
        rules: RULES,
    }
}

/// Whether [`fold`] reads anything, answered before [`crate::sidecar::read`]
/// is paid for.
pub fn wants(record: &Record, clips: &Path, survey: &Survey) -> bool {
    record.gate != Some(gate(clips, survey))
}

/// Join the two sources into `record` through [`Record::take_highlights`],
/// `clippings` the parsed `My Clippings.txt` that
/// [`crate::identify::from_sources_at`] reads too. Nothing is read while [`gate`] holds.
pub fn fold(
    record: &mut Record,
    clips: &Path,
    clippings: &[Clipping],
    sidecars: &Sidecars,
    survey: &Survey,
) -> Merge {
    let now = gate(clips, survey);
    let mut out = Merge {
        sidecars: sidecars.rosters.iter().map(|r| r.annotations.len()).sum(),
        parsed: sidecars.parsed,
        trusted: sidecars.rosters.iter().filter(|r| r.trusted).count(),
        held: record.highlights.len(),
        ..Merge::default()
    };
    if record.gate == Some(now) {
        return out;
    }
    out.read = true;
    out.clippings = clippings.len();
    let (highlights, spoken) = merged(clippings, sidecars, &record.books, &record.clock);
    for highlight in &highlights {
        match highlight.state {
            State::Live => out.live += 1,
            State::Retired => out.retired += 1,
            State::Unconfirmed => out.unconfirmed += 1,
        }
    }
    // A `State::Retired` row is counted here and stored nowhere.
    let held: Vec<Highlight> = highlights
        .into_iter()
        .filter(|m| m.state.is_in_the_book())
        .collect();
    record.take_highlights(held, &spoken, now);
    out.held = record.highlights.len();
    out
}

/// The books one pass read a trusted roster for: by extent, and by title for a
/// row naming no extent.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct Spoken {
    extents: HashSet<i64>,
    titles: HashSet<String>,
}

impl Spoken {
    /// Whether a roster this pass read spoke for `highlight`'s book.
    pub fn for_book_of(&self, highlight: &Highlight) -> bool {
        match highlight.extent {
            0 => self.titles.contains(&normalise(&highlight.title)),
            extent => self.extents.contains(&extent),
        }
    }
}

/// `fresh`, one pass's rows, over `held`, the record's: a fresh row takes what
/// it lacks from the held row of its identity, sidecar place or stamp; a book
/// `spoken` names takes the pass's rows, any other keeps its own.
pub fn over(held: &[Highlight], fresh: Vec<Highlight>, spoken: &Spoken) -> Vec<Highlight> {
    let mut by_identity: HashMap<(&str, Kind, &str, i64, i64), &Highlight> =
        HashMap::with_capacity(held.len());
    let mut by_place: HashMap<(i64, Kind, i64, i64), &Highlight> = HashMap::new();
    let mut by_book: HashMap<(i64, String, Kind), Vec<&Highlight>> = HashMap::new();
    // A held row a clipping placed, by the second and kind it was made at:
    // `None` where two share one.
    let mut by_stamp: HashMap<(&str, Kind), Option<&Highlight>> = HashMap::new();
    for row in held {
        by_identity.entry(row.identity()).or_insert(row);
        if row.start >= 0 {
            by_place
                .entry((row.extent, row.kind, row.start, row.end))
                .or_insert(row);
        }
        if row.location >= 0 {
            by_stamp
                .entry((row.at.as_str(), row.kind))
                .and_modify(|one| *one = None)
                .or_insert(Some(row));
        }
        by_book.entry(book_of(row)).or_default().push(row);
    }
    // The fresh rows only a sidecar placed, counted by the same.
    let mut unplaced: HashMap<(String, Kind), usize> = HashMap::new();
    for row in fresh.iter().filter(|row| row.location < 0) {
        *unplaced.entry((row.at.clone(), row.kind)).or_default() += 1;
    }
    // The held rows a fresh one stands in place of, in a book no roster spoke for.
    let mut replaced: HashSet<(&str, Kind, &str, i64, i64)> = HashSet::new();
    let mut out = Vec::with_capacity(fresh.len().max(held.len()));
    for mut row in fresh {
        let spoke = spoken.for_book_of(&row);
        match by_identity.get(&row.identity()).copied() {
            Some(stored) if !spoke && stored.state == State::Live => continue,
            Some(stored) => {
                row.keep_from(stored);
                replaced.insert(stored.identity());
            }
            None => {
                let placed = (row.start >= 0)
                    .then(|| by_place.get(&(row.extent, row.kind, row.start, row.end)))
                    .flatten()
                    .copied();
                let stamped = (row.location < 0
                    && unplaced.get(&(row.at.clone(), row.kind)) == Some(&1))
                .then(|| {
                    by_stamp
                        .get(&(row.at.as_str(), row.kind))
                        .copied()
                        .flatten()
                })
                .flatten();
                match placed.or(stamped) {
                    Some(stored) => {
                        row.keep_place_from(stored);
                        replaced.insert(stored.identity());
                    }
                    None if !spoke
                        && by_book.get(&book_of(&row)).is_some_and(|rows| {
                            rows.iter().any(|stored| words_hold(stored, &row))
                        }) =>
                    {
                        continue;
                    }
                    None => {}
                }
            }
        }
        out.push(row);
    }
    out.extend(
        held.iter()
            .filter(|row| !spoken.for_book_of(row) && !replaced.contains(&row.identity()))
            .cloned(),
    );
    out
}

/// What [`over`] groups a row's book and kind by: the extent, and the title
/// for a row naming none.
fn book_of(highlight: &Highlight) -> (i64, String, Kind) {
    let title = match highlight.extent {
        0 => normalise(&highlight.title),
        _ => String::new(),
    };
    (highlight.extent, title, highlight.kind)
}

/// One roster, with the book it belongs to and which of its annotations a clipping
/// has claimed.
struct Held<'a> {
    roster: &'a Roster,
    /// The [`record::Book`] this roster belongs to, where a `p_location` names it.
    book: Option<&'a record::Book>,
    /// `created` as epoch seconds per annotation, placed on a clock to meet a clipping.
    created: Vec<i64>,
    /// The offset each annotation's pair settled on, `None` where no clipping claimed it.
    offset: Vec<Option<i64>>,
    claimed: Vec<bool>,
}

impl Held<'_> {
    /// The wall clock annotation `i` is read on: the offset its own pair settled
    /// on, else the best the record and the zone file can offer.
    fn at(&self, i: usize, clock: &Clock, offsets: &[i64]) -> String {
        let epoch = self.created[i];
        match self.offset[i] {
            Some(offset) => date::local_at(epoch, Some(offset))
                .map(|(d, s)| date::stamp(d, s))
                .unwrap_or_default(),
            None => match offsets.first() {
                Some(offset) if clock.is_empty() => date::local_at(epoch, Some(*offset))
                    .map(|(d, s)| date::stamp(d, s))
                    .unwrap_or_default(),
                _ => clock::stamp(clock, epoch),
            },
        }
    }
}

/// [`fold`]'s arithmetic, over `clippings` and `sidecars` as read, and the
/// books a trusted roster among them spoke for.
fn merged(
    clippings: &[Clipping],
    sidecars: &Sidecars,
    books: &[record::Book],
    clock: &Clock,
) -> (Vec<Highlight>, Spoken) {
    // Both ways into `books`, built once.
    let by_stem = stems(books);
    let by_title = titles(books);
    let mut held: Vec<Held> = sidecars
        .rosters
        .iter()
        .map(|roster| Held {
            book: by_stem.get(roster.file.as_str()).map(|&at| &books[at]),
            created: roster
                .annotations
                .iter()
                .map(|a| a.created.div_euclid(1_000))
                .collect(),
            offset: vec![None; roster.annotations.len()],
            claimed: vec![false; roster.annotations.len()],
            roster,
        })
        .collect();

    // The record's own clocks first, then the zone file's offset for this instant,
    // all a Kindle with no annotations to measure has.
    let mut offsets = clock.offsets();
    if let Some(now) = crate::zone::offset_at(date::epoch_now())
        && !offsets.contains(&now)
    {
        offsets.push(now);
    }

    // The book each clipping names, read once for both joins.
    let named: Vec<Option<&record::Book>> = clippings
        .iter()
        .map(|clip| match clip.title.is_empty() {
            true => None,
            false => by_title
                .get(&normalise(&clip.title))
                .copied()
                .flatten()
                .map(|at| &books[at]),
        })
        .collect();

    // The stamp first, which is exact wherever it holds.
    let mut paired: Vec<Option<(usize, usize)>> = Vec::with_capacity(clippings.len());
    for (clip, named) in clippings.iter().zip(&named) {
        let found = matching(clip, &held, *named, &offsets);
        if let Some((at, i, offset)) = found {
            held[at].claimed[i] = true;
            held[at].offset[i] = Some(offset);
        }
        paired.push(found.map(|(at, i, _)| (at, i)));
    }
    in_order(clippings, &named, &mut held, &mut paired);

    let mut out: Vec<Highlight> = Vec::new();
    for ((clip, named), found) in clippings.iter().zip(&named).zip(&paired) {
        out.push(one(clip, *named, *found, &held));
    }
    // Every sidecar annotation no clipping speaks for is a highlight the file cannot
    // reach — handwriting, which the file never holds, or an emptied
    // `My Clippings.txt`. It is stored without words.
    for at in &held {
        for (i, annotation) in at.roster.annotations.iter().enumerate() {
            if at.claimed[i] {
                continue;
            }
            out.push(Highlight {
                extent: at.book.map_or(0, |b| b.extent),
                title: at
                    .book
                    .map(|b| b.title.clone())
                    .unwrap_or_else(|| at.roster.file.clone()),
                kind: annotation.kind,
                at: at.at(i, clock, &offsets),
                state: State::Live,
                start: annotation.start_position().unwrap_or(-1),
                end: annotation.end_position().unwrap_or(-1),
                location: -1,
                page: String::new(),
                colour: annotation.colour.clone(),
                body: annotation.body.clone(),
            });
        }
    }
    let mut spoken = Spoken::default();
    for at in held.iter().filter(|h| h.roster.trusted) {
        match at.book {
            Some(book) => {
                if book.extent != 0 {
                    spoken.extents.insert(book.extent);
                }
                spoken.titles.insert(normalise(&book.title));
            }
            None => {
                spoken.titles.insert(normalise(&at.roster.file));
            }
        }
    }
    // The bodies are compared whole and cut after: two passages differing
    // past [`EXCERPT`] are two highlights.
    let settled = settle(out)
        .into_iter()
        .map(|mut highlight| {
            highlight.body = bounded(highlight.kind, &highlight.body);
            highlight
        })
        .collect();
    (settled, spoken)
}

/// The second join, for a sidecar written anew whose restamped annotations miss the
/// clippings: within one book and one kind, equal counts of leftovers
/// pair k-th to k-th in reading order, and unequal counts pair nothing.
fn in_order(
    clippings: &[Clipping],
    named: &[Option<&record::Book>],
    held: &mut [Held],
    paired: &mut [Option<(usize, usize)>],
) {
    for (at, one) in held.iter_mut().enumerate() {
        let Some(extent) = one.book.map(|b| b.extent) else {
            continue;
        };
        for kind in Kind::ALL {
            // The sidecar's leftovers, up the book.
            let mut mine: Vec<usize> = (0..one.roster.annotations.len())
                .filter(|i| !one.claimed[*i] && one.roster.annotations[*i].kind == kind)
                .collect();
            if mine.is_empty() {
                continue;
            }
            // The file's leftovers for the same book and kind, up the book.
            let mut theirs: Vec<usize> = (0..clippings.len())
                .filter(|c| {
                    paired[*c].is_none()
                        && clippings[*c].kind == kind
                        && named[*c].is_some_and(|b| b.extent == extent)
                })
                .collect();
            if mine.len() != theirs.len() {
                continue;
            }
            mine.sort_by_key(|i| {
                let annotation = &one.roster.annotations[*i];
                (
                    annotation.start_position().unwrap_or(i64::MAX),
                    annotation.created,
                )
            });
            theirs.sort_by_key(|c| (clippings[*c].start, clippings[*c].at.clone()));
            for (i, c) in mine.into_iter().zip(theirs) {
                one.claimed[i] = true;
                paired[c] = Some((at, i));
            }
        }
    }
}

/// One clipping as a [`Highlight`], taking the positions, the colour and a
/// note's words from the sidecar annotation `found` names, where it names one.
fn one(
    clip: &Clipping,
    named: Option<&record::Book>,
    found: Option<(usize, usize)>,
    held: &[Held],
) -> Highlight {
    let mut highlight = Highlight {
        extent: named.map_or(0, |b| b.extent),
        title: clip.title.clone(),
        kind: clip.kind,
        at: clip.at.clone(),
        state: State::Unconfirmed,
        start: -1,
        end: -1,
        location: clip.start,
        page: clip.page.clone(),
        colour: String::new(),
        body: clip.body.clone(),
    };
    match found {
        Some((at, i)) => {
            let annotation = &held[at].roster.annotations[i];
            highlight.state = State::Live;
            highlight.start = annotation.start_position().unwrap_or(-1);
            highlight.end = annotation.end_position().unwrap_or(-1);
            highlight.colour = annotation.colour.clone();
            // `annotation.body` is a note's words as last saved, the clipping's as first saved.
            if !annotation.body.is_empty() {
                highlight.body = annotation.body.clone();
            }
            // The roster names the book the clipping only described.
            if let Some(book) = held[at].book {
                highlight.extent = book.extent;
            }
        }
        // Absence means deletion only where a sidecar is in a position to
        // say so. A book with none, or an unparsed one, says nothing.
        None => {
            let roster = held
                .iter()
                .find(|h| h.book.is_some() && h.book.map(|b| b.extent) == named.map(|b| b.extent));
            if roster.is_some_and(|h| h.roster.trusted) {
                highlight.state = State::Retired;
            }
        }
    }
    highlight
}

/// The sidecar annotation carrying `clip`'s kind and instant, and which of
/// `offsets` placed it there. A stamp is unique down the file: one match
/// settles it; a tie the named book cannot break is left unmatched.
fn matching(
    clip: &Clipping,
    held: &[Held],
    named: Option<&record::Book>,
    offsets: &[i64],
) -> Option<(usize, usize, i64)> {
    let wall = instant(&clip.at)?;
    for offset in offsets {
        let found: Vec<(usize, usize)> = held
            .iter()
            .enumerate()
            .flat_map(|(at, h)| {
                h.roster
                    .annotations
                    .iter()
                    .enumerate()
                    .filter(move |(i, a)| {
                        !h.claimed[*i] && a.kind == clip.kind && h.created[*i] + offset == wall
                    })
                    .map(move |(i, _)| (at, i))
            })
            .collect();
        let settled = match found.as_slice() {
            [] => continue,
            [one] => Some(*one),
            several => several
                .iter()
                .find(|(at, _)| {
                    held[*at].book.map(|b| b.extent) == named.map(|b| b.extent) && named.is_some()
                })
                .copied(),
        };
        if let Some((at, i)) = settled {
            return Some((at, i, *offset));
        }
    }
    None
}

/// A `YYYY-MM-DDTHH:MM:SS` as seconds, the axis a wall clock and a placed
/// epoch are compared on.
fn instant(at: &str) -> Option<i64> {
    let day = date::parse_day(date::day_of(at))?;
    Some(day * 86_400 + date::secs_of(at))
}

/// The offsets measured off the one instant written twice: `My Clippings.txt`
/// in wall clock, the sidecar in epoch milliseconds. Best-supported first, and
/// never below [`SUPPORT`].
fn offsets_seen(clippings: &[Clipping], sidecars: &Sidecars) -> Vec<i64> {
    // The file's instants by kind, ascending: one annotation reads only the
    // clippings a clock could reach — never the whole file.
    let mut by_kind: HashMap<Kind, Vec<i64>> = HashMap::new();
    for clip in clippings {
        if let Some(at) = instant(&clip.at) {
            by_kind.entry(clip.kind).or_default().push(at);
        }
    }
    for found in by_kind.values_mut() {
        found.sort_unstable();
    }
    let mut count: HashMap<i64, usize> = HashMap::new();
    for roster in &sidecars.rosters {
        for annotation in &roster.annotations {
            let created = annotation.created.div_euclid(1_000);
            let Some(found) = by_kind.get(&annotation.kind) else {
                continue;
            };
            let from = found.partition_point(|at| *at < created - clock::BOUND_SECS);
            let to = found.partition_point(|at| *at <= created + clock::BOUND_SECS);
            for at in &found[from..to] {
                let offset = at - created;
                if clock::plausible(offset) {
                    *count.entry(offset).or_default() += 1;
                }
            }
        }
    }
    let mut out: Vec<(i64, usize)> = count.into_iter().filter(|(_, n)| *n >= SUPPORT).collect();
    // Most-supported first, the larger offset first on a tie: one order every time.
    out.sort_by(|a, b| b.1.cmp(&a.1).then(b.0.cmp(&a.0)));
    out.into_iter().map(|(offset, _)| offset).collect()
}

/// How many annotations a difference must stand for before it is read as a clock.
const SUPPORT: usize = 2;

/// `(instant, offset)` per annotation the two files agree about. Must run before
/// any pass placing a true epoch on [`Clock`], [`merged`] included.
pub fn clocks_seen(clippings: &[Clipping], sidecars: &Sidecars) -> Vec<(i64, i64)> {
    let offsets = offsets_seen(clippings, sidecars);
    if offsets.is_empty() {
        return Vec::new();
    }
    // A second holds one clipping: the file is appended from one thread.
    let mut by_instant: HashMap<(Kind, i64), ()> = HashMap::new();
    for clip in clippings {
        if let Some(at) = instant(&clip.at) {
            by_instant.insert((clip.kind, at), ());
        }
    }
    let mut out = Vec::new();
    for roster in &sidecars.rosters {
        for annotation in &roster.annotations {
            let created = annotation.created.div_euclid(1_000);
            // The best-supported clock that lands this annotation on a write the
            // file holds. An annotation no clock places states nothing.
            if let Some(offset) = offsets
                .iter()
                .find(|offset| by_instant.contains_key(&(annotation.kind, created + *offset)))
            {
                out.push((created, *offset));
            }
        }
    }
    out
}

/// Each book's `.sdr` name — its path, directory and last suffix cut — to
/// its slot, first of a run winning.
fn stems(books: &[record::Book]) -> HashMap<&str, usize> {
    let mut out = HashMap::with_capacity(books.len());
    for (at, book) in books.iter().enumerate() {
        if !book.location.is_empty() {
            out.entry(stem_of(&book.location)).or_insert(at);
        }
    }
    out
}

/// Each book's title to its slot, matched the way two claims on one end position
/// are compared, and to `None` where more than one book carries it: a title
/// two books answer to names neither.
fn titles(books: &[record::Book]) -> HashMap<String, Option<usize>> {
    let mut out: HashMap<String, Option<usize>> = HashMap::with_capacity(books.len());
    for (at, book) in books.iter().enumerate() {
        out.entry(normalise(&book.title))
            .and_modify(|held| *held = None)
            .or_insert(Some(at));
    }
    out
}

/// A book file's path with its directory and its last suffix cut.
fn stem_of(location: &str) -> &str {
    let name = location.rsplit('/').next().unwrap_or(location);
    match name.rsplit_once('.') {
        Some((stem, _)) => stem,
        None => name,
    }
}

/// `body` as the record keeps it: whole for [`Kind::Note`], and
/// [`EXCERPT`] characters for the book's own words.
fn bounded(kind: Kind, body: &str) -> String {
    if kind.is_the_readers_own() || body.chars().count() <= EXCERPT {
        return body.to_string();
    }
    let mut out: String = body.chars().take(EXCERPT).collect();
    out.push(CUT);
    out
}

/// The rows one merge came to, deduplicated by the sidecar's `(start, end)`
/// where it stated one, and by `(title, kind, body)` without.
fn settle(mut highlights: Vec<Highlight>) -> Vec<Highlight> {
    // Earliest first: the row kept is the one stating when the highlight was
    // made, and the bodies that follow are the later words.
    highlights.sort_by(|a, b| {
        (&a.at, &a.title, a.kind, a.start).cmp(&(&b.at, &b.title, b.kind, b.start))
    });

    let mut out: Vec<Highlight> = Vec::new();
    // Two ways back into `out`, neither arm scanning it. `standing` is the
    // exact arm's whole test; `under` is the only pair `words_hold` can be
    // asked about.
    let mut standing: HashMap<(i64, Kind, i64, i64), usize> = HashMap::new();
    let mut under: HashMap<(String, Kind), Vec<usize>> = HashMap::new();
    for highlight in highlights {
        let title = normalise(&highlight.title);
        let at = match highlight.state {
            State::Live => (highlight.start >= 0)
                .then(|| {
                    standing.get(&(
                        highlight.extent,
                        highlight.kind,
                        highlight.start,
                        highlight.end,
                    ))
                })
                .flatten()
                .copied(),
            _ => under
                .get(&(title.clone(), highlight.kind))
                .into_iter()
                .flatten()
                .copied()
                .find(|&held| words_hold(&out[held], &highlight)),
        };
        match at {
            // The held row opened the highlight; the later one restates its
            // words, but for a note whose words a sidecar states.
            Some(at) if !highlight.body.is_empty() && !out[at].states_its_own_words() => {
                out[at].body = highlight.body
            }
            Some(_) => {}
            None => {
                let slot = out.len();
                if highlight.state == State::Live && highlight.start >= 0 {
                    standing
                        .entry((
                            highlight.extent,
                            highlight.kind,
                            highlight.start,
                            highlight.end,
                        ))
                        .or_insert(slot);
                }
                under.entry((title, highlight.kind)).or_default().push(slot);
                out.push(highlight);
            }
        }
    }
    // A row under `State::Retired` or `State::Unconfirmed` whose words
    // a sidecar holds is an earlier write, and `under` names it.
    let keep: Vec<bool> = out
        .iter()
        .map(|m| {
            m.state == State::Live
                || m.body.is_empty()
                || !under
                    .get(&(normalise(&m.title), m.kind))
                    .into_iter()
                    .flatten()
                    .any(|&held| {
                        out[held].state == State::Live
                            && !out[held].body.is_empty()
                            && words_hold(&out[held], m)
                    })
        })
        .collect();
    let mut keep = keep.into_iter();
    out.retain(|_| keep.next().unwrap_or(true));
    out
}

/// Whether either passage holds the other, the book and kind being the
/// caller's.
fn words_hold(a: &Highlight, b: &Highlight) -> bool {
    if a.body.is_empty() || b.body.is_empty() {
        return false;
    }
    let (short, long) = match a.body.chars().count() <= b.body.chars().count() {
        true => (&a.body, &b.body),
        false => (&b.body, &a.body),
    };
    long.contains(short.as_str())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// [`merged`] with the clocks these two files state, answering highlights alone.
    fn merge(
        clippings: &[Clipping],
        sidecars: &Sidecars,
        books: &[record::Book],
    ) -> Vec<Highlight> {
        let mut clock = Clock::default();
        clock.observe(clocks_seen(clippings, sidecars));
        merged(clippings, sidecars, books, &clock).0
    }

    use crate::sidecar::Annotation;

    /// A [`record::Book`] carrying `p_location`.
    fn book(extent: i64, title: &str, file: &str) -> record::Book {
        record::Book {
            extent,
            cde_key: format!("KEY{extent}"),
            title: title.into(),
            location: format!("/mnt/us/documents/{file}.kfx"),
            on_kindle: true,
            percent: -1.0,
            read_state: -1,
            ..record::Book::default()
        }
    }

    /// A highlight over a known range, for the pairing below.
    fn highlight(state: State, start: i64, end: i64) -> Highlight {
        Highlight {
            extent: 1_000,
            title: "A Book".into(),
            kind: Kind::Highlight,
            at: "2026-09-04T00:52:49".into(),
            state,
            start,
            end,
            location: 30,
            page: String::new(),
            colour: "orange".into(),
            body: "a line".into(),
        }
    }

    /// A clipping stating everything one carries.
    fn clip(title: &str, kind: Kind, at: &str, location: i64, body: &str) -> Clipping {
        Clipping {
            title: title.into(),
            author: String::new(),
            kind,
            page: String::new(),
            start: location,
            end: location,
            at: at.into(),
            body: body.into(),
        }
    }

    /// The epoch milliseconds whose wall clock is `at`, for writing a sidecar annotation
    /// and a clipping that meet.
    fn created(at: &str) -> i64 {
        let day = date::parse_day(date::day_of(at)).expect("a day");
        let local = day * 86_400 + date::secs_of(at);
        // The merge measures the offset back off this same difference.
        let offset = crate::zone::offset_at(local).unwrap_or_else(|| {
            let (d, s) = date::local_of(0).unwrap_or((0, 0));
            d * 86_400 + s
        });
        (local - offset) * 1_000
    }

    fn annotation(kind: Kind, at: &str, start: i64, end: i64, colour: &str) -> Annotation {
        Annotation {
            kind,
            start: format!("AQEAAAAAAAAA:{start}"),
            end: format!("AQEAAAAAAAAA:{end}"),
            created: created(at),
            modified: created(at),
            flags: "0\u{FFFC}0".into(),
            colour: colour.into(),
            body: String::new(),
        }
    }

    fn roster(file: &str, trusted: bool, annotations: Vec<Annotation>) -> Roster {
        Roster {
            file: file.into(),
            trusted,
            annotations,
        }
    }

    fn sidecars_of(rosters: Vec<Roster>) -> Sidecars {
        Sidecars {
            rosters,
            ..Sidecars::default()
        }
    }

    #[test]
    fn a_clipping_and_a_sidecar_annotation_meet_on_the_second() {
        let books = [book(1000, "A Book", "A Book")];
        let sidecars = sidecars_of(vec![roster(
            "A Book",
            true,
            vec![annotation(
                Kind::Highlight,
                "2026-08-08T13:27:38",
                500,
                560,
                "orange",
            )],
        )]);
        let got = merge(
            &[clip(
                "A Book",
                Kind::Highlight,
                "2026-08-08T13:27:38",
                69,
                "a line",
            )],
            &sidecars,
            &books,
        );
        assert_eq!(got.len(), 1);
        assert_eq!(got[0].state, State::Live);
        // The words come from the file, the position and the colour from the
        // sidecar, and neither source holds both.
        assert_eq!(got[0].body, "a line");
        assert_eq!((got[0].start, got[0].end), (500, 560));
        assert_eq!(got[0].colour, "orange");
        assert_eq!(got[0].location, 69, "the display location stays its own");
        assert_eq!(got[0].extent, 1000);
    }

    #[test]
    fn a_highlight_the_reader_deleted_is_retired_and_not_a_ghost() {
        // `My Clippings.txt` keeps a cleared bookmark; no sidecar holds
        // one.
        let books = [book(1000, "A Book", "A Book")];
        let sidecars = sidecars_of(vec![roster("A Book", true, Vec::new())]);
        let got = merge(
            &[clip(
                "A Book",
                Kind::Bookmark,
                "2026-08-08T13:27:38",
                69,
                "",
            )],
            &sidecars,
            &books,
        );
        assert_eq!(got.len(), 1);
        assert_eq!(got[0].state, State::Retired);
    }

    /// A note saved with a typo and corrected on the Kindle: the sidecar
    /// holds the correction, and the clippings file both versions.
    fn corrected_note() -> (Vec<record::Book>, Sidecars, Vec<Clipping>) {
        let title = "A Therapeutic Journey: Lessons from The School of Life";
        let typo = "Do I need to \"imagine\" letting everyone done? Or did I already?";
        let fixed = "Do I need to \"imagine\" letting everyone down? Or did I already?";
        let passage = "This is a book about getting unwell, imagining that we have let \
                       everyone down\u{2014}and losing direction and hope.";
        let books = vec![book(501_352, title, "A Therapeutic Journey")];
        let sidecars = sidecars_of(vec![roster(
            "A Therapeutic Journey",
            true,
            vec![
                Annotation {
                    body: fixed.into(),
                    ..annotation(Kind::Note, "2026-10-07T09:46:44", 2172, 2176, "")
                },
                annotation(Kind::Highlight, "2026-10-07T09:46:51", 2069, 2176, "yellow"),
            ],
        )]);
        let clippings = vec![
            clip(title, Kind::Note, "2026-10-07T09:46:44", 42, typo),
            clip(title, Kind::Highlight, "2026-10-07T09:46:51", 42, passage),
            clip(title, Kind::Note, "2026-10-12T20:03:10", 42, fixed),
        ];
        (books, sidecars, clippings)
    }

    #[test]
    fn a_note_states_the_words_its_sidecar_holds() {
        let (books, sidecars, clippings) = corrected_note();
        let got: Vec<Highlight> = merge(&clippings, &sidecars, &books)
            .into_iter()
            .filter(|m| m.state.is_in_the_book())
            .collect();
        assert_eq!(got.len(), 2, "{got:#?}");
        let note = got.iter().find(|m| m.kind == Kind::Note).expect("the note");
        assert_eq!(
            note.body,
            "Do I need to \"imagine\" letting everyone down? Or did I already?"
        );
        // The note keeps the stamp it was made at, which is what names it.
        assert_eq!(note.at, "2026-10-07T09:46:44");
        assert_eq!(
            (note.state, note.start, note.end),
            (State::Live, 2172, 2176)
        );
        assert_eq!(note.location, 42);
    }

    /// One launch's [`fold`] of `clippings` and `sidecars` into `record`, its
    /// gate opened by `launch`.
    fn launch(record: &mut Record, clippings: &[Clipping], sidecars: &Sidecars, launch: u64) {
        record.clock.observe(clocks_seen(clippings, sidecars));
        let survey = Survey {
            dirs: sidecars.rosters.len(),
            stamp: launch,
            rare: launch,
        };
        fold(
            record,
            Path::new("/nonexistent/My Clippings.txt"),
            clippings,
            sidecars,
            &survey,
        );
    }

    #[test]
    fn a_corrected_note_outlives_its_book_and_is_one_note() {
        let (books, sidecars, clippings) = corrected_note();
        let mut record = Record {
            books,
            ..Record::default()
        };
        launch(&mut record, &clippings, &sidecars, 1);
        let before = record.highlights.clone();
        assert_eq!(before.len(), 2, "{before:#?}");
        // The book and its `.sdr` deleted: the clippings file alone speaks.
        launch(&mut record, &clippings, &sidecars_of(Vec::new()), 2);
        assert_eq!(record.highlights, before, "{:#?}", record.highlights);
    }

    #[test]
    fn an_emptied_clippings_file_takes_no_words() {
        let (books, sidecars, clippings) = corrected_note();
        let mut record = Record {
            books,
            ..Record::default()
        };
        launch(&mut record, &clippings, &sidecars, 1);
        let before = record.highlights.clone();
        launch(&mut record, &[], &sidecars, 2);
        assert_eq!(record.highlights, before, "{:#?}", record.highlights);
    }

    #[test]
    fn a_book_gone_from_the_kindle_keeps_its_words_once_the_clippings_file_goes() {
        // The library names no file for the book, and its `.sdr` stays: no
        // roster reaches the book by the path.
        let books = vec![record::Book {
            location: String::new(),
            on_kindle: false,
            ..book(1000, "A Book", "A Book")
        }];
        let sidecars = sidecars_of(vec![roster(
            "[An Author] A Book.8e24",
            true,
            vec![annotation(
                Kind::Highlight,
                "2026-08-29T01:54:22",
                5274,
                5353,
                "orange",
            )],
        )]);
        let clippings = [clip(
            "A Book",
            Kind::Highlight,
            "2026-08-29T01:54:22",
            86,
            "a line the book no longer on the Kindle holds",
        )];
        let mut record = Record {
            books,
            ..Record::default()
        };
        launch(&mut record, &clippings, &sidecars, 1);
        let before = record.highlights.clone();
        assert_eq!(before.len(), 1, "{before:#?}");
        launch(&mut record, &[], &sidecars, 2);
        assert_eq!(record.highlights, before, "{:#?}", record.highlights);
    }

    #[test]
    fn a_widened_highlight_keeps_its_place_once_its_book_is_gone() {
        let books = vec![book(1000, "A Book", "A Book")];
        let widened = sidecars_of(vec![roster(
            "A Book",
            true,
            vec![annotation(
                Kind::Highlight,
                "2026-08-01T17:49:58",
                100,
                200,
                "orange",
            )],
        )]);
        let clippings = [
            clip(
                "A Book",
                Kind::Highlight,
                "2026-08-01T17:49:48",
                10,
                "the war had come",
            ),
            clip(
                "A Book",
                Kind::Highlight,
                "2026-08-01T17:49:58",
                10,
                "no one remembered why the war had come about",
            ),
        ];
        let mut record = Record {
            books,
            ..Record::default()
        };
        launch(&mut record, &clippings, &widened, 1);
        let before = record.highlights.clone();
        assert_eq!(before.len(), 1, "{before:#?}");
        assert_eq!((before[0].start, before[0].end), (100, 200));
        launch(&mut record, &clippings, &sidecars_of(Vec::new()), 2);
        assert_eq!(record.highlights, before, "{:#?}", record.highlights);
    }

    #[test]
    fn a_book_whose_sidecar_is_gone_says_nothing_either_way() {
        let books = [book(1000, "A Book", "A Book")];
        // The `.sdr` is there and holds nothing that parses: it was in no
        // position to state a roster.
        let sidecars = sidecars_of(vec![roster("A Book", false, Vec::new())]);
        let got = merge(
            &[clip(
                "A Book",
                Kind::Highlight,
                "2026-08-08T13:27:38",
                69,
                "a line",
            )],
            &sidecars,
            &books,
        );
        assert_eq!(got[0].state, State::Unconfirmed);

        // And a book with no `.sdr` at all is the same answer.
        let got = merge(
            &[clip(
                "A Book",
                Kind::Highlight,
                "2026-08-08T13:27:38",
                69,
                "a line",
            )],
            &sidecars_of(Vec::new()),
            &books,
        );
        assert_eq!(got[0].state, State::Unconfirmed);
    }

    #[test]
    fn three_highlights_at_one_location_are_three_rows() {
        // Six clippings, three distinct bodies, every one at Location 69, on the
        // book whose sidecar is gone.
        let said = [
            "窓辺には花が飾られていた。",
            "太い茎がすっと伸び、",
            "茎の先でいくつも白い花が咲いている。",
        ];
        let at = [
            "2026-08-08T13:27:38",
            "2026-08-08T13:27:41",
            "2026-08-08T13:27:44",
            "2026-08-08T13:27:47",
            "2026-08-08T13:27:51",
            "2026-08-08T13:27:54",
        ];
        let clippings: Vec<Clipping> = at
            .iter()
            .enumerate()
            .map(|(i, at)| clip("透明な夜に", Kind::Highlight, at, 69, said[i / 2]))
            .collect();
        let got = merge(&clippings, &sidecars_of(Vec::new()), &[]);
        assert_eq!(got.len(), 3, "{got:#?}");
        // Each row opens at the first of its pair: that is when the highlight was
        // made, and the second clipping is the same write appended again.
        let stamps: Vec<&str> = got.iter().map(|m| m.at.as_str()).collect();
        assert_eq!(stamps, [at[0], at[2], at[4]]);
    }

    #[test]
    fn an_extended_selection_is_one_highlight_at_the_words_it_became() {
        // A dragged handle files the passage twice, the shorter inside
        // the longer. One highlight stands, at the sidecar's own words.
        let books = [book(1000, "觀念史研究", "觀念史研究")];
        let sidecars = sidecars_of(vec![roster(
            "觀念史研究",
            true,
            vec![annotation(
                Kind::Highlight,
                "2026-05-18T22:56:49",
                7874,
                7986,
                "orange",
            )],
        )]);
        let got = merge(
            &[
                clip(
                    "觀念史研究",
                    Kind::Highlight,
                    "2026-05-18T22:56:38",
                    156,
                    "觀念是指人用某一個關鍵詞所表達的思想。",
                ),
                clip(
                    "觀念史研究",
                    Kind::Highlight,
                    "2026-05-18T22:56:49",
                    156,
                    "觀念是指人用某一個關鍵詞所表達的思想。細一點講…",
                ),
            ],
            &sidecars,
            &books,
        );
        assert_eq!(got.len(), 1, "{got:#?}");
        assert_eq!(got[0].state, State::Live);
        assert_eq!(got[0].at, "2026-05-18T22:56:49");
    }

    #[test]
    fn a_passage_widened_with_no_sidecar_to_say_so_is_still_one_highlight() {
        // No `.sdr` says which of the three the book holds; containment
        // does: each inside the next is one passage widened twice, opened when
        // the first was made.
        let said = [
            "The AI\u{2019}s primary job is to decrease the wage",
            "The AI\u{2019}s primary job is to decrease the wage bill",
            "The AI\u{2019}s primary job is to decrease the wage bill, and it does",
        ];
        let got = merge(
            &said
                .iter()
                .enumerate()
                .map(|(i, body)| {
                    clip(
                        "A Book",
                        Kind::Highlight,
                        &format!("2026-07-14T11:38:{:02}", 24 + i * 3),
                        315,
                        body,
                    )
                })
                .collect::<Vec<Clipping>>(),
            &sidecars_of(Vec::new()),
            &[],
        );
        assert_eq!(got.len(), 1, "{got:#?}");
        assert_eq!(got[0].at, "2026-07-14T11:38:24");
        assert_eq!(got[0].body, said[2]);
    }

    #[test]
    fn two_passages_that_hold_neither_other_stay_two_highlights() {
        // Distinct highlights a few seconds apart at one display location: the
        // stamps cannot tell them from a revision and the location is wide
        // enough for both: only the words can, and these share none.
        let got = merge(
            &[
                clip(
                    "A Book",
                    Kind::Highlight,
                    "2026-08-08T13:27:38",
                    69,
                    "窓辺には花が飾られていた。",
                ),
                clip(
                    "A Book",
                    Kind::Highlight,
                    "2026-08-08T13:27:44",
                    69,
                    "太い茎がすっと伸び、",
                ),
                clip(
                    "A Book",
                    Kind::Highlight,
                    "2026-08-08T13:27:51",
                    69,
                    "茎の先でいくつも白い花が咲いている。",
                ),
            ],
            &sidecars_of(Vec::new()),
            &[],
        );
        assert_eq!(got.len(), 3, "{got:#?}");
    }

    #[test]
    fn an_update_writing_the_same_words_again_is_one_highlight() {
        // An update appends a whole second clipping:
        // the file carries the words twice and the sidecar carries one.
        let books = [book(1000, "A Book", "A Book")];
        let sidecars = sidecars_of(vec![roster(
            "A Book",
            true,
            vec![annotation(
                Kind::Highlight,
                "2026-08-08T13:27:44",
                500,
                560,
                "orange",
            )],
        )]);
        let got = merge(
            &[
                clip(
                    "A Book",
                    Kind::Highlight,
                    "2026-08-08T13:27:38",
                    69,
                    "a line",
                ),
                clip(
                    "A Book",
                    Kind::Highlight,
                    "2026-08-08T13:27:44",
                    69,
                    "a line",
                ),
            ],
            &sidecars,
            &books,
        );
        assert_eq!(got.len(), 1, "{got:#?}");
        assert_eq!(got[0].state, State::Live);
    }

    #[test]
    fn a_book_re_downloaded_keeps_the_words_its_new_sidecar_lost() {
        // A new `.sdr` restamps `created` past the clipping appended
        // first. One leftover each side of one book and kind.
        let books = [book(1000, "A Book", "A Book")];
        let sidecars = sidecars_of(vec![roster(
            "A Book",
            true,
            vec![annotation(
                Kind::Highlight,
                "2026-08-01T17:49:48",
                21_336,
                21_413,
                "orange",
            )],
        )]);
        let got = merge(
            &[clip(
                "A Book",
                Kind::Highlight,
                "2026-07-29T21:37:19",
                179,
                "no one today remembered why the war had come about",
            )],
            &sidecars,
            &books,
        );
        assert_eq!(got.len(), 1, "{got:#?}");
        assert_eq!(got[0].state, State::Live);
        // The words off the file, the position off the sidecar, and the display
        // location the clipping stated kept beside it.
        assert_eq!(
            got[0].body,
            "no one today remembered why the war had come about"
        );
        assert_eq!((got[0].start, got[0].end), (21_336, 21_413));
        assert_eq!(got[0].location, 179);
        assert_eq!(got[0].colour, "orange");
        // The stamp kept is the clipping's: that is when the highlight was made.
        assert_eq!(got[0].at, "2026-07-29T21:37:19");
    }

    #[test]
    fn leftovers_that_do_not_answer_one_for_one_are_left_alone() {
        // Two annotations and one clipping: which of the two the words belong to
        // is a choice, and there is no ground to make it on.
        let books = [book(1000, "A Book", "A Book")];
        let sidecars = sidecars_of(vec![roster(
            "A Book",
            true,
            vec![
                annotation(Kind::Highlight, "2026-08-01T17:49:48", 100, 160, "orange"),
                annotation(Kind::Highlight, "2026-08-01T17:50:11", 900, 960, "orange"),
            ],
        )]);
        let got = merge(
            &[clip(
                "A Book",
                Kind::Highlight,
                "2026-07-29T21:37:19",
                179,
                "a line",
            )],
            &sidecars,
            &books,
        );
        // The clipping stands alone and both annotations stand alone.
        assert_eq!(got.len(), 3, "{got:#?}");
        assert_eq!(
            got.iter().filter(|m| m.body.is_empty()).count(),
            2,
            "neither annotation took the words"
        );
    }

    #[test]
    fn leftovers_pair_up_the_book_and_never_by_their_stamps() {
        // Three of each, the sidecar's stamps in the opposite order: the
        // pairing is by position in the book, not by stamp.
        let books = [book(1000, "A Book", "A Book")];
        let sidecars = sidecars_of(vec![roster(
            "A Book",
            true,
            vec![
                annotation(Kind::Highlight, "2026-08-01T17:52:00", 700, 760, "orange"),
                annotation(Kind::Highlight, "2026-08-01T17:51:00", 400, 460, "orange"),
                annotation(Kind::Highlight, "2026-08-01T17:50:00", 100, 160, "orange"),
            ],
        )]);
        let got = merge(
            &[
                clip(
                    "A Book",
                    Kind::Highlight,
                    "2026-07-29T21:00:00",
                    10,
                    "first",
                ),
                clip(
                    "A Book",
                    Kind::Highlight,
                    "2026-07-29T21:10:00",
                    40,
                    "second",
                ),
                clip(
                    "A Book",
                    Kind::Highlight,
                    "2026-07-29T21:20:00",
                    70,
                    "third",
                ),
            ],
            &sidecars,
            &books,
        );
        assert_eq!(got.len(), 3, "{got:#?}");
        let mut placed: Vec<(i64, &str)> = got.iter().map(|m| (m.start, m.body.as_str())).collect();
        placed.sort();
        assert_eq!(placed, [(100, "first"), (400, "second"), (700, "third")]);
    }

    #[test]
    fn a_sidecar_annotation_no_clipping_speaks_for_is_still_a_highlight() {
        // Ink is in the sidecar and nowhere else: the file never holds
        // handwriting.
        let books = [book(1000, "A Book", "A Book")];
        let sidecars = sidecars_of(vec![roster(
            "A Book",
            true,
            vec![annotation(
                Kind::Handwriting,
                "2026-08-08T13:27:38",
                500,
                560,
                "",
            )],
        )]);
        let got = merge(&[], &sidecars, &books);
        assert_eq!(got.len(), 1);
        assert_eq!(got[0].state, State::Live);
        assert_eq!(got[0].kind, Kind::Handwriting);
        assert_eq!(got[0].extent, 1000);
    }

    #[test]
    fn a_sdr_names_its_book_by_the_path_the_library_states() {
        // A `.sdr` directory is the book's own file with its last suffix cut,
        // and `p_location` is that file.
        let named = "[An Author] A Book (2011).8e24abcd";
        let books = [book(1000, "A Book", &format!("Sidle/{named}"))];
        assert_eq!(
            stems(&books).get(named).map(|&at| books[at].extent),
            Some(1000)
        );
        // The bridge is the whole stem, never a prefix: two copies of one
        // book differ only in the appended hash.
        assert!(
            !stems(&books).contains_key("[An Author] A Book (2011).ffffffff"),
            "a stem that only shares a prefix names no book"
        );
        assert_eq!(
            stem_of("/mnt/us/documents/Sidle/A Book (2011).8e24.kfx"),
            named
                .replace("[An Author] ", "")
                .replace("8e24abcd", "8e24")
        );
    }

    #[test]
    fn a_highlight_is_bounded_and_a_note_is_kept_whole() {
        let long: String = "文".repeat(EXCERPT + 40);
        let cut = bounded(Kind::Highlight, &long);
        assert_eq!(cut.chars().count(), EXCERPT + 1);
        assert!(cut.ends_with(CUT));
        assert_eq!(bounded(Kind::Note, &long), long);
        assert_eq!(bounded(Kind::Highlight, "short"), "short");
    }

    #[test]
    fn a_note_folds_into_the_passage_its_own_range_sits_inside() {
        // `Note` carries no id of a highlight: the ranges are the only link.
        let mut passage = highlight(State::Live, 325, 374);
        passage.at = "2026-09-04T00:52:49".into();
        let mut note = highlight(State::Live, 372, 374);
        note.kind = Kind::Note;
        note.body = "我不是很确定但觉得这是在骂人。".into();
        note.at = "2026-09-04T00:53:52".into();
        let held = [&passage, &note];
        let rows = paired(&held);
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].highlight.kind, Kind::Highlight);
        assert_eq!(
            rows[0].note.map(|n| n.body.as_str()),
            Some(note.body.as_str())
        );
    }

    #[test]
    fn a_note_no_passage_covers_stands_on_its_own() {
        let passage = highlight(State::Live, 100, 200);
        let mut note = highlight(State::Live, 900, 905);
        note.kind = Kind::Note;
        let held = [&passage, &note];
        let rows = paired(&held);
        assert_eq!(rows.len(), 2, "{rows:#?}");
        assert!(rows.iter().all(|r| r.note.is_none()));

        // And a note the sidecar never placed is not guessed onto a
        // neighbour: nothing states where it is.
        let mut adrift = highlight(State::Unconfirmed, -1, -1);
        adrift.kind = Kind::Note;
        let held = [&passage, &adrift];
        assert_eq!(paired(&held).len(), 2);
    }

    #[test]
    fn a_note_inside_two_passages_goes_under_the_closer_one() {
        let wide = highlight(State::Live, 0, 1_000);
        let mut close = highlight(State::Live, 300, 400);
        close.at = "2026-09-04T00:52:49".into();
        let mut note = highlight(State::Live, 350, 355);
        note.kind = Kind::Note;
        let held = [&wide, &close, &note];
        let rows = paired(&held);
        assert_eq!(rows.len(), 2);
        let noted = rows
            .iter()
            .find(|r| r.note.is_some())
            .expect("a paired row");
        assert_eq!((noted.highlight.start, noted.highlight.end), (300, 400));
    }

    #[test]
    fn a_state_goes_to_a_word_and_comes_back() {
        for state in [State::Live, State::Retired, State::Unconfirmed] {
            assert_eq!(State::from_stored(state.as_str()), state);
        }
        assert_eq!(State::from_stored("anything else"), State::Unconfirmed);
    }
}
