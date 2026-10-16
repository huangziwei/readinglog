//! `highlights.toml`: corrections kept beside the record and never written into
//! it, each row keyed by what survives a re-import of the same reading and by
//! the Kindle whose record states it.

use std::collections::HashSet;
use std::path::Path;

use anyhow::{Context, Result, anyhow};
use serde::{Deserialize, Serialize};

use readinglog_native::clippings::Kind;
use readinglog_native::highlights::Highlight;
use readinglog_native::record::Record;

use crate::workspace::{Workspace, write_toml};

pub const HIGHLIGHTS_FILE: &str = "highlights.toml";

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
    /// The words set in place of the record's.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub text: Option<String>,
    /// The words the record stated when `text` was set. `text` stands while
    /// the record states them; `None` stands it over any.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub was: Option<String>,
    /// Every other key the row holds, written back as it was read.
    #[serde(flatten)]
    pub rest: toml::Table,
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
        self.text.is_some() || !self.rest.is_empty()
    }
}

#[derive(Default, Serialize, Deserialize)]
struct HighlightsFile {
    #[serde(default, rename = "highlight")]
    rows: Vec<HighlightEdit>,
}

/// Every correction a workspace holds.
#[derive(Debug, Default, Clone, PartialEq)]
pub struct Edits {
    pub highlights: Vec<HighlightEdit>,
}

impl Edits {
    /// `highlights.toml` under `dir`. A missing one holds nothing.
    pub fn load(dir: &Path) -> Result<Self> {
        Ok(Self {
            highlights: read::<HighlightsFile>(&dir.join(HIGHLIGHTS_FILE))?.rows,
        })
    }

    /// Write `highlights.toml` under `dir`, the rows in a fixed order: unchanged
    /// rows give unchanged bytes. A row stating nothing past its key is left
    /// out.
    pub fn save(&self, dir: &Path) -> Result<()> {
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
        write_toml(
            &dir.join(HIGHLIGHTS_FILE),
            &HighlightsFile { rows: highlights },
        )
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
    fn a_highlight_is_named_by_what_survives_a_reimport() {
        let one = passage("2026-09-05T18:22:26");
        let mut moved = one.clone();
        moved.body = "Some words, read again".into();
        moved.start = 4000;
        // The words and the sidecar fields are not a placed highlight's identity.
        assert_eq!(HighlightId::of(&moved), HighlightId::of(&one));
        assert!(HighlightEdit::of(&one).names(&moved));

        // Two a sidecar alone states, sharing a stamp, are two.
        let unplaced = |start: i64| Highlight {
            location: -1,
            start,
            ..passage("2026-06-23T09:58:49")
        };
        assert_ne!(
            HighlightId::of(&unplaced(1920)),
            HighlightId::of(&unplaced(4089))
        );
        assert!(!HighlightEdit::of(&unplaced(1920)).names(&unplaced(4089)));
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
    fn the_file_round_trips_in_a_fixed_order_and_keeps_every_key_it_holds() {
        let dir = scratch("edits");
        let held = "[[highlight]]\nat = \"2026-09-06T18:00:00\"\nkind = \"highlight\"\n\
                    title = \"B\"\nlocation = 120\nkindle = \"G003H2-0001\"\n\
                    text = \"Fixed\"\nwas = \"Some words\"\n\n\
                    [[highlight]]\nat = \"2026-09-05T18:00:00\"\nkind = \"highlight\"\n\
                    title = \"A\"\nlocation = 120\nshown = true\ncomment = \"Mine\"\n";
        std::fs::write(dir.join(HIGHLIGHTS_FILE), held).unwrap();
        let mut read = Edits::load(&dir).unwrap();
        read.highlights
            .push(HighlightEdit::of(&passage("2026-09-07T18:00:00")));
        read.save(&dir).unwrap();
        let first = std::fs::read(dir.join(HIGHLIGHTS_FILE)).unwrap();

        // The row stating nothing is left out, and a key no field reads stays.
        let again = Edits::load(&dir).unwrap();
        let titles: Vec<&str> = again.highlights.iter().map(|h| h.title.as_str()).collect();
        assert_eq!(titles, ["A", "B"]);
        assert_eq!(
            again.highlights[0]
                .rest
                .get("comment")
                .and_then(|v| v.as_str()),
            Some("Mine")
        );
        assert_eq!(
            again.highlights[0]
                .rest
                .get("shown")
                .and_then(|v| v.as_bool()),
            Some(true)
        );
        assert_eq!(again.highlights[1].text.as_deref(), Some("Fixed"));
        again.save(&dir).unwrap();
        assert_eq!(std::fs::read(dir.join(HIGHLIGHTS_FILE)).unwrap(), first);
        assert_eq!(
            Edits::load(&scratch("edits-none")).unwrap(),
            Edits::default()
        );
    }
}
