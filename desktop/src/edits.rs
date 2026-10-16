//! The edits kept beside the record and never written into it: `books.toml`,
//! `highlights.toml` and `sittings.toml`, each row keyed by what survives a
//! re-import of the same reading.

use std::collections::HashSet;
use std::path::Path;

use anyhow::{Context, Result, anyhow};
use serde::{Deserialize, Serialize};

use readinglog_native::clippings::Kind;
use readinglog_native::highlights::Highlight;
use readinglog_native::log::sitting::Sitting;
use readinglog_native::record::Record;

use crate::workspace::{NewBooks, Workspace, write_toml};

pub const BOOKS_FILE: &str = "books.toml";
pub const HIGHLIGHTS_FILE: &str = "highlights.toml";
pub const SITTINGS_FILE: &str = "sittings.toml";

/// One `[[book]]`: the choices made about one book.
#[derive(Debug, Default, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct BookEdit {
    /// The book's `cde_key`, which stays where a re-fetch moves the extent.
    #[serde(skip_serializing_if = "String::is_empty")]
    pub key: String,
    /// The book's `extent`, which names a book carrying no key.
    #[serde(skip_serializing_if = "is_zero")]
    pub extent: i64,
    /// Whether it is on the site. `None` follows `site.toml`'s `new_books`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub shown: Option<bool>,
    /// Whether its highlights are shown, each but those its own row decides.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub highlights: Option<bool>,
    /// What the site states in place of the record's title and author.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub author: Option<String>,
    /// Its address on the site, set once and kept.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub slug: Option<String>,
}

fn is_zero(n: &i64) -> bool {
    *n == 0
}

impl BookEdit {
    /// Whether this row is the one for the book `key` and `extent` describe:
    /// by key where the book carries one, else by extent.
    pub fn names(&self, key: &str, extent: i64) -> bool {
        match key.is_empty() {
            false => self.key == key,
            true => self.key.is_empty() && self.extent == extent,
        }
    }

    /// Whether the row states anything past its key.
    fn says_anything(&self) -> bool {
        self.shown.is_some()
            || self.highlights.is_some()
            || self.title.is_some()
            || self.author.is_some()
            || self.slug.is_some()
    }
}

/// What names one highlight across imports: `Highlight::identity`.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct HighlightId {
    pub at: String,
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

    /// A highlight answering to this identity, for `Edits` to find its row by.
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

    /// Whether `highlight` is the one this names.
    pub fn names(&self, highlight: &Highlight) -> bool {
        *self == Self::of(highlight)
    }
}

/// One `[[highlight]]`, keyed by `Highlight::identity` and the Kindle whose
/// record states it.
#[derive(Debug, Default, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct HighlightEdit {
    pub at: String,
    /// The kind's stored word, `Kind::as_str`.
    pub kind: String,
    pub title: String,
    pub location: i64,
    /// The sidecar's position, which `Highlight::identity` holds for a
    /// highlight no clipping placed, and -1 for every other.
    #[serde(default = "unplaced", skip_serializing_if = "is_unplaced")]
    pub start: i64,
    /// The Kindle's serial, cut; empty for the backups naming no Kindle.
    #[serde(skip_serializing_if = "String::is_empty")]
    pub kindle: String,
    /// Whether it is shown. `None` follows its book's `highlights`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub shown: Option<bool>,
    /// The words set in place of the record's.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub text: Option<String>,
    /// The words the record stated when `text` was set. `text` stands while
    /// the record states them; `None` stands it over any.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub was: Option<String>,
    /// A comment on it, in Markdown.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub comment: Option<String>,
}

fn unplaced() -> i64 {
    -1
}

fn is_unplaced(n: &i64) -> bool {
    *n == -1
}

impl HighlightEdit {
    /// The row for `highlight`, stating nothing yet.
    fn of(highlight: &Highlight) -> Self {
        let (at, kind, title, location, start) = highlight.identity();
        Self {
            at: at.into(),
            kind: kind.as_str().into(),
            title: title.into(),
            location,
            start,
            ..Self::default()
        }
    }

    pub fn names(&self, highlight: &Highlight) -> bool {
        let (at, kind, title, location, start) = highlight.identity();
        self.at == at
            && self.kind == kind.as_str()
            && self.title == title
            && self.location == location
            && self.start == start
    }

    /// Whether this row is `highlight`'s on the Kindle `kindle` names.
    fn on(&self, highlight: &Highlight, kindle: &str) -> bool {
        self.kindle == kindle && self.names(highlight)
    }

    /// The words this row sets in place of `highlight`'s, where it sets any and
    /// the record states the words they were set over.
    fn corrects(&self, highlight: &Highlight) -> Option<&str> {
        let text = self.text.as_deref()?;
        match &self.was {
            Some(was) if *was != highlight.body => None,
            _ => Some(text),
        }
    }

    fn says_anything(&self) -> bool {
        self.shown.is_some() || self.text.is_some() || self.comment.is_some()
    }
}

/// One `[[sitting]]`, keyed by `(started_at, end_position)`.
#[derive(Debug, Default, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct SittingEdit {
    pub started_at: String,
    pub end_position: i64,
    /// Whether it counts on the site. `None` counts it.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub shown: Option<bool>,
}

impl SittingEdit {
    pub fn names(&self, sitting: &Sitting) -> bool {
        self.started_at == sitting.started_at && self.end_position == sitting.end_position
    }
}

#[derive(Default, Serialize, Deserialize)]
struct BooksFile {
    #[serde(default, rename = "book")]
    rows: Vec<BookEdit>,
}

#[derive(Default, Serialize, Deserialize)]
struct HighlightsFile {
    #[serde(default, rename = "highlight")]
    rows: Vec<HighlightEdit>,
}

#[derive(Default, Serialize, Deserialize)]
struct SittingsFile {
    #[serde(default, rename = "sitting")]
    rows: Vec<SittingEdit>,
}

/// Every edit a workspace holds.
#[derive(Debug, Default, Clone, PartialEq)]
pub struct Edits {
    pub books: Vec<BookEdit>,
    pub highlights: Vec<HighlightEdit>,
    pub sittings: Vec<SittingEdit>,
}

impl Edits {
    /// The three files under `dir`. A missing one holds nothing.
    pub fn load(dir: &Path) -> Result<Self> {
        Ok(Self {
            books: read::<BooksFile>(&dir.join(BOOKS_FILE))?.rows,
            highlights: read::<HighlightsFile>(&dir.join(HIGHLIGHTS_FILE))?.rows,
            sittings: read::<SittingsFile>(&dir.join(SITTINGS_FILE))?.rows,
        })
    }

    /// Write the three files under `dir`, the rows in a fixed order: unchanged
    /// choices give unchanged bytes. A row stating nothing past its key is left
    /// out.
    pub fn save(&self, dir: &Path) -> Result<()> {
        let mut books: Vec<BookEdit> = self
            .books
            .iter()
            .filter(|b| b.says_anything())
            .cloned()
            .collect();
        books.sort_by(|a, b| (&a.key, a.extent).cmp(&(&b.key, b.extent)));
        let mut highlights: Vec<HighlightEdit> = self
            .highlights
            .iter()
            .filter(|h| h.says_anything())
            .cloned()
            .collect();
        highlights.sort_by(|a, b| {
            (&a.title, &a.at, &a.kind, a.location, &a.kindle)
                .cmp(&(&b.title, &b.at, &b.kind, b.location, &b.kindle))
        });
        let mut sittings: Vec<SittingEdit> = self
            .sittings
            .iter()
            .filter(|s| s.shown.is_some())
            .cloned()
            .collect();
        sittings
            .sort_by(|a, b| (&a.started_at, a.end_position).cmp(&(&b.started_at, b.end_position)));
        write_toml(&dir.join(BOOKS_FILE), &BooksFile { rows: books })?;
        write_toml(
            &dir.join(HIGHLIGHTS_FILE),
            &HighlightsFile { rows: highlights },
        )?;
        write_toml(&dir.join(SITTINGS_FILE), &SittingsFile { rows: sittings })
    }

    pub fn book(&self, key: &str, extent: i64) -> Option<&BookEdit> {
        self.books.iter().find(|b| b.names(key, extent))
    }

    /// The row for the book `key` and `extent` describe, added where none is.
    pub fn book_mut(&mut self, key: &str, extent: i64) -> &mut BookEdit {
        let at = match self.books.iter().position(|b| b.names(key, extent)) {
            Some(at) => at,
            None => {
                self.books.push(BookEdit {
                    key: key.into(),
                    extent: match key.is_empty() {
                        true => extent,
                        false => 0,
                    },
                    ..BookEdit::default()
                });
                self.books.len() - 1
            }
        };
        &mut self.books[at]
    }

    pub fn highlight(&self, highlight: &Highlight) -> Option<&HighlightEdit> {
        self.highlights.iter().find(|h| h.names(highlight))
    }

    /// The row for `highlight`, added where none is.
    pub fn highlight_mut(&mut self, highlight: &Highlight) -> &mut HighlightEdit {
        let at = match self.highlights.iter().position(|h| h.names(highlight)) {
            Some(at) => at,
            None => {
                self.highlights.push(HighlightEdit::of(highlight));
                self.highlights.len() - 1
            }
        };
        &mut self.highlights[at]
    }

    /// Every highlight of `record`, the record of the Kindle `kindle` names,
    /// in the words a row sets in place of its own; answers the ones it set.
    pub fn correct(&self, record: &mut Record, kindle: &str) -> HashSet<HighlightId> {
        let mut out = HashSet::new();
        for highlight in &mut record.highlights {
            let Some(text) = self
                .highlights
                .iter()
                .filter(|row| row.on(highlight, kindle))
                .find_map(|row| row.corrects(highlight))
            else {
                continue;
            };
            highlight.body = text.to_string();
            out.insert(HighlightId::of(highlight));
        }
        out
    }

    /// Set `text` as the words of the highlight `id` names in `record`, the
    /// record of the Kindle `kindle` names, keeping the words the record states
    /// beside it. `None`, blank words or the record's own take the row's away.
    pub fn set_words(
        &mut self,
        record: &Record,
        kindle: &str,
        id: &HighlightId,
        text: Option<String>,
    ) -> Result<()> {
        let held = record
            .highlights
            .iter()
            .find(|highlight| id.names(highlight))
            .ok_or_else(|| anyhow!("no highlight at {} in {}", id.at, id.title))?;
        let at = match self.highlights.iter().position(|row| row.on(held, kindle)) {
            Some(at) => at,
            None => {
                self.highlights.push(HighlightEdit {
                    kindle: kindle.into(),
                    ..HighlightEdit::of(held)
                });
                self.highlights.len() - 1
            }
        };
        let row = &mut self.highlights[at];
        match text.filter(|t| !t.trim().is_empty() && *t != held.body) {
            Some(text) => {
                row.text = Some(text);
                row.was = Some(held.body.clone());
            }
            None => {
                row.text = None;
                row.was = None;
            }
        }
        Ok(())
    }

    pub fn sitting(&self, sitting: &Sitting) -> Option<&SittingEdit> {
        self.sittings.iter().find(|s| s.names(sitting))
    }

    /// The row for `sitting`, added where none is.
    pub fn sitting_mut(&mut self, sitting: &Sitting) -> &mut SittingEdit {
        let at = match self.sittings.iter().position(|s| s.names(sitting)) {
            Some(at) => at,
            None => {
                self.sittings.push(SittingEdit {
                    started_at: sitting.started_at.clone(),
                    end_position: sitting.end_position,
                    shown: None,
                });
                self.sittings.len() - 1
            }
        };
        &mut self.sittings[at]
    }

    /// Whether the book `key` and `extent` describe is on the site.
    pub fn book_shown(&self, key: &str, extent: i64, new_books: NewBooks) -> bool {
        self.book(key, extent)
            .and_then(|b| b.shown)
            .unwrap_or(new_books == NewBooks::Shown)
    }

    /// Whether `highlight`, in the book `key` and `extent` describe, is shown:
    /// as its own row says, else as its book's `highlights` says, else not.
    pub fn highlight_shown(&self, highlight: &Highlight, key: &str, extent: i64) -> bool {
        self.highlight(highlight)
            .and_then(|h| h.shown)
            .or_else(|| self.book(key, extent).and_then(|b| b.highlights))
            .unwrap_or(false)
    }

    /// Whether `sitting` counts on the site.
    pub fn sitting_shown(&self, sitting: &Sitting) -> bool {
        self.sitting(sitting).and_then(|s| s.shown).unwrap_or(true)
    }
}

/// Set each of `changes`, a highlight and the words it is to state, in
/// `ws`'s `highlights.toml`, against the record of the Kindle `kindle` names,
/// as [`Edits::set_words`] sets one.
pub fn correct_words(
    ws: &Workspace,
    kindle: &str,
    changes: &[(HighlightId, Option<String>)],
) -> Result<()> {
    let source = ws
        .sources()
        .into_iter()
        .find(|s| s.serial.as_deref().unwrap_or_default() == kindle)
        .ok_or_else(|| anyhow!("no record of {kindle} in {}", ws.dir.display()))?;
    let mut edits = Edits::load(&ws.dir)?;
    for (id, text) in changes {
        edits.set_words(&source.record, kindle, id, text.clone())?;
    }
    edits.save(&ws.dir)
}

/// The TOML at `path` as `T`, and `T::default()` where there is no file.
fn read<T: Default + for<'de> Deserialize<'de>>(path: &Path) -> Result<T> {
    match std::fs::read_to_string(path) {
        Ok(text) => toml::from_str(&text).with_context(|| format!("reading {}", path.display())),
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => Ok(T::default()),
        Err(err) => Err(err).with_context(|| format!("reading {}", path.display())),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tests::scratch;
    use readinglog_native::clippings::Kind;

    fn passage(at: &str) -> Highlight {
        Highlight {
            extent: 7,
            title: "A Book".into(),
            kind: Kind::Highlight,
            at: at.into(),
            location: 120,
            body: "Some words".into(),
            ..Highlight::default()
        }
    }

    #[test]
    fn a_choice_is_found_again_by_what_survives_a_reimport() {
        let mut edits = Edits::default();
        edits.book_mut("B00KEY", 111).shown = Some(false);
        edits.book_mut("", 222).title = Some("Named".into());
        // A re-fetch moves the extent under a kept key.
        assert_eq!(edits.book("B00KEY", 999).and_then(|b| b.shown), Some(false));
        // A book with no key is found by its extent, and only by it.
        assert!(edits.book("", 222).is_some());
        assert!(edits.book("", 111).is_none());
        assert!(!edits.book_shown("B00KEY", 999, NewBooks::Shown));
        assert!(edits.book_shown("OTHER", 1, NewBooks::Shown));
        assert!(!edits.book_shown("OTHER", 1, NewBooks::Hidden));

        let one = passage("2026-09-05T18:22:26");
        let mut moved = one.clone();
        moved.body = "Some words, read again".into();
        moved.start = 4000;
        edits.highlight_mut(&one).comment = Some("Mine".into());
        // The words and the sidecar fields are not a placed highlight's identity.
        assert_eq!(
            edits.highlight(&moved).and_then(|h| h.comment.clone()),
            Some("Mine".into())
        );

        // Two a sidecar alone states, sharing a stamp, are two rows.
        let unplaced = |start: i64| Highlight {
            location: -1,
            start,
            ..passage("2026-06-23T09:58:49")
        };
        edits.highlight_mut(&unplaced(1920)).text = Some("First".into());
        assert!(edits.highlight(&unplaced(4089)).is_none());
    }

    #[test]
    fn a_correction_stands_while_its_kindle_states_the_words_it_was_set_over() {
        let kindle = "G003H2-0001";
        let record = Record {
            highlights: vec![passage("2026-09-05T18:22:26")],
            ..Record::default()
        };
        let id = HighlightId::of(&record.highlights[0]);
        let mut edits = Edits::default();
        edits
            .set_words(&record, kindle, &id, Some("Some words.".into()))
            .unwrap();
        let mut drawn = record.clone();
        assert_eq!(
            edits.correct(&mut drawn, kindle),
            HashSet::from([id.clone()])
        );
        assert_eq!(drawn.highlights[0].body, "Some words.");
        assert_eq!(
            drawn.highlights.len(),
            1,
            "corrected in place, never beside"
        );
        // The same highlight in another Kindle's record is that Kindle's own.
        assert!(edits.correct(&mut record.clone(), "G0027J-ABCD").is_empty());
        // The Kindle corrected it too: its words stand.
        let mut moved = record.clone();
        moved.highlights[0].body = "Some other words".into();
        assert!(edits.correct(&mut moved, kindle).is_empty());
        assert_eq!(moved.highlights[0].body, "Some other words");

        // Kept through the file, the words it was set over beside it.
        let dir = scratch("edits-corrected");
        edits.save(&dir).unwrap();
        let read = Edits::load(&dir).unwrap();
        assert_eq!(read.highlights[0].was.as_deref(), Some("Some words"));
        assert_eq!(read.highlights[0].kindle, kindle);
        assert!(!read.correct(&mut record.clone(), kindle).is_empty());

        // The record's own words, blank words or none take the correction away.
        for text in [Some("Some words"), Some("  "), None] {
            let mut edits = read.clone();
            edits
                .set_words(&record, kindle, &id, text.map(String::from))
                .unwrap();
            assert!(edits.correct(&mut record.clone(), kindle).is_empty());
            edits.save(&dir).unwrap();
            assert!(Edits::load(&dir).unwrap().highlights.is_empty());
        }
    }

    #[test]
    fn a_highlight_follows_its_book_unless_its_own_row_decides() {
        let mut edits = Edits::default();
        let (one, two) = (
            passage("2026-09-05T18:00:00"),
            passage("2026-09-06T18:00:00"),
        );
        assert!(!edits.highlight_shown(&one, "B00KEY", 7));
        edits.book_mut("B00KEY", 7).highlights = Some(true);
        assert!(edits.highlight_shown(&one, "B00KEY", 7));
        edits.highlight_mut(&two).shown = Some(false);
        assert!(!edits.highlight_shown(&two, "B00KEY", 7));
        assert!(edits.highlight_shown(&one, "B00KEY", 7));
    }

    #[test]
    fn the_files_round_trip_in_a_fixed_order_and_drop_empty_rows() {
        let dir = scratch("edits");
        let mut edits = Edits::default();
        edits.book_mut("B2", 2).shown = Some(true);
        edits.book_mut("B1", 1).slug = Some("first".into());
        edits.book_mut("B3", 3);
        edits.highlight_mut(&passage("2026-09-05T18:00:00")).text = Some("Fixed".into());
        let sitting = Sitting {
            started_at: "2026-09-05T18:00:00".into(),
            end_position: 42,
            ..Sitting::default()
        };
        edits.sitting_mut(&sitting).shown = Some(false);
        edits.save(&dir).unwrap();

        let read = Edits::load(&dir).unwrap();
        let keys: Vec<&str> = read.books.iter().map(|b| b.key.as_str()).collect();
        assert_eq!(keys, ["B1", "B2"]);
        assert_eq!(read.highlights.len(), 1);
        assert!(!read.sitting_shown(&sitting));
        let first = std::fs::read(dir.join(BOOKS_FILE)).unwrap();
        read.save(&dir).unwrap();
        assert_eq!(std::fs::read(dir.join(BOOKS_FILE)).unwrap(), first);
        assert_eq!(
            Edits::load(&scratch("edits-none")).unwrap(),
            Edits::default()
        );
    }
}
