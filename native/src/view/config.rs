//! The settings, as sections of rows: a heading with a rule under it, then one
//! row per setting with every value beside it, each its own tap target.
//! A change applies on the tap and is written as it is made.

use crate::font::Script;
use crate::lang::Lang;
use crate::settings::{Settings, TextSize, WeekStart};
use crate::ui::chrome;
use crate::ui::paint::Rect;
use crate::ui::theme::Theme;
use crate::update;

use super::{Ctx, Hit};

/// The index no option is drawn filled at. A row of one chip that is a button
/// rather than a setting takes it: nothing it does is a state this page could
/// be showing.
const NONE_ON: usize = usize::MAX;

/// One setting: what it is called, and the values it takes.
struct Row<'a> {
    label: &'a str,
    options: Vec<(String, Script)>,
    on: usize,
    /// What a tap on option `i` does.
    hit: fn(usize) -> Hit,
}

/// One line of a section.
enum Line<'a> {
    /// A setting: its name, its values, and which is in use.
    Set(Row<'a>),
    /// A fact the page states and does not set. Its value stands at the same
    /// column the chips do, so a stated line and a set one read as one list.
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

/// The page, built from what is set. Kept apart from the drawing: the shape
/// of the page is asserted without a framebuffer.
fn sections<'a>(lang: Lang, settings: &Settings) -> Vec<Section<'a>> {
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
        hit: |i| Hit::Language(Lang::ALL[i.min(Lang::ALL.len() - 1)]),
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
        hit: |i| Hit::WeekStart(WeekStart::ALL[i.min(WeekStart::ALL.len() - 1)]),
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
        hit: |i| Hit::TextSize(TextSize::ALL[i.min(TextSize::ALL.len() - 1)]),
    };

    let unnamed = Row {
        label: s.unnamed_row,
        options: vec![
            (s.unnamed_show.to_string(), plain),
            (s.unnamed_hide.to_string(), plain),
        ],
        on: !settings.show_unnamed as usize,
        hit: |i| Hit::ShowUnnamed(i == 0),
    };

    // Never filled: it is a button rather than a setting.
    let update = Row {
        label: s.update_row,
        options: vec![(s.update_check.to_string(), plain)],
        on: NONE_ON,
        hit: |_| Hit::Update,
    };

    vec![
        Section {
            heading: s.interface,
            lines: vec![Line::Set(language), Line::Set(size)],
        },
        Section {
            heading: s.the_calendar,
            lines: vec![Line::Set(week)],
        },
        Section {
            heading: s.the_record,
            lines: vec![Line::Set(unnamed)],
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

pub fn draw(cx: &mut Ctx, area: Rect, settings: &Settings) {
    let theme: &Theme = cx.theme;
    let air = between(theme);
    let page = sections(cx.lang, settings);

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
    let mut rest = area;

    for section in page {
        // Laid out once. The same answer sizes the row and places the chips:
        // a wrapped option never falls outside the height it was given.
        let borrowed: Vec<Vec<(&str, Script)>> = section.lines.iter().map(Line::options).collect();
        let placed: Vec<Vec<Rect>> = borrowed
            .iter()
            .map(|options| chrome::chip_layout(cx.text, theme, options, width))
            .collect();
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

    /// The setting at line `at` of section `of`. Panics where that line is a
    /// fact rather than a setting, which is what the caller meant to read.
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
        let page = sections(Lang::English, &settings);
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
        let page = sections(Lang::English, &settings);
        let language = row(&page, 0, 0);
        assert_eq!(
            language.options.len(),
            Lang::ALL.len(),
            "one chip per language"
        );
        assert_eq!(language.on, 4, "the device's Japanese is what is lit");

        settings.language = Lang::TraditionalChinese;
        let page = sections(Lang::English, &settings);
        assert_eq!(row(&page, 0, 0).on, 3);
    }

    #[test]
    fn each_language_names_itself_in_its_own_script() {
        // 日本語 drawn from a Simplified face is the defect this prevents.
        let settings = Settings::new(Lang::English);
        let page = sections(Lang::English, &settings);
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
        let page = sections(Lang::English, &settings);
        let size = row(&page, 0, 1);
        assert_eq!(size.options.len(), TextSize::ALL.len());
        assert_eq!(size.on, 2);
        assert_eq!((size.hit)(0), Hit::TextSize(TextSize::Small));
    }

    #[test]
    fn the_week_row_names_its_days_in_the_interface_s_language() {
        let settings = Settings::new(Lang::German);
        let page = sections(Lang::German, &settings);
        let week = row(&page, 1, 0);
        assert_eq!(week.options[0].0, "Mo");
        assert_eq!(week.options[1].0, "So");
    }

    #[test]
    fn the_page_states_which_build_it_is_and_offers_a_newer_one() {
        let settings = Settings::new(Lang::English);
        let page = sections(Lang::English, &settings);
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

    #[test]
    fn every_language_names_the_about_section_and_its_button() {
        for lang in Lang::ALL {
            let settings = Settings::new(lang);
            let page = sections(lang, &settings);
            let about = page.last().expect("a section");
            assert_eq!(about.heading, lang.strings().about, "{lang:?}");
            let update = row(&page, page.len() - 1, 1);
            assert!(!update.options[0].0.is_empty(), "{lang:?}");
        }
    }

    #[test]
    fn a_tap_names_the_option_under_it() {
        let settings = Settings::new(Lang::English);
        let page = sections(Lang::English, &settings);
        let language = row(&page, 0, 0);
        for (i, lang) in Lang::ALL.iter().enumerate() {
            assert_eq!((language.hit)(i), Hit::Language(*lang));
        }
        // A chip index past the end cannot panic: the row is drawn from the
        // same list, but the two are separated by the paint.
        assert_eq!((language.hit)(99), Hit::Language(Lang::Japanese));
    }
}
