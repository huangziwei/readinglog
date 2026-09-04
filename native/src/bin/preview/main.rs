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
use readinglog_native::settings::{ColorScheme, TextSize, WeekStart};
use readinglog_native::stats::Stats;
use readinglog_native::store::Store;
use readinglog_native::ui::chrome::Tab;
use readinglog_native::ui::paint;
use readinglog_native::ui::text::TextRenderer;
use readinglog_native::ui::theme::Theme;
use readinglog_native::update::{Doing, Failure, Outcome};
use readinglog_native::view::{Shelf, Sort, Span};

/// The day the preview is set to, and the second of it: a Wednesday evening in
/// the middle of a month.
const DAY: (i64, i64, i64) = (2026, 9, 16);
const NOW: i64 = 20 * 3600 + 15 * 60;

/// The panels, by the name `--panel` takes.
const PANELS: &[(&str, u32, u32)] = &[
    // Paperwhite, Colorsoft, Oasis 2.
    ("pw", 1264, 1680),
    ("scribe", 1860, 2480),
];

/// Where the PNGs land under `--out`.
const OUT: &str = "artifacts/preview";

/// Where the fixture's jackets are drawn, under `--art`. An input the store's
/// records point at, and the same files whatever a run draws, so one set stands
/// apart from any run's output directory.
const ART: &str = "artifacts/preview/art";

/// The update banner a shot can name, and what it is saying. The one screen
/// with no tab under it, drawn over the whole panel, and where a translation
/// that runs long shows.
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

/// What one of them is saying, made on demand: an [`Outcome`] holds a
/// `String` and a `const` cannot.
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
    art: PathBuf,
    /// A store to draw in place of the fixture, by the directory holding it.
    store: Option<PathBuf>,
    sheet: Option<String>,
    scale: u32,
    crop: Option<sheet::Crop>,
    /// Whether every hit box is outlined over the frame. A tap target is
    /// invisible in a render, and a screen that looks right can still take a
    /// tap where it should take none.
    hits: bool,
    /// The scheme drawn in.
    scheme: ColorScheme,
    /// What `eink::fb::has_cfa` is drawn as answering.
    colour: bool,
    /// Whether a total counts the sittings no record names.
    unnamed: bool,
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
            art: PathBuf::from(ART),
            store: None,
            sheet: None,
            scale: 40,
            crop: None,
            hits: false,
            unnamed: true,
        }
    }
}

fn main() {
    if let Err(err) = run() {
        eprintln!("preview: {err:#}");
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
    std::fs::create_dir_all(&opts.art).context("make the cover directory")?;
    let standing = shots_in(&opts.out);

    let library = match &opts.store {
        Some(dir) => Store::load(dir),
        None => fixture::library(opts.day, &opts.art),
    };
    let mut tiles: Vec<sheet::Tile> = Vec::new();
    let mut wrote: Vec<PathBuf> = Vec::new();
    for (w, h) in opts.panels.iter().copied() {
        for lang in opts.langs.iter().copied() {
            for size in opts.sizes.iter().copied() {
                for shot in &opts.shots {
                    let store = thinned_for(shot, &opts, &opts.art);
                    let store = store.as_ref().unwrap_or(&library);
                    let mut fb = Framebuffer::offscreen(w, h);
                    let mut app = open(store, &opts, w, h, lang, size)?;
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
    // A directory holding shots this run did not draw mixes two rounds, and
    // nothing in the filename says which round a picture belongs to.
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

/// The PNGs already in `dir`, which this run will leave standing wherever it
/// does not draw over them.
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
const FONTS: &str = "no font — point READINGLOG_FONTS at the device's font directory";

/// An [`App`] over `store`, at the panel and the settings the run names.
fn open(store: &Store, opts: &Opts, w: u32, h: u32, lang: Lang, size: TextSize) -> Result<App> {
    if Stats::build(store, opts.day, true).books.is_empty() {
        bail!("the fixture named no book");
    }
    let theme = Theme::sized(w, h, size);
    let text = TextRenderer::load(theme.body_px).context(FONTS)?;
    let mut app = App::new(store.clone(), theme, text);
    app.set_clock(opts.day, NOW);
    app.set_language(lang);
    app.set_text_size(size);
    app.set_week_start(opts.week);
    app.set_colour(opts.colour);
    app.set_color_scheme(opts.scheme);
    app.set_unnamed(opts.unnamed);
    Ok(app)
}

/// The store a shot wants, where it wants one of its own.
fn thinned_for(shot: &Shot, opts: &Opts, art: &Path) -> Option<Store> {
    let keep = match (shot.name.as_str(), shot.of.as_deref()) {
        ("today", Some("quiet")) => 1,
        ("today", Some("empty")) => 0,
        ("today", Some("busy")) => return Some(fixture::crowded(opts.day, art)),
        _ => return None,
    };
    Some(fixture::thinned(opts.day, art, keep))
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
    let Some((_, tab)) = SCREENS.iter().find(|(name, _)| *name == shot.name) else {
        return Err(anyhow!("no screen or sketch called {}", shot.name));
    };
    let book = match shot.name.as_str() {
        "book" => Some(shot.of.as_deref().unwrap_or("0").parse().unwrap_or(0)),
        _ => None,
    };
    app.show(*tab, book);
    set_span(app, shot, week)?;
    app.draw(fb)
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

/// Every hit box the frame recorded, outlined over it.
fn outline_hits(app: &App, fb: &mut Framebuffer) {
    for (_, area) in app.hits() {
        paint::stroke(fb, *area, paint::INK, 2);
    }
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
        "longest" => app.set_sort(Sort::Longest),
        "furthest" => app.set_sort(Sort::Furthest),
        "last" => app.open_books(usize::MAX),
        // A page with a list either side of it, where both jump marks stand.
        "mid" => app.open_books(5),
        "week" => app.set_span(Span::Week),
        // A span stepped off the one holding today, where the way back to it
        // is drawn. The week is the tightest of them: its name is the longest
        // and the chip stands beside it.
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
        // A day with nothing on it picked off the week. Its columns take a tap
        // where a year's cells do not, and the record ends part way through
        // the week showing, so its last day is always one of these.
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

/// The fixture's fullest day, its book list opened at `from`: how a day of
/// more books than the page holds reads.
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
                "  (:all :trends :week :month :year :back :weekback :picked :day\n   :yearbusy :weekbusy :weekempty :busy :busy2 :busyend)"
            }
            "today" => "  (:quiet :empty :busy)",
            "book" => "  (:<index>)",
            "books" => "  (:finished :longest :furthest :mid :last)",
            _ => "",
        };
        println!("  {name}{of}");
    }
    // Named off `BANNERS` rather than written out: it is the only list here
    // the code can state for itself.
    let banners: Vec<&str> = BANNERS.iter().map(|(name, _)| *name).collect();
    println!("  update  (:{})", banners.join(" :"));
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
            "--hide-unnamed" => opts.unnamed = false,
            "--hits" => opts.hits = true,
            "--out" => opts.out = PathBuf::from(value()?),
            "--art" => opts.art = PathBuf::from(value()?),
            "--store" => opts.store = Some(PathBuf::from(value()?)),
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
        "rhythm:yearbusy",
        "rhythm:weekbusy",
        "rhythm:weekempty",
        "rhythm:day",
        "books",
        "books:finished",
        "books:longest",
        "books:furthest",
        "books:mid",
        "book",
        "config",
        "update:downloading",
        "update:done",
        "update:failed",
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
        ColorScheme::Azure => "azure",
        ColorScheme::AsagiShu => "asagi",
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

/// `YYYY-MM-DD`, or `today` for the day the machine is in.
fn day(spec: &str) -> Result<i64> {
    if spec == "today" {
        return Ok(date::now().0);
    }
    date::parse_day(spec).ok_or_else(|| anyhow!("no day called {spec} — YYYY-MM-DD or today"))
}
