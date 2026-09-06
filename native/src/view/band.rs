//! The progress band every screen draws a book's place in: a track, the
//! `Finished` chip right-anchored on the line over it, and the figure set
//! small over the place the reading stands at.

use crate::stats::BookStat;
use crate::ui::paint::{self, INK, Rect, WHITE};
use crate::ui::text::TextRenderer;
use crate::ui::theme::Theme;

use super::Ctx;

/// The bar stating how far through a book is.
pub fn bar_height(theme: &Theme) -> i32 {
    theme.gap.max(6)
}

/// The height a band takes: the bar and the figure over it.
pub fn height(text: &mut TextRenderer, theme: &Theme) -> i32 {
    text.set_px(theme.small_px);
    bar_height(theme) + theme.gap / 2 + text.line_height() as i32
}

/// What one band states.
pub struct Band {
    /// How full the track draws, from [`BookStat::bar_percent`].
    pub fill: i64,
    /// The place the figure names and the notch cuts at. `None` draws
    /// neither.
    pub at: Option<i64>,
    /// Whether the `Finished` chip stands at the right.
    pub finished: bool,
}

impl Band {
    /// One book's band. `finished` is [`BookStat::is_finished`] where the
    /// caller states no narrower rule.
    pub fn of(book: &BookStat, finished: bool) -> Self {
        Self {
            fill: book.bar_percent(),
            at: book.has_percent().then(|| book.percent_shown()),
            finished,
        }
    }
}

/// Draw `of` across the foot of `area`, answering the track it filled.
///
/// The chip takes its place first; the figure keeps to what is left.
pub fn draw(cx: &mut Ctx, area: Rect, of: Band) -> Rect {
    let theme: &Theme = cx.theme;
    let track = Rect::new(
        area.x,
        area.bottom() - bar_height(theme),
        area.w,
        bar_height(theme),
    );
    paint::progress(cx.fb, track, of.fill, 100, INK);
    cx.text.set_px(theme.small_px);
    let baseline = track.y - theme.gap / 2;
    let chip = of.finished.then(|| chip(cx, area, baseline));
    let room = left_of(area, chip, theme.gap * 2);
    if let Some(at) = of.at {
        mark(cx, room, track, at, cx.s().percent_plain, of.fill > at);
    }
    track
}

/// `line` up to `chip`, `air` clear of it.
fn left_of(line: Rect, chip: Option<Rect>, air: i32) -> Rect {
    let right = chip.map_or(line.right(), |c| c.x - air);
    Rect::new(line.x, line.y, (right - line.x).max(1), line.h)
}

/// A book read through, stated at the right of `line` and answering the box it
/// took. Filled, where every other chip on a screen is outlined: this one is
/// not a control and states an end state.
fn chip(cx: &mut Ctx, line: Rect, baseline: i32) -> Rect {
    let theme: &Theme = cx.theme;
    let said = cx.s().shelf_finished;
    let script = cx.ui_script();
    cx.text.set_px(theme.small_px);
    let (w, cap) = (
        cx.text.measure_width_in(script, said) as i32,
        cx.text.cap_height() as i32,
    );
    let h = cx.text.line_height() as i32 + theme.gap / 2;
    let box_ = Rect::new(
        line.right() - w - theme.gap * 2,
        baseline - cap / 2 - h / 2,
        w + theme.gap * 2,
        h,
    );
    paint::fill(cx.fb, box_, INK);
    cx.text.draw_in(
        script,
        cx.fb,
        box_.x + theme.gap,
        box_.center_y() + cap / 2,
        said,
        true,
    );
    box_
}

/// How wide [`mark`] cuts.
fn mark_width(theme: &Theme) -> i32 {
    paint::notch_width(theme.gap)
}

/// Where [`mark`] cuts in `track`, `at` per cent along it.
fn mark_x(theme: &Theme, track: Rect, at: i64) -> i32 {
    paint::notch_x(track, at, mark_width(theme))
}

/// `said` filled with `at` and set small over the place it names, inside
/// `line`. Under `cut` a white notch cuts the ink there.
fn mark(cx: &mut Ctx, line: Rect, track: Rect, at: i64, said: &str, cut: bool) {
    let theme: &Theme = cx.theme;
    let (w, x) = (mark_width(theme), mark_x(theme, track, at));
    if cut {
        paint::fill(cx.fb, Rect::new(x, track.y, w, track.h), WHITE);
    }
    let script = cx.ui_script();
    cx.text.set_px(theme.small_px);
    let said = crate::lang::counted(said, at);
    let tw = cx.text.measure_width_in(script, &said) as i32;
    let put = (x + w / 2 - tw / 2).clamp(line.x, (line.right() - tw).max(line.x));
    cx.text
        .draw_in(script, cx.fb, put, track.y - theme.gap, &said, false);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_mark_stands_inside_the_track_at_either_end() {
        let theme = Theme::for_screen(1264, 1680);
        let track = Rect::new(39, 900, 1186, bar_height(&theme));
        for at in [0, 1, 55, 99, 100] {
            let x = mark_x(&theme, track, at);
            assert!(x >= track.x, "{at}%: the mark starts left of the track");
            assert!(
                x + mark_width(&theme) <= track.right(),
                "{at}%: the mark runs past the track"
            );
        }
        // The mark tracks the figure it stands for.
        assert!(mark_x(&theme, track, 20) < mark_x(&theme, track, 80));
    }

    #[test]
    fn a_chip_leaves_the_figure_the_line_up_to_it() {
        let line = Rect::new(0, 0, 1000, 40);
        let chip = Rect::new(800, 0, 200, 40);
        assert_eq!(left_of(line, Some(chip), 10).w, 790);
        assert_eq!(left_of(line, None, 10).w, 1000);
        // A chip wider than the line leaves one column, never a negative.
        assert_eq!(left_of(line, Some(Rect::new(-50, 0, 200, 40)), 10).w, 1);
    }
}
