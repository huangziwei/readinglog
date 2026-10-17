//! The window's screens: each [`Route`] drawn as HTML off the record as of the
//! clock's day, the charts inline SVG, every figure a Kindle view's own and
//! every day, span and book a link to its own page.

mod book;
mod books;
mod chart;
mod day;
mod journal;
mod look;
mod rhythm;
mod settings;
mod tile;
mod words;

use std::collections::{HashMap, HashSet};
use std::fmt::Write as _;
use std::path::PathBuf;

use readinglog_native::date;
use readinglog_native::font::Script;
use readinglog_native::lang::{Lang, Strings};
use readinglog_native::record::Record;
use readinglog_native::settings::{Settings, StatsFrom, WeekStart};
use readinglog_native::stats::Stats;
use readinglog_native::ui::paint::Palette;
use readinglog_native::view::{Shelf, Sort, Span, Way, Window};

pub use look::Look;
pub use words::Words;

use crate::edits::HighlightId;
use crate::notes::Laid;
use crate::workspace::Workspace;

/// One screen as the window sets it.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct Page {
    pub title: String,
    pub side: String,
    pub main: String,
    /// [`Look::css`] for the colour scheme set.
    pub vars: String,
    /// `Lang::language_tag` of the language set, for the page's `<html>`.
    pub lang: String,
}

/// Where a page is asked for, as the window's hash states it: `/`, `/day/`,
/// `/week/`, `/month/` and `/year/` with a date (`<yyyy-mm>` and `<yyyy>` the
/// first day), `/all`, `/books`, `/book/<key>`, `/journal`, `/settings`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Route {
    /// The clock's day, as the Kindle's Today states it.
    Today,
    Day(i64),
    /// A span of Rhythm by the day it holds, the Kindle's `State::day`; `AllTime` 0.
    Span(Span, i64),
    Books {
        shelf: Shelf,
        order: Sort,
        way: Way,
        query: String,
        window: Option<Window>,
    },
    Book {
        key: String,
        /// Which reading the graphs draw, from the first; `None` the standing one.
        reading: Option<usize>,
        /// Which end the journal opens at.
        way: Way,
        query: String,
    },
    /// `/journal`: every book's highlights and notes, under its search.
    Journal(String),
    Settings,
}

impl Route {
    pub fn parse(said: &str) -> Route {
        let (path, query) = said.split_once('?').unwrap_or((said, ""));
        let asked = |name: &str| {
            query
                .split('&')
                .filter_map(|pair| pair.split_once('='))
                .find(|(key, _)| *key == name)
                .map(|(_, value)| unescape(value))
                .unwrap_or_default()
        };
        let way = |said: String| match said.as_str() {
            "up" => Way::Up,
            _ => Way::Down,
        };
        let parts: Vec<&str> = path.trim_matches('/').split('/').collect();
        let day = |said: &str| date::parse_day(said);
        // `at` names a whole date, else a month or a year that `opens` completes.
        let within = |at: &str, opens: &str| match at.len() {
            10 => day(at),
            _ => day(&format!("{at}{opens}")),
        };
        match parts.as_slice() {
            ["day", at] => day(at).map_or(Route::Today, Route::Day),
            ["week", at] => day(at).map_or(Route::Today, |d| Route::Span(Span::Week, d)),
            ["month", at] => {
                within(at, "-01").map_or(Route::Today, |d| Route::Span(Span::Month, d))
            }
            ["year", at] => {
                within(at, "-01-01").map_or(Route::Today, |d| Route::Span(Span::Year, d))
            }
            ["all"] => Route::Span(Span::AllTime, 0),
            ["books"] => Route::Books {
                shelf: match asked("shelf").as_str() {
                    "finished" => Shelf::Finished,
                    "unfinished" => Shelf::Unfinished,
                    _ => Shelf::All,
                },
                order: match asked("order").as_str() {
                    "time" => Sort::Time,
                    "progress" => Sort::Progress,
                    _ => Sort::Recent,
                },
                way: way(asked("way")),
                query: asked("q"),
                window: {
                    let span = match asked("span").as_str() {
                        "week" => Some(Span::Week),
                        "month" => Some(Span::Month),
                        "year" => Some(Span::Year),
                        _ => None,
                    };
                    span.zip(day(&asked("day")))
                        .map(|(span, day)| Window { span, day })
                },
            },
            ["book", key] => Route::Book {
                key: unescape(key),
                reading: asked("reading").parse().ok(),
                way: way(asked("way")),
                query: asked("q"),
            },
            ["journal"] => Route::Journal(asked("q")),
            ["settings"] => Route::Settings,
            _ => Route::Today,
        }
    }

    /// The hash that asks for this route.
    pub fn link(&self) -> String {
        let query = |pairs: &[(&str, String)]| {
            let said: Vec<String> = pairs
                .iter()
                .filter(|(_, value)| !value.is_empty())
                .map(|(key, value)| format!("{key}={}", escape(value)))
                .collect();
            match said.is_empty() {
                true => String::new(),
                false => format!("?{}", said.join("&")),
            }
        };
        let way = |way: &Way| match way {
            Way::Down => String::new(),
            Way::Up => "up".to_string(),
        };
        match self {
            Route::Today => "#/".into(),
            Route::Day(day) => format!("#/day/{}", day_key(*day)),
            Route::Span(Span::AllTime, _) => "#/all".into(),
            Route::Span(Span::Week, day) => format!("#/week/{}", day_key(*day)),
            Route::Span(Span::Month, day) => format!("#/month/{}", day_key(*day)),
            Route::Span(Span::Year, day) => format!("#/year/{}", day_key(*day)),
            Route::Books {
                shelf,
                order,
                way: turned,
                query: search,
                window,
            } => {
                let shelf = match shelf {
                    Shelf::All => "",
                    Shelf::Finished => "finished",
                    Shelf::Unfinished => "unfinished",
                };
                let order = match order {
                    Sort::Recent => "",
                    Sort::Time => "time",
                    Sort::Progress => "progress",
                };
                let (span, day) = match window {
                    Some(Window { span, day }) => (
                        match span {
                            Span::Week => "week",
                            Span::Month => "month",
                            Span::Year => "year",
                            Span::AllTime => "",
                        },
                        day_key(*day),
                    ),
                    None => ("", String::new()),
                };
                format!(
                    "#/books{}",
                    query(&[
                        ("shelf", shelf.into()),
                        ("order", order.into()),
                        ("way", way(turned)),
                        ("q", search.clone()),
                        ("span", span.into()),
                        ("day", day),
                    ])
                )
            }
            Route::Book {
                key,
                reading,
                way: turned,
                query: search,
            } => format!(
                "#/book/{}{}",
                escape(key),
                query(&[
                    (
                        "reading",
                        reading.map(|r| r.to_string()).unwrap_or_default()
                    ),
                    ("way", way(turned)),
                    ("q", search.clone()),
                ])
            ),
            Route::Journal(search) => format!("#/journal{}", query(&[("q", search.clone())])),
            Route::Settings => "#/settings".into(),
        }
    }

    /// The Books route on `shelf` under `window`, in the Kindle's default order.
    fn shelf(shelf: Shelf, window: Option<Window>) -> Route {
        Route::Books {
            shelf,
            order: Sort::Recent,
            way: Way::Down,
            query: String::new(),
            window,
        }
    }

    /// The route of a book, at its standing reading.
    fn book(key: String) -> Route {
        Route::Book {
            key,
            reading: None,
            way: Way::Down,
            query: String::new(),
        }
    }
}

/// `day` as `YYYY-MM-DD`, which `date::parse_day` reads back.
fn day_key(day: i64) -> String {
    let (y, m, d) = date::civil_from_days(day);
    format!("{y:04}-{m:02}-{d:02}")
}

/// The day of `year` the Kindle's arrows step `today` to, a year a step.
fn in_year(today: i64, year: i64) -> i64 {
    Span::Year.step(today, year - date::civil_from_days(today).0)
}

/// How a page names a cover file.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Covers {
    /// A `data-cover` path, for the window to turn into an asset URL.
    Window,
    /// A `file:` URL, for a page read off the disk.
    Files,
}

/// The record as every screen states it: `Stats` as of `today` under the
/// settings `Workspace::config` names, and each book's cover, tone and Kindle.
pub struct Shown {
    pub record: Record,
    pub stats: Stats,
    pub settings: Settings,
    pub lang: Lang,
    /// The clock's day, which every screen states as today.
    pub today: i64,
    pub covers: Vec<Option<PathBuf>>,
    pub tones: Vec<[u8; 3]>,
    /// Each book's Kindle as `kindles::labels` calls it, `None` throughout for
    /// one source.
    pub kindles: Vec<Option<String>>,
    /// Each source's Kindle by its cut serial, empty for the backups naming none.
    pub serials: Vec<String>,
    /// What `kindles::labels` calls each source.
    pub labels: Vec<String>,
    /// The highlights in words a correction sets, by their Kindle's serial.
    pub corrected: HashSet<(String, HighlightId)>,
    /// The notes laid into the record, by their Kindle's serial and the
    /// identity each takes there.
    pub written: HashMap<(String, HighlightId), Laid>,
}

impl Shown {
    /// The record `ws` holds as of `today`, the day `date::now` answers, each
    /// source as `Record::drawn` draws it; `None` where the record holds no
    /// sitting.
    pub fn of(ws: &Workspace, today: i64) -> Option<Shown> {
        let mut sources = ws.sources();
        let serials: Vec<String> = sources
            .iter()
            .map(|s| s.serial.clone().unwrap_or_default())
            .collect();
        let mut corrected = HashSet::new();
        let mut written = HashMap::new();
        for (source, serial) in sources.iter_mut().zip(&serials) {
            let drawn = source.record.drawn();
            corrected.extend(drawn.corrected.into_iter().map(|id| (serial.clone(), id)));
            written.extend(
                drawn
                    .written
                    .into_iter()
                    .map(|(id, laid)| ((serial.clone(), id), laid)),
            );
            source.record = drawn.record;
        }
        let mut record = crate::kindles::together(&sources);
        if record.sittings.is_empty() {
            return None;
        }
        let dirs: Vec<PathBuf> = sources.iter().map(|s| s.dir.clone()).collect();
        record.books = record
            .books
            .iter()
            .map(|b| crate::kindles::on_this_computer(b, &dirs))
            .collect();
        let (settings, _) = ws.config.settings();
        let mut stats = Stats::build(
            &record,
            today,
            settings.show_unidentified,
            settings.stats_from,
            settings.sitting_floor,
        );
        stats.written = written
            .iter()
            .map(|((_, id), laid)| (id.clone(), laid.clone()))
            .collect();
        // `thumbnail`: the copy `on_this_computer` names, where `has_cover`.
        let covers: Vec<Option<PathBuf>> = stats
            .books
            .iter()
            .map(|b| b.has_cover().then(|| PathBuf::from(&b.thumbnail)))
            .collect();
        let labels = crate::kindles::labels(&sources, Words::of(ws.config.lang()).from_backup);
        let kindles = stats
            .books
            .iter()
            .map(|b| match sources.len() > 1 {
                true => crate::kindles::origin(b.extent)
                    .0
                    .and_then(|n| labels.get(n).cloned()),
                false => None,
            })
            .collect();
        let tones = stats
            .books
            .iter()
            .zip(&covers)
            .map(|(b, c)| {
                c.as_deref()
                    .and_then(look::cover_tone)
                    .unwrap_or_else(|| look::title_tone(&b.title))
            })
            .collect();
        Some(Shown {
            record,
            stats,
            lang: ws.config.lang(),
            settings,
            today,
            covers,
            tones,
            kindles,
            serials,
            labels,
            corrected,
            written,
        })
    }

    /// What the Kindle the cut serial `serial` names is called: as its source
    /// is in [`Shown::labels`], else by its model.
    pub fn called(&self, serial: &str) -> String {
        self.serials
            .iter()
            .position(|held| held == serial)
            .and_then(|n| self.labels.get(n).cloned())
            .unwrap_or_else(|| crate::kindles::model(serial).to_string())
    }

    /// The years holding a sitting, newest first.
    fn years(&self) -> Vec<i64> {
        let year = |at: Option<&(i64, i64)>| at.map(|(day, _)| date::civil_from_days(*day).0);
        match (year(self.stats.days.first()), year(self.stats.days.last())) {
            (Some(first), Some(last)) => (first..=last).rev().collect(),
            _ => Vec::new(),
        }
    }

    /// The book `key` names, by its index in `Stats::books`.
    fn book(&self, key: &str) -> Option<usize> {
        self.stats.books.iter().position(|b| key_of(b) == key)
    }
}

/// What a book's route names it by: its `cde_key`, else `e` and its extent.
pub fn key_of(book: &readinglog_native::stats::BookStat) -> String {
    match book.cde_key.is_empty() {
        true => format!("e{}", book.extent),
        false => book.cde_key.clone(),
    }
}

/// `route` drawn `width` wide off `shown`, the sidebar beside it. `kindle` is
/// the cut serial of the Kindle plugged in, drawn as [`kindle_card`].
pub fn render(
    shown: Option<&Shown>,
    ws: &Workspace,
    route: &Route,
    width: f32,
    links: Covers,
    kindle: Option<&str>,
) -> Page {
    let lang = shown.map_or(ws.config.lang(), |s| s.lang);
    let s = lang.strings();
    let w = Words::of(lang);
    let (settings, _) = ws.config.settings();
    let look = Look::light(&Palette::of(settings.color_scheme));
    let vars = look.css();
    let Some(shown) = shown else {
        let main = match route {
            Route::Settings => settings::page(ws, None, s, w),
            _ => settings::welcome(w, kindle),
        };
        return Page {
            title: "Reading Log".into(),
            side: side(None, route, s, w, kindle),
            main,
            vars,
            lang: lang.language_tag().into(),
        };
    };
    let cx = Ctx {
        stats: &shown.stats,
        s,
        lang,
        week: shown.settings.week_start,
        from: shown.settings.stats_from,
        today: shown.today,
        opened: shown.stats.opened(shown.today),
        look,
        tones: &shown.tones,
        covers: &shown.covers,
        kindles: &shown.kindles,
        serials: &shown.serials,
        corrected: &shown.corrected,
        written: &shown.written,
        links,
        uncovered: shown.settings.show_uncovered,
        width: (width - 120.0).max(320.0),
    };
    let route = match route {
        Route::Book { key, .. } if shown.book(key).is_none() => Route::Today,
        other => other.clone(),
    };
    let (title, mut main) = match &route {
        Route::Today => (s.today.to_string(), day::page(&cx, shown.today, true)),
        Route::Day(day) => (date::long_day(*day, s), day::page(&cx, *day, false)),
        Route::Span(Span::AllTime, _) => (
            s.all_time.to_string(),
            rhythm::all_time(&cx, &shown.years()),
        ),
        Route::Span(span, day) => (span.name(*day, cx.week, s), rhythm::span(&cx, *span, *day)),
        Route::Books {
            shelf,
            order,
            way,
            query,
            window,
        } => (
            s.books.to_string(),
            books::page(&cx, *shelf, *order, *way, query, *window),
        ),
        Route::Book {
            key,
            reading,
            way,
            query,
        } => {
            let at = shown.book(key).unwrap_or(0);
            let title = shown.stats.books[at].title.clone();
            (title, book::page(&cx, at, *reading, *way, query))
        }
        Route::Journal(query) => (s.journal_tab.to_string(), journal::page(&cx, query)),
        Route::Settings => (s.config.to_string(), settings::page(ws, Some(shown), s, w)),
    };
    main.push_str(&editor(s, w));
    Page {
        title,
        side: side(Some(shown), &route, s, w, kindle),
        main,
        vars,
        lang: lang.language_tag().into(),
    }
}

/// The editors `app.js` opens: a highlight's, its text areas above these
/// buttons and `revert` only on one in words set here; and a desktop note's,
/// `delete-note` taken away where it opens on no note.
fn editor(s: &Strings, w: &Words) -> String {
    format!(
        "<template id=\"editor\"><div class=\"editor\"><p class=\"acts\">\
         <button data-act=\"revert\" data-tip=\"{}\">{}</button>\
         <button data-act=\"cancel\">{}</button>\
         <button class=\"save\" data-act=\"save\">{}</button></p></div></template>\
         <template id=\"note-editor\"><div class=\"editor\"><textarea data-part=\"note\"></textarea>\
         <p class=\"acts\"><button data-act=\"delete-note\">{}</button>\
         <button data-act=\"cancel-note\">{}</button>\
         <button class=\"save\" data-act=\"save-note\">{}</button></p></div></template>",
        esc(w.revert_tip),
        esc(w.revert),
        esc(s.cancel),
        esc(w.save),
        esc(w.delete),
        esc(s.cancel),
        esc(w.save)
    )
}

/// What every page of one render reads.
struct Ctx<'a> {
    stats: &'a Stats,
    s: &'static Strings,
    lang: Lang,
    week: WeekStart,
    from: StatsFrom,
    /// The clock's day, which every page reads as today.
    today: i64,
    /// The record's first day with reading on it.
    opened: i64,
    look: Look,
    tones: &'a [[u8; 3]],
    covers: &'a [Option<PathBuf>],
    kindles: &'a [Option<String>],
    /// [`Shown::serials`], [`Shown::corrected`] and [`Shown::written`].
    serials: &'a [String],
    corrected: &'a HashSet<(String, HighlightId)>,
    written: &'a HashMap<(String, HighlightId), Laid>,
    links: Covers,
    /// `Settings::show_uncovered`, which `view::books::listed` takes.
    uncovered: bool,
    /// The width a chart inside a card takes.
    width: f32,
}

impl Ctx<'_> {
    /// The route of the book at `at` in `Stats::books`.
    fn book_link(&self, at: usize) -> String {
        Route::book(key_of(&self.stats.books[at])).link()
    }

    /// The cut serial of the Kindle the book at `at` is from, empty for the
    /// backups naming none.
    fn serial(&self, at: usize) -> &str {
        let book = &self.stats.books[at];
        let from = match self.serials.len() {
            1 => Some(0),
            _ => crate::kindles::origin(book.extent)
                .0
                .or_else(|| crate::kindles::key_origin(&book.cde_key).0),
        };
        from.and_then(|n| self.serials.get(n))
            .map_or("", String::as_str)
    }

    /// The Kindle the book at `at` is from, where `kindles` names one.
    fn kindle(&self, at: usize) -> Option<&str> {
        self.kindles.get(at).and_then(|k| k.as_deref())
    }

    /// The note written here that `Record::drawn` laid as `highlight`, in the
    /// book at `at`.
    fn laid(
        &self,
        at: usize,
        highlight: &readinglog_native::highlights::Highlight,
    ) -> Option<&Laid> {
        self.written
            .get(&(self.serial(at).to_string(), HighlightId::of(highlight)))
    }

    /// The cover of the book at `at`, or one made of its title on its tone.
    fn cover(&self, at: usize) -> String {
        let book = &self.stats.books[at];
        if let Some(path) = &self.covers[at] {
            let path = esc(&path.to_string_lossy());
            return match self.links {
                Covers::Window => format!("<img data-cover=\"{path}\" alt=\"\">"),
                Covers::Files => format!("<img src=\"file://{path}\" alt=\"\">"),
            };
        }
        let [r, g, b] = self.tones[at];
        format!(
            "<div class=\"made\"{} style=\"background: linear-gradient(160deg, rgb({r},{g},{b}), rgb({},{},{}))\">\
             <span>{}</span><small{}>{}</small></div>",
            lang(&book.language, &book.title),
            r / 2,
            g / 2,
            b / 2,
            esc(&book.title),
            lang(&book.language, &book.author),
            esc(&book.author)
        )
    }

    /// The note closing a list of books: how many unidentified books `days`
    /// holds and the time on them, empty where it holds none.
    fn unidentified(&self, days: std::ops::RangeInclusive<i64>) -> String {
        let (books, seconds) = self.stats.unidentified_over(days);
        match books {
            0 => String::new(),
            _ => format!(
                "<p class=\"more\">{books} {} · {}</p>",
                esc(self.s.unidentified),
                esc(&date::duration(seconds, self.s))
            ),
        }
    }
}

/// The sidebar: under `shown`, the screens, the years with reading, a Kindle
/// plugged in and the way to a backup; under any record, the way to the settings.
fn side(
    shown: Option<&Shown>,
    route: &Route,
    s: &Strings,
    w: &Words,
    kindle: Option<&str>,
) -> String {
    let mut out = String::from("<p class=\"name\">Reading Log</p>");
    let item = |out: &mut String, to: &Route, name: &str, lit: bool| {
        let _ = write!(
            out,
            "<a href=\"{}\"{}>{}</a>",
            to.link(),
            if lit { " class=\"on\"" } else { "" },
            esc(name)
        );
    };
    if let Some(shown) = shown {
        out.push_str("<div class=\"nav\">");
        let today = matches!(route, Route::Today) || *route == Route::Day(shown.today);
        item(&mut out, &Route::Today, s.today, today);
        let rhythm = !today && matches!(route, Route::Span(..) | Route::Day(_));
        item(&mut out, &Route::Span(Span::AllTime, 0), s.rhythm, rhythm);
        let books = matches!(route, Route::Books { .. } | Route::Book { .. });
        item(&mut out, &Route::shelf(Shelf::All, None), s.books, books);
        let journal = matches!(route, Route::Journal(_));
        item(
            &mut out,
            &Route::Journal(String::new()),
            s.journal_tab,
            journal,
        );
        let _ = write!(
            out,
            "</div><div class=\"nav years\"><p class=\"label\">{}</p>",
            esc(s.year)
        );
        for year in shown.years() {
            let to = Route::Span(Span::Year, in_year(shown.today, year));
            let lit = matches!(route, Route::Span(Span::Year, day) if date::civil_from_days(*day).0 == year);
            item(&mut out, &to, &year.to_string(), lit);
        }
        let all = Route::Span(Span::AllTime, 0);
        item(&mut out, &all, s.all_time, *route == all);
        out.push_str("</div>");
    }
    out.push_str("<div class=\"foot\">");
    if let (Some(shown), Some(serial)) = (shown, kindle) {
        out.push_str(&kindle_card(w, &shown.called(serial), serial, ""));
    }
    if shown.is_some() {
        let _ = write!(
            out,
            "<button type=\"button\" class=\"quiet\" data-act=\"pick\">{}</button>",
            esc(w.choose_backup)
        );
    }
    let _ = write!(
        out,
        "<a href=\"#/settings\" class=\"quiet{}\">{}</a></div>",
        if *route == Route::Settings { " on" } else { "" },
        esc(s.config)
    );
    out
}

/// The card of the Kindle plugged in under the cut serial `serial`, `name`
/// over the line `app.js` sets as a sync goes, and `try_again`, hidden until a
/// sync stops; `class` is added to `kindle`.
pub(super) fn kindle_card(w: &Words, name: &str, serial: &str, class: &str) -> String {
    format!(
        "<div class=\"kindle{class}\" data-serial=\"{serial}\"><b>{name}</b><p class=\"state\"></p>\
         <button type=\"button\" class=\"button\" data-act=\"kindle\" data-serial=\"{serial}\" hidden>{again}</button></div>",
        serial = esc(serial),
        name = esc(name),
        again = esc(w.try_again),
    )
}

/// A link drawn as a chip, lit where it is the route standing.
fn chip(out: &mut String, href: &str, name: &str, on: bool) {
    let _ = write!(
        out,
        "<a class=\"chip{}\" href=\"{href}\">{}</a>",
        if on { " on" } else { "" },
        esc(name)
    );
}

/// `text` with `&`, `<`, `>` and `"` written as entities.
pub fn esc(text: &str) -> String {
    text.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}

/// The language `text` of a book tagged `tag` is set in: the Han convention
/// `Script::resolve` reads off the two, as the Kindle picks a face, else `tag`.
fn language_of(tag: &str, text: &str) -> String {
    let named = match Script::resolve(Script::of_language(tag), text) {
        Script::Japanese => "ja",
        Script::SimplifiedChinese => "zh-Hans",
        Script::TraditionalChinese => "zh-Hant",
        Script::Korean => "ko",
        Script::Unknown => tag.trim(),
    };
    named.to_string()
}

/// A `lang` attribute stating [`language_of`], or nothing where it names none.
fn lang(tag: &str, text: &str) -> String {
    let named = language_of(tag, text);
    match named.is_empty() {
        true => String::new(),
        false => format!(" lang=\"{}\"", esc(&named)),
    }
}

/// `text` as a URL part: every byte outside `A-Z a-z 0-9 - . _ ~` as `%XX`.
fn escape(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for byte in text.bytes() {
        match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'.' | b'_' | b'~' => {
                out.push(byte as char)
            }
            _ => {
                let _ = write!(out, "%{byte:02X}");
            }
        }
    }
    out
}

/// [`escape`] read back, and `+` as a space; a `%` with no two hex digits
/// after it stands as itself.
fn unescape(text: &str) -> String {
    let bytes = text.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut at = 0;
    while at < bytes.len() {
        let hex = |b: u8| (b as char).to_digit(16);
        match bytes[at] {
            b'%' if at + 2 < bytes.len() => match (hex(bytes[at + 1]), hex(bytes[at + 2])) {
                (Some(high), Some(low)) => {
                    out.push((high * 16 + low) as u8);
                    at += 3;
                    continue;
                }
                _ => out.push(b'%'),
            },
            b'+' => out.push(b' '),
            other => out.push(other),
        }
        at += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tests::{book, highlight, record_of, scratch, sitting};
    use readinglog_native::view::book::DASH;

    /// A workspace holding two books read over three days, one highlight on
    /// the first.
    fn read(name: &str) -> Workspace {
        let ws = Workspace::create(&scratch(name)).unwrap();
        let mut record = record_of(
            vec![
                book(100, "B1", "A Map of Small Rivers"),
                book(200, "B2", "川辺の記録"),
            ],
            vec![
                sitting("2026-09-01T09:00:00", 100, 600),
                sitting("2026-09-02T21:00:00", 200, 1200),
                sitting("2026-10-01T09:00:00", 100, 1800),
            ],
        );
        record.highlights = vec![highlight(
            100,
            "A Map of Small Rivers",
            "2026-09-01T09:05:00",
        )];
        record.save(&ws.record_dir()).unwrap();
        ws
    }

    #[test]
    fn a_route_reads_back_the_link_it_makes() {
        let day = date::parse_day("2026-09-02").unwrap();
        let routes = [
            Route::Today,
            Route::Day(day),
            Route::Span(Span::Week, day),
            Route::Span(Span::Month, day),
            Route::Span(Span::Year, day),
            Route::Span(Span::AllTime, 0),
            Route::Books {
                shelf: Shelf::Finished,
                order: Sort::Time,
                way: Way::Up,
                query: "川辺 & co".into(),
                window: Some(Window {
                    span: Span::Month,
                    day,
                }),
            },
            Route::shelf(Shelf::All, None),
            Route::Book {
                key: "*AB/CD?".into(),
                reading: Some(1),
                way: Way::Up,
                query: "a b".into(),
            },
            Route::book("B1".into()),
            Route::Journal("a b".into()),
            Route::Journal(String::new()),
            Route::Settings,
        ];
        for route in routes {
            let link = route.link();
            assert_eq!(Route::parse(link.trim_start_matches('#')), route, "{link}");
        }
        assert_eq!(Route::parse("/nowhere"), Route::Today);
        assert_eq!(Route::parse("/year/soon"), Route::Today);
        let first = |said: &str| date::parse_day(said).unwrap();
        assert_eq!(
            Route::parse("/month/2026-09"),
            Route::Span(Span::Month, first("2026-09-01"))
        );
        assert_eq!(
            Route::parse("/year/2026"),
            Route::Span(Span::Year, first("2026-01-01"))
        );
    }

    #[test]
    fn a_broken_escape_stands_as_itself() {
        assert_eq!(unescape("100%"), "100%");
        assert_eq!(unescape("%zz%2"), "%zz%2");
        assert_eq!(unescape("a+b%20c"), "a b c");
    }

    /// The day the clock stands at in these tests, a fortnight past the newest
    /// sitting [`read`] holds.
    fn clock() -> i64 {
        date::parse_day("2026-10-15").unwrap()
    }

    #[test]
    fn every_route_draws_off_the_record_as_of_the_clocks_day() {
        let ws = read("screen-routes");
        let shown = Shown::of(&ws, clock()).unwrap();
        let s = Lang::English.strings();
        let draw = |route: &str| {
            render(
                Some(&shown),
                &ws,
                &Route::parse(route),
                1072.0,
                Covers::Files,
                None,
            )
        };
        let today = draw("/");
        assert_eq!(today.title, "Today");
        assert!(
            today.main.contains(&date::long_day(clock(), s)),
            "{}",
            today.main
        );
        assert!(today.main.contains(s.nothing_read), "nothing read on it");
        assert!(
            today.side.contains("class=\"on\">Today</a>"),
            "{}",
            today.side
        );
        assert!(
            today.side.contains("href=\"#/year/2026-10-15\""),
            "a year opens at the clock's day"
        );
        assert!(
            draw("/day/2026-10-01")
                .main
                .contains("A Map of Small Rivers"),
            "the newest sitting's day states its book"
        );
        let year = draw("/year/2026-09-02");
        assert!(
            year.main.contains("href=\"#/day/2026-09-02\""),
            "a day read on opens"
        );
        assert!(
            !year.main.contains("href=\"#/day/2026-09-03\""),
            "a day with none opens nothing"
        );
        assert!(
            year.main.contains("href=\"#/month/2026-08-02\""),
            "a month name opens its month at the year's day"
        );
        assert!(
            year.main.contains("href=\"#/week/2026-09-02\""),
            "Week opens on the year's day"
        );
        let week = draw("/week/2026-09-02");
        assert!(week.main.contains("href=\"#/day/2026-09-01\""));
        assert!(week.main.contains("川辺の記録"), "what was read that week");
        let month = draw("/month/2026-09");
        assert!(month.main.contains("href=\"#/day/2026-09-02\""));
        let day = draw("/day/2026-09-01");
        assert!(day.main.contains("Some words"), "the day's highlight");
        let books = draw("/books?q=small");
        assert!(books.main.contains("A Map of Small Rivers"));
        assert!(
            !books.main.contains("川辺の記録"),
            "the search narrows the grid"
        );
        let one = draw("/book/B1");
        assert_eq!(one.title, "A Map of Small Rivers");
        assert!(one.main.contains("Some words"), "its highlight");
        assert!(draw("/all").main.contains("<svg"));
        assert!(
            draw("/journal?q=words")
                .main
                .contains("Some <mark>words</mark>")
        );
        assert!(!draw("/journal?q=nothing").main.contains("Some words"));
        assert!(draw("/settings").main.contains("data-set=\"show_unnamed\""));
        for route in [
            "/",
            "/day/2026-09-02",
            "/week/2026-09-02",
            "/month/2026-09",
            "/year/2026",
            "/all",
            "/books",
            "/book/B1",
            "/book/B2",
            "/journal",
        ] {
            let page = draw(route);
            assert!(!page.main.contains(DASH), "{route} states a dash");
            assert!(page.vars.contains("--accent"), "{route}");
        }
    }

    #[test]
    fn every_width_opens_on_now_and_offers_now_away_from_it() {
        let ws = read("screen-now");
        let shown = Shown::of(&ws, clock()).unwrap();
        let draw = |route: &str| {
            render(
                Some(&shown),
                &ws,
                &Route::parse(route),
                1072.0,
                Covers::Files,
                None,
            )
            .main
        };
        let all = draw("/all");
        for span in ["week", "month", "year"] {
            assert!(
                all.contains(&format!("class=\"chip\" href=\"#/{span}/2026-10-15\"")),
                "{span}: {all}"
            );
        }
        for (away, now) in [
            ("/day/2026-09-02", "#/day/2026-10-15"),
            ("/week/2026-09-02", "#/week/2026-10-15"),
            ("/month/2026-09-02", "#/month/2026-10-15"),
            ("/year/2025-10-15", "#/year/2026-10-15"),
        ] {
            let page = draw(away);
            assert!(
                page.contains(&format!("<a class=\"chip\" href=\"{now}\">Now</a>")),
                "{away}: {page}"
            );
            assert!(page.contains("data-key=\"next\""), "{away}");
        }
        for at in [
            "/",
            "/day/2026-10-15",
            "/week/2026-10-15",
            "/month/2026-10-15",
            "/year/2026-10-15",
        ] {
            let page = draw(at);
            assert!(!page.contains(">Now</a>"), "{at}: {page}");
            assert!(!page.contains("data-key=\"next\""), "{at}: no › past now");
        }
    }

    #[test]
    fn a_window_narrows_the_books_to_its_span() {
        let ws = read("screen-window");
        let shown = Shown::of(&ws, clock()).unwrap();
        let page = render(
            Some(&shown),
            &ws,
            &Route::parse("/books?span=month&day=2026-09-02"),
            1072.0,
            Covers::Files,
            None,
        );
        assert!(page.main.contains("川辺の記録"), "put down in September");
        assert!(
            !page.main.contains("A Map of Small Rivers"),
            "put down in October"
        );
    }

    #[test]
    fn a_book_no_route_names_draws_today() {
        let ws = read("screen-lost");
        let shown = Shown::of(&ws, clock()).unwrap();
        let page = render(
            Some(&shown),
            &ws,
            &Route::parse("/book/gone"),
            1072.0,
            Covers::Files,
            None,
        );
        assert_eq!(page.title, "Today");
    }

    #[test]
    fn a_book_read_on_one_day_draws_one_bar_a_row_wide_stating_its_time() {
        let ws = read("screen-one-day");
        let shown = Shown::of(&ws, clock()).unwrap();
        let page = render(
            Some(&shown),
            &ws,
            &Route::parse("/book/B2"),
            1072.0,
            Covers::Files,
            None,
        );
        // `1072.0` leaves a 952 px chart, and `x` 457 sets the one 38 px bar in its middle.
        assert!(
            page.main
                .contains("x=\"457.0\" y=\"0.0\" width=\"38.0\" height=\"150.0\""),
            "{}",
            page.main
        );
        assert!(
            page.main
                .contains("<text x=\"476.0\" y=\"12.9\" font-size=\"11.0\""),
            "its time, inside the bar's head"
        );
        assert!(page.main.contains(">20m</text>"));
        assert!(page.main.contains("<h1 lang=\"ja\">川辺の記録</h1>"));
        assert_eq!(page.lang, "en");
    }

    #[test]
    fn a_stretch_draws_a_rectangle_and_a_moment_a_line() {
        let look = Look::light(&Palette::GREY);
        let s = Lang::English.strings();
        // `stretches` holds 30 s at 01:00 and an hour from 02:00, drawn across 960 px.
        let stretches = [(3600, 3630), (7200, 10_800)];
        let day = chart::timeline(&stretches, &look, 960.0, s);
        let book = chart::spans(&stretches, &look, [0x40, 0x60, 0x80], 960.0);
        for drawn in [&day, &book] {
            assert!(!drawn.contains("rx="), "{drawn}");
        }
        assert!(
            day.contains("<rect x=\"40.0\" y=\"3\" width=\"2.0\""),
            "{day}"
        );
        assert!(day.contains("<rect x=\"80.0\" y=\"3\" width=\"40.0\""));
        assert!(
            book.contains("<rect x=\"40.0\" y=\"0\" width=\"2.0\""),
            "{book}"
        );
        assert!(book.contains("<rect x=\"80.0\" y=\"0\" width=\"40.0\""));
    }

    #[test]
    fn a_highlight_stands_behind_a_rule_in_its_colour_with_what_a_search_found_washed() {
        let ws = read("screen-highlight");
        let shown = Shown::of(&ws, clock()).unwrap();
        let page = render(
            Some(&shown),
            &ws,
            &Route::parse("/journal?q=WORDS"),
            1072.0,
            Covers::Files,
            None,
        );
        // `read` holds a highlight stating no `colour`, drawn `#f3ce2b`.
        assert!(
            page.main.contains(
                "<blockquote style=\"border-color: #f3ce2b\" lang=\"en\">Some <mark>words</mark></blockquote>"
            ),
            "{}",
            page.main
        );
        assert!(page.main.contains(
            "<span class=\"when\"><button class=\"edit\" data-act=\"add-note\">Add note</button>\
             <button class=\"edit\" data-act=\"edit\">Edit</button>Sep 1, 2026</span></p>"
        ));
    }

    #[test]
    fn a_book_stands_as_one_tile_wherever_books_stand_as_covers() {
        let ws = read("screen-tiles");
        let shown = Shown::of(&ws, clock()).unwrap();
        let draw = |route: &str| {
            render(
                Some(&shown),
                &ws,
                &Route::parse(route),
                1072.0,
                Covers::Files,
                None,
            )
            .main
        };
        let opens = "<a class=\"tile\" href=\"#/book/B1\"><div class=\"cover\">";
        let named = "<p class=\"title\" lang=\"en\">A Map of Small Rivers</p>\
                     <p class=\"author\" lang=\"en\">Someone</p>";
        // `/books` and `/journal` state the whole record, `/week/` and `/day/` their own days.
        for (route, stands) in [
            ("/books", "40m · 40%"),
            ("/journal", "40m · 40%"),
            ("/week/2026-09-01", "10m"),
            ("/day/2026-09-01", "10m"),
        ] {
            let main = draw(route);
            assert!(main.contains(opens), "{route}: {main}");
            assert!(main.contains(named), "{route}");
            assert!(
                main.contains(&format!(
                    "<p class=\"meta\">{stands}</p><p class=\"meta\">1 highlight</p></a>"
                )),
                "{route}: {main}"
            );
        }
        let journal = draw("/journal");
        assert!(journal.contains("<section class=\"run\"><aside><a class=\"tile\""));
        assert!(journal.contains("<p class=\"meta\">Last read Oct 1, 2026</p></aside>"));
    }

    #[test]
    fn text_is_tagged_with_the_convention_its_book_and_characters_name() {
        assert_eq!(language_of("", "川辺の記録"), "ja", "kana");
        assert_eq!(language_of("", "盛世"), "zh-Hans", "bare Han");
        assert_eq!(language_of("zh-TW", "盛世"), "zh-Hant");
        assert_eq!(language_of("ja", "盛世"), "ja", "the tag before the text");
        assert_eq!(language_of("en", "The Iliad"), "en");
        assert_eq!(lang("", "The Iliad"), "", "nothing names a language");
    }

    #[test]
    fn a_record_with_no_sitting_opens_on_the_welcome() {
        let ws = Workspace::open(&scratch("screen-empty")).unwrap();
        assert!(Shown::of(&ws, clock()).is_none());
        let page = render(
            None,
            &ws,
            &Route::Today,
            1072.0,
            Covers::Window,
            Some("G0001234"),
        );
        let w = Words::of(Lang::English);
        assert!(page.main.contains(w.drop_hint), "{}", page.main);
        assert!(
            page.main.contains("data-serial=\"G0001234\""),
            "the Kindle plugged in"
        );
        assert!(!page.side.contains("class=\"nav\""), "no screen to go to");
        assert!(
            !page.side.contains("data-act"),
            "the welcome holds the Kindle and the picker"
        );
    }

    #[test]
    fn a_book_whose_cover_is_held_here_stands_where_books_without_one_are_hidden() {
        let mut ws = read("screen-uncovered");
        // Each book names its cover by the Kindle's own path; `B1` alone has a
        // copy on this computer.
        let mut record = Record::load(&ws.record_dir());
        for book in &mut record.books {
            book.cover = format!("/mnt/us/extensions/readinglog/covers/{}.jpg", book.cde_key);
        }
        record.save(&ws.record_dir()).unwrap();
        let cover = ws.record_dir().join("covers").join("B1.jpg");
        std::fs::create_dir_all(cover.parent().unwrap()).unwrap();
        std::fs::write(&cover, b"\xff\xd8\xff\xe0 a cover").unwrap();
        ws.config.show_uncovered = "no".into();
        let shown = Shown::of(&ws, clock()).unwrap();
        let draw = |route: &str| {
            render(
                Some(&shown),
                &ws,
                &Route::parse(route),
                1072.0,
                Covers::Files,
                None,
            )
            .main
        };
        for route in ["/books", "/week/2026-09-01"] {
            let main = draw(route);
            assert!(main.contains("A Map of Small Rivers"), "{route}: {main}");
            assert!(main.contains("src=\"file://"), "{route}: its cover drawn");
        }
        assert!(!draw("/books").contains("川辺の記録"), "no cover anywhere");
    }

    #[test]
    fn a_backup_states_what_it_holds_against_its_own_kindle_and_carries_its_delete() {
        use readinglog_native::update::archive::{self, Source};
        let ws = read("screen-backups");
        let text = Record::load(&ws.record_dir()).text();
        // `about` states `written` and `serial` alone.
        let about = "written\t261015:200600\nserial\tG003H2-0001\n";
        let zip = scratch("screen-backups-zip").join("readinglog-261015-200600-kindle.zip");
        archive::write(
            &zip,
            &[
                ("sessions.tsv".to_string(), Source::Bytes(text.as_bytes())),
                ("about.tsv".to_string(), Source::Bytes(about.as_bytes())),
            ],
        )
        .unwrap();
        crate::import::backup(&ws, &zip).unwrap();
        let shown = Shown::of(&ws, clock()).unwrap();
        let page = render(
            Some(&shown),
            &ws,
            &Route::Settings,
            1072.0,
            Covers::Files,
            None,
        )
        .main;
        assert!(
            page.contains("3 sittings · 2026-09-01 to 2026-10-01 · all of it in the record"),
            "{page}"
        );
        let kept = ws
            .kindle_dir("G003H2-0001")
            .join("backups")
            .join("readinglog-261015-200600-kindle.zip");
        let delete = format!(
            "data-act=\"delete\" data-path=\"{}\"",
            esc(&kept.to_string_lossy())
        );
        assert!(page.contains(&delete), "{page}");
        assert!(page.contains("data-act=\"delete-all\""));
        assert!(page.contains("data-act=\"reset\""));
    }

    #[test]
    fn a_corrected_note_is_drawn_once_across_imports_of_its_kindles_own_words() {
        use readinglog_native::clippings::Kind;
        use readinglog_native::highlights::Highlight;
        use readinglog_native::update::archive::{self, Source};
        let ws = Workspace::create(&scratch("screen-corrected")).unwrap();
        let title = "A Map of Small Rivers";
        let passage = Highlight {
            start: 10,
            end: 40,
            body: "a passage about rivers".into(),
            ..highlight(100, title, "2026-09-01T09:05:00")
        };
        let note = |words: &str| Highlight {
            kind: Kind::Note,
            start: 38,
            end: 40,
            body: words.into(),
            ..highlight(100, title, "2026-09-01T09:05:10")
        };
        // One backup of the Kindle `G003H2-0001`, written at `stamp`.
        let import = |stamp: &str, highlights: Vec<Highlight>| {
            let mut record = record_of(
                vec![book(100, "B1", title)],
                vec![sitting("2026-09-01T09:00:00", 100, 600)],
            );
            record.highlights = highlights;
            let text = record.text();
            let about = format!("written\t{stamp}\nserial\tG003H2-0001\n");
            let zip = scratch("screen-corrected-zip").join(format!("readinglog-{stamp}.zip"));
            archive::write(
                &zip,
                &[
                    ("sessions.tsv".to_string(), Source::Bytes(text.as_bytes())),
                    ("about.tsv".to_string(), Source::Bytes(about.as_bytes())),
                ],
            )
            .unwrap();
            crate::import::backup(&ws, &zip).unwrap();
        };
        let page = || {
            let shown = Shown::of(&ws, clock()).unwrap();
            render(
                Some(&shown),
                &ws,
                &Route::parse("/book/B1"),
                1072.0,
                Covers::Files,
                None,
            )
            .main
        };
        let (typo, fixed) = ("letting everyone done?", "letting everyone down?");
        import("261015-200600", vec![passage.clone(), note(typo)]);
        crate::edits::correct_words(
            &ws,
            "G003H2-0001",
            &[(HighlightId::of(&note(typo)), Some(fixed.into()))],
        )
        .unwrap();
        let drawn = page();
        assert_eq!(drawn.matches(fixed).count(), 1, "{drawn}");
        assert!(!drawn.contains(typo));
        assert_eq!(drawn.matches("<div class=\"highlight\"").count(), 1);
        assert!(drawn.contains(" data-edited"));
        assert!(drawn.contains("data-kindle=\"G003H2-0001\""));
        assert!(drawn.contains("data-note-at=\"2026-09-01T09:05:10\""));

        // `import` again: the record states the words the correction was set
        // over, beside a highlight made since.
        let since = Highlight {
            start: 900,
            end: 960,
            body: "a highlight made since".into(),
            ..highlight(100, title, "2026-09-03T21:00:00")
        };
        import(
            "261016-200600",
            vec![passage.clone(), note(typo), since.clone()],
        );
        let drawn = page();
        assert_eq!(drawn.matches(fixed).count(), 1, "{drawn}");
        assert!(!drawn.contains(typo));
        assert_eq!(drawn.matches("<div class=\"highlight\"").count(), 2);

        // `import` with the note reworded on the Kindle: its words stand over the correction.
        import("261017-200600", vec![passage, note("down, at last"), since]);
        let drawn = page();
        assert!(drawn.contains("down, at last"), "{drawn}");
        assert!(!drawn.contains(fixed));
        assert!(!drawn.contains(" data-edited"));
    }

    #[test]
    fn a_cover_is_named_for_the_window_or_for_the_disk() {
        let ws = read("screen-covers");
        let cover = ws.record_dir().join("covers").join("B1.jpg");
        std::fs::create_dir_all(cover.parent().unwrap()).unwrap();
        std::fs::write(&cover, b"\xff\xd8\xff\xe0 a cover").unwrap();
        let shown = Shown::of(&ws, clock()).unwrap();
        let route = Route::parse("/book/B1");
        let window = render(Some(&shown), &ws, &route, 1072.0, Covers::Window, None);
        let path = esc(&cover.to_string_lossy());
        assert!(
            window.main.contains(&format!("data-cover=\"{path}\"")),
            "{}",
            window.main
        );
        let files = render(Some(&shown), &ws, &route, 1072.0, Covers::Files, None);
        assert!(files.main.contains(&format!("src=\"file://{path}\"")));
    }

    #[test]
    fn a_kindle_plugged_in_is_carded_by_its_model_with_try_again_hidden() {
        let ws = read("screen-card");
        let shown = Shown::of(&ws, clock()).unwrap();
        let serial = Some("G003H2-7890");
        let page = render(
            Some(&shown),
            &ws,
            &Route::Today,
            1072.0,
            Covers::Window,
            serial,
        );
        let w = Words::of(Lang::English);
        assert!(
            page.side
                .contains("<b>Kindle Colorsoft</b><p class=\"state\"></p>"),
            "{}",
            page.side
        );
        let again = format!(
            "data-serial=\"G003H2-7890\" hidden>{}</button>",
            w.try_again
        );
        assert!(page.side.contains(&again), "{}", page.side);
    }

    /// [`read`] with the Kindle's own note on its one passage.
    fn noted(name: &str) -> Workspace {
        let ws = read(name);
        let mut record = Record::load(&ws.record_dir());
        let mut note = highlight(100, "A Map of Small Rivers", "2026-09-01T09:06:00");
        note.kind = readinglog_native::clippings::Kind::Note;
        note.body = "The Kindle's own".into();
        record.highlights.push(note);
        record.save(&ws.record_dir()).unwrap();
        ws
    }

    fn drawn(ws: &Workspace, route: &str) -> String {
        let shown = Shown::of(ws, clock()).unwrap();
        render(
            Some(&shown),
            ws,
            &Route::parse(route),
            1072.0,
            Covers::Files,
            None,
        )
        .main
    }

    #[test]
    fn a_note_written_here_stands_under_its_passage_or_alone_with_its_day() {
        use readinglog_native::written::{Note, Passage};
        let ws = noted("screen-notes");
        let passage = highlight(100, "A Map of Small Rivers", "2026-09-01T09:05:00");
        let note = |written: &str, text: &str, on: bool| Note {
            written: written.into(),
            key: "B1".into(),
            extent: 100,
            text: text.into(),
            on: on.then(|| Passage {
                id: HighlightId::of(&passage),
                from: 0,
                to: 0,
                words: "Some words".into(),
            }),
            ..Note::default()
        };
        // Before any note is written here, the Kindle's note states no day.
        let page = drawn(&ws, "/book/B1");
        assert!(
            page.contains("<p class=\"note\">The Kindle's own</p><p class=\"where\">"),
            "{page}"
        );
        let mut record = Record::load(&ws.record_dir());
        record.add_note(note("2026-10-01T10:00:00", "On the passage", true));
        record.add_note(note("2026-10-02T08:00:00", "On the book alone", false));
        record.save(&ws.record_dir()).unwrap();
        let page = drawn(&ws, "/book/B1");
        assert!(
            page.contains(
                "The Kindle's own</p><p class=\"stamp\">Sep 1, 2026</p>\
                 <div class=\"dnote\" data-written=\"2026-10-01T10:00:00\">"
            ),
            "{page}"
        );
        assert!(
            page.contains("<p>On the passage</p>\n</div><p class=\"stamp\">Oct 1, 2026<button")
        );
        assert!(page.contains(
            "<div class=\"highlight alone\" data-kindle=\"\" data-written=\"2026-10-02T08:00:00\">"
        ));
        assert!(!page.contains("class=\"edited\""), "no row is marked");
        // A day lists a passage made before it when a note on it was written then.
        let day = drawn(&ws, "/day/2026-10-01");
        assert!(day.contains("On the passage"), "{day}");
        assert!(day.contains("<span>Journal<span class=\"count\">1</span></span>"));
    }

    #[test]
    fn a_note_written_here_is_drawn_as_its_markdown_and_names_no_place() {
        use readinglog_native::written::Note;
        let ws = noted("screen-markdown");
        let mut record = Record::load(&ws.record_dir());
        record.add_note(Note {
            written: "2026-10-16T20:00:00".into(),
            key: "B1".into(),
            extent: 100,
            text: "# I don’t like this book\n\nvery incoherent, a bundle of *scripts*.\n\n\
                   > Some words\\\n> — Location 120\n\n<b>bold</b>"
                .into(),
            ..Note::default()
        });
        record.save(&ws.record_dir()).unwrap();
        let page = drawn(&ws, "/book/B1");
        assert!(
            page.contains(
                "<h1>I don’t like this book</h1>\n<p>very incoherent, a bundle of <em>scripts</em>.</p>"
            ),
            "{page}"
        );
        assert!(page.contains("data-text=\"# I don’t like this book\n\nvery incoherent"));
        assert!(page.contains("<span class=\"place\">— Location 120</span>"));
        assert!(
            page.contains("&lt;b&gt;bold&lt;/b&gt;"),
            "HTML in a note is its text"
        );
        assert!(
            page.contains("<p class=\"where\"><span></span>"),
            "no place on the book alone"
        );
        assert!(
            !page.contains("id=\"posts\"") && !page.contains("#/post"),
            "{page}"
        );
        // `q` washes the words and leaves every tag as it stands.
        let found = drawn(&ws, "/book/B1?q=incoherent");
        assert!(found.contains("very <mark>incoherent</mark>, a"), "{found}");
        let p = drawn(&ws, "/book/B1?q=p");
        assert!(p.contains("<p>") && !p.contains("<<mark>"), "{p}");
    }
}
