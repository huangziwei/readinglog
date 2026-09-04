//! The frame every screen sits in: one strip along the bottom holding Exit and
//! the four tabs. There is no title bar — the tab drawn in reverse names the
//! screen, and every screen states its own figures in its body.

use crate::eink::fb::Framebuffer;

use crate::lang::Lang;

use super::paint::{self, INK, LIGHT, PALE, Rect, WHITE};
use super::text::TextRenderer;
use super::theme::Theme;

/// The screens, in the order their tabs sit in.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Tab {
    /// The settings, first because it sits beside Exit and neither is a
    /// figure about reading.
    Config,
    Home,
    Rhythm,
    Books,
}

impl Tab {
    pub const ALL: [Tab; 4] = [Tab::Config, Tab::Home, Tab::Rhythm, Tab::Books];

    /// What this tab is called, in the interface's own language.
    pub fn label(self, lang: Lang) -> &'static str {
        let s = lang.strings();
        match self {
            Tab::Config => s.config,
            Tab::Home => s.today,
            Tab::Rhythm => s.rhythm,
            Tab::Books => s.books,
        }
    }
}

/// Clear the screen to paper.
pub fn clear(fb: &mut Framebuffer, theme: &Theme) {
    paint::fill(fb, theme.screen, WHITE);
}

// Exit is set, not drawn: no face on this firmware carries a power symbol,
// and the only close marks that exist sit in `code2000` and the display faces,
// where they would stand against Ember's letters in another face's weight.

/// The bottom strip: Exit, then the four tabs, in five cells of one width,
/// answering a hit box each. The tab showing is drawn in reverse; a book is
/// shown over the tab it was opened from, so tapping that tab closes it.
pub fn tabs(
    fb: &mut Framebuffer,
    text: &mut TextRenderer,
    theme: &Theme,
    lang: Lang,
    active: Tab,
) -> (Rect, Vec<(Tab, Rect)>) {
    let (strip, _) = theme.screen.split_bottom(theme.tabs_h);
    paint::fill(fb, strip, WHITE);
    paint::hline(fb, 0, strip.y, strip.w, LIGHT, 2);

    // Exit takes the first of five equal cells; the tabs take the rest.
    let mut cells = strip.columns(Tab::ALL.len() as i32 + 1, 0).into_iter();
    let exit = cells.next().unwrap_or(strip);

    text.set_px(theme.tab_px);
    let baseline = strip.center_y() + text.cap_height() as i32 / 2;
    let script = crate::font::Script::of_language(lang.language_tag());
    let label = lang.strings().exit;
    let w = text.measure_width_in(script, label) as i32;
    text.draw_in(
        script,
        fb,
        exit.x + (exit.w - w) / 2,
        baseline,
        label,
        false,
    );
    paint::vline(fb, exit.right(), strip.y, strip.h, LIGHT, 2);

    let mut out = Vec::new();
    for (tab, cell) in Tab::ALL.iter().zip(cells) {
        let on = *tab == active;
        if on {
            paint::fill(fb, cell.inset(theme.gap / 2), INK);
        }
        let label = tab.label(lang);
        let w = text.measure_width_in(script, label) as i32;
        text.draw_in(script, fb, cell.x + (cell.w - w) / 2, baseline, label, on);
        out.push((*tab, cell));
    }
    (exit, out)
}

/// The content box: the screen above the strip.
///
/// Air on all four sides, `theme.gap * 2` top and bottom.
pub fn content(theme: &Theme, area: Rect) -> Rect {
    let (_, rest) = area.split_bottom(theme.tabs_h);
    let air = theme.gap * 2;
    Rect::new(
        theme.pad,
        rest.y + air,
        theme.screen.w - theme.pad * 2,
        (rest.h - air * 2).max(1),
    )
}

/// [`content`] from the theme alone.
pub fn content_box(theme: &Theme) -> Rect {
    content(theme, theme.screen)
}

/// A section heading with a rule under it, and the box left below.
pub fn section(
    fb: &mut Framebuffer,
    text: &mut TextRenderer,
    theme: &Theme,
    area: Rect,
    title: &str,
) -> Rect {
    text.set_px(theme.small_px);
    let h = text.line_height() as i32 + theme.gap;
    text.draw(fb, area.x, area.y + text.cap_height() as i32, title, false);
    paint::hline(fb, area.x, area.y + h - theme.gap / 2, area.w, PALE, 1);
    let (_, rest) = area.split_top(h + theme.gap / 2);
    rest
}

/// The row a figure or a chip stands on at the right of a section heading,
/// centred on the title's own ink. Everything on the row takes this one centre,
/// so a baseline derived from it lands on the title's own.
pub fn heading_row(text: &mut TextRenderer, theme: &Theme, head: Rect) -> Rect {
    text.set_px(theme.small_px);
    let cap = text.cap_height() as i32;
    let h = (text.line_height() as i32 + theme.gap / 2).min(head.h.max(1));
    Rect::new(head.x, head.y + cap / 2 - h / 2, head.w, h)
}

pub fn section_height(text: &mut TextRenderer, theme: &Theme) -> i32 {
    text.set_px(theme.small_px);
    text.line_height() as i32 + theme.gap + theme.gap / 2
}

/// The height [`figure`] draws into, set no larger than `ceiling`.
pub fn figure_height_at(text: &mut TextRenderer, theme: &Theme, ceiling: f32) -> i32 {
    text.set_px(ceiling);
    let value = text.cap_height() as i32;
    text.set_px(theme.small_px);
    value + theme.gap + text.line_height() as i32
}

/// The height [`figure`] draws into at [`Theme::display_px`].
pub fn figure_height(text: &mut TextRenderer, theme: &Theme) -> i32 {
    figure_height_at(text, theme, theme.display_px)
}

/// The width a figure set at `px` needs: the wider of the number and the name
/// under it.
fn figure_width(text: &mut TextRenderer, theme: &Theme, value: &str, label: &str, px: f32) -> i32 {
    text.set_px(px);
    let value = text.measure_width(value) as i32;
    text.set_px(theme.small_px);
    value.max(text.measure_width(label) as i32)
}

/// A figure at `px` with its name under it.
fn figure(
    fb: &mut Framebuffer,
    text: &mut TextRenderer,
    theme: &Theme,
    area: Rect,
    value: &str,
    label: &str,
    px: f32,
) {
    text.set_px(px);
    let w = text.measure_width(value) as i32;
    let top = area.y + text.cap_height() as i32;
    text.draw(fb, area.x + (area.w - w) / 2, top, value, false);

    text.set_px(theme.small_px);
    let lw = text.measure_width(label) as i32;
    text.draw(
        fb,
        area.x + (area.w - lw) / 2,
        top + theme.gap + text.line_height() as i32,
        label,
        false,
    );
}

/// `stated` spread across `row`, the first flush left and the last flush right,
/// at [`Theme::display_px`] or the largest size down to [`Theme::body_px`] that
/// fits. [`figures_at`] caps it lower for a row standing among other bands.
pub fn figures(
    fb: &mut Framebuffer,
    text: &mut TextRenderer,
    theme: &Theme,
    row: Rect,
    stated: &[(String, &str)],
) {
    figures_at(fb, text, theme, row, stated, theme.display_px);
}

/// [`figures`] set no larger than `ceiling`, for a row that is one band of a
/// page rather than its head.
pub fn figures_at(
    fb: &mut Framebuffer,
    text: &mut TextRenderer,
    theme: &Theme,
    row: Rect,
    stated: &[(String, &str)],
    ceiling: f32,
) {
    // Air enough that two figures read as two and not as one long number.
    // The size gives way to it, never the air.
    let air = theme.gap * 3;
    let between = air * (stated.len() as i32 - 1).max(0);
    let measure = |text: &mut TextRenderer, px: f32| -> Vec<i32> {
        stated
            .iter()
            .map(|(value, label)| figure_width(text, theme, value, label, px))
            .collect()
    };
    let px = figures_px(ceiling, theme.body_px, row.w, |px| {
        measure(text, px).iter().sum::<i32>() + between
    });
    let widths = measure(text, px);
    for (cell, (value, label)) in row.spread(&widths, air).into_iter().zip(stated) {
        figure(fb, text, theme, cell, value, label, px);
    }
}

/// The size a row of figures is set at: `display` where the set fits `room`,
/// else the largest size down to `floor` that does. `needed` states the width
/// the set takes at a size.
fn figures_px(display: f32, floor: f32, room: i32, mut needed: impl FnMut(f32) -> i32) -> f32 {
    let mut px = display;
    let mut takes = needed(px);
    while px > floor && takes > room {
        // A width is near enough proportional to `px` to land in one step; the
        // pixel taken off it settles the rounding.
        px = (px * room as f32 / takes.max(1) as f32)
            .min(px - 1.0)
            .max(floor);
        takes = needed(px);
    }
    px
}

/// One line of a key and its value, the value set hard against the right.
pub fn row(
    fb: &mut Framebuffer,
    text: &mut TextRenderer,
    theme: &Theme,
    area: Rect,
    key: &str,
    value: &str,
) {
    text.set_px(theme.body_px);
    let baseline = area.center_y() + text.cap_height() as i32 / 2;
    text.draw(fb, area.x, baseline, key, false);
    let w = text.measure_width(value) as i32;
    text.draw(fb, area.right() - w, baseline, value, false);
}

/// Blank space either side of a chip's text, and between one chip and the next.
const CHIP_PAD: i32 = 20;
const CHIP_GAP: i32 = 14;

/// The air a chip keeps either side of its label.
pub fn chip_pad() -> i32 {
    CHIP_PAD
}

/// How tall one chip is.
pub fn chip_height(theme: &Theme) -> i32 {
    theme.row_h * 2 / 3
}

/// Where the second column starts on every row: from the widest label, pulled
/// back until the widest chip run fits, held between `width / 3` and
/// `width / 2`. One column for the whole page, so the rows line up.
pub fn chip_column(
    text: &mut TextRenderer,
    theme: &Theme,
    labels: &[&str],
    runs: &[Vec<(&str, crate::font::Script)>],
    width: i32,
) -> i32 {
    text.set_px(theme.body_px);
    let widest = labels
        .iter()
        .map(|label| text.measure_width(label) as i32)
        .max()
        .unwrap_or(0);
    let runs: Vec<i32> = runs.iter().map(|run| run_width(text, theme, run)).collect();
    column_from(widest, &runs, width)
}

/// [`chip_column`]'s arithmetic, over widths already measured.
fn column_from(widest_label: i32, runs: &[i32], width: i32) -> i32 {
    let wanted = widest_label + CHIP_GAP * 3;
    let room = runs.iter().map(|run| width - run).min().unwrap_or(i32::MAX);
    wanted.min(room.max(width / 3).min(width / 2))
}

/// How wide a run of chips is once tiled, gaps included.
fn run_width(
    text: &mut TextRenderer,
    theme: &Theme,
    options: &[(&str, crate::font::Script)],
) -> i32 {
    text.set_px(theme.body_px);
    let chips: i32 = options
        .iter()
        .map(|(o, script)| text.measure_width_in(*script, o) as i32 + CHIP_PAD * 2)
        .sum();
    chips + CHIP_GAP * (options.len().saturating_sub(1)) as i32
}

/// Where every chip of a row lands, wrapped to `width`, laid out from `(0, 0)`.
/// Separated from the paint so a dropped chip — a setting the reader cannot
/// reach — is caught by a test.
pub fn chip_layout(
    text: &mut TextRenderer,
    theme: &Theme,
    options: &[(&str, crate::font::Script)],
    width: i32,
) -> Vec<Rect> {
    text.set_px(theme.body_px);
    let height = chip_height(theme);
    let (mut x, mut y) = (0, 0);
    let mut out = Vec::new();
    for (option, script) in options {
        let w = text.measure_width_in(*script, option) as i32 + CHIP_PAD * 2;
        if x > 0 && x + w > width {
            x = 0;
            y += height + CHIP_GAP;
        }
        out.push(Rect::new(x, y, w, height));
        x += w + CHIP_GAP;
    }
    out
}

/// The name of one setting, on the left of its row.
pub fn setting(
    fb: &mut Framebuffer,
    text: &mut TextRenderer,
    theme: &Theme,
    row: Rect,
    label: &str,
) {
    text.set_px(theme.body_px);
    let baseline = row.center_y() + text.cap_height() as i32 / 2;
    text.draw(fb, row.x, baseline, label, false);
}

/// Every option of a setting, side by side, the one in use filled and the rest
/// outlined, at the places [`chip_layout`] put them, answering one box each.
/// The caller must size the row from that same layout.
pub fn chips(
    fb: &mut Framebuffer,
    text: &mut TextRenderer,
    theme: &Theme,
    area: Rect,
    options: &[(&str, crate::font::Script)],
    placed: &[Rect],
    on: usize,
) -> Vec<Rect> {
    text.set_px(theme.body_px);
    let mut out = Vec::new();
    for (i, (at, (option, script))) in placed.iter().zip(options).enumerate() {
        let chip = Rect::new(area.x + at.x, area.y + at.y, at.w, at.h);
        let tw = text.measure_width_in(*script, option) as i32;
        let picked = i == on;
        match picked {
            true => paint::fill(fb, chip, INK),
            false => paint::stroke(fb, chip, INK, 2),
        }
        let baseline = chip.center_y() + text.cap_height() as i32 / 2;
        text.draw_in(
            *script,
            fb,
            chip.x + (chip.w - tw) / 2,
            baseline,
            option,
            picked,
        );
        out.push(chip);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::font::Script;
    use crate::lang::Lang;

    /// A metric with no font behind it: every character 0.6 em, which is
    /// wider than Ember sets and narrower than an ideograph, so a layout that
    /// fits under it fits on the device.
    fn measured(theme: &Theme, options: &[(&str, Script)], width: i32) -> Vec<Rect> {
        let height = chip_height(theme);
        let (mut x, mut y) = (0, 0);
        let mut out = Vec::new();
        for (option, _) in options {
            let em = theme.body_px * 0.6;
            let w = (option.chars().count() as f32 * em) as i32 + CHIP_PAD * 2;
            if x > 0 && x + w > width {
                x = 0;
                y += height + CHIP_GAP;
            }
            out.push(Rect::new(x, y, w, height));
            x += w + CHIP_GAP;
        }
        out
    }

    #[test]
    fn every_chip_is_placed_however_narrow_the_row() {
        // A dropped chip is a setting the reader cannot reach. Every option
        // gets a box, on every panel, in every language.
        let names: Vec<(&str, Script)> = Lang::ALL
            .iter()
            .map(|l| (l.label(), Script::Unknown))
            .collect();
        for (w, h) in [(1264, 1680), (1860, 2480)] {
            let theme = Theme::for_screen(w, h);
            let area = content_box(&theme);
            let width = area.w - area.w / 3;
            let placed = measured(&theme, &names, width);
            assert_eq!(placed.len(), names.len(), "{w}x{h} drops a chip");
            for chip in &placed {
                assert!(chip.right() <= width, "{w}x{h}: {chip:?} runs past {width}");
            }
        }
    }

    /// The stub's width for a run of chips, gaps included.
    fn run_of(theme: &Theme, options: &[(&str, Script)]) -> i32 {
        let em = theme.body_px * 0.6;
        let chips: i32 = options
            .iter()
            .map(|(o, _)| (o.chars().count() as f32 * em) as i32 + CHIP_PAD * 2)
            .sum();
        chips + CHIP_GAP * (options.len().saturating_sub(1)) as i32
    }

    #[test]
    fn the_language_row_stands_on_one_line() {
        // What the abbreviations are for: all five languages stand on one
        // line on the narrow panel. A row that wraps still draws.
        let theme = Theme::for_screen(1264, 1680);
        let names: Vec<(&str, Script)> = Lang::ALL
            .iter()
            .map(|l| (l.label(), Script::Unknown))
            .collect();
        let area = content_box(&theme);
        let sizes: Vec<(&str, Script)> = [
            ("Small", Script::Unknown),
            ("Medium", Script::Unknown),
            ("Large", Script::Unknown),
        ]
        .into();
        let week: Vec<(&str, Script)> = [("Mon", Script::Unknown), ("Sun", Script::Unknown)].into();

        let em = theme.body_px * 0.6;
        let widest = ["Language", "Text size", "Week starts on"]
            .iter()
            .map(|l| (l.chars().count() as f32 * em) as i32)
            .max()
            .unwrap_or(0);
        let runs = [
            run_of(&theme, &names),
            run_of(&theme, &sizes),
            run_of(&theme, &week),
        ];
        let column = column_from(widest, &runs, area.w);

        let placed = measured(&theme, &names, area.w - column);
        let lines: std::collections::BTreeSet<i32> = placed.iter().map(|c| c.y).collect();
        assert_eq!(lines.len(), 1, "the language row wraps: {placed:?}");
        for chip in &placed {
            assert!(
                chip.right() <= area.w - column,
                "{chip:?} runs past the row"
            );
        }
    }

    #[test]
    fn a_row_of_figures_comes_down_to_the_size_that_fits_it() {
        // A set as wide as nine ems, which is what three headline figures run
        // to on a book of a hundred hours.
        let needed = |px: f32| (px * 9.0) as i32;
        assert_eq!(figures_px(99.0, 38.0, 900, needed), 99.0);

        let px = figures_px(99.0, 38.0, 450, needed);
        assert!(needed(px) <= 450, "{px}px still takes {}", needed(px));
        assert!(px > 38.0, "{px}px gives up more than it has to");

        // A row too narrow at any size stops at the floor.
        assert_eq!(figures_px(99.0, 38.0, 10, needed), 38.0);
    }
}
