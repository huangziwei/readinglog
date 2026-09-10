//! Reading Log — reading statistics on a Kindle, from the Kindle's own logs.
//! Four modes: no argument collects then draws, `--collect` collects alone,
//! `--dump` prints the store, `--version` states its version.

use std::path::Path;

use anyhow::{Context, Result};

use readinglog_native::eink::buttons::Buttons;
use readinglog_native::eink::fb::Framebuffer;
use readinglog_native::eink::input::Input;
use readinglog_native::eink::touch::Touch;
use readinglog_native::orientation::Orientation;
use readinglog_native::stats::Stats;
use readinglog_native::store::Store;
use readinglog_native::{
    annotate, app, catalog, clippings, date, font, identify, journal, lang, settings, sidecar,
    store, ui, vocab, zone,
};

fn main() {
    let mode = std::env::args().nth(1).unwrap_or_default();
    // `show` states the version inside its header block, under the panel and
    // font facts that only it knows. The modes that draw nothing have no such
    // block, so they say it on a line of their own.
    if matches!(mode.as_str(), "--collect" | "--dump") {
        eprintln!(
            "build: {} {}",
            readinglog_native::update::VERSION,
            readinglog_native::update::BUILD
        );
    }
    // Before anything is written: the log is ours, and bounding it is ours to
    // do. `--dump` and `--version` write nothing and leave it alone.
    if matches!(mode.as_str(), "--collect" | "")
        && let Some(cut) = journal::trim(Path::new(journal::LOG_PATH), journal::CEILING)
    {
        eprintln!(
            "log: trimmed {} B to {} B, {} blocks",
            cut.before, cut.after, cut.blocks
        );
    }
    let result = match mode.as_str() {
        "--collect" => collect().map(|_| ()),
        "--dump" => dump(),
        "--version" => version(),
        _ => show(),
    };
    // A launch that dies before `show` states its header block would sit under
    // the previous build's block and be read as that build's. The failure
    // names its own.
    if let Err(err) = result {
        eprintln!(
            "!! readinglog: {err:#} — build {} {}",
            readinglog_native::update::VERSION,
            readinglog_native::update::BUILD,
        );
        std::process::exit(1);
    }
}

/// Which version this build is, on one line and nothing beside it —
/// `states_version` reads it back off a staged copy. The one mode that opens
/// nothing: no display, no log, no store.
fn version() -> Result<()> {
    println!("{}", readinglog_native::update::VERSION);
    Ok(())
}

/// Read the log and the catalog into the store, and answer with the store.
/// `catalog` is read here and nowhere else, and what it states is written into
/// `store`.
fn collect() -> Result<Store> {
    let dir = Path::new(store::STORE_DIR);
    let mut store = Store::open(dir);
    let said = collect_into(&mut store, dir, &mut |_, _| {});
    said.state(&journal::stamp_now(), journal::Mode::Collect);
    Ok(store)
}

/// [`collect`] over a loaded `store`, reporting log files opened and log files
/// to open.
fn collect_into(
    store: &mut Store,
    dir: &Path,
    on: &mut dyn FnMut(usize, usize),
) -> journal::Launch {
    let mut said = journal::Launch::default();
    let pass = store.update(on);
    // A clock that stepped back left the stretch it stepped over below the
    // mark; the pass pulled the mark back and read that stretch again. Rare
    // enough to be worth its own line when it happens.
    if pass.rewound > 0 {
        eprintln!(
            "clock: the device stepped back {}, and the log was read again from there",
            date::duration(pass.rewound, lang::Lang::English.strings()),
        );
    }
    // `catalog::read` speaks first. `identify::rescue` asks the three sources
    // that name what it left unnamed, and `annotate::fold` joins the two the
    // same walk reaches.
    let books = catalog::read();
    let stated = store.remember(&books);
    // Both gates, off a walk that opens nothing. A launch that read, marked
    // and installed nothing leaves both standing and opens no sidecar at all.
    let clips = Path::new(clippings::CLIPPINGS_FILE);
    let documents = Path::new(sidecar::DOCUMENTS_DIR);
    let (survey, naming, marks) =
        identify::asked(store, Path::new(vocab::VOCAB_DB), clips, documents);
    let shelf = match naming.is_some() || marks {
        true => identify::shelf_for(store, documents, naming.is_some()),
        false => sidecar::Shelf::default(),
    };
    // One read of `My Clippings.txt`, for whichever of the two passes wants
    // it: the naming pass reads it as witnesses and the annotation pass as
    // marks, and neither may open it for itself.
    let wants_clips = naming.is_some() && store.wants_naming(store::Named::Clippings);
    let records = match marks || wants_clips {
        true => clippings::read(clips),
        false => Vec::new(),
    };
    // Whether this pass moved the gate, which is a change to the record like
    // any other: a gate written and not saved is a gate that never holds, and
    // the sources would be read again on every launch for ever.
    let gated = naming.is_some();
    let rescue = naming.map(|gate| {
        let out = identify::rescue(store, &records, &shelf);
        // The gate as it stood *before* the pass: a pass that named something
        // moved the record, so the next launch asks once more and settles.
        store.sources = Some(gate);
        out
    });
    let merge = match marks {
        true => annotate::fold(store, clips, &records, &shelf, &survey),
        false => annotate::Merge::default(),
    };
    let jackets = store.keep_covers(dir);
    let refreshed = stated + rescue.map_or(0, |r| r.named()) + jackets.kept;
    said.log = Some(format!(
        "log={}/{}l{}c{}d{}s",
        pass.lines, pass.from.live, pass.from.chunks, pass.from.dumps, pass.from.skipped,
    ));
    said.sittings = Some(format!(
        "s=+{}~{}/{}",
        pass.added,
        pass.extended,
        store.sessions.len()
    ));
    said.catalog = Some(format!("cat={}/{stated}", books.len()));
    said.records = Some(format!("rec={}b", store.books.len()));
    said.covers = Some(jackets.said());
    said.identify = rescue.as_ref().and_then(rescue_said);
    said.annotate = marks_said(&merge);
    // The books whose jackets the device has lost are the same ones every
    // launch, so they are named beside the count rather than a line apiece.
    if let Some(lost) = jackets.lost_said() {
        eprintln!("covers: the device has lost artwork for {lost}");
    }
    // An unchanged store is left on disk unwritten.
    if pass.added + pass.extended + refreshed == 0 && !merge.read && !gated {
        return said;
    }
    // A failed `save` leaves `store` drawable and unsaved.
    if let Err(err) = store.save(dir) {
        eprintln!("!! collect: the record did not reach the store: {err}");
    }
    said
}

/// What the sources that name a book the catalog cannot came to, one line, and
/// nothing at all where the catalog left them nothing to do.
fn rescue_said(rescue: &identify::Rescue) -> Option<String> {
    if rescue.lookups + rescue.clippings + rescue.sidecars == 0 {
        return None;
    }
    Some(format!(
        "id={}v{}c{}s/{}?{}",
        rescue.by_vocab, rescue.by_clippings, rescue.by_sidecars, rescue.unnamed, rescue.contested,
    ))
}

/// What the two annotation sources came to, one line, and nothing at all where
/// the gate held and neither was read.
fn marks_said(merge: &annotate::Merge) -> Option<String> {
    if !merge.read {
        return None;
    }
    Some(format!(
        "ann={}c{}s/{}L{}r{}?",
        merge.clippings, merge.sidecars, merge.live, merge.retired, merge.unconfirmed,
    ))
}

/// The store as text, one sitting a line.
fn dump() -> Result<()> {
    let store = collect()?;
    let (today, _) = date::now();
    let settings = settings::Settings::load(lang::Lang::detect());
    let stats = Stats::build(
        &store,
        today,
        settings.show_unnamed,
        settings.figures,
        settings.sitting_floor,
    );
    println!(
        "{} read over {} days, {} books, streak {} (longest {})",
        date::duration(stats.total_seconds, lang::Lang::English.strings()),
        stats.days_read(),
        stats.books.len(),
        stats.current_streak,
        stats.longest_streak,
    );
    for book in &stats.books {
        println!(
            "  {:>8}  {:>4} sittings  {:>3} days  {:>5}%  {}",
            date::duration(book.seconds, lang::Lang::English.strings()),
            book.sittings,
            book.days,
            if book.has_percent() {
                format!("{:.0}", book.percent)
            } else {
                "—".into()
            },
            book.title,
        );
    }
    Ok(())
}

/// The launch banner: the same headline and note, at whatever `step` the
/// collect has reached.
fn launching<'a>(script: font::Script, note: &'a [String], step: &'a str) -> ui::splash::Words<'a> {
    ui::splash::Words {
        script,
        headline: "Reading Log",
        note,
        step,
    }
}

/// Collect, then put it on the screen.
fn show() -> Result<()> {
    let mut fb = Framebuffer::open().context("open the display")?;
    let orientation = Orientation::detect();
    let touch =
        Touch::open(orientation, fb.var.xres, fb.var.yres).context("open the touchscreen")?;
    // Taken before `touch` is handed to `Input`, for the header block below.
    let input_said = touch.describe().to_string();
    // `Buttons::open` grabs the bezel before the first draw. A model with no
    // page buttons answers `None`, which the header block states.
    let buttons = Buttons::open().unwrap_or_else(|err| {
        eprintln!("?? buttons: {err:#} — running touch-only");
        None
    });
    let input_said = format!(
        "{input_said} {}",
        readinglog_native::eink::buttons::describe(buttons.as_ref())
    );
    let mut input = Input::new(touch, buttons);
    input.set_orientation(orientation);

    // `splash::show` paints before the first gunzip.
    let dir = Path::new(store::STORE_DIR);
    let mut store = Store::open(dir);
    let theme = ui::theme::Theme::for_screen(fb.var.xres, fb.var.yres);
    let mut text = ui::text::TextRenderer::load(theme.body_px)?;
    // The facts that only change when the build, the panel or the clock does.
    // A block identical to the one already standing is not written again.
    journal::Header {
        version: readinglog_native::update::VERSION,
        build: readinglog_native::update::BUILD,
        arch: std::env::consts::ARCH,
        panel: format!(
            "{}x{} at {} ppi, body {} px, {}",
            fb.var.xres,
            fb.var.yres,
            theme.dpi(),
            theme.body_px,
            match readinglog_native::eink::fb::has_cfa() {
                true => "colour filter present",
                false => "no colour filter",
            },
        ),
        surface: fb.describe().to_string(),
        input: input_said,
        fonts: text.chain_summary(),
        store: store.said(),
        zone: zone::describe(date::epoch_now()),
    }
    .state(Path::new(journal::LOG_PATH));
    // `splash` draws before `App` and detects for itself.
    let splash_lang = lang::Lang::detect();
    let note = ui::splash::note(&store.mark, splash_lang.strings());
    let script = font::Script::of_language(splash_lang.language_tag());
    ui::splash::show(
        &mut fb,
        &mut text,
        &theme,
        &launching(script, &note, ""),
        true,
    )?;

    let mut painted = 0;
    let mut said = collect_into(&mut store, dir, &mut |done, total| {
        if done == painted {
            return;
        }
        painted = done;
        let step = ui::splash::step(splash_lang.strings().step_logs, done, total);
        let step_said = launching(script, &note, &step);
        let _ = ui::splash::show(&mut fb, &mut text, &theme, &step_said, false);
    });

    let mut app = app::App::new(store, theme, text);
    said.drawn = Some(app.drawn());
    said.state(&journal::stamp_now(), journal::Mode::Run);
    app.run(&mut fb, &mut input)
}
