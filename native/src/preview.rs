//! Every screen drawn to a PNG.
//!
//! [`preview_every_screen`] takes its directory from [`out_dir`].

use std::path::{Path, PathBuf};

use crate::app::App;
use crate::date;
use crate::eink::fb::Framebuffer;
use crate::lang::Lang;
use crate::log::session::{Measure, Session};
use crate::settings::TextSize;
use crate::stats::Stats;
use crate::store::{BookRecord, Store};
use crate::ui::chrome::Tab;
use crate::ui::text::TextRenderer;
use crate::ui::theme::Theme;
use crate::view::Span;

/// Days of reading [`fixture`] lays down behind `today`.
const DAYS: i64 = 40;

/// An invented shelf: titles in three scripts, from one word to a line and a
/// half, and progress from 2 to 70.
const SHELF: &[(&str, &str, &str, f64)] = &[
    ("灰的重量", "何允之", "zh", 4.0),
    ("ねむらない街の図鑑 ～第一巻～", "白鳥ゆかり", "ja", 70.0),
    (
        "A Complete History of Nothing in Particular, with Notes and an Index, Volume 1",
        "Margaret Ellery",
        "en",
        6.0,
    ),
    ("夢遊症候群", "林素", "zh", 43.0),
    (
        "沒有名字的河：一段流域史與它的居民",
        "周牧（Anne Chou）",
        "zh",
        9.0,
    ),
    ("Writing the Slow Chase", "Iris Vandermeer", "en", 17.0),
    (
        "The Ninth Winter and Other Stories",
        "Cordelia Nash",
        "en",
        4.0,
    ),
    ("Interval", "Tomas Reidy", "en", 2.0),
];

/// Where the PNGs land: `READINGLOG_PREVIEW_OUT`, else a scratch directory.
fn out_dir() -> PathBuf {
    match std::env::var("READINGLOG_PREVIEW_OUT") {
        Ok(dir) if !dir.is_empty() => PathBuf::from(dir),
        _ => std::env::temp_dir().join("readinglog-preview"),
    }
}

/// A stand-in jacket for the book at `slot`, 217x330.
///
/// Two bands and a block.
fn jacket(dir: &Path, slot: usize) -> String {
    let (w, h) = (217u32, 330u32);
    let hue = [
        [200u8, 40, 40],
        [30, 60, 140],
        [180, 140, 40],
        [40, 120, 90],
        [90, 40, 130],
        [220, 180, 40],
        [40, 40, 40],
        [150, 90, 60],
    ][slot % 8];
    let mut img = image::RgbImage::from_pixel(w, h, image::Rgb(hue));
    for y in 0..h {
        for x in 0..w {
            let band = (h / 3..h / 3 + h / 12).contains(&y);
            let block = (h * 2 / 3..h * 2 / 3 + h / 5).contains(&y) && (20..w - 20).contains(&x);
            if band || block {
                img.put_pixel(x, y, image::Rgb([250, 250, 250]));
            }
        }
    }
    let path = dir.join(format!("cover{slot}.png"));
    img.save(&path).expect("a written jacket");
    path.display().to_string()
}

/// One sitting, ending `minutes` after the hour it started on.
fn sitting(day: i64, hour: i64, minutes: i64, extent: i64, turns: i64) -> Session {
    let (y, m, d) = date::civil_from_days(day);
    let stamp = |secs: i64| {
        format!(
            "{y:04}-{m:02}-{d:02}T{:02}:{:02}:{:02}",
            (secs / 3600).min(23),
            (secs / 60) % 60,
            secs % 60
        )
    };
    Session {
        started_at: stamp(hour * 3600),
        ended_at: stamp(hour * 3600 + minutes * 60),
        end_position: extent,
        seconds: minutes * 60,
        page_turns: turns,
        words: turns * 260,
        hours: vec![(hour as u8, minutes * 60)],
        measure: Measure::Counted,
        asin: None,
        progress: None,
    }
}

/// A store holding [`SHELF`] and [`DAYS`] of reading over it.
fn fixture(today: i64, art: &Path) -> Store {
    let mut store = Store::default();
    for (slot, (title, author, language, percent)) in SHELF.iter().enumerate() {
        store.books.push(BookRecord {
            extent: 100_000 + slot as i64,
            cde_key: format!("KEY{slot}"),
            title: (*title).into(),
            author: (*author).into(),
            thumbnail: jacket(art, slot),
            language: (*language).into(),
            percent: *percent,
            on_device: slot % 5 != 4,
            cover: String::new(),
        });
    }

    // One or two sittings a day, none on some.
    for back in 0..DAYS {
        let day = today - back;
        if back % 9 == 4 {
            continue;
        }
        let slot = (back % SHELF.len() as i64) as usize;
        let extent = store.books[slot].extent;
        let minutes = 12 + (back * 7) % 51;
        store
            .sessions
            .push(sitting(day, 7 + back % 3, minutes, extent, minutes / 2));
        if back % 3 != 1 {
            let other = ((back + 3) % SHELF.len() as i64) as usize;
            let extent = store.books[other].extent;
            let minutes = 8 + (back * 11) % 37;
            store
                .sessions
                .push(sitting(day, 21 + back % 2, minutes, extent, minutes / 2));
        }
    }
    for (hour, minutes, slot) in [(5i64, 22i64, 0usize), (6, 41, 1), (11, 23, 3)] {
        let extent = store.books[slot].extent;
        store
            .sessions
            .push(sitting(today, hour, minutes, extent, minutes * 2));
    }
    // A narrow window with dwell booked to two far-apart hours.
    let (y, m, d) = date::civil_from_days(today);
    store.sessions.push(Session {
        started_at: format!("{y:04}-{m:02}-{d:02}T03:10:00"),
        ended_at: format!("{y:04}-{m:02}-{d:02}T03:10:20"),
        end_position: store.books[2].extent,
        seconds: 600,
        page_turns: 20,
        words: 5_200,
        hours: vec![(3, 300), (9, 300)],
        measure: Measure::Dwell,
        asin: None,
        progress: None,
    });
    store
        .sessions
        .sort_by(|a, b| a.started_at.cmp(&b.started_at));
    store
}

/// [`fixture`] with `keep` of today's sittings left on it: how Today reads on
/// a day that has barely started, and on one with nothing on it yet.
fn thinned(today: i64, art: &Path, keep: usize) -> Store {
    let mut store = fixture(today, art);
    let mut seen = 0usize;
    store.sessions.retain(|s| {
        if date::parse_day(date::day_of(&s.started_at)) != Some(today) {
            return true;
        }
        seen += 1;
        seen <= keep
    });
    store
}

/// Every screen, one PNG each.
///
/// Ignored: [`Framebuffer::open`] wants a display.
#[test]
#[ignore]
fn preview_every_screen() {
    let out = out_dir();
    let art = out.join("art");
    std::fs::create_dir_all(&art).expect("a preview directory");

    let mut fb = Framebuffer::open().expect("a display — start Xvfb and set DISPLAY");
    let theme = Theme::for_screen(fb.var.xres, fb.var.yres);
    let text = TextRenderer::load(theme.body_px).expect("a font — set READINGLOG_FONTS");

    let (today, _) = date::now();
    let store = fixture(today, &art);
    let stats = Stats::build(&store, today);
    assert!(!stats.books.is_empty(), "the fixture named no book");

    let mut app = App::new(stats, theme, text);
    let screens = [
        ("config", Tab::Config, None),
        ("today", Tab::Home, None),
        ("rhythm", Tab::Rhythm, None),
        ("books", Tab::Books, None),
        ("book", Tab::Books, Some(0)),
    ];
    // Every screen in every language: German is where a row collides first,
    // and a CJK label set in the wrong face shows nowhere else.
    for lang in Lang::ALL {
        app.set_language(lang);
        let dir = match lang {
            Lang::English => out.clone(),
            other => out.join(other.language_tag()),
        };
        std::fs::create_dir_all(&dir).expect("a language directory");
        for (name, tab, book) in screens {
            app.show(tab, book);
            app.draw(&mut fb).expect("a drawn screen");
            let path = dir.join(format!("{name}.png"));
            fb.capture_png(&path).expect("a written screen");
            println!("preview: {}", path.display());
        }
    }

    // Today on a day that has barely started, and on one with nothing on it:
    // the page is sized by what was read, and both ends have to hold up.
    app.set_language(Lang::English);
    for (name, keep) in [("today-quiet", 1usize), ("today-empty", 0)] {
        let stats = Stats::build(&thinned(today, &art, keep), today);
        let theme = Theme::for_screen(fb.var.xres, fb.var.yres);
        let text = TextRenderer::load(theme.body_px).expect("a font");
        let mut thin = App::new(stats, theme, text);
        thin.show(Tab::Home, None);
        thin.draw(&mut fb).expect("a drawn screen");
        let path = out.join(format!("{name}.png"));
        fb.capture_png(&path).expect("a written screen");
        println!("preview: {}", path.display());
    }

    // Rhythm at each zoom: the grid changes shape and the page holds.
    app.set_language(Lang::English);
    app.show(Tab::Rhythm, None);
    for span in Span::ALL {
        app.set_span(span);
        app.draw(&mut fb).expect("a drawn screen");
        let path = out.join(format!("rhythm-{}.png", span_token(span)));
        fb.capture_png(&path).expect("a written screen");
        println!("preview: {}", path.display());
    }
    // A day opened off the calendar, which is where its books are read.
    app.open_day(today);
    app.draw(&mut fb).expect("a drawn screen");
    let path = out.join("rhythm-day.png");
    fb.capture_png(&path).expect("a written screen");
    println!("preview: {}", path.display());
    app.set_span(Span::Month);

    // The size ladder, in English: the content grows and the strip does not.
    for size in TextSize::ALL {
        app.set_text_size(size);
        for (name, tab, book) in [
            ("today", Tab::Home, None),
            ("rhythm", Tab::Rhythm, None),
            ("config", Tab::Config, None),
        ] {
            app.show(tab, book);
            app.draw(&mut fb).expect("a drawn screen");
            let path = out.join(format!("{name}-{}.png", size_token(size)));
            fb.capture_png(&path).expect("a written screen");
            println!("preview: {}", path.display());
        }
    }
}

/// What a span is called in a filename.
fn span_token(span: Span) -> &'static str {
    match span {
        Span::Week => "week",
        Span::Month => "month",
        Span::Year => "year",
    }
}

/// What a size is called in a filename.
fn size_token(size: TextSize) -> &'static str {
    match size {
        TextSize::Small => "small",
        TextSize::Medium => "medium",
        TextSize::Large => "large",
    }
}
