//! [`link`] and [`separate`] write [`Link`]s, `l` rows, into every record of a workspace,
//! [`renamed`] and [`refiled`] carry them over a rename, and [`of`] reads a workspace's;
//! [`Link`] and [`Links`] are native's.

use std::path::Path;

use anyhow::{Context, Result, anyhow};
use serde::{Deserialize, Serialize};

use readinglog_native::record::Record;

pub use readinglog_native::metadata::links::{Link, Linked, Links, Name, rows, take};

use super::{Set, Undo, Values, after, clean, folded, names};
use crate::workspace::{Workspace, write_file, writing};

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

/// `link` counts `asked`'s names and every name linked to one of them as one value, in every record
/// of `ws` as of `at`: the link holding the most books first, the other links taken back. `link`
/// answers what [`super::undo`] takes back.
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

/// `renamed` sets `to` beside `from` in the link holding `from`, joined with the link `to` is in,
/// in every record of `ws` as of `at`; a tag's links, those under a type of `r#type`'s value.
/// `renamed` answers what [`super::undo`] takes back.
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::metadata::{By, Change, Named, Patch, edit, rename, undo};
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

    /// `one`: whether `ws`'s links count `a` and `b`, authors, as one.
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
}
