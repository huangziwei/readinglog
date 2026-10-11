//! The window's screens: each [`Route`] drawn as HTML off the record as of its
//! newest sitting, the charts inline SVG, every figure a Kindle view's own.

mod book;
mod books;
mod chart;
mod highlights;
mod look;
mod rhythm;
mod settings;
mod words;

use std::fmt::Write as _;
use std::path::{Path, PathBuf};

use readinglog_native::covers;
use readinglog_native::date;
use readinglog_native::lang::{Lang, Strings};
use readinglog_native::settings::{Settings, StatsFrom, WeekStart};
use readinglog_native::stats::Stats;
use readinglog_native::ui::paint::Palette;
use readinglog_native::view::{Shelf, Sort};

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
}

/// Where a page is asked for, as the window's hash states it: `/books`,
/// `/year/2026`, `/book/<key>`, and so on.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Route {
    Home,
    Books {
        shelf: Shelf,
        order: Sort,
        query: String,
    },
    Year(i64),
    AllTime,
    Book(String),
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
        let parts: Vec<&str> = path.trim_matches('/').split('/').collect();
        match parts.as_slice() {
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
                query: asked("q"),
            },
            ["year", year] => year.parse().map_or(Route::Home, Route::Year),
            ["all"] => Route::AllTime,
            ["book", key] => Route::Book(unescape(key)),
            ["highlights"] => Route::Highlights(asked("q")),
            ["settings"] => Route::Settings,
            _ => Route::Home,
        }
    }

    /// The hash that asks for this route.
    pub fn link(&self) -> String {
        match self {
            Route::Home => "#/".into(),
            Route::Books {
                shelf,
                order,
                query,
            } => {
                let shelf = match shelf {
                    Shelf::All => "all",
                    Shelf::Finished => "finished",
                    Shelf::Unfinished => "unfinished",
                };
                let order = match order {
                    Sort::Recent => "recent",
                    Sort::Time => "time",
                    Sort::Progress => "progress",
                };
                match query.is_empty() {
                    true => format!("#/books?shelf={shelf}&order={order}"),
                    false => format!("#/books?shelf={shelf}&order={order}&q={}", escape(query)),
                }
            }
            Route::Year(year) => format!("#/year/{year}"),
            Route::AllTime => "#/all".into(),
            Route::Book(key) => format!("#/book/{}", escape(key)),
            Route::Highlights(query) => match query.is_empty() {
                true => "#/highlights".into(),
                false => format!("#/highlights?q={}", escape(query)),
            },
            Route::Settings => "#/settings".into(),
        }
    }
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
            Route::Settings => settings::page(ws, s, w),
            _ => settings::welcome(w, kindle),
        };
        return Page {
            title: "Reading Log".into(),
            side: side(None, route, s, w, kindle),
            main,
            vars,
        };
    };
    let cx = Ctx {
        stats: &shown.stats,
        s,
        w,
        lang,
        week: shown.settings.week_start,
        from: shown.settings.stats_from,
        today: shown.as_of,
        look,
        tones: &shown.tones,
        covers: &shown.covers,
        links,
        uncovered: shown.settings.show_uncovered,
        width: (width - 120.0).max(320.0),
    };
    let latest = shown.years().first().copied().unwrap_or(0);
    let route = match route {
        Route::Home => Route::Year(latest),
        Route::Book(key) if shown.book(key).is_none() => Route::Year(latest),
        other => other.clone(),
    };
    let (title, main) = match &route {
        Route::Books {
            shelf,
            order,
            query,
        } => (s.books.to_string(), books::page(&cx, *shelf, *order, query)),
        Route::Year(year) => (year.to_string(), rhythm::year(&cx, *year)),
        Route::AllTime => (
            s.all_time.to_string(),
            rhythm::all_time(&cx, &shown.years()),
        ),
        Route::Book(key) => {
            let at = shown.book(key).unwrap_or(0);
            (shown.stats.books[at].title.clone(), book::page(&cx, at))
        }
        Route::Highlights(query) => (s.highlights_tab.to_string(), highlights::page(&cx, query)),
        Route::Settings | Route::Home => (s.config.to_string(), settings::page(ws, s, w)),
    };
    Page {
        title,
        side: side(Some(shown), &route, s, w, kindle),
        main,
        vars,
    }
}

/// What every page of one render reads.
struct Ctx<'a> {
    stats: &'a Stats,
    s: &'static Strings,
    w: &'static Words,
    lang: Lang,
    week: WeekStart,
    from: StatsFrom,
    today: i64,
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
        Route::Book(key_of(&self.stats.books[at])).link()
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
            "<div class=\"made\" style=\"background: linear-gradient(160deg, rgb({r},{g},{b}), rgb({},{},{}))\">\
             <span{}>{}</span><small>{}</small></div>",
            r / 2,
            g / 2,
            b / 2,
            lang(&book.language),
            esc(&book.title),
            esc(&book.author)
        )
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
        let latest = shown.years().first().copied().unwrap_or(0);
        out.push_str("<div class=\"nav\">");
        let rhythm = matches!(route, Route::Year(_) | Route::AllTime);
        item(&mut out, &Route::Year(latest), s.rhythm, rhythm);
        let books = matches!(route, Route::Books { .. } | Route::Book(_));
        item(&mut out, &Route::parse("/books"), s.books, books);
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
            item(
                &mut out,
                &Route::Year(year),
                &year.to_string(),
                *route == Route::Year(year),
            );
        }
        item(
            &mut out,
            &Route::AllTime,
            s.all_time,
            *route == Route::AllTime,
        );
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

/// A `lang` attribute stating `tag`, or nothing where `tag` is blank.
fn lang(tag: &str) -> String {
    match tag.trim().is_empty() {
        true => String::new(),
        false => format!(" lang=\"{}\"", esc(tag.trim())),
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
        let routes = [
            Route::Home,
            Route::Books {
                shelf: Shelf::Finished,
                order: Sort::Time,
                query: "川辺 & co".into(),
            },
            Route::Books {
                shelf: Shelf::All,
                order: Sort::Recent,
                query: String::new(),
            },
            Route::Year(2026),
            Route::AllTime,
            Route::Book("*AB/CD?".into()),
            Route::Highlights("a b".into()),
            Route::Highlights(String::new()),
            Route::Settings,
        ];
        for route in routes {
            let link = route.link();
            assert_eq!(Route::parse(link.trim_start_matches('#')), route, "{link}");
        }
        assert_eq!(Route::parse("/nowhere"), Route::Home);
        assert_eq!(Route::parse("/year/soon"), Route::Home);
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
        let year = draw("/");
        assert_eq!(year.title, "2026");
        assert!(year.main.contains("<svg"), "the year's strip");
        assert!(year.side.contains("class=\"on\">2026</a>"), "{}", year.side);
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
        assert!(draw("/highlights?q=words").main.contains("Some words"));
        assert!(!draw("/highlights?q=nothing").main.contains("Some words"));
        assert!(draw("/settings").main.contains("data-set=\"figures\""));
        for route in ["/", "/books", "/book/B1", "/book/B2", "/all", "/highlights"] {
            let page = draw(route);
            assert!(!page.main.contains(DASH), "{route} states a dash");
            assert!(page.vars.contains("--accent"), "{route}");
        }
    }

    #[test]
    fn a_book_no_route_names_draws_the_year() {
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
        assert_eq!(page.title, "2026");
    }

    #[test]
    fn a_record_with_no_sitting_opens_on_the_welcome() {
        let ws = Workspace::open(&scratch("screen-empty")).unwrap();
        assert!(Shown::of(&ws).is_none());
        let kindle = Path::new("/Volumes/Kindle");
        let page = render(
            None,
            &ws,
            &Route::Home,
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
