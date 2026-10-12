//! The window's screens: each [`Route`] drawn as HTML off the record as of its
//! newest sitting, the charts inline SVG, every figure a Kindle view's own and
//! every day, span and book a link to its own page.

mod book;
mod books;
mod chart;
mod day;
mod highlights;
mod look;
mod rhythm;
mod settings;
mod words;

use std::fmt::Write as _;
use std::path::{Path, PathBuf};

use readinglog_native::covers;
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
/// `/week/`, `/month/` and `/year/` with a date (`<yyyy-mm>` and `<yyyy>` read
/// as the first day), `/all`, `/books`, `/book/<key>`, `/highlights`, `/settings`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Route {
    /// The record's newest day, as the Kindle's Today states it.
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
        /// Which end the highlights open at.
        way: Way,
        query: String,
    },
    Highlights(String),
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
            ["highlights"] => Route::Highlights(asked("q")),
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
            Route::Highlights(search) => format!("#/highlights{}", query(&[("q", search.clone())])),
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

/// The record as every screen states it: `Stats` as of the newest sitting
/// under the settings `Workspace::site` names, and each book's cover and tone.
pub struct Shown {
    pub record: Record,
    pub stats: Stats,
    pub settings: Settings,
    pub lang: Lang,
    pub as_of: i64,
    pub covers: Vec<Option<PathBuf>>,
    pub tones: Vec<[u8; 3]>,
}

impl Shown {
    /// `None` where the record holds no sitting.
    pub fn of(ws: &Workspace) -> Option<Shown> {
        let record = ws.record();
        let as_of = record
            .sittings
            .iter()
            .filter_map(|s| date::parse_day(date::day_of(&s.started_at)))
            .max()?;
        let (settings, _) = ws.site.settings();
        let stats = Stats::build(
            &record,
            as_of,
            settings.show_unidentified,
            settings.stats_from,
            settings.sitting_floor,
        );
        let dir = ws.record_dir();
        let covers: Vec<Option<PathBuf>> = stats
            .books
            .iter()
            .map(|b| {
                (!b.cde_key.is_empty() && covers::held(&dir, &b.cde_key))
                    .then(|| covers::path(&dir, &b.cde_key))
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
            lang: ws.site.lang(),
            settings,
            as_of,
            covers,
            tones,
        })
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

/// `route` drawn `width` wide off `shown`, the sidebar beside it. `kindle` is a
/// Kindle mounted with a record on it.
pub fn render(
    shown: Option<&Shown>,
    ws: &Workspace,
    route: &Route,
    width: f32,
    links: Covers,
    kindle: Option<&Path>,
) -> Page {
    let lang = shown.map_or(ws.site.lang(), |s| s.lang);
    let s = lang.strings();
    let w = Words::of(lang);
    let (settings, _) = ws.site.settings();
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
        today: shown.as_of,
        opened: shown.stats.opened(shown.as_of),
        look,
        tones: &shown.tones,
        covers: &shown.covers,
        links,
        uncovered: shown.settings.show_uncovered,
        width: (width - 120.0).max(320.0),
    };
    let route = match route {
        Route::Book { key, .. } if shown.book(key).is_none() => Route::Today,
        other => other.clone(),
    };
    let (title, main) = match &route {
        Route::Today => (s.today.to_string(), day::page(&cx, shown.as_of, true)),
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
        Route::Highlights(query) => (s.highlights_tab.to_string(), highlights::page(&cx, query)),
        Route::Settings => (s.config.to_string(), settings::page(ws, Some(shown), s, w)),
    };
    Page {
        title,
        side: side(Some(shown), &route, s, w, kindle),
        main,
        vars,
        lang: lang.language_tag().into(),
    }
}

/// What every page of one render reads.
struct Ctx<'a> {
    stats: &'a Stats,
    s: &'static Strings,
    lang: Lang,
    week: WeekStart,
    from: StatsFrom,
    /// The record's newest day, which every page reads as today.
    today: i64,
    /// The record's first day with reading on it.
    opened: i64,
    look: Look,
    tones: &'a [[u8; 3]],
    covers: &'a [Option<PathBuf>],
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
/// mounted and the way to a backup; under any record, the way to the settings.
fn side(
    shown: Option<&Shown>,
    route: &Route,
    s: &Strings,
    w: &Words,
    kindle: Option<&Path>,
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
        let today = matches!(route, Route::Today) || *route == Route::Day(shown.as_of);
        item(&mut out, &Route::Today, s.today, today);
        let rhythm = !today && matches!(route, Route::Span(..) | Route::Day(_));
        item(&mut out, &Route::Span(Span::AllTime, 0), s.rhythm, rhythm);
        let books = matches!(route, Route::Books { .. } | Route::Book { .. });
        item(&mut out, &Route::shelf(Shelf::All, None), s.books, books);
        let marks = matches!(route, Route::Highlights(_));
        item(
            &mut out,
            &Route::Highlights(String::new()),
            s.highlights_tab,
            marks,
        );
        let _ = write!(
            out,
            "</div><div class=\"nav years\"><p class=\"label\">{}</p>",
            esc(s.year)
        );
        for year in shown.years() {
            let to = Route::Span(Span::Year, in_year(shown.as_of, year));
            let lit = matches!(route, Route::Span(Span::Year, day) if date::civil_from_days(*day).0 == year);
            item(&mut out, &to, &year.to_string(), lit);
        }
        let all = Route::Span(Span::AllTime, 0);
        item(&mut out, &all, s.all_time, *route == all);
        out.push_str("</div>");
    }
    out.push_str("<div class=\"foot\">");
    if let (Some(_), Some(root)) = (shown, kindle) {
        let _ = write!(
            out,
            "<div class=\"kindle\"><b>Kindle</b>{}<button type=\"button\" class=\"button\" data-act=\"kindle\" data-root=\"{}\">{}</button></div>",
            esc(w.connected),
            esc(&root.to_string_lossy()),
            esc(w.import)
        );
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
            Route::Highlights("a b".into()),
            Route::Highlights(String::new()),
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

    #[test]
    fn every_route_draws_off_the_record_as_of_its_newest_sitting() {
        let ws = read("screen-routes");
        let shown = Shown::of(&ws).unwrap();
        assert_eq!(Some(shown.as_of), date::parse_day("2026-10-01"));
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
            today.main.contains("A Map of Small Rivers"),
            "the day's book"
        );
        assert!(
            today.side.contains("class=\"on\">Today</a>"),
            "{}",
            today.side
        );
        assert!(
            today.side.contains("href=\"#/year/2026-10-01\""),
            "a year opens at the record's newest day"
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
            draw("/highlights?q=words")
                .main
                .contains("Some <mark>words</mark>")
        );
        assert!(!draw("/highlights?q=nothing").main.contains("Some words"));
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
            "/highlights",
        ] {
            let page = draw(route);
            assert!(!page.main.contains(DASH), "{route} states a dash");
            assert!(page.vars.contains("--accent"), "{route}");
        }
    }

    #[test]
    fn a_window_narrows_the_books_to_its_span() {
        let ws = read("screen-window");
        let shown = Shown::of(&ws).unwrap();
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
        let shown = Shown::of(&ws).unwrap();
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
        let shown = Shown::of(&ws).unwrap();
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
        let shown = Shown::of(&ws).unwrap();
        let page = render(
            Some(&shown),
            &ws,
            &Route::parse("/highlights?q=WORDS"),
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
        assert!(page.main.contains("<span>Sep 1, 2026</span></p>"));
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
        assert!(Shown::of(&ws).is_none());
        let kindle = Path::new("/Volumes/Kindle");
        let page = render(
            None,
            &ws,
            &Route::Today,
            1072.0,
            Covers::Window,
            Some(kindle),
        );
        let w = Words::of(Lang::English);
        assert!(page.main.contains(w.drop_hint), "{}", page.main);
        assert!(
            page.main.contains("data-root=\"/Volumes/Kindle\""),
            "the Kindle mounted"
        );
        assert!(!page.side.contains("class=\"nav\""), "no screen to go to");
        assert!(
            !page.side.contains("data-act"),
            "the welcome holds the Kindle and the picker"
        );
    }

    #[test]
    fn a_cover_is_named_for_the_window_or_for_the_disk() {
        let ws = read("screen-covers");
        let cover = ws.record_dir().join("covers").join("B1.jpg");
        std::fs::create_dir_all(cover.parent().unwrap()).unwrap();
        std::fs::write(&cover, b"\xff\xd8\xff\xe0 a cover").unwrap();
        let shown = Shown::of(&ws).unwrap();
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
}
