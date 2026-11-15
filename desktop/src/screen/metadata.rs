//! [`head`], [`listed`], [`chip`], [`renaming`], [`chosen`], [`sheet`], [`suggestions`] and
//! [`charts`]: a book's metadata on its head, the tags over Books, Books narrowed and picked,
//! and the charts.

use std::collections::BTreeMap;
use std::fmt::Write as _;
use std::ops::RangeInclusive;

use readinglog_native::date;
use readinglog_native::lang::counted;
use readinglog_native::view::{Shelf, Sort, Way, Window};
use serde::Serialize;

use super::words::Words;
use super::{Ctx, Route, esc, lang, language_of};
use crate::metadata::links::{Linked, Linking, Name};
use crate::metadata::{self, By, Named, Patch, Tag, Values, names};
use readinglog_native::metadata::breakdown::{self, Ranks};

/// `HUES`: each language's hue, by the record's languages in time read; `OTHER` past the fourth.
const HUES: [&str; 4] = ["#0f86a3", "#b8781e", "#7d4fb0", "#c4456e"];
const OTHER: &str = "#9a958b";

/// `UNSET`, `BAR`, `MOST`: the bar of a value no book sets, a ranked bar, the most read one.
const UNSET: &str = "#ccc8bf";
const BAR: &str = "#79a260";
const MOST: &str = "#d23f6b";

/// `TOP`: the bars a ranked chart shows until Show all opens the rest.
const TOP: usize = 10;

/// `values`: what the book at `at` states.
pub(super) fn values(cx: &Ctx, at: usize) -> Values {
    Values::of(&cx.own[at].1, &cx.metadata[at])
}

/// `named`: the [`Named`] of the book at `at`, as the commands take it.
pub(super) fn named(cx: &Ctx, at: usize) -> Named {
    let book = &cx.stats.books[at];
    Named {
        kindle: cx.serial(at).to_string(),
        key: crate::kindles::key_origin(&book.cde_key).1.to_string(),
        extent: crate::kindles::origin(book.extent).1,
    }
}

/// `narrowed`: [`Route::Books`] narrowed to `by`, under `window`.
pub(super) fn narrowed(by: By, window: Option<Window>) -> Route {
    Route::Books {
        shelf: Shelf::All,
        order: Sort::Recent,
        way: Way::Down,
        query: String::new(),
        window,
        by: vec![by],
        editing: false,
    }
}

/// `code`'s name in `cx.lang`; `code` itself where [`names`] lists none.
fn language_name(cx: &Ctx, code: &str) -> String {
    names::name(code, cx.lang).unwrap_or(code).to_string()
}

/// `value` as JSON inside an attribute.
fn json(value: &impl Serialize) -> String {
    esc(&serde_json::to_string(value).unwrap_or_default())
}

/// `others`, a value's other names, as its tooltip says them.
fn said_also(cx: &Ctx, others: &[&str]) -> String {
    Words::of(cx.lang)
        .also
        .replace("{names}", &others.join(" / "))
}

/// `tip_of`: a ` data-tip` saying `others`, a value's other names; nothing where it has none.
fn tip_of(cx: &Ctx, others: &[&str]) -> String {
    match others.is_empty() {
        true => String::new(),
        false => format!(" data-tip=\"{}\"", esc(&said_also(cx, others))),
    }
}

/// `line`, an author line, as the book holds it, each of `names` in it opening Books
/// narrowed to it, the other names of its value in its tooltip; joined by `、` where
/// `line` holds one, else by ` & `, where `line` holds the joined names and no more.
fn author_line(cx: &Ctx, line: &str, names: &[String]) -> String {
    let link = |name: &str| {
        format!(
            "<a href=\"{}\"{}>{}</a>",
            narrowed(By::Author(Some(name.to_string())), None).link(),
            tip_of(cx, &cx.linked.others(Name::Author(name), name)),
            esc(name)
        )
    };
    let joint = match line.contains('、') {
        true => "、",
        false => " & ",
    };
    let joined = || {
        names
            .iter()
            .map(|n| link(n))
            .collect::<Vec<String>>()
            .join(joint)
    };
    if names.join(joint) == line {
        return joined();
    }
    let mut out = String::new();
    let mut rest = line;
    for name in names {
        let Some(at) = rest.find(name.as_str()) else {
            return joined();
        };
        out.push_str(&esc(&rest[..at]));
        out.push_str(&link(name));
        rest = &rest[at + name.len()..];
    }
    out.push_str(&esc(rest));
    out
}

/// `at`'s head under its title: its authors (each opening the books by them),
/// the facts line with Edit, [`types`], and the editor Edit opens, hidden
/// until `app.js` shows it.
pub(super) fn head(cx: &Ctx, at: usize) -> String {
    let w = Words::of(cx.lang);
    let book = &cx.stats.books[at];
    let values = values(cx, at);
    let named = named(cx, at);
    let mut out = format!("<div class=\"meta\" data-book=\"{}\">", json(&named));
    if !values.authors.is_empty() {
        let _ = write!(
            out,
            "<p class=\"author\"{}>{}</p>",
            lang(&book.language, &book.author),
            author_line(cx, &book.author, &values.authors)
        );
    }
    let publisher = values
        .publisher
        .as_ref()
        .map(|publisher| {
            format!(
                "<a href=\"{}\"{}>{}</a>",
                narrowed(By::Publisher(Some(publisher.clone())), None).link(),
                tip_of(cx, &cx.linked.others(Name::Publisher(publisher), publisher)),
                esc(publisher)
            )
        })
        .unwrap_or_default();
    let _ = write!(
        out,
        "<p class=\"facts\">{publisher}<button class=\"edit\" data-act=\"edit-meta\">{}</button></p>",
        esc(w.edit)
    );
    out.push_str(&types(cx, &values));
    out.push_str(&editor(cx, at, &values));
    out.push_str(&picks(cx));
    out.push_str("</div>");
    out
}

/// `words`' width in a field, in `em`, reckoned by each letter's breadth as
/// `app.js`'s `reckoned` reckons it.
fn width(words: &str) -> f64 {
    words
        .chars()
        .map(|c| match c {
            c if c as u32 >= 0x2E80 => 1.0,
            'i' | 'j' | 'l' | 'I' | '.' | ',' | ':' | ';' | '\'' | '!' | '|' => 0.26,
            'f' | 'r' | 't' | ' ' | '-' | '(' | ')' | '/' => 0.36,
            'm' | 'w' | 'M' | 'W' => 0.85,
            'A'..='Z' => 0.68,
            _ => 0.57,
        })
        .sum()
}

/// `words`' [`width`] as a field's `width`, a caret's room past them.
fn fit(words: &str) -> String {
    format!("calc({:.2}em + 2px)", width(words))
}

/// `field`: a field `which` holding `words`, as wide as they are, named in
/// `app.js` by `key`, `more` its other attributes.
fn field(which: &str, words: &str, key: &[&str], more: &str) -> String {
    format!(
        "<input data-field=\"{which}\" value=\"{0}\" data-was=\"{0}\" data-key=\"{1}\" \
         data-fit style=\"width: {2}\"{more} autocomplete=\"off\" spellcheck=\"false\">",
        esc(words),
        json(&key),
        fit(words)
    )
}

/// `named_type`: the `.name` field of `r#type`, renaming it on every book, `data-shown`
/// naming the type `shown` it is drawn among where that is another name.
fn named_type(cx: &Ctx, r#type: &str, shown: &str) -> String {
    let w = Words::of(cx.lang);
    let among = match shown.to_lowercase() == r#type.to_lowercase() {
        true => String::new(),
        false => format!(" data-shown=\"{}\"", esc(&shown.to_lowercase())),
    };
    field(
        "type",
        r#type,
        &["type", r#type],
        &format!(
            " class=\"name\" data-tip=\"{}\"{among}",
            esc(w.rename_every_tip)
        ),
    )
}

/// `most_held`: of `names`, a field's, the one the most books hold, the first on a tie.
fn most_held<'a>(cx: &Ctx, names: impl Iterator<Item = Name<'a>>) -> Option<&'a str> {
    let mut best: Option<(&'a str, usize)> = None;
    for name in names {
        let books = cx.linked.books(name);
        if best.is_none_or(|(_, most)| books > most) {
            best = Some((name.words(), books));
        }
    }
    best.map(|(words, _)| words)
}

/// `.types`: each type [`in_use`] by the name of it `values` holds ([`most_held`] of two, else
/// `shown`), a field renaming it on every book, its tags opening Books with × beside each, a
/// `.more` field adding one; `.fresh` under them, a type and its first tag.
fn types(cx: &Ctx, values: &Values) -> String {
    let w = Words::of(cx.lang);
    let (in_use, _) = in_use(cx);
    let mut out = String::from("<div class=\"types\">");
    for (shown, _) in &in_use {
        let held: Vec<&Tag> = values
            .tags
            .iter()
            .filter(|t| cx.linked.same(Name::Type(&t.r#type), Name::Type(shown)))
            .collect();
        let drawn = most_held(cx, held.iter().map(|t| Name::Type(&t.r#type)))
            .unwrap_or(shown)
            .to_string();
        let _ = write!(
            out,
            "<div class=\"kind\">{}<span class=\"held\">",
            named_type(cx, &drawn, shown)
        );
        for tag in held {
            let _ = write!(
                out,
                "<span class=\"tag\" data-tag=\"{1}\"><a href=\"{0}\"{4}>{1}</a><button class=\"off\" \
                 data-act=\"untag\" data-type=\"{2}\" data-tag=\"{1}\" data-tip=\"{3}\">×</button></span>",
                narrowed(By::Tag(tag.r#type.clone(), Some(tag.tag.clone())), None).link(),
                esc(&tag.tag),
                esc(&tag.r#type),
                esc(w.remove_tip),
                tip_of(
                    cx,
                    &cx.linked.others(Name::Tag(&tag.r#type, &tag.tag), &tag.tag)
                )
            );
        }
        let _ = write!(
            out,
            "<span class=\"more\"><input data-field=\"tag\" data-pick=\"tags\" placeholder=\"+\" \
             data-key=\"{}\" autocomplete=\"off\" spellcheck=\"false\"></span></span></div>",
            json(&["more", &drawn])
        );
    }
    let _ = write!(
        out,
        "<div class=\"kind fresh\"><input class=\"name\" data-field=\"type\" placeholder=\"{}\" \
         data-key=\"{}\" data-fit style=\"width: {}\" autocomplete=\"off\" spellcheck=\"false\">\
         <span class=\"held\"><span class=\"more\"><input data-field=\"tag\" data-pick=\"tags\" \
         placeholder=\"{}\" data-key=\"{}\" autocomplete=\"off\" spellcheck=\"false\" hidden></span></span></div></div>",
        esc(w.add_type),
        json(&["fresh"]),
        fit(w.add_type),
        esc(w.tag_field),
        json(&["fresh", "tag"])
    );
    out
}

/// `.types.all`: over Books, each type [`in_use`] with each tag and `not set` lit by `by` or held
/// among the books `shown`, by how many there and stating it, `pill` linking it into `by` or out
/// where lit, through `to`; `editing`, every name a field; `.edit` between the two.
pub(super) fn listed(
    cx: &Ctx,
    by: &[By],
    shown: &[usize],
    editing: bool,
    to: impl Fn(Vec<By>, bool) -> String,
) -> String {
    let w = Words::of(cx.lang);
    let (types, tags) = in_use(cx);
    if types.is_empty() {
        return String::new();
    }
    let spelled = spelled(cx);
    let (_, among) = held_among(cx, shown.iter().copied());
    let names = |held: &By, r#type: &str, tag: Option<&str>| match held {
        By::Tag(t, held) if cx.linked.same(Name::Type(t), Name::Type(r#type)) => {
            match (held, tag) {
                (Some(held), Some(tag)) => {
                    cx.linked.same(Name::Tag(t, held), Name::Tag(r#type, tag))
                }
                (None, None) => true,
                _ => false,
            }
        }
        _ => false,
    };
    // `pill`: whether `r#type`'s `tag` (`None`, not set) is lit, and its link taking it off
    // `by` where it is, adding it where it is not.
    let pill = |r#type: &str, tag: Option<&str>| {
        let lit = by.iter().any(|held| names(held, r#type, tag));
        let link = match lit {
            true => by
                .iter()
                .filter(|held| !names(held, r#type, tag))
                .cloned()
                .collect(),
            false => {
                let mut more = by.to_vec();
                more.push(By::Tag(r#type.to_string(), tag.map(str::to_string)));
                more
            }
        };
        (lit, to(link, false))
    };
    let held = |r#type: &str, tag: &str| {
        among
            .iter()
            .find(|(t, g, _)| t == r#type && g == tag)
            .map_or(0, |(.., n)| *n)
    };
    let unset = |r#type: &str| {
        let none = By::Tag(r#type.to_string(), None);
        shown
            .iter()
            .filter(|at| none.holds(&values(cx, **at), cx.linked))
            .count()
    };
    let (class, edit) = match editing {
        true => (" editing", w.done),
        false => ("", w.edit_tags),
    };
    let mut out = format!("<div class=\"types all{class}\">");
    for (r#type, _) in &types {
        let mut under: Vec<&str> = tags
            .iter()
            .filter(|(t, ..)| t == r#type)
            .map(|(_, tag, _)| tag.as_str())
            .collect();
        let numbered = under.iter().all(|tag| tag.parse::<i64>().is_ok());
        if numbered {
            under.sort_by_key(|tag| tag.parse::<i64>().unwrap_or(0));
        }
        if editing {
            let _ = write!(
                out,
                "<div class=\"kind\">{}<span class=\"held\">",
                type_fields(cx, r#type, &spelled)
            );
            for tag in under {
                out.push_str(&tag_fields(cx, r#type, tag, &spelled));
            }
            out.push_str("</span></div>");
            continue;
        }
        let mut offered: Vec<(&str, bool, String, usize)> = under
            .into_iter()
            .filter_map(|tag| {
                let (lit, link) = pill(r#type, Some(tag));
                let n = held(r#type, tag);
                (lit || n > 0).then_some((tag, lit, link, n))
            })
            .collect();
        if !numbered {
            offered.sort_by_key(|offer| std::cmp::Reverse(offer.3));
        }
        let (lit_unset, unset_link) = pill(r#type, None);
        let unset_n = unset(r#type);
        if offered.is_empty() && !lit_unset && unset_n == 0 {
            continue;
        }
        let _ = write!(
            out,
            "<div class=\"kind\"><span class=\"name\"{}>{}</span><span class=\"held\">",
            tip_of(cx, &cx.linked.also(Name::Type(r#type))),
            esc(r#type)
        );
        let on = |lit: bool| if lit { " on" } else { "" };
        for (tag, lit, link, n) in offered {
            let _ = write!(
                out,
                "<span class=\"tag{}\"><a href=\"{link}\"{}>{}<sup>{n}</sup></a></span>",
                on(lit),
                tip_of(cx, &cx.linked.also(Name::Tag(r#type, tag))),
                esc(tag)
            );
        }
        if lit_unset || unset_n > 0 {
            let _ = write!(
                out,
                "<span class=\"tag unset{}\"><a href=\"{unset_link}\">{}<sup>{unset_n}</sup></a></span>",
                on(lit_unset),
                esc(w.not_set)
            );
        }
        out.push_str("</span></div>");
    }
    let _ = write!(
        out,
        "<a class=\"edit\" href=\"{}\">{}</a></div>",
        to(by.to_vec(), !editing),
        esc(edit)
    );
    out
}

/// `.names`: the `.name` fields of the type `shown` and of its other names in use, together
/// in a `.linked` group with *Separate* where an `l` row joins them; *Same as…* after them
/// but for a type of [`names::TYPES`], which links no other.
fn type_fields(cx: &Ctx, shown: &str, spelled: &Spelled) -> String {
    let named: Vec<&str> = spelled
        .types
        .iter()
        .map(|(t, _)| t.as_str())
        .filter(|t| cx.linked.same(Name::Type(t), Name::Type(shown)))
        .collect();
    let named = match named.is_empty() {
        true => vec![shown],
        false => named,
    };
    let fields: String = named.iter().map(|t| named_type(cx, t, shown)).collect();
    let asked = |names: Vec<String>| Linking {
        linked: Linked::Type,
        r#type: String::new(),
        names,
    };
    let mut out = match cx.linked.linked(Name::Type(shown)) {
        true => format!(
            "<span class=\"linked\">{fields}{}</span>",
            separate(cx, &asked(owned(&named)), "apart")
        ),
        false => fields,
    };
    if !names::is_type(shown) {
        let mut not: Vec<&str> = named.clone();
        not.extend(names::TYPES);
        out.push_str(&same_as(
            cx,
            &asked(vec![shown.to_string()]),
            "types",
            shown,
            &not,
        ));
    }
    format!("<span class=\"names\">{out}</span>")
}

/// `tag_fields`: the fields of the tag `shown` of `r#type` and its other names in use, each with ×
/// taking it off every book, in a `.linked` group with *Separate* where an `l` row joins them, and
/// *Same as…* beside them but under a type of [`names::TYPES`], whose tags link by their codes.
fn tag_fields(cx: &Ctx, r#type: &str, shown: &str, spelled: &Spelled) -> String {
    let w = Words::of(cx.lang);
    let named: Vec<(&str, &str)> = spelled
        .tags
        .iter()
        .map(|(t, g, _)| (t.as_str(), g.as_str()))
        .filter(|(t, g)| cx.linked.same(Name::Tag(t, g), Name::Tag(r#type, shown)))
        .collect();
    let named = match named.is_empty() {
        true => vec![(r#type, shown)],
        false => named,
    };
    let asked = |names: Vec<String>| Linking {
        linked: Linked::Tag,
        r#type: r#type.to_string(),
        names,
    };
    let same = match names::is_type(r#type) {
        true => String::new(),
        false => {
            let not: Vec<&str> = named.iter().map(|(_, g)| *g).collect();
            same_as(cx, &asked(vec![shown.to_string()]), "tags", shown, &not)
        }
    };
    let linked = cx.linked.linked(Name::Tag(r#type, shown));
    let mut pills = String::new();
    for (own, tag) in &named {
        let _ = write!(
            pills,
            "<span class=\"tag\" data-tag=\"{0}\">{1}{4}<button class=\"off\" data-act=\"untag-every\" \
             data-type=\"{2}\" data-tag=\"{0}\" data-tip=\"{3}\">×</button></span>",
            esc(tag),
            field(
                "tag",
                tag,
                &["tag", own, tag],
                &format!(
                    " data-tip=\"{}\" data-type=\"{}\"",
                    esc(w.rename_every_tip),
                    esc(own)
                )
            ),
            esc(own),
            esc(w.remove_every_tip),
            match linked {
                true => "",
                false => same.as_str(),
            }
        );
    }
    match linked {
        true => {
            let names: Vec<&str> = named.iter().map(|(_, g)| *g).collect();
            format!(
                "<span class=\"linked\">{pills}{same}{}</span>",
                separate(cx, &asked(owned(&names)), "apart")
            )
        }
        false => pills,
    }
}

/// `names` owned.
fn owned(names: &[&str]) -> Vec<String> {
    names.iter().map(|n| n.to_string()).collect()
}

/// `=`: the button opening `.linking` to count `asked`'s value, drawn as `shown`, as one
/// with a name of the pick list `pick`, `not` left out of it.
fn same_as(cx: &Ctx, asked: &Linking, pick: &str, shown: &str, not: &[&str]) -> String {
    let w = Words::of(cx.lang);
    format!(
        "<button class=\"same\" {}data-tip=\"{}\">=</button>",
        same_as_data(cx, asked, pick, shown, not),
        esc(w.same_as)
    )
}

/// `same_as_data`: the attributes a *Same as…* button sets `.linking` out by: `asked`, the
/// question naming `shown`, the pick list `pick`, the type it offers tags of, `not` lower case.
fn same_as_data(cx: &Ctx, asked: &Linking, pick: &str, shown: &str, not: &[&str]) -> String {
    let w = Words::of(cx.lang);
    let not: Vec<String> = not.iter().map(|n| n.to_lowercase()).collect();
    format!(
        "data-act=\"same-as\" data-linking=\"{}\" data-ask=\"{}\" data-pick=\"{pick}\" \
         data-type=\"{}\" data-not=\"{}\" ",
        json(asked),
        esc(&w.same_as_ask.replace("{v}", shown)),
        esc(&asked.r#type.to_lowercase()),
        json(&not)
    )
}

/// `separate`: *Separate*, of the class `class`, the button counting `asked`'s names apart again.
fn separate(cx: &Ctx, asked: &Linking, class: &str) -> String {
    let w = Words::of(cx.lang);
    format!(
        "<button class=\"{class}\" data-act=\"separate\" data-linking=\"{}\" data-said=\"{}\" \
         data-tip=\"{}\">{}</button>",
        json(asked),
        esc(w.counted_apart),
        esc(w.separate_tip),
        esc(w.separate)
    )
}

/// `at`'s editor: its title, authors and publisher, each field holding what
/// `values` states; under a title or authors `cx.metadata` sets, the book's own
/// words with Revert.
fn editor(cx: &Ctx, at: usize, values: &Values) -> String {
    let w = Words::of(cx.lang);
    let book = &cx.stats.books[at];
    let set = &cx.metadata[at];
    let (own_title, own_author) = &cx.own[at];
    let says = |own: &str, field: &str| {
        format!(
            "<p class=\"book\">{}<button data-act=\"revert-field\" data-field=\"{field}\">{}</button></p>",
            esc(&w.book_says.replace("{s}", own)),
            esc(w.revert)
        )
    };
    let field = |name: &str, field: &str, value: &str, class: &str, more: &str| {
        format!(
            "<label class=\"field\"><span>{}</span><input class=\"box{class}\" data-field=\"{field}\" \
             value=\"{}\" data-was=\"{}\"{more} autocomplete=\"off\" spellcheck=\"false\"></label>",
            esc(name),
            esc(value),
            esc(value)
        )
    };
    let authors = metadata::joined(&values.authors);
    let mut out = String::from("<div class=\"metaedit\" hidden>");
    out.push_str(&field(w.title_field, "title", &book.title, " title", ""));
    if set.title.is_some() {
        out.push_str(&says(own_title, "title"));
    }
    out.push_str(&field(w.authors_field, "authors", &authors, "", ""));
    if set.authors.is_some() {
        out.push_str(&says(own_author, "authors"));
    }
    out.push_str(&field(
        w.publisher_field,
        "publisher",
        values.publisher.as_deref().unwrap_or_default(),
        "",
        "",
    ));
    let _ = write!(
        out,
        "<p class=\"acts\"><button data-act=\"cancel-meta\">{}</button>\
         <button class=\"save\" data-act=\"save-meta\">{}</button></p></div>",
        esc(cx.s.cancel),
        esc(w.save)
    );
    out
}

/// [`names::LANGUAGES`] as a tag field under a type of [`names::TYPES`] offers
/// them past the tags in use, each named in `cx.lang` and its own; those a tag
/// of `tags` names left out.
fn languages(cx: &Ctx, tags: &Tags) -> String {
    let held: Vec<&str> = tags
        .iter()
        .filter(|(r#type, ..)| names::is_type(r#type))
        .filter_map(|(_, tag, _)| names::code_of(tag))
        .collect();
    let mut out = String::new();
    for (code, ..) in names::LANGUAGES.iter().filter(|(c, ..)| !held.contains(c)) {
        let name = language_name(cx, code);
        let own = names::own(code).unwrap_or(code);
        let shown = match own == name {
            true => name.clone(),
            false => format!("{own} · {name}"),
        };
        let _ = write!(
            out,
            "<div data-lang=\"{}\" data-name=\"{}\" data-words=\"{}\"><span>{}</span><small>{}</small></div>",
            esc(code),
            esc(&name),
            esc(&format!("{own} {name} {code}").to_lowercase()),
            esc(&shown),
            esc(code)
        );
    }
    out
}

/// `Types`: each type in use and the books holding a tag of it.
type Types = Vec<(String, usize)>;

/// `Tags`: each tag in use, its type and the books holding it.
type Tags = Vec<(String, String, usize)>;

/// `Spelled`: each type and each tag in use as the books hold them, a value of several
/// names under each name apart.
struct Spelled {
    types: Types,
    tags: Tags,
}

/// [`Spelled`] across `cx.metadata`, most held first.
fn spelled(cx: &Ctx) -> Spelled {
    let mut types: Types = Vec::new();
    let mut tags: Tags = Vec::new();
    for set in cx.metadata {
        let mut seen: Vec<&str> = Vec::new();
        for tag in &set.tags {
            if !seen.contains(&tag.r#type.as_str()) {
                seen.push(&tag.r#type);
                match types.iter_mut().find(|(t, _)| *t == tag.r#type) {
                    Some((_, n)) => *n += 1,
                    None => types.push((tag.r#type.clone(), 1)),
                }
            }
            match tags
                .iter_mut()
                .find(|(t, g, _)| *t == tag.r#type && *g == tag.tag)
            {
                Some((.., n)) => *n += 1,
                None => tags.push((tag.r#type.clone(), tag.tag.clone(), 1)),
            }
        }
    }
    types.sort_by(|a, b| b.1.cmp(&a.1).then(a.0.cmp(&b.0)));
    tags.sort_by(|a, b| b.2.cmp(&a.2).then(a.1.cmp(&b.1)));
    Spelled { types, tags }
}

/// [`Types`] and [`Tags`] across `cx.metadata`, a value of several names once under the
/// name it is drawn by, a book holding two of its names counted once; most held first.
fn in_use(cx: &Ctx) -> (Types, Tags) {
    held_among(cx, 0..cx.metadata.len())
}

/// `held_among`: [`in_use`] over the books `among` alone.
fn held_among(cx: &Ctx, among: impl IntoIterator<Item = usize>) -> (Types, Tags) {
    let mut types: Types = Vec::new();
    let mut tags: Tags = Vec::new();
    for set in among.into_iter().map(|at| &cx.metadata[at]) {
        let mut seen: Vec<&str> = Vec::new();
        let mut had: Vec<(&str, &str)> = Vec::new();
        for tag in &set.tags {
            let r#type = cx.linked.shown(Name::Type(&tag.r#type));
            if !seen.contains(&r#type) {
                seen.push(r#type);
                match types.iter_mut().find(|(t, _)| t == r#type) {
                    Some((_, n)) => *n += 1,
                    None => types.push((r#type.to_string(), 1)),
                }
            }
            let shown = cx.linked.shown(Name::Tag(&tag.r#type, &tag.tag));
            if had.contains(&(r#type, shown)) {
                continue;
            }
            had.push((r#type, shown));
            match tags.iter_mut().find(|(t, g, _)| t == r#type && g == shown) {
                Some((.., n)) => *n += 1,
                None => tags.push((r#type.to_string(), shown.to_string(), 1)),
            }
        }
    }
    types.sort_by(|a, b| b.1.cmp(&a.1).then(a.0.cmp(&b.0)));
    tags.sort_by(|a, b| b.2.cmp(&a.2).then(a.1.cmp(&b.1)));
    (types, tags)
}

/// `picks`: [`spelled`]'s types and tags as the lists a field offers, held once for `app.js` to
/// move under the field focused, a tag under its type's drawn name, a type drawn among another's
/// naming it `data-shown`; under a type of [`names::TYPES`], `data-languages` adds [`languages`].
fn picks(cx: &Ctx) -> String {
    let Spelled { types, tags } = spelled(cx);
    let books = |n: usize| counted(cx.s.n_books, n as i64);
    let mut out = String::from("<div class=\"pick\" data-for=\"types\" hidden>");
    for (r#type, n) in &types {
        let shown = cx.linked.shown(Name::Type(r#type)).to_lowercase();
        let among = match shown == r#type.to_lowercase() {
            true => String::new(),
            false => format!(" data-shown=\"{}\"", esc(&shown)),
        };
        let _ = write!(
            out,
            "<div data-name=\"{}\" data-words=\"{}\"{among}><span>{}</span><small>{}</small></div>",
            esc(r#type),
            esc(&r#type.to_lowercase()),
            esc(r#type),
            esc(&books(*n))
        );
    }
    let types_of_language: Vec<String> = names::TYPES.iter().map(|t| t.to_lowercase()).collect();
    let _ = write!(
        out,
        "</div><div class=\"pick\" data-for=\"tags\" data-languages=\"{}\" hidden>",
        esc(&types_of_language.join("|"))
    );
    for (r#type, tag, n) in &tags {
        let _ = write!(
            out,
            "<div data-type=\"{}\" data-name=\"{}\" data-words=\"{}\"><span>{}</span><small>{}</small></div>",
            esc(&cx.linked.shown(Name::Type(r#type)).to_lowercase()),
            esc(tag),
            esc(&tag.to_lowercase()),
            esc(tag),
            esc(&books(*n))
        );
    }
    out.push_str(&languages(cx, &tags));
    out.push_str("</div>");
    out
}

/// `.adding`: the bulk editor's Type and Tag fields with Add, offering [`picks`].
fn adder(cx: &Ctx) -> String {
    let w = Words::of(cx.lang);
    format!(
        "<div class=\"adding\"><div class=\"field\"><span>{}</span><input class=\"box\" \
         data-field=\"type\" data-pick=\"types\" autocomplete=\"off\" spellcheck=\"false\"></div>\
         <div class=\"field\"><span>{}</span><input class=\"box\" data-field=\"tag\" \
         data-pick=\"tags\" autocomplete=\"off\" spellcheck=\"false\"></div><p class=\"acts\">\
         <button class=\"save\" data-act=\"save-tag\">{}</button></p></div>",
        esc(w.type_field),
        esc(w.tag_field),
        esc(w.add)
    )
}

/// `by`'s name and field: the [`Name`] its value is, and the field it links in.
fn name_of(by: &By) -> Option<(Name<'_>, Linked)> {
    match by {
        By::Author(Some(v)) => Some((Name::Author(v), Linked::Author)),
        By::Publisher(Some(v)) => Some((Name::Publisher(v), Linked::Publisher)),
        By::Tag(t, Some(v)) => Some((Name::Tag(t, v), Linked::Tag)),
        _ => None,
    }
}

/// `chip`: `by`'s chip on Books, its value drawn by its most held name, the others in its
/// tooltip, its × going back to `back`, with Rename… where `by` names a value, *Same as…* for
/// an author or a publisher, and *Separate* where an `l` row joins its names.
pub(super) fn chip(cx: &Ctx, by: &By, back: &str) -> String {
    let w = Words::of(cx.lang);
    let what = match by {
        By::Author(_) => w.author_chip.to_string(),
        By::Publisher(_) => w.publisher_field.to_string(),
        By::Tag(t, _) => t.clone(),
    };
    let named = name_of(by);
    let mut out = format!(
        "<span class=\"chip filter\"{}><small>{}</small>{}<a class=\"off\" href=\"{back}\">×</a></span>",
        named
            .map(|(name, _)| tip_of(cx, &cx.linked.also(name)))
            .unwrap_or_default(),
        esc(&what),
        esc(named.map_or(w.not_set, |(name, _)| cx.linked.shown(name)))
    );
    let Some((name, linked)) = named else {
        return out;
    };
    let _ = write!(
        out,
        "<button class=\"chip act\" data-act=\"rename\">{}</button>",
        esc(w.rename)
    );
    if linked == Linked::Tag {
        return out;
    }
    let shown = cx.linked.shown(name);
    let every = cx.linked.every(name);
    let asked = |names: Vec<String>| Linking {
        linked,
        r#type: String::new(),
        names,
    };
    let _ = write!(
        out,
        "<button class=\"chip act\" {}>{}</button>",
        same_as_data(cx, &asked(vec![shown.to_string()]), "names", shown, &every),
        esc(w.same_as)
    );
    if cx.linked.linked(name) {
        out.push_str(&separate(cx, &asked(owned(&every)), "chip act"));
    }
    out
}

/// `.renaming`: the field renaming `by` on the books `shown` holding it, hidden until
/// Rename… opens it; for a value of several names, a field for each name the books shown
/// hold, renaming that name alone.
pub(super) fn renaming(cx: &Ctx, by: &By, shown: &[usize]) -> String {
    let w = Words::of(cx.lang);
    let Some((name, _)) = name_of(by) else {
        return String::new();
    };
    let field = |value: &str, count: usize, own: &str| {
        format!(
            "<label class=\"field\"{own}><span>{}</span><input class=\"box\" value=\"{}\"{} \
             autocomplete=\"off\" spellcheck=\"false\"></label>",
            esc(&counted(w.rename_ask, count as i64).replace("{v}", value)),
            esc(value),
            match own.is_empty() {
                true => String::new(),
                false => format!(" data-was=\"{}\"", esc(value)),
            }
        )
    };
    let fields = match cx.linked.linked(name) {
        false => field(name.words(), shown.len(), ""),
        true => cx
            .linked
            .every(name)
            .into_iter()
            .filter_map(|each| {
                let one = match by {
                    By::Author(_) => By::Author(Some(each.to_string())),
                    By::Publisher(_) => By::Publisher(Some(each.to_string())),
                    By::Tag(t, _) => By::Tag(t.clone(), Some(each.to_string())),
                };
                let count = shown
                    .iter()
                    .filter(|at| one.holds(&values(cx, **at), &Default::default()))
                    .count();
                (count > 0).then(|| field(each, count, &format!(" data-by=\"{}\"", json(&one))))
            })
            .collect(),
    };
    format!(
        "<div class=\"renaming adding\" hidden data-by=\"{}\">{fields}\
         <p class=\"acts\"><button data-act=\"cancel-rename\">{}</button>\
         <button class=\"save\" data-act=\"save-rename\">{}</button></p></div>",
        json(by),
        esc(cx.s.cancel),
        esc(w.rename_do)
    )
}

/// `.linking`: the field counting a value as one with another, hidden until a *Same as…*
/// sets it out and opens it; with `by` an author or a publisher, its pick list holds that
/// field's names in use, each apart, most held first.
pub(super) fn linking(cx: &Ctx, by: Option<&By>) -> String {
    let w = Words::of(cx.lang);
    let named = |at: usize| -> Vec<String> {
        let values = values(cx, at);
        match by {
            Some(By::Author(Some(_))) => values.authors,
            Some(By::Publisher(Some(_))) => values.publisher.into_iter().collect(),
            _ => Vec::new(),
        }
    };
    let mut held: Vec<(String, usize)> = Vec::new();
    for at in 0..cx.stats.books.len() {
        for name in named(at) {
            match held.iter_mut().find(|(n, _)| *n == name) {
                Some((_, books)) => *books += 1,
                None => held.push((name, 1)),
            }
        }
    }
    held.sort_by(|a, b| b.1.cmp(&a.1).then(a.0.cmp(&b.0)));
    let mut pick = String::new();
    if !held.is_empty() {
        pick.push_str("<div class=\"pick\" data-for=\"names\" hidden>");
        for (name, books) in &held {
            let _ = write!(
                pick,
                "<div data-name=\"{}\" data-words=\"{}\"><span>{}</span><small>{}</small></div>",
                esc(name),
                esc(&name.to_lowercase()),
                esc(name),
                esc(&counted(cx.s.n_books, *books as i64))
            );
        }
        pick.push_str("</div>");
    }
    format!(
        "<div class=\"linking adding\" hidden data-said=\"{}\"><label class=\"field\"><span></span>\
         <input class=\"box\" autocomplete=\"off\" spellcheck=\"false\"><input data-field=\"type\" \
         hidden>{pick}</label><p class=\"acts\"><button data-act=\"cancel-link\">{}</button>\
         <button class=\"save\" data-act=\"save-link\">{}</button></p></div>",
        esc(w.counted_one),
        esc(cx.s.cancel),
        esc(w.same_as_do)
    )
}

/// `.chosen`: the selection's bar over `total` books, hidden until a book is picked, in the
/// shelf chips' place.
pub(super) fn chosen(cx: &Ctx, total: usize) -> String {
    let w = Words::of(cx.lang);
    format!(
        "<div class=\"chosen\" hidden data-said=\"{}\" data-total=\"{total}\"><b></b>\
         <button class=\"button\" data-act=\"bulk\">{}</button>\
         <button class=\"quiet\" data-act=\"unpick\">{}</button></div>",
        esc(w.chosen),
        esc(w.edit_many),
        esc(w.unpick)
    )
}

/// `.sheet`: the bulk editor, hidden until Edit… opens it on the books picked, each
/// field's books and their tags set out by `app.js`'s `openSheet`.
pub(super) fn sheet(cx: &Ctx) -> String {
    let w = Words::of(cx.lang);
    let field = |name: &str, field: &str, back: &str| {
        format!(
            "<div class=\"field\"><span>{}<button class=\"clear\" data-act=\"bulk-back\" \
             data-field=\"{field}\">{}</button></span><input class=\"box\" data-field=\"{field}\" \
             autocomplete=\"off\" spellcheck=\"false\"></div>",
            esc(name),
            esc(back)
        )
    };
    format!(
        "<div class=\"sheet\" hidden data-head=\"{}\" data-leave=\"{}\" data-values=\"{}\" \
         data-none=\"{}\" data-some=\"{}\" data-apply=\"{}\"><h3></h3><p class=\"lede\">{}</p>{}{}\
         <div class=\"field retag\" data-off=\"{}\"><span>{}</span></div>{}{}\
         <p class=\"acts\"><button data-act=\"cancel-bulk\">{}</button>\
         <button class=\"save\" data-act=\"apply-bulk\"></button></p></div>",
        esc(w.edit_books),
        esc(w.leave),
        esc(w.leave_values),
        esc(w.leave_none),
        esc(w.of_some),
        esc(w.apply_to),
        esc(w.leave_note),
        field(w.authors_field, "authors", w.each_own),
        field(w.publisher_field, "publisher", w.empty),
        esc(w.remove_tip),
        esc(w.tags_field),
        adder(cx),
        picks(cx),
        esc(cx.s.cancel)
    )
}

/// `data-book` and `data-values` of the tile of `at`: [`named`] and [`values`].
pub(super) fn tile_data(cx: &Ctx, at: usize) -> String {
    format!(
        " data-book=\"{}\" data-values=\"{}\"",
        json(&named(cx, at)),
        json(&values(cx, at))
    )
}

/// `One`: one book's own patch, as `edit_metadata_each` takes it.
#[derive(Serialize)]
struct One<'a> {
    book: Named,
    patch: &'a Patch,
}

/// `.suggest` over the books `shown`: a language tag their characters settle where
/// `cx.metadata` holds none, and authors written surname first turned round.
pub(super) fn suggestions(cx: &Ctx, shown: &[usize]) -> String {
    let w = Words::of(cx.lang);
    let (types, tags) = in_use(cx);
    // `kind`: the type of `names::TYPES` most held, else the one `cx.lang` names.
    let kind = types
        .iter()
        .find(|(t, _)| names::is_type(t))
        .map_or_else(|| names::type_word(cx.lang).to_string(), |(t, _)| t.clone());
    let mut by_language: BTreeMap<&str, Vec<usize>> = BTreeMap::new();
    let mut turned: Vec<(usize, String)> = Vec::new();
    for &at in shown {
        let book = &cx.stats.books[at];
        let spoken = cx.metadata[at]
            .tags
            .iter()
            .any(|t| names::is_type(&t.r#type));
        if !spoken && let Some(code) = metadata::suggested_language(&book.title, &book.author) {
            by_language.entry(code).or_default().push(at);
        }
        if let Some(authors) = metadata::suggested_authors(&book.author) {
            turned.push((at, authors));
        }
    }
    let mut out = String::new();
    for (code, books) in &by_language {
        let titles: Vec<String> = books
            .iter()
            .map(|at| cx.stats.books[*at].title.clone())
            .collect();
        // `name`: the tag in use naming `code`, else its name in `cx.lang`.
        let name = tags
            .iter()
            .find(|(t, tag, _)| names::is_type(t) && names::code_of(tag) == Some(*code))
            .map_or_else(|| language_name(cx, code), |(_, tag, _)| tag.clone());
        let patch = Patch {
            add: vec![Tag {
                r#type: kind.clone(),
                tag: name.clone(),
            }],
            ..Patch::default()
        };
        let each: Vec<One> = books
            .iter()
            .map(|at| One {
                book: named(cx, *at),
                patch: &patch,
            })
            .collect();
        let _ = write!(
            out,
            "<p><span>{}</span><button class=\"button\" data-act=\"suggest\" data-each=\"{}\">{}</button></p>",
            esc(&counted(w.look_lang, books.len() as i64)
                .replace("{lang}", &name)
                .replace("{titles}", &titles.join(", "))),
            json(&each),
            esc(&w.set_lang.replace("{lang}", &name))
        );
    }
    if !turned.is_empty() {
        let list: Vec<String> = turned
            .iter()
            .map(|(at, to)| format!("{} → {to}", cx.stats.books[*at].author))
            .collect();
        let patches: Vec<Patch> = turned
            .iter()
            .map(|(_, to)| Patch {
                authors: Some(metadata::Change::To(to.clone())),
                ..Patch::default()
            })
            .collect();
        let each: Vec<One> = turned
            .iter()
            .zip(&patches)
            .map(|((at, _), patch)| One {
                book: named(cx, *at),
                patch,
            })
            .collect();
        let _ = write!(
            out,
            "<p><span>{}</span><button class=\"button\" data-act=\"suggest\" data-each=\"{}\">{}</button></p>",
            esc(&counted(w.surname_first, turned.len() as i64).replace("{list}", &list.join(" · "))),
            json(&each),
            esc(w.turn_them)
        );
    }
    match out.is_empty() {
        true => out,
        false => format!("<div class=\"suggest\">{out}</div>"),
    }
}

/// `Bar`: one value's bar, its name, time and books, the Books it opens, and the
/// value's other names its tooltip says.
struct Bar {
    name: String,
    lang: Option<String>,
    secs: i64,
    books: usize,
    to: Route,
    also: String,
}

/// `bar`'s tooltip: its name, the value's other names, then `figures`.
fn bar_tip(bar: &Bar, figures: &[&str]) -> String {
    let mut parts: Vec<&str> = vec![bar.name.as_str()];
    if !bar.also.is_empty() {
        parts.push(&bar.also);
    }
    parts.extend(figures);
    parts.join(" · ")
}

/// `head`, a card's head naming a type drawn among other names, escaped, its other names
/// in a tooltip.
fn headed(cx: &Ctx, head: &str, others: &[&str]) -> String {
    match others.is_empty() {
        true => esc(head),
        false => format!("<span{}>{}</span>", tip_of(cx, others), esc(head)),
    }
}

/// `days`' charts, the whole record's where `None`: authors, publishers, languages and each
/// type of tag, a value not set as its own bar, each bar opening Books narrowed to it under
/// `window`; empty where no book read sets any. `breakdown` gives the figures.
pub(super) fn charts(
    cx: &Ctx,
    days: Option<RangeInclusive<i64>>,
    window: Option<Window>,
) -> String {
    let w = Words::of(cx.lang);
    let read = breakdown::read(
        cx.stats,
        days.as_ref(),
        |at| cx.stats.books[at].read_seconds(cx.from),
        |at| values(cx, at),
    );
    if read.is_empty() {
        return String::new();
    }
    let total: i64 = read.iter().map(|(_, secs, _)| secs).sum();
    let figured = breakdown::ranked(&read, cx.linked);
    let mut cards: Vec<String> = Vec::new();
    for card in figured
        .iter()
        .filter(|c| !matches!(c.ranks, Ranks::Type(_)))
    {
        let (chart, head) = match card.ranks {
            Ranks::Authors => ("authors", esc(w.authors_head)),
            _ => ("publishers", esc(w.publishers_head)),
        };
        let (bars, unset) = drawn(cx, card, window);
        cards.push(ranked(cx, chart, &head, total, bars, unset, None));
    }
    let languages = languages_chart(cx, &read, days.as_ref(), window, total);
    if !languages.is_empty() {
        cards.push(languages);
    }
    for card in &figured {
        let Ranks::Type(r#type) = &card.ranks else {
            continue;
        };
        let (bars, unset) = drawn(cx, card, window);
        let note = card
            .doubled
            .then(|| w.counts_each.replace("{t}", &date::duration(total, cx.s)));
        let also: Vec<&str> = card.also.iter().map(String::as_str).collect();
        let head = headed(cx, r#type, &also);
        cards.push(match card.numbers {
            true => columns(cx, &head, bars, unset, note),
            false => ranked(
                cx,
                &format!("type {}", r#type),
                &head,
                total,
                bars,
                unset,
                note,
            ),
        });
    }
    let mut out = String::new();
    for pair in cards.chunks(2) {
        let _ = write!(out, "<section class=\"split\">{}</section>", pair.join(""));
    }
    out
}

/// `drawn`: `card`'s bars and its bar not set as a card draws them, each opening Books
/// narrowed to its value under `window`, an author's name in its first book's language.
fn drawn(cx: &Ctx, card: &breakdown::Ranked, window: Option<Window>) -> (Vec<Bar>, Vec<Bar>) {
    let w = Words::of(cx.lang);
    let one = |bar: &breakdown::Bar| {
        let by = match &card.ranks {
            Ranks::Authors => By::Author(bar.name.clone()),
            Ranks::Publishers => By::Publisher(bar.name.clone()),
            Ranks::Type(r#type) => By::Tag(r#type.clone(), bar.name.clone()),
        };
        let lang = match (&card.ranks, &bar.name) {
            (Ranks::Authors, Some(name)) => {
                let named = language_of(&cx.stats.books[bar.first].language, name);
                (!named.is_empty()).then_some(named)
            }
            _ => None,
        };
        let also: Vec<&str> = bar.also.iter().map(String::as_str).collect();
        Bar {
            name: bar.name.as_deref().unwrap_or(w.not_set).to_string(),
            lang,
            secs: bar.secs,
            books: bar.books,
            to: narrowed(by, window),
            also: match also.is_empty() {
                true => String::new(),
                false => said_also(cx, &also),
            },
        }
    };
    (
        card.bars.iter().map(one).collect(),
        card.unset.iter().map(one).collect(),
    )
}

/// `chart`'s card of ranked bars headed `head`: `bars` by time, the most read in the mark's
/// colour, the first [`TOP`] shown and the rest behind Show all, then `unset` and `note`;
/// `head`, escaped, states `total` and the most.
fn ranked(
    cx: &Ctx,
    chart: &str,
    head: &str,
    total: i64,
    mut bars: Vec<Bar>,
    unset: Vec<Bar>,
    note: Option<String>,
) -> String {
    let w = Words::of(cx.lang);
    bars.sort_by(|a, b| b.secs.cmp(&a.secs).then(a.name.cmp(&b.name)));
    let most = bars
        .iter()
        .chain(&unset)
        .map(|b| b.secs)
        .max()
        .unwrap_or(0)
        .max(1);
    let books = |n: usize| counted(cx.s.n_books, n as i64);
    let mut out = format!(
        "<div data-chart=\"{}\"><h2>{}<span class=\"said\">{} · {} {}</span></h2><div class=\"card\"><div class=\"ranked\">",
        esc(chart),
        head,
        esc(&date::duration(total, cx.s)),
        esc(cx.s.most),
        esc(bars.first().map(|b| b.name.as_str()).unwrap_or_default())
    );
    let rows = bars
        .iter()
        .map(|bar| (bar, false))
        .chain(unset.iter().map(|bar| (bar, true)));
    for (n, (bar, not_set)) in rows.enumerate() {
        let colour = match (n, not_set) {
            (_, true) => UNSET,
            (0, false) => MOST,
            _ => BAR,
        };
        let past = !not_set && n >= TOP;
        let width = bar.secs as f64 / most as f64 * 78.0;
        let time = date::duration(bar.secs, cx.s);
        let _ = write!(
            out,
            "<a class=\"r{}\" href=\"{}\" data-tip=\"{}\"{}><span class=\"who\"{}>{}</span>\
             <span class=\"bar\"><i style=\"width: {width:.1}%; background: {colour}\"></i></span>\
             <b>{}</b><span class=\"n\">{}</span></a>",
            match (colour, past) {
                (UNSET, _) => " unset",
                (_, true) => " past",
                _ => "",
            },
            bar.to.link(),
            esc(&bar_tip(bar, &[&time, &books(bar.books)])),
            match past {
                true => " hidden",
                false => "",
            },
            bar.lang
                .as_deref()
                .map(|l| format!(" lang=\"{}\"", esc(l)))
                .unwrap_or_default(),
            esc(&bar.name),
            esc(&time),
            esc(&books(bar.books))
        );
    }
    out.push_str("</div>");
    if bars.len() > TOP {
        let all = w.show_all.replace("{d}", &bars.len().to_string());
        let _ = write!(
            out,
            "<button class=\"whole\" data-act=\"whole\" data-all=\"{}\" data-top=\"{}\">{}</button>",
            esc(&all),
            esc(&w.show_top.replace("{d}", &TOP.to_string())),
            esc(&all)
        );
    }
    if let Some(note) = note {
        let _ = write!(out, "<p class=\"note\">{}</p>", esc(&note));
    }
    out.push_str("</div></div>");
    out
}

/// `bars` as columns under `head`, escaped, for a type whose tags are all numbers: one a
/// number, in their order, every whole number between standing; `unset` stated
/// under them.
fn columns(cx: &Ctx, head: &str, bars: Vec<Bar>, unset: Vec<Bar>, note: Option<String>) -> String {
    let w = Words::of(cx.lang);
    let numbered: Vec<(i64, &Bar)> = bars
        .iter()
        .filter_map(|b| b.name.parse::<i64>().ok().map(|n| (n, b)))
        .collect();
    let low = numbered.iter().map(|(n, _)| *n).min().unwrap_or(0).min(1);
    let high = numbered.iter().map(|(n, _)| *n).max().unwrap_or(0);
    let most = numbered
        .iter()
        .map(|(_, b)| b.secs)
        .max()
        .unwrap_or(0)
        .max(1);
    let top = numbered
        .iter()
        .max_by_key(|(n, b)| (b.secs, *n))
        .map(|(n, _)| n.to_string())
        .unwrap_or_default();
    let mut out = format!(
        "<div><h2>{}<span class=\"said\">{} {}</span></h2><div class=\"card\"><div class=\"cols\">",
        head,
        esc(cx.s.most),
        esc(&top)
    );
    for n in low..=high.min(low + 40) {
        match numbered.iter().find(|(m, _)| *m == n) {
            Some((_, bar)) => {
                let colour = match bar.secs == most {
                    true => MOST,
                    false => BAR,
                };
                let _ = write!(
                    out,
                    "<a class=\"c\" href=\"{}\" data-tip=\"{}\"><b>{}</b>\
                     <i style=\"height: {:.0}px; background: {colour}\"></i><span>{n}</span></a>",
                    bar.to.link(),
                    esc(&bar_tip(bar, &[&counted(cx.s.n_books, bar.books as i64)])),
                    esc(&date::duration(bar.secs, cx.s)),
                    bar.secs as f64 / most as f64 * 130.0
                );
            }
            None => {
                let _ = write!(
                    out,
                    "<div class=\"c\"><i class=\"none\"></i><span>{n}</span></div>"
                );
            }
        }
    }
    out.push_str("</div>");
    if let Some(bar) = unset.first() {
        let _ = write!(
            out,
            "<p class=\"note\"><a href=\"{}\">{} · {} · {}</a></p>",
            bar.to.link(),
            esc(w.not_set),
            esc(&date::duration(bar.secs, cx.s)),
            esc(&counted(cx.s.n_books, bar.books as i64))
        );
    }
    if let Some(note) = note {
        let _ = write!(out, "<p class=\"note\">{}</p>", esc(&note));
    }
    out.push_str("</div></div>");
    out
}

/// `every`: each book's values, by its place in `cx.stats.books`.
fn every(cx: &Ctx) -> Vec<Values> {
    (0..cx.metadata.len()).map(|at| values(cx, at)).collect()
}

/// `read`'s languages card off `breakdown::languages`: each language's share of the time
/// read, a book's split evenly among its languages, its legend, and the months split by
/// language where `days` holds more than one.
fn languages_chart(
    cx: &Ctx,
    read: &[(usize, i64, Values)],
    days: Option<&RangeInclusive<i64>>,
    window: Option<Window>,
    total: i64,
) -> String {
    let w = Words::of(cx.lang);
    let months = months(cx, days);
    let spans: Vec<RangeInclusive<i64>> = months.iter().map(|(_, span)| span.clone()).collect();
    let Some(card) = breakdown::languages(cx.stats, read, &every(cx), cx.linked, &spans) else {
        return String::new();
    };
    let kind = &card.r#type;
    let hue = hues(cx);
    let colour = |tag: &Option<Tag>| match tag {
        None => UNSET.to_string(),
        Some(tag) => hue(&tag.tag),
    };
    let top = card
        .parts
        .iter()
        .find_map(|(tag, _)| tag.as_ref().map(|t| t.tag.clone()))
        .unwrap_or_default();
    let also: Vec<&str> = card.also.iter().map(String::as_str).collect();
    let mut out = format!(
        "<div><h2>{}<span class=\"said\">{} · {} {}</span></h2><div class=\"card\"><div class=\"share\">",
        headed(cx, kind, &also),
        esc(&date::duration(total, cx.s)),
        esc(cx.s.most),
        esc(&top)
    );
    for (tag, secs) in &card.parts {
        let _ = write!(
            out,
            "<i style=\"flex: {secs}; background: {}\"></i>",
            colour(tag)
        );
    }
    out.push_str("</div><div class=\"legend\">");
    for ((tag, secs), share) in card.parts.iter().zip(&card.shares) {
        let (class, name, by) = match tag {
            Some(tag) => (
                "",
                tag.tag.as_str(),
                By::Tag(tag.r#type.clone(), Some(tag.tag.clone())),
            ),
            None => (" unset", w.not_set, By::Tag(kind.clone(), None)),
        };
        let also = tag
            .as_ref()
            .map(|t| tip_of(cx, &cx.linked.also(Name::Tag(&t.r#type, &t.tag))))
            .unwrap_or_default();
        let _ = write!(
            out,
            "<a class=\"r{class}\" href=\"{}\"{also}><i style=\"background: {}\"></i><span>{}</span>\
             <span class=\"h\">{}</span><span class=\"p\">{share}%</span></a>",
            narrowed(by, window).link(),
            colour(tag),
            esc(name),
            esc(&date::duration(*secs, cx.s))
        );
    }
    out.push_str("</div>");
    if months.len() > 1 {
        let tallest = card
            .months
            .iter()
            .map(|parts| parts.iter().sum::<i64>())
            .max()
            .unwrap_or(0)
            .max(1);
        out.push_str("<div class=\"months\">");
        for ((name, _), parts) in months.iter().zip(&card.months) {
            let sum: i64 = parts.iter().sum();
            let _ = write!(
                out,
                "<div class=\"m\"><b>{}</b>",
                match sum {
                    0 => String::new(),
                    _ => esc(&date::duration(sum, cx.s)),
                }
            );
            for ((lang, _), secs) in card.parts.iter().zip(parts).rev().filter(|(_, s)| **s > 0) {
                let _ = write!(
                    out,
                    "<i style=\"height: {:.1}px; background: {}\"></i>",
                    *secs as f64 / tallest as f64 * 120.0,
                    colour(lang)
                );
            }
            let _ = write!(out, "<span>{}</span></div>", esc(name));
        }
        out.push_str("</div>");
    }
    if card.split {
        let _ = write!(out, "<p class=\"note\">{}</p>", esc(w.split_each));
    }
    out.push_str("</div></div>");
    out
}

/// [`HUES`] by a language's words, in the order the whole record's languages take by
/// time read (`breakdown::language_order`), the same on every span.
fn hues<'a>(cx: &'a Ctx) -> impl Fn(&str) -> String + 'a {
    let order = breakdown::language_order(&every(cx), cx.linked, |at| {
        cx.stats.books[at].read_seconds(cx.from)
    });
    move |words: &str| {
        order
            .iter()
            .position(|held| held == words)
            .and_then(|n| HUES.get(n))
            .unwrap_or(&OTHER)
            .to_string()
    }
}

/// `days`' months, the record's from its first day to `cx.today` where `None`,
/// each named and as its days.
fn months(cx: &Ctx, days: Option<&RangeInclusive<i64>>) -> Vec<(String, RangeInclusive<i64>)> {
    let (first, last) = match days {
        Some(days) => (*days.start(), *days.end()),
        None => (cx.opened, cx.today),
    };
    let (mut y, mut m, _) = date::civil_from_days(first);
    let (ly, lm, _) = date::civil_from_days(last);
    let mut out = Vec::new();
    while (y, m) <= (ly, lm) && out.len() < 240 {
        let start = date::days_from_civil(y, m, 1);
        let end = date::days_from_civil(y, m, date::days_in_month(y, m));
        let name = cx.s.months_short[(m - 1).clamp(0, 11) as usize].to_string();
        out.push((name, start.max(first)..=end.min(last)));
        (y, m) = match m {
            12 => (y + 1, 1),
            _ => (y, m + 1),
        };
    }
    out
}
