//! [`Metadata`]: a `d` row of a Kindle's record, one field of one book. [`rows`] reads them off
//! a record's text, [`splice`] writes them into it, [`edit`] sets a [`Patch`] on books, and
//! [`Set::of`] is what one book's rows set.

mod authors;
mod lang;
pub mod names;

use std::fmt::Write as _;
use std::path::Path;

use anyhow::{Context, Result, anyhow};
use serde::{Deserialize, Serialize};

use readinglog_native::font::Script;
use readinglog_native::record::{Book, Record};
use readinglog_native::stats::BookStat;
use readinglog_native::written::{self, later};

use crate::workspace::{Workspace, write_file, writing};

/// `ROW`: the first field of a `d` row.
const ROW: &str = "d";

/// `NONE`: a row's field for a value it holds none of.
const NONE: &str = "\\N";

/// `Field`: one field of a book's metadata.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Field {
    Title,
    Authors,
    Publisher,
    Language,
    Tags,
}

impl Field {
    /// `self` as a row stores it.
    fn as_str(self) -> &'static str {
        match self {
            Field::Title => "title",
            Field::Authors => "authors",
            Field::Publisher => "publisher",
            Field::Language => "language",
            Field::Tags => "tags",
        }
    }

    /// The [`Field`] `word` names; `None` for any other word.
    fn from_stored(word: &str) -> Option<Field> {
        match word.trim() {
            "title" => Some(Field::Title),
            "authors" => Some(Field::Authors),
            "publisher" => Some(Field::Publisher),
            "language" => Some(Field::Language),
            "tags" => Some(Field::Tags),
            _ => None,
        }
    }
}

/// `Tag`: its `type` and its `tag`, the two fields a tag is typed in.
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
pub struct Tag {
    pub r#type: String,
    pub tag: String,
}

/// `Metadata`: a `d` row, one field of one book and the value set on it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Metadata {
    /// `edited`: `YYYY-MM-DDTHH:MM:SS`; [`take`] keeps the later of two copies.
    pub edited: String,
    /// `key`: the book's `cde_key`, empty where it carries none; `extent`, its extent.
    pub key: String,
    pub extent: i64,
    pub field: Field,
    /// `value`: the value set, `None` once taken back; [`tags_value`] lays tags out.
    pub value: Option<String>,
    /// `was`: the book's own title or author when `value` was set.
    pub was: Option<String>,
}

impl Metadata {
    /// Whether this row is of the book `key` and `extent` name: by key where
    /// it carries one, else by extent.
    pub fn names(&self, key: &str, extent: i64) -> bool {
        match self.key.is_empty() {
            false => self.key == key,
            true => key.is_empty() && self.extent == extent,
        }
    }

    /// What names this row among a record's: its book and its field.
    fn id(&self) -> (&str, i64, Field) {
        let extent = match self.key.is_empty() {
            true => self.extent,
            false => 0,
        };
        (&self.key, extent, self.field)
    }
}

/// `Set`: what one book's rows set, each field `None` where the book's own stands.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct Set {
    pub title: Option<String>,
    pub authors: Option<String>,
    pub publisher: Option<String>,
    pub language: Option<String>,
    pub tags: Vec<Tag>,
}

impl Set {
    /// What `rows` set on the book `key` and `extent` name, over whatever its own
    /// row states, then or since.
    pub fn of(rows: &[Metadata], key: &str, extent: i64) -> Set {
        let mut out = Set::default();
        for row in rows.iter().filter(|r| r.names(key, extent)) {
            let Some(value) = row.value.clone().filter(|v| !v.is_empty()) else {
                continue;
            };
            match row.field {
                Field::Title => out.title = Some(value),
                Field::Authors => out.authors = Some(value),
                Field::Publisher => out.publisher = Some(value),
                Field::Language => out.language = Some(value),
                Field::Tags => out.tags = tags_of(&value),
            }
        }
        out
    }

    /// `book` drawn in the title, authors and language this sets over its own.
    pub fn lay_over(&self, book: &mut BookStat) {
        if let Some(title) = &self.title {
            book.title.clone_from(title);
        }
        if let Some(authors) = &self.authors {
            book.author.clone_from(authors);
        }
        if let Some(language) = &self.language {
            book.language.clone_from(language);
        }
    }

    /// What this holds for `field`, as a row states it.
    fn value(&self, field: Field) -> Option<String> {
        match field {
            Field::Title => self.title.clone(),
            Field::Authors => self.authors.clone(),
            Field::Publisher => self.publisher.clone(),
            Field::Language => self.language.clone(),
            Field::Tags => (!self.tags.is_empty()).then(|| tags_value(&self.tags)),
        }
    }
}

/// `By`: a value Books narrows to and Rename… renames; `None`, a value not set.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum By {
    Author(Option<String>),
    Publisher(Option<String>),
    Language(Option<String>),
    /// `Type`: books holding any tag of this type.
    Type(String),
    /// `Tag`: books holding this tag of this type; `None`, those holding no tag of it.
    Tag(String, Option<String>),
}

impl By {
    /// Whether a book stating `values` is one this narrows to.
    pub fn holds(&self, values: &Values) -> bool {
        let typed = |t: &Tag, r#type: &str| folded(&t.r#type) == folded(r#type);
        match self {
            By::Author(None) => values.authors.is_empty(),
            By::Author(Some(name)) => values.authors.iter().any(|a| folded(a) == folded(name)),
            By::Publisher(want) => {
                values.publisher.as_deref().map(folded) == want.as_deref().map(folded)
            }
            By::Language(want) => values.language == *want,
            By::Type(r#type) => values.tags.iter().any(|t| typed(t, r#type)),
            By::Tag(r#type, None) => !values.tags.iter().any(|t| typed(t, r#type)),
            By::Tag(r#type, Some(tag)) => values
                .tags
                .iter()
                .any(|t| typed(t, r#type) && folded(&t.tag) == folded(tag)),
        }
    }
}

/// `Values`: a book's authors, publisher, language and tags, as Books narrows
/// and the charts count by them.
#[derive(Debug, Default, Clone, PartialEq, Eq, Serialize)]
pub struct Values {
    pub authors: Vec<String>,
    pub publisher: Option<String>,
    pub language: Option<String>,
    pub tags: Vec<Tag>,
}

impl Values {
    /// What `set` states over the own author line `author`.
    pub fn of(author: &str, set: &Set) -> Values {
        Values {
            authors: authors::split_display(set.authors.as_deref().unwrap_or(author)),
            publisher: set.publisher.clone(),
            language: set.language.clone(),
            tags: set.tags.clone(),
        }
    }
}

/// `Named`: a book's Kindle by its cut serial, empty for the backups naming
/// none, and the book's key and extent there.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Named {
    pub kindle: String,
    pub key: String,
    pub extent: i64,
}

impl Named {
    /// Whether `book` is the one this names: by key where this carries one,
    /// else by extent.
    fn names(&self, book: &Book) -> bool {
        match self.key.is_empty() {
            false => book.cde_key == self.key,
            true => book.cde_key.is_empty() && book.extent == self.extent,
        }
    }

    /// Whether `other` names the book this does, on the same Kindle.
    fn is(&self, other: &Named) -> bool {
        self.kindle == other.kindle
            && match self.key.is_empty() {
                false => self.key == other.key,
                true => other.key.is_empty() && self.extent == other.extent,
            }
    }
}

/// `Change`: a field's new value, or `Back` to the book's own.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Change {
    To(String),
    Back,
}

/// `Retag`: a tag a book holds, `from`, and the tag it becomes, `to`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Retag {
    pub from: Tag,
    pub to: Tag,
}

/// `Patch`: what one edit changes on one book or many; a field `None` stands
/// as it is.
#[derive(Debug, Default, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct Patch {
    pub title: Option<Change>,
    pub authors: Option<Change>,
    pub publisher: Option<Change>,
    pub language: Option<Change>,
    /// `tags`: each book's tags whole where given; `retag`, `remove` and `add` apply over them.
    pub tags: Option<Vec<Tag>>,
    /// `retag`: a tag held folding as a `from` becomes its `to`, in its place.
    pub retag: Vec<Retag>,
    pub add: Vec<Tag>,
    pub remove: Vec<Tag>,
}

impl Patch {
    /// Whether this sets each book's tags.
    fn retags(&self) -> bool {
        self.tags.is_some()
            || !self.retag.is_empty()
            || !self.add.is_empty()
            || !self.remove.is_empty()
    }

    /// Each field this changes on `book` (its rows setting `held`) and the value
    /// it takes; `None`, the book's own, for blank words or the title or
    /// authors its row states.
    fn values(&self, book: &Book, held: &Set, spelling: &Spelling) -> Vec<(Field, Option<String>)> {
        let typed = |change: &Option<Change>| {
            change.as_ref().map(|change| match change {
                Change::To(words) => words.clone(),
                Change::Back => String::new(),
            })
        };
        let mut out = Vec::new();
        if let Some(words) = typed(&self.title) {
            let title = clean(&words);
            out.push((
                Field::Title,
                (!title.is_empty() && title != book.title).then_some(title),
            ));
        }
        if let Some(words) = typed(&self.authors) {
            let names = authors::join_display(&authors::parse_input(&words));
            out.push((
                Field::Authors,
                (!names.is_empty() && names != book.author).then_some(names),
            ));
        }
        if let Some(words) = typed(&self.publisher) {
            let publisher = clean(&words);
            out.push((
                Field::Publisher,
                (!publisher.is_empty()).then_some(publisher),
            ));
        }
        if let Some(words) = typed(&self.language) {
            let language = lang::normalize(&words);
            out.push((Field::Language, (!language.is_empty()).then_some(language)));
        }
        if self.retags() {
            let mut tags: Vec<Tag> = Vec::new();
            for held in self.tags.as_ref().unwrap_or(&held.tags) {
                if self.remove.iter().any(|gone| same(held, gone)) {
                    continue;
                }
                // `to` with a blank field leaves `held` as it is.
                let tag = self
                    .retag
                    .iter()
                    .find(|r| same(&r.from, held))
                    .and_then(|r| spelling.spell(&r.to))
                    .or_else(|| spelling.spell(held));
                if let Some(tag) = tag
                    && !tags.iter().any(|t| same(t, &tag))
                {
                    tags.push(tag);
                }
            }
            for typed in &self.add {
                if let Some(tag) = spelling.spell(typed) {
                    tags.retain(|t| !same(t, &tag));
                    tags.push(tag);
                }
            }
            out.push((Field::Tags, (!tags.is_empty()).then(|| tags_value(&tags))));
        }
        out
    }
}

/// `Undo`: the rows one edit replaced in one Kindle's record as they stood, a
/// row stating no value for none; [`undo`] sets them back.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Undo {
    pub kindle: String,
    pub rows: Vec<Metadata>,
}

/// Set `patch` on each of `books` as of `at`, in the record of each one's
/// Kindle. Answers what [`undo`] takes back, one [`Undo`] for each record it
/// wrote.
pub fn edit(ws: &Workspace, books: &[Named], patch: &Patch, at: &str) -> Result<Vec<Undo>> {
    let each: Vec<(Named, Patch)> = books.iter().map(|b| (b.clone(), patch.clone())).collect();
    edit_each(ws, &each, at)
}

/// Rename `by`'s value to `to` on each of `books`, as of `at`: an author's
/// name among each book's, a publisher, a language, a tag, or a type on
/// every tag under it. Answers what [`undo`] takes back.
pub fn rename(ws: &Workspace, books: &[Named], by: &By, to: &str, at: &str) -> Result<Vec<Undo>> {
    let to = clean(to);
    if to.is_empty() {
        return Err(anyhow!("nothing to rename it to"));
    }
    if matches!(
        by,
        By::Author(None) | By::Publisher(None) | By::Language(None) | By::Tag(_, None)
    ) {
        return Err(anyhow!("a value not set has no name to change"));
    }
    let each: Vec<(Named, Patch)> = loaded(ws, books)?
        .into_iter()
        .filter(|(_, values)| by.holds(values))
        .map(|(named, values)| {
            let patch = match by {
                By::Author(Some(from)) => Patch {
                    authors: Some(Change::To(authors::join_display(
                        &values
                            .authors
                            .iter()
                            .flat_map(|name| match folded(name) == folded(from) {
                                true => authors::parse_input(&to),
                                false => vec![name.clone()],
                            })
                            .collect::<Vec<String>>(),
                    ))),
                    ..Patch::default()
                },
                By::Publisher(Some(_)) => Patch {
                    publisher: Some(Change::To(to.clone())),
                    ..Patch::default()
                },
                By::Language(Some(_)) => Patch {
                    language: Some(Change::To(to.clone())),
                    ..Patch::default()
                },
                By::Tag(r#type, Some(from)) => Patch {
                    retag: vec![Retag {
                        from: Tag {
                            r#type: r#type.clone(),
                            tag: from.clone(),
                        },
                        to: Tag {
                            r#type: r#type.clone(),
                            tag: to.clone(),
                        },
                    }],
                    ..Patch::default()
                },
                By::Type(from) => Patch {
                    retag: values
                        .tags
                        .iter()
                        .filter(|t| folded(&t.r#type) == folded(from))
                        .map(|t| Retag {
                            from: t.clone(),
                            to: Tag {
                                r#type: to.clone(),
                                tag: t.tag.clone(),
                            },
                        })
                        .collect(),
                    ..Patch::default()
                },
                _ => Patch::default(),
            };
            (named, patch)
        })
        .collect();
    edit_each(ws, &each, at)
}

/// Each of `books` with what it states, each Kindle's record read once.
fn loaded(ws: &Workspace, books: &[Named]) -> Result<Vec<(Named, Values)>> {
    let mut kindles: Vec<&str> = books.iter().map(|b| b.kindle.as_str()).collect();
    kindles.sort_unstable();
    kindles.dedup();
    let mut out = Vec::with_capacity(books.len());
    for kindle in kindles {
        let source = ws.source(kindle)?;
        let rows = read(&source.dir);
        for named in books.iter().filter(|b| b.kindle == kindle) {
            let book = source
                .record
                .books
                .iter()
                .find(|b| named.names(b))
                .ok_or_else(|| {
                    anyhow!(
                        "no book {} at {} in the record of {kindle}",
                        named.key,
                        named.extent
                    )
                })?;
            let set = Set::of(&rows, &named.key, named.extent);
            out.push((named.clone(), Values::of(&book.author, &set)));
        }
    }
    Ok(out)
}

/// Each book of `ws` holding a tag `spelling` spells unlike its row, with a patch
/// setting its tags as held; `each`'s books with a patch of their tags left out.
fn respelled(ws: &Workspace, spelling: &Spelling, each: &[(Named, Patch)]) -> Vec<(Named, Patch)> {
    let mut out: Vec<(Named, Patch)> = Vec::new();
    for source in ws.sources() {
        let kindle = source.serial.clone().unwrap_or_default();
        for row in read(&source.dir) {
            let Some(value) = row.value.filter(|_| row.field == Field::Tags) else {
                continue;
            };
            let tags = tags_of(&value);
            if tags.iter().all(|t| spelling.spell(t).as_ref() == Some(t)) {
                continue;
            }
            let book = Named {
                kindle: kindle.clone(),
                key: row.key,
                extent: row.extent,
            };
            let named = each.iter().any(|(b, patch)| b.is(&book) && patch.retags());
            if named || !source.record.books.iter().any(|b| book.names(b)) {
                continue;
            }
            out.push((
                book,
                Patch {
                    tags: Some(tags),
                    ..Patch::default()
                },
            ));
        }
    }
    out
}

/// Set each book's own patch as of `at`, in the record of each one's Kindle;
/// a [`Retag`] respelling a type or a tag respells it on every book of `ws`.
/// Answers what [`undo`] takes back, one [`Undo`] for each record it wrote.
pub fn edit_each(ws: &Workspace, each: &[(Named, Patch)], at: &str) -> Result<Vec<Undo>> {
    let mut spelling = Spelling::of(ws);
    let mut each = each.to_vec();
    if spelling.respell(each.iter().flat_map(|(_, patch)| &patch.retag)) {
        let more = respelled(ws, &spelling, &each);
        each.extend(more);
    }
    let mut kindles: Vec<&str> = each.iter().map(|(b, _)| b.kindle.as_str()).collect();
    kindles.sort_unstable();
    kindles.dedup();
    let mut out = Vec::new();
    for kindle in kindles {
        let source = ws.source(kindle)?;
        let held = read(&source.dir);
        let mut fresh = Vec::new();
        let mut replaced = Vec::new();
        for (named, patch) in each.iter().filter(|(b, _)| b.kindle == kindle) {
            let book = source
                .record
                .books
                .iter()
                .find(|b| named.names(b))
                .ok_or_else(|| {
                    anyhow!(
                        "no book {} at {} in the record of {kindle}",
                        named.key,
                        named.extent
                    )
                })?;
            let set = Set::of(&held, &named.key, named.extent);
            for (field, value) in patch.values(book, &set, &spelling) {
                if value == set.value(field) {
                    continue;
                }
                let before = held
                    .iter()
                    .find(|r| r.field == field && r.names(&named.key, named.extent))
                    .cloned();
                let was = match field {
                    Field::Title => Some(book.title.clone()),
                    Field::Authors => Some(book.author.clone()),
                    _ => None,
                }
                .filter(|_| value.is_some());
                fresh.push(Metadata {
                    edited: after(before.as_ref().map_or("", |r| r.edited.as_str()), at),
                    key: named.key.clone(),
                    extent: named.extent,
                    field,
                    value,
                    was,
                });
                replaced.push(before.unwrap_or_else(|| Metadata {
                    edited: String::new(),
                    key: named.key.clone(),
                    extent: named.extent,
                    field,
                    value: None,
                    was: None,
                }));
            }
        }
        if keep(&source.dir, &fresh)? {
            out.push(Undo {
                kindle: kindle.to_string(),
                rows: replaced,
            });
        }
    }
    Ok(out)
}

/// Set back each row `undone` holds, as of `at`, over any row standing since.
pub fn undo(ws: &Workspace, undone: &[Undo], at: &str) -> Result<()> {
    for each in undone {
        let source = ws.source(&each.kindle)?;
        let held = read(&source.dir);
        let rows: Vec<Metadata> = each
            .rows
            .iter()
            .map(|row| Metadata {
                edited: after(
                    held.iter()
                        .find(|h| h.id() == row.id())
                        .map_or("", |h| h.edited.as_str()),
                    at,
                ),
                ..row.clone()
            })
            .collect();
        keep(&source.dir, &rows)?;
    }
    Ok(())
}

/// `names` as one author line, joined as the editor joins typed ones.
pub fn joined(names: &[String]) -> String {
    authors::join_display(names)
}

/// `One`: one book's own patch, as `edit_metadata_each` takes it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct One {
    pub book: Named,
    pub patch: Patch,
}

/// `"ja"` where `title` or `author` holds kana, `"ko"` where either holds
/// Hangul; `None` for any other text, Han alone among it.
pub fn suggested_language(title: &str, author: &str) -> Option<&'static str> {
    match Script::resolve(Script::Unknown, &format!("{title} {author}")) {
        Script::Japanese => Some("ja"),
        Script::Korean => Some("ko"),
        _ => None,
    }
}

/// `author` with each name written `Surname, Given` turned to `Given Surname`,
/// as the editor turns a typed one; `None` where no name turns.
pub fn suggested_authors(author: &str) -> Option<String> {
    let turned = authors::join_display(&authors::parse_input(author));
    (!turned.is_empty() && turned != author).then_some(turned)
}

/// The `d` rows of the record's file in `dir`, none where it holds none.
pub fn read(dir: &Path) -> Vec<Metadata> {
    std::fs::read_to_string(Record::file(dir))
        .map(|text| rows(&text))
        .unwrap_or_default()
}

/// Every `d` row of `text`, a record's file, that [`read_row`] reads: one per
/// book and field, of two copies the later `edited`.
pub fn rows(text: &str) -> Vec<Metadata> {
    let read: Vec<Metadata> = text.lines().filter_map(read_row).collect();
    let mut out = Vec::with_capacity(read.len());
    take(&mut out, &read);
    out
}

/// Take `fresh` into `held`: each `row` `held` lacks is added, and of two
/// copies of one row the later `edited` stands. Answers whether anything moved.
pub fn take(held: &mut Vec<Metadata>, fresh: &[Metadata]) -> bool {
    let mut moved = false;
    for row in fresh {
        match held.iter_mut().find(|h| h.id() == row.id()) {
            Some(h) if row.edited > h.edited => {
                *h = row.clone();
                moved = true;
            }
            Some(_) => {}
            None => {
                held.push(row.clone());
                moved = true;
            }
        }
    }
    if moved {
        held.sort_by(|a, b| a.id().cmp(&b.id()));
    }
    moved
}

/// `text`, a record's file, with its `d` rows replaced by `rows` and its `w`
/// and `r` rows by `from`'s, closed again by `written::splice`; every other line,
/// a `d` row [`read_row`] cannot read among them, as `text` holds it.
pub fn splice(text: &str, from: &Record, rows: &[Metadata]) -> String {
    let mut body = String::with_capacity(text.len());
    for line in text.lines().filter(|line| read_row(line).is_none()) {
        body.push_str(line);
        body.push('\n');
    }
    for row in rows {
        write_row(&mut body, row);
    }
    written::splice(&body, from)
}

/// Take `fresh` into the `d` rows of the record's file in `dir`, under
/// [`writing`], writing the file only where a row moved; answers whether one did.
fn keep(dir: &Path, fresh: &[Metadata]) -> Result<bool> {
    let _held = writing();
    let file = Record::file(dir);
    let text =
        std::fs::read_to_string(&file).with_context(|| format!("reading {}", file.display()))?;
    let mut rows = rows(&text);
    if !take(&mut rows, fresh) {
        return Ok(false);
    }
    write_file(
        &file,
        splice(&text, &Record::from_text(&text), &rows).as_bytes(),
    )?;
    Ok(true)
}

/// `line` as a `d` row; `None` for any other line, and for one naming a field
/// [`Field::from_stored`] does not know.
fn read_row(line: &str) -> Option<Metadata> {
    let mut f = line.split('\t');
    if f.next()? != ROW {
        return None;
    }
    let mut next = || f.next().unwrap_or_default();
    let edited = next().trim().to_string();
    let key = unescaped(next());
    let extent = next().trim().parse().unwrap_or(0);
    let field = Field::from_stored(next())?;
    let value = read_optional(next());
    let was = read_optional(next());
    (!edited.is_empty()).then_some(Metadata {
        edited,
        key,
        extent,
        field,
        value,
        was,
    })
}

/// `row` as a line of a record's file.
fn write_row(out: &mut String, row: &Metadata) {
    let _ = writeln!(
        out,
        "{ROW}\t{}\t{}\t{}\t{}\t{}\t{}",
        row.edited,
        escaped(&row.key),
        row.extent,
        row.field.as_str(),
        optional(row.value.as_deref()),
        optional(row.was.as_deref()),
    );
}

/// Each [`Tag`] a `tags` value holds.
fn tags_of(value: &str) -> Vec<Tag> {
    value
        .lines()
        .filter_map(|line| {
            let (r#type, tag) = line.split_once('\t')?;
            (!r#type.is_empty() && !tag.is_empty()).then(|| Tag {
                r#type: r#type.into(),
                tag: tag.into(),
            })
        })
        .collect()
}

/// `tags` as a `tags` value: one a line, type and tag apart by a tab.
fn tags_value(tags: &[Tag]) -> String {
    tags.iter()
        .map(|t| format!("{}\t{}", t.r#type, t.tag))
        .collect::<Vec<String>>()
        .join("\n")
}

/// `Spelling`: the tags in use across a workspace, whose spellings a typed tag takes.
struct Spelling {
    tags: Vec<Tag>,
}

impl Spelling {
    /// The [`Spelling`] of every tag `ws`'s records hold.
    fn of(ws: &Workspace) -> Self {
        let tags = ws
            .record_dirs()
            .iter()
            .flat_map(|dir| read(dir))
            .filter(|row| row.field == Field::Tags)
            .filter_map(|row| row.value)
            .flat_map(|value| tags_of(&value))
            .collect();
        Self { tags }
    }

    /// `typed` as the tags in use spell it: its type as a type in use that
    /// folds alike, and its tag as one under that type; `None` where either
    /// field is blank.
    fn spell(&self, typed: &Tag) -> Option<Tag> {
        let (r#type, tag) = (clean(&typed.r#type), clean(&typed.tag));
        if r#type.is_empty() || tag.is_empty() {
            return None;
        }
        let r#type = self
            .tags
            .iter()
            .find(|t| folded(&t.r#type) == folded(&r#type))
            .map_or(r#type, |t| t.r#type.clone());
        let tag = self
            .tags
            .iter()
            .find(|t| t.r#type == r#type && folded(&t.tag) == folded(&tag))
            .map_or(tag, |t| t.tag.clone());
        Some(Tag { r#type, tag })
    }

    /// Take each of `retags` whose `to` respells its `from`'s type, or its tag under
    /// a type folding alike, as that type's or tag's spelling wherever it is in
    /// use. Answers whether a spelling in use moved.
    fn respell<'a>(&mut self, retags: impl Iterator<Item = &'a Retag>) -> bool {
        let mut moved = false;
        for retag in retags {
            let (from, to) = (&retag.from, &retag.to);
            let (r#type, tag) = (clean(&to.r#type), clean(&to.tag));
            if folded(&from.r#type) != folded(&r#type) || tag.is_empty() {
                continue;
            }
            let retyped = clean(&from.r#type) != r#type;
            let retagged = clean(&from.tag) != tag && folded(&from.tag) == folded(&tag);
            for held in &mut self.tags {
                if folded(&held.r#type) != folded(&r#type) {
                    continue;
                }
                if retyped && held.r#type != r#type {
                    held.r#type.clone_from(&r#type);
                    moved = true;
                }
                if retagged && folded(&held.tag) == folded(&tag) && held.tag != tag {
                    held.tag.clone_from(&tag);
                    moved = true;
                }
            }
        }
        moved
    }
}

/// Whether `a` and `b` are one tag spelled two ways.
fn same(a: &Tag, b: &Tag) -> bool {
    folded(&a.r#type) == folded(&b.r#type) && folded(&a.tag) == folded(&b.tag)
}

/// `words` trimmed, every run of white space inside them one space.
fn clean(words: &str) -> String {
    words.split_whitespace().collect::<Vec<&str>>().join(" ")
}

/// `words` as two spellings of one word compare: [`clean`], in lower case, a
/// full-width letter, digit or sign as its ASCII.
fn folded(words: &str) -> String {
    clean(words)
        .chars()
        .map(|c| match c {
            '\u{FF01}'..='\u{FF5E}' => char::from_u32(c as u32 - 0xFEE0).unwrap_or(c),
            c => c,
        })
        .flat_map(char::to_lowercase)
        .collect()
}

/// `at`, or a second past `held` where `at` is no later: each edit of a row
/// stamped after the one before it.
fn after(held: &str, at: &str) -> String {
    match at > held {
        true => at.to_string(),
        false => later(held),
    }
}

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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tests::{book, record_of, scratch, sitting};
    use crate::workspace::{KINDLES, RECORD};
    use readinglog_native::record::closed;

    const KINDLE: &str = "G003H2-0001";

    /// `name`'s workspace of one Kindle's record, `B00KEY` at extent 7 and a
    /// keyless book at 9; answers it and the record's folder.
    fn workspace(name: &str) -> (Workspace, std::path::PathBuf) {
        let dir = scratch(name);
        let ws = Workspace::create(&dir).unwrap();
        let mut keyed = book(7, "B00KEY", "A Book");
        keyed.author = "Doe, John".into();
        let mut keyless = book(9, "", "Notes To Self");
        keyless.author = "Roe, Jane".into();
        let record = record_of(
            vec![keyed, keyless],
            vec![
                sitting("2026-10-09T16:00:00", 7, 600),
                sitting("2026-10-10T16:00:00", 9, 600),
            ],
        );
        let at = dir.join(KINDLES).join(KINDLE).join(RECORD);
        record.save(&at).unwrap();
        (ws, at)
    }

    fn named(key: &str, extent: i64) -> Named {
        Named {
            kindle: KINDLE.into(),
            key: key.into(),
            extent,
        }
    }

    fn tag(r#type: &str, tag: &str) -> Tag {
        Tag {
            r#type: r#type.into(),
            tag: tag.into(),
        }
    }

    fn to(words: &str) -> Option<Change> {
        Some(Change::To(words.into()))
    }

    #[test]
    fn a_row_reads_back_as_it_was_written_and_every_other_line_stands() {
        let record = record_of(
            vec![book(7, "B00KEY", "A Book")],
            vec![sitting("2026-10-09T16:00:00", 7, 600)],
        );
        let text = record.text();
        let mut held = vec![
            Metadata {
                edited: "2026-10-31T10:00:00".into(),
                key: "B00\tKEY".into(),
                extent: 7,
                field: Field::Tags,
                value: Some(tags_value(&[
                    tag("genre", "sci-fi"),
                    tag("mood", "slow\\burn"),
                ])),
                was: None,
            },
            Metadata {
                edited: "2026-10-31T10:00:01".into(),
                key: String::new(),
                extent: 9,
                field: Field::Title,
                value: None,
                was: None,
            },
        ];
        // `stray` names a field `Field::from_stored` does not know.
        let stray = "d\t2026-10-31T10:00:02\tB00KEY\t7\tseries\tA Series\t\\N";
        let spliced = splice(&format!("{text}{stray}\n"), &record, &held);
        assert!(closed(&spliced));
        assert!(spliced.lines().any(|line| line == stray));
        held.sort_by(|a, b| a.id().cmp(&b.id()));
        assert_eq!(rows(&spliced), held);
        assert_eq!(
            Set::of(&held, "B00\tKEY", 7).tags,
            vec![tag("genre", "sci-fi"), tag("mood", "slow\\burn")]
        );
        assert_eq!(Record::from_text(&spliced).text(), text);
    }

    #[test]
    fn of_two_copies_the_later_edit_stands_and_a_taken_back_one_keeps_its_row() {
        let row = |edited: &str, value: Option<&str>| Metadata {
            edited: edited.into(),
            key: "B1".into(),
            extent: 1,
            field: Field::Title,
            value: value.map(Into::into),
            was: Some("Own".into()),
        };
        let mut held = vec![row("2026-10-31T10:00:00", Some("First"))];
        assert!(!take(
            &mut held,
            &[row("2026-10-30T10:00:00", Some("Older"))]
        ));
        assert!(take(&mut held, &[row("2026-10-31T11:00:00", None)]));
        assert_eq!(held, vec![row("2026-10-31T11:00:00", None)]);
        // `moved`: the key kept, the extent moved, one row.
        let moved = Metadata {
            extent: 2,
            ..row("2026-10-31T12:00:00", Some("Again"))
        };
        assert!(take(&mut held, std::slice::from_ref(&moved)));
        assert_eq!(held, vec![moved]);
    }

    #[test]
    fn each_field_is_set_on_its_book_and_undo_sets_it_back() {
        let (ws, at) = workspace("metadata-edit");
        let patch = Patch {
            title: to("A Better Title"),
            authors: to("Doe, John & Jane Roe"),
            publisher: to("  Example   Press "),
            language: to("en-US"),
            tags: Some(vec![tag("genre", "sci-fi")]),
            ..Patch::default()
        };
        let undone = edit(&ws, &[named("B00KEY", 7)], &patch, "2026-10-31T10:00:00").unwrap();
        let rows = read(&at);
        let set = Set::of(&rows, "B00KEY", 7);
        assert_eq!(set.title.as_deref(), Some("A Better Title"));
        assert_eq!(set.authors.as_deref(), Some("John Doe & Jane Roe"));
        assert_eq!(set.publisher.as_deref(), Some("Example Press"));
        assert_eq!(set.language.as_deref(), Some("en"));
        assert_eq!(set.tags, vec![tag("genre", "sci-fi")]);
        let title = rows.iter().find(|r| r.field == Field::Title).unwrap();
        assert_eq!(title.was.as_deref(), Some("A Book"));
        assert_eq!(Set::of(&rows, "", 9), Set::default(), "the other book");
        // `loaded`: the record's own rows as they were.
        let loaded = Record::load(&at);
        assert_eq!(loaded.books.len(), 2);
        assert!(loaded.books.iter().any(|b| b.title == "A Book"));
        // `patch` again: no row moves.
        assert!(
            edit(&ws, &[named("B00KEY", 7)], &patch, "2026-10-31T10:00:01")
                .unwrap()
                .is_empty()
        );
        // `A Book`, the book's own title, takes the edit back.
        edit(
            &ws,
            &[named("B00KEY", 7)],
            &Patch {
                title: to("A Book"),
                ..Patch::default()
            },
            "2026-10-31T10:00:02",
        )
        .unwrap();
        assert_eq!(Set::of(&read(&at), "B00KEY", 7).title, None);
        undo(&ws, &undone, "2026-10-31T10:00:03").unwrap();
        assert_eq!(Set::of(&read(&at), "B00KEY", 7), Set::default());
        assert_eq!(read(&at).len(), 5, "every row kept, each taken back");
        assert!(
            edit(&ws, &[named("NONE", 1)], &patch, "2026-10-31T10:00:04").is_err(),
            "a book the record does not hold"
        );
    }

    #[test]
    fn a_tag_takes_the_spelling_in_use_and_one_with_a_blank_field_is_none() {
        let (ws, at) = workspace("metadata-spelling");
        let add = |tags: Vec<Tag>| Patch {
            add: tags,
            ..Patch::default()
        };
        edit(
            &ws,
            &[named("B00KEY", 7)],
            &add(vec![tag("Genre", "Krimi")]),
            "2026-10-31T10:00:00",
        )
        .unwrap();
        edit(
            &ws,
            &[named("", 9)],
            &add(vec![tag(" ｇｅｎｒｅ ", "krimi"), tag("mood", " ")]),
            "2026-10-31T10:00:01",
        )
        .unwrap();
        assert_eq!(Set::of(&read(&at), "", 9).tags, vec![tag("Genre", "Krimi")]);
    }

    #[test]
    fn a_rename_reaches_each_book_holding_the_value_and_keeps_the_rest() {
        let (ws, at) = workspace("metadata-rename");
        let both = [named("B00KEY", 7), named("", 9)];
        let authors = |key: &str, extent: i64| Set::of(&read(&at), key, extent).authors;
        edit(
            &ws,
            &both[1..],
            &Patch {
                authors: to("Jane Roe & John Doe"),
                ..Patch::default()
            },
            "2026-10-31T10:00:00",
        )
        .unwrap();
        // `Doe, John`: book 7's own author; book 9 holds none of it.
        let by = By::Author(Some("Doe, John".into()));
        rename(&ws, &both, &by, "John Doe", "2026-10-31T10:00:01").unwrap();
        assert_eq!(authors("B00KEY", 7).as_deref(), Some("John Doe"));
        assert_eq!(authors("", 9).as_deref(), Some("Jane Roe & John Doe"));
        let by = By::Author(Some("john doe".into()));
        rename(&ws, &both, &by, "J. Doe", "2026-10-31T10:00:02").unwrap();
        assert_eq!(authors("B00KEY", 7).as_deref(), Some("J. Doe"));
        assert_eq!(authors("", 9).as_deref(), Some("Jane Roe & J. Doe"));
        // `scifi` renamed `sci-fi`; `genre` respelled `Genre`.
        edit(
            &ws,
            &both,
            &Patch {
                add: vec![tag("genre", "scifi"), tag("mood", "dark")],
                ..Patch::default()
            },
            "2026-10-31T10:00:03",
        )
        .unwrap();
        let by = By::Tag("genre".into(), Some("scifi".into()));
        rename(&ws, &both, &by, "sci-fi", "2026-10-31T10:00:04").unwrap();
        rename(
            &ws,
            &both,
            &By::Type("genre".into()),
            "Genre",
            "2026-10-31T10:00:05",
        )
        .unwrap();
        for (key, extent) in [("B00KEY", 7), ("", 9)] {
            assert_eq!(
                Set::of(&read(&at), key, extent).tags,
                vec![tag("Genre", "sci-fi"), tag("mood", "dark")]
            );
        }
        assert!(rename(&ws, &both, &By::Language(None), "en", "2026-10-31T10:00:06").is_err());
    }

    #[test]
    fn the_json_app_js_sends_reads_as_the_commands_take_it() {
        let patch: Patch = serde_json::from_str(
            r#"{"publisher":{"to":"Example Press"},"language":"back",
                "retag":[{"from":{"type":"genre","tag":"scifi"},"to":{"type":"genre","tag":"sci-fi"}}],
                "add":[{"type":"pace","tag":"slow"}],"remove":[{"type":"genre","tag":"sci-fi"}]}"#,
        )
        .unwrap();
        assert_eq!(
            patch,
            Patch {
                publisher: to("Example Press"),
                language: Some(Change::Back),
                retag: vec![Retag {
                    from: tag("genre", "scifi"),
                    to: tag("genre", "sci-fi"),
                }],
                add: vec![tag("pace", "slow")],
                remove: vec![tag("genre", "sci-fi")],
                ..Patch::default()
            }
        );
        let by: By = serde_json::from_str(r#"{"author":"John Doe"}"#).unwrap();
        assert_eq!(by, By::Author(Some("John Doe".into())));
        let by: By = serde_json::from_str(r#"{"tag":["genre",null]}"#).unwrap();
        assert_eq!(by, By::Tag("genre".into(), None));
        assert_eq!(
            serde_json::to_string(&By::Language(None)).unwrap(),
            r#"{"language":null}"#
        );
        let one: One = serde_json::from_str(
            r#"{"book":{"kindle":"G0K1-0001","key":"B1","extent":7},"patch":{"language":{"to":"ja"}}}"#,
        )
        .unwrap();
        assert_eq!(one.patch.language, to("ja"));
    }

    #[test]
    fn books_are_narrowed_by_what_they_state() {
        let values = Values {
            authors: vec!["John Doe".into(), "Jane Roe".into()],
            publisher: None,
            language: Some("ja".into()),
            tags: vec![tag("genre", "Sci-Fi")],
        };
        assert!(By::Author(Some("jane roe".into())).holds(&values));
        assert!(!By::Author(None).holds(&values));
        assert!(By::Publisher(None).holds(&values));
        assert!(By::Language(Some("ja".into())).holds(&values));
        assert!(!By::Language(None).holds(&values));
        assert!(By::Type("Genre".into()).holds(&values));
        assert!(By::Tag("genre".into(), Some("sci-fi".into())).holds(&values));
        assert!(By::Tag("mood".into(), None).holds(&values));
        assert!(!By::Tag("genre".into(), None).holds(&values));
    }

    #[test]
    fn a_language_is_offered_only_where_the_characters_settle_it() {
        assert_eq!(suggested_language("川辺の記録", "山田太郎"), Some("ja"));
        assert_eq!(suggested_language("Small Rivers", "김철수"), Some("ko"));
        assert_eq!(suggested_language("长夜闲谈", "李明"), None, "Han alone");
        assert_eq!(suggested_language("Der stille Hafen", "Klara Weiss"), None);
    }

    #[test]
    fn an_author_written_surname_first_is_offered_turned() {
        assert_eq!(suggested_authors("Doe, John").as_deref(), Some("John Doe"));
        assert_eq!(
            suggested_authors("Doe, John & Roe, Jane").as_deref(),
            Some("John Doe & Jane Roe")
        );
        assert_eq!(suggested_authors("John Doe"), None);
        assert_eq!(suggested_authors("Davis, Jr."), None);
        assert_eq!(suggested_authors(""), None);
    }

    #[test]
    fn tags_are_added_and_removed_over_each_books_own() {
        let (ws, at) = workspace("metadata-bulk");
        let both = [named("B00KEY", 7), named("", 9)];
        edit(
            &ws,
            &both[..1],
            &Patch {
                tags: Some(vec![tag("genre", "sci-fi"), tag("mood", "dark")]),
                ..Patch::default()
            },
            "2026-10-31T10:00:00",
        )
        .unwrap();
        edit(
            &ws,
            &both,
            &Patch {
                add: vec![tag("pace", "slow")],
                remove: vec![tag("MOOD", "Dark")],
                ..Patch::default()
            },
            "2026-10-31T10:00:01",
        )
        .unwrap();
        let rows = read(&at);
        assert_eq!(
            Set::of(&rows, "B00KEY", 7).tags,
            vec![tag("genre", "sci-fi"), tag("pace", "slow")]
        );
        assert_eq!(Set::of(&rows, "", 9).tags, vec![tag("pace", "slow")]);
    }

    #[test]
    fn a_tag_edited_in_place_changes_its_book_and_a_respelling_every_book() {
        let (ws, at) = workspace("metadata-retag");
        let both = [named("B00KEY", 7), named("", 9)];
        let tags = |key: &str, extent: i64| Set::of(&read(&at), key, extent).tags;
        let retag = |from: Tag, to: Tag| Patch {
            retag: vec![Retag { from, to }],
            ..Patch::default()
        };
        edit(
            &ws,
            &both,
            &Patch {
                add: vec![tag("genre", "sci-fi"), tag("mood", "dark")],
                ..Patch::default()
            },
            "2026-10-31T10:00:00",
        )
        .unwrap();
        // `science fiction`: another tag, on book 7 alone.
        let other = retag(tag("genre", "sci-fi"), tag("genre", "science fiction"));
        edit(&ws, &both[..1], &other, "2026-10-31T10:00:01").unwrap();
        assert_eq!(
            tags("B00KEY", 7),
            vec![tag("genre", "science fiction"), tag("mood", "dark")]
        );
        assert_eq!(
            tags("", 9),
            vec![tag("genre", "sci-fi"), tag("mood", "dark")]
        );
        // `Mood` and `Dark`: a type and a tag respelled on book 7 reach book 9 too.
        let respelled = retag(tag("mood", "dark"), tag("Mood", "Dark"));
        let undone = edit(&ws, &both[..1], &respelled, "2026-10-31T10:00:02").unwrap();
        assert_eq!(
            tags("B00KEY", 7),
            vec![tag("genre", "science fiction"), tag("Mood", "Dark")]
        );
        assert_eq!(
            tags("", 9),
            vec![tag("genre", "sci-fi"), tag("Mood", "Dark")]
        );
        assert_eq!(undone[0].rows.len(), 2, "both books' rows");
        // `MOOD` takes the spelling in use; a `to` with a blank field changes nothing.
        let patch = Patch {
            retag: vec![
                Retag {
                    from: tag("GENRE", "Sci-Fi"),
                    to: tag("MOOD", "calm"),
                },
                Retag {
                    from: tag("mood", "dark"),
                    to: tag("mood", " "),
                },
            ],
            ..Patch::default()
        };
        edit(&ws, &both[1..], &patch, "2026-10-31T10:00:03").unwrap();
        assert_eq!(tags("", 9), vec![tag("Mood", "calm"), tag("Mood", "Dark")]);
        undo(&ws, &undone, "2026-10-31T10:00:04").unwrap();
        assert_eq!(
            tags("B00KEY", 7),
            vec![tag("genre", "science fiction"), tag("mood", "dark")]
        );
        assert_eq!(
            tags("", 9),
            vec![tag("genre", "sci-fi"), tag("mood", "dark")]
        );
    }
}
