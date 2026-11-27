//! A book read on paper, as the window writes it: Books' button and the form adding one
//! ([`adding`]); on its own page the form logging a sitting and the sittings logged ([`panel`]),
//! and in its editor its pages, its cover (on the page's own, [`jacket`]) and the way to delete
//! it ([`editing`], [`deleting`]). [`refusal`] states why a command wrote nothing.

use std::fmt::Write as _;

use readinglog_native::date;
use readinglog_native::paper::{PaperSitting, Refusal};

use super::words::Words;
use super::{Ctx, esc};

/// `adding`: Books' button adding a paper book, and the form it opens, hidden until `app.js` shows
/// it: its cover, title, authors, pages, and the Kindle whose record it goes into where the folder
/// holds several. Its language is a tag set on its page, as any book's is. In a folder holding
/// none, the button says `no_kindle` and opens nothing.
pub(super) fn adding(cx: &Ctx) -> (String, String) {
    let s = cx.s;
    let w = Words::of(cx.lang);
    let held: Vec<(&str, &str)> = cx
        .serials
        .iter()
        .zip(cx.labels)
        .filter(|(serial, _)| !serial.is_empty())
        .map(|(serial, label)| (serial.as_str(), label.as_str()))
        .collect();
    if held.is_empty() {
        let button = format!(
            "<button type=\"button\" class=\"chip\" data-act=\"no-kindle\" data-said=\"{}\">{}</button>",
            esc(w.no_kindle),
            esc(w.add_paper)
        );
        return (button, String::new());
    }
    let button = format!(
        "<button type=\"button\" class=\"chip\" data-act=\"add-paper\">{}</button>",
        esc(w.add_paper)
    );
    // `well`: the cover, a picture chosen in its dialog or dropped on it, beside the fields;
    // `app.js` puts the picture's path in `cover` and shows it as the book will.
    let mut form = format!(
        "<div class=\"card paperadd\" data-paper-add hidden><h3>{}</h3><div class=\"entry\">\
         <div class=\"jacket\"><button type=\"button\" class=\"well\" data-act=\"pick-cover\" data-cover-drop>\
         <span>{}</span><img alt=\"\" hidden></button>\
         <button type=\"button\" class=\"chip\" data-act=\"unpick-cover\" hidden>{}</button>\
         <input type=\"hidden\" name=\"cover\" value=\"\"></div><div class=\"named\">\
         <label class=\"field\"><span>{}</span><input class=\"box title\" name=\"title\"></label>\
         <label class=\"field\"><span>{}</span><input class=\"box\" name=\"author\"></label>\
         <div class=\"fields\"><label class=\"field\"><span>{}</span>\
         <input class=\"box\" type=\"number\" min=\"1\" step=\"1\" name=\"pages\"></label>",
        esc(w.paper_heading),
        esc(w.drop_cover),
        esc(w.remove_cover),
        esc(w.title_field),
        esc(w.authors_field),
        esc(w.pages_field)
    );
    match held.as_slice() {
        [(serial, _)] => {
            let _ = write!(
                form,
                "<input type=\"hidden\" name=\"kindle\" value=\"{}\">",
                esc(serial)
            );
        }
        many => {
            let _ = write!(
                form,
                "<label class=\"field\"><span>{}</span><select class=\"box\" name=\"kindle\">",
                esc(w.kindle_field)
            );
            for (serial, label) in many {
                let _ = write!(
                    form,
                    "<option value=\"{}\">{}</option>",
                    esc(serial),
                    esc(label)
                );
            }
            form.push_str("</select></label>");
        }
    }
    let _ = write!(
        form,
        "</div></div></div><p class=\"acts\"><button type=\"button\" data-act=\"cancel-paper\">{}</button>\
         <button type=\"button\" class=\"save\" data-act=\"save-paper\">{}</button></p></div>",
        esc(s.cancel),
        esc(w.add)
    );
    (button, form)
}

/// `minute`: the `HH:MM` of `stamp`, a `YYYY-MM-DDTHH:MM:SS` instant.
fn minute(stamp: &str) -> &str {
    stamp.get(11..16).unwrap_or_default()
}

/// `CLOCK`: a time field: a minute typed as `HH:MM` on a 24-hour clock, as the window tells every
/// time. `type="time"` would draw it the locale's way, with AM and PM in English. `app.js` reads
/// what is typed into `HH:MM` and steps it with the arrow keys (`[data-clock]`).
const CLOCK: &str = "class=\"box\" type=\"text\" inputmode=\"numeric\" maxlength=\"5\" placeholder=\"--:--\" data-clock";

/// `fields`: a sitting's five fields, each holding what `read` states, `day` its day where it states
/// none, the two times [`CLOCK`] fields, and `max` the book's pages; the form `app.js` reads
/// `log_sitting`'s and `edit_sitting`'s `read` off.
fn fields(w: &Words, read: Option<&PaperSitting>, day: &str, from_page: i64, max: i64) -> String {
    let (day, from, to, opened, closed) = match read {
        Some(r) => (
            date::day_of(&r.from).to_string(),
            minute(&r.from).to_string(),
            minute(&r.to).to_string(),
            r.from_page.to_string(),
            r.to_page.to_string(),
        ),
        None => (
            day.to_string(),
            String::new(),
            String::new(),
            from_page.to_string(),
            String::new(),
        ),
    };
    format!(
        "<div class=\"fields\">\
         <label class=\"field\"><span>{}</span><input class=\"box\" type=\"date\" name=\"day\" value=\"{}\"></label>\
         <label class=\"field\"><span>{}</span><input {CLOCK} name=\"from\" value=\"{}\"></label>\
         <label class=\"field\"><span>{}</span><input {CLOCK} name=\"to\" value=\"{}\"></label>\
         <label class=\"field\"><span>{}</span><input class=\"box\" type=\"number\" min=\"0\" max=\"{max}\" step=\"1\" name=\"from_page\" value=\"{}\"></label>\
         <label class=\"field\"><span>{}</span><input class=\"box\" type=\"number\" min=\"0\" max=\"{max}\" step=\"1\" name=\"to_page\" value=\"{}\"></label></div>",
        esc(w.day_field),
        esc(&day),
        esc(w.from_field),
        esc(&from),
        esc(w.to_field),
        esc(&to),
        esc(w.from_page_field),
        esc(&opened),
        esc(w.to_page_field),
        esc(&closed)
    )
}

/// `panel`: the paper book at `at`'s own part: the form logging a sitting, its from page at the
/// bookmark, and the sittings logged, newest first, each with Edit and Delete. Its pages, its
/// cover and the way to delete it are in its editor ([`editing`], [`jacket`], [`deleting`]). Empty
/// for a Kindle book.
pub(super) fn panel(cx: &Ctx, at: usize) -> String {
    let Some((rows, book)) = cx.paper_of(at) else {
        return String::new();
    };
    let s = cx.s;
    let w = Words::of(cx.lang);
    let today = date::stamp(cx.today, 0);
    let today = date::day_of(&today);
    let read: Vec<&PaperSitting> = rows.paper_sittings_of(&book.added);
    let mut out = format!(
        "<div class=\"paper\" data-kindle=\"{}\" data-added=\"{}\">\
         <section><h2>{}</h2><div class=\"card\" data-log>{}\
         <p class=\"acts\"><button type=\"button\" class=\"save\" data-act=\"log-sitting\">{}</button></p></div></section>",
        esc(cx.serial(at)),
        esc(&book.added),
        esc(w.log_sitting),
        fields(w, None, today, rows.bookmark(&book.added), book.pages),
        esc(w.log_do)
    );
    if !read.is_empty() {
        let _ = write!(
            out,
            "<section><h2>{}<span class=\"said\">{}</span></h2><div class=\"card rows logged\">",
            esc(s.sittings),
            read.len()
        );
        for sitting in read.iter().rev() {
            out.push_str(&logged(cx, sitting));
        }
        out.push_str("</div></section>");
    }
    let _ = write!(
        out,
        "<template id=\"sitting-form\"><div class=\"editor\" data-sitting>{}<p class=\"acts\">\
         <button type=\"button\" data-act=\"cancel-sitting\">{}</button>\
         <button type=\"button\" class=\"save\" data-act=\"save-sitting\">{}</button></p></div></template></div>",
        fields(w, None, today, 0, book.pages),
        esc(s.cancel),
        esc(w.save)
    );
    out
}

/// `editing`: the paper book at `at`'s part of its editor, written with the rest on Save: its pages,
/// and the cover the hero shows while it is open (`jacket`), `uncovered` being the hero's cover with
/// none and `data-covered` saying the book has one. Empty for a Kindle book.
pub(super) fn editing(cx: &Ctx, at: usize) -> String {
    let Some((_, book)) = cx.paper_of(at) else {
        return String::new();
    };
    let w = Words::of(cx.lang);
    let covered = match book.cover.is_empty() {
        true => "",
        false => " data-covered",
    };
    format!(
        "<div class=\"paperedit\" data-paper-edit data-kindle=\"{}\" data-added=\"{}\"{covered}>\
         <label class=\"field\"><span>{}</span><input class=\"box\" type=\"number\" min=\"1\" step=\"1\" \
         name=\"pages\" value=\"{}\" data-was=\"{}\"></label><template id=\"uncovered\">{}</template></div>",
        esc(cx.serial(at)),
        esc(&book.added),
        esc(w.pages_field),
        book.pages,
        book.pages,
        cx.made(at)
    )
}

/// `jacket`: the paper book at `at`'s cover on its page, which takes a picture dropped on it into
/// its editor, with Remove the Cover under it while the editor is open and a cover shows; `None`
/// for a Kindle book, whose cover is its own.
pub(super) fn jacket(cx: &Ctx, at: usize) -> Option<String> {
    let (_, book) = cx.paper_of(at)?;
    let remove = match book.cover.is_empty() {
        true => " hidden",
        false => "",
    };
    Some(format!(
        "<div class=\"jacket\"><div class=\"cover\" data-cover-drop>{}</div>\
         <button type=\"button\" class=\"chip\" data-act=\"remove-paper-cover\"{remove}>{}</button></div>",
        cx.cover(at),
        esc(Words::of(cx.lang).remove_cover)
    ))
}

/// `deleting`: Delete This Book…, the paper book at `at`'s, first of its editor's buttons; empty
/// for a Kindle book.
pub(super) fn deleting(cx: &Ctx, at: usize) -> String {
    match cx.paper_of(at) {
        Some(_) => format!(
            "<button type=\"button\" data-act=\"delete-paper\">{}</button>",
            esc(Words::of(cx.lang).delete_paper)
        ),
        None => String::new(),
    }
}

/// `logged`: one sitting as its row in the list: its day and minutes, its pages and how long it
/// ran, Edit and Delete, the fields `app.js` sets its editor to in its data.
fn logged(cx: &Ctx, sitting: &PaperSitting) -> String {
    let s = cx.s;
    let w = Words::of(cx.lang);
    let day = date::parse_day(date::day_of(&sitting.from)).map_or_else(
        || date::day_of(&sitting.from).to_string(),
        |day| date::year_day(day, s),
    );
    let secs = sitting.span().map_or(0, |(from, to)| to - from);
    let pages = w
        .pages_span
        .replace("{a}", &sitting.from_page.to_string())
        .replace("{b}", &sitting.to_page.to_string());
    format!(
        "<div class=\"row\" data-logged=\"{}\" data-day=\"{}\" data-from=\"{}\" data-to=\"{}\" \
         data-from-page=\"{}\" data-to-page=\"{}\"><span>{} · {}–{}</span><span class=\"what\">{} · {}</span>\
         <span class=\"acts\"><button type=\"button\" data-act=\"edit-sitting\">{}</button>\
         <button type=\"button\" data-act=\"delete-sitting\">{}</button></span></div>",
        esc(&sitting.logged),
        esc(date::day_of(&sitting.from)),
        esc(minute(&sitting.from)),
        esc(minute(&sitting.to)),
        sitting.from_page,
        sitting.to_page,
        esc(&day),
        esc(minute(&sitting.from)),
        esc(minute(&sitting.to)),
        esc(&pages),
        esc(&date::duration(secs, s)),
        esc(w.edit),
        esc(w.delete)
    )
}

/// `refusal`: why a paper book, its cover or a sitting was not written, in the window's words,
/// where `err` is a `Refusal`, `paper::NoKindle` or `paper::NotAPicture`; `None` for any other
/// failure.
pub fn refusal(err: &anyhow::Error, w: &Words) -> Option<&'static str> {
    if err.downcast_ref::<crate::paper::NoKindle>().is_some() {
        return Some(w.no_kindle);
    }
    if err.downcast_ref::<crate::paper::NotAPicture>().is_some() {
        return Some(w.not_a_picture);
    }
    Some(match err.downcast_ref::<Refusal>()? {
        Refusal::Untitled => w.refused_untitled,
        Refusal::NoPages => w.refused_no_pages,
        Refusal::UnderLogged => w.refused_under_logged,
        Refusal::NotAnInstant => w.refused_not_an_instant,
        Refusal::Backwards => w.refused_backwards,
        Refusal::TooLong => w.refused_too_long,
        Refusal::Ahead => w.refused_ahead,
        Refusal::Pages => w.refused_pages,
        Refusal::Overlaps => w.refused_overlaps,
        Refusal::NoBook | Refusal::NoSitting => w.refused_gone,
    })
}
