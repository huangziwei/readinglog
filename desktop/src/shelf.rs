//! What the app's windows list and change: every book in the record with its
//! edit, one book's highlights with theirs, and the posts. Each row is stated
//! over the whole record, the public one being a choice among them.

use anyhow::{Context, Result, anyhow};
use serde::{Deserialize, Serialize};

use readinglog_native::clippings::Kind;
use readinglog_native::date;
use readinglog_native::highlights::Highlight;
use readinglog_native::stats::{BookStat, Stats};
use readinglog_native::view::highlights as highlights_view;

use crate::edits::Edits;
use crate::posts;
use crate::workspace::{POSTS, Workspace, write_file};

/// One book of the record, as the Books window lists it.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct BookRow {
    pub key: String,
    pub extent: i64,
    /// The title and author the site states, and the record's own.
    pub title: String,
    pub author: String,
    pub record_title: String,
    pub record_author: String,
    pub language: String,
    /// The whole record's reading, as the Kindle's Books list states it.
    pub read: String,
    /// `BookStat::percent_shown`, where the library states a progress.
    pub percent: Option<i64>,
    pub finished: bool,
    /// The day it was last read, `YYYY-MM-DD`, empty for a book with none.
    pub last_read: String,
    /// Whether it is on the site, and whether its own row decides it.
    pub shown: bool,
    pub chosen: bool,
    /// Its highlights as its Highlights tab counts them, and its row's `highlights`.
    pub highlights: usize,
    pub highlights_shown: Option<bool>,
    /// Its address on the site, once one is pinned.
    pub slug: Option<String>,
}

/// What names one highlight across imports: `Highlight::identity`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct HighlightId {
    pub at: String,
    pub kind: String,
    pub title: String,
    pub location: i64,
    pub start: i64,
}

impl HighlightId {
    fn of(highlight: &Highlight) -> Self {
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
    fn highlight(&self) -> Result<Highlight> {
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

/// One `a` row of a book, with its edit.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Entry {
    pub id: HighlightId,
    /// The record's own words.
    pub body: String,
    /// Its correction and its comment.
    pub text: Option<String>,
    pub comment: Option<String>,
    /// Whether it is shown, and whether its own row says so.
    pub shown: bool,
    pub chosen: bool,
}

/// One row of a book's highlights, as its Highlights tab lists them: a
/// passage and the note written on it.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct HighlightRow {
    pub passage: Entry,
    pub note: Option<Entry>,
    /// Where it falls and what kind it is, as the Kindle labels the row.
    pub label: String,
    /// The day it was made, `YYYY-MM-DD`.
    pub made: String,
}

/// One post, as the Posts window lists it.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct PostRow {
    /// Its file's name under `posts/`, without `.md`.
    pub name: String,
    pub slug: String,
    pub title: String,
    pub date: String,
    pub draft: bool,
    pub books: Vec<String>,
}

/// The whole record's stats under `site.toml`'s settings, every identified
/// book listed.
fn whole(ws: &Workspace) -> Stats {
    let (settings, _) = ws.site.settings();
    let record = ws.record();
    let today = record
        .sittings
        .iter()
        .filter_map(|s| date::parse_day(date::day_of(&s.started_at)))
        .max()
        .unwrap_or(0);
    Stats::build(
        &record,
        today,
        false,
        settings.stats_from,
        settings.sitting_floor,
    )
}

/// Every book in the record, most recently read first.
pub fn books(ws: &Workspace) -> Result<Vec<BookRow>> {
    let edits = Edits::load(&ws.dir)?;
    let stats = whole(ws);
    let (settings, _) = ws.site.settings();
    let s = ws.site.lang().strings();
    Ok(stats
        .books
        .iter()
        .enumerate()
        .map(|(at, book)| {
            let edit = edits.book(&book.cde_key, book.extent);
            BookRow {
                key: book.cde_key.clone(),
                extent: book.extent,
                title: edit
                    .and_then(|e| e.title.clone())
                    .unwrap_or_else(|| book.title.clone()),
                author: edit
                    .and_then(|e| e.author.clone())
                    .unwrap_or_else(|| book.author.clone()),
                record_title: book.title.clone(),
                record_author: book.author.clone(),
                language: book.language.clone(),
                read: date::duration(book.read_seconds(settings.stats_from), s),
                percent: book.has_percent().then(|| book.percent_shown()),
                finished: book.is_finished(),
                last_read: iso_of(book),
                shown: edits.book_shown(&book.cde_key, book.extent, ws.site.new_books),
                chosen: edit.and_then(|e| e.shown).is_some(),
                highlights: stats.highlights_held(at),
                highlights_shown: edit.and_then(|e| e.highlights),
                slug: edit.and_then(|e| e.slug.clone()),
            }
        })
        .collect())
}

/// `book`'s last day as `YYYY-MM-DD`, empty where it has no reading.
fn iso_of(book: &BookStat) -> String {
    match book.sittings {
        0 => String::new(),
        _ => iso(book.last_day),
    }
}

fn iso(day: i64) -> String {
    let (y, m, d) = date::civil_from_days(day);
    format!("{y:04}-{m:02}-{d:02}")
}

/// One book's highlights, as its Highlights tab lists them.
pub fn highlights(ws: &Workspace, key: &str, extent: i64) -> Result<Vec<HighlightRow>> {
    let edits = Edits::load(&ws.dir)?;
    let stats = whole(ws);
    let s = ws.site.lang().strings();
    // By key where the book carries one, else by extent, as `books.toml` names it.
    let at = stats
        .books
        .iter()
        .position(|b| match key.is_empty() {
            false => b.cde_key == key,
            true => b.cde_key.is_empty() && b.extent == extent,
        })
        .ok_or_else(|| anyhow!("no book {key} {extent} in the record"))?;
    let book = &stats.books[at];
    let entry = |highlight: &Highlight| {
        let edit = edits.highlight(highlight);
        Entry {
            id: HighlightId::of(highlight),
            body: highlight.body.clone(),
            text: edit.and_then(|e| e.text.clone()),
            comment: edit.and_then(|e| e.comment.clone()),
            shown: edits.highlight_shown(highlight, &book.cde_key, book.extent),
            chosen: edit.and_then(|e| e.shown).is_some(),
        }
    };
    Ok(stats
        .book_highlights(at)
        .into_iter()
        .map(|row| HighlightRow {
            passage: entry(row.highlight),
            note: row.note.map(entry),
            label: highlights_view::label(row.highlight, book.extent, s),
            made: row.highlight.day().map(iso).unwrap_or_default(),
        })
        .collect())
}

/// A choice, or the default in its place.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Choice {
    Shown,
    Hidden,
    Default,
}

impl Choice {
    /// The choice as an edit row holds it.
    fn stated(self) -> Option<bool> {
        match self {
            Choice::Shown => Some(true),
            Choice::Hidden => Some(false),
            Choice::Default => None,
        }
    }
}

/// A change to one book's row. A field left out leaves its choice as it
/// stands; an empty title or author gives the record's back.
#[derive(Debug, Default, Clone, PartialEq, Deserialize)]
pub struct BookChange {
    pub shown: Option<bool>,
    pub highlights: Option<Choice>,
    pub title: Option<String>,
    pub author: Option<String>,
}

/// Write `change` into `books.toml` for the book `key` and `extent` name.
pub fn change_book(ws: &Workspace, key: &str, extent: i64, change: BookChange) -> Result<()> {
    let mut edits = Edits::load(&ws.dir)?;
    let row = edits.book_mut(key, extent);
    if let Some(shown) = change.shown {
        row.shown = Some(shown);
    }
    if let Some(highlights) = change.highlights {
        row.highlights = highlights.stated();
    }
    if let Some(title) = change.title {
        row.title = Some(title.trim().to_string()).filter(|t| !t.is_empty());
    }
    if let Some(author) = change.author {
        row.author = Some(author.trim().to_string()).filter(|a| !a.is_empty());
    }
    edits.save(&ws.dir)
}

/// Show or hide every book at once, in each one's own row.
pub fn show_all(ws: &Workspace, shown: bool) -> Result<()> {
    let mut edits = Edits::load(&ws.dir)?;
    for book in &whole(ws).books {
        edits.book_mut(&book.cde_key, book.extent).shown = Some(shown);
    }
    edits.save(&ws.dir)
}

/// A change to one highlight's row, as [`BookChange`] is to a book's. An empty
/// correction or comment takes it away.
#[derive(Debug, Default, Clone, PartialEq, Deserialize)]
pub struct HighlightChange {
    pub shown: Option<Choice>,
    pub text: Option<String>,
    pub comment: Option<String>,
}

/// Write `change` into `highlights.toml` for the highlight `id` names.
pub fn change_highlight(ws: &Workspace, id: &HighlightId, change: HighlightChange) -> Result<()> {
    let mut edits = Edits::load(&ws.dir)?;
    let row = edits.highlight_mut(&id.highlight()?);
    if let Some(shown) = change.shown {
        row.shown = shown.stated();
    }
    if let Some(text) = change.text {
        row.text = Some(text).filter(|t| !t.trim().is_empty());
    }
    if let Some(comment) = change.comment {
        row.comment = Some(comment).filter(|c| !c.trim().is_empty());
    }
    edits.save(&ws.dir)
}

/// Every post, newest first, and a line for each file that is not one.
pub fn post_rows(ws: &Workspace) -> (Vec<PostRow>, Vec<String>) {
    let (found, problems) = posts::load(&ws.posts());
    let rows = found
        .into_iter()
        .map(|post| PostRow {
            name: post.name.clone(),
            slug: post.slug.clone(),
            title: post.title.clone(),
            date: iso(post.date),
            draft: post.draft,
            books: post.books.clone(),
        })
        .collect();
    (rows, problems)
}

/// The text of the post `name`.
pub fn read_post(ws: &Workspace, name: &str) -> Result<String> {
    let at = post_file(ws, name)?;
    std::fs::read_to_string(&at).with_context(|| format!("reading {}", at.display()))
}

/// Write `text` as the post `name`.
pub fn write_post(ws: &Workspace, name: &str, text: &str) -> Result<()> {
    write_file(&post_file(ws, name)?, text.as_bytes())
}

/// A new post titled `title`, dated `day`, naming `books`. Answers its name.
pub fn new_post(ws: &Workspace, title: &str, day: i64, books: &[String]) -> Result<String> {
    let stem = match posts::slug(title) {
        stem if stem.is_empty() => "post".to_string(),
        stem => stem,
    };
    let name = (1..)
        .map(|n| match n {
            1 => stem.clone(),
            n => format!("{stem}-{n}"),
        })
        .find(|name| !ws.posts().join(format!("{name}.md")).exists())
        .expect("a free name");
    let books = books
        .iter()
        .map(|b| format!("{b:?}"))
        .collect::<Vec<_>>()
        .join(", ");
    let text = format!(
        "+++\ntitle = {title:?}\ndate = {}\nbooks = [{books}]\ndraft = true\n+++\n\n",
        iso(day)
    );
    write_post(ws, &name, &text)?;
    Ok(name)
}

/// The file of the post `name`, refusing a name that reaches outside `posts/`.
fn post_file(ws: &Workspace, name: &str) -> Result<std::path::PathBuf> {
    let plain = !name.is_empty()
        && !name.starts_with('.')
        && !name.contains(['/', '\\'])
        && !name.contains("..");
    match plain {
        true => Ok(ws.dir.join(POSTS).join(format!("{name}.md"))),
        false => Err(anyhow!("{name} is not a post's name")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tests::{book, highlight, record_of, scratch, sitting};

    fn workspace(name: &str) -> Workspace {
        let ws = Workspace::create(&scratch(name)).unwrap();
        let mut record = record_of(
            vec![
                book(100, "B1", "A Map of Small Rivers"),
                book(200, "B2", "Two"),
            ],
            vec![
                sitting("2026-09-01T09:00:00", 100, 600),
                sitting("2026-09-02T09:00:00", 200, 1200),
            ],
        );
        record.highlights = vec![highlight(
            100,
            "A Map of Small Rivers",
            "2026-09-01T09:05:00",
        )];
        record.save(&ws.record_dir()).unwrap();
        ws
    }

    #[test]
    fn the_books_window_lists_every_book_with_the_readers_choice() {
        let ws = workspace("shelf-books");
        let rows = books(&ws).unwrap();
        assert_eq!(rows.len(), 2);
        assert!(rows.iter().all(|r| r.shown && !r.chosen));
        change_book(
            &ws,
            "B1",
            100,
            BookChange {
                shown: Some(false),
                title: Some("  Retitled ".into()),
                ..BookChange::default()
            },
        )
        .unwrap();
        let rows = books(&ws).unwrap();
        let one = rows.iter().find(|r| r.key == "B1").unwrap();
        assert!(!one.shown && one.chosen);
        assert_eq!(
            (one.title.as_str(), one.record_title.as_str()),
            ("Retitled", "A Map of Small Rivers")
        );
        assert_eq!(one.highlights, 1);
        change_book(
            &ws,
            "B1",
            100,
            BookChange {
                title: Some(String::new()),
                ..BookChange::default()
            },
        )
        .unwrap();
        assert_eq!(
            books(&ws)
                .unwrap()
                .iter()
                .find(|r| r.key == "B1")
                .unwrap()
                .title,
            "A Map of Small Rivers"
        );
        show_all(&ws, false).unwrap();
        assert!(books(&ws).unwrap().iter().all(|r| !r.shown));
    }

    #[test]
    fn a_highlight_is_found_and_changed_by_its_identity() {
        let ws = workspace("shelf-highlights");
        let rows = highlights(&ws, "B1", 100).unwrap();
        assert_eq!(rows.len(), 1);
        assert!(!rows[0].passage.shown);
        let id = rows[0].passage.id.clone();
        change_highlight(
            &ws,
            &id,
            HighlightChange {
                shown: Some(Choice::Shown),
                text: Some("Better words".into()),
                comment: Some("  ".into()),
            },
        )
        .unwrap();
        let row = &highlights(&ws, "B1", 100).unwrap()[0];
        assert!(row.passage.shown && row.passage.chosen);
        assert_eq!(row.passage.text.as_deref(), Some("Better words"));
        assert_eq!(row.passage.comment, None);
        assert!(highlights(&ws, "B9", 9).is_err());
    }

    #[test]
    fn the_window_says_a_choice_in_words_and_leaves_the_rest_alone() {
        let change: BookChange = serde_json::from_str(r#"{"highlights":"default"}"#).unwrap();
        assert_eq!(change.highlights, Some(Choice::Default));
        assert_eq!((change.shown, change.title), (None, None));
        let change: HighlightChange =
            serde_json::from_str(r#"{"shown":"hidden","text":""}"#).unwrap();
        assert_eq!(change.shown.and_then(Choice::stated), Some(false));
        assert_eq!(change.text.as_deref(), Some(""));
        assert!(serde_json::from_str::<BookChange>(r#"{"highlights":true}"#).is_err());
    }

    #[test]
    fn a_new_post_is_a_draft_under_a_free_name() {
        let ws = workspace("shelf-posts");
        let day = date::days_from_civil(2026, 10, 11);
        let first = new_post(&ws, "On \"Rivers\"", day, &["B1".into()]).unwrap();
        let second = new_post(&ws, "On \"Rivers\"", day, &[]).unwrap();
        assert_eq!(
            (first.as_str(), second.as_str()),
            ("on-rivers", "on-rivers-2")
        );
        let (rows, problems) = post_rows(&ws);
        assert!(problems.is_empty(), "{problems:?}");
        assert_eq!(rows.len(), 2);
        assert!(
            rows.iter()
                .all(|r| r.draft && r.title == "On \"Rivers\"" && r.date == "2026-10-11")
        );
        assert!(
            read_post(&ws, "on-rivers")
                .unwrap()
                .contains("books = [\"B1\"]")
        );
        assert!(write_post(&ws, "../escape", "x").is_err());
    }
}
