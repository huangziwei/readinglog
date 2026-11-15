//! [`Link`]: an `l` row, names of one field counted as one value wherever many books
//! meet, held in every record of a workspace; [`Links`]: the values of a record's books,
//! the value each name is in and the name each value is drawn by.

use std::collections::HashMap;
use std::fmt::Write as _;

use serde::{Deserialize, Serialize};

use super::{Values, escaped, folded, names, unescaped};

/// `ROW`: the first field of an `l` row.
const ROW: &str = "l";

/// `NONE`: a row's field for a value it holds none of.
const NONE: &str = "\\N";

/// `Linked`: the field a link's names are of.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Linked {
    Author,
    Publisher,
    Type,
    Tag,
}

impl Linked {
    /// `self` as a row stores it.
    fn as_str(self) -> &'static str {
        match self {
            Linked::Author => "author",
            Linked::Publisher => "publisher",
            Linked::Type => "type",
            Linked::Tag => "tag",
        }
    }

    /// `from_stored`: the [`Linked`] `word` names; `None` for any other word.
    fn from_stored(word: &str) -> Option<Linked> {
        match word.trim() {
            "author" => Some(Linked::Author),
            "publisher" => Some(Linked::Publisher),
            "type" => Some(Linked::Type),
            "tag" => Some(Linked::Tag),
            _ => None,
        }
    }
}

/// `Link`: an `l` row, names of one field counted as one value.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Link {
    /// `edited`: `YYYY-MM-DDTHH:MM:SS`; [`take`] keeps the later of two copies.
    pub edited: String,
    pub linked: Linked,
    /// `type`: the type a tag link was made under, empty for any other field.
    pub r#type: String,
    /// `first`: the name the link is filed under, the first in its order.
    pub first: String,
    /// `names`: its other names in order; `None` once taken back.
    pub names: Option<Vec<String>>,
}

impl Link {
    /// `id`: what names `self` among a record's rows: its field, its type and its
    /// first name, case and width aside.
    pub fn id(&self) -> (Linked, String, String) {
        (self.linked, folded(&self.r#type), folded(&self.first))
    }

    /// `all`: its names in order, `first` among them; none once taken back.
    pub fn all(&self) -> Vec<&str> {
        match &self.names {
            Some(names) => std::iter::once(self.first.as_str())
                .chain(names.iter().map(String::as_str))
                .collect(),
            None => Vec::new(),
        }
    }

    /// `holds`: whether `name` is one of [`Link::all`], case and width aside.
    pub fn holds(&self, name: &str) -> bool {
        let name = folded(name);
        self.all().iter().any(|n| folded(n) == name)
    }

    /// `back`: `self` taken back.
    pub fn back(&self) -> Link {
        Link {
            names: None,
            ..self.clone()
        }
    }
}

/// `rows`: every `l` row of `text`, a record's file, that [`read_row`] reads, one per
/// link, of two copies the later `edited`.
pub fn rows(text: &str) -> Vec<Link> {
    let read: Vec<Link> = text.lines().filter_map(read_row).collect();
    let mut out = Vec::with_capacity(read.len());
    take(&mut out, &read);
    out
}

/// `take` takes `fresh` into `held`, adding each link `held` lacks and keeping the
/// later `edited` of two copies of one, and answers whether anything moved.
pub fn take(held: &mut Vec<Link>, fresh: &[Link]) -> bool {
    let mut moved = false;
    for link in fresh {
        match held.iter_mut().find(|h| h.id() == link.id()) {
            Some(h) if link.edited > h.edited => {
                *h = link.clone();
                moved = true;
            }
            Some(_) => {}
            None => {
                held.push(link.clone());
                moved = true;
            }
        }
    }
    if moved {
        held.sort_by_key(Link::id);
    }
    moved
}

/// `line` as an `l` row; `None` for any other line, and for one naming a field
/// [`Linked::from_stored`] does not know.
pub fn read_row(line: &str) -> Option<Link> {
    let mut f = line.split('\t');
    if f.next()? != ROW {
        return None;
    }
    let mut next = || f.next().unwrap_or_default();
    let edited = next().trim().to_string();
    let linked = Linked::from_stored(next())?;
    let r#type = optional(next()).unwrap_or_default();
    let first = unescaped(next());
    let names = optional(next()).map(|value| {
        value
            .lines()
            .filter(|n| !n.is_empty())
            .map(str::to_string)
            .collect::<Vec<String>>()
    });
    (!edited.is_empty() && !first.is_empty()).then_some(Link {
        edited,
        linked,
        r#type,
        first,
        names: names.filter(|n| !n.is_empty()),
    })
}

/// `link` as a line of a record's file.
pub fn write_row(out: &mut String, link: &Link) {
    let field = |text: Option<&str>| text.map_or_else(|| NONE.to_string(), escaped);
    let names = link.names.as_ref().map(|names| names.join("\n"));
    let _ = writeln!(
        out,
        "{ROW}\t{}\t{}\t{}\t{}\t{}",
        link.edited,
        link.linked.as_str(),
        field((!link.r#type.is_empty()).then_some(link.r#type.as_str())),
        escaped(&link.first),
        field(names.as_deref()),
    );
}

/// `field` as [`write_row`] wrote it, `None` for [`NONE`].
fn optional(field: &str) -> Option<String> {
    (field != NONE).then(|| unescaped(field))
}

/// `Name`: a name of one field, a tag's with its type.
#[derive(Debug, Clone, Copy)]
pub enum Name<'a> {
    Author(&'a str),
    Publisher(&'a str),
    Type(&'a str),
    Tag(&'a str, &'a str),
}

impl<'a> Name<'a> {
    /// `words`: the name itself, a tag's without its type.
    pub fn words(self) -> &'a str {
        match self {
            Name::Author(n) | Name::Publisher(n) | Name::Type(n) | Name::Tag(_, n) => n,
        }
    }
}

/// `Value`: the names one value goes by, in order, each with the books holding it;
/// `linked` where an `l` row joins them.
#[derive(Debug, Clone, Default, PartialEq)]
struct Value {
    names: Vec<(String, usize)>,
    linked: bool,
}

impl Value {
    /// `shown`: the name the most books hold, the first in order on a tie.
    fn shown(&self) -> &str {
        let mut best: Option<&(String, usize)> = None;
        for named in &self.names {
            if best.is_none_or(|b| named.1 > b.1) {
                best = Some(named);
            }
        }
        best.map_or("", |(name, _)| name.as_str())
    }

    /// `others`: every name a book holds but `drawn`, case and width aside, in order.
    fn others(&self, drawn: &str) -> Vec<&str> {
        let drawn = folded(drawn);
        self.names
            .iter()
            .filter(|(name, books)| *books > 0 && folded(name) != drawn)
            .map(|(name, _)| name.as_str())
            .collect()
    }
}

/// `Table`: one field's values, the value each name is in by its folded words, and,
/// for a language type's tags, the value each code is.
#[derive(Debug, Clone, Default, PartialEq)]
struct Table {
    at: HashMap<String, usize>,
    values: Vec<Value>,
    codes: HashMap<&'static str, usize>,
}

impl Table {
    /// `place` puts `name` in the value `into`, or in one of its own where `into` is
    /// `None`, where no value holds it, and answers the value holding it.
    fn place(&mut self, name: &str, into: Option<usize>) -> usize {
        let key = folded(name);
        if let Some(&at) = self.at.get(&key) {
            return at;
        }
        let at = into.unwrap_or_else(|| {
            self.values.push(Value::default());
            self.values.len() - 1
        });
        self.values[at].names.push((name.to_string(), 0));
        self.at.insert(key, at);
        at
    }

    /// `place_language`: [`Table::place`] for a tag of a language type, every tag
    /// naming one code in one value.
    fn place_language(&mut self, name: &str) -> usize {
        match names::code_of(name) {
            Some(code) => {
                let into = self.codes.get(code).copied();
                let at = self.place(name, into);
                self.codes.insert(code, at);
                at
            }
            None => self.place(name, None),
        }
    }

    /// `join` puts the names of `link` that no value holds and `skip` passes in one value
    /// of their own.
    fn join(&mut self, link: &Link, skip: impl Fn(&str) -> bool) {
        let free: Vec<&str> = link
            .all()
            .into_iter()
            .filter(|n| !skip(n) && !self.at.contains_key(&folded(n)))
            .collect();
        if free.is_empty() {
            return;
        }
        self.values.push(Value::default());
        let at = self.values.len() - 1;
        for name in free {
            self.place(name, Some(at));
        }
        self.values[at].linked = self.values[at].names.len() > 1;
    }

    /// `count` counts one more book holding `name`.
    fn count(&mut self, name: &str) {
        let at = self.place(name, None);
        let value = &mut self.values[at];
        match value.names.iter_mut().find(|(n, _)| n == name) {
            Some((_, books)) => *books += 1,
            None => value.names.push((name.to_string(), 1)),
        }
    }

    /// `of`: the value holding `name`, `language` where it is a language type's tag.
    fn of(&self, name: &str, language: bool) -> Option<usize> {
        self.at
            .get(&folded(name))
            .copied()
            .or_else(|| match language {
                true => names::code_of(name).and_then(|code| self.codes.get(code).copied()),
                false => None,
            })
    }
}

/// `Links`: the values of a workspace's books, by field, a tag's within its type's
/// value.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Links {
    authors: Table,
    publishers: Table,
    types: Table,
    /// `tags`: each type value's tags, by the type value's place in `types`.
    tags: HashMap<usize, Table>,
    /// `language`: the language type's place in `types`, its five words in one value.
    language: Option<usize>,
}

impl Links {
    /// `new`: the values of `rows`, `l` rows, over `books`, each name counted once per book:
    /// the language type's words one type and its tags naming one code one language, each
    /// link's names one value (a name two hold, the later's), any other name alone.
    pub fn new(rows: &[Link], books: &[Values]) -> Links {
        let mut out = Links::default();
        let language = out.types.place(names::TYPES[0], None);
        for word in names::TYPES {
            out.types.place(word, Some(language));
        }
        out.language = Some(language);
        let mut standing: Vec<&Link> = rows.iter().filter(|l| l.names.is_some()).collect();
        standing.sort_by(|a, b| b.edited.cmp(&a.edited));
        for link in standing.iter().filter(|l| l.linked == Linked::Type) {
            out.types.join(link, names::is_type);
        }
        for link in &standing {
            match link.linked {
                Linked::Author => out.authors.join(link, |_| false),
                Linked::Publisher => out.publishers.join(link, |_| false),
                Linked::Type => {}
                Linked::Tag => {
                    let r#type = out.types.place(&link.r#type, None);
                    if Some(r#type) != out.language {
                        out.tags.entry(r#type).or_default().join(link, |_| false);
                    }
                }
            }
        }
        for values in books {
            let mut seen: Vec<String> = Vec::new();
            let mut first = |field: &str, name: &str| {
                let key = format!("{field}\t{}", folded(name));
                let fresh = !seen.contains(&key);
                if fresh {
                    seen.push(key);
                }
                fresh
            };
            for name in &values.authors {
                if first("a", name) {
                    out.authors.count(name);
                }
            }
            if let Some(publisher) = &values.publisher {
                out.publishers.count(publisher);
            }
            for tag in &values.tags {
                let at = out.types.place(&tag.r#type, None);
                if first("t", &tag.r#type) {
                    out.types.count(&tag.r#type);
                }
                let table = out.tags.entry(at).or_default();
                if Some(at) == out.language {
                    table.place_language(&tag.tag);
                }
                if first(&format!("g{at}"), &tag.tag) {
                    table.count(&tag.tag);
                }
            }
        }
        out
    }

    /// `table`: the table `name`'s field is counted in, and whether it is a language
    /// type's tags; `None` for a tag of a type no book holds and no link names.
    fn table(&self, name: Name) -> Option<(&Table, bool)> {
        match name {
            Name::Author(_) => Some((&self.authors, false)),
            Name::Publisher(_) => Some((&self.publishers, false)),
            Name::Type(_) => Some((&self.types, false)),
            Name::Tag(r#type, _) => {
                let at = self.types.of(r#type, false)?;
                Some((self.tags.get(&at)?, Some(at) == self.language))
            }
        }
    }

    /// `value`: the value holding `name`.
    fn value(&self, name: Name) -> Option<&Value> {
        let (table, language) = self.table(name)?;
        table.of(name.words(), language).map(|at| &table.values[at])
    }

    /// `shown`: the name `name`'s value is drawn by: the one the most books hold, the
    /// first in its order on a tie; `name`'s own words where no value holds it.
    pub fn shown<'a>(&'a self, name: Name<'a>) -> &'a str {
        self.value(name)
            .map(Value::shown)
            .filter(|shown| !shown.is_empty())
            .unwrap_or_else(|| name.words())
    }

    /// `also`: the other names of `name`'s value a book holds, its shown one left out.
    pub fn also<'a>(&'a self, name: Name<'a>) -> Vec<&'a str> {
        self.others(name, self.shown(name))
    }

    /// `others`: the names of `name`'s value a book holds, `drawn` left out.
    pub fn others<'a>(&'a self, name: Name<'a>, drawn: &str) -> Vec<&'a str> {
        self.value(name)
            .map(|value| value.others(drawn))
            .unwrap_or_default()
    }

    /// `books`: how many books hold `name` itself, case and width aside.
    pub fn books(&self, name: Name) -> usize {
        let words = folded(name.words());
        self.value(name).map_or(0, |value| {
            value
                .names
                .iter()
                .filter(|(n, _)| folded(n) == words)
                .map(|(_, books)| books)
                .sum()
        })
    }

    /// `every`: every name of `name`'s value, `name` alone where no value holds it.
    pub fn every<'a>(&'a self, name: Name<'a>) -> Vec<&'a str> {
        match self.value(name) {
            Some(value) => value.names.iter().map(|(n, _)| n.as_str()).collect(),
            None => vec![name.words()],
        }
    }

    /// `linked`: whether an `l` row joins `name`'s value.
    pub fn linked(&self, name: Name) -> bool {
        self.value(name).is_some_and(|value| value.linked)
    }

    /// `same`: whether `a` and `b` are names of one value; where no value holds one,
    /// whether they are one name, case and width aside.
    pub fn same(&self, a: Name, b: Name) -> bool {
        let at = |name: Name| {
            let (table, language) = self.table(name)?;
            table.of(name.words(), language)
        };
        let alike = match (at(a), at(b)) {
            (Some(x), Some(y)) => x == y,
            _ => folded(a.words()) == folded(b.words()),
        };
        match (a, b) {
            (Name::Author(_), Name::Author(_))
            | (Name::Publisher(_), Name::Publisher(_))
            | (Name::Type(_), Name::Type(_)) => alike,
            (Name::Tag(x, _), Name::Tag(y, _)) => self.same(Name::Type(x), Name::Type(y)) && alike,
            _ => false,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::metadata::Tag;

    fn row(edited: &str, linked: Linked, r#type: &str, names: &[&str]) -> Link {
        Link {
            edited: edited.into(),
            linked,
            r#type: r#type.into(),
            first: names[0].into(),
            names: Some(names[1..].iter().map(|n| n.to_string()).collect()),
        }
    }

    fn tag(r#type: &str, tag: &str) -> Tag {
        Tag {
            r#type: r#type.into(),
            tag: tag.into(),
        }
    }

    fn by(authors: &[&str], tags: &[Tag]) -> Values {
        Values {
            authors: authors.iter().map(|a| a.to_string()).collect(),
            publisher: None,
            tags: tags.to_vec(),
        }
    }

    #[test]
    fn a_row_reads_back_as_it_was_written() {
        let held = vec![
            row(
                "2026-11-14T10:00:00",
                Linked::Tag,
                "genre",
                &["mystery", "ミステリー", "推理小說"],
            ),
            Link {
                names: None,
                ..row(
                    "2026-11-14T10:00:01",
                    Linked::Author,
                    "",
                    &["Ann\tWriter", "x"],
                )
            },
        ];
        let mut text = String::new();
        for row in &held {
            write_row(&mut text, row);
        }
        assert!(
            text.lines().all(|line| line.split('\t').count() == 6),
            "{text}"
        );
        let mut sorted = held.clone();
        sorted.sort_by_key(Link::id);
        assert_eq!(rows(&text), sorted);
        assert_eq!(read_row("l\t2026-11-14T10:00:00\tseries\t\\N\tA\tB"), None);
    }

    #[test]
    fn a_link_counts_its_names_as_one_value_drawn_by_the_most_held() {
        let rows = [row(
            "2026-11-14T10:00:00",
            Linked::Author,
            "",
            &["Ann Writer", "アン・ライター"],
        )];
        let books = [
            by(&["アン・ライター", "春野一郎"], &[]),
            by(&["アン・ライター"], &[]),
            by(&["Ann Writer"], &[]),
            by(&["Bo Cowriter"], &[]),
        ];
        let links = Links::new(&rows, &books);
        assert_eq!(links.shown(Name::Author("Ann Writer")), "アン・ライター");
        assert_eq!(links.also(Name::Author("ann writer")), ["Ann Writer"]);
        assert!(links.same(Name::Author("Ann Writer"), Name::Author("アン・ライター")));
        assert!(!links.same(Name::Author("Ann Writer"), Name::Author("Bo Cowriter")));
        assert!(links.linked(Name::Author("Ann Writer")));
        assert!(!links.linked(Name::Author("Bo Cowriter")));
        assert_eq!(links.shown(Name::Author("Nobody Here")), "Nobody Here");
        assert!(links.same(Name::Author("Nobody Here"), Name::Author("nobody here")));
        assert!(!links.same(Name::Author("Ann Writer"), Name::Publisher("Ann Writer")));
    }

    #[test]
    fn a_tie_goes_to_the_link_s_order_and_a_name_no_book_holds_is_drawn_nowhere() {
        let rows = [row(
            "2026-11-14T10:00:00",
            Linked::Publisher,
            "",
            &["Minato Shobo", "みなと書房", "Harbour Books"],
        )];
        let books = [
            Values {
                publisher: Some("みなと書房".into()),
                ..Values::default()
            },
            Values {
                publisher: Some("Minato Shobo".into()),
                ..Values::default()
            },
        ];
        let links = Links::new(&rows, &books);
        assert_eq!(links.shown(Name::Publisher("みなと書房")), "Minato Shobo");
        assert_eq!(links.also(Name::Publisher("みなと書房")), ["みなと書房"]);
        assert_eq!(
            links.every(Name::Publisher("みなと書房")),
            ["Minato Shobo", "みなと書房", "Harbour Books"]
        );
    }

    #[test]
    fn a_name_two_links_hold_is_the_later_one_s() {
        let rows = [
            row("2026-11-14T10:00:00", Linked::Author, "", &["A", "B", "C"]),
            row("2026-11-14T11:00:00", Linked::Author, "", &["C", "D"]),
        ];
        let links = Links::new(&rows, &[]);
        assert!(links.same(Name::Author("C"), Name::Author("D")));
        assert!(!links.same(Name::Author("C"), Name::Author("A")));
        assert!(links.same(Name::Author("A"), Name::Author("B")));
    }

    #[test]
    fn linked_types_count_their_tags_as_one_and_a_tag_link_holds_across_them() {
        let rows = [
            row(
                "2026-11-14T10:00:00",
                Linked::Type,
                "",
                &["genre", "ジャンル"],
            ),
            row(
                "2026-11-14T10:00:01",
                Linked::Tag,
                "genre",
                &["mystery", "ミステリー"],
            ),
        ];
        let books = [
            by(&[], &[tag("genre", "mystery"), tag("genre", "SF")]),
            by(&[], &[tag("ジャンル", "ミステリー"), tag("ジャンル", "SF")]),
            by(&[], &[tag("ジャンル", "ミステリー")]),
        ];
        let links = Links::new(&rows, &books);
        assert_eq!(links.shown(Name::Type("genre")), "ジャンル");
        assert!(links.same(
            Name::Tag("genre", "mystery"),
            Name::Tag("ジャンル", "ミステリー")
        ));
        assert!(links.same(Name::Tag("genre", "SF"), Name::Tag("ジャンル", "sf")));
        assert_eq!(links.shown(Name::Tag("genre", "mystery")), "ミステリー");
        assert!(!links.same(Name::Tag("genre", "mystery"), Name::Tag("mood", "mystery")));
    }

    #[test]
    fn the_language_type_links_its_words_and_its_tags_by_code() {
        let rows = [row(
            "2026-11-14T10:00:00",
            Linked::Type,
            "",
            &["language", "lang"],
        )];
        let books = [
            by(&[], &[tag("language", "English")]),
            by(&[], &[tag("言語", "英語")]),
            by(&[], &[tag("language", "English"), tag("言語", "英語")]),
        ];
        let links = Links::new(&rows, &books);
        assert!(links.same(Name::Type("language"), Name::Type("言語")));
        assert!(!links.same(Name::Type("language"), Name::Type("lang")));
        assert!(links.same(Name::Tag("language", "English"), Name::Tag("言語", "英語")));
        assert!(links.same(
            Name::Tag("Sprache", "Englisch"),
            Name::Tag("language", "English")
        ));
        assert_eq!(links.shown(Name::Tag("言語", "英語")), "English");
        assert!(!links.linked(Name::Tag("language", "English")));
    }
}
