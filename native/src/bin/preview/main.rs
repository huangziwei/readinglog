//! Every screen drawn to a PNG, with no display behind it. A shot names a
//! screen and, after a colon, what it is showing: `rhythm:year`, `book:3`.
//! `--list` names them all, `--sheet` sheets a run, `--crop` cuts each shot.

mod fixture;
mod sheet;
mod sketch;

use std::path::{Path, PathBuf};

use anyhow::{Context, Result, anyhow, bail};

use readinglog_native::app::App;
use readinglog_native::date;
use readinglog_native::eink::fb::Framebuffer;
use readinglog_native::lang::Lang;
use readinglog_native::record::Record;
use readinglog_native::settings::{
    ColorScheme, Scope, SittingFloor, StatsFrom, TextSize, WeekStart,
};
use readinglog_native::stats::Stats;
use readinglog_native::ui::chrome::Tab;
use readinglog_native::ui::paint;
use readinglog_native::ui::splash;
use readinglog_native::ui::text::TextRenderer;
use readinglog_native::ui::theme::Theme;
use readinglog_native::update::{Doing, Failure, Outcome};
use readinglog_native::view::{Act, Ask, BookTab, Search, Shelf, Sort, Span, Way, Window};

/// The day the preview is set to, and the second of it.
const DAY: (i64, i64, i64) = (2026, 9, 16);
const NOW: i64 = 20 * 3600 + 15 * 60;

/// The panels, by the name `--panel` takes: the first drawn where a run names
/// none and untagged by `panel_tag`, each at `ui::scale`'s density; `--panel
/// WxH` draws any other size.
const PANELS: &[(&str, u32, u32)] = &[
    // Colorsoft, Oasis 2-3.
    ("pw", 1264, 1680),
    ("scribe", 1860, 2480),
    // Paperwhite 5-6.
    ("pw5", 1236, 1648),
    // Voyage, Paperwhite 3-4, Basic 4.
    ("voyage", 1072, 1448),
    // Paperwhite 1-2, at 212 ppi.
    ("pw2", 758, 1024),
    // Kindle Touch, Basic 1-2, at 167 ppi.
    ("basic", 600, 800),
];

/// Where the PNGs land under `--out`.
const OUT: &str = "artifacts/preview";

/// Where the fixture's covers are drawn, under `--covers`, apart from [`OUT`].
const COVERS: &str = "artifacts/preview/art";

/// The update banner a shot can name, and what it is saying. The one screen
/// with no tab under it, drawn over the whole panel.
const BANNERS: &[(&str, Said)] = &[
    ("asking", || Banner::Doing(Doing::Asking)),
    ("downloading", || {
        Banner::Doing(Doing::Downloading {
            got: 1_400_000,
            total: Some(3_300_000),
        })
    }),
    ("checking", || Banner::Doing(Doing::Checking)),
    ("done", || {
        Banner::Ended(Outcome::Installed("v0.2.0".into()))
    }),
    ("current", || Banner::Ended(Outcome::UpToDate)),
    ("offline", || Banner::Ended(Outcome::Offline)),
    ("failed", || {
        Banner::Ended(Outcome::Failed(Failure::WrongBuild))
    }),
];

/// The banner over a running act, by the name an `act:<of>` shot takes.
const ACTS: &[(&str, Act)] = &[
    ("reset", Act::Reset(true)),
    ("restore", Act::Restore(0)),
    ("rebuild", Act::Rebuild),
];

/// What one of [`BANNERS`] is saying, made when a shot names it.
type Said = fn() -> Banner;

/// Either half of the banner's life: running, or over.
enum Banner {
    Doing(Doing),
    Ended(Outcome),
}

/// The screens a shot can name, and the tab each sits under.
const SCREENS: &[(&str, Tab)] = &[
    ("config", Tab::Config),
    ("today", Tab::Home),
    ("rhythm", Tab::Rhythm),
    ("books", Tab::Books),
    ("book", Tab::Books),
    ("search", Tab::Books),
    ("journal", Tab::Books),
];

/// One picture to draw: a screen or a sketch, and what it is showing.
struct Shot {
    name: String,
    of: Option<String>,
}

impl Shot {
    fn read(spec: &str) -> Self {
        match spec.split_once(':') {
            Some((name, of)) => Self {
                name: name.into(),
                of: Some(of.into()),
            },
            None => Self {
                name: spec.into(),
                of: None,
            },
        }
    }

    /// What the file is called, and what the sheet captions it.
    fn label(&self) -> String {
        match &self.of {
            Some(of) => format!("{}-{of}", self.name),
            None => self.name.clone(),
        }
    }
}

/// What the run was asked for.
struct Opts {
    shots: Vec<Shot>,
    panels: Vec<(u32, u32)>,
    langs: Vec<Lang>,
    sizes: Vec<TextSize>,
    week: WeekStart,
    day: i64,
    out: PathBuf,
    covers: PathBuf,
    /// A record to draw in place of the fixture, by the directory holding it.
    record: Option<PathBuf>,
    sheet: Option<String>,
    scale: u32,
    crop: Option<sheet::Crop>,
    /// Whether every hit box is outlined over the frame.
    hits: bool,
    /// The scheme drawn in.
    scheme: ColorScheme,
    /// What `eink::fb::has_cfa` is drawn as answering.
    colour: bool,
    /// Whether a total counts the sittings on unidentified books.
    unidentified: bool,
    /// The shortest sitting a total counts as reading.
    floor: SittingFloor,
    /// Which of the two sources the stats are taken from.
    stats_from: StatsFrom,
    /// Whether a list holds the books no cover can be drawn for.
    uncovered: bool,
    /// Whether every shot draws `fixture::written`, notes written on the desktop.
    written: bool,
}

impl Default for Opts {
    fn default() -> Self {
        Self {
            shots: Vec::new(),
            panels: vec![(PANELS[0].1, PANELS[0].2)],
            langs: vec![Lang::English],
            sizes: vec![TextSize::Medium],
            week: WeekStart::Monday,
            scheme: ColorScheme::default(),
            colour: true,
            day: date::days_from_civil(DAY.0, DAY.1, DAY.2),
            out: PathBuf::from(OUT),
            covers: PathBuf::from(COVERS),
            record: None,
            sheet: None,
            scale: 40,
            crop: None,
            hits: false,
            unidentified: true,
            floor: SittingFloor::default(),
            stats_from: StatsFrom::default(),
            uncovered: true,
            written: false,
        }
    }
}

fn main() {
    if let Err(err) = run() {
        eprintln!("!! preview: {err:#}");
        std::process::exit(1);
    }
}

fn run() -> Result<()> {
    let opts = read_args(std::env::args().skip(1))?;
    if opts.shots.is_empty() {
        list();
        return Ok(());
    }
    let started = std::time::Instant::now();
    std::fs::create_dir_all(&opts.out).context("make the output directory")?;
    std::fs::create_dir_all(&opts.covers).context("make the cover directory")?;
    let standing = shots_in(&opts.out);

    let library = match &opts.record {
        Some(dir) => Record::load(dir),
        None => fixture::library(opts.day, &opts.covers),
    };
    let mut tiles: Vec<sheet::Tile> = Vec::new();
    let mut wrote: Vec<PathBuf> = Vec::new();
    for (w, h) in opts.panels.iter().copied() {
        for lang in opts.langs.iter().copied() {
            for size in opts.sizes.iter().copied() {
                for shot in &opts.shots {
                    let record = thinned_for(shot, &opts, &opts.covers, lang);
                    let record = record.as_ref().unwrap_or(&library);
                    let mut fb = Framebuffer::offscreen(w, h);
                    let mut app = open(record, &opts, w, h, lang, size)?;
                    if let Some(dir) = backups_for(shot, record, &opts.out)? {
                        app.set_dir(dir);
                    }
                    draw(&mut app, &mut fb, shot, opts.week)?;
                    if opts.hits {
                        outline_hits(&app, &mut fb);
                    }
                    let path = opts.out.join(format!(
                        "{}{}{}{}.png",
                        shot.label(),
                        panel_tag(w, h),
                        lang_tag(lang, &opts),
                        size_tag(size, &opts),
                    ));
                    let name = path
                        .file_stem()
                        .map(|s| s.to_string_lossy().into_owned())
                        .unwrap_or_default();
                    let tile = sheet::Tile::of(name, &fb);
                    let tile = match opts.crop {
                        Some(crop) => tile.cropped(crop),
                        None => tile,
                    };
                    tile.save(&path).context("write the shot")?;
                    println!("{}", path.display());
                    wrote.push(path);
                    if opts.sheet.is_some() {
                        tiles.push(tile);
                    }
                }
            }
        }
    }

    if let Some(name) = &opts.sheet {
        let theme = Theme::for_screen(PANELS[0].1, PANELS[0].2);
        let mut text = TextRenderer::load(theme.body_px).context(FONTS)?;
        let sheet = sheet::compose(&tiles, opts.scale, &mut text, theme.small_px);
        let path = opts.out.join(format!("{name}.png"));
        sheet.capture_png(&path).context("write the sheet")?;
        println!("{}", path.display());
        wrote.push(path);
    }
    eprintln!(
        "preview: {} shots in {:.1}s → {}",
        wrote.len(),
        started.elapsed().as_secs_f32(),
        opts.out.display()
    );
    // The PNGs `standing` held that `wrote` names none of.
    let stale = standing.iter().filter(|p| !wrote.contains(p)).count();
    if stale > 0 {
        eprintln!(
            "preview: and {stale} older PNG{} beside them — `--out DIR` gives a round \
             its own directory",
            if stale == 1 { "" } else { "s" }
        );
    }
    Ok(())
}

/// The PNGs `dir` holds when the run opens.
fn shots_in(dir: &Path) -> Vec<PathBuf> {
    std::fs::read_dir(dir)
        .into_iter()
        .flatten()
        .flatten()
        .map(|entry| entry.path())
        .filter(|path| path.extension().is_some_and(|ext| ext == "png"))
        .collect()
}

/// What to say when no face was found.
const FONTS: &str = "no font — point READINGLOG_FONTS at the Kindle's font directory";

/// An [`App`] over `record`, at the panel and the settings the run names.
fn open(record: &Record, opts: &Opts, w: u32, h: u32, lang: Lang, size: TextSize) -> Result<App> {
    // The fixture identifies a book; `opts.record` may name an empty record.
    if opts.record.is_none()
        && Stats::build(
            record,
            opts.day,
            true,
            StatsFrom::Kindle,
            readinglog_native::settings::SittingFloor::All,
        )
        .books
        .is_empty()
    {
        bail!("the fixture identified no book");
    }
    let theme = Theme::sized(w, h, size);
    let text = TextRenderer::load(theme.body_px).context(FONTS)?;
    let mut app = App::new(record.clone(), theme, text);
    app.set_clock(opts.day, NOW);
    app.set_language(lang);
    app.set_text_size(size);
    app.set_week_start(opts.week);
    app.set_colour(opts.colour);
    app.set_color_scheme(opts.scheme);
    app.set_unidentified(opts.unidentified);
    app.set_sitting_floor(opts.floor);
    app.set_stats_from(opts.stats_from);
    app.set_uncovered(opts.uncovered);
    Ok(app)
}

/// The record a shot wants, where it wants one of its own, drawn in `lang`.
fn thinned_for(shot: &Shot, opts: &Opts, covers: &Path, lang: Lang) -> Option<Record> {
    let keep = match (shot.name.as_str(), shot.of.as_deref()) {
        // Notes written on the desktop, in the language the shot is drawn in.
        (name, of)
            if opts.written || (name == "book" && of.is_some_and(|of| of.contains(":written"))) =>
        {
            return Some(fixture::written(opts.day, covers, lang == Lang::Japanese));
        }
        ("today", Some("quiet")) => 1,
        ("today", Some("empty")) => 0,
        ("today", Some("busy")) => return Some(fixture::crowded(opts.day, covers)),
        // A record that was reset, which is what offers the rebuild from the logs.
        (
            "config",
            Some("restore" | "rebuild" | "many" | "many2" | "backups" | "backups2" | "gone"),
        ) => {
            let mut record = fixture::library(opts.day, covers);
            record.reset_at = "260810:120000".into();
            return Some(record);
        }
        // The book listed last, cleared and kept, sorting last either way and
        // leaving every other index where it stands.
        ("book" | "books", Some(of)) if of.ends_with("cleared") => {
            let mut record = fixture::library(opts.day, covers);
            let last = Stats::build(
                &record,
                opts.day,
                true,
                StatsFrom::Kindle,
                readinglog_native::settings::SittingFloor::All,
            )
            .books
            .last()?
            .clone();
            let (extent, key) = (last.extent, last.cde_key);
            record.watermark = "260916:200000".into();
            record.clear_book(extent, &key);
            return Some(record);
        }
        _ => return None,
    };
    Some(fixture::thinned(opts.day, covers, keep))
}

/// A directory of backups for the shots that draw them, under `out`, each
/// written by `backup::keep_record`.
fn backups_for(shot: &Shot, record: &Record, out: &Path) -> Result<Option<PathBuf>> {
    let stamps: &[&str] = match (shot.name.as_str(), shot.of.as_deref()) {
        ("config", Some("restore")) => &["260906:010231", "260830:184500"],
        ("config", Some("gone")) => &["260906:010231"],
        ("config", Some("many" | "many2" | "backups" | "backups2")) => &[
            "260906:010231",
            "260830:184500",
            "260412:093000",
            "260311:220000",
            "260228:071500",
        ],
        _ => return Ok(None),
    };
    let dir = out.join("record");
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir)?;
    for stamp in stamps {
        readinglog_native::backup::keep_record(&dir, record, stamp, false)
            .map_err(|e| anyhow!("write a backup: {e}"))?;
    }
    Ok(Some(dir))
}

/// Set `app` to what `shot` names and draw it into `fb`.
fn draw(app: &mut App, fb: &mut Framebuffer, shot: &Shot, week: WeekStart) -> Result<()> {
    if let Some(sketch) = sketch::ALL.iter().find(|s| s.name == shot.name) {
        app.show(sketch.tab, None);
        set_span(app, shot, week)?;
        let state = app.state().clone();
        let draw = sketch.draw;
        return app.frame(fb, &mut |cx, area| draw(cx, area, &state));
    }
    if shot.name == "update" {
        return banner(app, fb, shot.of.as_deref().unwrap_or("asking"));
    }
    if shot.name == "act" {
        return acting(app, fb, shot.of.as_deref().unwrap_or("rebuild"));
    }
    let Some((_, tab)) = SCREENS.iter().find(|(name, _)| *name == shot.name) else {
        return Err(anyhow!("no screen or sketch called {}", shot.name));
    };
    // `book:<index>`, `book:<index>:graphs` and `book:<index>:journal` for the
    // book's other pages, and `book:<index>:<question>` with one up over it.
    let (book, asking, page, from, find) = match shot.name.as_str() {
        "book" => {
            let of = shot.of.as_deref().unwrap_or("0");
            // `<shot>:open:<n>` opens the row `n` down on its own page, `:open:<n>:<k>` at page `k`.
            let of = of.split_once(":open:").map_or(of, |(of, _)| of);
            let (at, asking, page, from, find) = match of.split_once(':') {
                Some((at, "cleared")) => (at, None, BookTab::Statistics, 0, None),
                // `journal` opens the list at its head, `journal:<n>` `n` rows down,
                // and `written`, `written:<n>` the same over `fixture::written`.
                Some((at, "journal" | "written")) => (at, None, BookTab::Journal, 0, None),
                Some((at, page)) if page.starts_with("written:") => (
                    at,
                    None,
                    BookTab::Journal,
                    page["written:".len()..].parse().unwrap_or(0),
                    None,
                ),
                // `graphs` draws the standing reading, `graphs:<n>` the one
                // standing `n` from the first.
                Some((at, page)) if page == "graphs" || page.starts_with("graphs:") => {
                    (at, None, BookTab::Graphs, 0, None)
                }
                // `find` puts a field over the book's own passages, and
                // `find:<of>` reads what a `search:<of>` shot reads.
                Some((at, page)) if page == "find" || page.starts_with("find:") => (
                    at,
                    None,
                    BookTab::Journal,
                    0,
                    Some(searching(page.strip_prefix("find:"))),
                ),
                Some((at, page)) if page.starts_with("journal:") => (
                    at,
                    None,
                    BookTab::Journal,
                    page["journal:".len()..].parse().unwrap_or(0),
                    None,
                ),
                Some((at, name)) => (at, Some(question(name)?), BookTab::Statistics, 0, None),
                None => (of, None, BookTab::Statistics, 0, None),
            };
            (Some(at.parse().unwrap_or(0)), asking, page, from, find)
        }
        _ => (None, None, BookTab::Statistics, 0, None),
    };
    app.show(*tab, book);
    app.set_book_tab(page, from);
    if let Some((_, open)) = shot.of.as_deref().and_then(|of| of.split_once(":open:")) {
        let mut at = open.split(':').map(|n| n.parse().unwrap_or(0));
        let row = at.next().unwrap_or(0);
        app.set_entry(row, at.next().unwrap_or(0));
    }
    app.set_reading(shot.of.as_deref().and_then(|of| {
        of.rsplit_once("graphs:")
            .and_then(|(_, at)| at.parse::<usize>().ok())
    }));
    app.set_journal_way(match shot.of.as_deref() {
        Some(of) if of.ends_with(":journal:earliest") => Way::Up,
        _ => Way::Down,
    });
    app.set_book_search(find);
    app.ask(book.zip(asking));
    if shot.name == "config" {
        let second = shot.of.as_deref() == Some("many2");
        app.set_config_page(usize::from(second));
        app.set_backups(match shot.of.as_deref() {
            Some("backups") => Some(0),
            Some("backups2") => Some(4),
            _ => None,
        });
        app.ask_about(match shot.of.as_deref() {
            Some("reset") => Some(Act::Reset(true)),
            Some("nobackup") => Some(Act::Reset(false)),
            Some("restore" | "gone") => Some(Act::Restore(0)),
            Some("rebuild") => Some(Act::Rebuild),
            Some("many" | "many2" | "backups" | "backups2") | None => None,
            Some(other) => bail!("no config shot called {other}"),
        });
    }
    if shot.name == "search" || shot.name == "journal" {
        app.set_search(Some(searching(shot.of.as_deref())));
        app.set_scope(match shot.name.as_str() {
            "journal" => Scope::Journal,
            _ => Scope::Books,
        });
    }
    set_span(app, shot, week)?;
    app.draw(fb)
}

/// What a `search:<of>` shot is showing. `of` is the query, but for `empty`,
/// `none`, `down` and `preedit`.
fn searching(of: Option<&str>) -> Search {
    let (query, preedit, keyboard) = match of.unwrap_or("the") {
        "empty" => ("", "", true),
        "none" => ("zzzz", "", true),
        "down" => ("the", "", false),
        // A pinyin run part way to a character, under its own rule.
        "preedit" => ("夢", "youzheng", true),
        said => (said, "", true),
    };
    Search {
        query: query.into(),
        preedit: preedit.into(),
        keyboard,
        from: 0,
    }
}

/// The question a `book:<index>:<name>` shot puts up over the book.
fn question(name: &str) -> Result<Ask> {
    match name {
        "restart" => Ok(Ask::Restart),
        "clear" => Ok(Ask::Clear),
        "mark" => Ok(Ask::Mark(true)),
        "unmark" => Ok(Ask::Mark(false)),
        _ => Err(anyhow!(
            "no question called {name} — restart, mark, unmark or clear"
        )),
    }
}

/// The update banner, at whichever of its lines `of` names.
fn banner(app: &mut App, fb: &mut Framebuffer, of: &str) -> Result<()> {
    let Some((_, said)) = BANNERS.iter().find(|(name, _)| *name == of) else {
        return Err(anyhow!("no update banner called {of}"));
    };
    let s = app.language().strings();
    let (headline, note, step) = match said() {
        Banner::Doing(doing) => {
            let (headline, note) = doing.banner(s);
            (headline, note, s.update_tap_to_stop)
        }
        Banner::Ended(outcome) => {
            let (headline, note) = outcome.banner(s);
            (headline, note, "")
        }
    };
    app.banner(fb, &headline, &note, step, true)
}

/// The banner over an act while it runs, at whichever of them `of` names,
/// drawn a quarter of the way through the files it counts.
fn acting(app: &mut App, fb: &mut Framebuffer, of: &str) -> Result<()> {
    let Some((_, about)) = ACTS.iter().find(|(name, _)| *name == of) else {
        return Err(anyhow!("no act banner called {of}"));
    };
    let s = app.language().strings();
    let (headline, note) = about.doing(s);
    let step = match about {
        Act::Reset(_) => String::new(),
        Act::Restore(_) => splash::step(s.step_files, 118, 436),
        Act::Rebuild => splash::step(s.step_logs, 31, 97),
    };
    app.banner(fb, &headline, &note, &step, true)
}

/// Every hit box the frame recorded, outlined over it.
fn outline_hits(app: &App, fb: &mut Framebuffer) {
    for (_, area) in app.hits() {
        paint::stroke(fb, *area, paint::INK, 2);
    }
}

/// The last day the record holds a book read through on, and the day showing
/// where it holds none.
fn last_finished(app: &App) -> i64 {
    app.stats()
        .books
        .iter()
        .filter(|b| b.is_finished())
        .map(|b| b.last_day)
        .max()
        .unwrap_or(app.state().day)
}

/// Rhythm's zoom, where the shot names one.
fn set_span(app: &mut App, shot: &Shot, week: WeekStart) -> Result<()> {
    let Some(of) = shot.of.as_deref() else {
        return Ok(());
    };
    match of {
        "all" => app.set_span(Span::AllTime),
        "trends" => {
            app.set_span(Span::AllTime);
            app.set_alltime_page(1);
        }
        "finished" => app.set_shelf(Shelf::Finished),
        "unfinished" => app.set_shelf(Shelf::Unfinished),
        "unfinished-progress" => {
            app.set_shelf(Shelf::Unfinished);
            app.set_sort(Sort::Progress);
        }
        // The list a span's own Finished figure opens: what was read through
        // inside one year, under the chip that drops the window.
        "window" => {
            app.set_shelf(Shelf::Finished);
            let day = app.state().day;
            app.set_window(Some(Window {
                span: Span::Year,
                day,
            }));
        }
        // The same list under the tightest span, whose name is the longest a
        // chip carries: the week a book was last read through in.
        "windowweek" => {
            app.set_shelf(Shelf::Finished);
            let day = last_finished(app);
            app.set_window(Some(Window {
                span: Span::Week,
                day,
            }));
        }
        // `Span::Week` sets the widest window chip, `Sort::Progress` the
        // widest sort chip.
        "windowprogress" => {
            app.set_shelf(Shelf::Finished);
            let day = last_finished(app);
            app.set_window(Some(Window {
                span: Span::Week,
                day,
            }));
            app.set_sort(Sort::Progress);
        }
        // A window the record holds no reading inside, which no figure opens
        // and a shelf tap can reach.
        "windowempty" => {
            let day = Span::Year.step(app.state().day, -3);
            app.set_window(Some(Window {
                span: Span::Year,
                day,
            }));
        }
        "time" => app.set_sort(Sort::Time),
        "progress" => app.set_sort(Sort::Progress),
        "up" => app.set_sort_way(Way::Up),
        "last" => app.open_books(usize::MAX),
        // A page with a list either side of it, where both jump marks stand.
        "mid" => app.open_books(5),
        "week" => app.set_span(Span::Week),
        // A span stepped off the one holding today, where the way back to it is
        // drawn; `weekback` the week, its name the longest, the chip beside it.
        "back" | "weekback" => {
            let span = match of {
                "weekback" => Span::Week,
                _ => Span::Year,
            };
            app.set_span(span);
            let day = app.state().day;
            app.set_day(span.step(day, -1));
        }
        "month" => app.set_span(Span::Month),
        "year" => app.set_span(Span::Year),
        "day" => app.open_day(app.state().day),
        // The day a book was read through on, where a row carries the chip.
        "dayfinished" => app.open_day(last_finished(app)),
        // The year with a day picked off its heatmap, which narrows the
        // covers to that day and offers the way into it.
        "picked" => {
            app.set_span(Span::Year);
            app.open_day(app.state().day);
        }
        // The binge day picked off the year's heatmap: a cover grid deeper
        // than one page, where the count and the chip share the heading.
        "yearbusy" => {
            app.set_span(Span::Year);
            busy(app, 0);
        }
        // The same day picked off its own week's bars, where the grid runs to
        // more than one row.
        "weekbusy" => {
            app.set_span(Span::Week);
            busy(app, 0);
        }
        // A day with nothing on it picked off the week's tappable columns, in
        // the week the record ends part way through.
        "weekempty" => {
            app.set_span(Span::Week);
            let day = app.state().day;
            app.open_day(*Span::Week.days(day, week).end());
        }
        "busy" => busy(app, 0),
        "busy2" => busy(app, 3),
        "busyend" => busy(app, usize::MAX),
        // `today:quiet`, `today:empty` and `book:3` name no span.
        _ => {}
    }
    Ok(())
}

/// The fixture's fullest day, its book list opened at `from`.
fn busy(app: &mut App, from: usize) {
    let day = fixture::binge_day(app.state().day);
    app.open_day(day);
    app.open_list(from);
}

/// The part of a filename naming the panel, where the run draws more than one.
fn panel_tag(w: u32, h: u32) -> String {
    match PANELS.iter().find(|(_, pw, ph)| *pw == w && *ph == h) {
        Some((name, _, _)) if *name == PANELS[0].0 => String::new(),
        Some((name, _, _)) => format!("-{name}"),
        None => format!("-{w}x{h}"),
    }
}

fn lang_tag(lang: Lang, opts: &Opts) -> String {
    match opts.langs.len() {
        1 => String::new(),
        _ => format!("-{}", lang.language_tag()),
    }
}

fn size_tag(size: TextSize, opts: &Opts) -> String {
    match opts.sizes.len() {
        1 => String::new(),
        _ => format!("-{}", size_name(size)),
    }
}

fn size_name(size: TextSize) -> &'static str {
    match size {
        TextSize::Small => "small",
        TextSize::Medium => "medium",
        TextSize::Large => "large",
    }
}

/// Every shot the run can be asked for.
fn list() {
    println!("screens:");
    for (name, _) in SCREENS {
        let of = match *name {
            "rhythm" => {
                "  (:all :trends :week :month :year :back :weekback :picked :day\n   :dayfinished :yearbusy :weekbusy :weekempty :busy :busy2 :busyend)"
            }
            "today" => "  (:quiet :empty :busy)",
            "book" => {
                "  (:<index> :<index>:graphs :<index>:graphs:<n> :<index>:journal\n   :<index>:journal:<n> :<index>:journal:earliest :<index>:find\n   :<index>:find:<query> :<index>:restart :<index>:mark :<index>:unmark\n   :<index>:clear :<index>:cleared :<index>:written :<index>:written:<n>\n   :<index>:journal:open:<n> :<index>:written:open:<n>:<page>)"
            }
            "config" => {
                "  (:reset :nobackup :restore :gone :rebuild :many :many2 :backups\n   :backups2)"
            }
            "search" | "journal" => "  (:<query> :empty :none :down :preedit)",
            "books" => {
                "  (:finished :unfinished :unfinished-progress :time :progress :up\n   :mid :last :window :windowweek :windowprogress :windowempty)"
            }
            _ => "",
        };
        println!("  {name}{of}");
    }
    // `banners` and `acts` state their own lists of qualifiers.
    let banners: Vec<&str> = BANNERS.iter().map(|(name, _)| *name).collect();
    println!("  update  (:{})", banners.join(" :"));
    let acts: Vec<&str> = ACTS.iter().map(|(name, _)| *name).collect();
    println!("  act     (:{})", acts.join(" :"));
    println!("sketches:");
    match sketch::ALL.is_empty() {
        true => println!("  (none)"),
        false => {
            for sketch in sketch::ALL {
                println!("  {}  (:week :month :year :day)", sketch.name);
            }
        }
    }
    println!("panels:");
    for (name, w, h) in PANELS {
        println!("  {name}  {w}x{h}");
    }
}

/// Read the command line.
fn read_args(args: impl Iterator<Item = String>) -> Result<Opts> {
    let mut opts = Opts::default();
    let (mut panels, mut langs, mut sizes) = (Vec::new(), Vec::new(), Vec::new());
    let mut args = args.peekable();
    while let Some(arg) = args.next() {
        let mut value = || {
            args.next()
                .ok_or_else(|| anyhow!("{arg} wants something after it"))
        };
        match arg.as_str() {
            "--panel" => panels.push(panel(&value()?)?),
            "--lang" => langs.push(lang(&value()?)?),
            "--size" => sizes.push(size(&value()?)?),
            "--week" => opts.week = week(&value()?)?,
            "--scheme" => opts.scheme = scheme(&value()?)?,
            "--no-colour" => opts.colour = false,
            "--day" => opts.day = day(&value()?)?,
            "--hide-unidentified" => opts.unidentified = false,
            "--floor" => opts.floor = floor(&value()?)?,
            "--stats-from" => opts.stats_from = stats_from(&value()?)?,
            "--hide-uncovered" => opts.uncovered = false,
            "--hits" => opts.hits = true,
            "--written" => opts.written = true,
            "--out" => opts.out = PathBuf::from(value()?),
            "--covers" => opts.covers = PathBuf::from(value()?),
            "--record" => opts.record = Some(PathBuf::from(value()?)),
            "--sheet" => opts.sheet = Some(value()?),
            "--scale" => opts.scale = value()?.parse().context("--scale wants a percentage")?,
            "--crop" => {
                let spec = value()?;
                opts.crop = Some(
                    sheet::Crop::read(&spec)
                        .ok_or_else(|| anyhow!("--crop wants WxH+X+Y, not {spec}"))?,
                );
            }
            "--all" => opts.shots.extend(everything()),
            "--list" => return Ok(Opts::default()),
            other if other.starts_with('-') => bail!("no option called {other}"),
            other => opts.shots.push(Shot::read(other)),
        }
    }
    if !panels.is_empty() {
        opts.panels = panels;
    }
    if !langs.is_empty() {
        opts.langs = langs;
    }
    if !sizes.is_empty() {
        opts.sizes = sizes;
    }
    Ok(opts)
}

/// Every shot `--all` draws, in the order the tab strip names their screens.
fn everything() -> Vec<Shot> {
    [
        "today",
        "today:busy",
        "today:quiet",
        "today:empty",
        "rhythm:all",
        "rhythm:trends",
        "rhythm:week",
        "rhythm:month",
        "rhythm:year",
        "rhythm:back",
        "rhythm:weekback",
        "rhythm:picked",
        "rhythm:dayfinished",
        "rhythm:yearbusy",
        "rhythm:weekbusy",
        "rhythm:weekempty",
        "rhythm:day",
        "books",
        "books:finished",
        "books:unfinished",
        "books:time",
        "books:progress",
        "books:mid",
        "books:window",
        "books:windowweek",
        "books:windowprogress",
        "books:windowempty",
        "book",
        // `book:1` is the library slot `fixture::RESTARTED` reads twice.
        "book:1:graphs",
        "book:2:graphs",
        "book:5:graphs",
        "book:5:journal",
        "book:8:journal",
        "book:8:journal:5",
        "book:13:journal",
        "book:12:journal",
        "book:0:journal",
        "config",
        "update:downloading",
        "update:done",
        "update:failed",
        "act:rebuild",
    ]
    .iter()
    .map(|spec| Shot::read(spec))
    .collect()
}

fn panel(name: &str) -> Result<(u32, u32)> {
    if let Some((_, w, h)) = PANELS.iter().find(|(n, _, _)| *n == name) {
        return Ok((*w, *h));
    }
    let (w, h) = name
        .split_once('x')
        .ok_or_else(|| anyhow!("no panel called {name}"))?;
    Ok((
        w.parse().context("panel width")?,
        h.parse().context("panel height")?,
    ))
}

fn lang(tag: &str) -> Result<Lang> {
    Lang::ALL
        .into_iter()
        .find(|l| l.language_tag() == tag)
        .ok_or_else(|| {
            let known: Vec<&str> = Lang::ALL.iter().map(|l| l.language_tag()).collect();
            anyhow!("no language called {tag} — one of {}", known.join(", "))
        })
}

fn size(name: &str) -> Result<TextSize> {
    TextSize::ALL
        .into_iter()
        .find(|s| size_name(*s) == name)
        .ok_or_else(|| anyhow!("no text size called {name} — small, medium or large"))
}

fn scheme(name: &str) -> Result<ColorScheme> {
    ColorScheme::ALL
        .into_iter()
        .find(|s| scheme_name(*s) == name)
        .ok_or_else(|| {
            let names: Vec<&str> = ColorScheme::ALL.into_iter().map(scheme_name).collect();
            anyhow!("no scheme called {name} — one of {}", names.join(", "))
        })
}

fn scheme_name(scheme: ColorScheme) -> &'static str {
    match scheme {
        ColorScheme::TobiKogane => "tobi",
        ColorScheme::SakuraWakatake => "wakatake",
        ColorScheme::KurenaiKon => "kon",
        ColorScheme::Grey => "grey",
    }
}

fn week(name: &str) -> Result<WeekStart> {
    match name {
        "mon" | "monday" => Ok(WeekStart::Monday),
        "sun" | "sunday" => Ok(WeekStart::Sunday),
        other => Err(anyhow!("no week start called {other} — mon or sun")),
    }
}

fn stats_from(name: &str) -> Result<StatsFrom> {
    match name {
        "kindle" => Ok(StatsFrom::Kindle),
        "app" => Ok(StatsFrom::App),
        other => Err(anyhow!("no stats source called {other} — kindle, app")),
    }
}

fn floor(name: &str) -> Result<SittingFloor> {
    match name {
        "all" | "0m" => Ok(SittingFloor::All),
        "1m" => Ok(SittingFloor::OneMinute),
        "5m" => Ok(SittingFloor::FiveMinutes),
        "15m" => Ok(SittingFloor::FifteenMinutes),
        other => Err(anyhow!(
            "no sitting floor called {other} — all, 1m, 5m, 15m"
        )),
    }
}

/// `YYYY-MM-DD`, or `today` for the day the machine is in.
fn day(spec: &str) -> Result<i64> {
    if spec == "today" {
        return Ok(date::now().0);
    }
    date::parse_day(spec).ok_or_else(|| anyhow!("no day called {spec} — YYYY-MM-DD or today"))
}
