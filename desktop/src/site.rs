//! The site, built in memory from a workspace: every page, the stylesheet and
//! each shown book's cover, as a path and its bytes, one workspace building one
//! set of bytes.

use std::collections::{BTreeMap, BTreeSet};
use std::fmt::Write as _;
use std::path::{Component, Path, PathBuf};

use anyhow::{Result, bail};

use readinglog_native::date;
use readinglog_native::highlights::Highlight;
use readinglog_native::lang::{self, Lang, Strings};
use readinglog_native::settings::{Settings, StatsFrom, WeekStart};
use readinglog_native::stats::{BookStat, Stats};
use readinglog_native::ui::paint::{self, Palette};
use readinglog_native::view::band::Band;
use readinglog_native::view::{
    self, Span, alltime, book as book_view, book_graphs, highlights as highlights_view, rhythm,
};

use crate::edits::Edits;
use crate::out;
use crate::posts::{self, Post};
use crate::public::Public;
use crate::workspace::{KINDLES, SiteToml, Workspace};

/// A site in memory.
pub struct Site {
    /// Every file, by its `/`-separated path under the site's folder.
    pub files: BTreeMap<String, Vec<u8>>,
    /// Each book on the site, in the order the books page lists them.
    pub books: Vec<Placed>,
    /// The address of each post on the site.
    pub posts: Vec<String>,
    /// Every book, post and highlight the site makes public.
    pub listed: Vec<Listed>,
    /// What the build passed over, a line each.
    pub problems: Vec<String>,
}

impl Site {
    /// [`Self::listed`] as the manifest keeps it.
    pub fn items(&self) -> BTreeSet<String> {
        self.listed.iter().map(|l| l.item.clone()).collect()
    }
}

/// One thing a site makes public.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Listed {
    /// Its manifest line: `book <key>`, `post <address>` or `highlight <identity>`.
    pub item: String,
    /// A book's title or a post's, or the words a highlight states.
    pub said: String,
    /// For a highlight, the title of the book it belongs to.
    pub book: String,
}

/// What writing a workspace's site makes public that its folder's last write
/// did not.
pub struct Plan {
    pub site: Site,
    /// The folder it is written to.
    pub out: PathBuf,
    /// Whether no site has been written there yet.
    pub first: bool,
    /// What it makes public that is not, in the order the site lists it.
    pub fresh: Vec<Listed>,
    /// How many things on the site it takes off.
    pub leaving: usize,
}

/// What [`publish`] makes public, against the folder's manifest.
pub fn plan(ws: &Workspace) -> Result<Plan> {
    let site = build(ws)?;
    let at = ws.out();
    let held = out::manifest(&at)?;
    let first = held.is_none();
    let held = held.map(|m| m.items).unwrap_or_default();
    let fresh = site
        .listed
        .iter()
        .filter(|l| !held.contains(&l.item))
        .cloned()
        .collect();
    let leaving = held.difference(&site.items()).count();
    Ok(Plan {
        site,
        out: at,
        first,
        fresh,
        leaving,
    })
}

/// One book on the site, and the address it was given.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Placed {
    pub key: String,
    pub extent: i64,
    pub title: String,
    pub slug: String,
    /// Whether `books.toml` holds the address.
    pub pinned: bool,
}

/// What [`publish`] did: the site built, and what writing it moved.
pub struct Published {
    pub site: Site,
    pub written: out::Written,
}

/// Build the site `ws` holds and write it to its folder, pinning in `books.toml`
/// each address made from a title.
pub fn publish(ws: &Workspace) -> Result<Published> {
    let at = ws.out();
    if trespasses(ws, &at) {
        bail!(
            "{} is the workspace, holds it, or lies in its record, backups or posts; \
             the site is written to a folder of its own",
            at.display()
        );
    }
    let site = build(ws)?;
    let written = out::write(&at, &site.files, &site.items())?;
    let fresh: Vec<&Placed> = site.books.iter().filter(|p| !p.pinned).collect();
    if !fresh.is_empty() {
        let mut edits = Edits::load(&ws.dir)?;
        for placed in fresh {
            edits.book_mut(&placed.key, placed.extent).slug = Some(placed.slug.clone());
        }
        edits.save(&ws.dir)?;
    }
    Ok(Published { site, written })
}

/// Whether writing a site to `out` could reach what the workspace keeps.
fn trespasses(ws: &Workspace, out: &Path) -> bool {
    let out = lexical(out);
    let home = lexical(&ws.dir);
    out == home
        || home.starts_with(&out)
        || [
            ws.record_dir(),
            ws.backups(),
            ws.posts(),
            ws.dir.join(KINDLES),
        ]
        .iter()
        .any(|kept| out.starts_with(lexical(kept)))
}

/// `path` made whole against the working folder, its `.` and `..` resolved
/// by name alone.
fn lexical(path: &Path) -> PathBuf {
    let whole = match path.is_absolute() {
        true => path.to_path_buf(),
        false => std::env::current_dir().unwrap_or_default().join(path),
    };
    let mut out = PathBuf::new();
    for part in whole.components() {
        match part {
            Component::CurDir => {}
            Component::ParentDir => {
                out.pop();
            }
            other => out.push(other),
        }
    }
    out
}

/// Build the site `ws` holds.
pub fn build(ws: &Workspace) -> Result<Site> {
    let (settings, mut problems) = ws.site.settings();
    let edits = Edits::load(&ws.dir)?;
    let sources = ws.sources();
    let dirs: Vec<PathBuf> = sources.iter().map(|s| s.dir.clone()).collect();
    let public = Public::of(
        &crate::kindles::together(&sources),
        &dirs,
        &edits,
        &settings,
        ws.site.new_books,
    );
    let (posts, unread) = posts::load(&ws.posts());
    problems.extend(unread);
    let placed = places(&public.stats, &edits);
    let posts: Vec<(Post, Vec<usize>)> = posts
        .into_iter()
        .filter(|post| !post.draft)
        .map(|post| {
            let books = named(&post, &placed, &mut problems);
            (post, books)
        })
        .collect();
    let covers: Vec<Option<(String, Vec<u8>)>> = (0..public.stats.books.len())
        .map(|at| cover_file(&public.stats.books[at], &placed[at].slug))
        .collect();
    let cx = Ctx {
        site: &ws.site,
        s: ws.site.lang().strings(),
        lang: ws.site.lang(),
        week: settings.week_start,
        from: settings.stats_from,
        stats: &public.stats,
        as_of: public.as_of,
        edits: &edits,
        placed: &placed,
        covered: covers
            .iter()
            .map(|c| c.as_ref().map(|(name, _)| name.clone()))
            .collect(),
        posts: &posts,
    };

    let mut files: BTreeMap<String, Vec<u8>> = BTreeMap::new();
    files.insert(".nojekyll".into(), Vec::new());
    files.insert("style.css".into(), style(&settings).into_bytes());
    files.insert("index.html".into(), cx.index());
    files.insert("books/index.html".into(), cx.books());
    for (at, cover) in covers.into_iter().enumerate() {
        files.insert(format!("books/{}/index.html", placed[at].slug), cx.book(at));
        if let Some((name, bytes)) = cover {
            files.insert(format!("covers/{name}"), bytes);
        }
    }
    for year in cx.years() {
        files.insert(format!("year/{year}/index.html"), cx.year(year));
    }
    for (post, books) in &posts {
        files.insert(
            format!("posts/{}/index.html", post.slug),
            cx.post(post, books),
        );
    }
    match cx.feed() {
        Some(feed) => {
            files.insert("feed.xml".into(), feed);
        }
        None if !posts.is_empty() => problems.push(format!(
            "{}: no url, so the posts have no feed",
            crate::workspace::SITE_FILE
        )),
        None => {}
    }
    let listed = cx.listed();
    Ok(Site {
        files,
        posts: posts.iter().map(|(post, _)| post.slug.clone()).collect(),
        books: placed,
        listed,
        problems,
    })
}

/// The manifest's line for a book.
fn book_item(key: &str, extent: i64) -> String {
    match key.is_empty() {
        false => format!("book {key}"),
        true => format!("book extent:{extent}"),
    }
}

/// The manifest's line for a highlight.
fn highlight_item(highlight: &Highlight) -> String {
    let (at, kind, title, location, start) = highlight.identity();
    format!(
        "highlight {at}\t{}\t{title}\t{location}\t{start}",
        kind.as_str()
    )
}

/// Each book's address: the one `books.toml` holds, else one made from the
/// title shown that no other book holds.
fn places(stats: &Stats, edits: &Edits) -> Vec<Placed> {
    let mut taken: BTreeSet<String> = BTreeSet::new();
    let mut out: Vec<Option<Placed>> = vec![None; stats.books.len()];
    for (at, book) in stats.books.iter().enumerate() {
        let pinned = edits
            .book(&book.cde_key, book.extent)
            .and_then(|e| e.slug.as_deref())
            .map(posts::slug)
            .filter(|slug| !slug.is_empty() && !taken.contains(slug));
        if let Some(slug) = pinned {
            taken.insert(slug.clone());
            out[at] = Some(placed(book, edits, slug, true));
        }
    }
    // `rest` in key order.
    let mut rest: Vec<usize> = (0..stats.books.len())
        .filter(|at| out[*at].is_none())
        .collect();
    rest.sort_by(|a, b| {
        let (a, b) = (&stats.books[*a], &stats.books[*b]);
        (&a.cde_key, a.extent).cmp(&(&b.cde_key, b.extent))
    });
    for at in rest {
        let book = &stats.books[at];
        let title = title_of(book, edits);
        // `named` is empty for a title `posts::latin` rejects, and `keyed` stands.
        let named = match posts::latin(&title) {
            true => posts::slug(&title),
            false => String::new(),
        };
        let keyed: String = posts::slug(&book.cde_key).chars().take(KEYED).collect();
        let stem = [named, keyed]
            .into_iter()
            .find(|s| !s.is_empty())
            .unwrap_or_else(|| format!("book-{}", book.extent));
        let mut slug = stem.clone();
        let mut step = 2;
        while taken.contains(&slug) {
            slug = format!("{stem}-{step}");
            step += 1;
        }
        taken.insert(slug.clone());
        out[at] = Some(placed(book, edits, slug, false));
    }
    out.into_iter()
        .map(|p| p.expect("every book placed"))
        .collect()
}

/// The most of a key an address made from it keeps.
const KEYED: usize = 12;

fn placed(book: &BookStat, edits: &Edits, slug: String, pinned: bool) -> Placed {
    Placed {
        key: book.cde_key.clone(),
        extent: book.extent,
        title: title_of(book, edits),
        slug,
        pinned,
    }
}

/// The title the site states for `book`: its row's, else the record's.
fn title_of(book: &BookStat, edits: &Edits) -> String {
    edits
        .book(&book.cde_key, book.extent)
        .and_then(|e| e.title.clone())
        .unwrap_or_else(|| book.title.clone())
}

/// The author the site states for `book`.
fn author_of(book: &BookStat, edits: &Edits) -> String {
    edits
        .book(&book.cde_key, book.extent)
        .and_then(|e| e.author.clone())
        .unwrap_or_else(|| book.author.clone())
}

/// The books `post` names, by their place among `placed`, and in `problems` a
/// line for each name reaching no book on the site.
fn named(post: &Post, placed: &[Placed], problems: &mut Vec<String>) -> Vec<usize> {
    let mut out = Vec::new();
    for name in &post.books {
        let name = name.trim();
        let found = placed.iter().position(|p| {
            (!p.key.is_empty() && p.key == name)
                || p.slug == name
                || (p.key.is_empty() && p.extent.to_string() == name)
        });
        match found {
            Some(at) if !out.contains(&at) => out.push(at),
            Some(_) => {}
            None => problems.push(format!(
                "posts/{}: \"{name}\" is no book on the site",
                post.slug
            )),
        }
    }
    out
}

/// The cover `book` names, as its file name under `covers/` and its bytes.
fn cover_file(book: &BookStat, slug: &str) -> Option<(String, Vec<u8>)> {
    if !book.has_cover() {
        return None;
    }
    let bytes = std::fs::read(&book.thumbnail).ok()?;
    let kind = match bytes.starts_with(b"\x89PNG") {
        true => "png",
        false => "jpg",
    };
    Some((format!("{slug}.{kind}"), bytes))
}

/// What every page is drawn from.
struct Ctx<'a> {
    site: &'a SiteToml,
    s: &'static Strings,
    lang: Lang,
    week: WeekStart,
    from: StatsFrom,
    stats: &'a Stats,
    as_of: Option<i64>,
    edits: &'a Edits,
    placed: &'a [Placed],
    /// Each book's cover, by its file name under `covers/`.
    covered: Vec<Option<String>>,
    posts: &'a [(Post, Vec<usize>)],
}

impl Ctx<'_> {
    /// Every book, highlight and post the site states: the books in the order
    /// the books page lists them, each one's highlights under it, then the
    /// posts.
    fn listed(&self) -> Vec<Listed> {
        let mut out = Vec::new();
        for (at, book) in self.stats.books.iter().enumerate() {
            let title = title_of(book, self.edits);
            out.push(Listed {
                item: book_item(&book.cde_key, book.extent),
                said: title.clone(),
                book: String::new(),
            });
            for row in self.stats.book_highlights(at) {
                for highlight in [Some(row.highlight), row.note].into_iter().flatten() {
                    out.push(Listed {
                        item: highlight_item(highlight),
                        said: self.words(highlight).to_string(),
                        book: title.clone(),
                    });
                }
            }
        }
        for (post, _) in self.posts {
            out.push(Listed {
                item: format!("post {}", post.slug),
                said: post.title.clone(),
                book: String::new(),
            });
        }
        out
    }

    /// The words `highlight` states on the site: its row's correction, else the
    /// record's own.
    fn words<'h>(&'h self, highlight: &'h Highlight) -> &'h str {
        self.edits
            .highlight(highlight)
            .and_then(|e| e.text.as_deref())
            .unwrap_or(&highlight.body)
    }

    /// Every year with reading in it, newest first.
    fn years(&self) -> Vec<i64> {
        let mut out: Vec<i64> = self
            .stats
            .days
            .iter()
            .map(|(day, _)| date::civil_from_days(*day).0)
            .collect();
        out.dedup();
        out.reverse();
        out
    }

    /// A whole page: `body` under the site's head, `depth` folders under the
    /// site's root, its title `title` where it has one of its own.
    fn page(&self, depth: usize, title: Option<&str>, body: &str) -> Vec<u8> {
        let root = "../".repeat(depth);
        let site = esc(&self.site.title);
        let title = match title {
            Some(title) => format!("{} · {site}", esc(title)),
            None => site.clone(),
        };
        let mut nav = format!(
            "<a href=\"{root}books/index.html\">{}</a>",
            esc(self.s.books)
        );
        for year in self.years() {
            let _ = write!(nav, "<a href=\"{root}year/{year}/index.html\">{year}</a>");
        }
        let by = match self.site.author.trim() {
            "" => String::new(),
            author => format!("<span class=\"by\">{}</span>", esc(author)),
        };
        let feed = match self.home().is_some() && !self.posts.is_empty() {
            true => format!(
                "<link rel=\"alternate\" type=\"application/atom+xml\" title=\"{site}\" href=\"{root}feed.xml\">\n"
            ),
            false => String::new(),
        };
        format!(
            "<!doctype html>\n\
             <html lang=\"{lang}\">\n\
             <head>\n\
             <meta charset=\"utf-8\">\n\
             <meta name=\"viewport\" content=\"width=device-width, initial-scale=1\">\n\
             <title>{title}</title>\n\
             <link rel=\"stylesheet\" href=\"{root}style.css\">\n\
             {feed}</head>\n\
             <body>\n\
             <header class=\"site\">\n\
             <p class=\"name\"><a href=\"{root}index.html\">{site}</a>{by}</p>\n\
             <nav>{nav}</nav>\n\
             </header>\n\
             <main>\n{body}</main>\n\
             <footer>{footer}</footer>\n\
             </body>\n\
             </html>\n",
            lang = self.lang.language_tag(),
            footer = esc(&self.since()),
        )
        .into_bytes()
    }

    /// Where the site is served from, ending in a `/`. `None` where
    /// `site.toml` names nowhere.
    fn home(&self) -> Option<String> {
        match self.site.url.trim() {
            "" => None,
            url if url.ends_with('/') => Some(url.to_string()),
            url => Some(format!("{url}/")),
        }
    }

    /// The posts as an Atom feed, newest first, each whole, its links made whole
    /// against [`Self::home`]: `None` without one. Dated by its newest post,
    /// never by the clock.
    fn feed(&self) -> Option<Vec<u8>> {
        let home = esc(&self.home()?);
        let (newest, _) = self.posts.first()?;
        let site = esc(&self.site.title);
        let author = match self.site.author.trim() {
            "" => site.clone(),
            author => esc(author),
        };
        let mut out = format!(
            "<?xml version=\"1.0\" encoding=\"utf-8\"?>\n\
             <feed xmlns=\"http://www.w3.org/2005/Atom\" xml:lang=\"{lang}\">\n\
             <title>{site}</title>\n\
             <link href=\"{home}\"/>\n\
             <link rel=\"self\" href=\"{home}feed.xml\"/>\n\
             <id>{home}</id>\n\
             <updated>{updated}T00:00:00Z</updated>\n\
             <author><name>{author}</name></author>\n",
            lang = self.lang.language_tag(),
            updated = iso(newest.date),
        );
        for (post, _) in self.posts {
            let _ = write!(
                out,
                "<entry>\n\
                 <title>{title}</title>\n\
                 <link href=\"{home}posts/{slug}/index.html\"/>\n\
                 <id>{home}posts/{slug}/</id>\n\
                 <updated>{updated}T00:00:00Z</updated>\n\
                 <content type=\"html\">{content}</content>\n\
                 </entry>\n",
                title = esc(&post.title),
                slug = post.slug,
                updated = iso(post.date),
                content = esc(&posts::html(&post.body)),
            );
        }
        out.push_str("</feed>\n");
        Some(out.into_bytes())
    }

    /// What the record covers: the month it opens in and the days since, as
    /// the All Time board states it.
    fn since(&self) -> String {
        let Some(as_of) = self.as_of else {
            return String::new();
        };
        let first = self.stats.opened(as_of);
        let (year, month, _) = date::civil_from_days(first);
        lang::counted(self.s.since_days, self.stats.covered(as_of))
            .replace("{m}", &date::month_name(year, month, self.s).to_uppercase())
    }

    fn index(&self) -> Vec<u8> {
        let mut body = String::new();
        if !self.posts.is_empty() {
            body.push_str("<section class=\"posts\">\n");
            for (post, books) in self.posts {
                body.push_str(&self.post_summary(post, books, 0));
            }
            body.push_str("</section>\n");
        }
        let Some(as_of) = self.as_of else {
            let _ = writeln!(body, "<p class=\"bare\">{}</p>", esc(self.s.nothing_read));
            return self.page(0, None, &body);
        };
        let week = Span::Week.days(as_of, self.week);
        let _ = write!(
            body,
            "<section>\n<h2 class=\"span\">{}</h2>\n",
            esc(&Span::Week.name(as_of, self.week, self.s))
        );
        body.push_str(&self.what_was_read(week, 0));
        body.push_str("</section>\n");

        let year = date::civil_from_days(as_of).0;
        let _ = write!(
            body,
            "<section>\n<h2 class=\"span\"><a href=\"year/{year}/index.html\">{}</a></h2>\n{}</section>\n",
            esc(&Span::Year.name(as_of, self.week, self.s)),
            figures(
                &rhythm::tallied(self.stats, Span::Year.days(as_of, self.week), self.s),
                "four"
            ),
        );
        let _ = write!(
            body,
            "<section>\n<h2 class=\"span\">{}</h2>\n{}</section>\n",
            esc(self.s.all_time),
            figures(
                &alltime::board(self.stats, as_of, self.week, self.s),
                "board"
            ),
        );
        self.page(0, None, &body)
    }

    /// The books read over `days`, most recently put down first, each with
    /// what was read of it there, under the total.
    fn what_was_read(&self, days: std::ops::RangeInclusive<i64>, depth: usize) -> String {
        let total = date::duration(self.stats.span_seconds(days.clone()), self.s);
        let mut out = format!(
            "<h3 class=\"head\">{} · {}</h3>\n<ul class=\"books\">\n",
            esc(self.s.what_was_read),
            esc(&total)
        );
        for (at, secs) in self.stats.book_totals_recent(days.clone()) {
            out.push_str(&self.book_row(at, secs, depth, Some(days.clone())));
        }
        out.push_str("</ul>\n");
        out
    }

    /// One book as a row of a list: its cover, title and author, `secs`, and how
    /// far through it is: under a span, `over`, where it stood as the span closed,
    /// as the Kindle's cover grid states it; over the whole record, its band.
    fn book_row(
        &self,
        at: usize,
        secs: i64,
        depth: usize,
        over: Option<std::ops::RangeInclusive<i64>>,
    ) -> String {
        let book = &self.stats.books[at];
        let root = "../".repeat(depth);
        let highlights = view::highlights_said(self.s, self.stats.highlights_counted(at));
        let (reached, band) = match over {
            Some(days) => (
                self.stats
                    .percent_over(at, days)
                    .map(|p| lang::counted(self.s.percent_reached, p))
                    .unwrap_or_default(),
                String::new(),
            ),
            None => (String::new(), self.band(book)),
        };
        let under = [author_of(book, self.edits), highlights, reached]
            .into_iter()
            .filter(|said| !said.is_empty())
            .map(|said| esc(&said))
            .collect::<Vec<_>>()
            .join(" · ");
        format!(
            "<li><a class=\"row\" href=\"{root}books/{slug}/index.html\">{cover}\
             <span class=\"words\"><span class=\"title\"{lang}>{title}</span>\
             <span class=\"under\">{under}</span>{band}</span>\
             <span class=\"time\">{time}</span></a></li>\n",
            slug = self.placed[at].slug,
            cover = self.cover(at, &root, "cover"),
            lang = lang_attr(&book.language),
            title = esc(&title_of(book, self.edits)),
            time = esc(&date::duration(secs, self.s)),
        )
    }

    /// `book`'s cover, or an empty box the size of one.
    fn cover(&self, at: usize, root: &str, class: &str) -> String {
        match &self.covered[at] {
            Some(name) => format!(
                "<img class=\"{class}\" src=\"{root}covers/{name}\" alt=\"\" loading=\"lazy\">"
            ),
            None => format!("<span class=\"{class} none\"></span>"),
        }
    }

    /// How far through `book` is, as `view::band` draws it: the track filled
    /// to `BookStat::bar_percent`, the per cent stated, and a book read through
    /// marked.
    fn band(&self, book: &BookStat) -> String {
        let of = Band::of(book, book.is_finished());
        if !book.has_percent() && !of.finished {
            return String::new();
        }
        let at = of
            .at
            .map(|at| lang::counted(self.s.percent_plain, at))
            .unwrap_or_default();
        let mark = match of.finished {
            true => format!("<span class=\"chip\">{}</span>", esc(self.s.shelf_finished)),
            false => String::new(),
        };
        format!(
            "<span class=\"band\"><span class=\"track\"><span class=\"fill\" style=\"width:{}%\"></span></span>\
             <span class=\"at\">{}</span>{mark}</span>",
            of.fill.clamp(0, 100),
            esc(&at)
        )
    }

    fn books(&self) -> Vec<u8> {
        let mut body = format!("<h1>{}</h1>\n<ul class=\"books\">\n", esc(self.s.books));
        for at in 0..self.stats.books.len() {
            let secs = self.stats.books[at].read_seconds(self.from);
            body.push_str(&self.book_row(at, secs, 1, None));
        }
        body.push_str("</ul>\n");
        self.page(1, Some(self.s.books), &body)
    }

    fn book(&self, at: usize) -> Vec<u8> {
        let book = &self.stats.books[at];
        let s = self.s;
        let title = title_of(book, self.edits);
        let author = author_of(book, self.edits);
        // `s.left` stands only on a book short of `BookStat::is_finished`.
        let headline: Vec<(String, &'static str)> = book_view::headline(book, self.from, s)
            .into_iter()
            .filter(|(_, name)| !(book.is_finished() && *name == s.left))
            .collect();
        let mut body = format!(
            "<article class=\"book\">\n<header class=\"book\">{cover}<div class=\"words\">\
             <h1{lang}>{title}</h1>{by}{figures}{band}</div></header>\n",
            cover = self.cover(at, "../../", "cover"),
            lang = lang_attr(&book.language),
            title = esc(&title),
            by = match author.is_empty() {
                true => String::new(),
                false => format!("<p class=\"author\">{}</p>", esc(&author)),
            },
            figures = figures(&headline, "three"),
            band = self.band(book),
        );

        let mine: Vec<&(Post, Vec<usize>)> = self
            .posts
            .iter()
            .filter(|(_, books)| books.contains(&at))
            .collect();
        if !mine.is_empty() {
            body.push_str("<section class=\"posts\">\n");
            for (post, books) in mine {
                body.push_str(&self.post_summary(post, books, 2));
            }
            body.push_str("</section>\n");
        }

        let _ = write!(
            body,
            "<section>\n<h2 class=\"head\">{}</h2>\n<dl class=\"rows\">\n",
            esc(s.the_reading)
        );
        let rows = book_view::reading_rows(book, self.from, s);
        for (name, value) in rows.iter().filter(|(_, value)| value != book_view::DASH) {
            let _ = writeln!(
                body,
                "<div><dt>{}</dt><dd>{}</dd></div>",
                esc(name),
                esc(value)
            );
        }
        body.push_str("</dl>\n</section>\n");

        let reads = self.stats.book_readings(at);
        if !reads.is_empty() {
            let _ = write!(
                body,
                "<section>\n<h2 class=\"head\">{}</h2>\n<ol class=\"readings\">\n",
                esc(s.read_through_band)
            );
            for (on, (from, to)) in reads.iter().enumerate() {
                let whole = book_graphs::whole(on, reads.len(), book.finished);
                let secs = self.stats.book_seconds_over(at, *from..=*to);
                let _ = writeln!(
                    body,
                    "<li><span>{}</span><span class=\"time\">{}</span></li>",
                    esc(&book_graphs::spanned(*from, *to, whole, s)),
                    esc(&date::duration(secs, s))
                );
            }
            body.push_str("</ol>\n</section>\n");
        }

        let rows = self.stats.book_highlights(at);
        if !rows.is_empty() {
            let counted = view::highlights_said(s, self.stats.highlights_counted(at));
            let _ = write!(
                body,
                "<section>\n<h2 class=\"head\">{}<span class=\"stated\">{}</span></h2>\n<ol class=\"highlights\">\n",
                esc(s.highlights_tab),
                esc(&counted)
            );
            for row in rows {
                body.push_str(&self.highlight(book, row.highlight, row.note));
            }
            body.push_str("</ol>\n</section>\n");
        }
        body.push_str("</article>\n");
        self.page(2, Some(&title), &body)
    }

    /// One row of a book's highlights: the passage, the note written on it, its
    /// comment, where and when it was made, and under a corrected passage the
    /// record's own words one tap away.
    fn highlight(
        &self,
        book: &BookStat,
        highlight: &Highlight,
        note: Option<&Highlight>,
    ) -> String {
        let lang = lang_attr(&book.language);
        let mut out = String::from("<li>");
        let said = self.words(highlight);
        let class = match highlight.kind.is_the_readers_own() {
            true => "note",
            false => "passage",
        };
        let _ = write!(
            out,
            "<blockquote class=\"{class}\"{lang}>{}</blockquote>",
            lines(said)
        );
        if said != highlight.body {
            let _ = write!(
                out,
                "<details class=\"original\"><summary>{}</summary><blockquote{lang}>{}</blockquote></details>",
                esc(original(self.lang)),
                lines(&highlight.body)
            );
        }
        if let Some(note) = note {
            let _ = write!(out, "<p class=\"note\">{}</p>", lines(self.words(note)));
        }
        let comments = [Some(highlight), note]
            .into_iter()
            .flatten()
            .filter_map(|h| self.edits.highlight(h)?.comment.as_deref())
            .filter(|c| !c.trim().is_empty());
        for comment in comments {
            let _ = write!(out, "<div class=\"comment\">{}</div>", posts::html(comment));
        }
        let when = highlight
            .day()
            .map(|day| {
                format!(
                    "<time datetime=\"{}\">{}</time>",
                    iso(day),
                    esc(&date::year_day(day, self.s))
                )
            })
            .unwrap_or_default();
        let _ = writeln!(
            out,
            "<p class=\"label\"><span>{}</span>{when}</p></li>",
            esc(&highlights_view::label(highlight, book.extent, self.s))
        );
        out
    }

    fn year(&self, year: i64) -> Vec<u8> {
        let day = date::days_from_civil(year, 1, 1);
        let days = Span::Year.days(day, self.week);
        let name = Span::Year.name(day, self.week, self.s);
        let mut body = format!(
            "<h1>{}</h1>\n{}",
            esc(&name),
            figures(&rhythm::tallied(self.stats, days.clone(), self.s), "four")
        );
        body.push_str("<section>\n");
        body.push_str(&self.what_was_read(days, 2));
        body.push_str("</section>\n");
        self.page(2, Some(&name), &body)
    }

    /// A post in a list: its title, its day and the books it names.
    fn post_summary(&self, post: &Post, books: &[usize], depth: usize) -> String {
        let root = "../".repeat(depth);
        format!(
            "<article class=\"summary\"><h2><a href=\"{root}posts/{slug}/index.html\">{title}</a></h2>\
             <p class=\"meta\">{when}{named}</p></article>\n",
            slug = post.slug,
            title = esc(&post.title),
            when = self.when(post.date),
            named = self.named(books, &root),
        )
    }

    /// `day` as a `<time>`.
    fn when(&self, day: i64) -> String {
        format!(
            "<time datetime=\"{}\">{}</time>",
            iso(day),
            esc(&date::year_day(day, self.s))
        )
    }

    /// The books at `books`, each a link to its page, after a separator.
    fn named(&self, books: &[usize], root: &str) -> String {
        books
            .iter()
            .map(|at| {
                let book = &self.stats.books[*at];
                format!(
                    " · <a href=\"{root}books/{}/index.html\"{}>{}</a>",
                    self.placed[*at].slug,
                    lang_attr(&book.language),
                    esc(&title_of(book, self.edits))
                )
            })
            .collect()
    }

    fn post(&self, post: &Post, books: &[usize]) -> Vec<u8> {
        let body = format!(
            "<article class=\"post\">\n<h1>{title}</h1>\n<p class=\"meta\">{when}{named}</p>\n\
             <div class=\"text\">\n{text}</div>\n</article>\n",
            title = esc(&post.title),
            when = self.when(post.date),
            named = self.named(books, "../../"),
            text = posts::html(&post.body),
        );
        self.page(2, Some(&post.title), &body)
    }
}

/// `stated` as a row of figures, each over its name.
fn figures(stated: &[(String, &'static str)], class: &str) -> String {
    let mut out = format!("<dl class=\"figures {class}\">");
    for (value, name) in stated.iter().filter(|(value, _)| value != book_view::DASH) {
        let _ = write!(
            out,
            "<div><dt>{}</dt><dd>{}</dd></div>",
            esc(name),
            esc(value)
        );
    }
    out.push_str("</dl>\n");
    out
}

/// What the summary over a corrected passage's own words reads.
fn original(lang: Lang) -> &'static str {
    match lang {
        Lang::English => "Original",
        Lang::German => "Original",
        Lang::Japanese | Lang::SimplifiedChinese | Lang::TraditionalChinese => "原文",
    }
}

/// `day` as `YYYY-MM-DD`.
fn iso(day: i64) -> String {
    let (y, m, d) = date::civil_from_days(day);
    format!("{y:04}-{m:02}-{d:02}")
}

/// ` lang="…"` for a library language tag, nothing for one that is no tag.
fn lang_attr(language: &str) -> String {
    let tag = language.trim().replace('_', "-");
    match !tag.is_empty() && tag.chars().all(|c| c.is_ascii_alphanumeric() || c == '-') {
        true => format!(" lang=\"{tag}\""),
        false => String::new(),
    }
}

/// `text` with a line break wherever it breaks a line.
fn lines(text: &str) -> String {
    esc(text.trim()).replace('\n', "<br>")
}

/// `text` with the characters HTML reads as markup written as references.
pub(crate) fn esc(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for c in text.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\'' => out.push_str("&#39;"),
            c => out.push(c),
        }
    }
    out
}

/// `#rrggbb`.
fn hex([r, g, b]: [u8; 3]) -> String {
    format!("#{r:02x}{g:02x}{b:02x}")
}

/// The stylesheet, in the colours of the scheme `settings` names.
fn style(settings: &Settings) -> String {
    let palette = Palette::of(settings.color_scheme);
    let mut out = String::from(":root {\n");
    for (name, ink) in [
        ("ink", paint::INK),
        ("dark", paint::DARK),
        ("light", paint::LIGHT),
        ("pale", paint::PALE),
        ("paper", paint::WHITE),
    ] {
        let _ = writeln!(out, "  --{name}: {};", hex([ink; 3]));
    }
    for (at, step) in palette.steps.iter().enumerate() {
        let _ = writeln!(out, "  --step-{}: {};", at + 1, hex(*step));
    }
    let _ = writeln!(out, "  --mark: {};", hex(palette.mark));
    out.push_str("}\n");
    out.push_str(STYLE);
    out
}

/// Everything in the stylesheet past its colours.
const STYLE: &str = include_str!("style.css");

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tests::{book, highlight, record_of, sitting};

    fn workspace(name: &str) -> Workspace {
        let ws = Workspace::create(&crate::tests::scratch(name)).unwrap();
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
    fn a_site_holds_a_page_for_each_book_year_and_post() {
        let ws = workspace("site-pages");
        std::fs::write(
            ws.posts().join("first.md"),
            "+++\ntitle = \"On the Rivers\"\ndate = 2026-10-02\nbooks = [\"B1\", \"nothing\"]\n+++\nGood.\n",
        )
        .unwrap();
        std::fs::write(
            ws.posts().join("later.md"),
            "+++\ntitle = \"Not yet\"\ndate = 2026-10-03\ndraft = true\n+++\n",
        )
        .unwrap();
        let site = build(&ws).unwrap();
        let paths: Vec<&str> = site.files.keys().map(String::as_str).collect();
        assert_eq!(
            paths,
            [
                ".nojekyll",
                "books/a-map-of-small-rivers/index.html",
                "books/b2/index.html",
                "books/index.html",
                "index.html",
                "posts/first/index.html",
                "style.css",
                "year/2026/index.html",
            ]
        );
        assert_eq!(site.posts, ["first"]);
        // A post naming no book on the site, and a site with no url for a feed.
        assert_eq!(site.problems.len(), 2, "{:?}", site.problems);
        let book = String::from_utf8(site.files["books/a-map-of-small-rivers/index.html"].clone())
            .unwrap();
        assert!(
            book.contains("On the Rivers"),
            "the review is on its book's page"
        );
        assert!(!book.contains("Some words"), "a highlight starts hidden");
        let cat = String::from_utf8(site.files["books/b2/index.html"].clone()).unwrap();
        assert!(cat.contains("<h1 lang=\"en\">川辺の記録</h1>"));
    }

    #[test]
    fn a_figure_with_nothing_to_state_is_left_off_the_page() {
        let ws = workspace("site-blank");
        let site = build(&ws).unwrap();
        let page = String::from_utf8(site.files["books/a-map-of-small-rivers/index.html"].clone())
            .unwrap();
        let left = format!("<dt>{}</dt>", Lang::English.strings().left);
        assert!(!page.contains(&left), "{page}");
        assert!(!page.contains(book_view::DASH), "{page}");
    }

    #[test]
    fn a_finished_book_states_nothing_left() {
        let ws = Workspace::create(&crate::tests::scratch("site-finished")).unwrap();
        let mut done = book(100, "B1", "A Map of Small Rivers");
        done.percent = 100.0;
        let record = record_of(vec![done], vec![sitting("2026-09-01T09:00:00", 100, 600)]);
        record.save(&ws.record_dir()).unwrap();
        let site = build(&ws).unwrap();
        let page = String::from_utf8(site.files["books/a-map-of-small-rivers/index.html"].clone())
            .unwrap();
        let s = Lang::English.strings();
        assert!(page.contains(&format!("<dt>{}</dt>", s.read)), "{page}");
        assert!(!page.contains(&format!("<dt>{}</dt>", s.left)), "{page}");
    }

    #[test]
    fn one_workspace_builds_one_set_of_bytes() {
        let ws = workspace("site-same");
        assert_eq!(build(&ws).unwrap().files, build(&ws).unwrap().files);
    }

    #[test]
    fn a_pinned_address_stands_and_a_made_one_steps_around_it() {
        let ws = workspace("site-slugs");
        let mut edits = Edits::default();
        edits.book_mut("B2", 200).slug = Some("a-map-of-small-rivers".into());
        edits.book_mut("B1", 100).title = Some("Retitled".into());
        edits.save(&ws.dir).unwrap();
        let site = build(&ws).unwrap();
        let slugs: BTreeMap<&str, (&str, bool)> = site
            .books
            .iter()
            .map(|p| (p.key.as_str(), (p.slug.as_str(), p.pinned)))
            .collect();
        assert_eq!(slugs["B2"], ("a-map-of-small-rivers", true));
        assert_eq!(slugs["B1"], ("retitled", false));
    }

    #[test]
    fn a_corrected_highlight_keeps_the_records_words_a_tap_away() {
        let ws = workspace("site-highlight");
        let record = ws.record();
        let mut edits = Edits::default();
        edits.book_mut("B1", 100).highlights = Some(true);
        let row = edits.highlight_mut(&record.highlights[0]);
        row.text = Some("Some better words".into());
        row.comment = Some("A *fine* line.".into());
        edits.save(&ws.dir).unwrap();
        let site = build(&ws).unwrap();
        let page = String::from_utf8(site.files["books/a-map-of-small-rivers/index.html"].clone())
            .unwrap();
        assert!(page.contains("Some better words"));
        assert!(page.contains("<details class=\"original\"><summary>Original</summary><blockquote lang=\"en\">Some words</blockquote>"));
        assert!(page.contains("<em>fine</em>"));
    }

    #[test]
    fn a_published_address_outlives_a_corrected_title() {
        let mut ws = workspace("site-publish");
        let first = publish(&ws).unwrap();
        assert!(first.written.changed.contains(&"index.html".to_string()));
        let edits = Edits::load(&ws.dir).unwrap();
        let pinned = edits.book("B1", 100).and_then(|b| b.slug.clone());
        assert_eq!(pinned.as_deref(), Some("a-map-of-small-rivers"));

        let mut edits = edits;
        edits.book_mut("B1", 100).title = Some("A Map of Smaller Rivers".into());
        edits.save(&ws.dir).unwrap();
        let second = publish(&ws).unwrap();
        assert!(
            second.written.removed.is_empty(),
            "{:?}",
            second.written.removed
        );
        let page = std::fs::read_to_string(ws.out().join("books/a-map-of-small-rivers/index.html"))
            .unwrap();
        assert!(page.contains("A Map of Smaller Rivers"));

        for kept in ["record", "backups/new", "posts", ".", ".."] {
            ws.site.out = kept.into();
            assert!(publish(&ws).is_err(), "{kept}");
        }
    }

    #[test]
    fn a_publish_names_first_what_it_makes_public_that_was_not() {
        let ws = workspace("site-plan");
        let first = plan(&ws).unwrap();
        assert!(first.first);
        let said: Vec<&str> = first.fresh.iter().map(|l| l.said.as_str()).collect();
        assert_eq!(said, ["A Map of Small Rivers", "川辺の記録"]);
        publish(&ws).unwrap();
        let again = plan(&ws).unwrap();
        assert!(!again.first && again.fresh.is_empty() && again.leaving == 0);

        let record = ws.record();
        let mut edits = Edits::load(&ws.dir).unwrap();
        edits.book_mut("B1", 100).highlights = Some(true);
        edits.book_mut("B2", 200).shown = Some(false);
        edits.save(&ws.dir).unwrap();
        let next = plan(&ws).unwrap();
        assert_eq!(next.fresh.len(), 1);
        assert_eq!(next.fresh[0].said, record.highlights[0].body);
        assert_eq!(next.fresh[0].book, "A Map of Small Rivers");
        assert_eq!(next.leaving, 1);
    }

    #[test]
    fn the_posts_are_a_feed_dated_by_the_newest() {
        let mut ws = workspace("site-feed");
        ws.site.url = "https://reader.example/reading".into();
        ws.save_site().unwrap();
        for (name, date) in [("older", "2026-09-20"), ("newer", "2026-10-02")] {
            std::fs::write(
                ws.posts().join(format!("{name}.md")),
                format!(
                    "+++\ntitle = \"{name} & more\"\ndate = {date}\n+++\nA <b>bold</b> line.\n"
                ),
            )
            .unwrap();
        }
        let site = build(&ws).unwrap();
        assert!(site.problems.is_empty(), "{:?}", site.problems);
        let feed = String::from_utf8(site.files["feed.xml"].clone()).unwrap();
        assert!(feed.contains("<updated>2026-10-02T00:00:00Z</updated>\n<author>"));
        assert!(
            feed.contains("<link href=\"https://reader.example/reading/posts/newer/index.html\"/>")
        );
        assert!(feed.contains("<title>newer &amp; more</title>"));
        assert!(feed.contains("&lt;b&gt;bold&lt;/b&gt;"));
        assert!(feed.find("newer").unwrap() < feed.find("older").unwrap());
        let index = String::from_utf8(site.files["index.html"].clone()).unwrap();
        assert!(index.contains("type=\"application/atom+xml\""));
    }

    #[test]
    fn markup_in_the_record_is_text_on_the_page() {
        assert_eq!(
            esc("<b>\"A&B\"</b>"),
            "&lt;b&gt;&quot;A&amp;B&quot;&lt;/b&gt;"
        );
        assert_eq!(lang_attr("en_US"), " lang=\"en-US\"");
        assert_eq!(lang_attr("x\" onload=\""), "");
    }
}
