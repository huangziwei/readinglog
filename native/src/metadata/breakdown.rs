//! [`ranked`]: what a span's reading comes to by author, publisher and each type of tag;
//! [`languages`]: by language; the figures every Breakdown card draws.

use std::ops::RangeInclusive;

use super::links::{Links, Name};
use super::{Tag, Values, names};
use crate::stats::Stats;

/// `Bar`: one value's time and books; `name` is the name it is drawn by, `None` for the
/// books holding no value; `also`, its other names a book holds; `first`, the place in
/// `Stats::books` of the first book counted under it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Bar {
    pub name: Option<String>,
    pub secs: i64,
    pub books: usize,
    pub also: Vec<String>,
    pub first: usize,
}

/// `Ranks`: what a [`Ranked`] card counts: authors, publishers or the tags of one type,
/// by the name the type is drawn by.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Ranks {
    Authors,
    Publishers,
    Type(String),
}

/// `Ranked`: one card of bars. `bars` runs by time, the longest first and a tie by name;
/// `unset` holds the books holding no value of it; `doubled` is whether a book counts
/// under two values; `numbers`, whether every value is a whole number.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Ranked {
    pub ranks: Ranks,
    /// `also`: the type's other names a book holds.
    pub also: Vec<String>,
    pub bars: Vec<Bar>,
    pub unset: Option<Bar>,
    pub doubled: bool,
    pub numbers: bool,
}

/// `read`: each book read over `days` (the whole record where `None`) with its seconds
/// there, by its place in `stats.books`, and the values `values` gives it.
pub fn read(
    stats: &Stats,
    days: Option<&RangeInclusive<i64>>,
    whole: impl Fn(usize) -> i64,
    values: impl Fn(usize) -> Values,
) -> Vec<(usize, i64, Values)> {
    let mut over = vec![0i64; stats.books.len()];
    if let Some(days) = days {
        for sitting in &stats.sittings {
            if let Some(book) = sitting.book.filter(|_| days.contains(&sitting.day)) {
                over[book] += sitting.seconds;
            }
        }
    }
    (0..stats.books.len())
        .map(|at| {
            let secs = match days {
                Some(_) => over[at],
                None => whole(at),
            };
            (at, secs, values(at))
        })
        .filter(|(_, secs, _)| *secs > 0)
        .collect()
}

/// `tally` counts `secs` of the book at `at` on the bar of the value `name` is in, drawn
/// by the name `links` draws it by; `None`, on the bar of no value.
fn tally(links: &Links, bars: &mut Vec<Bar>, name: Option<Name>, at: usize, secs: i64) {
    let shown = name.map(|name| links.shown(name).to_string());
    match bars.iter_mut().find(|b| b.name == shown) {
        Some(bar) => {
            bar.secs += secs;
            bar.books += 1;
        }
        None => bars.push(Bar {
            name: shown,
            secs,
            books: 1,
            also: name
                .map(|name| links.also(name).into_iter().map(str::to_string).collect())
                .unwrap_or_default(),
            first: at,
        }),
    }
}

/// `card` sorts `bars` by time, the longest first and a tie by name, into a card.
fn card(
    ranks: Ranks,
    also: Vec<String>,
    mut bars: Vec<Bar>,
    unset: Vec<Bar>,
    doubled: bool,
) -> Ranked {
    bars.sort_by(|a, b| b.secs.cmp(&a.secs).then(a.name.cmp(&b.name)));
    let numbers = bars
        .iter()
        .all(|b| b.name.as_deref().is_some_and(|n| n.parse::<i64>().is_ok()));
    Ranked {
        ranks,
        also,
        bars,
        unset: unset.into_iter().next(),
        doubled,
        numbers,
    }
}

/// `ranked`: `read`'s cards, Authors where a book read names one, Publishers where one
/// states one, then each type in use by the name it is drawn by, the language type left
/// out; each value counted once per book, a book holding two of its names among them.
pub fn ranked(read: &[(usize, i64, Values)], links: &Links) -> Vec<Ranked> {
    let mut authors = Vec::new();
    let mut publishers = Vec::new();
    let (mut authors_unset, mut publishers_unset) = (Vec::new(), Vec::new());
    for (at, secs, values) in read {
        let mut once: Vec<&str> = Vec::new();
        for name in &values.authors {
            let shown = links.shown(Name::Author(name));
            if once.contains(&shown) {
                continue;
            }
            once.push(shown);
            tally(links, &mut authors, Some(Name::Author(name)), *at, *secs);
        }
        if values.authors.is_empty() {
            tally(links, &mut authors_unset, None, *at, *secs);
        }
        match &values.publisher {
            Some(p) => tally(links, &mut publishers, Some(Name::Publisher(p)), *at, *secs),
            None => tally(links, &mut publishers_unset, None, *at, *secs),
        }
    }
    let mut out = Vec::new();
    if !authors.is_empty() {
        out.push(card(
            Ranks::Authors,
            Vec::new(),
            authors,
            authors_unset,
            false,
        ));
    }
    if !publishers.is_empty() {
        out.push(card(
            Ranks::Publishers,
            Vec::new(),
            publishers,
            publishers_unset,
            false,
        ));
    }
    let mut types: Vec<&str> = Vec::new();
    for (_, _, values) in read {
        for tag in &values.tags {
            let shown = links.shown(Name::Type(&tag.r#type));
            if !types.contains(&shown) {
                types.push(shown);
            }
        }
    }
    for r#type in types.into_iter().filter(|t| !names::is_type(t)) {
        let mut bars = Vec::new();
        let mut unset = Vec::new();
        let mut doubled = false;
        for (at, secs, values) in read {
            let mut under: Vec<(&Tag, &str)> = Vec::new();
            for tag in values
                .tags
                .iter()
                .filter(|t| links.same(Name::Type(&t.r#type), Name::Type(r#type)))
            {
                let shown = links.shown(Name::Tag(&tag.r#type, &tag.tag));
                if !under.iter().any(|(_, s)| *s == shown) {
                    under.push((tag, shown));
                }
            }
            doubled |= under.len() > 1;
            for (tag, _) in &under {
                let name = Name::Tag(&tag.r#type, &tag.tag);
                tally(links, &mut bars, Some(name), *at, *secs);
            }
            if under.is_empty() {
                tally(links, &mut unset, None, *at, *secs);
            }
        }
        let also = links
            .also(Name::Type(r#type))
            .into_iter()
            .map(str::to_string)
            .collect();
        out.push(card(
            Ranks::Type(r#type.to_string()),
            also,
            bars,
            unset,
            doubled,
        ));
    }
    out
}

/// `spoken`: the languages `tags` holds, each tag under a type of [`names::TYPES`], once
/// each, as the type and the tag are drawn where many books meet.
pub fn spoken(links: &Links, tags: &[Tag]) -> Vec<Tag> {
    let mut out: Vec<Tag> = Vec::new();
    for tag in tags.iter().filter(|t| names::is_type(&t.r#type)) {
        let shown = Tag {
            r#type: links.shown(Name::Type(&tag.r#type)).to_string(),
            tag: links.shown(Name::Tag(&tag.r#type, &tag.tag)).to_string(),
        };
        if !out.iter().any(|t| t.tag == shown.tag) {
            out.push(shown);
        }
    }
    out
}

/// `part`: the `n`th of `of` even parts of `secs`, the first `secs % of` of them a second
/// longer, the parts summing to `secs`.
pub fn part(secs: i64, n: usize, of: usize) -> i64 {
    let of = of.max(1) as i64;
    secs / of + i64::from((n as i64) < secs % of)
}

/// `shares`: `parts` as whole percents summing to 100, by the largest remainder.
pub fn shares(parts: &[i64]) -> Vec<i64> {
    let total: i64 = parts.iter().sum();
    if total == 0 {
        return vec![0; parts.len()];
    }
    let mut out: Vec<(usize, i64, i64)> = parts
        .iter()
        .enumerate()
        .map(|(n, p)| (n, p * 100 / total, p * 100 % total))
        .collect();
    let short = 100 - out.iter().map(|(_, floor, _)| floor).sum::<i64>();
    let mut order: Vec<usize> = (0..out.len()).collect();
    order.sort_by(|a, b| out[*b].2.cmp(&out[*a].2).then(a.cmp(b)));
    for n in order.into_iter().take(short.max(0) as usize) {
        out[n].1 += 1;
    }
    out.into_iter().map(|(_, share, _)| share).collect()
}

/// `Languages`: the language card.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Languages {
    /// `r#type`: the language type by the word it is drawn by; `also`, its other words.
    pub r#type: String,
    pub also: Vec<String>,
    /// `parts`: each language's tag and seconds, the longest first, `None` (no language) last.
    pub parts: Vec<(Option<Tag>, i64)>,
    /// `shares`: each part's whole percent.
    pub shares: Vec<i64>,
    /// `months`: each month's seconds by part, in `parts`' order.
    pub months: Vec<Vec<i64>>,
    /// `split`: whether a book's time is split among several languages.
    pub split: bool,
}

/// `languages`: `read`'s language card, a book's time split evenly among its languages,
/// and `months`' seconds by language off `stats.sittings`; `None` where no book of
/// `every`, each book's values, holds a language or no book read does.
pub fn languages(
    stats: &Stats,
    read: &[(usize, i64, Values)],
    every: &[Values],
    links: &Links,
    months: &[RangeInclusive<i64>],
) -> Option<Languages> {
    let tag = every
        .iter()
        .flat_map(|values| &values.tags)
        .find(|t| names::is_type(&t.r#type))?;
    let r#type = links.shown(Name::Type(&tag.r#type)).to_string();
    let books: Vec<(usize, Vec<Tag>)> = read
        .iter()
        .map(|(at, _, values)| (*at, spoken(links, &values.tags)))
        .collect();
    if books.iter().all(|(_, these)| these.is_empty()) {
        return None;
    }
    let mut parts: Vec<(Option<Tag>, i64)> = Vec::new();
    let mut split = false;
    for ((_, secs, _), (_, these)) in read.iter().zip(&books) {
        split |= these.len() > 1;
        let each: Vec<(Option<&Tag>, i64)> = match these.is_empty() {
            true => vec![(None, *secs)],
            false => these
                .iter()
                .enumerate()
                .map(|(n, tag)| (Some(tag), part(*secs, n, these.len())))
                .collect(),
        };
        for (tag, secs) in each {
            let words = tag.map(|t| t.tag.as_str());
            match parts
                .iter_mut()
                .find(|(t, _)| t.as_ref().map(|t| t.tag.as_str()) == words)
            {
                Some((_, s)) => *s += secs,
                None => parts.push((tag.cloned(), secs)),
            }
        }
    }
    parts.sort_by(|a, b| (a.0.is_none(), b.1).cmp(&(b.0.is_none(), a.1)));
    let shares = shares(&parts.iter().map(|(_, s)| *s).collect::<Vec<i64>>());
    let mut by_book: Vec<Vec<i64>> = vec![Vec::new(); stats.books.len()];
    for sitting in &stats.sittings {
        let Some(book) = sitting.book else { continue };
        let Some(month) = months.iter().position(|span| span.contains(&sitting.day)) else {
            continue;
        };
        let held = &mut by_book[book];
        if held.is_empty() {
            *held = vec![0; months.len()];
        }
        held[month] += sitting.seconds;
    }
    let months = (0..months.len())
        .map(|month| {
            parts
                .iter()
                .map(|(lang, _)| {
                    books
                        .iter()
                        .map(|(at, these)| {
                            let secs = by_book[*at].get(month).copied().unwrap_or(0);
                            match lang {
                                None if these.is_empty() => secs,
                                None => 0,
                                Some(lang) => these
                                    .iter()
                                    .position(|t| t.tag == lang.tag)
                                    .map_or(0, |n| part(secs, n, these.len())),
                            }
                        })
                        .sum()
                })
                .collect()
        })
        .collect();
    let also = links
        .also(Name::Type(&r#type))
        .into_iter()
        .map(str::to_string)
        .collect();
    Some(Languages {
        r#type,
        also,
        parts,
        shares,
        months,
        split,
    })
}

/// `language_order`: the languages of `every`, each book's values, by the time
/// `whole(at)` gives the book at `at` read across the record, each book's split as
/// [`languages`] splits it, the most read first and a tie by name.
pub fn language_order(
    every: &[Values],
    links: &Links,
    whole: impl Fn(usize) -> i64,
) -> Vec<String> {
    let mut order: Vec<(String, i64)> = Vec::new();
    for (at, values) in every.iter().enumerate() {
        let these = spoken(links, &values.tags);
        let secs = whole(at);
        for (n, tag) in these.iter().enumerate() {
            let secs = part(secs, n, these.len());
            match order.iter_mut().find(|(words, _)| *words == tag.tag) {
                Some((_, s)) => *s += secs,
                None => order.push((tag.tag.clone(), secs)),
            }
        }
    }
    order.sort_by(|a, b| b.1.cmp(&a.1).then(a.0.cmp(&b.0)));
    order.into_iter().map(|(words, _)| words).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn shares_sum_to_a_hundred_by_the_largest_remainder() {
        assert_eq!(shares(&[232, 156, 65, 42, 61]), vec![42, 28, 12, 7, 11]);
        assert_eq!(shares(&[1, 1, 1]), vec![34, 33, 33]);
        assert_eq!(shares(&[0, 0]), vec![0, 0]);
    }
}
