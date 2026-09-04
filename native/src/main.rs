//! Reading Log — reading statistics on a Kindle, from the Kindle's own logs.
//! Three modes: no argument collects then draws, `--collect` collects alone,
//! `--dump` prints what the store holds.

use std::path::Path;

use anyhow::{Context, Result};

use readinglog_native::eink::buttons::Buttons;
use readinglog_native::eink::fb::Framebuffer;
use readinglog_native::eink::input::Input;
use readinglog_native::eink::touch::Touch;
use readinglog_native::orientation::Orientation;
use readinglog_native::stats::Stats;
use readinglog_native::store::Store;
use readinglog_native::{app, catalog, date, lang, settings, store, ui};

fn main() {
    let mode = std::env::args().nth(1).unwrap_or_default();
    let result = match mode.as_str() {
        "--collect" => collect().map(|_| ()),
        "--dump" => dump(),
        _ => show(),
    };
    if let Err(err) = result {
        eprintln!("readinglog: {err:#}");
        std::process::exit(1);
    }
}

/// Read the log and the catalog into the store, and answer with the store.
/// `catalog` is read here and nowhere else, and what it states is written into
/// `store`.
fn collect() -> Result<Store> {
    let dir = Path::new(store::STORE_DIR);
    let mut store = Store::load(dir);
    collect_into(&mut store, dir, &mut |_, _| {});
    Ok(store)
}

/// [`collect`] over a loaded `store`, reporting log files opened and log files
/// to open.
fn collect_into(store: &mut Store, dir: &Path, on: &mut dyn FnMut(usize, usize)) {
    let pass = store.update(on);
    let books = catalog::read();
    let refreshed = store.remember(&books) + store.keep_covers(dir);
    eprintln!(
        "collect: {} lines (live {}, chunks {}, dumps {}, skipped {}) \
         -> {} added, {} extended, {} sittings held",
        pass.lines,
        pass.from.live,
        pass.from.chunks,
        pass.from.dumps,
        pass.from.skipped,
        pass.added,
        pass.extended,
        store.sessions.len(),
    );
    eprintln!(
        "catalog: {} rows from {}; {} book records refreshed, {} held",
        books.len(),
        catalog::path().map_or("nowhere".into(), |p| p.display().to_string()),
        refreshed,
        store.books.len(),
    );
    // An unchanged store is left on disk unwritten.
    if pass.added + pass.extended + refreshed == 0 {
        return;
    }
    // A failed `save` leaves `store` drawable and unsaved.
    if let Err(err) = store.save(dir) {
        eprintln!("collect: could not write the store: {err}");
    }
}

/// The store as text, one sitting a line.
fn dump() -> Result<()> {
    let store = collect()?;
    let (today, _) = date::now();
    let settings = settings::Settings::load(lang::Lang::detect());
    let stats = Stats::build(&store, today, settings.show_unnamed);
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

/// Collect, then put it on the screen.
fn show() -> Result<()> {
    let mut fb = Framebuffer::open().context("open the display")?;
    let orientation = Orientation::detect();
    let touch =
        Touch::open(orientation, fb.var.xres, fb.var.yres).context("open the touchscreen")?;
    // `Buttons::open` grabs the bezel before the first draw.
    let buttons = Buttons::open().unwrap_or_else(|err| {
        eprintln!("buttons: {err:#} — running touch-only");
        None
    });
    let mut input = Input::new(touch, buttons);
    input.set_orientation(orientation);

    // `splash::show` paints before the first gunzip.
    let dir = Path::new(store::STORE_DIR);
    let mut store = Store::load(dir);
    let theme = ui::theme::Theme::for_screen(fb.var.xres, fb.var.yres);
    let mut text = ui::text::TextRenderer::load(theme.body_px)?;
    eprintln!("fonts: {}", text.chain_description());
    // `splash` draws before `App` and detects for itself.
    let splash_lang = lang::Lang::detect();
    let note = ui::splash::note(&store.mark, splash_lang.strings());
    ui::splash::show(&mut fb, &mut text, &theme, "Reading Log", &note, "", true)?;

    let mut painted = 0;
    collect_into(&mut store, dir, &mut |done, total| {
        if done == painted {
            return;
        }
        painted = done;
        let step = format!("log {done} of {total}");
        let _ = ui::splash::show(
            &mut fb,
            &mut text,
            &theme,
            "Reading Log",
            &note,
            &step,
            false,
        );
    });

    let mut app = app::App::new(store, theme, text);
    eprintln!("stats: {}", app.counted(lang::Lang::English.strings()));
    app.run(&mut fb, &mut input)
}
