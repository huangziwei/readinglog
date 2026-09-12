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

// Exit is set, not drawn: no face on this firmware carries a power symbol.

/// The bottom strip: Exit, then the four tabs, in five cells of one width,
/// answering a hit box each. The tab showing is drawn in reverse; a book is
/// shown over the tab it was opened from, and a tap on that tab closes it.
pub fn tabs(
    fb: &mut Framebuffer,
    text: &mut TextRenderer,
    theme: &Theme,
    lang: Lang,
    active: Tab,
) -> (Rect, Vec<(Tab, Rect)>) {
    let (strip, _) = theme.screen.split_bottom(theme.tabs_h);
    paint::fill(fb, strip, WHITE);
    paint::hline(fb, 0, strip.y, strip.w, LIGHT, theme.rule());

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
    paint::vline(fb, exit.right(), strip.y, strip.h, LIGHT, theme.rule());

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

/// The air [`content`] leaves between its foot and the tab strip: half a gap
/// under the `theme.gap * 2` it stands in. Anything set against the box's foot
/// centres on this band as well as its own.
pub fn floor_air(theme: &Theme) -> i32 {
    theme.gap * 2 - theme.gap / 2
}

/// The content box: the screen above the strip. `theme.pad` either side,
/// [`floor_air`] under the box, and what that leaves of `theme.gap * 4`
/// over it.
pub fn content(theme: &Theme, area: Rect) -> Rect {
    let (_, rest) = area.split_bottom(theme.tabs_h);
    let under = floor_air(theme);
    let over = theme.gap * 4 - under;
    Rect::new(
        theme.pad,
        rest.y + over,
        theme.screen.w - theme.pad * 2,
        (rest.h - over - under).max(1),
    )
}

/// [`content`] from the theme alone.
pub fn content_box(theme: &Theme) -> Rect {
    content(theme, theme.screen)
}

/// How far under the size it opens at type may be set to fit its room.
pub const SHRINK_FLOOR: f32 = 0.6;

/// The largest size at or under `opening` that sets every line of `said`
/// inside the room it stands in, never under `floor`. `room` answers the width
/// the line at that index has; `width` measures a line at a size.
pub fn shrink_to_fit(
    opening: f32,
    floor: f32,
    said: &[&str],
    room: &dyn Fn(usize) -> i32,
    mut width: impl FnMut(f32, &str) -> i32,
) -> f32 {
    let mut px = opening;
    while px > floor {
        // The line furthest over the room it stands in settles the step.
        let tightest = said
            .iter()
            .enumerate()
            .map(|(at, line)| room(at) as f32 / width(px, line).max(1) as f32)
            .fold(f32::INFINITY, f32::min);
        if tightest >= 1.0 {
            break;
        }
        // A width is near enough proportional to `px` to land in one step; the
        // pixel taken off it settles the rounding.
        px = (px * tightest).min(px - 1.0).max(floor);
    }
    px
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
/// centred on the title's own ink. Everything on the row takes this one
/// centre.
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

/// The height `figure` draws into, set no larger than `ceiling`.
pub fn figure_height_at(text: &mut TextRenderer, theme: &Theme, ceiling: f32) -> i32 {
    text.set_px(ceiling);
    let value = text.cap_height() as i32;
    text.set_px(theme.small_px);
    value + theme.gap + text.line_height() as i32
}

/// The height `figure` draws into at [`Theme::display_px`].
pub fn figure_height(text: &mut TextRenderer, theme: &Theme) -> i32 {
    figure_height_at(text, theme, theme.display_px)
}

/// The width a figure set at `px` needs: the wider of the number and the name
/// under it.
fn figure_width(text: &mut TextRenderer, theme: &Theme, value: &str, label: &str, px: f32) -> i32 {
    text.set_px(px);
    let value = text.measure_solid(value) as i32;
    text.set_px(theme.small_px);
    value.max(text.measure_width(label) as i32)
}

/// A figure at `px` with its name under it, answering the box the name took.
fn figure(
    fb: &mut Framebuffer,
    text: &mut TextRenderer,
    theme: &Theme,
    area: Rect,
    value: &str,
    label: &str,
    px: f32,
) -> Rect {
    text.set_px(px);
    let w = text.measure_solid(value) as i32;
    let top = area.y + text.cap_height() as i32;
    text.draw_solid(fb, area.x + (area.w - w) / 2, top, value, false);

    text.set_px(theme.small_px);
    let lw = text.measure_width(label) as i32;
    let baseline = top + theme.gap + text.line_height() as i32;
    let named = Rect::new(
        area.x + (area.w - lw) / 2,
        baseline - text.cap_height() as i32,
        lw,
        text.cap_height() as i32,
    );
    text.draw(fb, named.x, baseline, label, false);
    named
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
    figures_at(fb, text, theme, row, stated, theme.display_px, &[]);
}

/// [`figures`] set no larger than `ceiling`, for a row standing among a
/// page's bands. A figure `opens` names is underlined, and each box is
/// answered for the hit the caller takes on it.
pub fn figures_at(
    fb: &mut Framebuffer,
    text: &mut TextRenderer,
    theme: &Theme,
    row: Rect,
    stated: &[(String, &str)],
    ceiling: f32,
    opens: &[bool],
) -> Vec<Rect> {
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
    let mut out = Vec::with_capacity(stated.len());
    for (at, (cell, (value, label))) in row.spread(&widths, air).into_iter().zip(stated).enumerate()
    {
        let named = figure(fb, text, theme, cell, value, label, px);
        if opens.get(at).copied().unwrap_or(false) {
            paint::hline(
                fb,
                named.x,
                named.bottom() + theme.gap / 2,
                named.w,
                LIGHT,
                theme.rule(),
            );
        }
        out.push(cell);
    }
    out
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

/// A chip's own blank space, in design pixels: its sides, the next chip, a break.
const CHIP_PAD: i32 = 20;
const CHIP_GAP: i32 = 14;
const CHIP_BREAK: i32 = 60;

/// The air a chip keeps either side of its label.
pub fn chip_pad(theme: &Theme) -> i32 {
    theme.px(CHIP_PAD)
}

/// The air between one chip and the next, and between a run and the label
/// beside it.
pub fn chip_gap(theme: &Theme) -> i32 {
    theme.px(CHIP_GAP)
}

/// The air standing before the chip a run breaks at, in place of [`chip_gap`].
fn chip_break(theme: &Theme) -> i32 {
    theme.px(CHIP_BREAK)
}

/// How tall one chip is.
pub fn chip_height(theme: &Theme) -> i32 {
    theme.row_h * 2 / 3
}

/// One row's chips: what each reads, its script, and the chip a break stands before.
pub type Run<'a> = (Vec<(&'a str, crate::font::Script)>, Option<usize>);

/// Where the second column starts on every row: from the widest label, pulled
/// back until the widest chip run fits, held between `width / 3` and
/// `width / 2`. One column for the whole page.
pub fn chip_column(
    text: &mut TextRenderer,
    theme: &Theme,
    labels: &[&str],
    runs: &[Run],
    width: i32,
) -> i32 {
    text.set_px(theme.body_px);
    let widest = labels
        .iter()
        .map(|label| text.measure_width(label) as i32)
        .max()
        .unwrap_or(0);
    let runs: Vec<i32> = runs
        .iter()
        .map(|(run, apart)| run_width(text, theme, run, *apart))
        .collect();
    column_from(widest, &runs, width, chip_gap(theme))
}

/// [`chip_column`]'s arithmetic, over measured widths. The labels have first
/// call: a column pulled back under the widest of them draws it over the chips
/// beside it. `chip_layout` wraps a run left short of space.
fn column_from(widest_label: i32, runs: &[i32], width: i32, gap: i32) -> i32 {
    let wanted = widest_label + gap * 3;
    let room = runs.iter().map(|run| width - run).min().unwrap_or(i32::MAX);
    wanted.min(room.max(wanted).min(width / 2))
}

/// How wide a run of chips is once tiled, gaps and any break included.
fn run_width(
    text: &mut TextRenderer,
    theme: &Theme,
    options: &[(&str, crate::font::Script)],
    apart: Option<usize>,
) -> i32 {
    text.set_px(theme.body_px);
    let pad = chip_pad(theme);
    let chips: i32 = options
        .iter()
        .map(|(o, script)| text.measure_width_in(*script, o) as i32 + pad * 2)
        .sum();
    let gaps = chip_gap(theme) * (options.len().saturating_sub(1)) as i32;
    chips + gaps + broken(theme, options.len(), apart)
}

/// What a break adds to a run of `count` chips, over the gap it stands in
/// place of. A break at neither end of the run, or none at all, adds nothing.
fn broken(theme: &Theme, count: usize, apart: Option<usize>) -> i32 {
    match apart {
        Some(at) if at > 0 && at < count => chip_break(theme) - chip_gap(theme),
        _ => 0,
    }
}

/// Where every chip of a row lands, wrapped to `width`, laid out from
/// `(0, 0)` with [`place`]'s break at `apart`. Separated from the paint.
pub fn chip_layout(
    text: &mut TextRenderer,
    theme: &Theme,
    options: &[(&str, crate::font::Script)],
    apart: Option<usize>,
    width: i32,
) -> Vec<Rect> {
    text.set_px(theme.body_px);
    let pad = chip_pad(theme);
    let widths: Vec<i32> = options
        .iter()
        .map(|(o, script)| text.measure_width_in(*script, o) as i32 + pad * 2)
        .collect();
    place(theme, &widths, apart, width)
}

/// [`chip_layout`] over measured widths. The chip `apart` names opens on
/// [`chip_break`] in place of the ordinary gap; one that wraps takes the head
/// of its line and keeps no break.
fn place(theme: &Theme, widths: &[i32], apart: Option<usize>, width: i32) -> Vec<Rect> {
    let (gap, height) = (chip_gap(theme), chip_height(theme));
    let (mut x, mut y) = (0, 0);
    let mut out = Vec::new();
    for (i, w) in widths.iter().enumerate() {
        if x > 0 && apart == Some(i) {
            x += chip_break(theme) - gap;
        }
        if x > 0 && x + w > width {
            x = 0;
            y += height + gap;
        }
        out.push(Rect::new(x, y, *w, height));
        x += w + gap;
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

/// One control: `said` centred in a 2 px outline. What the book screen's own
/// controls and every dialog answer are drawn as.
pub fn outlined(cx: &mut crate::view::Ctx, box_: Rect, said: &str) {
    let theme: &Theme = cx.theme;
    let script = cx.ui_script();
    cx.text.set_px(theme.body_px);
    paint::stroke(cx.fb, box_, INK, theme.rule());
    let tw = cx.text.measure_width_in(script, said) as i32;
    let baseline = box_.center_y() + cx.text.cap_height() as i32 / 2;
    cx.text.draw_in(
        script,
        cx.fb,
        box_.x + (box_.w - tw) / 2,
        baseline,
        said,
        false,
    );
}

/// Every option of a setting, side by side, the one in use filled and the rest
/// outlined, at the places [`chip_layout`] put them, answering one box each.
/// The caller sizes the row from that same layout.
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
            false => paint::stroke(fb, chip, INK, theme.rule()),
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

    /// A metric with no font behind it: every character 0.6 em, wider than
    /// Ember sets and narrower than an ideograph.
    fn widths(theme: &Theme, options: &[(&str, Script)]) -> Vec<i32> {
        let em = theme.body_px * 0.6;
        options
            .iter()
            .map(|(o, _)| (o.chars().count() as f32 * em) as i32 + chip_pad(theme) * 2)
            .collect()
    }

    /// [`place`] on [`widths`].
    fn measured(theme: &Theme, options: &[(&str, Script)], width: i32) -> Vec<Rect> {
        place(theme, &widths(theme, options), None, width)
    }

    /// Every panel the app draws on, densest first.
    const PANELS: [(u32, u32); 6] = [
        (1264, 1680),
        (1860, 2480),
        (1236, 1648),
        (1072, 1448),
        (758, 1024),
        (600, 800),
    ];

    #[test]
    fn the_content_box_hangs_below_the_centre_of_the_page() {
        for (w, h) in PANELS {
            let theme = Theme::for_screen(w, h);
            let box_ = content_box(&theme);
            let floor = h as i32 - theme.tabs_h;
            let (over, under) = (box_.y, floor - box_.bottom());
            assert_eq!(under, floor_air(&theme), "{w}x{h}: the floor air differs");
            assert!(
                under < over,
                "{w}x{h}: {under} px under the box against {over} px over it"
            );
            // Below centre, and only just: the block moves, it does not shrink.
            assert!(
                over - under <= theme.gap,
                "{w}x{h}: {} px is more than a nudge",
                over - under
            );
            assert_eq!(over + under, theme.gap * 4, "{w}x{h}: the box resized");
        }
    }

    #[test]
    fn every_chip_is_placed_however_narrow_the_row() {
        // Every option gets a box, on every panel, in every language.
        let names: Vec<(&str, Script)> = Lang::ALL
            .iter()
            .map(|l| (l.label(), Script::Unknown))
            .collect();
        for (w, h) in PANELS {
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
        let chips: i32 = widths(theme, options).iter().sum();
        chips + chip_gap(theme) * (options.len().saturating_sub(1)) as i32
    }

    #[test]
    fn a_chip_set_apart_opens_wider_than_the_run_it_follows() {
        for (w, h) in PANELS {
            let theme = Theme::for_screen(w, h);
            let each = widths(&theme, &[("Show", Script::Unknown); 3]);
            let room = each.iter().sum::<i32>() + chip_break(&theme) * 3;

            let run = place(&theme, &each, None, room);
            let apart = place(&theme, &each, Some(2), room);
            // The two values stand where they stood; the button alone moves,
            // and it moves by more than the air between two chips.
            assert_eq!(apart[..2], run[..2], "{w}x{h}");
            assert_eq!(apart[2].y, run[2].y, "{w}x{h}: the button wrapped");
            let moved = apart[2].x - run[2].x;
            assert_eq!(moved, chip_break(&theme) - chip_gap(&theme), "{w}x{h}");
            assert!(
                moved > chip_gap(&theme),
                "{w}x{h}: {moved} px reads as a gap"
            );

            // A break at the head of a run is no break: nothing precedes it.
            assert_eq!(place(&theme, &each, Some(0), room), run, "{w}x{h}");
        }
    }

    #[test]
    fn a_chip_set_apart_takes_the_head_of_its_line_where_it_wraps() {
        let theme = Theme::for_screen(PANELS[0].0, PANELS[0].1);
        let each = widths(&theme, &[("Show", Script::Unknown); 3]);
        // Room for the three tiled, and none for the break.
        let tight = each.iter().sum::<i32>() + chip_gap(&theme) * 2;
        let apart = place(&theme, &each, Some(2), tight);
        assert_eq!(apart[2].x, 0, "a wrapped button keeps its break");
        assert!(apart[2].y > apart[1].y, "the button did not wrap");
        assert!(apart[2].right() <= tight, "the button runs past the row");
    }

    #[test]
    fn a_run_is_measured_with_the_break_it_will_be_drawn_with() {
        let theme = Theme::for_screen(PANELS[0].0, PANELS[0].1);
        let extra = chip_break(&theme) - chip_gap(&theme);
        assert_eq!(broken(&theme, 3, Some(2)), extra);
        // A break at neither end of the run stands for nothing.
        assert_eq!(broken(&theme, 3, None), 0);
        assert_eq!(broken(&theme, 3, Some(0)), 0);
        assert_eq!(broken(&theme, 3, Some(3)), 0);
    }

    #[test]
    fn the_language_row_stands_on_one_line() {
        // All five languages stand on one line on every panel: the chips and
        // the type shrink with the density together.
        let names: Vec<(&str, Script)> = Lang::ALL
            .iter()
            .map(|l| (l.label(), Script::Unknown))
            .collect();
        let sizes: Vec<(&str, Script)> = [
            ("Small", Script::Unknown),
            ("Medium", Script::Unknown),
            ("Large", Script::Unknown),
        ]
        .into();
        let week: Vec<(&str, Script)> = [("Mon", Script::Unknown), ("Sun", Script::Unknown)].into();

        for (w, h) in PANELS {
            let theme = Theme::for_screen(w, h);
            let area = content_box(&theme);
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
            let column = column_from(widest, &runs, area.w, chip_gap(&theme));

            let placed = measured(&theme, &names, area.w - column);
            let lines: std::collections::BTreeSet<i32> = placed.iter().map(|c| c.y).collect();
            assert_eq!(
                lines.len(),
                1,
                "{w}x{h}: the language row wraps: {placed:?}"
            );
            for chip in &placed {
                assert!(
                    chip.right() <= area.w - column,
                    "{w}x{h}: {chip:?} runs past the row"
                );
            }
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
