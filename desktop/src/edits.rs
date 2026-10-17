//! Corrections of a highlight's or a note's words: `r` rows of the record of
//! the Kindle stating them; [`take_file`] reads a workspace's `highlights.toml`
//! into those rows.

use std::collections::HashMap;

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};

use readinglog_native::record::Record;
use readinglog_native::written::Correction;
pub use readinglog_native::written::HighlightId;

use crate::workspace::{Workspace, keep_written, now, write_toml};

pub const HIGHLIGHTS_FILE: &str = "highlights.toml";

/// Set each of `changes`, a highlight and the words it is to state, in the
/// record of the Kindle whose cut serial is `kindle`, as
/// `Record::set_words` sets one.
pub fn correct_words(
    ws: &Workspace,
    kindle: &str,
    changes: &[(HighlightId, Option<String>)],
) -> Result<()> {
    let mut source = ws.source(kindle)?;
    let at = now();
    for (id, text) in changes {
        source.record.set_words(id, text.clone(), &at)?;
    }
    keep_written(&source)
}

/// One `[[highlight]]` of `highlights.toml`.
#[derive(Debug, Default, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
struct HighlightEdit {
    at: String,
    kind: String,
    title: String,
    location: i64,
    #[serde(default = "unplaced", skip_serializing_if = "is_unplaced")]
    start: i64,
    #[serde(skip_serializing_if = "String::is_empty")]
    kindle: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    text: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    was: Option<String>,
    /// Every key no correction reads, kept in the file as it was read.
    #[serde(flatten)]
    rest: toml::Table,
}

fn unplaced() -> i64 {
    -1
}

fn is_unplaced(n: &i64) -> bool {
    *n == -1
}

#[derive(Default, Serialize, Deserialize)]
struct HighlightsFile {
    #[serde(default, rename = "highlight")]
    rows: Vec<HighlightEdit>,
}

/// Read `ws`'s `highlights.toml` into the `r` rows of each Kindle's record.
/// The file keeps a row naming a record the workspace holds none of, and the
/// keys no correction reads; it goes once it keeps nothing.
pub fn take_file(ws: &Workspace) -> Result<()> {
    let path = ws.dir.join(HIGHLIGHTS_FILE);
    let text = match std::fs::read_to_string(&path) {
        Ok(text) => text,
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => return Ok(()),
        Err(err) => return Err(err).with_context(|| format!("reading {}", path.display())),
    };
    let file: HighlightsFile =
        toml::from_str(&text).with_context(|| format!("reading {}", path.display()))?;
    let mut sources = ws.sources();
    let at = now();
    let mut taken: HashMap<usize, Record> = HashMap::new();
    let mut left = Vec::new();
    for row in file.rows {
        let held = sources
            .iter()
            .position(|s| s.serial.as_deref().unwrap_or_default() == row.kindle);
        let (Some(held), Some(words)) = (held, row.text.clone()) else {
            if row.text.is_some() || !row.rest.is_empty() {
                left.push(row);
            }
            continue;
        };
        taken.entry(held).or_default().corrections.push(Correction {
            id: HighlightId {
                at: row.at.clone(),
                kind: row.kind.clone(),
                title: row.title.clone(),
                location: row.location,
                start: row.start,
            },
            edited: at.clone(),
            text: Some(words),
            was: row.was.clone(),
        });
        if !row.rest.is_empty() {
            left.push(HighlightEdit {
                text: None,
                was: None,
                ..row
            });
        }
    }
    for (held, rows) in taken {
        sources[held].record.take_written(&rows);
        keep_written(&sources[held])?;
    }
    match left.is_empty() {
        true => std::fs::remove_file(&path).with_context(|| format!("removing {}", path.display())),
        false => write_toml(&path, &HighlightsFile { rows: left }),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tests::{book, highlight, record_of, scratch, sitting};
    use crate::workspace::{KINDLES, RECORD};

    #[test]
    fn highlights_toml_is_read_once_into_the_records_and_keeps_what_no_correction_reads() {
        let dir = scratch("edits-taken");
        let ws = Workspace::create(&dir).unwrap();
        let mut record = record_of(
            vec![book(7, "B00KEY", "A Book")],
            vec![sitting("2026-09-05T18:00:00", 7, 600)],
        );
        record.highlights = vec![highlight(7, "A Book", "2026-09-05T18:22:26")];
        let kindle = dir.join(KINDLES).join("G003H2-0001").join(RECORD);
        record.save(&kindle).unwrap();
        let id = HighlightId::of(&record.highlights[0]);
        std::fs::write(
            dir.join(HIGHLIGHTS_FILE),
            format!(
                "[[highlight]]\nat = \"{}\"\nkind = \"{}\"\ntitle = \"{}\"\nlocation = {}\n\
                 kindle = \"G003H2-0001\"\ntext = \"Fixed\"\nwas = \"Some words\"\n\n\
                 [[highlight]]\nat = \"2026-09-05T18:00:00\"\nkind = \"highlight\"\n\
                 title = \"B\"\nlocation = 120\nshown = true\n",
                id.at, id.kind, id.title, id.location
            ),
        )
        .unwrap();
        take_file(&ws).unwrap();
        let read = Record::load(&kindle);
        assert_eq!(read.corrections.len(), 1);
        assert_eq!(read.corrections[0].text.as_deref(), Some("Fixed"));
        assert_eq!(read.drawn().record.highlights[0].body, "Fixed");
        let kept = std::fs::read_to_string(dir.join(HIGHLIGHTS_FILE)).unwrap();
        assert!(
            kept.contains("shown = true") && !kept.contains("Fixed"),
            "{kept}"
        );
        // `take_file` again moves nothing.
        let before = std::fs::read(Record::file(&kindle)).unwrap();
        take_file(&ws).unwrap();
        assert_eq!(std::fs::read(Record::file(&kindle)).unwrap(), before);
    }

    #[test]
    fn a_correction_is_kept_in_the_record_and_given_back_there() {
        let dir = scratch("edits-record");
        let ws = Workspace::create(&dir).unwrap();
        let mut record = record_of(
            vec![book(7, "B00KEY", "A Book")],
            vec![sitting("2026-09-05T18:00:00", 7, 600)],
        );
        record.highlights = vec![highlight(7, "A Book", "2026-09-05T18:22:26")];
        let kindle = dir.join(KINDLES).join("G003H2-0001").join(RECORD);
        record.save(&kindle).unwrap();
        let id = HighlightId::of(&record.highlights[0]);
        correct_words(
            &ws,
            "G003H2-0001",
            &[(id.clone(), Some("Some words.".into()))],
        )
        .unwrap();
        let read = Record::load(&kindle);
        assert_eq!(read.drawn().record.highlights[0].body, "Some words.");
        assert_eq!(
            read.highlights[0].body, "Some words",
            "the Kindle's own stand beside it"
        );
        correct_words(&ws, "G003H2-0001", &[(id, None)]).unwrap();
        let back = Record::load(&kindle);
        assert!(back.drawn().corrected.is_empty());
        assert_eq!(back.corrections[0].text, None, "taken back, its row stands");
    }
}
