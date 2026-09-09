//! The pager along a page's foot: which stretch of the page is showing, and
//! the marks that step or jump to another. A screen paged from its foot
//! stands on this one, so the foot reads the same wherever it is met; a list
//! paged from its own heading carries its marks there instead.

use crate::ui::chrome;
use crate::ui::paint::Rect;
use crate::ui::theme::Theme;

use super::{Ctx, Hit};

/// The four marks along the foot: both ends, and one step either way.
const JUMP_FIRST: &str = "«";
const JUMP_LAST: &str = "»";
const STEP_BACK: &str = "‹";
const STEP_ON: &str = "›";

/// The strip under the page that the counter sits in,
/// [`chrome::chip_height`] tall.
pub fn height(theme: &Theme) -> i32 {
    chrome::chip_height(theme)
}

/// The bottom [`height`] of `area`, deepened by [`chrome::floor_air`].
/// Its foot is the tab strip's top edge.
pub fn foot(theme: &Theme, area: Rect) -> Rect {
    let high = height(theme);
    Rect::new(
        area.x,
        area.bottom() - high,
        area.w,
        high + chrome::floor_air(theme),
    )
}

/// The width a mark along the foot takes, its air included.
fn mark_reach(cx: &mut Ctx) -> i32 {
    let theme: &Theme = cx.theme;
    cx.text.set_px(theme.small_px);
    cx.text.measure_width(JUMP_LAST) as i32 + theme.gap * 4
}

/// The baseline `said` takes for its own ink to centre on `foot`, through
/// [`crate::ui::text::TextRenderer::ink_box`]. `foot.center_y()` where `said`
/// inks nothing.
fn on_centre(cx: &mut Ctx, foot: Rect, said: &str) -> i32 {
    let Some((top, bottom)) = cx.text.ink_box(said) else {
        return foot.center_y();
    };
    foot.center_y() - (top + bottom) / 2
}

/// A mark [`mark_reach`] wide, centred in the run between `from` and `to`,
/// and never over either end of it. A run too narrow to hold the mark opens
/// it at `from`.
fn between(from: i32, to: i32, reach: i32) -> i32 {
    (from + (to - from - reach) / 2).clamp(from, (to - reach).max(from))
}

/// The pager across `foot`: [`JUMP_FIRST`] and [`JUMP_LAST`] at the ends of
/// the row, `label` in the middle, and [`STEP_BACK`] and [`STEP_ON`] each
/// centred in the run left between the two. `open` states whether each way
/// leads anywhere, and `ends` where each jump lands.
pub fn draw(cx: &mut Ctx, foot: Rect, label: &str, open: [bool; 2], ends: [Hit; 2]) {
    let theme: &Theme = cx.theme;
    let reach = mark_reach(cx);
    cx.text.set_px(theme.small_px);
    let width = cx.text.measure_width(label) as i32;
    let middle = foot.x + (foot.w - width) / 2;
    // The steps hold the white either side of `label`, clear of the jumps.
    let back = between(foot.x + reach, middle, reach);
    let on = between(middle + width, foot.right() - reach, reach);
    let marks = [
        (foot.x, JUMP_FIRST, open[0].then_some(ends[0])),
        (back, STEP_BACK, open[0].then_some(Hit::Prev)),
        (on, STEP_ON, open[1].then_some(Hit::Next)),
        (foot.right() - reach, JUMP_LAST, open[1].then_some(ends[1])),
    ];
    for (x, said, hit) in marks {
        mark(cx, foot, x, said, hit);
    }
    cx.text.set_px(theme.small_px);
    let baseline = on_centre(cx, foot, label);
    let script = cx.ui_script();
    cx.text
        .draw_in(script, cx.fb, middle, baseline, label, false);
}

/// One mark of the pager, [`mark_reach`] wide at `x`, taking a tap onto
/// `hit`. A `hit` of `None` draws nothing: the page stands at that end.
fn mark(cx: &mut Ctx, foot: Rect, x: i32, said: &str, hit: Option<Hit>) {
    let Some(hit) = hit else { return };
    let reach = mark_reach(cx);
    let box_ = Rect::new(x, foot.y, reach, foot.h);
    cx.text.set_px(cx.theme.small_px);
    let w = cx.text.measure_width(said) as i32;
    let baseline = on_centre(cx, foot, said);
    cx.text
        .draw(cx.fb, box_.x + (box_.w - w) / 2, baseline, said, false);
    cx.hit(hit, box_);
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ui::theme::tests::PANELS;

    #[test]
    fn the_strip_stands_on_the_tab_strip() {
        for (w, h) in PANELS {
            let theme = Theme::for_screen(w, h);
            let area = chrome::content_box(&theme);
            let strip = foot(&theme, area);
            assert_eq!(
                strip.bottom(),
                h as i32 - theme.tabs_h,
                "{w}x{h}: the strip stops short of the tab strip"
            );
        }
    }

    #[test]
    fn a_step_holds_the_middle_of_the_white_it_stands_in() {
        // A run of 100 holding a mark of 20 opens it 40 in.
        assert_eq!(between(0, 100, 20), 40);
        assert_eq!(between(300, 400, 20), 340);
        // Never over either end, whatever the run.
        assert_eq!(between(0, 20, 20), 0);
        assert_eq!(between(0, 10, 20), 0);
        for to in 0..200 {
            let at = between(30, to, 20);
            assert!(at >= 30, "{to}: the step opens before its run");
            match to - 30 >= 20 {
                true => assert!(at + 20 <= to, "{to}: the step runs past its end"),
                false => assert_eq!(at, 30, "{to}: the step left the run's own start"),
            }
        }
    }

    #[test]
    fn the_strip_takes_its_height_off_the_page_and_borrows_the_floor_air() {
        for (w, h) in PANELS {
            let theme = Theme::for_screen(w, h);
            let area = chrome::content_box(&theme);
            let strip = foot(&theme, area);
            // What a screen sets aside: the strip's own height, no more.
            assert_eq!(
                strip.y,
                area.bottom() - height(&theme),
                "{w}x{h}: the strip does not open where the page ends"
            );
            // The air under the page is read against, never drawn in.
            assert!(
                strip.bottom() > area.bottom(),
                "{w}x{h}: the strip stops inside the page"
            );
        }
    }
}
