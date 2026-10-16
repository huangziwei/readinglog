//! Reading Log — reading statistics on a Kindle, from the Kindle's own logs.
//! Four modes: no argument collects then draws, `--collect` collects alone,
//! `--dump` prints the record, `--version` states its version.

use std::path::Path;

use anyhow::{Context, Result};

use readinglog_native::eink::buttons::Buttons;
use readinglog_native::eink::fb::Framebuffer;
use readinglog_native::eink::input::Input;
use readinglog_native::eink::touch::Touch;
use readinglog_native::orientation::Orientation;
use readinglog_native::record::Record;
use readinglog_native::stats::Stats;
use readinglog_native::{
    app, backup, clippings, date, font, highlights, identify, journal, lang, library, record,
    settings, sidecar, ui, vocab, zone,
};

fn main() {
    let mode = std::env::args().nth(1).unwrap_or_default();
    // `show` states the version inside its header block, under the panel and
    // font facts that only it knows. The modes that draw nothing have no such
    // block and say it on a line of their own.
    if matches!(mode.as_str(), "--collect" | "--dump") {
        eprintln!(
            "build: {} {}",
            readinglog_native::update::VERSION,
            readinglog_native::update::BUILD
        );
    }
    // Before anything is written: `journal::trim` bounds the log.
    // `--dump` and `--version` write nothing and leave it alone.
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
    // A failure names its build, `show`'s header block not standing above it.
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
/// `update::states_version` reads it back off a staged copy. The one mode that
/// opens nothing: no display, no log, no record.
fn version() -> Result<()> {
    println!("{}", readinglog_native::update::VERSION);
    Ok(())
}

/// Read the log and the library into the record, and answer with the record.
/// `library` is read here and nowhere else, and what it states is written into
/// `record`.
fn collect() -> Result<Record> {
    let dir = Path::new(record::RECORD_DIR);
    let mut record = Record::open(dir);
    let said = collect_into(&mut record, dir, &mut |_, _| {});
    said.state(&journal::stamp_now(), journal::Mode::Collect);
    Ok(record)
}

/// [`collect`] over a loaded `record`, reporting log files opened and log files
/// to open. [`backup::drop_nested`] runs ahead of the pass.
fn collect_into(
    record: &mut Record,
    dir: &Path,
    on: &mut dyn FnMut(usize, usize),
) -> journal::Launch {
    let mut said = journal::Launch::default();
    match backup::drop_nested(dir, record) {
        Ok(Some((went, at))) => eprintln!(
            "record: {went} nested sittings dropped, backed up first to {}",
            at.display()
        ),
        Ok(None) => {}
        Err(err) => eprintln!("!! record: the nested sittings stand: {err}"),
    }
    let pass = record.update(on);
    // `pass.rewound`: the seconds a clock stepping back pulled the watermark
    // back by, the span under it read again.
    if pass.rewound > 0 {
        eprintln!(
            "clock: the Kindle stepped back {}, and the log was read again from there",
            date::duration(pass.rewound, lang::Lang::English.strings()),
        );
    }
    // `library::read` speaks first. `identify::from_sources` asks the three sources
    // for what it left unidentified, and `highlights::fold` joins the two the
    // same walk reaches.
    let books = library::read();
    let stated = record.remember(&books);
    // Both gates, off a walk that opens nothing. With nothing read, highlighted
    // or installed, both stand and no sidecar opens at all.
    let clips = Path::new(clippings::CLIPPINGS_FILE);
    let documents = Path::new(sidecar::DOCUMENTS_DIR);
    let (survey, identifying, highlights) =
        identify::asked(record, Path::new(vocab::VOCAB_DB), clips, documents);
    let sidecars = match identifying.is_some() || highlights {
        true => identify::sidecars_for(
            record,
            documents,
            identifying.is_some(),
            highlights.then_some(dir),
        ),
        false => sidecar::Sidecars::default(),
    };
    // One read of `My Clippings.txt`, for whichever of the two passes wants
    // it: the identifying pass reads it as witnesses and the highlights pass as
    // highlights, and neither may open it for itself.
    let wants_clips =
        identifying.is_some() && record.wants_identifying(record::IdentifiedBy::Clippings);
    let clippings = match highlights || wants_clips {
        true => clippings::read(clips),
        false => Vec::new(),
    };
    // Whether this pass moved the gate, which saves the record like any other change.
    let gated = identifying.is_some();
    // Measured before either pass: both place a true instant on the Kindle's
    // own clock, and this is what says which clock that was.
    if record
        .clock
        .observe(highlights::clocks_seen(&clippings, &sidecars))
    {
        let held: Vec<String> = record
            .clock
            .offsets()
            .iter()
            .map(|o| zone::hhmm(*o))
            .collect();
        eprintln!(
            "clock: {} seen, {}",
            record.clock.rows().len(),
            held.join(" ")
        );
    }
    let identified = identifying.map(|gate| {
        let out = identify::from_sources(record, &clippings, &sidecars);
        // The gate as it stood *before* the pass: a pass that identified something
        // moved the record, and the next launch's gate asks once more and settles.
        record.sources = Some(gate);
        out
    });
    let merge = match highlights {
        true => highlights::fold(record, clips, &clippings, &sidecars, &survey),
        false => highlights::Merge::default(),
    };
    let covers = record.keep_covers(dir);
    let refreshed = stated + identified.map_or(0, |r| r.found()) + covers.kept;
    said.log = Some(format!(
        "log={}/{}l{}c{}d{}s",
        pass.lines, pass.from.live, pass.from.chunks, pass.from.dumps, pass.from.skipped,
    ));
    said.sittings = Some(format!(
        "s=+{}~{}/{}",
        pass.added,
        pass.extended,
        record.sittings.len()
    ));
    said.library = Some(format!("cat={}/{stated}", books.len()));
    said.record = Some(format!("rec={}b", record.books.len()));
    said.covers = Some(covers.said());
    said.identify = identified.as_ref().and_then(identified_said);
    said.highlights = highlights_said(&merge);
    // The books whose covers the Kindle has lost are the same ones every
    // launch: they are named beside the count, not a line apiece.
    if let Some(lost) = covers.lost_said() {
        eprintln!("covers: the Kindle has lost covers for {lost}");
    }
    // An unchanged record is left on disk unwritten.
    if pass.added + pass.extended + refreshed == 0 && !merge.read && !gated {
        return said;
    }
    // A failed `save` leaves `record` drawable and unsaved.
    if let Err(err) = record.save(dir) {
        eprintln!("!! collect: the record was not written: {err}");
    }
    said
}

/// What the sources that identify a book the library cannot came to, one line, and
/// nothing at all where the library left them nothing to do.
fn identified_said(identified: &identify::Identified) -> Option<String> {
    if identified.lookups + identified.clippings + identified.sidecars == 0 {
        return None;
    }
    Some(format!(
        "id={}v{}c{}s/{}?{}",
        identified.by_vocab,
        identified.by_clippings,
        identified.by_sidecars,
        identified.unidentified,
        identified.contested,
    ))
}

/// What the two highlight sources came to, one line, and nothing at all where
/// the gate held and neither was read.
fn highlights_said(merge: &highlights::Merge) -> Option<String> {
    if !merge.read {
        return None;
    }
    Some(format!(
        "ann={}c{}s{}p/{}L{}r{}?",
        merge.clippings, merge.sidecars, merge.parsed, merge.live, merge.retired, merge.unconfirmed,
    ))
}

/// The record as text, one sitting a line.
fn dump() -> Result<()> {
    let record = collect()?;
    let (today, _) = date::now();
    let settings = settings::Settings::load(lang::Lang::detect());
    let stats = Stats::build(
        &record,
        today,
        settings.show_unidentified,
        settings.stats_from,
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
    let dir = Path::new(record::RECORD_DIR);
    let mut record = Record::open(dir);
    let theme = ui::theme::Theme::for_screen(fb.var.xres, fb.var.yres);
    let mut text = ui::text::TextRenderer::load(theme.body_px)?;
    // The facts that only change when the build, the panel or the clock does.
    // A block identical to the one standing is not written again.
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
        record: record.said(),
        zone: zone::describe(date::epoch_now()),
    }
    .state(Path::new(journal::LOG_PATH));
    // `splash` draws before `App` and detects for itself.
    let splash_lang = lang::Lang::detect();
    let note = ui::splash::note(&record.watermark, splash_lang.strings());
    let script = font::Script::of_language(splash_lang.language_tag());
    ui::splash::show(
        &mut fb,
        &mut text,
        &theme,
        &launching(script, &note, ""),
        true,
    )?;

    let mut painted = 0;
    let mut said = collect_into(&mut record, dir, &mut |done, total| {
        if done == painted {
            return;
        }
        painted = done;
        let step = ui::splash::step(splash_lang.strings().step_logs, done, total);
        let step_said = launching(script, &note, &step);
        let _ = ui::splash::show(&mut fb, &mut text, &theme, &step_said, false);
    });

    let mut app = app::App::new(record, theme, text);
    said.drawn = Some(app.drawn());
    said.state(&journal::stamp_now(), journal::Mode::Opened);
    app.run(&mut fb, &mut input)
}
