//! The frame every screen sits in: one strip along the bottom holding Exit and
//! the four tabs. There is no title bar — the tab drawn in reverse names the
//! screen, and every screen states its own figures in its body.

use crate::eink::fb::Framebuffer;

use super::paint::{self, INK, LIGHT, PALE, Rect, WHITE};
use super::text::TextRenderer;
use super::theme::Theme;

/// The screens, in the order their tabs sit in.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Tab {
    Home,
    Calendar,
    Books,
    Clock,
}

impl Tab {
    pub const ALL: [Tab; 4] = [Tab::Home, Tab::Calendar, Tab::Books, Tab::Clock];

    pub fn label(self) -> &'static str {
        match self {
            Tab::Home => "Today",
            Tab::Calendar => "Calendar",
            Tab::Books => "Books",
            Tab::Clock => "Clock",
        }
    }
}

/// Clear the screen to paper.
pub fn clear(fb: &mut Framebuffer, theme: &Theme) {
    paint::fill(fb, theme.screen, WHITE);
}

/// What the leftmost cell of the strip says. Exit is set, not drawn: no face
/// on this firmware carries a power symbol, and the only close marks that
/// exist sit in `code2000` and the display faces, where they would stand
/// against Ember's letters in another face's weight.
const EXIT: &str = "Exit";

/// The bottom strip: Exit, then the four tabs, in five cells of one width.
/// Answers the hit box for Exit and one per tab.
///
/// The tab for the screen showing is drawn in reverse, which is what names the
/// screen. A book is shown over the tab it was opened from, so that tab stays
/// lit and tapping it closes the book.
pub fn tabs(
    fb: &mut Framebuffer,
    text: &mut TextRenderer,
    theme: &Theme,
    active: Tab,
) -> (Rect, Vec<(Tab, Rect)>) {
    let (strip, _) = theme.screen.split_bottom(theme.tabs_h);
    paint::fill(fb, strip, WHITE);
    paint::hline(fb, 0, strip.y, strip.w, LIGHT, 2);

    // Exit takes the first of five equal cells; the tabs take the rest.
    let mut cells = strip.columns(Tab::ALL.len() as i32 + 1, 0).into_iter();
    let exit = cells.next().unwrap_or(strip);

    text.set_px(theme.body_px);
    let baseline = strip.center_y() + text.cap_height() as i32 / 2;
    let w = text.measure_width(EXIT) as i32;
    text.draw(fb, exit.x + (exit.w - w) / 2, baseline, EXIT, false);
    paint::vline(fb, exit.right(), strip.y, strip.h, LIGHT, 2);

    let mut out = Vec::new();
    for (tab, cell) in Tab::ALL.iter().zip(cells) {
        let on = *tab == active;
        if on {
            paint::fill(fb, cell.inset(theme.gap / 2), INK);
        }
        let label = tab.label();
        let w = text.measure_width(label) as i32;
        text.draw(fb, cell.x + (cell.w - w) / 2, baseline, label, on);
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

/// The height [`section`] takes above the box it answers with.
pub fn section_height(text: &mut TextRenderer, theme: &Theme) -> i32 {
    text.set_px(theme.small_px);
    text.line_height() as i32 + theme.gap + theme.gap / 2
}

/// The height [`figure`] draws into.
pub fn figure_height(text: &mut TextRenderer, theme: &Theme) -> i32 {
    text.set_px(theme.display_px);
    let value = text.cap_height() as i32;
    text.set_px(theme.small_px);
    value + theme.gap + text.line_height() as i32
}

/// The width [`figure`] needs: the wider of the number and the name under it.
pub fn figure_width(text: &mut TextRenderer, theme: &Theme, value: &str, label: &str) -> i32 {
    text.set_px(theme.display_px);
    let value = text.measure_width(value) as i32;
    text.set_px(theme.small_px);
    value.max(text.measure_width(label) as i32)
}

/// A figure with its name under it.
pub fn figure(
    fb: &mut Framebuffer,
    text: &mut TextRenderer,
    theme: &Theme,
    area: Rect,
    value: &str,
    label: &str,
) {
    text.set_px(theme.display_px);
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
