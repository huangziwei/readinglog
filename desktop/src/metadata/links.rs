//! [`Link`]: an `l` row, names of one field counted as one value wherever many books
//! meet, held in every record of a workspace; [`Links`]: the values of a workspace's
//! books, the value each name is in and the name each value is drawn by.

use std::collections::HashMap;
use std::fmt::Write as _;
use std::path::Path;

use anyhow::{Context, Result, anyhow};
use serde::{Deserialize, Serialize};

use readinglog_native::record::Record;

use super::{Set, Undo, Values, after, clean, escaped, folded, names, unescaped};
use crate::workspace::{Workspace, write_file, writing};

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
    fn id(&self) -> (Linked, String, String) {
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
    fn holds(&self, name: &str) -> bool {
        let name = folded(name);
        self.all().iter().any(|n| folded(n) == name)
    }

    /// `back`: `self` taken back.
    fn back(&self) -> Link {
        Link {
            names: None,
            ..self.clone()
        }
    }
}

/// `Linking`: names of one field, a tag's under its type, as `link_names` and
/// `unlink_names` take them.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Linking {
    pub linked: Linked,
    #[serde(default)]
    pub r#type: String,
    pub names: Vec<String>,
}

/// `read`: the `l` rows of the record's file in `dir`, none where it holds none.
pub fn read(dir: &Path) -> Vec<Link> {
    std::fs::read_to_string(Record::file(dir))
        .map(|text| rows(&text))
        .unwrap_or_default()
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

/// `of`: every link the records of `ws` hold, of two copies the later `edited`.
pub fn of(ws: &Workspace) -> Vec<Link> {
    let mut out = Vec::new();
    for source in ws.sources() {
        take(&mut out, &read(&source.dir));
    }
    out
}

/// `spread` takes every link of `ws` into each of its records, writing a record only
/// where a row moved.
pub fn spread(ws: &Workspace) -> Result<()> {
    let every = of(ws);
    for source in ws.sources() {
        keep(&source.dir, &every)?;
    }
    Ok(())
}

/// `keep` takes `fresh` into the `l` rows of the record's file in `dir`, under
/// [`writing`], writing the file only where a row moved, and answers whether one did.
pub(super) fn keep(dir: &Path, fresh: &[Link]) -> Result<bool> {
    let _held = writing();
    let file = Record::file(dir);
    let text =
        std::fs::read_to_string(&file).with_context(|| format!("reading {}", file.display()))?;
    let mut links = rows(&text);
    if !take(&mut links, fresh) {
        return Ok(false);
    }
    let spliced = super::splice(
        &text,
        &Record::from_text(&text),
        &super::rows(&text),
        &links,
    );
    write_file(&file, spliced.as_bytes())?;
    Ok(true)
}

/// `stamped`: each of `fresh` as of `at`, a second past any copy of it `held` holds.
fn stamped(held: &[Link], fresh: &[Link], at: &str) -> Vec<Link> {
    fresh
        .iter()
        .map(|link| Link {
            edited: after(
                held.iter()
                    .find(|h| h.id() == link.id())
                    .map_or("", |h| h.edited.as_str()),
                at,
            ),
            ..link.clone()
        })
        .collect()
}

/// `put_back` sets `undone`, links as they stood, back in the record's file in `dir`
/// as of `at`, over any copy standing since.
pub(super) fn put_back(dir: &Path, undone: &[Link], at: &str) -> Result<bool> {
    keep(dir, &stamped(&read(dir), undone, at))
}

/// `put` writes `fresh` into every record of `ws` as of `at`, each row a second past
/// any copy of it held, and answers what [`super::undo`] takes back: each record's
/// rows as they stood, a row taken back where it held none.
fn put(ws: &Workspace, fresh: &[Link], at: &str) -> Result<Vec<Undo>> {
    let mut out = Vec::new();
    for source in ws.sources() {
        let held = read(&source.dir);
        let replaced: Vec<Link> = fresh
            .iter()
            .map(|link| {
                held.iter()
                    .find(|h| h.id() == link.id())
                    .cloned()
                    .unwrap_or_else(|| Link {
                        edited: String::new(),
                        ..link.back()
                    })
            })
            .collect();
        if keep(&source.dir, &stamped(&held, fresh, at))? {
            out.push(Undo {
                kindle: source.serial.clone().unwrap_or_default(),
                rows: Vec::new(),
                links: replaced,
            });
        }
    }
    Ok(out)
}

/// `standing`: the links of `held` not taken back that are of `asked`'s field and, for a
/// tag, filed under a type of the value `asked`'s type is in, by `scope`'s types.
fn standing<'a>(held: &'a [Link], asked: &Linking, scope: &Links) -> Vec<&'a Link> {
    held.iter()
        .filter(|l| l.names.is_some() && l.linked == asked.linked)
        .filter(|l| {
            l.linked != Linked::Tag || scope.same(Name::Type(&l.r#type), Name::Type(&asked.r#type))
        })
        .collect()
}

/// `refused`: why `asked` cannot be linked, where it cannot: the language type and
/// its tags are linked by their own words and codes.
fn refused(asked: &Linking) -> Option<&'static str> {
    match asked.linked {
        Linked::Type if asked.names.iter().any(|n| names::is_type(n)) => {
            Some("the language type is linked by its own words")
        }
        Linked::Tag if names::is_type(&asked.r#type) => Some("a language is linked by its code"),
        _ => None,
    }
}

/// `link` counts `asked`'s names and every name linked to one of them as one value, in every
/// record of `ws` as of `at`: the link holding the most books first, the other links taken
/// back. Answers what [`super::undo`] takes back.
pub fn link(ws: &Workspace, asked: &Linking, at: &str) -> Result<Vec<Undo>> {
    if let Some(why) = refused(asked) {
        return Err(anyhow!(why));
    }
    let held = of(ws);
    let scope = Links::new(&held, &[]);
    let standing = standing(&held, asked, &scope);
    let mut groups: Vec<Vec<String>> = Vec::new();
    for name in asked
        .names
        .iter()
        .map(|n| clean(n))
        .filter(|n| !n.is_empty())
    {
        if groups.iter().flatten().any(|n| folded(n) == folded(&name)) {
            continue;
        }
        let group = standing
            .iter()
            .filter(|l| l.holds(&name))
            .max_by(|a, b| a.edited.cmp(&b.edited))
            .map_or_else(
                || vec![name.clone()],
                |l| l.all().into_iter().map(str::to_string).collect(),
            );
        groups.push(group);
    }
    let books = every(ws);
    groups.sort_by_key(|group| std::cmp::Reverse(held_by(&books, asked, group, &scope)));
    let mut all: Vec<String> = Vec::new();
    for name in groups.into_iter().flatten() {
        if !all.iter().any(|n| folded(n) == folded(&name)) {
            all.push(name);
        }
    }
    if all.len() < 2 {
        return Err(anyhow!("two names to count as one"));
    }
    let joined = Link {
        edited: String::new(),
        linked: asked.linked,
        r#type: match asked.linked {
            Linked::Tag => clean(&asked.r#type),
            _ => String::new(),
        },
        first: all[0].clone(),
        names: Some(all[1..].to_vec()),
    };
    let mut fresh: Vec<Link> = standing
        .iter()
        .filter(|l| l.id() != joined.id() && all.iter().any(|n| l.holds(n)))
        .map(|l| l.back())
        .collect();
    fresh.push(joined);
    if asked.linked == Linked::Type {
        fresh.extend(joined_tags(&held, &all));
    }
    put(ws, &fresh, at)
}

/// `joined_tags`: the tag links of `held` standing under one of `types` that share a
/// name, joined: each set of them one link, filed as the first of them is, the names
/// of each in their order, the others taken back.
fn joined_tags(held: &[Link], types: &[String]) -> Vec<Link> {
    let under = |l: &&Link| {
        l.names.is_some()
            && l.linked == Linked::Tag
            && types.iter().any(|t| folded(t) == folded(&l.r#type))
    };
    let mut sets: Vec<Vec<&Link>> = held.iter().filter(under).map(|l| vec![l]).collect();
    let shares = |a: &[&Link], b: &[&Link]| {
        a.iter()
            .any(|x| b.iter().any(|y| x.all().iter().any(|n| y.holds(n))))
    };
    while let Some((i, j)) = (0..sets.len())
        .flat_map(|i| ((i + 1)..sets.len()).map(move |j| (i, j)))
        .find(|(i, j)| shares(&sets[*i], &sets[*j]))
    {
        let moved = sets.remove(j);
        sets[i].extend(moved);
    }
    let mut out = Vec::new();
    for set in sets.into_iter().filter(|s| s.len() > 1) {
        let mut names: Vec<String> = Vec::new();
        for name in set.iter().flat_map(|l| l.all()) {
            if !names.iter().any(|n| folded(n) == folded(name)) {
                names.push(name.to_string());
            }
        }
        out.extend(set[1..].iter().map(|l| l.back()));
        out.push(Link {
            names: Some(names[1..].to_vec()),
            ..set[0].clone()
        });
    }
    out
}

/// `separate` takes back every link holding one of `asked`'s names, as of `at`, in
/// every record of `ws`, and answers what [`super::undo`] takes back.
pub fn separate(ws: &Workspace, asked: &Linking, at: &str) -> Result<Vec<Undo>> {
    let held = of(ws);
    let scope = Links::new(&held, &[]);
    let fresh: Vec<Link> = standing(&held, asked, &scope)
        .into_iter()
        .filter(|l| asked.names.iter().any(|n| l.holds(n)))
        .map(Link::back)
        .collect();
    match fresh.is_empty() {
        true => Ok(Vec::new()),
        false => put(ws, &fresh, at),
    }
}

/// `renamed` sets `to` beside `from` in the link holding `from`, joined with the link `to` is
/// in, in every record of `ws` as of `at`; a tag's links, those under a type of `r#type`'s
/// value. Answers what [`super::undo`] takes back.
pub(super) fn renamed(
    ws: &Workspace,
    linked: Linked,
    r#type: &str,
    from: &str,
    to: &str,
    at: &str,
) -> Result<Vec<Undo>> {
    let to = clean(to);
    if folded(from) == folded(&to) || to.is_empty() {
        return Ok(Vec::new());
    }
    let asked = Linking {
        linked,
        r#type: r#type.to_string(),
        names: Vec::new(),
    };
    let held = of(ws);
    let scope = Links::new(&held, &[]);
    let standing = standing(&held, &asked, &scope);
    let Some(holding) = standing
        .iter()
        .filter(|l| l.holds(from))
        .max_by(|a, b| a.edited.cmp(&b.edited))
    else {
        return Ok(Vec::new());
    };
    if holding.holds(&to) {
        return Ok(Vec::new());
    }
    let mut all: Vec<String> = Vec::new();
    for name in holding.all() {
        all.push(name.to_string());
        if folded(name) == folded(from) {
            all.push(to.clone());
        }
    }
    let joining = standing
        .iter()
        .filter(|l| l.id() != holding.id() && l.holds(&to))
        .max_by(|a, b| a.edited.cmp(&b.edited));
    let mut fresh: Vec<Link> = Vec::new();
    if let Some(joining) = joining {
        for name in joining.all() {
            if !all.iter().any(|n| folded(n) == folded(name)) {
                all.push(name.to_string());
            }
        }
        fresh.push(joining.back());
    }
    fresh.push(Link {
        names: Some(all[1..].to_vec()),
        ..(*holding).clone()
    });
    put(ws, &fresh, at)
}

/// `refiled` files each tag link made under the type `from` under `to`, as of `at`, in
/// every record of `ws`, and answers what [`super::undo`] takes back.
pub(super) fn refiled(ws: &Workspace, from: &str, to: &str, at: &str) -> Result<Vec<Undo>> {
    let to = clean(to);
    if folded(from) == folded(&to) || to.is_empty() {
        return Ok(Vec::new());
    }
    let mut fresh: Vec<Link> = Vec::new();
    for link in of(ws)
        .iter()
        .filter(|l| l.names.is_some() && l.linked == Linked::Tag)
        .filter(|l| folded(&l.r#type) == folded(from))
    {
        fresh.push(link.back());
        fresh.push(Link {
            r#type: to.clone(),
            ..link.clone()
        });
    }
    match fresh.is_empty() {
        true => Ok(Vec::new()),
        false => put(ws, &fresh, at),
    }
}

/// `every`: what each book of every record of `ws` states.
fn every(ws: &Workspace) -> Vec<Values> {
    let mut out = Vec::new();
    for source in ws.sources() {
        let rows = super::read(&source.dir);
        for book in &source.record.books {
            let set = Set::of(&rows, &book.cde_key, book.extent);
            out.push(Values::of(&book.author, &set));
        }
    }
    out
}

/// `held_by`: how many of `books` hold one of `names`, of `asked`'s field and, for a
/// tag, under a type of the value `asked`'s type is in, by `scope`'s types.
fn held_by(books: &[Values], asked: &Linking, names: &[String], scope: &Links) -> usize {
    let named = |words: &str| names.iter().any(|n| folded(n) == folded(words));
    books
        .iter()
        .filter(|values| match asked.linked {
            Linked::Author => values.authors.iter().any(|a| named(a)),
            Linked::Publisher => values.publisher.as_deref().is_some_and(named),
            Linked::Type => values.tags.iter().any(|t| named(&t.r#type)),
            Linked::Tag => values.tags.iter().any(|t| {
                named(&t.tag) && scope.same(Name::Type(&t.r#type), Name::Type(&asked.r#type))
            }),
        })
        .count()
}

/// `line` as an `l` row; `None` for any other line, and for one naming a field
/// [`Linked::from_stored`] does not know.
pub(super) fn read_row(line: &str) -> Option<Link> {
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
pub(super) fn write_row(out: &mut String, link: &Link) {
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
#[derive(Debug, Clone, Default)]
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
#[derive(Debug, Clone, Default)]
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
#[derive(Debug, Clone, Default)]
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
    use crate::metadata::{By, Change, Named, Patch, Tag, edit, rename, undo};
    use crate::tests::{book, record_of, scratch, sitting};
    use crate::workspace::{KINDLES, RECORD};
    use readinglog_native::record::closed;

    const KINDLES_HELD: [(&str, &str); 2] = [
        ("G003H2-0001", "Ann Writer"),
        ("G003H2-0002", "アン・ライター"),
    ];

    /// `name`'s workspace of two Kindles' records, the first's book by `Ann Writer`
    /// and the second's by `アン・ライター`, with each record's folder.
    fn two_kindles(name: &str) -> (Workspace, Vec<std::path::PathBuf>) {
        let dir = scratch(name);
        let ws = Workspace::create(&dir).unwrap();
        let mut dirs = Vec::new();
        for (kindle, author) in KINDLES_HELD {
            let mut one = book(7, "B00KEY", "A Book");
            one.author = author.into();
            let at = dir.join(KINDLES).join(kindle).join(RECORD);
            record_of(vec![one], vec![sitting("2026-10-09T16:00:00", 7, 600)])
                .save(&at)
                .unwrap();
            dirs.push(at);
        }
        (ws, dirs)
    }

    /// Whether `ws`'s links count `a` and `b`, authors, as one.
    fn one(ws: &Workspace, a: &str, b: &str) -> bool {
        Links::new(&of(ws), &every(ws)).same(Name::Author(a), Name::Author(b))
    }

    fn authors(names: &[&str]) -> Linking {
        Linking {
            linked: Linked::Author,
            r#type: String::new(),
            names: names.iter().map(|n| n.to_string()).collect(),
        }
    }

    #[test]
    fn a_link_is_written_into_every_record_undone_and_separated() {
        let (ws, dirs) = two_kindles("links-every-record");
        let undone = link(
            &ws,
            &authors(&["アン・ライター", "Ann Writer"]),
            "2026-11-14T10:00:00",
        )
        .unwrap();
        assert_eq!(undone.len(), 2, "one record each");
        for dir in &dirs {
            let held = read(dir);
            assert_eq!(held.len(), 1, "{}", dir.display());
            assert_eq!(held[0].all().len(), 2);
            assert!(closed(&std::fs::read_to_string(Record::file(dir)).unwrap()));
        }
        assert!(one(&ws, "Ann Writer", "アン・ライター"));

        undo(&ws, &undone, "2026-11-14T10:00:01").unwrap();
        assert!(!one(&ws, "Ann Writer", "アン・ライター"));

        link(
            &ws,
            &authors(&["Ann Writer", "アン・ライター"]),
            "2026-11-14T10:00:02",
        )
        .unwrap();
        assert!(one(&ws, "Ann Writer", "アン・ライター"));
        separate(&ws, &authors(&["アン・ライター"]), "2026-11-14T10:00:03").unwrap();
        assert!(!one(&ws, "Ann Writer", "アン・ライター"));
        assert!(
            of(&ws).iter().all(|l| l.names.is_none()),
            "a link taken back keeps its row"
        );
    }

    #[test]
    fn a_rename_and_an_edit_keep_the_link() {
        let (ws, dirs) = two_kindles("links-renamed");
        link(
            &ws,
            &authors(&["Ann Writer", "アン・ライター"]),
            "2026-11-14T10:00:00",
        )
        .unwrap();
        let first = Named {
            kindle: KINDLES_HELD[0].0.into(),
            key: "B00KEY".into(),
            extent: 7,
        };
        let by = By::Author(Some("Ann Writer".into()));
        rename(
            &ws,
            std::slice::from_ref(&first),
            &by,
            "Ann B. Writer",
            "2026-11-14T10:00:01",
        )
        .unwrap();
        assert!(one(&ws, "Ann B. Writer", "アン・ライター"));
        let retitled = Patch {
            title: Some(Change::To("Another Book".into())),
            ..Patch::default()
        };
        edit(&ws, &[first], &retitled, "2026-11-14T10:00:02").unwrap();
        assert!(
            !read(&dirs[0]).is_empty(),
            "a `d` row's write keeps the `l` rows"
        );
        assert!(one(&ws, "Ann B. Writer", "アン・ライター"));
    }

    #[test]
    fn linking_two_types_joins_their_tags_links_sharing_a_name() {
        let (ws, _) = two_kindles("links-types-join-tags");
        let tags = |r#type: &str, names: &[&str]| Linking {
            linked: Linked::Tag,
            r#type: r#type.into(),
            names: names.iter().map(|n| n.to_string()).collect(),
        };
        link(
            &ws,
            &tags("genre", &["mystery", "ミステリー"]),
            "2026-11-14T10:00:00",
        )
        .unwrap();
        link(
            &ws,
            &tags("ジャンル", &["ミステリー", "推理小說"]),
            "2026-11-14T10:00:01",
        )
        .unwrap();
        let types = Linking {
            linked: Linked::Type,
            r#type: String::new(),
            names: vec!["genre".into(), "ジャンル".into()],
        };
        link(&ws, &types, "2026-11-14T10:00:02").unwrap();
        let links = Links::new(&of(&ws), &[]);
        assert!(links.same(
            Name::Tag("genre", "mystery"),
            Name::Tag("ジャンル", "推理小說")
        ));
        let standing = of(&ws)
            .into_iter()
            .filter(|l| l.linked == Linked::Tag && l.names.is_some())
            .count();
        assert_eq!(standing, 1, "one tag link, the other taken back");
    }

    #[test]
    fn the_language_type_and_its_tags_take_no_link() {
        let (ws, _) = two_kindles("links-language");
        let types = Linking {
            linked: Linked::Type,
            r#type: String::new(),
            names: vec!["language".into(), "lang".into()],
        };
        assert!(link(&ws, &types, "2026-11-14T10:00:00").is_err());
        let tags = Linking {
            linked: Linked::Tag,
            r#type: "言語".into(),
            names: vec!["English".into(), "Englisch".into()],
        };
        assert!(link(&ws, &tags, "2026-11-14T10:00:00").is_err());
    }

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
