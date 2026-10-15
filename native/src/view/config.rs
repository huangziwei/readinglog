//! The settings, as sections of rows: a heading with a rule under it, then one
//! row per setting with every value beside it, each its own tap target.

use crate::font::Script;
use crate::lang::Lang;
use crate::settings::{ColorScheme, Settings, SittingFloor, StatsFrom, TextSize, WeekStart};
use crate::ui::chrome;
use crate::ui::paint::Rect;
use crate::ui::theme::Theme;
use crate::update;

use super::{Act, Confirm, Ctx, Hit, pager};

/// The index no option is drawn filled at.
const NONE_ON: usize = usize::MAX;

/// One setting: what it is called, and the values it takes.
struct Row<'a> {
    label: &'a str,
    options: Vec<(String, Script)>,
    on: usize,
    /// What a tap on option `i` does.
    hit: Box<dyn Fn(usize) -> Hit + 'a>,
    /// Whether this run keeps only the chips its first line holds.
    one_row: bool,
    /// The chip this run breaks before, set off from the values beside it: a
    /// button standing in a row of settings, never one of the set.
    apart: Option<usize>,
}

/// One line of a section.
enum Line<'a> {
    /// A setting: its name, its values, and which is in use.
    Set(Row<'a>),
    /// A fact the page states and does not set, `value` at the same column
    /// the chips stand at.
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

    /// Where this line's run breaks, which a stated line has none of.
    fn apart(&self) -> Option<usize> {
        match self {
            Line::Set(row) => row.apart,
            Line::Says { .. } => None,
        }
    }
}

/// A section of the page.
struct Section<'a> {
    heading: &'a str,
    /// A figure the section is measured in, stated on the heading's own line,
    /// taking no row and no chip.
    said: Option<String>,
    lines: Vec<Line<'a>>,
}

/// What the record section states, gathered before the page is built: how much
/// is in the record, and what backups are on disk.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct Summary {
    /// The length of `Record::sittings`, which [`Act::Reset`] deletes.
    pub sittings: usize,
    /// [`crate::stats::Stats::book_count`].
    pub books: usize,
    /// How many backups are on disk, which is what offers the list.
    pub backups: usize,
    /// What the backups take on disk, together.
    pub backup_bytes: u64,
    /// Whether the record was reset, which is what offers a rebuild from the logs.
    pub was_reset: bool,
}

impl Summary {
    /// The record and the backups beside it, ready for [`sections`].
    pub fn of(
        record: &crate::record::Record,
        stats: &crate::stats::Stats,
        dir: &std::path::Path,
        was_reset: bool,
    ) -> Summary {
        Summary {
            sittings: record.sittings.len(),
            books: stats.book_count(),
            backups: crate::backup::list(dir).len(),
            backup_bytes: crate::backup::sizes(dir).1,
            was_reset,
        }
    }

    /// Every backup under `dir` as [`crate::view::backups`] lists it, and the
    /// figure its heading states.
    pub fn listed(
        dir: &std::path::Path,
        record: &crate::record::Record,
        lang: Lang,
    ) -> crate::view::backups::Listed {
        let s = lang.strings();
        let held = crate::backup::list(dir);
        let rows = held
            .iter()
            .map(|backup| {
                let said = crate::backup::about(&backup.path).unwrap_or_default();
                row(backup, &said, record, s)
            })
            .collect();
        crate::view::backups::Listed {
            rows,
            said: s
                .n_files
                .replace("{n}", &held.len().to_string())
                .replace("{size}", &bytes(held.iter().map(|b| b.bytes).sum())),
        }
    }
}

/// `backup` as [`crate::view::backups`] lists it, by what `said` states of it:
/// when it was written, what it weighs, and what it holds against `record`.
pub fn row(
    backup: &crate::backup::Backup,
    said: &crate::backup::About,
    record: &crate::record::Record,
    s: &crate::lang::Strings,
) -> crate::view::backups::Row {
    crate::view::backups::Row {
        when: when(backup, said, s),
        size: bytes(backup.bytes),
        holds: holds(&backup.path, record, said, s),
    }
}

/// The clock a backup was written at, else the file's own name.
fn when(
    backup: &crate::backup::Backup,
    said: &crate::backup::About,
    s: &crate::lang::Strings,
) -> String {
    let stamp = match said.written.is_empty() {
        true => backup.stamp.clone(),
        false => said.written.replace(':', "-"),
    };
    let day = stamp_day(&stamp, s);
    match stamp.len() >= 13 {
        true => format!("{day} {}", stamp_clock(&stamp)),
        false => day,
    }
}

/// What a backup holds, and whether `record` holds all of it.
fn holds(
    at: &std::path::Path,
    record: &crate::record::Record,
    said: &crate::backup::About,
    s: &crate::lang::Strings,
) -> String {
    if said.is_empty() {
        return s.backup_silent.to_string();
    }
    // A backup stating no first or last day states its count alone.
    let mut out = match said.first.is_empty() || said.last.is_empty() {
        true => crate::lang::counted(s.n_sittings, said.sittings as i64),
        false => s
            .backup_holds
            .replace("{n}", &said.sittings.to_string())
            .replace("{from}", &said.first)
            .replace("{to}", &said.last),
    };
    if crate::backup::peek(at).is_ok_and(|inside| crate::backup::holds(record, &inside)) {
        out.push_str(" · ");
        out.push_str(s.backup_whole);
    }
    out
}

/// `YYMMDD-HHMMSS` as the day it names, and as `HH:MM`; a `stamp` that will
/// not parse reads as itself.
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

/// What the record section states on its heading: the sittings and books the
/// record holds and what the backups take, `None` for an empty record.
pub fn recorded(summary: &Summary, s: &crate::lang::Strings) -> Option<String> {
    (summary.sittings > 0).then(|| {
        let mut said = format!(
            "{} · {}",
            crate::lang::counted(s.n_sittings, summary.sittings as i64),
            crate::lang::counted(s.n_books, summary.books as i64)
        );
        // `summary.backup_bytes`, where any backup is on disk.
        if summary.backup_bytes > 0 {
            said.push_str(" · ");
            said.push_str(
                &s.n_in_backups
                    .replace("{size}", &bytes(summary.backup_bytes)),
            );
        }
        said
    })
}

/// The page, built from what is set.
fn sections<'a>(
    lang: Lang,
    settings: &Settings,
    colour: bool,
    summary: &'a Summary,
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
        apart: None,
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
        apart: None,
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
        apart: None,
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
        apart: None,
    });

    let stats_row = Row {
        label: s.stats_from_row,
        options: vec![
            (s.stats_from_kindle.to_string(), plain),
            (s.stats_from_app.to_string(), plain),
        ],
        on: StatsFrom::ALL
            .iter()
            .position(|f| *f == settings.stats_from)
            .unwrap_or(0),
        hit: Box::new(|i| Hit::StatsFrom(StatsFrom::ALL[i.min(StatsFrom::ALL.len() - 1)])),
        one_row: false,
        apart: None,
    };

    let sitting_floor = Row {
        label: s.sitting_floor_row,
        options: SittingFloor::ALL
            .iter()
            .map(|floor| (floor.label(s), plain))
            .collect(),
        on: SittingFloor::ALL
            .iter()
            .position(|floor| *floor == settings.sitting_floor)
            .unwrap_or(0),
        hit: Box::new(|i| Hit::SittingFloor(SittingFloor::ALL[i.min(SittingFloor::ALL.len() - 1)])),
        one_row: false,
        apart: None,
    };

    let unidentified = Row {
        label: s.unidentified_row,
        options: vec![
            (s.chip_show.to_string(), plain),
            (s.chip_hide.to_string(), plain),
        ],
        on: !settings.show_unidentified as usize,
        hit: Box::new(|i| Hit::ShowUnidentified(i == 0)),
        one_row: false,
        apart: None,
    };

    // `Hit::ShowUncovered` hides rows and empty boxes; every total keeps the books.
    let uncovered = Row {
        label: s.uncovered_row,
        options: vec![
            (s.chip_show.to_string(), plain),
            (s.chip_hide.to_string(), plain),
        ],
        on: !settings.show_uncovered as usize,
        hit: Box::new(|i| Hit::ShowUncovered(i == 0)),
        one_row: false,
        apart: None,
    };

    // `NONE_ON`: one chip, a button, never filled.
    let update = Row {
        label: s.update_row,
        options: vec![(s.update_check.to_string(), plain)],
        on: NONE_ON,
        hit: Box::new(|_| Hit::Update),
        one_row: false,
        apart: None,
    };

    // `recorded` measures the record `reset` and `restore` act on, on the heading.
    let recorded = recorded(summary, s);

    let reset = (summary.sittings > 0).then(|| Row {
        label: s.reset_row,
        options: vec![
            (s.reset_keep.to_string(), plain),
            (s.reset_none.to_string(), plain),
        ],
        on: NONE_ON,
        hit: Box::new(|i| Hit::Reset(i == 0)),
        one_row: false,
        apart: None,
    });

    // `restore`: two buttons and, where `summary.was_reset`, the rebuild set
    // apart; `view::backups` lists the backups.
    let logs = summary.was_reset;
    let lists = summary.backups > 0;
    let anything = summary.sittings > 0 || lists || logs;
    let restore = anything.then(|| Row {
        label: s.backups_row,
        options: [(s.back_up.to_string(), plain)]
            .into_iter()
            .chain(lists.then(|| (s.restore.to_string(), plain)))
            .chain(logs.then(|| (s.rebuild.to_string(), plain)))
            .collect(),
        on: NONE_ON,
        hit: Box::new(move |i| match (i, lists) {
            (0, _) => Hit::BackUp,
            (1, true) => Hit::Backups,
            _ => Hit::Rebuild,
        }),
        one_row: false,
        apart: Some(1 + lists as usize),
    });

    vec![
        Section {
            heading: s.interface,
            said: None,
            lines: [Line::Set(language), Line::Set(size)]
                .into_iter()
                .chain(scheme.map(Line::Set))
                .collect(),
        },
        Section {
            heading: s.the_calendar,
            said: None,
            lines: vec![Line::Set(week)],
        },
        Section {
            // `unidentified` and `uncovered` lead under `recorded`, then
            // `stats_row` and `sitting_floor`.
            heading: s.the_record,
            said: recorded,
            lines: [
                Line::Set(unidentified),
                Line::Set(uncovered),
                Line::Set(stats_row),
                Line::Set(sitting_floor),
            ]
            .into_iter()
            .chain(reset.map(Line::Set))
            .chain(restore.map(Line::Set))
            .collect(),
        },
        Section {
            heading: s.about,
            said: None,
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
        Act::Reset(true) => (
            s.reset_keep_ask.into(),
            filled(s.reset_keep_note),
            s.reset_keep_do.to_string(),
        ),
        Act::Reset(false) => (
            s.reset_none_ask.into(),
            filled(s.reset_none_note),
            s.reset_none_do.to_string(),
        ),
        Act::Restore(_) => (
            s.restore_ask.into(),
            filled(s.restore_note),
            s.restore_do.to_string(),
        ),
        Act::Rebuild => (
            s.rebuild_ask.into(),
            s.rebuild_note.to_string(),
            s.rebuild_do.to_string(),
        ),
    }
}

/// `count` as MB to one place, KB under that.
pub fn bytes(count: u64) -> String {
    match count {
        0..=999_999 => format!("{} KB", count.div_ceil(1024).max(1)),
        _ => format!("{:.1} MB", count as f64 / (1024.0 * 1024.0)),
    }
}

/// A question drawn over the config page, through [`crate::ui::dialog`].
pub fn asking(cx: &mut Ctx, area: Rect, confirm: &Confirm) {
    let s = cx.s();
    let (heading, note, answer) = question(confirm, s);
    let carry = match confirm.about {
        Act::Reset(keep) => Hit::ResetDone(keep),
        Act::Restore(at) => Hit::Restored(at),
        Act::Rebuild => Hit::Rebuilt,
    };
    // `Act::Restore` carries a third answer, `Hit::Deleted`.
    let mut answers: Vec<(&str, Hit)> = vec![(s.cancel, Hit::Dismiss), (&answer, carry)];
    if let Act::Restore(at) = confirm.about {
        answers.push((s.delete_do, Hit::Deleted(at)));
    }
    crate::ui::dialog::draw(
        cx,
        area,
        &crate::ui::dialog::Question {
            heading: &heading,
            note: &note,
            answers: &answers,
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
            let placed = chrome::chip_layout(cx.text, theme, &options, line.apart(), width);
            let block = placed.iter().map(|c| c.bottom()).max().unwrap_or(0);
            (block + theme.gap).max(theme.row_h)
        })
        .sum();
    chrome::section_height(cx.text, theme) + rows + air
}

/// Where each page of sections starts and stops, from what each is tall in
/// `tall` and the `room` a page has; one taller than `room` takes a page to
/// itself, clipped there.
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

pub fn draw(
    cx: &mut Ctx,
    area: Rect,
    settings: &Settings,
    colour: bool,
    summary: &Summary,
    at: usize,
) {
    let theme: &Theme = cx.theme;
    let air = between(theme);
    let page = sections(cx.lang, settings, colour, summary);

    // `column` starts every line's second column at one place: the widest
    // label, pulled back until the widest run fits.
    let labels: Vec<&str> = page
        .iter()
        .flat_map(|s| s.lines.iter().map(Line::label))
        .collect();
    let runs: Vec<chrome::Run> = page
        .iter()
        .flat_map(|s| s.lines.iter().map(|line| (line.options(), line.apart())))
        .collect();
    let column = chrome::chip_column(cx.text, theme, &labels, &runs, area.w);
    let width = (area.w - column).max(1);

    // Each section's whole height, which `paged` deals into pages.
    let tall: Vec<i32> = page
        .iter()
        .map(|s| section_height(cx, s, theme, width, air))
        .collect();
    // `leaves` is packed against `area.h` less the strip the pager takes, and
    // against the whole of `area.h` where one page holds every section.
    let strip = pager::height(theme);
    let leaves = match paged(&tall, area.h - strip).len() > 1 {
        true => paged(&tall, area.h - strip),
        false => paged(&tall, area.h),
    };
    let pages = leaves.len().max(1);
    let at = at.min(pages - 1);
    let mut rest = match pages > 1 {
        true => area.split_bottom(strip).1,
        false => area,
    };
    if pages > 1 {
        let label = format!("{} {} {}", at + 1, cx.s().of, pages);
        pager::draw(
            cx,
            pager::foot(theme, area),
            &label,
            [at > 0, at + 1 < pages],
            [Hit::ConfigPage(0), Hit::ConfigPage(pages - 1)],
        );
    }
    let (from, upto) = leaves.get(at).copied().unwrap_or((0, page.len()));

    for section in page.into_iter().take(upto).skip(from) {
        // `placed` is laid out once, sizing the row and placing the chips alike.
        let mut borrowed: Vec<Vec<(&str, Script)>> =
            section.lines.iter().map(Line::options).collect();
        let mut placed: Vec<Vec<Rect>> = borrowed
            .iter()
            .zip(section.lines.iter())
            .map(|(options, line)| {
                chrome::chip_layout(cx.text, theme, options, line.apart(), width)
            })
            .collect();
        // A `one_row` run keeps what fits on its own line.
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
        // `heights` adds air: a wrapped row clears the next one's chips.
        let heights: Vec<i32> = blocks
            .iter()
            .map(|block| (block + theme.gap).max(theme.row_h))
            .collect();

        let need = chrome::section_height(cx.text, theme) + heights.iter().sum::<i32>() + air;
        let (band, left) = rest.split_top(need.min(rest.h));
        rest = left;

        let mut inner = chrome::section_stating(
            cx.fb,
            cx.text,
            theme,
            band,
            section.heading,
            section.said.as_deref(),
        );
        for (at, stated) in section.lines.iter().enumerate() {
            let (line, below) = inner.split_top(heights[at].min(inner.h));
            inner = below;
            chrome::setting(cx.fb, cx.text, theme, line, stated.label());
            let box_ = chip_box(line, column, blocks[at]);
            match stated {
                Line::Says { value, .. } => {
                    // `Line::Says` stands on the chips' column and the label's line.
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
    use crate::ui::theme::tests::PANELS;

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
        // `tall`, taller than its row, opens at the top of it.
        let tall = chip_box(row, 300, theme.row_h * 2);
        assert_eq!(tall.y, row.y);
    }

    #[test]
    fn the_sections_stand_a_half_row_apart() {
        for (w, h) in PANELS {
            let theme = Theme::for_screen(w, h);
            let air = between(&theme);
            assert!(air >= theme.row_h / 2, "{w}x{h}: {air} px between sections");
            assert!(air < theme.row_h, "{w}x{h}: {air} px reads as a blank row");
        }
    }

    #[test]
    fn the_page_holds_more_than_one_setting() {
        let settings = Settings::new(Lang::English);
        let empty = Summary::default();
        let page = sections(Lang::English, &settings, true, &empty);
        assert!(page.len() >= 2, "a page of one section is a stub");
        let lines: usize = page.iter().map(|s| s.lines.len()).sum();
        assert!(lines >= 2, "got {lines} lines");
        for section in &page {
            assert!(!section.heading.is_empty());
            assert!(!section.lines.is_empty(), "a heading with nothing under it");
            for line in &section.lines {
                assert!(!line.label().is_empty(), "a line with no label");
            }
        }
    }

    #[test]
    fn every_language_is_offered_and_the_set_one_is_lit() {
        // `Lang::ALL` holds no Automatic chip: the language set is lit.
        let mut settings = Settings::new(Lang::Japanese);
        let empty = Summary::default();
        let page = sections(Lang::English, &settings, true, &empty);
        let language = row(&page, 0, 0);
        assert_eq!(
            language.options.len(),
            Lang::ALL.len(),
            "one chip per language"
        );
        assert_eq!(language.on, 4, "the Kindle's Japanese is what is lit");

        settings.language = Lang::TraditionalChinese;
        let empty = Summary::default();
        let page = sections(Lang::English, &settings, true, &empty);
        assert_eq!(row(&page, 0, 0).on, 3);
    }

    #[test]
    fn each_language_names_itself_in_its_own_script() {
        // `日本語` takes the Japanese face, never a Simplified one.
        let settings = Settings::new(Lang::English);
        let empty = Summary::default();
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
        let empty = Summary::default();
        let page = sections(Lang::English, &settings, true, &empty);
        let size = row(&page, 0, 1);
        assert_eq!(size.options.len(), TextSize::ALL.len());
        assert_eq!(size.on, 2);
        assert_eq!((size.hit)(0), Hit::TextSize(TextSize::Small));
    }

    #[test]
    fn the_week_row_names_its_days_in_the_interface_s_language() {
        let settings = Settings::new(Lang::German);
        let empty = Summary::default();
        let page = sections(Lang::German, &settings, true, &empty);
        let week = row(&page, 1, 0);
        assert_eq!(week.options[0].0, "Mo");
        assert_eq!(week.options[1].0, "So");
    }

    #[test]
    fn the_page_states_which_build_it_is_and_offers_a_newer_one() {
        let settings = Settings::new(Lang::English);
        let empty = Summary::default();
        let page = sections(Lang::English, &settings, true, &empty);
        let about = page.last().expect("a section");

        let Line::Says { label, value } = &about.lines[0] else {
            panic!("the version is stated, not set");
        };
        assert_eq!(*label, Lang::English.strings().version_row);
        assert_eq!(value, crate::update::VERSION);

        // `update`: one chip, never lit.
        let update = row(&page, page.len() - 1, 1);
        assert_eq!(update.options.len(), 1);
        assert_eq!((update.hit)(0), Hit::Update);
        assert!(update.on >= update.options.len(), "a button drawn filled");
    }

    /// The record section of a page built over `record`, in `lang`.
    fn record_section(lang: Lang, summary: &Summary) -> Section<'_> {
        let settings = Settings::new(lang);
        let page = sections(lang, &settings, true, summary);
        page.into_iter()
            .find(|s| s.heading == lang.strings().the_record)
            .expect("the record section")
    }

    /// Every row of that section, by its label.
    fn the_summary(summary: &Summary) -> Vec<String> {
        record_section(Lang::English, summary)
            .lines
            .iter()
            .map(|l| l.label().to_string())
            .collect()
    }

    /// The record section's four settings, in order: the two that move the figure first.
    const SETTINGS: [fn(&crate::lang::Strings) -> &'static str; 4] = [
        |s| s.unidentified_row,
        |s| s.uncovered_row,
        |s| s.stats_from_row,
        |s| s.sitting_floor_row,
    ];

    /// [`SETTINGS`]' labels, then `rest`.
    fn expected(rest: &[&'static str]) -> Vec<&'static str> {
        let s = Lang::English.strings();
        SETTINGS
            .iter()
            .map(|f| f(s))
            .chain(rest.iter().copied())
            .collect()
    }

    #[test]
    fn a_backup_stating_no_days_lists_its_count_alone() {
        let s = Lang::English.strings();
        let backup = crate::backup::Backup {
            path: "/nowhere/readinglog-260910-200851.zip".into(),
            stamp: "260910-200851".into(),
            kind: crate::backup::Kind::Record,
            bytes: 2048,
        };
        let record = crate::record::Record::default();
        let bare = crate::backup::About {
            written: "260910:200851".into(),
            ..crate::backup::About::default()
        };
        assert_eq!(super::row(&backup, &bare, &record, s).holds, "0 sittings");
        let spanned = crate::backup::About {
            sittings: 572,
            first: "2026-08-01".into(),
            last: "2026-09-10".into(),
            ..bare
        };
        assert_eq!(
            super::row(&backup, &spanned, &record, s).holds,
            "572 sittings · 2026-08-01 to 2026-09-10"
        );
    }

    #[test]
    fn a_record_with_nothing_in_it_offers_no_reset() {
        assert_eq!(the_summary(&Summary::default()), expected(&[]));
        // `said` stays `None` on an empty record.
        assert_eq!(
            record_section(Lang::English, &Summary::default()).said,
            None
        );
    }

    #[test]
    fn a_record_with_reading_in_it_states_what_it_holds_and_offers_a_reset() {
        let summary = Summary {
            sittings: 12,
            books: 3,
            ..Summary::default()
        };
        let s = Lang::English.strings();
        // `reset_row` and `backups_row` stand under reading.
        assert_eq!(
            the_summary(&summary),
            expected(&[s.reset_row, s.backups_row])
        );
        // `said` holds the figure, on the heading.
        assert_eq!(
            record_section(Lang::English, &summary).said.as_deref(),
            Some("12 sittings · 3 books")
        );
    }

    /// `said` clears the heading's words and the right margin in every
    /// language, on the narrowest panel at the largest type.
    #[test]
    fn the_record_figure_shares_the_heading_line() {
        let Ok(mut text) = crate::ui::text::TextRenderer::load(24.0) else {
            return;
        };
        let summary = Summary {
            sittings: 2332,
            books: 19,
            backups: 2,
            backup_bytes: 1_258_291,
            was_reset: true,
        };
        let theme = Theme::sized(600, 800, TextSize::Large);
        let room = chrome::content_box(&theme).w;
        text.set_px(theme.small_px);
        for lang in Lang::ALL {
            let s = lang.strings();
            let said = record_section(lang, &summary).said.expect("a figure");
            let head = text.measure_width(s.the_record) as i32;
            let figure = text.measure_width(&said) as i32;
            assert!(
                head + theme.gap + figure < room,
                "{lang:?}: {:?} and {said:?} want {}px of {room}px",
                s.the_record,
                head + theme.gap + figure
            );
        }
    }

    #[test]
    fn the_backups_row_stands_for_a_backup_or_for_a_reset() {
        let s = Lang::English.strings();
        let kept = Summary {
            sittings: 12,
            books: 3,
            backups: 1,
            backup_bytes: 0,
            was_reset: false,
        };
        assert_eq!(the_summary(&kept), expected(&[s.reset_row, s.backups_row]));
        let was_reset = Summary {
            backups: 0,
            backup_bytes: 0,
            was_reset: true,
            ..kept
        };
        assert_eq!(
            the_summary(&was_reset),
            expected(&[s.reset_row, s.backups_row])
        );
    }

    /// `restore`'s run: `back_up`, then `restore` where a backup is on disk,
    /// then `rebuild` where the record was reset; `view::backups` lists the
    /// backups.
    #[test]
    fn the_backups_row_is_two_buttons_and_the_logs_apart_from_them() {
        let settings = Settings::new(Lang::English);
        let s = Lang::English.strings();
        let held = |backups: usize, was_reset: bool| Summary {
            sittings: 12,
            books: 3,
            backups,
            backup_bytes: 0,
            was_reset,
        };
        let backups = |summary: &Summary| {
            let page = sections(Lang::English, &settings, true, summary);
            let at = page.len() - 2;
            let last = page[at].lines.len() - 1;
            let row = row(&page, at, last);
            (
                row.options
                    .iter()
                    .map(|(o, _)| o.clone())
                    .collect::<Vec<_>>(),
                (0..row.options.len()).map(&row.hit).collect::<Vec<_>>(),
                row.apart,
                row.one_row,
            )
        };

        let (said, hits, apart, one_row) = backups(&held(2, true));
        assert_eq!(said, [s.back_up, s.restore, s.rebuild]);
        assert_eq!(hits, [Hit::BackUp, Hit::Backups, Hit::Rebuild]);
        assert_eq!(apart, Some(2), "the logs stand apart from the two buttons");
        assert!(!one_row, "the run is fixed and never truncated");

        // `held(0, true)`: no backup on disk.
        let (said, hits, apart, _) = backups(&held(0, true));
        assert_eq!(said, [s.back_up, s.rebuild]);
        assert_eq!(hits, [Hit::BackUp, Hit::Rebuild]);
        assert_eq!(apart, Some(1));

        // `held(2, false)`: never reset, no rebuild.
        let (said, hits, _, _) = backups(&held(2, false));
        assert_eq!(said, [s.back_up, s.restore]);
        assert_eq!(hits, [Hit::BackUp, Hit::Backups]);

        // `held(0, false)`: `back_up` alone.
        let (said, hits, _, _) = backups(&held(0, false));
        assert_eq!(said, [s.back_up]);
        assert_eq!(hits, [Hit::BackUp]);
    }

    /// The row `label` names, wherever it sits on the page.
    fn named_row<'a>(page: &'a [Section<'a>], label: &str) -> &'a Row<'a> {
        page.iter()
            .flat_map(|section| section.lines.iter())
            .find_map(|line| match line {
                Line::Set(row) if row.label == label => Some(row),
                _ => None,
            })
            .unwrap_or_else(|| panic!("no row called {label}"))
    }

    /// The unidentified books row of a page drawn in `lang`, whatever else the
    /// record section offers under it.
    fn unidentified_row<'a>(page: &'a [Section<'a>], lang: Lang) -> &'a Row<'a> {
        named_row(page, lang.strings().unidentified_row)
    }

    fn stats_from_row<'a>(page: &'a [Section<'a>], lang: Lang) -> &'a Row<'a> {
        named_row(page, lang.strings().stats_from_row)
    }

    #[test]
    fn every_long_pass_asks_before_it_runs() {
        let s = Lang::English.strings();
        let blank = Confirm {
            about: Act::Rebuild,
            sittings: 0,
            books: 0,
            bytes: 0,
            named: String::new(),
        };
        for about in [
            Act::Rebuild,
            Act::Reset(true),
            Act::Reset(false),
            Act::Restore(0),
        ] {
            let asked = Confirm {
                about,
                ..blank.clone()
            };
            let (heading, note, answer) = question(&asked, s);
            assert!(!heading.is_empty(), "{about:?} asks nothing");
            assert!(!note.is_empty(), "{about:?} states nothing");
            assert!(!answer.is_empty(), "{about:?} offers no answer");
        }
        // `Act::Rebuild`'s note names the minutes.
        let (_, note, _) = question(&blank, s);
        assert!(
            note.contains("minute"),
            "the rebuild names no time: {note:?}"
        );
    }

    #[test]
    fn the_stats_from_row_holds_its_two_values_alone() {
        let empty = Summary::default();
        let page = sections(Lang::English, &Settings::new(Lang::English), true, &empty);
        let stats_row = stats_from_row(&page, Lang::English);
        assert_eq!(stats_row.options.len(), 2);
        assert_eq!((stats_row.hit)(0), Hit::StatsFrom(StatsFrom::Kindle));
        assert_eq!((stats_row.hit)(1), Hit::StatsFrom(StatsFrom::App));
        assert_eq!(stats_row.apart, None);
    }

    #[test]
    fn the_unidentified_row_holds_its_two_values_alone() {
        let empty = Summary::default();
        let page = sections(Lang::English, &Settings::new(Lang::English), true, &empty);
        let unidentified = unidentified_row(&page, Lang::English);
        assert_eq!(unidentified.options.len(), 2);
        assert_eq!((unidentified.hit)(0), Hit::ShowUnidentified(true));
        assert_eq!((unidentified.hit)(1), Hit::ShowUnidentified(false));
        assert_eq!(unidentified.apart, None);
    }

    #[test]
    fn the_sitting_row_offers_every_floor_and_lights_the_set_one() {
        let mut settings = Settings::new(Lang::English);
        let empty = Summary::default();
        for floor in SittingFloor::ALL {
            settings.sitting_floor = floor;
            let page = sections(Lang::English, &settings, true, &empty);
            let row = named_row(&page, Lang::English.strings().sitting_floor_row);
            assert_eq!(row.options.len(), SittingFloor::ALL.len());
            assert_eq!(row.apart, None, "no button stands in this run");
            assert_eq!(
                (row.hit)(row.on),
                Hit::SittingFloor(floor),
                "the lit chip is `floor`"
            );
            for (at, floor) in SittingFloor::ALL.iter().enumerate() {
                assert_eq!((row.hit)(at), Hit::SittingFloor(*floor));
            }
        }
    }

    #[test]
    fn every_language_states_the_floors_as_its_own_minutes() {
        for lang in Lang::ALL {
            let s = lang.strings();
            assert!(!s.sitting_floor_row.is_empty(), "{lang:?}");
            // `sitting_floor_row` repeats no other row's label.
            assert_ne!(s.sitting_floor_row, s.stats_from_row, "{lang:?}");
            assert_ne!(s.sitting_floor_row, s.unidentified_row, "{lang:?}");
            assert_ne!(s.sitting_floor_row, s.uncovered_row, "{lang:?}");

            let settings = Settings::new(lang);
            let empty = Summary::default();
            let page = sections(lang, &settings, true, &empty);
            let row = named_row(&page, s.sitting_floor_row);
            let chips: Vec<&str> = row.options.iter().map(|(t, _)| t.as_str()).collect();
            for (chip, floor) in chips.iter().zip(SittingFloor::ALL) {
                assert_eq!(*chip, floor.label(s), "{lang:?}");
            }
            // Every `label` reads differently.
            let mut sorted = chips.clone();
            sorted.sort_unstable();
            sorted.dedup();
            assert_eq!(sorted.len(), chips.len(), "{lang:?} repeats a chip");
        }
    }

    #[test]
    fn the_label_column_is_not_widened_by_the_sitting_row() {
        // `chip_column` takes the widest label on the page. 600x800 at
        // `TextSize::Large` is the narrowest column.
        let Ok(mut text) = crate::ui::text::TextRenderer::load(24.0) else {
            return;
        };
        for lang in Lang::ALL {
            let s = lang.strings();
            let theme = Theme::sized(600, 800, TextSize::Large);
            text.set_px(theme.body_px);
            let settings = Settings::new(lang);
            let empty = Summary::default();
            let page = sections(lang, &settings, true, &empty);
            let widest = page
                .iter()
                .flat_map(|section| section.lines.iter())
                .map(|line| line.label())
                .filter(|label| *label != s.sitting_floor_row)
                .map(|label| text.measure_width(label))
                .max()
                .unwrap_or(0);
            let mine = text.measure_width(s.sitting_floor_row);
            assert!(
                mine <= widest,
                "{lang:?}: {:?} sets {mine}px against the page's {widest}px",
                s.sitting_floor_row
            );
        }
    }

    #[test]
    fn the_record_row_states_the_file_and_not_what_the_page_counts() {
        // `Summary::sittings` counts `Record::sittings`, never `Stats::sittings`.
        let record = crate::record::Record {
            sittings: vec![crate::log::sitting::Sitting {
                started_at: "2026-08-05T09:00:00".into(),
                ended_at: "2026-08-05T09:00:30".into(),
                end_position: 100,
                seconds: 30,
                ..crate::log::sitting::Sitting::default()
            }],
            ..crate::record::Record::default()
        };
        let stats = crate::stats::Stats::build(
            &record,
            crate::date::days_from_civil(2026, 8, 6),
            true,
            StatsFrom::Kindle,
            SittingFloor::OneMinute,
        );
        assert!(stats.sittings.is_empty(), "the skim is not counted");
        let summary = Summary::of(&record, &stats, std::path::Path::new("/nowhere"), false);
        assert_eq!(summary.sittings, 1, "but the file still holds it");
        // `reset_row` stands where `Record::sittings` is above zero.
        let labels = the_summary(&summary);
        assert!(labels.contains(&Lang::English.strings().reset_row.to_string()));
    }

    #[test]
    fn the_covers_row_offers_the_two_values_and_no_button() {
        let mut settings = Settings::new(Lang::English);
        let empty = Summary::default();
        for show in [true, false] {
            settings.show_uncovered = show;
            let page = sections(Lang::English, &settings, true, &empty);
            let uncovered = named_row(&page, Lang::English.strings().uncovered_row);
            assert_eq!(uncovered.options.len(), 2, "a button crept in");
            assert_eq!((uncovered.hit)(0), Hit::ShowUncovered(true));
            assert_eq!((uncovered.hit)(1), Hit::ShowUncovered(false));
            assert_eq!(uncovered.on, !show as usize);
            assert_eq!(uncovered.apart, None);
        }
    }

    #[test]
    fn every_language_names_the_two_rows_apart() {
        for lang in Lang::ALL {
            let s = lang.strings();
            assert!(!s.uncovered_row.is_empty(), "{lang:?}");
            assert_ne!(s.uncovered_row, s.unidentified_row, "{lang:?}");
            assert!(!s.no_cover.is_empty(), "{lang:?}");
        }
    }

    #[test]
    fn every_page_of_settings_holds_what_it_can_and_no_more() {
        // Three sections into a page holding two of them.
        assert_eq!(paged(&[100, 100, 100], 250), [(0, 2), (2, 3)]);
        // `[100, 100]` on one page.
        assert_eq!(paged(&[100, 100], 250), [(0, 2)]);
        // `400`, taller than the page, gets one to itself.
        assert_eq!(paged(&[100, 400, 100], 250), [(0, 1), (1, 2), (2, 3)]);
        assert_eq!(paged(&[], 250), Vec::new());
    }

    #[test]
    fn a_size_reads_as_a_figure_a_reader_can_weigh() {
        assert_eq!(bytes(0), "1 KB");
        assert_eq!(bytes(2_048), "2 KB");
        assert_eq!(bytes(5_000_000), "4.8 MB");
    }

    /// `when` names the day and clock a backup was written at, else the stamp
    /// in its name, else the name as it spells.
    #[test]
    fn a_row_names_the_day_and_the_clock_a_backup_was_written_at() {
        let s = Lang::English.strings();
        let one = crate::backup::Backup {
            path: std::path::PathBuf::from("a.zip"),
            stamp: "260906-010231".into(),
            kind: crate::backup::Kind::Record,
            bytes: 0,
        };
        let said = crate::backup::About {
            written: "260906:184500".into(),
            ..crate::backup::About::default()
        };
        assert_eq!(when(&one, &said, s), "Sep 6 18:45");
        assert_eq!(
            when(&one, &crate::backup::About::default(), s),
            "Sep 6 01:02",
            "the name's own stamp, where the backup states none"
        );
        let odd = crate::backup::Backup {
            stamp: "nonsense".into(),
            ..one
        };
        assert_eq!(when(&odd, &crate::backup::About::default(), s), "nonsense");
    }

    #[test]
    fn every_language_names_the_about_section_and_its_button() {
        for lang in Lang::ALL {
            let settings = Settings::new(lang);
            let empty = Summary::default();
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
        let empty = Summary::default();
        let page = sections(Lang::English, &settings, true, &empty);
        let language = row(&page, 0, 0);
        for (i, lang) in Lang::ALL.iter().enumerate() {
            assert_eq!((language.hit)(i), Hit::Language(*lang));
        }
        // `hit(99)`, past the end, answers without a panic.
        assert_eq!((language.hit)(99), Hit::Language(Lang::Japanese));
    }
}
