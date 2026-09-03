//! The settings, as sections of rows: a heading with a rule under it, then one
//! row per setting with every value beside it.
//!
//! Every value is shown at once and each is its own tap target. A control that
//! cycles hides how many values it has, and there is room here for all of
//! them. Changes apply on the tap and are written as they are made.

use crate::font::Script;
use crate::lang::Lang;
use crate::settings::{Settings, TextSize, WeekStart};
use crate::ui::chrome;
use crate::ui::paint::Rect;
use crate::ui::theme::Theme;

use super::{Ctx, Hit};

/// One setting: what it is called, and the values it takes.
struct Row<'a> {
    label: &'a str,
    options: Vec<(String, Script)>,
    on: usize,
    /// What a tap on option `i` does.
    hit: fn(usize) -> Hit,
}

/// A section of the page.
struct Section<'a> {
    heading: &'a str,
    rows: Vec<Row<'a>>,
}

/// The page, built from what is set. Kept apart from the drawing so the shape
/// of the page can be asserted without a framebuffer.
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

    vec![
        Section {
            heading: s.interface,
            rows: vec![language, size],
        },
        Section {
            heading: s.the_calendar,
            rows: vec![week],
        },
    ]
}

pub fn draw(cx: &mut Ctx, area: Rect, settings: &Settings) {
    let theme: &Theme = cx.theme;
    let air = theme.gap * 2;
    let page = sections(cx.lang, settings);

    // Every row's chips start at one column, taken from the widest label and
    // pulled back until the widest run fits — so no row wraps that need not.
    let labels: Vec<&str> = page
        .iter()
        .flat_map(|s| s.rows.iter().map(|r| r.label))
        .collect();
    let runs: Vec<Vec<(&str, Script)>> = page
        .iter()
        .flat_map(|s| s.rows.iter())
        .map(|row| {
            row.options
                .iter()
                .map(|(t, script)| (t.as_str(), *script))
                .collect()
        })
        .collect();
    let column = chrome::chip_column(cx.text, theme, &labels, &runs, area.w);
    let width = (area.w - column).max(1);
    let mut rest = area;

    for section in page {
        // Laid out once. The same answer sizes the row and places the chips,
        // so a wrapped option can never fall outside the height it was given.
        let borrowed: Vec<Vec<(&str, Script)>> = section
            .rows
            .iter()
            .map(|row| {
                row.options
                    .iter()
                    .map(|(text, script)| (text.as_str(), *script))
                    .collect()
            })
            .collect();
        let placed: Vec<Vec<Rect>> = borrowed
            .iter()
            .map(|options| chrome::chip_layout(cx.text, theme, options, width))
            .collect();
        let heights: Vec<i32> = placed
            .iter()
            .map(|row| {
                // Plus air, so a row that wrapped does not sit against the
                // next one's chips.
                (row.iter().map(|c| c.bottom()).max().unwrap_or(0) + theme.gap).max(theme.row_h)
            })
            .collect();

        let need = chrome::section_height(cx.text, theme) + heights.iter().sum::<i32>() + air;
        let (band, left) = rest.split_top(need.min(rest.h));
        rest = left;

        let mut inner = chrome::section(cx.fb, cx.text, theme, band, section.heading);
        for (((row, options), at), height) in section
            .rows
            .iter()
            .zip(&borrowed)
            .zip(&placed)
            .zip(&heights)
        {
            let (line, below) = inner.split_top((*height).min(inner.h));
            inner = below;
            chrome::setting(cx.fb, cx.text, theme, line, row.label);
            let box_ = chip_box(line, column);
            let chips = chrome::chips(cx.fb, cx.text, theme, box_, options, at, row.on);
            for (i, chip) in chips.into_iter().enumerate() {
                cx.hit((row.hit)(i), chip);
            }
        }
    }
}

/// Where a row's chips sit: right of the shared label column.
fn chip_box(row: Rect, column: i32) -> Rect {
    let left = row.x + column;
    Rect::new(left, row.y, (row.right() - left).max(1), row.h)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_page_holds_more_than_one_setting() {
        let settings = Settings::new(Lang::English);
        let page = sections(Lang::English, &settings);
        assert!(page.len() >= 2, "a page of one section is a stub");
        let rows: usize = page.iter().map(|s| s.rows.len()).sum();
        assert!(rows >= 2, "got {rows} rows");
        for section in &page {
            assert!(!section.heading.is_empty());
            assert!(!section.rows.is_empty(), "a heading with nothing under it");
        }
    }

    #[test]
    fn every_language_is_offered_and_the_set_one_is_lit() {
        // No Automatic chip: the device's language is the default, and the
        // default is simply the one that starts out lit.
        let mut settings = Settings::new(Lang::Japanese);
        let page = sections(Lang::English, &settings);
        let row = &page[0].rows[0];
        assert_eq!(row.options.len(), Lang::ALL.len(), "one chip per language");
        assert_eq!(row.on, 4, "the device's Japanese is what is lit");

        settings.language = Lang::TraditionalChinese;
        let page = sections(Lang::English, &settings);
        assert_eq!(page[0].rows[0].on, 3);
    }

    #[test]
    fn each_language_names_itself_in_its_own_script() {
        // 日本語 drawn from a Simplified face is the defect this prevents.
        let settings = Settings::new(Lang::English);
        let page = sections(Lang::English, &settings);
        let by_name = |want: &str| {
            page[0].rows[0]
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
        let size = &page[0].rows[1];
        assert_eq!(size.options.len(), TextSize::ALL.len());
        assert_eq!(size.on, 2);
        assert_eq!((size.hit)(0), Hit::TextSize(TextSize::Small));
    }

    #[test]
    fn the_week_row_names_its_days_in_the_interface_s_language() {
        let settings = Settings::new(Lang::German);
        let page = sections(Lang::German, &settings);
        let week = &page[1].rows[0];
        assert_eq!(week.options[0].0, "Mo");
        assert_eq!(week.options[1].0, "So");
    }

    #[test]
    fn a_tap_names_the_option_under_it() {
        let settings = Settings::new(Lang::English);
        let page = sections(Lang::English, &settings);
        let row = &page[0].rows[0];
        for (i, lang) in Lang::ALL.iter().enumerate() {
            assert_eq!((row.hit)(i), Hit::Language(*lang));
        }
        // A chip index past the end cannot panic: the row is drawn from the
        // same list, but the two are separated by the paint.
        assert_eq!((row.hit)(99), Hit::Language(Lang::Japanese));
    }
}
