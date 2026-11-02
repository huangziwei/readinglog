//! [`head`], [`listed`], [`chip`], [`renaming`], [`chosen`], [`sheet`], [`suggestions`] and
//! [`charts`]: a book's metadata on its head, the tags over Books, Books narrowed and picked,
//! and the charts.

use std::collections::{BTreeMap, HashMap};
use std::fmt::Write as _;
use std::ops::RangeInclusive;

use readinglog_native::date;
use readinglog_native::lang::counted;
use readinglog_native::view::{Shelf, Sort, Way, Window};
use serde::Serialize;

use super::words::Words;
use super::{Ctx, Route, esc, lang, language_of};
use crate::metadata::{self, By, Named, Patch, Tag, Values, names};

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
        by: Some(by),
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
        let joint = match book.author.contains('、') {
            true => "、",
            false => " & ",
        };
        let names: Vec<String> = values
            .authors
            .iter()
            .map(|name| {
                format!(
                    "<a href=\"{}\">{}</a>",
                    narrowed(By::Author(Some(name.clone())), None).link(),
                    esc(name)
                )
            })
            .collect();
        let _ = write!(
            out,
            "<p class=\"author\"{}>{}</p>",
            lang(&book.language, &book.author),
            names.join(joint)
        );
    }
    let publisher = values
        .publisher
        .as_ref()
        .map(|publisher| {
            format!(
                "<a href=\"{}\">{}</a>",
                narrowed(By::Publisher(Some(publisher.clone())), None).link(),
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

/// `named_type`: the `.name` field of `r#type`, renaming it on every book.
fn named_type(cx: &Ctx, r#type: &str) -> String {
    let w = Words::of(cx.lang);
    field(
        "type",
        r#type,
        &["type", r#type],
        &format!(" class=\"name\" data-tip=\"{}\"", esc(w.rename_every_tip)),
    )
}

/// `.types`: each type [`in_use`] beside the tags of it `values` holds, the type a field
/// renaming it on every book, each tag opening Books narrowed to it with × beside it, and a
/// `.more` field adding one; under them, `.fresh`, a type and its first tag.
fn types(cx: &Ctx, values: &Values) -> String {
    let w = Words::of(cx.lang);
    let (in_use, _) = in_use(cx);
    let mut out = String::from("<div class=\"types\">");
    for (r#type, _) in &in_use {
        let _ = write!(
            out,
            "<div class=\"kind\">{}<span class=\"held\">",
            named_type(cx, r#type)
        );
        for tag in values.tags.iter().filter(|t| t.r#type == *r#type) {
            let _ = write!(
                out,
                "<span class=\"tag\" data-tag=\"{1}\"><a href=\"{0}\">{1}</a><button class=\"off\" \
                 data-act=\"untag\" data-type=\"{2}\" data-tag=\"{1}\" data-tip=\"{3}\">×</button></span>",
                narrowed(By::Tag(r#type.clone(), Some(tag.tag.clone())), None).link(),
                esc(&tag.tag),
                esc(r#type),
                esc(w.remove_tip)
            );
        }
        let _ = write!(
            out,
            "<span class=\"more\"><input data-field=\"tag\" data-pick=\"tags\" placeholder=\"+\" \
             data-key=\"{}\" autocomplete=\"off\" spellcheck=\"false\"></span></span></div>",
            json(&["more", r#type])
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

/// `.types.all`: over Books, each type [`in_use`] with its tags and `not set`, each opening
/// Books narrowed to it by `to`, the one `by` names lit and going back; `editing`, each a
/// field renaming it on every book, × taking a tag off every book; `.edit` between the two.
pub(super) fn listed(
    cx: &Ctx,
    by: Option<&By>,
    editing: bool,
    to: impl Fn(Option<By>, bool) -> String,
) -> String {
    let w = Words::of(cx.lang);
    let (types, tags) = in_use(cx);
    if types.is_empty() {
        return String::new();
    }
    let lit = |r#type: &str, tag: Option<&str>| match by {
        Some(By::Tag(t, held)) if metadata::alike(t, r#type) => match (held, tag) {
            (Some(held), Some(tag)) => metadata::alike(held, tag),
            (None, None) => true,
            _ => false,
        },
        _ => false,
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
        if under.iter().all(|tag| tag.parse::<i64>().is_ok()) {
            under.sort_by_key(|tag| tag.parse::<i64>().unwrap_or(0));
        }
        let name = match editing {
            true => named_type(cx, r#type),
            false => format!("<span class=\"name\">{}</span>", esc(r#type)),
        };
        let _ = write!(out, "<div class=\"kind\">{name}<span class=\"held\">");
        for tag in under {
            if editing {
                let _ = write!(
                    out,
                    "<span class=\"tag\" data-tag=\"{0}\">{1}<button class=\"off\" data-act=\"untag-every\" \
                     data-type=\"{2}\" data-tag=\"{0}\" data-tip=\"{3}\">×</button></span>",
                    esc(tag),
                    field(
                        "tag",
                        tag,
                        &["tag", r#type, tag],
                        &format!(" data-tip=\"{}\"", esc(w.rename_every_tip))
                    ),
                    esc(r#type),
                    esc(w.remove_every_tip)
                );
                continue;
            }
            let (on, link) = match lit(r#type, Some(tag)) {
                true => (" on", to(None, false)),
                false => (
                    "",
                    to(Some(By::Tag(r#type.clone(), Some(tag.to_string()))), false),
                ),
            };
            let _ = write!(
                out,
                "<span class=\"tag{on}\"><a href=\"{link}\">{}</a></span>",
                esc(tag)
            );
        }
        if !editing {
            let (on, link) = match lit(r#type, None) {
                true => (" on", to(None, false)),
                false => ("", to(Some(By::Tag(r#type.clone(), None)), false)),
            };
            let _ = write!(
                out,
                "<span class=\"tag unset{on}\"><a href=\"{link}\">{}</a></span>",
                esc(w.not_set)
            );
        }
        out.push_str("</span></div>");
    }
    let _ = write!(
        out,
        "<a class=\"edit\" href=\"{}\">{}</a></div>",
        to(by.cloned(), !editing),
        esc(edit)
    );
    out
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

/// [`Types`] and [`Tags`] across `cx.metadata`, most held first.
fn in_use(cx: &Ctx) -> (Types, Tags) {
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
    (types, tags)
}

/// [`in_use`]'s types and tags as the lists a type or tag field offers, held once
/// beside the fields for `app.js` to move under the one focused; under a type of
/// [`names::TYPES`], `data-languages`, a tag's list offers [`languages`] too.
fn picks(cx: &Ctx) -> String {
    let (types, tags) = in_use(cx);
    let books = |n: usize| counted(cx.s.n_books, n as i64);
    let mut out = String::from("<div class=\"pick\" data-for=\"types\" hidden>");
    for (r#type, n) in &types {
        let _ = write!(
            out,
            "<div data-name=\"{}\" data-words=\"{}\"><span>{}</span><small>{}</small></div>",
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
            esc(&r#type.to_lowercase()),
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

/// `by`'s chip on Books, its × going back to `back`, and Rename… where `by`
/// names a value.
pub(super) fn chip(cx: &Ctx, by: &By, back: &str) -> String {
    let w = Words::of(cx.lang);
    let (what, value) = match by {
        By::Author(v) => (w.author_chip.to_string(), v.clone()),
        By::Publisher(v) => (w.publisher_field.to_string(), v.clone()),
        By::Tag(t, v) => (t.clone(), v.clone()),
    };
    let mut out = format!(
        "<span class=\"chip filter\"><small>{}</small>{}<a class=\"off\" href=\"{back}\">×</a></span>",
        esc(&what),
        esc(value.as_deref().unwrap_or(w.not_set))
    );
    if value.is_some() {
        let _ = write!(
            out,
            "<button class=\"chip act\" data-act=\"rename\">{}</button>",
            esc(w.rename)
        );
    }
    out
}

/// `.renaming`: the field renaming `by` on `count` books, hidden until Rename… opens it.
pub(super) fn renaming(cx: &Ctx, by: &By, count: usize) -> String {
    let w = Words::of(cx.lang);
    if matches!(
        by,
        By::Author(None) | By::Publisher(None) | By::Tag(_, None)
    ) {
        return String::new();
    }
    let value = match by {
        By::Author(v) | By::Publisher(v) | By::Tag(_, v) => v.clone(),
    }
    .unwrap_or_default();
    let ask = counted(w.rename_ask, count as i64).replace("{v}", &value);
    format!(
        "<div class=\"renaming adding\" hidden data-by=\"{}\"><label class=\"field\"><span>{}</span>\
         <input class=\"box\" value=\"{}\" autocomplete=\"off\" spellcheck=\"false\"></label>\
         <p class=\"acts\"><button data-act=\"cancel-rename\">{}</button>\
         <button class=\"save\" data-act=\"save-rename\">{}</button></p></div>",
        json(by),
        esc(&ask),
        esc(&value),
        esc(cx.s.cancel),
        esc(w.rename_do)
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

/// `Bar`: one value's bar, its name, time and books, and the Books it opens.
struct Bar {
    name: String,
    lang: Option<String>,
    secs: i64,
    books: usize,
    to: Route,
}

/// `days`' charts, the whole record's where `None`: authors, publishers, languages and each
/// type of tag, a value not set as its own bar, each bar opening Books narrowed to it under
/// `window`; empty where no book read sets any.
pub(super) fn charts(
    cx: &Ctx,
    days: Option<RangeInclusive<i64>>,
    window: Option<Window>,
) -> String {
    let w = Words::of(cx.lang);
    // `over`: each book's seconds over `days`, summed in one pass over `cx.stats.sittings`.
    let mut over: HashMap<usize, i64> = HashMap::new();
    if let Some(days) = &days {
        for sitting in &cx.stats.sittings {
            if let Some(book) = sitting.book.filter(|_| days.contains(&sitting.day)) {
                *over.entry(book).or_default() += sitting.seconds;
            }
        }
    }
    let read: Vec<(usize, i64, Values)> = (0..cx.stats.books.len())
        .map(|at| {
            let secs = match &days {
                Some(_) => over.get(&at).copied().unwrap_or(0),
                None => cx.stats.books[at].read_seconds(cx.from),
            };
            (at, secs, values(cx, at))
        })
        .filter(|(_, secs, _)| *secs > 0)
        .collect();
    if read.is_empty() {
        return String::new();
    }
    let total: i64 = read.iter().map(|(_, secs, _)| secs).sum();
    let to = |by: By| narrowed(by, window);
    let tally =
        |name: &str, lang: Option<String>, by: By, bars: &mut Vec<Bar>, secs: i64| match bars
            .iter_mut()
            .find(|b| b.name == name)
        {
            Some(bar) => {
                bar.secs += secs;
                bar.books += 1;
            }
            None => bars.push(Bar {
                name: name.to_string(),
                lang,
                secs,
                books: 1,
                to: to(by),
            }),
        };
    let mut authors = Vec::new();
    let mut publishers = Vec::new();
    let (mut authors_unset, mut publishers_unset) = (Vec::new(), Vec::new());
    for (at, secs, values) in &read {
        let book = &cx.stats.books[*at];
        for name in &values.authors {
            let named = language_of(&book.language, name);
            let lang = (!named.is_empty()).then_some(named);
            tally(
                name,
                lang,
                By::Author(Some(name.clone())),
                &mut authors,
                *secs,
            );
        }
        if values.authors.is_empty() {
            tally(w.not_set, None, By::Author(None), &mut authors_unset, *secs);
        }
        match &values.publisher {
            Some(p) => tally(
                p,
                None,
                By::Publisher(Some(p.clone())),
                &mut publishers,
                *secs,
            ),
            None => tally(
                w.not_set,
                None,
                By::Publisher(None),
                &mut publishers_unset,
                *secs,
            ),
        }
    }
    let mut out = String::new();
    let mut cards: Vec<String> = Vec::new();
    if !authors.is_empty() {
        cards.push(ranked(
            cx,
            "authors",
            w.authors_head,
            total,
            authors,
            authors_unset,
            None,
        ));
    }
    if !publishers.is_empty() {
        cards.push(ranked(
            cx,
            "publishers",
            w.publishers_head,
            total,
            publishers,
            publishers_unset,
            None,
        ));
    }
    let languages = languages_chart(cx, &read, days.as_ref(), window, total);
    if !languages.is_empty() {
        cards.push(languages);
    }
    let mut types: Vec<String> = Vec::new();
    for (_, _, values) in &read {
        for tag in &values.tags {
            if !types.contains(&tag.r#type) {
                types.push(tag.r#type.clone());
            }
        }
    }
    for r#type in types.into_iter().filter(|t| !names::is_type(t)) {
        let mut bars = Vec::new();
        let mut unset = Vec::new();
        let mut doubled = false;
        for (_, secs, values) in &read {
            let under: Vec<&Tag> = values.tags.iter().filter(|t| t.r#type == r#type).collect();
            doubled |= under.len() > 1;
            for tag in &under {
                tally(
                    &tag.tag,
                    None,
                    By::Tag(r#type.clone(), Some(tag.tag.clone())),
                    &mut bars,
                    *secs,
                );
            }
            if under.is_empty() {
                tally(
                    w.not_set,
                    None,
                    By::Tag(r#type.clone(), None),
                    &mut unset,
                    *secs,
                );
            }
        }
        let note = doubled.then(|| w.counts_each.replace("{t}", &date::duration(total, cx.s)));
        let numbers = bars.iter().all(|b| b.name.parse::<i64>().is_ok());
        cards.push(match numbers {
            true => columns(cx, &r#type, bars, unset, note),
            false => ranked(
                cx,
                &format!("type {}", r#type),
                &r#type,
                total,
                bars,
                unset,
                note,
            ),
        });
    }
    for pair in cards.chunks(2) {
        let _ = write!(out, "<section class=\"split\">{}</section>", pair.join(""));
    }
    out
}

/// `chart`'s card of ranked bars headed `head`: `bars` by time, the most read in the mark's
/// colour, the first [`TOP`] shown and the rest behind Show all, then `unset` and `note`;
/// `head` states `total` and the most.
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
        esc(head),
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
            "<a class=\"r{}\" href=\"{}\" data-tip=\"{} · {} · {}\"{}><span class=\"who\"{}>{}</span>\
             <span class=\"bar\"><i style=\"width: {width:.1}%; background: {colour}\"></i></span>\
             <b>{}</b><span class=\"n\">{}</span></a>",
            match (colour, past) {
                (UNSET, _) => " unset",
                (_, true) => " past",
                _ => "",
            },
            bar.to.link(),
            esc(&bar.name),
            esc(&time),
            esc(&books(bar.books)),
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

/// `bars` as columns under `head`, for a type whose tags are all numbers: one a
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
        esc(head),
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
                    "<a class=\"c\" href=\"{}\" data-tip=\"{} · {}\"><b>{}</b>\
                     <i style=\"height: {:.0}px; background: {colour}\"></i><span>{n}</span></a>",
                    bar.to.link(),
                    esc(&bar.name),
                    esc(&counted(cx.s.n_books, bar.books as i64)),
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

/// `spoken`: the languages `tags` holds, each tag under a type of
/// [`names::TYPES`], its words once.
fn spoken(tags: &[Tag]) -> Vec<&Tag> {
    let mut out: Vec<&Tag> = Vec::new();
    for tag in tags.iter().filter(|t| names::is_type(&t.r#type)) {
        if !out.iter().any(|t| t.tag == tag.tag) {
            out.push(tag);
        }
    }
    out
}

/// `part`: the `n`th of `of` even parts of `secs`, the first `secs % of` of them
/// a second longer, the parts summing to `secs`.
fn part(secs: i64, n: usize, of: usize) -> i64 {
    let of = of.max(1) as i64;
    secs / of + i64::from((n as i64) < secs % of)
}

/// `read`'s languages card under the type of [`names::TYPES`] most held: each language's
/// share of the time read, a book's split evenly among its languages, its legend, and
/// the months split by language where `days` holds more than one.
fn languages_chart(
    cx: &Ctx,
    read: &[(usize, i64, Values)],
    days: Option<&RangeInclusive<i64>>,
    window: Option<Window>,
    total: i64,
) -> String {
    let w = Words::of(cx.lang);
    let books: Vec<(usize, Vec<&Tag>)> = read
        .iter()
        .map(|(at, _, values)| (*at, spoken(&values.tags)))
        .collect();
    let (types, _) = in_use(cx);
    let Some((kind, _)) = types.iter().find(|(t, _)| names::is_type(t)) else {
        return String::new();
    };
    if books.iter().all(|(_, these)| these.is_empty()) {
        return String::new();
    }
    let hue = hues(cx);
    // `langs`: each language's tag, `None` for a book holding none, and its seconds.
    let mut langs: Vec<(Option<Tag>, i64)> = Vec::new();
    let mut split = false;
    for ((_, secs, _), (_, these)) in read.iter().zip(&books) {
        split |= these.len() > 1;
        let parts: Vec<(Option<&Tag>, i64)> = match these.is_empty() {
            true => vec![(None, *secs)],
            false => these
                .iter()
                .enumerate()
                .map(|(n, tag)| (Some(*tag), part(*secs, n, these.len())))
                .collect(),
        };
        for (tag, secs) in parts {
            let words = tag.map(|t| t.tag.as_str());
            match langs
                .iter_mut()
                .find(|(t, _)| t.as_ref().map(|t| t.tag.as_str()) == words)
            {
                Some((_, s)) => *s += secs,
                None => langs.push((tag.cloned(), secs)),
            }
        }
    }
    langs.sort_by(|a, b| (a.0.is_none(), b.1).cmp(&(b.0.is_none(), a.1)));
    let colour = |tag: &Option<Tag>| match tag {
        None => UNSET.to_string(),
        Some(tag) => hue(&tag.tag),
    };
    let top = langs
        .iter()
        .find_map(|(tag, _)| tag.as_ref().map(|t| t.tag.clone()))
        .unwrap_or_default();
    let mut out = format!(
        "<div><h2>{}<span class=\"said\">{} · {} {}</span></h2><div class=\"card\"><div class=\"share\">",
        esc(kind),
        esc(&date::duration(total, cx.s)),
        esc(cx.s.most),
        esc(&top)
    );
    let shares = shares(&langs.iter().map(|(_, s)| *s).collect::<Vec<i64>>());
    for (tag, secs) in &langs {
        let _ = write!(
            out,
            "<i style=\"flex: {secs}; background: {}\"></i>",
            colour(tag)
        );
    }
    out.push_str("</div><div class=\"legend\">");
    for ((tag, secs), share) in langs.iter().zip(&shares) {
        let (class, name, by) = match tag {
            Some(tag) => (
                "",
                tag.tag.as_str(),
                By::Tag(tag.r#type.clone(), Some(tag.tag.clone())),
            ),
            None => (" unset", w.not_set, By::Tag(kind.clone(), None)),
        };
        let _ = write!(
            out,
            "<a class=\"r{class}\" href=\"{}\"><i style=\"background: {}\"></i><span>{}</span>\
             <span class=\"h\">{}</span><span class=\"p\">{share}%</span></a>",
            narrowed(by, window).link(),
            colour(tag),
            esc(name),
            esc(&date::duration(*secs, cx.s))
        );
    }
    out.push_str("</div>");
    let months = months(cx, days);
    if months.len() > 1 {
        // `by_book`: each book's seconds in each of `months`, off `cx.stats.sittings`.
        let mut by_book: HashMap<usize, Vec<i64>> = HashMap::new();
        for sitting in &cx.stats.sittings {
            let Some(book) = sitting.book else { continue };
            let Some(month) = months
                .iter()
                .position(|(_, span)| span.contains(&sitting.day))
            else {
                continue;
            };
            by_book.entry(book).or_insert_with(|| vec![0; months.len()])[month] += sitting.seconds;
        }
        let split_by: Vec<Split> = months
            .into_iter()
            .enumerate()
            .map(|(month, (name, _))| {
                let parts = langs
                    .iter()
                    .map(|(lang, _)| {
                        let secs = books
                            .iter()
                            .map(|(at, these)| {
                                let secs = by_book.get(at).map_or(0, |secs| secs[month]);
                                match lang {
                                    None if these.is_empty() => secs,
                                    None => 0,
                                    Some(lang) => these
                                        .iter()
                                        .position(|t| t.tag == lang.tag)
                                        .map_or(0, |n| part(secs, n, these.len())),
                                }
                            })
                            .sum();
                        (lang.clone(), secs)
                    })
                    .collect();
                (name, parts)
            })
            .collect();
        let tallest = split_by
            .iter()
            .map(|(_, parts)| parts.iter().map(|(_, s)| s).sum::<i64>())
            .max()
            .unwrap_or(0)
            .max(1);
        out.push_str("<div class=\"months\">");
        for (name, parts) in &split_by {
            let sum: i64 = parts.iter().map(|(_, s)| s).sum();
            let _ = write!(
                out,
                "<div class=\"m\"><b>{}</b>",
                match sum {
                    0 => String::new(),
                    _ => esc(&date::duration(sum, cx.s)),
                }
            );
            for (lang, secs) in parts.iter().rev().filter(|(_, s)| *s > 0) {
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
    if split {
        let _ = write!(out, "<p class=\"note\">{}</p>", esc(w.split_each));
    }
    out.push_str("</div></div>");
    out
}

/// `Split`: a month's name and each language's time read in it.
type Split = (String, Vec<(Option<Tag>, i64)>);

/// [`HUES`] by a language's words, in the order the whole record's languages take
/// by time read, each book's split as [`languages_chart`] splits it, the same on
/// every span.
fn hues<'a>(cx: &'a Ctx) -> impl Fn(&str) -> String + 'a {
    let mut order: Vec<(String, i64)> = Vec::new();
    for at in 0..cx.stats.books.len() {
        let these = spoken(&cx.metadata[at].tags);
        let secs = cx.stats.books[at].read_seconds(cx.from);
        for (n, tag) in these.iter().enumerate() {
            let secs = part(secs, n, these.len());
            match order.iter_mut().find(|(words, _)| *words == tag.tag) {
                Some((_, s)) => *s += secs,
                None => order.push((tag.tag.clone(), secs)),
            }
        }
    }
    order.sort_by(|a, b| b.1.cmp(&a.1).then(a.0.cmp(&b.0)));
    move |words: &str| {
        order
            .iter()
            .position(|(held, _)| held == words)
            .and_then(|n| HUES.get(n))
            .unwrap_or(&OTHER)
            .to_string()
    }
}

/// `parts` as whole percents summing to 100, by the largest remainder.
fn shares(parts: &[i64]) -> Vec<i64> {
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

#[cfg(test)]
mod tests {
    use super::shares;

    #[test]
    fn shares_sum_to_a_hundred_by_the_largest_remainder() {
        assert_eq!(shares(&[232, 156, 65, 42, 61]), vec![42, 28, 12, 7, 11]);
        assert_eq!(shares(&[1, 1, 1]), vec![34, 33, 33]);
        assert_eq!(shares(&[0, 0]), vec![0, 0]);
    }
}
