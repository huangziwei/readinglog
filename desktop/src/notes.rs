//! Notes written on the desktop, each a Markdown document: `w` rows of the
//! record of the Kindle whose book they are on. [`take_file`] and
//! [`take_markdown`] read a workspace's `notes.toml` and `posts/` into those rows.

use std::collections::{HashMap, HashSet};
use std::path::PathBuf;

use anyhow::{Context, Result, anyhow};
use serde::Deserialize;

use readinglog_native::date;
use readinglog_native::record::Record;
pub use readinglog_native::written::Laid;
use readinglog_native::written::{HighlightId, Note, Passage};

use crate::kindles::Source;
use crate::workspace::{MARKDOWN, Workspace, keep_written, now};

pub const NOTES_FILE: &str = "notes.toml";

/// `markdown` as HTML: CommonMark with tables, footnotes, strikethrough and task
/// lists, any HTML it holds set as its own text.
pub fn html(markdown: &str) -> String {
    use pulldown_cmark::{Event, Options, Parser, html::push_html};
    let options = Options::ENABLE_TABLES
        | Options::ENABLE_FOOTNOTES
        | Options::ENABLE_STRIKETHROUGH
        | Options::ENABLE_TASKLISTS;
    let events = Parser::new_ext(markdown, options).map(|event| match event {
        Event::Html(raw) | Event::InlineHtml(raw) => Event::Text(raw),
        other => other,
    });
    let mut out = String::new();
    push_html(&mut out, events);
    out
}

/// What a note is written on, as the window names it.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum On {
    /// A passage, by its `HighlightId`.
    Passage(HighlightId),
    /// A book alone, by its key and extent as its Kindle states them.
    Book { key: String, extent: i64 },
}

/// Write `text` as a new note in the record of the Kindle whose cut serial is
/// `kindle`, on `on`, at the clock's moment. Answers the moment that names it.
pub fn write(ws: &Workspace, kindle: &str, on: &On, text: &str) -> Result<String> {
    let mut source = ws.source(kindle)?;
    let note = noted(&source.record, kindle, on, text, &now())?;
    let written = source.record.add_note(note);
    keep_written(&source)?;
    Ok(written)
}

/// The note `text` makes on `on` in `record`, the record of the Kindle `kindle`
/// names, written at `at`.
fn noted(record: &Record, kindle: &str, on: &On, text: &str, at: &str) -> Result<Note> {
    if text.trim().is_empty() {
        return Err(anyhow!("a note holds no words"));
    }
    let (book, passage) = match on {
        On::Book { key, extent } => {
            let book = record
                .books
                .iter()
                .find(|b| match key.is_empty() {
                    false => b.cde_key == *key,
                    true => b.cde_key.is_empty() && b.extent == *extent,
                })
                .ok_or_else(|| anyhow!("no book {key} in the record of {kindle}"))?;
            (book, None)
        }
        On::Passage(id) => {
            let held = record
                .highlights
                .iter()
                .find(|h| id.names(h))
                .ok_or_else(|| anyhow!("no highlight at {} in {}", id.at, id.title))?;
            let book = record
                .books
                .iter()
                .find(|b| held.extent != 0 && b.extent == held.extent)
                .or_else(|| record.books.iter().find(|b| b.title == held.title))
                .ok_or_else(|| anyhow!("no book holds the highlight at {}", id.at))?;
            let passage = Passage {
                id: HighlightId::of(held),
                from: held.start,
                to: held.end,
                words: held.body.clone(),
            };
            (book, Some(passage))
        }
    };
    Ok(Note {
        written: at.to_string(),
        edited: at.to_string(),
        key: book.cde_key.clone(),
        extent: book.extent,
        on: passage,
        text: text.to_string(),
        deleted: false,
    })
}

/// Set the words of the note written at `written` in the Kindle `kindle`'s record.
pub fn reword(ws: &Workspace, kindle: &str, written: &str, text: &str) -> Result<()> {
    let mut source = ws.source(kindle)?;
    source.record.reword_note(written, text, &now())?;
    keep_written(&source)
}

/// Delete the note written at `written` in the Kindle `kindle`'s record.
pub fn remove(ws: &Workspace, kindle: &str, written: &str) -> Result<()> {
    let mut source = ws.source(kindle)?;
    source.record.delete_note(written, &now())?;
    keep_written(&source)
}

/// One `[[note]]` of `notes.toml`.
#[derive(Debug, Default, Clone, PartialEq, Deserialize)]
#[serde(default)]
struct Held {
    written: String,
    kindle: String,
    key: String,
    extent: i64,
    text: String,
    on: Option<HeldPassage>,
}

/// The passage of one `[[note]]`.
#[derive(Debug, Clone, PartialEq, Deserialize)]
struct HeldPassage {
    #[serde(flatten)]
    id: HighlightId,
    from: i64,
    to: i64,
    words: String,
}

#[derive(Default, Deserialize)]
struct NotesFile {
    #[serde(default, rename = "note")]
    rows: Vec<Held>,
}

/// Read `ws`'s `notes.toml` into the `w` rows of each Kindle's record; the file
/// goes once each of its notes stands in one, and stays while one names a
/// record the workspace holds none of.
pub fn take_file(ws: &Workspace) -> Result<()> {
    let path = ws.dir.join(NOTES_FILE);
    let text = match std::fs::read_to_string(&path) {
        Ok(text) => text,
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => return Ok(()),
        Err(err) => return Err(err).with_context(|| format!("reading {}", path.display())),
    };
    let file: NotesFile =
        toml::from_str(&text).with_context(|| format!("reading {}", path.display()))?;
    let mut sources = ws.sources();
    let mut taken: HashMap<usize, Record> = HashMap::new();
    let mut unplaced = false;
    for row in file.rows {
        let Some(held) = sources
            .iter()
            .position(|s| s.serial.as_deref().unwrap_or_default() == row.kindle)
        else {
            unplaced = true;
            continue;
        };
        taken.entry(held).or_default().notes.push(Note {
            edited: row.written.clone(),
            written: row.written,
            key: row.key,
            extent: row.extent,
            on: row.on.map(|p| Passage {
                id: p.id,
                from: p.from,
                to: p.to,
                words: p.words,
            }),
            text: row.text,
            deleted: false,
        });
    }
    for (held, rows) in taken {
        sources[held].record.take_written(&rows);
        keep_written(&sources[held])?;
    }
    match unplaced {
        true => Ok(()),
        false => {
            std::fs::remove_file(&path).with_context(|| format!("removing {}", path.display()))
        }
    }
}

/// One Markdown file under `posts/`: what its `+++` front matter states, then
/// its words.
#[derive(Debug, PartialEq)]
struct Markdown {
    title: String,
    day: Option<i64>,
    books: Vec<String>,
    words: String,
}

#[derive(Deserialize)]
struct Front {
    #[serde(default)]
    title: String,
    date: Option<toml::Value>,
    #[serde(default)]
    books: Vec<String>,
}

impl Markdown {
    /// The file `text` holds, `None` where it opens on no front matter.
    fn of(text: &str) -> Option<Self> {
        let text = text
            .strip_prefix('\u{feff}')
            .unwrap_or(text)
            .replace("\r\n", "\n");
        let rest = text.strip_prefix("+++\n")?;
        let (front, words) = rest
            .split_once("\n+++\n")
            .or_else(|| rest.strip_suffix("\n+++").map(|front| (front, "")))?;
        let front: Front = toml::from_str(front).ok()?;
        let day = front.date.and_then(|value| match value {
            toml::Value::Datetime(at) => at
                .date
                .map(|d| date::days_from_civil(d.year.into(), d.month.into(), d.day.into())),
            toml::Value::String(said) => date::parse_day(said.trim()),
            _ => None,
        });
        Some(Self {
            title: front.title.trim().to_string(),
            day,
            books: front.books,
            words: words.trim().to_string(),
        })
    }

    /// The note's words: `title` as a `# ` heading over the rest.
    fn text(&self) -> String {
        match self.title.is_empty() {
            true => self.words.clone(),
            false => format!("# {}\n\n{}", self.title, self.words),
        }
    }
}

/// The first book `named` reaches among `sources`, as `SERIAL/KEY`, a bare key,
/// or `e` and an extent: the source's index, and the book's key and extent.
fn book_named(sources: &[Source], named: &str) -> Option<(usize, String, i64)> {
    let (serial, key) = match named.split_once('/') {
        Some((serial, key)) => (Some(serial), key),
        None => (None, named),
    };
    let extent = key.strip_prefix('e').and_then(|n| n.parse::<i64>().ok());
    sources.iter().enumerate().find_map(|(at, source)| {
        if serial.is_some_and(|s| source.serial.as_deref() != Some(s)) {
            return None;
        }
        source
            .record
            .books
            .iter()
            .find(|b| b.cde_key == key || (b.cde_key.is_empty() && Some(b.extent) == extent))
            .map(|b| (at, b.cde_key.clone(), b.extent))
    })
}

/// Read `ws`'s `posts/` into notes once: each Markdown file a note on the first
/// book its front matter names, written at noon on its day, the file gone once
/// the note stands; a file naming no book the workspace holds stays.
pub fn take_markdown(ws: &Workspace) -> Result<()> {
    let dir = ws.dir.join(MARKDOWN);
    let Ok(entries) = std::fs::read_dir(&dir) else {
        return Ok(());
    };
    let mut files: Vec<PathBuf> = entries
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.extension().is_some_and(|x| x == "md"))
        .collect();
    if files.is_empty() {
        return Ok(());
    }
    files.sort();
    let mut sources = ws.sources();
    let mut taken = Vec::new();
    for path in files {
        let text = std::fs::read_to_string(&path)
            .with_context(|| format!("reading {}", path.display()))?;
        let Some(file) = Markdown::of(&text) else {
            continue;
        };
        let Some((at, key, extent)) = file.books.iter().find_map(|b| book_named(&sources, b))
        else {
            continue;
        };
        let written = date::stamp(file.day.unwrap_or_else(|| date::now().0), 12 * 3600);
        sources[at].record.add_note(Note {
            edited: written.clone(),
            written,
            key,
            extent,
            on: None,
            text: file.text(),
            deleted: false,
        });
        taken.push((at, path));
    }
    let touched: HashSet<usize> = taken.iter().map(|(at, _)| *at).collect();
    for at in touched {
        keep_written(&sources[at])?;
    }
    for (_, path) in taken {
        std::fs::remove_file(&path).with_context(|| format!("removing {}", path.display()))?;
    }
    // `remove_dir` takes the folder only where nothing is left in it.
    let _ = std::fs::remove_dir(&dir);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tests::{book, highlight, record_of, scratch, sitting};
    use crate::workspace::{KINDLES, RECORD};

    const KINDLE: &str = "G003H2-0001";

    /// A workspace holding one Kindle's record: one book at extent 7 and one
    /// passage at 100..200 in it. Answers it and the record's folder.
    fn workspace(name: &str) -> (Workspace, std::path::PathBuf) {
        let dir = scratch(name);
        let ws = Workspace::create(&dir).unwrap();
        let mut record = record_of(
            vec![book(7, "B00KEY", "A Book")],
            vec![sitting("2026-10-09T16:00:00", 7, 600)],
        );
        let mut passage = highlight(7, "A Book", "2026-10-09T16:54:03");
        (passage.start, passage.end) = (100, 200);
        record.highlights = vec![passage];
        let at = dir.join(KINDLES).join(KINDLE).join(RECORD);
        record.save(&at).unwrap();
        (ws, at)
    }

    #[test]
    fn a_note_is_written_reworded_and_deleted_in_its_kindles_record() {
        let (ws, at) = workspace("notes-record");
        let id = HighlightId::of(&Record::load(&at).highlights[0]);
        let on = write(&ws, KINDLE, &On::Passage(id), "on the passage").unwrap();
        let alone = write(
            &ws,
            KINDLE,
            &On::Book {
                key: "B00KEY".into(),
                extent: 7,
            },
            "# A title\n\nalone",
        )
        .unwrap();
        let read = Record::load(&at);
        assert_eq!(read.notes.len(), 2);
        assert_eq!(read.drawn().written.len(), 2);
        reword(&ws, KINDLE, &on, "reworded").unwrap();
        remove(&ws, KINDLE, &alone).unwrap();
        let read = Record::load(&at);
        let words: Vec<&str> = read.notes.iter().map(|n| n.text.as_str()).collect();
        assert!(words.contains(&"reworded"), "{words:?}");
        assert_eq!(
            read.drawn().written.len(),
            1,
            "the deleted one draws nowhere"
        );
        assert_eq!(read.notes.len(), 2, "and its row stands");
        assert!(
            write(
                &ws,
                KINDLE,
                &On::Passage(HighlightId::of(&read.highlights[0])),
                " "
            )
            .is_err()
        );
    }

    #[test]
    fn notes_toml_is_read_once_into_the_records() {
        let (ws, at) = workspace("notes-taken");
        let id = HighlightId::of(&Record::load(&at).highlights[0]);
        std::fs::write(
            ws.dir.join(NOTES_FILE),
            format!(
                "[[note]]\nwritten = \"2026-10-10T08:00:00\"\nkindle = \"{KINDLE}\"\n\
                 key = \"B00KEY\"\nextent = 7\ntext = \"on it\"\n\n[note.on]\nat = \"{}\"\n\
                 kind = \"{}\"\ntitle = \"{}\"\nlocation = {}\nstart = {}\nfrom = 100\nto = 200\n\
                 words = \"Some words\"\n\n[[note]]\nwritten = \"2026-10-11T08:00:00\"\n\
                 kindle = \"{KINDLE}\"\nkey = \"B00KEY\"\nextent = 7\ntext = \"alone\"\n",
                id.at, id.kind, id.title, id.location, id.start
            ),
        )
        .unwrap();
        take_file(&ws).unwrap();
        assert!(!ws.dir.join(NOTES_FILE).exists());
        let read = Record::load(&at);
        assert_eq!(read.notes.len(), 2);
        assert_eq!(read.notes[0].on.as_ref().map(|p| p.from), Some(100));
        assert_eq!(read.drawn().written.len(), 2);
    }

    #[test]
    fn markdown_is_drawn_and_html_in_it_is_its_text() {
        assert_eq!(
            html("# Head\n\nWords *here*."),
            "<h1>Head</h1>\n<p>Words <em>here</em>.</p>\n"
        );
        assert!(html("<script>x</script>").contains("&lt;script&gt;"));
    }

    #[test]
    fn posts_files_are_read_once_into_notes_on_their_first_book() {
        let (ws, at) = workspace("notes-markdown");
        let dir = ws.dir.join(MARKDOWN);
        std::fs::create_dir_all(&dir).unwrap();
        let file = |name: &str, books: &str, words: &str| {
            let front =
                format!("+++\ntitle = \"On it\"\ndate = 2026-10-12\nbooks = [\"{books}\"]\n+++\n");
            std::fs::write(dir.join(name), format!("{front}{words}\n")).unwrap();
        };
        file("on-it.md", &format!("{KINDLE}/B00KEY"), "The book opens.");
        file("nowhere.md", "NOBOOK", "Words");
        take_markdown(&ws).unwrap();
        let read = Record::load(&at);
        assert_eq!(read.notes.len(), 1);
        assert_eq!(read.notes[0].text, "# On it\n\nThe book opens.");
        assert_eq!(read.notes[0].written, "2026-10-12T12:00:00");
        assert!(!dir.join("on-it.md").exists());
        assert!(
            dir.join("nowhere.md").exists(),
            "a file naming no book here stays"
        );
        take_markdown(&ws).unwrap();
        assert_eq!(Record::load(&at).notes.len(), 1, "once");
    }
}
