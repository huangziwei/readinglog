//! [`Metadata`]: a `d` row, one field of one book; [`rows`] reads them off a record's text,
//! [`Set::of`] is what one book's rows set and [`Set::lay_over`] draws the book in it;
//! [`links`], names counted as one value.

pub mod authors;
pub mod breakdown;
pub mod lang;
pub mod links;
pub mod names;

use std::fmt::Write as _;

use serde::{Deserialize, Serialize};

use crate::lang::Lang;
use crate::stats::BookStat;
pub use crate::written::{escaped, unescaped};
use crate::written::{optional, read_optional};

/// `ROW`: the first field of a `d` row.
const ROW: &str = "d";

/// `Field`: one field of a book's metadata; a `Language` row states a code, read
/// as a tag of [`names::TYPES`]' type.
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

    /// `from_stored`: the [`Field`] `word` names; `None` for any other word.
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
    /// `names`: whether `self` is of the book `key` and `extent` name, by `key`
    /// where it carries one, else by `extent`.
    pub fn names(&self, key: &str, extent: i64) -> bool {
        match self.key.is_empty() {
            false => self.key == key,
            true => key.is_empty() && self.extent == extent,
        }
    }

    /// `id`: what names `self` among a record's rows, its book and its field.
    pub fn id(&self) -> (&str, i64, Field) {
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
    /// `language`: the code a `language` row states; `tags` holds it as `stated`.
    pub language: Option<String>,
    pub tags: Vec<Tag>,
    /// `stated`: the tag `tags` holds for `language` and the `tags` row lacks.
    stated: Option<Tag>,
}

impl Set {
    /// `of`: what `rows` set on the book `key` and `extent` name, over whatever
    /// its own row states, then or since.
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
        if let Some(code) = &out.language
            && !names::codes(&out.tags).contains(&code.as_str())
        {
            let r#type = out
                .tags
                .iter()
                .find(|t| names::is_type(&t.r#type))
                .map_or_else(
                    || names::type_word(Lang::English).to_string(),
                    |t| t.r#type.clone(),
                );
            let tag = Tag {
                r#type,
                tag: names::name(code, Lang::English).unwrap_or(code).to_string(),
            };
            out.tags.push(tag.clone());
            out.stated = Some(tag);
        }
        out
    }

    /// `lay_over` draws `book` in the title and authors `self` sets over its own, and in
    /// the language the first of `tags` under a [`names::TYPES`] type names.
    pub fn lay_over(&self, book: &mut BookStat) {
        if let Some(title) = &self.title {
            book.title.clone_from(title);
        }
        if let Some(authors) = &self.authors {
            book.author.clone_from(authors);
        }
        if let Some(code) = names::codes(&self.tags).first() {
            book.language = (*code).to_string();
        }
    }

    /// `value`: what `self` holds for `field`, as a row states it.
    pub fn value(&self, field: Field) -> Option<String> {
        match field {
            Field::Title => self.title.clone(),
            Field::Authors => self.authors.clone(),
            Field::Publisher => self.publisher.clone(),
            Field::Language => self.language.clone(),
            Field::Tags => {
                let row: Vec<Tag> = self
                    .tags
                    .iter()
                    .filter(|t| Some(*t) != self.stated.as_ref())
                    .cloned()
                    .collect();
                (!row.is_empty()).then(|| tags_value(&row))
            }
        }
    }
}

/// `Values`: a book's authors, publisher and tags, its languages among them, as
/// Books narrows and the charts count by them.
#[derive(Debug, Default, Clone, PartialEq, Eq, Serialize)]
pub struct Values {
    pub authors: Vec<String>,
    pub publisher: Option<String>,
    pub tags: Vec<Tag>,
}

impl Values {
    /// `of`: what `set` states over the own author line `author`.
    pub fn of(author: &str, set: &Set) -> Values {
        Values {
            authors: authors::split_display(set.authors.as_deref().unwrap_or(author)),
            publisher: set.publisher.clone(),
            tags: set.tags.clone(),
        }
    }
}

/// `By`: a value Books narrows to; `None`, a value not set.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum By {
    Author(Option<String>),
    Publisher(Option<String>),
    /// `Tag`: books holding this tag of this type; `None`, those holding no tag of it.
    Tag(String, Option<String>),
}

impl By {
    /// `holds`: whether a book stating `values` is one `self` narrows to, a name
    /// standing for every name `links` counts as one value with it.
    pub fn holds(&self, values: &Values, links: &links::Links) -> bool {
        use links::Name;
        match self {
            By::Author(None) => values.authors.is_empty(),
            By::Author(Some(name)) => values
                .authors
                .iter()
                .any(|a| links.same(Name::Author(a), Name::Author(name))),
            By::Publisher(None) => values.publisher.is_none(),
            By::Publisher(Some(name)) => values
                .publisher
                .as_deref()
                .is_some_and(|p| links.same(Name::Publisher(p), Name::Publisher(name))),
            By::Tag(r#type, None) => !values
                .tags
                .iter()
                .any(|t| links.same(Name::Type(&t.r#type), Name::Type(r#type))),
            By::Tag(r#type, Some(tag)) => values
                .tags
                .iter()
                .any(|t| links.same(Name::Tag(&t.r#type, &t.tag), Name::Tag(r#type, tag))),
        }
    }
}

/// `narrows`: whether a book stating `values` is one every value of `by` narrows to,
/// two tags of one type both held.
pub fn narrows(by: &[By], values: &Values, links: &links::Links) -> bool {
    by.iter().all(|one| one.holds(values, links))
}

/// `rows`: every `d` row of `text`, a record's file, that [`read_row`] reads, one
/// per book and field, of two copies the later `edited`.
pub fn rows(text: &str) -> Vec<Metadata> {
    let read: Vec<Metadata> = text.lines().filter_map(read_row).collect();
    let mut out = Vec::with_capacity(read.len());
    take(&mut out, &read);
    out
}

/// `take` takes `fresh` into `held`, adding each `row` `held` lacks and keeping
/// the later `edited` of two copies of one row, and answers whether anything moved.
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

/// `line` as a `d` row; `None` for any other line, and for one naming a field
/// [`Field::from_stored`] does not know.
pub fn read_row(line: &str) -> Option<Metadata> {
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
pub fn write_row(out: &mut String, row: &Metadata) {
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

/// `tags_of`: each [`Tag`] `value` holds.
pub fn tags_of(value: &str) -> Vec<Tag> {
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
pub fn tags_value(tags: &[Tag]) -> String {
    tags.iter()
        .map(|t| format!("{}\t{}", t.r#type, t.tag))
        .collect::<Vec<String>>()
        .join("\n")
}

/// `same`: whether `a` and `b` are one tag spelled two ways.
pub fn same(a: &Tag, b: &Tag) -> bool {
    alike(&a.r#type, &b.r#type) && alike(&a.tag, &b.tag)
}

/// `alike`: whether `a` and `b` are one word spelled two ways, case and width aside.
pub fn alike(a: &str, b: &str) -> bool {
    folded(a) == folded(b)
}

/// `words` trimmed, every run of white space inside them one space.
pub fn clean(words: &str) -> String {
    words.split_whitespace().collect::<Vec<&str>>().join(" ")
}

/// `words` as two spellings of one word compare: [`clean`], in lower case, a
/// full-width letter, digit or sign as its ASCII.
pub fn folded(words: &str) -> String {
    clean(words)
        .chars()
        .map(|c| match c {
            '\u{FF01}'..='\u{FF5E}' => char::from_u32(c as u32 - 0xFEE0).unwrap_or(c),
            c => c,
        })
        .flat_map(char::to_lowercase)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::record::Record;

    /// `Record::text` writes back every readable `d` and `l` row `Record::from_text`
    /// read, `written::splice` keeps a `d` row naming a field no build here reads, and
    /// `take_written` keeps the later of two copies.
    #[test]
    fn a_kindle_keeps_the_desktops_rows_through_its_own_save() {
        let tags = Metadata {
            edited: "2026-11-14T10:00:00".into(),
            key: "B00\tKEY".into(),
            extent: 7,
            field: Field::Tags,
            value: Some(tags_value(&[
                tag("Genre", "Mystery"),
                tag("Language", "English"),
            ])),
            was: None,
        };
        let link = links::Link {
            edited: "2026-11-14T10:00:01".into(),
            linked: links::Linked::Tag,
            r#type: "Genre".into(),
            first: "Mystery".into(),
            names: Some(vec!["推理".into()]),
        };
        let stray = "d\t2026-11-14T10:00:02\tB00KEY\t7\tseries\tA Series\t\\N";
        let mut text = String::new();
        write_row(&mut text, &tags);
        links::write_row(&mut text, &link);
        text.push_str(stray);
        text.push('\n');

        let record = Record::from_text(&text);
        assert_eq!(record.metadata, vec![tags.clone()]);
        assert_eq!(record.links, vec![link.clone()]);
        let saved = Record::from_text(&record.text());
        assert_eq!(saved.metadata, record.metadata);
        assert_eq!(saved.links, record.links);

        let spliced = crate::written::splice(&text, &record);
        assert!(spliced.lines().any(|line| line == stray), "{spliced}");
        assert_eq!(rows(&spliced), vec![tags.clone()]);
        assert_eq!(links::rows(&spliced), vec![link]);

        let newer = Metadata {
            edited: "2026-11-14T11:00:00".into(),
            value: None,
            ..tags
        };
        let desk = Record {
            metadata: vec![newer.clone()],
            ..Record::default()
        };
        let mut kindle = record.clone();
        assert!(kindle.take_written(&desk));
        assert_eq!(kindle.metadata, vec![newer]);
        assert!(!kindle.take_written(&record), "an older copy moves nothing");
    }

    fn tag(r#type: &str, tag: &str) -> Tag {
        Tag {
            r#type: r#type.into(),
            tag: tag.into(),
        }
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
}
