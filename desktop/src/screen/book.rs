//! One book: its head; the Kindle's part, the reading picked as the Graphs tab
//! draws it and its figures as a board; and its journal, its highlights and
//! notes in either order, narrowed by a search.

use std::fmt::Write as _;

use readinglog_native::clippings::Kind;
use readinglog_native::date;
use readinglog_native::highlights::Highlight;
use readinglog_native::ui::paint;
use readinglog_native::view::Way;
use readinglog_native::view::book as book_view;
use readinglog_native::view::book_graphs;
use readinglog_native::view::journal as journal_view;

use super::chart::{self, BookStrip};
use super::look::{Look, hex};
use super::{Ctx, Route, Words, esc, key_of, lang};
use crate::edits::HighlightId;
use crate::notes::Laid;

/// The journal's column beside the Kindle's part.
const JOURNAL_COLUMN: f32 = 640.0;

/// The gap between the Kindle's part and [`JOURNAL_COLUMN`].
const BESIDE_GAP: f32 = 40.0;

/// The narrowest a chart of the Kindle's part is drawn beside the journal.
const BESIDE_CHART: f32 = 560.0;

/// The width the Kindle's part draws its charts at beside the journal, where
/// `width`, a whole card's chart, leaves it [`BESIDE_CHART`] at the least.
fn beside(width: f32) -> Option<f32> {
    Some(width - JOURNAL_COLUMN - BESIDE_GAP).filter(|chart| *chart >= BESIDE_CHART)
}

pub(super) fn page(cx: &Ctx, at: usize, reading: Option<usize>, way: Way, query: &str) -> String {
    let s = cx.s;
    let book = &cx.stats.books[at];
    let tone = cx.tones[at];
    let percent = book.bar_percent().max(0);
    let standing = match (book.is_finished(), book.has_percent()) {
        (true, _) => s.shelf_finished.to_string(),
        (false, true) => s.percent_plain.replace("{d}", &percent.to_string()),
        (false, false) => String::new(),
    };
    let listed = journal_view::listing(
        cx.stats,
        at,
        book,
        way,
        (!query.is_empty()).then_some(query),
    );
    let charted = beside(cx.width);
    // `jumps` take the bar where the two parts stand one over the other.
    let jumps = match charted {
        Some(_) => String::new(),
        None => format!(
            "<span class=\"chips\"><button class=\"chip\" data-act=\"jump\" data-to=\"kindles\">{}</button>\
             <button class=\"chip\" data-act=\"jump\" data-to=\"journal\">{} {}</button></span>",
            esc(s.graphs_tab),
            esc(s.journal_tab),
            listed.len()
        ),
    };
    let mut out = format!(
        "<div class=\"slim\" hidden><b{}>{}</b>{jumps}</div>\
         <div class=\"hero\"><div class=\"cover\">{}</div><div class=\"words\"><h1{}>{}</h1><p class=\"author\"{}>{}</p>\
         <div class=\"meter\"><div class=\"track\"><i style=\"width: {percent}%; background: {}\"></i></div><b>{}</b></div>\
         <dl class=\"figures\">",
        lang(&book.language, &book.title),
        esc(&book.title),
        cx.cover(at),
        lang(&book.language, &book.title),
        esc(&book.title),
        lang(&book.language, &book.author),
        esc(&book.author),
        hex(tone),
        esc(&standing)
    );
    // `s.left` stands only on a book short of `BookStat::is_finished`.
    let stated = book_view::headline(book, cx.from, s);
    let told = stated
        .iter()
        .filter(|(value, _)| value != book_view::DASH)
        .filter(|(_, name)| !(book.is_finished() && *name == s.left));
    for (value, name) in told {
        let _ = write!(
            out,
            "<div><dt>{}</dt><dd>{}</dd></div>",
            esc(name),
            esc(value)
        );
    }
    out.push_str("</dl></div></div>");
    match charted {
        Some(_) => {
            let _ = write!(
                out,
                "<div class=\"parts beside\" style=\"--journal: {JOURNAL_COLUMN}px; --beside: {BESIDE_GAP}px\">"
            );
        }
        None => out.push_str("<div class=\"parts\">"),
    }
    out.push_str("<div class=\"part\" id=\"kindles\">");
    out.push_str(&kindles(
        cx,
        at,
        reading,
        way,
        query,
        charted.unwrap_or(cx.width),
    ));
    out.push_str("</div><div class=\"part\">");
    out.push_str(&journal(cx, at, reading, way, query, &listed));
    out.push_str("</div></div>");
    out
}

/// The Kindle's part, its charts `width` wide: every reading as a chip where
/// there are several, The Days and Where You Sat in one frame on one day axis,
/// The Clock, and the reading's figures as a board.
fn kindles(
    cx: &Ctx,
    at: usize,
    reading: Option<usize>,
    way: Way,
    query: &str,
    width: f32,
) -> String {
    let s = cx.s;
    let book = &cx.stats.books[at];
    let tone = cx.tones[at];
    let mut out = String::new();
    let reads = cx.stats.book_readings(at);
    let picked = reads
        .len()
        .checked_sub(1)
        .map(|last| reading.unwrap_or(last).min(last));
    if reads.len() > 1 {
        out.push_str(&readings(cx, at, &reads, picked.unwrap_or(0), way, query));
    }
    if let Some(on) = picked {
        let strip = BookStrip::of(cx.stats, at, reads[on], cx.week, s, width);
        let whole = book_graphs::whole(on, reads.len(), book.is_finished());
        let _ = write!(
            out,
            "<section><h2>{}<span class=\"said\">{} · {}</span></h2><div class=\"card\">{}\
             <h3 class=\"sub\">{}</h3>{}</div></section>",
            esc(s.the_days),
            esc(&date::duration(
                cx.stats.book_seconds_over(at, strip.from..=strip.to),
                s
            )),
            esc(&book_graphs::spanned(strip.from, strip.to, whole, s)),
            chart::book_days(&strip, s, &cx.look, tone),
            esc(s.where_you_sat),
            chart::book_sat(cx.stats, at, &strip, s, &cx.look, tone)
        );
    }
    let (from, to) = picked.map_or((book.first_day, book.last_day), |on| reads[on]);
    let hours = cx.stats.book_hours(at, from..=to);
    let fold = cx.stats.fold(hours.to_vec(), hours.iter().sum());
    let names: Vec<String> = (0..24).map(|h| format!("{h:02}")).collect();
    let toned = Look {
        bar: tone,
        ..cx.look
    };
    // `fold.busiest` names an hour only where the reading holds time in one.
    if fold.busiest.is_some() {
        let _ = write!(
            out,
            "<section><h2>{}<span class=\"said\">{}</span></h2><div class=\"card\">{}</div></section>",
            esc(s.the_clock),
            esc(&chart::stated(
                &fold,
                &names,
                date::duration(fold.each, s),
                s
            )),
            chart::fold(&fold, &names, book_graphs::HOURS_NAMED, s, &toned, width)
        );
    }
    let _ = write!(
        out,
        "<section><h2>{}</h2><div class=\"card board\">",
        esc(s.the_reading)
    );
    let mut rows = book_view::reading_rows(book, cx.from, s);
    rows.push((s.on_the_kindle, book_view::where_note(book, s)));
    for (name, value) in rows.iter().filter(|(_, value)| value != book_view::DASH) {
        let _ = write!(
            out,
            "<div><dd>{}</dd><dt>{}</dt></div>",
            esc(value),
            esc(name)
        );
    }
    out.push_str("</div></section>");
    out
}

/// Every reading the book has been given, each its span and the time it
/// took, the one `picked` lit and each a link onto its own graphs.
fn readings(
    cx: &Ctx,
    at: usize,
    reads: &[(i64, i64)],
    picked: usize,
    way: Way,
    query: &str,
) -> String {
    let s = cx.s;
    let book = &cx.stats.books[at];
    let mut out = format!(
        "<section><h2>{}<span class=\"said\">{}</span></h2><div class=\"chips\">",
        esc(s.read_through_band),
        reads.len()
    );
    for (on, (from, to)) in reads.iter().enumerate() {
        let whole = book_graphs::whole(on, reads.len(), book.is_finished());
        let to_route = Route::Book {
            key: key_of(book),
            reading: Some(on),
            way,
            query: query.to_string(),
        };
        let said = format!(
            "{} · {}",
            date::duration(cx.stats.book_seconds_over(at, *from..=*to), s),
            book_graphs::spanned(*from, *to, whole, s)
        );
        super::chip(&mut out, &to_route.link(), &said, on == picked);
    }
    out.push_str("</div></section>");
    out
}

/// A section's heading: its `name`, `count` beside it, and `acts` at the
/// line's other end.
fn heading(name: &str, count: usize, acts: &str) -> String {
    format!(
        "<h2><span>{}<span class=\"count\">{count}</span></span><span class=\"said\">{acts}</span></h2>",
        esc(name)
    )
}

/// The book's highlights and notes as its Journal tab lists them through
/// `journal_view::listing`, newest first or turned over and narrowed by `query`,
/// under a heading carrying the search, and the field a note is written in.
fn journal(
    cx: &Ctx,
    at: usize,
    reading: Option<usize>,
    way: Way,
    query: &str,
    listed: &[journal_view::Row],
) -> String {
    let s = cx.s;
    let w = Words::of(cx.lang);
    let book = &cx.stats.books[at];
    let flipped = Route::Book {
        key: key_of(book),
        reading,
        way: way.flipped(),
        query: query.to_string(),
    };
    let acts = match cx.stats.highlights_held(at) {
        0 => String::new(),
        _ => format!(
            "<input class=\"search\" type=\"search\" data-search=\"book\" value=\"{}\" placeholder=\"{}\">\
             <a class=\"chip\" href=\"{}\">{}</a>",
            esc(query),
            esc(s.search_hint_journal),
            flipped.link(),
            way.caret()
        ),
    };
    let (key, extent) = own(book);
    let mut out = format!(
        "<section id=\"journal\">{}<div class=\"composer\" data-kindle=\"{}\" \
         data-key=\"{}\" data-extent=\"{extent}\"><textarea data-part=\"note\" placeholder=\"{}\" rows=\"1\"></textarea>\
         <p class=\"acts\"><button data-act=\"cancel-note\">{}</button>\
         <button class=\"save\" data-act=\"write-note\">{}</button></p></div><div class=\"highlights\">",
        heading(s.journal_tab, listed.len(), &acts),
        esc(cx.serial(at)),
        esc(&key),
        esc(w.write_note),
        esc(s.cancel),
        esc(w.save)
    );
    if listed.is_empty() && cx.stats.highlights_held(at) > 0 {
        let _ = write!(out, "<p class=\"lede\">{}</p>", esc(s.no_highlights_here));
    }
    for row in listed {
        let notes: Vec<&Highlight> = row.notes.iter().chain(&row.written).collect();
        out.push_str(&highlight(cx, at, &row.highlight, &notes, query));
    }
    out.push_str("</div></section>");
    out
}

/// The book's key and extent as its own Kindle states them.
fn own(book: &readinglog_native::stats::BookStat) -> (String, i64) {
    (
        crate::kindles::key_origin(&book.cde_key).1.to_string(),
        crate::kindles::origin(book.extent).1,
    )
}

/// One row of the book at `at`, as the Kindle's highlights row sets it: the
/// passage behind a rule in `paint::highlight_colour`, the Kindle's note under
/// it, then the notes written here, each with its day; a note alone, unruled.
pub(super) fn highlight(
    cx: &Ctx,
    at: usize,
    highlight: &Highlight,
    notes: &[&Highlight],
    needle: &str,
) -> String {
    if highlight.kind == Kind::Note {
        return alone(cx, at, highlight, needle);
    }
    let book = &cx.stats.books[at];
    let w = Words::of(cx.lang);
    let serial = cx.serial(at);
    let (kindles, mine) = ordered(cx, at, notes);
    let edited = std::iter::once(highlight)
        .chain(kindles.iter().copied())
        .any(|h| {
            cx.corrected
                .contains(&(serial.to_string(), HighlightId::of(h)))
        });
    let mut out = format!(
        "<div class=\"highlight\" data-kindle=\"{}\"{}{}{}>\
         <blockquote style=\"border-color: {}\"{}>{}</blockquote>",
        esc(serial),
        named("", highlight),
        kindles
            .first()
            .map(|n| named("note-", n))
            .unwrap_or_default(),
        if edited { " data-edited" } else { "" },
        hex(paint::highlight_colour(&highlight.colour, true)),
        lang(&book.language, &highlight.body),
        washed(&highlight.body, needle)
    );
    // `lang` reads a note's language off its words, which name no book's.
    for note in &kindles {
        let _ = write!(
            out,
            "<p class=\"note\"{}>{}</p>",
            lang("", &note.body),
            washed(&note.body, needle)
        );
        if !mine.is_empty() {
            let _ = write!(out, "<p class=\"stamp\">{}</p>", esc(&made(cx, note)));
        }
    }
    for note in &mine {
        let written = cx.laid(at, note).map_or("", |l| l.written.as_str());
        let _ = write!(
            out,
            "<div class=\"dnote\" data-written=\"{}\">{}\
             <p class=\"stamp\">{}<button class=\"edit\" data-act=\"edit-note\">{}</button></p></div>",
            esc(written),
            written_note(note, needle),
            esc(&made(cx, note)),
            esc(w.edit)
        );
    }
    let _ = write!(
        out,
        "<p class=\"where\"><span>{}</span><span class=\"when\">\
         <button class=\"edit\" data-act=\"add-note\">{}</button>\
         <button class=\"edit\" data-act=\"edit\">{}</button>{}</span></p></div>",
        esc(&journal_view::label(
            highlight,
            crate::kindles::origin(book.extent).1,
            cx.s
        )),
        esc(w.add_note),
        esc(w.edit),
        esc(&made(cx, highlight))
    );
    out
}

/// `notes`, a row's in the book at `at`: the Kindle's in the order given, then
/// those written here in the order they were written.
pub(super) fn ordered<'a>(
    cx: &Ctx,
    at: usize,
    notes: &[&'a Highlight],
) -> (Vec<&'a Highlight>, Vec<&'a Highlight>) {
    let (kindles, mut mine): (Vec<&Highlight>, Vec<&Highlight>) =
        notes.iter().partition(|n| cx.laid(at, n).is_none());
    mine.sort_by(|a, b| a.at.cmp(&b.at));
    (kindles, mine)
}

/// A note standing as a row of its own, set as a note with no rule: under the
/// passage it was written on where that passage is gone, its place the
/// Kindle's label where it has one, and none for a note on the book alone.
fn alone(cx: &Ctx, at: usize, note: &Highlight, needle: &str) -> String {
    let book = &cx.stats.books[at];
    let w = Words::of(cx.lang);
    let serial = cx.serial(at);
    let laid: Option<&Laid> = cx.laid(at, note);
    let place = match (laid, note.location >= 0) {
        (Some(_), false) => String::new(),
        _ => journal_view::label(note, crate::kindles::origin(book.extent).1, cx.s),
    };
    let edited = cx
        .corrected
        .contains(&(serial.to_string(), HighlightId::of(note)));
    let mut out = match laid {
        Some(laid) => format!(
            "<div class=\"highlight alone\" data-kindle=\"{}\" data-written=\"{}\">",
            esc(serial),
            esc(&laid.written)
        ),
        None => format!(
            "<div class=\"highlight alone\" data-kindle=\"{}\"{}{}>",
            esc(serial),
            named("note-", note),
            if edited { " data-edited" } else { "" }
        ),
    };
    if let Some(words) = laid.and_then(|l| l.gone.as_deref()) {
        let _ = write!(
            out,
            "<blockquote style=\"border-color: {}\"{}>{}</blockquote>",
            hex(paint::highlight_colour("", true)),
            lang(&book.language, words),
            washed(words, needle)
        );
    }
    let (act, words) = match laid {
        Some(_) => ("edit-note", written_note(note, needle)),
        None => (
            "edit",
            format!(
                "<p class=\"note\"{}>{}</p>",
                lang("", &note.body),
                washed(&note.body, needle)
            ),
        ),
    };
    let _ = write!(
        out,
        "{words}<p class=\"where\"><span>{}</span><span class=\"when\">\
         <button class=\"edit\" data-act=\"{act}\">{}</button>{}</span></p></div>",
        esc(&place),
        esc(w.edit),
        esc(&made(cx, note))
    );
    out
}

/// A note written here: its Markdown drawn, each run of `needle` washed in its
/// words, and the Markdown itself in `data-text`, what Edit opens on.
fn written_note(note: &Highlight, needle: &str) -> String {
    format!(
        "<div class=\"note md\"{} data-text=\"{}\">{}</div>",
        lang("", &note.body),
        esc(&note.body),
        ruled(&washed_html(&crate::notes::html(&note.body), needle))
    )
}

/// `html` with each run of `needle` in its text washed as [`washed`] washes one,
/// every tag as it stands.
fn washed_html(html: &str, needle: &str) -> String {
    if needle.is_empty() {
        return html.to_string();
    }
    let mut out = String::with_capacity(html.len());
    let mut rest = html;
    while let Some(opens) = rest.find('<') {
        out.push_str(&washed(&unescaped(&rest[..opens]), needle));
        let closes = rest[opens..]
            .find('>')
            .map_or(rest.len(), |at| opens + at + 1);
        out.push_str(&rest[opens..closes]);
        rest = &rest[closes..];
    }
    out.push_str(&washed(&unescaped(rest), needle));
    out
}

/// What [`esc`] and the Markdown writer escape, written back as the characters.
fn unescaped(text: &str) -> String {
    text.replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&quot;", "\"")
        .replace("&#39;", "'")
        .replace("&amp;", "&")
}

/// `html` with each `<blockquote>` ruled in the colour a passage naming none
/// takes, and a quoted line opening on `— ` after a hard break set as the
/// quotation's `.place`.
pub fn ruled(html: &str) -> String {
    let ruled = html.replace(
        "<blockquote>",
        &format!(
            "<blockquote style=\"border-color: {}\">",
            hex(paint::highlight_colour("", true))
        ),
    );
    let mut out = String::with_capacity(ruled.len());
    let mut rest = ruled.as_str();
    let (broken, opens) = ("<br />\n", "<br />\n— ");
    while let Some(at) = rest.find(opens) {
        let (head, tail) = rest.split_at(at);
        let ends = tail.find("</p>").unwrap_or(tail.len());
        let _ = write!(
            out,
            "{head}<span class=\"place\">{}</span>",
            &tail[broken.len()..ends]
        );
        rest = &tail[ends..];
    }
    out.push_str(rest);
    out
}

/// The day `highlight` was made, as the Kindle's `year_day` states it.
fn made(cx: &Ctx, highlight: &Highlight) -> String {
    highlight
        .day()
        .map(|day| date::year_day(day, cx.s))
        .unwrap_or_default()
}

/// `highlight`'s `HighlightId` as data attributes, each name opening `prefix`.
fn named(prefix: &str, highlight: &Highlight) -> String {
    let id = HighlightId::of(highlight);
    format!(
        " data-{prefix}at=\"{}\" data-{prefix}kind=\"{}\" data-{prefix}title=\"{}\" \
         data-{prefix}location=\"{}\" data-{prefix}start=\"{}\"",
        esc(&id.at),
        esc(&id.kind),
        esc(&id.title),
        id.location,
        id.start
    )
}

/// `text` escaped, each run of `needle` `journal_view::runs_in` finds in a
/// `<mark>`.
pub(super) fn washed(text: &str, needle: &str) -> String {
    let mut out = String::new();
    let mut from = 0;
    for (opens, closes) in journal_view::runs_in(text, needle) {
        let _ = write!(
            out,
            "{}<mark>{}</mark>",
            esc(&text[from..opens]),
            esc(&text[opens..closes])
        );
        from = closes;
    }
    out.push_str(&esc(&text[from..]));
    out
}
