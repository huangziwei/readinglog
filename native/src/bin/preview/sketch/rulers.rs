//! What a screen has to spend: the content box, banded into rows, against the
//! sizes the theme states for this panel.

use readinglog_native::ui::paint::{self, DARK, INK, LIGHT, PALE, Rect};
use readinglog_native::view::{Ctx, State};

/// The content box in rows, each numbered, with the theme's own sizes listed
/// down the right.
pub fn draw(cx: &mut Ctx, area: Rect, _state: &State) {
    let theme = cx.theme;
    let (row_h, gap, pad) = (theme.row_h, theme.gap, theme.pad);
    paint::stroke(cx.fb, area, DARK, 2);

    let rows = area.h / row_h;
    for at in 0..rows {
        let band = Rect::new(area.x, area.y + at * row_h, area.w, row_h);
        paint::hline(cx.fb, band.x, band.y, band.w, LIGHT, 1);
        // Every other band is shaded, which makes the count readable.
        if at % 2 == 1 {
            paint::fill(cx.fb, band.inset(1), 0xF6);
        }
        cx.text.set_px(theme.small_px);
        let mark = format!("{}", at + 1);
        let baseline = band.y + theme.small_px as i32;
        cx.text.draw(cx.fb, band.x + gap, baseline, &mark, false);
    }

    // `pad` and `gap` drawn at their own width, against the left edge.
    paint::fill(cx.fb, Rect::new(area.x, area.y, pad, row_h / 3), PALE);
    paint::fill(
        cx.fb,
        Rect::new(area.x, area.y + row_h / 3, gap, row_h / 3),
        LIGHT,
    );

    let sizes = [
        ("screen", format!("{}x{}", theme.screen.w, theme.screen.h)),
        ("content", format!("{}x{}", area.w, area.h)),
        ("rows", format!("{rows} of {row_h}px")),
        ("pad / gap", format!("{pad} / {gap}")),
        ("display", format!("{:.0}", theme.display_px)),
        ("head", format!("{:.0}", theme.head_px)),
        ("body", format!("{:.0}", theme.body_px)),
        ("small", format!("{:.0}", theme.small_px)),
        ("tabs", format!("{}", theme.tabs_h)),
    ];
    let mut y = area.y + row_h;
    for (name, value) in sizes {
        cx.text.set_px(theme.body_px);
        let line = format!("{name}  {value}");
        let w = cx.text.measure_width(&line) as i32;
        let box_ = Rect::new(
            area.right() - w - gap * 2,
            y - theme.body_px as i32,
            w + gap * 2,
            theme.body_px as i32 + gap,
        );
        paint::fill(cx.fb, box_, paint::WHITE);
        paint::stroke(cx.fb, box_, INK, 1);
        cx.text.draw(cx.fb, box_.x + gap, y, &line, false);
        y += row_h;
    }
}
