//! [`head`], [`chip`], [`renaming`], [`chosen`], [`sheet`], [`suggestions`] and [`charts`]: a
//! book's metadata on its head, on Books narrowed and picked, and charted.

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

/// `HUES`: each language's hue, in the order the record's languages take by
/// time read; a fifth and on take `OTHER`.
const HUES: [&str; 4] = ["#0f86a3", "#b8781e", "#7d4fb0", "#c4456e"];
const OTHER: &str = "#9a958b";

/// `UNSET`, `BAR`, `MOST`: the bar of a value no book sets, a ranked bar, the most read one.
const UNSET: &str = "#ccc8bf";
const BAR: &str = "#79a260";
const MOST: &str = "#d23f6b";

/// What the book at `at` states.
pub(super) fn values(cx: &Ctx, at: usize) -> Values {
    Values::of(&cx.own[at].1, &cx.metadata[at])
}

/// The book at `at` as the commands name it.
pub(super) fn named(cx: &Ctx, at: usize) -> Named {
    let book = &cx.stats.books[at];
    Named {
        kindle: cx.serial(at).to_string(),
        key: crate::kindles::key_origin(&book.cde_key).1.to_string(),
        extent: crate::kindles::origin(book.extent).1,
    }
}

/// Books narrowed to `by`, under `window`.
pub(super) fn narrowed(by: By, window: Option<Window>) -> Route {
    Route::Books {
        shelf: Shelf::All,
        order: Sort::Recent,
        way: Way::Down,
        query: String::new(),
        window,
        by: Some(by),
    }
}

/// The language `code` names, in the interface's language; the code itself
/// where no name is listed.
fn language_name(cx: &Ctx, code: &str) -> String {
    names::name(code, cx.lang).unwrap_or(code).to_string()
}

/// `tags` grouped by type, each type and its tags in the order they stand.
fn grouped(tags: &[Tag]) -> Vec<(&str, Vec<&str>)> {
    let mut out: Vec<(&str, Vec<&str>)> = Vec::new();
    for tag in tags {
        match out.iter_mut().find(|(t, _)| *t == tag.r#type) {
            Some((_, under)) => under.push(&tag.tag),
            None => out.push((&tag.r#type, vec![&tag.tag])),
        }
    }
    out
}

/// `value` as JSON inside an attribute.
fn json(value: &impl Serialize) -> String {
    esc(&serde_json::to_string(value).unwrap_or_default())
}

/// `at`'s head under its title: its authors (each opening the books by them),
/// the facts line with Edit, its tags with + Tag, and the editors those open,
/// hidden until `app.js` shows them.
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
    let mut facts = Vec::new();
    if let Some(publisher) = &values.publisher {
        facts.push(format!(
            "<a href=\"{}\">{}</a>",
            narrowed(By::Publisher(Some(publisher.clone())), None).link(),
            esc(publisher)
        ));
    }
    if let Some(code) = &values.language {
        facts.push(format!(
            "<a href=\"{}\">{}</a>",
            narrowed(By::Language(Some(code.clone())), None).link(),
            esc(&language_name(cx, code))
        ));
    }
    let _ = write!(
        out,
        "<p class=\"facts\">{}<button class=\"edit\" data-act=\"edit-meta\">{}</button></p><div class=\"tags\">",
        facts.join("<span class=\"dot\">·</span>"),
        esc(w.edit)
    );
    for (r#type, tags) in grouped(&values.tags) {
        let _ = write!(
            out,
            "<a class=\"type\" href=\"{}\">{}</a>",
            narrowed(By::Type(r#type.to_string()), None).link(),
            esc(r#type)
        );
        for tag in tags {
            let _ = write!(
                out,
                "<span class=\"tag\"><a href=\"{}\">{}</a><button class=\"off\" data-act=\"untag\" \
                 data-type=\"{}\" data-tag=\"{}\" data-tip=\"{}\">×</button></span>",
                narrowed(By::Tag(r#type.to_string(), Some(tag.to_string())), None).link(),
                esc(tag),
                esc(r#type),
                esc(tag),
                esc(w.remove_tip)
            );
        }
    }
    let _ = write!(
        out,
        "<button class=\"add\" data-act=\"add-tag\">{}</button></div>",
        esc(w.add_tag)
    );
    out.push_str(&editor(cx, at, &values));
    out.push_str(&adder(cx, true));
    out.push_str("</div>");
    out
}

/// The editor of `at`'s title, authors, publisher and language, each field
/// holding what `values` states; under a title or authors `cx.metadata` sets,
/// the book's own words with Revert.
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
    let language = values
        .language
        .as_deref()
        .map(|code| language_name(cx, code))
        .unwrap_or_default();
    let mut out = String::from("<div class=\"metaedit\" hidden>");
    out.push_str(&field(w.title_field, "title", &book.title, " title", ""));
    if set.title.is_some() {
        out.push_str(&says(own_title, "title"));
    }
    out.push_str(&field(w.authors_field, "authors", &authors, "", ""));
    if set.authors.is_some() {
        out.push_str(&says(own_author, "authors"));
    }
    out.push_str("<div class=\"pair\">");
    out.push_str(&field(
        w.publisher_field,
        "publisher",
        values.publisher.as_deref().unwrap_or_default(),
        "",
        "",
    ));
    let code = values.language.clone().unwrap_or_default();
    let _ = write!(
        out,
        "<div class=\"field\"><span>{}</span><input class=\"box\" data-field=\"language\" value=\"{}\" \
         data-was=\"{}\" data-code=\"{}\" data-held=\"{}\" data-pick=\"languages\" autocomplete=\"off\" spellcheck=\"false\">{}</div>",
        esc(w.language_field),
        esc(&language),
        esc(&language),
        esc(&code),
        esc(&code),
        languages(cx)
    );
    let _ = write!(
        out,
        "</div><p class=\"acts\"><button data-act=\"cancel-meta\">{}</button>\
         <button class=\"save\" data-act=\"save-meta\">{}</button></p></div>",
        esc(cx.s.cancel),
        esc(w.save)
    );
    out
}

/// The list a language field offers: the languages `cx.metadata` sets first,
/// then every other [`names::LANGUAGES`] lists, each named in `cx.lang` and its own.
fn languages(cx: &Ctx) -> String {
    let w = Words::of(cx.lang);
    let mut used: Vec<(String, usize)> = Vec::new();
    for set in cx.metadata {
        if let Some(code) = &set.language {
            match used.iter_mut().find(|(c, _)| c == code) {
                Some((_, n)) => *n += 1,
                None => used.push((code.clone(), 1)),
            }
        }
    }
    used.sort_by(|a, b| b.1.cmp(&a.1).then(a.0.cmp(&b.0)));
    let option = |code: &str| {
        let name = language_name(cx, code);
        let own = names::own(code).unwrap_or(code);
        let shown = match own == name {
            true => name.clone(),
            false => format!("{own} · {name}"),
        };
        format!(
            "<div data-code=\"{}\" data-name=\"{}\" data-words=\"{}\"><span>{}</span><small>{}</small></div>",
            esc(code),
            esc(&name),
            esc(&format!("{} {} {}", own, name, code).to_lowercase()),
            esc(&shown),
            esc(code)
        )
    };
    let mut out = String::from("<div class=\"pick\" hidden>");
    if !used.is_empty() {
        let _ = write!(out, "<div class=\"head\">{}</div>", esc(w.in_use_head));
        for (code, _) in &used {
            out.push_str(&option(code));
        }
        let _ = write!(out, "<div class=\"head\">{}</div>", esc(w.others_head));
    }
    for (code, ..) in names::LANGUAGES {
        if !used.iter().any(|(c, _)| c == code) {
            out.push_str(&option(code));
        }
    }
    out.push_str("</div>");
    out
}

/// `Types`: each type in use and the books holding a tag of it.
type Types = Vec<(String, usize)>;

/// `Tags`: each tag in use, its type and the books holding it.
type Tags = Vec<(String, String, usize)>;

/// Every type in use and every tag under each, with the books holding them,
/// most held first.
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

/// The form adding a tag, its fields offering the types and tags in use; with
/// `alone`, hidden until + Tag opens it and with its own Cancel.
fn adder(cx: &Ctx, alone: bool) -> String {
    let w = Words::of(cx.lang);
    let (types, tags) = in_use(cx);
    let books = |n: usize| counted(cx.s.n_books, n as i64);
    let mut out = format!(
        "<div class=\"adding\"{}><div class=\"field\"><span>{}</span><input class=\"box\" \
         data-field=\"type\" data-pick=\"types\" autocomplete=\"off\" spellcheck=\"false\"><div class=\"pick\" hidden>",
        match alone {
            true => " hidden",
            false => "",
        },
        esc(w.type_field)
    );
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
    let _ = write!(
        out,
        "</div></div><div class=\"field\"><span>{}</span><input class=\"box\" data-field=\"tag\" \
         data-pick=\"tags\" autocomplete=\"off\" spellcheck=\"false\"><div class=\"pick\" hidden>",
        esc(w.tag_field)
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
    out.push_str("</div></div><p class=\"acts\">");
    if alone {
        let _ = write!(
            out,
            "<button data-act=\"cancel-tag\">{}</button>",
            esc(cx.s.cancel)
        );
    }
    let _ = write!(
        out,
        "<button class=\"save\" data-act=\"save-tag\">{}</button></p></div>",
        esc(w.add)
    );
    out
}

/// The chip naming the value Books is narrowed to, its × going back to the
/// books unnarrowed at `back`, and Rename… where the value is set.
pub(super) fn chip(cx: &Ctx, by: &By, back: &str) -> String {
    let w = Words::of(cx.lang);
    let (what, value) = match by {
        By::Author(v) => (w.author_chip.to_string(), v.clone()),
        By::Publisher(v) => (w.publisher_field.to_string(), v.clone()),
        By::Language(v) => (
            w.language_field.to_string(),
            v.as_deref().map(|code| language_name(cx, code)),
        ),
        By::Type(t) => (w.type_field.to_string(), Some(t.clone())),
        By::Tag(t, v) => (t.clone(), v.clone()),
    };
    let mut out = format!(
        "<span class=\"chip filter\"><small>{}</small>{}<a class=\"off\" href=\"{back}\">×</a></span>",
        esc(&what),
        esc(value.as_deref().unwrap_or(w.not_set))
    );
    if value.is_some() && !matches!(by, By::Language(_)) {
        let _ = write!(
            out,
            "<button class=\"chip act\" data-act=\"rename\">{}</button>",
            esc(w.rename)
        );
    }
    out
}

/// Rename…'s field over `count` books, hidden until Rename… opens it.
pub(super) fn renaming(cx: &Ctx, by: &By, count: usize) -> String {
    let w = Words::of(cx.lang);
    if matches!(
        by,
        By::Author(None) | By::Publisher(None) | By::Language(_) | By::Tag(_, None)
    ) {
        return String::new();
    }
    let value = match by {
        By::Author(v) | By::Publisher(v) | By::Tag(_, v) => v.clone(),
        By::Language(v) => v.clone(),
        By::Type(t) => Some(t.clone()),
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

/// The selection's bar over `total` books, hidden until a book is picked, in the shelf chips' place.
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

/// The bulk editor, hidden until Edit… opens it on the books picked; `app.js`
/// states each field's books.
pub(super) fn sheet(cx: &Ctx) -> String {
    let w = Words::of(cx.lang);
    let field = |name: &str, field: &str, back: &str, more: &str| {
        format!(
            "<div class=\"field\"><span>{}<button class=\"clear\" data-act=\"bulk-back\" \
             data-field=\"{field}\">{}</button></span><input class=\"box\" data-field=\"{field}\"{more} \
             autocomplete=\"off\" spellcheck=\"false\">{}</div>",
            esc(name),
            esc(back),
            match field {
                "language" => languages(cx),
                _ => String::new(),
            }
        )
    };
    format!(
        "<div class=\"sheet\" hidden data-head=\"{}\" data-leave=\"{}\" data-values=\"{}\" \
         data-none=\"{}\" data-some=\"{}\" data-apply=\"{}\"><h3></h3><p class=\"lede\">{}</p>{}{}{}\
         <div class=\"field\"><span>{}</span><div class=\"tags\"></div></div>{}\
         <p class=\"acts\"><button data-act=\"cancel-bulk\">{}</button>\
         <button class=\"save\" data-act=\"apply-bulk\"></button></p></div>",
        esc(w.edit_books),
        esc(w.leave),
        esc(w.leave_values),
        esc(w.leave_none),
        esc(w.of_some),
        esc(w.apply_to),
        esc(w.leave_note),
        field(w.authors_field, "authors", w.each_own, ""),
        field(w.publisher_field, "publisher", w.empty, ""),
        field(
            w.language_field,
            "language",
            w.empty,
            " data-pick=\"languages\" data-code=\"\""
        ),
        esc(w.tags_field),
        adder(cx, false),
        esc(cx.s.cancel)
    )
}

/// What a tile carries for the selection: the book as the commands name it,
/// and what it states.
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

/// The suggestions over the books `shown`: a language their characters settle
/// for books setting none, and authors written surname first turned round.
pub(super) fn suggestions(cx: &Ctx, shown: &[usize]) -> String {
    let w = Words::of(cx.lang);
    let mut by_language: BTreeMap<&str, Vec<usize>> = BTreeMap::new();
    let mut turned: Vec<(usize, String)> = Vec::new();
    for &at in shown {
        let book = &cx.stats.books[at];
        let set = &cx.metadata[at];
        if set.language.is_none()
            && let Some(code) = metadata::suggested_language(&book.title, &book.author)
        {
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
        let name = language_name(cx, code);
        let patch = Patch {
            language: Some(metadata::Change::To((*code).to_string())),
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

/// The charts of the books read over `days` (the whole record where none): authors, publishers,
/// languages and each type of tag, a value not set as its own bar, each bar opening Books
/// narrowed to it under `window`; empty where no book read sets any.
pub(super) fn charts(
    cx: &Ctx,
    days: Option<RangeInclusive<i64>>,
    window: Option<Window>,
) -> String {
    let w = Words::of(cx.lang);
    // `over`: each book's seconds over `days` as `Stats::book_seconds_over` sums them, in one pass.
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
    for r#type in types {
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
            false => ranked(cx, &r#type, total, bars, unset, note),
        });
    }
    for pair in cards.chunks(2) {
        let _ = write!(out, "<section class=\"split\">{}</section>", pair.join(""));
    }
    out
}

/// A card of ranked bars headed `head`: `bars` by time, the most read in the
/// mark's colour, and `unset` under them, then `note`; `head` states `total` and
/// the most.
fn ranked(
    cx: &Ctx,
    head: &str,
    total: i64,
    mut bars: Vec<Bar>,
    unset: Vec<Bar>,
    note: Option<String>,
) -> String {
    bars.sort_by(|a, b| b.secs.cmp(&a.secs).then(a.name.cmp(&b.name)));
    let most = bars.first().map(|b| b.secs).unwrap_or(0).max(1);
    let books = |n: usize| counted(cx.s.n_books, n as i64);
    let mut out = format!(
        "<div><h2>{}<span class=\"said\">{} · {} {}</span></h2><div class=\"card\"><div class=\"ranked\">",
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
        let width = bar.secs as f64 / most as f64 * 78.0;
        let time = date::duration(bar.secs, cx.s);
        let _ = write!(
            out,
            "<a class=\"r{}\" href=\"{}\" data-tip=\"{} · {} · {}\"><span class=\"who\"{}>{}</span>\
             <span class=\"bar\"><i style=\"width: {width:.1}%; background: {colour}\"></i></span>\
             <b>{}</b><span class=\"n\">{}</span></a>",
            match colour {
                UNSET => " unset",
                _ => "",
            },
            bar.to.link(),
            esc(&bar.name),
            esc(&time),
            esc(&books(bar.books)),
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
    if let Some(note) = note {
        let _ = write!(out, "<p class=\"note\">{}</p>", esc(&note));
    }
    out.push_str("</div></div>");
    out
}

/// A card of columns headed `head`, for a type whose tags are all numbers:
/// one column a number, in their order, every whole number between standing;
/// `unset` stated under them.
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

/// The languages card: each language's share of the time read, its legend,
/// and the months split by language where the span holds more than one.
fn languages_chart(
    cx: &Ctx,
    read: &[(usize, i64, Values)],
    days: Option<&RangeInclusive<i64>>,
    window: Option<Window>,
    total: i64,
) -> String {
    let w = Words::of(cx.lang);
    if read.iter().all(|(_, _, v)| v.language.is_none()) {
        return String::new();
    }
    let hue = hues(cx);
    let mut langs: Vec<(Option<String>, i64)> = Vec::new();
    for (_, secs, values) in read {
        match langs.iter_mut().find(|(code, _)| *code == values.language) {
            Some((_, s)) => *s += secs,
            None => langs.push((values.language.clone(), *secs)),
        }
    }
    langs.sort_by(|a, b| (a.0.is_none(), b.1).cmp(&(b.0.is_none(), a.1)));
    let colour = |code: &Option<String>| match code {
        None => UNSET.to_string(),
        Some(code) => hue(code),
    };
    let top = langs
        .iter()
        .find(|(code, _)| code.is_some())
        .and_then(|(code, _)| code.as_deref())
        .map(|code| language_name(cx, code))
        .unwrap_or_default();
    let mut out = format!(
        "<div><h2>{}<span class=\"said\">{} · {} {}</span></h2><div class=\"card\"><div class=\"share\">",
        esc(w.languages_head),
        esc(&date::duration(total, cx.s)),
        esc(cx.s.most),
        esc(&top)
    );
    let shares = shares(&langs.iter().map(|(_, s)| *s).collect::<Vec<i64>>());
    for (code, secs) in &langs {
        let _ = write!(
            out,
            "<i style=\"flex: {secs}; background: {}\"></i>",
            colour(code)
        );
    }
    out.push_str("</div><div class=\"legend\">");
    for ((code, secs), share) in langs.iter().zip(&shares) {
        let name = match code {
            Some(code) => {
                let name = language_name(cx, code);
                match names::own(code) {
                    Some(own) if own != name => format!("{own} · {name}"),
                    _ => name,
                }
            }
            None => w.not_set.to_string(),
        };
        let _ = write!(
            out,
            "<a class=\"r{}\" href=\"{}\"><i style=\"background: {}\"></i><span>{}</span>\
             <span class=\"h\">{}</span><span class=\"p\">{share}%</span></a>",
            match code {
                None => " unset",
                Some(_) => "",
            },
            narrowed(By::Language(code.clone()), window).link(),
            colour(code),
            esc(&name),
            esc(&date::duration(*secs, cx.s))
        );
    }
    out.push_str("</div>");
    let months = months(cx, days);
    if months.len() > 1 {
        // `by_book`: each book's seconds in each month as `Stats::book_seconds_over` sums them.
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
        let split: Vec<Split> = months
            .into_iter()
            .enumerate()
            .map(|(month, (name, _))| {
                let parts = langs
                    .iter()
                    .map(|(code, _)| {
                        let secs = read
                            .iter()
                            .filter(|(_, _, v)| v.language == *code)
                            .map(|(at, ..)| by_book.get(at).map_or(0, |secs| secs[month]))
                            .sum();
                        (code.clone(), secs)
                    })
                    .collect();
                (name, parts)
            })
            .collect();
        let tallest = split
            .iter()
            .map(|(_, parts)| parts.iter().map(|(_, s)| s).sum::<i64>())
            .max()
            .unwrap_or(0)
            .max(1);
        out.push_str("<div class=\"months\">");
        for (name, parts) in &split {
            let sum: i64 = parts.iter().map(|(_, s)| s).sum();
            let _ = write!(
                out,
                "<div class=\"m\"><b>{}</b>",
                match sum {
                    0 => String::new(),
                    _ => esc(&date::duration(sum, cx.s)),
                }
            );
            for (code, secs) in parts.iter().rev().filter(|(_, s)| *s > 0) {
                let _ = write!(
                    out,
                    "<i style=\"height: {:.1}px; background: {}\"></i>",
                    *secs as f64 / tallest as f64 * 120.0,
                    colour(code)
                );
            }
            let _ = write!(out, "<span>{}</span></div>", esc(name));
        }
        out.push_str("</div>");
    }
    out.push_str("</div></div>");
    out
}

/// `Split`: a month's name and each language's time read in it.
type Split = (String, Vec<(Option<String>, i64)>);

/// Each language's hue, in the order the whole record's languages take by time
/// read, the same on every span.
fn hues<'a>(cx: &'a Ctx) -> impl Fn(&str) -> String + 'a {
    let mut order: Vec<(String, i64)> = Vec::new();
    for at in 0..cx.stats.books.len() {
        if let Some(code) = &cx.metadata[at].language {
            let secs = cx.stats.books[at].read_seconds(cx.from);
            match order.iter_mut().find(|(c, _)| c == code) {
                Some((_, s)) => *s += secs,
                None => order.push((code.clone(), secs)),
            }
        }
    }
    order.sort_by(|a, b| b.1.cmp(&a.1).then(a.0.cmp(&b.0)));
    move |code: &str| {
        order
            .iter()
            .position(|(c, _)| c == code)
            .and_then(|n| HUES.get(n))
            .unwrap_or(&OTHER)
            .to_string()
    }
}

/// Whole percents of `parts` summing to 100, by the largest remainder.
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

/// The months `days` spans, the record's from its first day to today where
/// none, each named and as its days.
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
