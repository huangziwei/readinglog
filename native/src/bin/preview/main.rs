//! Every screen drawn to a PNG, with no display behind it.
//!
//! ```text
//! cargo run --bin preview -- rhythm:week rhythm:month --sheet spans
//! ```
//!
//! A shot names a screen, and after a colon what it is showing:
//! `rhythm:year`, `today:empty`, `book:3`. `--list` names them all.
//! `--sheet` puts the run’s shots on one captioned sheet beside each other,
//! and `--crop WxH+X+Y` cuts every shot down to the band worth looking at.

mod fixture;
mod sheet;
mod sketch;

use std::path::{Path, PathBuf};

use anyhow::{Context, Result, anyhow, bail};

use readinglog_native::app::App;
use readinglog_native::date;
use readinglog_native::eink::fb::Framebuffer;
use readinglog_native::lang::Lang;
use readinglog_native::settings::{TextSize, WeekStart};
use readinglog_native::stats::Stats;
use readinglog_native::store::Store;
use readinglog_native::ui::chrome::Tab;
use readinglog_native::ui::text::TextRenderer;
use readinglog_native::ui::theme::Theme;
use readinglog_native::view::Span;

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
    sheet: Option<String>,
    scale: u32,
    crop: Option<sheet::Crop>,
}

impl Default for Opts {
    fn default() -> Self {
        Self {
            shots: Vec::new(),
            panels: vec![(PANELS[0].1, PANELS[0].2)],
            langs: vec![Lang::English],
            sizes: vec![TextSize::Medium],
            week: WeekStart::Monday,
            day: date::days_from_civil(DAY.0, DAY.1, DAY.2),
            out: PathBuf::from(OUT),
            sheet: None,
            scale: 40,
            crop: None,
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
    let art = opts.out.join("art");
    std::fs::create_dir_all(&art).context("make the cover directory")?;

    let library = fixture::library(opts.day, &art);
    let mut tiles: Vec<sheet::Tile> = Vec::new();
    let mut written = 0usize;
    for (w, h) in opts.panels.iter().copied() {
        for lang in opts.langs.iter().copied() {
            for size in opts.sizes.iter().copied() {
                for shot in &opts.shots {
                    let store = thinned_for(shot, &opts, &art);
                    let store = store.as_ref().unwrap_or(&library);
                    let mut fb = Framebuffer::offscreen(w, h);
                    let mut app = open(store, &opts, w, h, lang, size)?;
                    draw(&mut app, &mut fb, shot)?;
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
                    written += 1;
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
    }
    eprintln!(
        "preview: {written} shots in {:.1}s",
        started.elapsed().as_secs_f32()
    );
    Ok(())
}

/// What to say when no face was found.
const FONTS: &str = "no font — point READINGLOG_FONTS at the device's font directory";

/// An [`App`] over `store`, at the panel and the settings the run names.
fn open(store: &Store, opts: &Opts, w: u32, h: u32, lang: Lang, size: TextSize) -> Result<App> {
    let stats = Stats::build(store, opts.day);
    if stats.books.is_empty() {
        bail!("the fixture named no book");
    }
    let theme = Theme::sized(w, h, size);
    let text = TextRenderer::load(theme.body_px).context(FONTS)?;
    let mut app = App::new(stats, theme, text);
    app.set_clock(opts.day, NOW);
    app.set_language(lang);
    app.set_text_size(size);
    app.set_week_start(opts.week);
    Ok(app)
}

/// The store a shot wants, where it wants one of its own.
fn thinned_for(shot: &Shot, opts: &Opts, art: &Path) -> Option<Store> {
    let keep = match (shot.name.as_str(), shot.of.as_deref()) {
        ("today", Some("quiet")) => 1,
        ("today", Some("empty")) => 0,
        _ => return None,
    };
    Some(fixture::thinned(opts.day, art, keep))
}

/// Set `app` to what `shot` names and draw it into `fb`.
fn draw(app: &mut App, fb: &mut Framebuffer, shot: &Shot) -> Result<()> {
    if let Some(sketch) = sketch::ALL.iter().find(|s| s.name == shot.name) {
        app.show(sketch.tab, None);
        set_span(app, shot)?;
        let state = app.state().clone();
        let draw = sketch.draw;
        return app.frame(fb, &mut |cx, area| draw(cx, area, &state));
    }
    let Some((_, tab)) = SCREENS.iter().find(|(name, _)| *name == shot.name) else {
        return Err(anyhow!("no screen or sketch called {}", shot.name));
    };
    let book = match shot.name.as_str() {
        "book" => Some(shot.of.as_deref().unwrap_or("0").parse().unwrap_or(0)),
        _ => None,
    };
    app.show(*tab, book);
    set_span(app, shot)?;
    app.draw(fb)
}

/// Rhythm's zoom, where the shot names one.
fn set_span(app: &mut App, shot: &Shot) -> Result<()> {
    let Some(of) = shot.of.as_deref() else {
        return Ok(());
    };
    match of {
        "all" => app.set_span(Span::AllTime),
        "week" => app.set_span(Span::Week),
        "month" => app.set_span(Span::Month),
        "year" => app.set_span(Span::Year),
        "day" => app.open_day(app.state().day),
        // `today:quiet`, `today:empty` and `book:3` name no span.
        _ => {}
    }
    Ok(())
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
            "rhythm" => "  (:all :week :month :year :day)",
            "today" => "  (:quiet :empty)",
            "book" => "  (:<index>)",
            _ => "",
        };
        println!("  {name}{of}");
    }
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
            "--day" => opts.day = day(&value()?)?,
            "--out" => opts.out = PathBuf::from(value()?),
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

/// Every screen worth a look, in the order a reader meets them.
fn everything() -> Vec<Shot> {
    [
        "today",
        "today:quiet",
        "today:empty",
        "rhythm:all",
        "rhythm:week",
        "rhythm:month",
        "rhythm:year",
        "rhythm:day",
        "books",
        "book",
        "config",
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
