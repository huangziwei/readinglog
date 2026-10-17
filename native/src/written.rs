//! `w` rows, notes, and `r` rows, corrections of a highlight's or a note's
//! words, beside the rows a Kindle derives: a deleted one keeps its row, and
//! [`Record::take_written`] makes two copies of a record one.

use std::collections::{HashMap, HashSet};
use std::fmt::Write as _;

use anyhow::{Result, anyhow};
use serde::{Deserialize, Serialize};

use crate::clippings::Kind;
use crate::date;
use crate::highlights::{Highlight, State};
use crate::record::{Book, Record};

/// What names one highlight across passes: [`Highlight::identity`], owned.
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
pub struct HighlightId {
    pub at: String,
    /// The kind's stored word, [`Kind::as_str`].
    pub kind: String,
    pub title: String,
    pub location: i64,
    pub start: i64,
}

impl HighlightId {
    pub fn of(highlight: &Highlight) -> Self {
        let (at, kind, title, location, start) = highlight.identity();
        Self {
            at: at.into(),
            kind: kind.as_str().into(),
            title: title.into(),
            location,
            start,
        }
    }

    /// Whether `highlight` is the one this names.
    pub fn names(&self, highlight: &Highlight) -> bool {
        let (at, kind, title, location, start) = highlight.identity();
        self.at == at
            && self.kind == kind.as_str()
            && self.title == title
            && self.location == location
            && self.start == start
    }

    /// A highlight answering to this identity, stating nothing past it.
    pub fn highlight(&self) -> Result<Highlight> {
        let kind = Kind::from_stored(&self.kind)
            .ok_or_else(|| anyhow!("{} is no kind of highlight", self.kind))?;
        Ok(Highlight {
            at: self.at.clone(),
            kind,
            title: self.title.clone(),
            location: self.location,
            start: match self.location < 0 {
                true => self.start,
                false => -1,
            },
            ..Highlight::default()
        })
    }
}

/// The passage a note is written on: its identity, its range then, and its words.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Passage {
    pub id: HighlightId,
    /// [`Highlight::start`] and [`Highlight::end`] as they stood when the note was written.
    pub from: i64,
    pub to: i64,
    pub words: String,
}

/// A `w` row: one note, named in its record by the moment it was written.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct Note {
    /// `YYYY-MM-DDTHH:MM:SS` on the clock that wrote it.
    pub written: String,
    /// When its words last changed or it was deleted: of two copies, the later stands.
    pub edited: String,
    /// The book's `cde_key`, empty where it carries none, and its `extent`.
    pub key: String,
    pub extent: i64,
    /// `None` for a note on the book alone.
    pub on: Option<Passage>,
    /// Markdown.
    pub text: String,
    /// `true` keeps the row with no words, drawn nowhere.
    pub deleted: bool,
}

impl Note {
    /// Whether `book` is this note's: by key where it carries one, else by extent.
    pub fn names(&self, book: &Book) -> bool {
        match self.key.is_empty() {
            false => book.cde_key == self.key,
            true => book.cde_key.is_empty() && book.extent == self.extent,
        }
    }
}

/// An `r` row: words set in place of a highlight's or a note's own.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Correction {
    pub id: HighlightId,
    /// When it was set or taken back: of two copies, the later stands.
    pub edited: String,
    /// The words set; `None` once taken back.
    pub text: Option<String>,
    /// The words the record stated when `text` was set; `None` stands it over any.
    pub was: Option<String>,
}

impl Correction {
    /// The words this sets in place of `highlight`'s, where it sets any and the
    /// record states the words they were set over.
    fn corrects(&self, highlight: &Highlight) -> Option<&str> {
        let text = self.text.as_deref()?;
        match &self.was {
            Some(was) if *was != highlight.body => None,
            _ => Some(text),
        }
    }
}

/// What a note laid into the highlights is there: the moment that names it, the
/// passage it was written on, and that passage's words where it is gone.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Laid {
    pub written: String,
    pub on: Option<HighlightId>,
    pub gone: Option<String>,
}

/// A record as it is drawn: each highlight in the words a correction sets, and
/// each note laid in as a highlight of [`Kind::Note`].
#[derive(Debug, Clone, Default)]
pub struct Drawn {
    pub record: Record,
    /// The highlights a correction set the words of.
    pub corrected: HashSet<HighlightId>,
    /// The notes laid in, by the identity each takes there.
    pub written: HashMap<HighlightId, Laid>,
}

impl Record {
    /// Take `other`'s notes and corrections: each one this record holds none of
    /// is added, and of two copies of one the later `edited` stands. Answers
    /// whether anything moved.
    pub fn take_written(&mut self, other: &Record) -> bool {
        let mut moved = false;
        for fresh in &other.notes {
            match self.notes.iter_mut().find(|n| n.written == fresh.written) {
                Some(held) if fresh.edited > held.edited => {
                    *held = fresh.clone();
                    moved = true;
                }
                Some(_) => {}
                None => {
                    self.notes.push(fresh.clone());
                    moved = true;
                }
            }
        }
        for fresh in &other.corrections {
            match self.corrections.iter_mut().find(|c| c.id == fresh.id) {
                Some(held) if fresh.edited > held.edited => {
                    *held = fresh.clone();
                    moved = true;
                }
                Some(_) => {}
                None => {
                    self.corrections.push(fresh.clone());
                    moved = true;
                }
            }
        }
        if moved {
            self.sort_written();
        }
        moved
    }

    /// Orders `notes` by when they were written and `corrections` by identity,
    /// one row each.
    pub(crate) fn sort_written(&mut self) {
        self.notes.sort_by(|a, b| a.written.cmp(&b.written));
        self.notes.dedup_by(|a, b| a.written == b.written);
        self.corrections.sort_by(|a, b| a.id.cmp(&b.id));
        self.corrections.dedup_by(|a, b| a.id == b.id);
    }

    /// This record as it is drawn: a copy with each correction set and each
    /// note laid in at its passage's start, else at the range it was written
    /// on, and a note alone at none.
    pub fn drawn(&self) -> Drawn {
        let mut record = self.clone();
        let mut corrected = HashSet::new();
        for highlight in &mut record.highlights {
            let Some(text) = record
                .corrections
                .iter()
                .filter(|c| c.id.names(highlight))
                .find_map(|c| c.corrects(highlight))
            else {
                continue;
            };
            highlight.body = text.to_string();
            corrected.insert(HighlightId::of(highlight));
        }
        let mut written = HashMap::new();
        let mut laid = Vec::new();
        for note in std::mem::take(&mut record.notes)
            .iter()
            .filter(|n| !n.deleted)
        {
            let Some(book) = record.books.iter().find(|b| note.names(b)) else {
                continue;
            };
            let (start, location, page, gone) = match &note.on {
                None => (-1, -1, String::new(), None),
                Some(on) => match record.highlights.iter().find(|h| on.id.names(h)) {
                    Some(passage) => (passage.start, passage.location, passage.page.clone(), None),
                    None => (
                        on.from,
                        on.id.location,
                        String::new(),
                        Some(on.words.clone()),
                    ),
                },
            };
            let highlight = Highlight {
                extent: book.extent,
                title: book.title.clone(),
                kind: Kind::Note,
                at: note.written.clone(),
                state: State::Live,
                start,
                end: start,
                location,
                page,
                colour: String::new(),
                body: note.text.clone(),
            };
            written.insert(
                HighlightId::of(&highlight),
                Laid {
                    written: note.written.clone(),
                    on: note.on.as_ref().map(|on| on.id.clone()),
                    gone,
                },
            );
            laid.push(highlight);
        }
        record.highlights.extend(laid);
        record.sort_highlights();
        Drawn {
            record,
            corrected,
            written,
        }
    }

    /// Hold `note`, its `written` moved a second on while another note holds the
    /// same; answers the moment that names it.
    pub fn add_note(&mut self, mut note: Note) -> String {
        while self.notes.iter().any(|n| n.written == note.written) {
            note.written = later(&note.written);
        }
        if note.edited.is_empty() || note.edited < note.written {
            note.edited.clone_from(&note.written);
        }
        let written = note.written.clone();
        self.notes.push(note);
        self.sort_written();
        written
    }

    /// Set the words of the note written at `written`, as of `now`.
    pub fn reword_note(&mut self, written: &str, text: &str, now: &str) -> Result<()> {
        if text.trim().is_empty() {
            return Err(anyhow!("a note holds no words"));
        }
        let note = self.live_note(written)?;
        note.text = text.to_string();
        note.edited = after(&note.edited, now);
        Ok(())
    }

    /// Delete the note written at `written`, as of `now`: its row stands with no
    /// words and no passage, drawn nowhere.
    pub fn delete_note(&mut self, written: &str, now: &str) -> Result<()> {
        let note = self.live_note(written)?;
        note.text = String::new();
        note.on = None;
        note.deleted = true;
        note.edited = after(&note.edited, now);
        Ok(())
    }

    /// The note written at `written`, where it is not deleted.
    fn live_note(&mut self, written: &str) -> Result<&mut Note> {
        self.notes
            .iter_mut()
            .find(|n| n.written == written && !n.deleted)
            .ok_or_else(|| anyhow!("no note written at {written}"))
    }

    /// Set `text` as the words of the highlight `id` names, as of `now`, beside
    /// the words the record states. `None`, blank words or the record's own take
    /// the correction back.
    pub fn set_words(&mut self, id: &HighlightId, text: Option<String>, now: &str) -> Result<()> {
        let held = self
            .highlights
            .iter()
            .find(|highlight| id.names(highlight))
            .ok_or_else(|| anyhow!("no highlight at {} in {}", id.at, id.title))?;
        let (text, was) = match text.filter(|t| !t.trim().is_empty() && *t != held.body) {
            Some(text) => (Some(text), Some(held.body.clone())),
            None => (None, None),
        };
        let at = match self.corrections.iter().position(|c| c.id == *id) {
            Some(at) => at,
            None if text.is_none() => return Ok(()),
            None => {
                self.corrections.push(Correction {
                    id: id.clone(),
                    edited: String::new(),
                    text: None,
                    was: None,
                });
                self.corrections.len() - 1
            }
        };
        let row = &mut self.corrections[at];
        row.text = text;
        row.was = was;
        row.edited = after(&row.edited, now);
        self.sort_written();
        Ok(())
    }
}

/// `now`, or a second past `held` where `now` is no later: each edit of a row
/// stamped after the one before it.
fn after(held: &str, now: &str) -> String {
    match now > held {
        true => now.to_string(),
        false => later(held),
    }
}

/// `at`, a `YYYY-MM-DDTHH:MM:SS` stamp, a second on, past midnight into the next day.
pub fn later(at: &str) -> String {
    let day = date::parse_day(date::day_of(at)).unwrap_or_default();
    let secs = date::secs_of(at) + 1;
    date::stamp(day + secs.div_euclid(86_400), secs)
}

/// The field a row states for a value it holds none of.
const NONE: &str = "\\N";

/// `text` as one field of a row: `\`, tab, newline and return escaped.
fn escaped(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for c in text.chars() {
        match c {
            '\\' => out.push_str("\\\\"),
            '\t' => out.push_str("\\t"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            c => out.push(c),
        }
    }
    out
}

/// What [`escaped`] wrote; an escape it does not write stands as it is.
fn unescaped(field: &str) -> String {
    let mut out = String::with_capacity(field.len());
    let mut chars = field.chars();
    while let Some(c) = chars.next() {
        if c != '\\' {
            out.push(c);
            continue;
        }
        match chars.next() {
            Some('\\') => out.push('\\'),
            Some('t') => out.push('\t'),
            Some('n') => out.push('\n'),
            Some('r') => out.push('\r'),
            Some(other) => {
                out.push('\\');
                out.push(other);
            }
            None => out.push('\\'),
        }
    }
    out
}

/// `text` as a field, [`NONE`] for none.
fn optional(text: Option<&str>) -> String {
    text.map_or_else(|| NONE.to_string(), escaped)
}

/// What [`optional`] wrote.
fn read_optional(field: &str) -> Option<String> {
    (field != NONE).then(|| unescaped(field))
}

/// A `w` row.
pub(crate) fn write_note(out: &mut String, n: &Note) {
    let state = match n.deleted {
        true => "deleted",
        false => "live",
    };
    let _ = write!(
        out,
        "w\t{}\t{}\t{}\t{}\t{state}\t",
        n.written,
        n.edited,
        n.extent,
        escaped(&n.key)
    );
    match &n.on {
        Some(p) => {
            let _ = write!(
                out,
                "{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t",
                escaped(&p.id.at),
                escaped(&p.id.kind),
                escaped(&p.id.title),
                p.id.location,
                p.id.start,
                p.from,
                p.to,
                escaped(&p.words)
            );
        }
        None => out.push_str("\t\t\t\t\t\t\t\t"),
    }
    out.push_str(&escaped(&n.text));
    out.push('\n');
}

/// A `w` row as a [`Note`], `None` where it states no moment.
pub(crate) fn read_note<'a>(f: &mut impl Iterator<Item = &'a str>) -> Option<Note> {
    let mut next = || f.next().unwrap_or_default();
    let written = next().trim().to_string();
    if written.is_empty() {
        return None;
    }
    let edited = next().trim().to_string();
    let extent = next().trim().parse().unwrap_or(0);
    let key = unescaped(next());
    let deleted = next().trim() == "deleted";
    let at = unescaped(next());
    let kind = unescaped(next());
    let title = unescaped(next());
    let number = |field: &str| field.trim().parse().unwrap_or(-1);
    let (location, start, from, to) = (
        number(next()),
        number(next()),
        number(next()),
        number(next()),
    );
    let words = unescaped(next());
    let text = unescaped(next());
    let on = (!kind.is_empty()).then_some(Passage {
        id: HighlightId {
            at,
            kind,
            title,
            location,
            start,
        },
        from,
        to,
        words,
    });
    Some(Note {
        edited: match edited.is_empty() {
            true => written.clone(),
            false => edited,
        },
        written,
        key,
        extent,
        on,
        text,
        deleted,
    })
}

/// An `r` row.
pub(crate) fn write_correction(out: &mut String, c: &Correction) {
    let _ = writeln!(
        out,
        "r\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}",
        c.edited,
        escaped(&c.id.at),
        escaped(&c.id.kind),
        escaped(&c.id.title),
        c.id.location,
        c.id.start,
        optional(c.was.as_deref()),
        optional(c.text.as_deref()),
    );
}

/// An `r` row as a [`Correction`], `None` where it names no kind.
pub(crate) fn read_correction<'a>(f: &mut impl Iterator<Item = &'a str>) -> Option<Correction> {
    let mut next = || f.next().unwrap_or_default();
    let edited = next().trim().to_string();
    let at = unescaped(next());
    let kind = unescaped(next());
    let title = unescaped(next());
    let location = next().trim().parse().unwrap_or(-1);
    let start = next().trim().parse().unwrap_or(-1);
    let was = read_optional(next());
    let text = read_optional(next());
    Kind::from_stored(&kind)?;
    Some(Correction {
        id: HighlightId {
            at,
            kind,
            title,
            location,
            start,
        },
        edited,
        text,
        was,
    })
}

/// `text`, a record's file, with its `w`, `r` and closing rows replaced by
/// `written`'s: every other line as `text` holds it, whatever build wrote it.
pub fn splice(text: &str, written: &Record) -> String {
    let mut out = String::with_capacity(text.len());
    for line in text.lines() {
        if matches!(
            line.split('\t').next(),
            Some("w" | "r" | crate::record::END)
        ) {
            continue;
        }
        out.push_str(line);
        out.push('\n');
    }
    for n in &written.notes {
        write_note(&mut out, n);
    }
    for c in &written.corrections {
        write_correction(&mut out, c);
    }
    crate::record::close(&mut out);
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn book() -> Book {
        Book {
            extent: 7,
            cde_key: "B00KEY".into(),
            title: "A Book".into(),
            ..Book::default()
        }
    }

    fn passage(at: &str, start: i64, end: i64) -> Highlight {
        Highlight {
            extent: 7,
            title: "A Book".into(),
            kind: Kind::Highlight,
            at: at.into(),
            state: State::Live,
            start,
            end,
            location: 120,
            body: "Some words".into(),
            ..Highlight::default()
        }
    }

    fn record() -> Record {
        Record {
            books: vec![book()],
            highlights: vec![passage("2026-10-09T16:54:03", 100, 200)],
            ..Record::default()
        }
    }

    fn note(written: &str, on: Option<&Highlight>, text: &str) -> Note {
        Note {
            written: written.into(),
            edited: written.into(),
            key: "B00KEY".into(),
            extent: 7,
            on: on.map(|h| Passage {
                id: HighlightId::of(h),
                from: h.start,
                to: h.end,
                words: h.body.clone(),
            }),
            text: text.into(),
            deleted: false,
        }
    }

    #[test]
    fn notes_and_corrections_round_trip_through_the_file_words_and_all() {
        let mut held = record();
        let on = held.highlights[0].clone();
        held.add_note(note(
            "2026-10-10T08:00:00",
            Some(&on),
            "# A title\n\nTwo\tlines \\ and a tab",
        ));
        held.add_note(note("2026-10-10T08:00:00", None, "alone"));
        held.set_words(
            &HighlightId::of(&on),
            Some("Some words.".into()),
            "2026-10-11T09:00:00",
        )
        .unwrap();
        let text = held.text();
        let read = Record::from_text(&text);
        assert_eq!(read.notes, held.notes);
        assert_eq!(read.corrections, held.corrections);
        assert_eq!(read.text(), text);
        assert_eq!(
            read.notes[1].written, "2026-10-10T08:00:01",
            "two in one second stay two"
        );
        assert!(
            text.lines().all(|l| !l.is_empty()),
            "no row spans two lines"
        );
        assert_eq!(later("2026-10-10T23:59:59"), "2026-10-11T00:00:00");
    }

    #[test]
    fn a_note_lays_under_its_passage_follows_its_range_and_stands_alone_once_gone() {
        let mut held = record();
        let on = held.highlights[0].clone();
        held.add_note(note("2026-10-10T08:00:00", Some(&on), "under it"));
        held.add_note(note("2026-10-13T08:00:00", Some(&on), "and again"));
        held.add_note(note("2026-10-08T21:10:00", None, "alone"));
        let drawn = held.drawn();
        assert_eq!(drawn.written.len(), 3);
        assert!(drawn.written.values().all(|l| l.gone.is_none()));
        let own = |drawn: &Drawn| {
            let written = drawn.written.clone();
            move |n: &Highlight| written.get(&HighlightId::of(n)).and_then(|l| l.on.clone())
        };
        let rows: Vec<&Highlight> = drawn.record.highlights.iter().collect();
        let paired = crate::highlights::paired(&rows, own(&drawn));
        assert_eq!(paired.len(), 2);
        let under = paired
            .iter()
            .find(|r| r.highlight.body == "Some words")
            .unwrap();
        assert_eq!(under.notes.len(), 2);
        assert!(drawn.record.notes.is_empty(), "laid, not held twice");

        // `nested` holds a shorter highlight opening where the passage does: the
        // notes stay under their own passage.
        let mut nested = held.clone();
        nested
            .highlights
            .push(passage("2026-10-11T09:00:00", 100, 120));
        nested.sort_highlights();
        let drawn = nested.drawn();
        let rows: Vec<&Highlight> = drawn.record.highlights.iter().collect();
        let paired = crate::highlights::paired(&rows, own(&drawn));
        let under = |end: i64| paired.iter().find(|r| r.highlight.end == end).unwrap();
        assert_eq!(under(200).notes.len(), 2);
        assert!(under(120).notes.is_empty());

        // `widened` holds a re-highlight: the passage gone, a wider one over its range.
        let mut widened = held.clone();
        widened.highlights[0].at = "2026-10-12T09:00:00".into();
        (widened.highlights[0].start, widened.highlights[0].end) = (90, 260);
        let drawn = widened.drawn();
        assert!(
            drawn
                .written
                .values()
                .any(|l| l.gone.as_deref() == Some("Some words"))
        );
        let rows: Vec<&Highlight> = drawn.record.highlights.iter().collect();
        assert_eq!(crate::highlights::paired(&rows, own(&drawn)).len(), 2);

        // `delete_note` keeps the row and draws it nowhere.
        let mut gone = held.clone();
        gone.delete_note("2026-10-08T21:10:00", "2026-10-14T10:00:00")
            .unwrap();
        assert_eq!(gone.drawn().written.len(), 2);
        assert_eq!(gone.notes.len(), 3);
        assert!(gone.notes[0].deleted && gone.notes[0].text.is_empty());
        assert!(
            gone.reword_note("2026-10-08T21:10:00", "back", "2026-10-15T10:00:00")
                .is_err()
        );
    }

    #[test]
    fn a_correction_stands_while_the_record_states_the_words_it_was_set_over() {
        let mut held = record();
        let id = HighlightId::of(&held.highlights[0]);
        held.set_words(&id, Some("Some words.".into()), "2026-10-11T09:00:00")
            .unwrap();
        let drawn = held.drawn();
        assert_eq!(drawn.record.highlights[0].body, "Some words.");
        assert_eq!(drawn.corrected, HashSet::from([id.clone()]));
        // `moved` states other words than `was`: its own stand.
        let mut moved = held.clone();
        moved.highlights[0].body = "Other words".into();
        assert!(moved.drawn().corrected.is_empty());
        // `set_words` with the record's own words, blank words or none takes it
        // back, and the row stands.
        for text in [Some("Some words"), Some("  "), None] {
            let mut back = held.clone();
            back.set_words(&id, text.map(String::from), "2026-10-12T09:00:00")
                .unwrap();
            assert!(back.drawn().corrected.is_empty());
            assert_eq!(back.corrections.len(), 1);
            assert_eq!(back.corrections[0].text, None);
        }
    }

    #[test]
    fn two_copies_become_one_the_later_edit_of_each_standing() {
        let mut desk = record();
        let mut kindle = record();
        let on = desk.highlights[0].clone();
        desk.add_note(note("2026-10-10T08:00:00", Some(&on), "first"));
        kindle.take_written(&desk);
        desk.reword_note("2026-10-10T08:00:00", "reworded", "2026-10-12T08:00:00")
            .unwrap();
        desk.add_note(note("2026-10-11T08:00:00", None, "second"));
        let mut older = kindle.clone();
        assert!(kindle.take_written(&desk));
        assert_eq!(kindle.notes.len(), 2);
        assert_eq!(kindle.notes[0].text, "reworded");
        // `older` moves nothing back, and `delete_note` outlasts it.
        assert!(!kindle.take_written(&older));
        desk.delete_note("2026-10-11T08:00:00", "2026-10-13T08:00:00")
            .unwrap();
        kindle.take_written(&desk);
        older.take_written(&kindle);
        assert!(older.notes[1].deleted);
        assert!(
            !older.take_written(&record()),
            "a copy holding none takes none away"
        );
        // `after` stamps a second edit in one second past the first.
        desk.reword_note("2026-10-10T08:00:00", "once", "2026-10-14T08:00:00")
            .unwrap();
        let mut copy = desk.clone();
        desk.reword_note("2026-10-10T08:00:00", "twice", "2026-10-14T08:00:00")
            .unwrap();
        assert!(copy.take_written(&desk));
        assert_eq!(copy.notes[0].text, "twice");
    }

    #[test]
    fn a_splice_keeps_every_other_line_as_it_stood() {
        let mut held = record();
        held.add_note(note("2026-10-10T08:00:00", None, "alone"));
        let text = format!("{}q\tfrom a later build\n", record().text());
        let spliced = splice(&text, &held);
        assert!(spliced.contains("q\tfrom a later build\n"));
        assert_eq!(Record::from_text(&spliced).notes, held.notes);
        assert!(crate::record::closed(&spliced));
        assert_eq!(splice(&held.text(), &held), held.text());
    }

    fn scratch(name: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!("readinglog-written-{name}"));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("a scratch directory");
        dir
    }

    #[test]
    fn a_record_cut_short_or_missing_gives_way_to_a_whole_partial_and_only_then() {
        use crate::record::PARTIAL_FILE;
        let dir = scratch("partial");
        let file = Record::file(&dir);
        let mut held = record();
        held.add_note(note("2026-10-10T08:00:00", None, "alone"));
        let whole = held.text();
        // `file` cut short beside a whole `PARTIAL_FILE`: the partial takes its place.
        std::fs::write(dir.join(PARTIAL_FILE), &whole).unwrap();
        std::fs::write(&file, &whole[..whole.len() / 2]).unwrap();
        assert_eq!(Record::load(&dir).notes, held.notes);
        assert_eq!(std::fs::read_to_string(&file).unwrap(), whole);
        assert!(!dir.join(PARTIAL_FILE).exists());
        // `file` gone between a delete and a rename: the same.
        std::fs::write(dir.join(PARTIAL_FILE), &whole).unwrap();
        std::fs::remove_file(&file).unwrap();
        assert_eq!(Record::open(&dir).notes, held.notes);
        // `PARTIAL_FILE` older than `file`, or cut itself, is left alone.
        std::fs::write(dir.join(PARTIAL_FILE), &whole).unwrap();
        std::fs::write(&file, record().text()).unwrap();
        assert!(Record::load(&dir).notes.is_empty());
        std::fs::write(dir.join(PARTIAL_FILE), &whole[..whole.len() - 3]).unwrap();
        std::fs::write(&file, &whole[..10]).unwrap();
        Record::load(&dir);
        assert_eq!(std::fs::read_to_string(&file).unwrap(), whole[..10]);
    }

    #[test]
    fn a_save_keeps_the_notes_another_app_wrote_into_the_file() {
        let dir = scratch("keep");
        let file = Record::file(&dir);
        let mut kindle = record();
        let mut seen = None;
        kindle.keep(&dir, &mut seen).unwrap();
        assert!(
            !kindle.follow(&dir, &mut seen),
            "its own write moves nothing"
        );
        let write = |desk: &Record| {
            let text = std::fs::read_to_string(&file).unwrap();
            std::fs::write(&file, splice(&text, desk)).unwrap();
        };
        let mut desk = Record::load(&dir);
        desk.add_note(note("2026-10-10T08:00:00", None, "from the desktop"));
        write(&desk);
        // `keep` from the record `kindle` holds keeps the note.
        kindle.highlights[0].colour = "yellow".into();
        kindle.keep(&dir, &mut seen).unwrap();
        let read = Record::load(&dir);
        assert_eq!(read.notes.len(), 1);
        assert_eq!(read.highlights[0].colour, "yellow");
        // `follow` finds the next while `kindle` is open.
        desk.add_note(note("2026-10-11T08:00:00", None, "another"));
        write(&desk);
        assert!(kindle.follow(&dir, &mut seen));
        assert_eq!(kindle.notes.len(), 2);
    }
}
