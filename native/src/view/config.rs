//! The settings, as sections of rows: a heading with a rule under it, then one
//! row per setting with every value beside it, each its own tap target.
//! A change applies on the tap and is written as it is made.

use crate::font::Script;
use crate::lang::Lang;
use crate::settings::{ColorScheme, Settings, TextSize, WeekStart};
use crate::ui::chrome;
use crate::ui::paint::Rect;
use crate::ui::theme::Theme;
use crate::update;

use super::{Confirm, Ctx, Hit, Reset};

/// The index no option is drawn filled at.
const NONE_ON: usize = usize::MAX;

/// One setting: what it is called, and the values it takes.
struct Row<'a> {
    label: &'a str,
    options: Vec<(String, Script)>,
    on: usize,
    /// What a tap on option `i` does. A closure, not a plain function: the
    /// archive row's chips depend on how many archives there are.
    hit: Box<dyn Fn(usize) -> Hit + 'a>,
    /// Whether this run keeps only the chips its first line holds.
    one_row: bool,
}

/// One line of a section.
enum Line<'a> {
    /// A setting: its name, its values, and which is in use.
    Set(Row<'a>),
    /// A fact the page states and does not set. Its value stands at the same
    /// column the chips do.
    Says { label: &'a str, value: String },
}

impl<'a> Line<'a> {
    fn label(&self) -> &'a str {
        match self {
            Line::Set(row) => row.label,
            Line::Says { label, .. } => label,
        }
    }

    /// The chips this line lays out, which a stated line has none of.
    fn options(&self) -> Vec<(&str, Script)> {
        match self {
            Line::Set(row) => row
                .options
                .iter()
                .map(|(text, script)| (text.as_str(), *script))
                .collect(),
            Line::Says { .. } => Vec::new(),
        }
    }
}

/// A section of the page.
struct Section<'a> {
    heading: &'a str,
    lines: Vec<Line<'a>>,
}

/// What the record section states, gathered before the page is built: how much
/// is in the record, and what archives are on disk.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct Record {
    pub sittings: usize,
    pub books: usize,
    /// One label per archive, newest first, as its chip reads.
    pub backups: Vec<String>,
    /// Whether the record stands on a floor, which is what offers the logs.
    pub floored: bool,
}

impl Record {
    /// The record and the archives beside it, ready for [`sections`].
    pub fn of(
        stats: &crate::stats::Stats,
        dir: &std::path::Path,
        floored: bool,
        lang: Lang,
    ) -> Record {
        Record {
            sittings: stats.sittings.len(),
            books: stats.books.len(),
            backups: labels(&crate::backup::list(dir), lang.strings()),
            floored,
        }
    }
}

/// What each archive's chip reads: the day it was written. Where two fall on
/// one day, both carry the clock as well, there being no other way to tell
/// them apart.
fn labels(held: &[crate::backup::Backup], s: &crate::lang::Strings) -> Vec<String> {
    let days: Vec<String> = held.iter().map(|b| stamp_day(&b.stamp, s)).collect();
    days.iter()
        .enumerate()
        .map(
            |(i, day)| match days.iter().filter(|d| *d == day).count() > 1 {
                true => format!("{day} {}", stamp_clock(&held[i].stamp)),
                false => day.clone(),
            },
        )
        .collect()
}

/// `YYMMDD-HHMMSS` as the day it names, and as `HH:MM`. A stamp that will not
/// parse reads as itself: a name on disk is never worth dropping a row over.
fn stamp_day(stamp: &str, s: &crate::lang::Strings) -> String {
    let n = |r: std::ops::Range<usize>| stamp.get(r).and_then(|t| t.parse::<i64>().ok());
    match (n(0..2), n(2..4), n(4..6)) {
        (Some(y), Some(m), Some(d)) => {
            crate::date::short_day(crate::date::days_from_civil(2000 + y, m, d), s)
        }
        _ => stamp.to_string(),
    }
}

fn stamp_clock(stamp: &str) -> String {
    match (stamp.get(7..9), stamp.get(9..11)) {
        (Some(h), Some(m)) => format!("{h}:{m}"),
        _ => String::new(),
    }
}

/// The page, built from what is set. Kept apart from the drawing: the shape
/// of the page is asserted without a framebuffer.
fn sections<'a>(
    lang: Lang,
    settings: &Settings,
    colour: bool,
    record: &'a Record,
) -> Vec<Section<'a>> {
    let s = lang.strings();
    let plain = Script::of_language(lang.language_tag());

    let language = Row {
        label: s.language_row,
        options: Lang::ALL
            .iter()
            .map(|l| (l.label().to_string(), Script::of_language(l.language_tag())))
            .collect(),
        on: Lang::ALL
            .iter()
            .position(|l| *l == settings.language)
            .unwrap_or(0),
        hit: Box::new(|i| Hit::Language(Lang::ALL[i.min(Lang::ALL.len() - 1)])),
        one_row: false,
    };

    let week = Row {
        label: s.week_starts_on,
        options: WeekStart::ALL
            .iter()
            .map(|w| {
                let day = match w {
                    WeekStart::Monday => s.weekdays_short[0],
                    WeekStart::Sunday => s.weekdays_short[6],
                };
                (day.to_string(), plain)
            })
            .collect(),
        on: WeekStart::ALL
            .iter()
            .position(|w| *w == settings.week_start)
            .unwrap_or(0),
        hit: Box::new(|i| Hit::WeekStart(WeekStart::ALL[i.min(WeekStart::ALL.len() - 1)])),
        one_row: false,
    };

    let size = Row {
        label: s.text_size,
        options: TextSize::ALL
            .iter()
            .map(|t| {
                let name = match t {
                    TextSize::Small => s.size_small,
                    TextSize::Medium => s.size_medium,
                    TextSize::Large => s.size_large,
                };
                (name.to_string(), plain)
            })
            .collect(),
        on: TextSize::ALL
            .iter()
            .position(|t| *t == settings.text_size)
            .unwrap_or(1),
        hit: Box::new(|i| Hit::TextSize(TextSize::ALL[i.min(TextSize::ALL.len() - 1)])),
        one_row: false,
    };

    let scheme = colour.then(|| Row {
        label: s.color_scheme,
        options: s
            .color_schemes
            .iter()
            .map(|name| (name.to_string(), plain))
            .collect(),
        on: ColorScheme::ALL
            .iter()
            .position(|c| *c == settings.color_scheme)
            .unwrap_or(0),
        hit: Box::new(|i| Hit::ColorScheme(ColorScheme::ALL[i.min(ColorScheme::ALL.len() - 1)])),
        one_row: false,
    });

    let unnamed = Row {
        label: s.unnamed_row,
        options: vec![
            (s.unnamed_show.to_string(), plain),
            (s.unnamed_hide.to_string(), plain),
        ],
        on: !settings.show_unnamed as usize,
        hit: Box::new(|i| Hit::ShowUnnamed(i == 0)),
        one_row: false,
    };

    // Never filled: one chip, a button.
    let update = Row {
        label: s.update_row,
        options: vec![(s.update_check.to_string(), plain)],
        on: NONE_ON,
        hit: Box::new(|_| Hit::Update),
        one_row: false,
    };

    // What the reader is about to act on, standing on the page before either
    // chip is touched.
    let recorded = (record.sittings > 0).then(|| Line::Says {
        label: s.recorded_row,
        value: format!(
            "{} · {}",
            crate::lang::counted(s.n_sittings, record.sittings as i64),
            crate::lang::counted(s.n_books, record.books as i64)
        ),
    });

    let reset = (record.sittings > 0).then(|| Row {
        label: s.reset_row,
        options: vec![
            (s.reset_keep.to_string(), plain),
            (s.reset_none.to_string(), plain),
        ],
        on: NONE_ON,
        hit: Box::new(|i| Hit::Wipe(i == 0)),
        one_row: false,
    });

    // The device's own logs where a reset put a floor up, then the archives,
    // newest first. The logs lead: this run keeps one line, and it is the
    // oldest archives that should fall off it rather than the one way back
    // that stands when there are no archives at all.
    let logs = record.floored;
    let restore = (!record.backups.is_empty() || logs).then(|| Row {
        label: s.restore_row,
        options: logs
            .then(|| (s.restore_logs.to_string(), plain))
            .into_iter()
            .chain(record.backups.iter().map(|at| (at.clone(), plain)))
            .collect(),
        on: NONE_ON,
        hit: Box::new(move |i| match (logs, i) {
            (true, 0) => Hit::Rebuild,
            (true, i) => Hit::Restore(i - 1),
            (false, i) => Hit::Restore(i),
        }),
        one_row: true,
    });

    vec![
        Section {
            heading: s.interface,
            lines: [Line::Set(language), Line::Set(size)]
                .into_iter()
                .chain(scheme.map(Line::Set))
                .collect(),
        },
        Section {
            heading: s.the_calendar,
            lines: vec![Line::Set(week)],
        },
        Section {
            heading: s.the_record,
            lines: [Line::Set(unnamed)]
                .into_iter()
                .chain(recorded)
                .chain(reset.map(Line::Set))
                .chain(restore.map(Line::Set))
                .collect(),
        },
        Section {
            heading: s.about,
            lines: vec![
                Line::Says {
                    label: s.version_row,
                    value: update::VERSION.to_string(),
                },
                Line::Set(update),
            ],
        },
    ]
}

/// The air under one section's rows, before the next heading.
fn between(theme: &Theme) -> i32 {
    theme.row_h * 2 / 3
}

/// What a [`Confirm`] says: the headline, the note with its figures filled in,
/// and the label on the answer that carries it out.
pub fn question(confirm: &Confirm, s: &crate::lang::Strings) -> (String, String, String) {
    let what = format!(
        "{} · {}",
        crate::lang::counted(s.n_sittings, confirm.sittings as i64),
        crate::lang::counted(s.n_books, confirm.books as i64)
    );
    let size = bytes(confirm.bytes);
    let filled = |note: &str| {
        note.replace("{what}", &what)
            .replace("{file}", &confirm.named)
            .replace("{size}", &size)
    };
    match confirm.about {
        Reset::Wipe(true) => (
            s.wipe_ask.into(),
            filled(s.wipe_note),
            s.wipe_do.to_string(),
        ),
        Reset::Wipe(false) => (
            s.nowipe_ask.into(),
            filled(s.nowipe_note),
            s.nowipe_do.to_string(),
        ),
        Reset::Restore(_) => (
            s.restore_ask.into(),
            filled(s.restore_note),
            s.restore_do.to_string(),
        ),
        Reset::Rebuild => (
            s.rebuild_ask.into(),
            s.rebuild_note.to_string(),
            s.rebuild_do.to_string(),
        ),
    }
}

/// A size a reader can weigh a decision against: MB to one place, KB under
/// that.
fn bytes(count: u64) -> String {
    match count {
        0..=999_999 => format!("{} KB", count.div_ceil(1024).max(1)),
        _ => format!("{:.1} MB", count as f64 / (1024.0 * 1024.0)),
    }
}

/// A question drawn over the config page, through [`ui::dialog`].
pub fn asking(cx: &mut Ctx, area: Rect, confirm: &Confirm) {
    let s = cx.s();
    let (heading, note, answer) = question(confirm, s);
    let carry = match confirm.about {
        Reset::Wipe(keep) => Hit::Wiped(keep),
        Reset::Restore(at) => Hit::Restored(at),
        Reset::Rebuild => Hit::Rebuilt,
    };
    crate::ui::dialog::draw(
        cx,
        area,
        &crate::ui::dialog::Question {
            heading: &heading,
            note: &note,
            answers: &[(s.cancel, Hit::Dismiss), (&answer, carry)],
        },
    );
}

/// How tall one section draws, heading, rows and the air under it.
fn section_height(cx: &mut Ctx, section: &Section, theme: &Theme, width: i32, air: i32) -> i32 {
    let rows: i32 = section
        .lines
        .iter()
        .map(|line| {
            let options = line.options();
            let placed = chrome::chip_layout(cx.text, theme, &options, width);
            let block = placed.iter().map(|c| c.bottom()).max().unwrap_or(0);
            (block + theme.gap).max(theme.row_h)
        })
        .sum();
    chrome::section_height(cx.text, theme) + rows + air
}

/// Where each page of sections starts and stops, given what each is tall and
/// the room a page has. A section taller than the page gets one to itself and
/// is clipped there, which is the old behaviour and only reachable by a
/// section grown past a whole screen.
fn paged(tall: &[i32], room: i32) -> Vec<(usize, usize)> {
    let mut out: Vec<(usize, usize)> = Vec::new();
    let (mut from, mut used) = (0, 0);
    for (at, high) in tall.iter().enumerate() {
        if used > 0 && used + high > room {
            out.push((from, at));
            (from, used) = (at, 0);
        }
        used += high;
    }
    if from < tall.len() {
        out.push((from, tall.len()));
    }
    out
}

/// Which page of the settings this is, between the two arrows that step it,
/// drawn as All Time draws its own. Each arrow's hit box reaches a third of
/// the way in: an arrow is a small target.
fn pager(cx: &mut Ctx, foot: Rect, at: usize, pages: usize) {
    let theme: &Theme = cx.theme;
    let script = cx.ui_script();
    let said = format!("{} {} {}", at + 1, cx.s().of, pages);
    cx.text.set_px(theme.small_px);
    let w = cx.text.measure_width_in(script, &said) as i32;
    let baseline = foot.y + cx.text.cap_height() as i32;
    cx.text.draw_in(
        script,
        cx.fb,
        foot.x + (foot.w - w) / 2,
        baseline,
        &said,
        false,
    );

    cx.text.set_px(theme.head_px);
    for (arrow, at_left, hit) in [("‹", true, Hit::Prev), ("›", false, Hit::Next)] {
        let reach = foot.w / 3;
        let aw = cx.text.measure_width_in(script, arrow) as i32;
        let (x, from) = match at_left {
            true => (foot.x, foot.x),
            false => (foot.right() - aw, foot.right() - reach),
        };
        cx.text.draw_in(script, cx.fb, x, baseline, arrow, false);
        cx.hit(hit, Rect::new(from, foot.y, reach, foot.h));
    }
}

pub fn draw(
    cx: &mut Ctx,
    area: Rect,
    settings: &Settings,
    colour: bool,
    record: &Record,
    at: usize,
) {
    let theme: &Theme = cx.theme;
    let air = between(theme);
    let page = sections(cx.lang, settings, colour, record);

    // Every line's second column starts at one place, taken from the widest
    // label and pulled back until the widest run fits. No row wraps that need
    // not.
    let labels: Vec<&str> = page
        .iter()
        .flat_map(|s| s.lines.iter().map(Line::label))
        .collect();
    let runs: Vec<Vec<(&str, Script)>> = page
        .iter()
        .flat_map(|s| s.lines.iter().map(Line::options))
        .collect();
    let column = chrome::chip_column(cx.text, theme, &labels, &runs, area.w);
    let width = (area.w - column).max(1);

    // Every section, measured whole, then dealt into pages that fit. Nothing
    // here scrolls, and a section drawn into no room draws over the one above
    // it — so a page that will not hold everything holds what it can and the
    // rest go on the next.
    let tall: Vec<i32> = page
        .iter()
        .map(|s| section_height(cx, s, theme, width, air))
        .collect();
    // Packed against the room a page has once the pager has taken its band —
    // and where that leaves everything on one page, packed again against the
    // whole area, there being no pager to make room for.
    let leaves = match paged(&tall, area.h - theme.row_h).len() > 1 {
        true => paged(&tall, area.h - theme.row_h),
        false => paged(&tall, area.h),
    };
    let pages = leaves.len().max(1);
    let at = at.min(pages - 1);
    let (foot, mut rest) = match pages > 1 {
        true => area.split_bottom(theme.row_h),
        false => (Rect::new(area.x, area.bottom(), area.w, 0), area),
    };
    if pages > 1 {
        pager(cx, foot, at, pages);
    }
    let (from, upto) = leaves.get(at).copied().unwrap_or((0, page.len()));

    for section in page.into_iter().take(upto).skip(from) {
        // Laid out once. The same answer sizes the row and places the chips:
        // a wrapped option never falls outside the height it was given.
        let mut borrowed: Vec<Vec<(&str, Script)>> =
            section.lines.iter().map(Line::options).collect();
        let mut placed: Vec<Vec<Rect>> = borrowed
            .iter()
            .map(|options| chrome::chip_layout(cx.text, theme, options, width))
            .collect();
        // A run that may not wrap keeps what fits on its own line. The page
        // has no scroll: a run free to grow pushes the sections under it off
        // the bottom, and the archives are the one run that grows without end.
        for (at, line) in section.lines.iter().enumerate() {
            if !matches!(line, Line::Set(row) if row.one_row) {
                continue;
            }
            let fits = placed[at].iter().take_while(|c| c.y == 0).count();
            borrowed[at].truncate(fits);
            placed[at].truncate(fits);
        }
        // The height one row's chips take, wrapped runs included.
        let blocks: Vec<i32> = placed
            .iter()
            .map(|row| row.iter().map(|c| c.bottom()).max().unwrap_or(0))
            .collect();
        // Plus air: a row that wrapped clears the next one's chips.
        let heights: Vec<i32> = blocks
            .iter()
            .map(|block| (block + theme.gap).max(theme.row_h))
            .collect();

        let need = chrome::section_height(cx.text, theme) + heights.iter().sum::<i32>() + air;
        let (band, left) = rest.split_top(need.min(rest.h));
        rest = left;

        let mut inner = chrome::section(cx.fb, cx.text, theme, band, section.heading);
        for (at, stated) in section.lines.iter().enumerate() {
            let (line, below) = inner.split_top(heights[at].min(inner.h));
            inner = below;
            chrome::setting(cx.fb, cx.text, theme, line, stated.label());
            let box_ = chip_box(line, column, blocks[at]);
            match stated {
                Line::Says { value, .. } => {
                    // On the chips' own column and on the label's own line: a
                    // fact reads as one of the list, not as a chip that will
                    // not light.
                    let said = Rect::new(box_.x, line.y, (line.right() - box_.x).max(1), line.h);
                    chrome::setting(cx.fb, cx.text, theme, said, value);
                }
                Line::Set(row) => {
                    let chips = chrome::chips(
                        cx.fb,
                        cx.text,
                        theme,
                        box_,
                        &borrowed[at],
                        &placed[at],
                        row.on,
                    );
                    for (i, chip) in chips.into_iter().enumerate() {
                        cx.hit((row.hit)(i), chip);
                    }
                }
            }
        }
    }
}

/// Where a row's chips sit: right of the shared label column, and a `block`
/// tall run centred against the label, which `chrome::setting` centres in the
/// row.
fn chip_box(row: Rect, column: i32, block: i32) -> Rect {
    let left = row.x + column;
    let top = row.y + (row.h - block).max(0) / 2;
    Rect::new(left, top, (row.right() - left).max(1), row.h)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The setting at line `at` of section `of`. Panics on a [`Line::Says`].
    fn row<'a>(page: &'a [Section<'a>], of: usize, at: usize) -> &'a Row<'a> {
        match &page[of].lines[at] {
            Line::Set(row) => row,
            Line::Says { label, .. } => panic!("{label} states, it does not set"),
        }
    }

    #[test]
    fn a_rows_chips_centre_against_its_label() {
        let theme = Theme::for_screen(1264, 1680);
        let row = Rect::new(0, 100, 1186, theme.row_h);
        // `chrome::setting` sets the label on `row.center_y()`.
        for block in [theme.row_h / 3, chrome::chip_height(&theme), theme.row_h] {
            let box_ = chip_box(row, 300, block);
            assert_eq!(box_.y + block / 2, row.center_y(), "a {block} px run");
            assert_eq!(box_.x, row.x + 300, "a {block} px run");
        }
        // A run taller than its row opens at the top of it.
        let tall = chip_box(row, 300, theme.row_h * 2);
        assert_eq!(tall.y, row.y);
    }

    #[test]
    fn the_sections_stand_a_half_row_apart() {
        for (w, h) in [(1264, 1680), (1272, 1696), (1860, 2480)] {
            let theme = Theme::for_screen(w, h);
            let air = between(&theme);
            assert!(air >= theme.row_h / 2, "{w}x{h}: {air} px between sections");
            assert!(air < theme.row_h, "{w}x{h}: {air} px reads as a blank row");
        }
    }

    #[test]
    fn the_page_holds_more_than_one_setting() {
        let settings = Settings::new(Lang::English);
        let empty = Record::default();
        let page = sections(Lang::English, &settings, true, &empty);
        assert!(page.len() >= 2, "a page of one section is a stub");
        let lines: usize = page.iter().map(|s| s.lines.len()).sum();
        assert!(lines >= 2, "got {lines} lines");
        for section in &page {
            assert!(!section.heading.is_empty());
            assert!(!section.lines.is_empty(), "a heading with nothing under it");
            for line in &section.lines {
                assert!(!line.label().is_empty(), "an unnamed line");
            }
        }
    }

    #[test]
    fn every_language_is_offered_and_the_set_one_is_lit() {
        // No Automatic chip: the device's language is the default, and the
        // default is simply the one that starts out lit.
        let mut settings = Settings::new(Lang::Japanese);
        let empty = Record::default();
        let page = sections(Lang::English, &settings, true, &empty);
        let language = row(&page, 0, 0);
        assert_eq!(
            language.options.len(),
            Lang::ALL.len(),
            "one chip per language"
        );
        assert_eq!(language.on, 4, "the device's Japanese is what is lit");

        settings.language = Lang::TraditionalChinese;
        let empty = Record::default();
        let page = sections(Lang::English, &settings, true, &empty);
        assert_eq!(row(&page, 0, 0).on, 3);
    }

    #[test]
    fn each_language_names_itself_in_its_own_script() {
        // 日本語 drawn from a Simplified face is the defect this prevents.
        let settings = Settings::new(Lang::English);
        let empty = Record::default();
        let page = sections(Lang::English, &settings, true, &empty);
        let by_name = |want: &str| {
            row(&page, 0, 0)
                .options
                .iter()
                .find(|(text, _)| text == want)
                .map(|(_, script)| *script)
                .expect(want)
        };
        assert_eq!(by_name("日"), Script::Japanese);
        assert_eq!(by_name("简"), Script::SimplifiedChinese);
        assert_eq!(by_name("繁"), Script::TraditionalChinese);
        assert_eq!(by_name("DE"), Script::Unknown);
    }

    #[test]
    fn the_size_row_offers_every_size_and_lights_the_set_one() {
        let mut settings = Settings::new(Lang::English);
        settings.text_size = TextSize::Large;
        let empty = Record::default();
        let page = sections(Lang::English, &settings, true, &empty);
        let size = row(&page, 0, 1);
        assert_eq!(size.options.len(), TextSize::ALL.len());
        assert_eq!(size.on, 2);
        assert_eq!((size.hit)(0), Hit::TextSize(TextSize::Small));
    }

    #[test]
    fn the_week_row_names_its_days_in_the_interface_s_language() {
        let settings = Settings::new(Lang::German);
        let empty = Record::default();
        let page = sections(Lang::German, &settings, true, &empty);
        let week = row(&page, 1, 0);
        assert_eq!(week.options[0].0, "Mo");
        assert_eq!(week.options[1].0, "So");
    }

    #[test]
    fn the_page_states_which_build_it_is_and_offers_a_newer_one() {
        let settings = Settings::new(Lang::English);
        let empty = Record::default();
        let page = sections(Lang::English, &settings, true, &empty);
        let about = page.last().expect("a section");

        let Line::Says { label, value } = &about.lines[0] else {
            panic!("the version is stated, not set");
        };
        assert_eq!(*label, Lang::English.strings().version_row);
        assert_eq!(value, crate::update::VERSION);

        // One chip, never lit: it is a button, and nothing it does is a state
        // this page could be showing.
        let update = row(&page, page.len() - 1, 1);
        assert_eq!(update.options.len(), 1);
        assert_eq!((update.hit)(0), Hit::Update);
        assert!(update.on >= update.options.len(), "a button drawn filled");
    }

    /// The record section of a page built over `record`.
    fn the_record(record: &Record) -> Vec<String> {
        let settings = Settings::new(Lang::English);
        let page = sections(Lang::English, &settings, true, record);
        let section = page
            .into_iter()
            .find(|s| s.heading == Lang::English.strings().the_record)
            .expect("the record section");
        section
            .lines
            .iter()
            .map(|l| l.label().to_string())
            .collect()
    }

    #[test]
    fn a_record_with_nothing_in_it_offers_no_reset() {
        assert_eq!(
            the_record(&Record::default()),
            [Lang::English.strings().unnamed_row]
        );
    }

    #[test]
    fn a_record_with_reading_in_it_states_what_it_holds_and_offers_a_reset() {
        let s = Lang::English.strings();
        let record = Record {
            sittings: 12,
            books: 3,
            ..Record::default()
        };
        assert_eq!(
            the_record(&record),
            [s.unnamed_row, s.recorded_row, s.reset_row]
        );
    }

    #[test]
    fn the_archives_row_stands_for_an_archive_or_for_a_floor() {
        let s = Lang::English.strings();
        let kept = Record {
            sittings: 12,
            books: 3,
            backups: vec!["Sep 6".into()],
            floored: false,
        };
        assert_eq!(
            the_record(&kept),
            [s.unnamed_row, s.recorded_row, s.reset_row, s.restore_row]
        );
        let floored = Record {
            backups: Vec::new(),
            floored: true,
            ..kept
        };
        assert_eq!(
            the_record(&floored),
            [s.unnamed_row, s.recorded_row, s.reset_row, s.restore_row]
        );
    }

    #[test]
    fn the_logs_lead_the_archives_and_answer_for_the_first_chip() {
        let settings = Settings::new(Lang::English);
        let record = Record {
            sittings: 12,
            books: 3,
            backups: vec!["Sep 6".into(), "Aug 30".into()],
            floored: true,
        };
        let page = sections(Lang::English, &settings, true, &record);
        let at = page.len() - 2;
        let archives = row(&page, at, 3);
        assert_eq!(archives.options[0].0, Lang::English.strings().restore_logs);
        assert_eq!((archives.hit)(0), Hit::Rebuild);
        assert_eq!((archives.hit)(1), Hit::Restore(0));
        assert_eq!((archives.hit)(2), Hit::Restore(1));
        assert!(archives.one_row, "the run would grow without end");
    }

    #[test]
    fn every_page_of_settings_holds_what_it_can_and_no_more() {
        // Three sections into a page holding two of them.
        assert_eq!(paged(&[100, 100, 100], 250), [(0, 2), (2, 3)]);
        // Everything on one page where it fits.
        assert_eq!(paged(&[100, 100], 250), [(0, 2)]);
        // A section taller than the page gets one to itself.
        assert_eq!(paged(&[100, 400, 100], 250), [(0, 1), (1, 2), (2, 3)]);
        assert_eq!(paged(&[], 250), Vec::new());
    }

    #[test]
    fn a_size_reads_as_a_figure_a_reader_can_weigh() {
        assert_eq!(bytes(0), "1 KB");
        assert_eq!(bytes(2_048), "2 KB");
        assert_eq!(bytes(5_000_000), "4.8 MB");
    }

    #[test]
    fn an_archive_chip_names_the_day_and_the_clock_where_two_share_one() {
        let s = Lang::English.strings();
        let one = crate::backup::Backup {
            path: std::path::PathBuf::from("a.zip"),
            stamp: "260906-010231".into(),
            kind: crate::backup::Kind::Record,
            bytes: 0,
        };
        let two = crate::backup::Backup {
            stamp: "260906-184500".into(),
            ..one.clone()
        };
        let other = crate::backup::Backup {
            stamp: "260830-090000".into(),
            ..one.clone()
        };
        assert_eq!(
            labels(&[one.clone(), other.clone()], s),
            ["Sep 6", "Aug 30"]
        );
        assert_eq!(
            labels(&[one, two, other], s),
            ["Sep 6 01:02", "Sep 6 18:45", "Aug 30"]
        );
    }

    #[test]
    fn every_language_names_the_about_section_and_its_button() {
        for lang in Lang::ALL {
            let settings = Settings::new(lang);
            let empty = Record::default();
            let page = sections(lang, &settings, true, &empty);
            let about = page.last().expect("a section");
            assert_eq!(about.heading, lang.strings().about, "{lang:?}");
            let update = row(&page, page.len() - 1, 1);
            assert!(!update.options[0].0.is_empty(), "{lang:?}");
        }
    }

    #[test]
    fn a_tap_names_the_option_under_it() {
        let settings = Settings::new(Lang::English);
        let empty = Record::default();
        let page = sections(Lang::English, &settings, true, &empty);
        let language = row(&page, 0, 0);
        for (i, lang) in Lang::ALL.iter().enumerate() {
            assert_eq!((language.hit)(i), Hit::Language(*lang));
        }
        // A chip index past the end cannot panic: the row is drawn from the
        // same list, but the two are separated by the paint.
        assert_eq!((language.hit)(99), Hit::Language(Lang::Japanese));
    }
}
