//! A headline, a note and a step, centred on [`Theme::screen`].

use crate::lang::Strings;
use anyhow::Result;

use crate::eink::fb::{Framebuffer, MxcfbRect, WAVEFORM_MODE_DU, WAVEFORM_MODE_GC16};
use crate::ui::paint::{self, WHITE};
use crate::ui::text::TextRenderer;
use crate::ui::theme::Theme;

/// Draw `headline`, `note` and `step` centred, and present the screen.
///
/// `first` picks [`WAVEFORM_MODE_GC16`] over [`WAVEFORM_MODE_DU`].
pub fn show(
    fb: &mut Framebuffer,
    text: &mut TextRenderer,
    theme: &Theme,
    headline: &str,
    note: &[String],
    step: &str,
    first: bool,
) -> Result<()> {
    paint::fill(fb, theme.screen, WHITE);
    let mid = theme.screen.y + theme.screen.h / 3;

    text.set_px(theme.head_px);
    centre(fb, text, theme, mid, headline);

    text.set_px(theme.body_px);
    let mut y = mid + theme.head_px as i32;
    for line in note {
        y += text.line_height() as i32;
        centre(fb, text, theme, y, line);
    }

    text.set_px(theme.small_px);
    centre(fb, text, theme, y + theme.head_px as i32, step);

    fb.send_update(
        MxcfbRect {
            top: 0,
            left: 0,
            width: theme.screen.w as u32,
            height: theme.screen.h as u32,
        },
        match first {
            true => WAVEFORM_MODE_GC16,
            false => WAVEFORM_MODE_DU,
        },
    )?;
    Ok(())
}

/// `s` on the baseline `y`, centred across [`Theme::screen`].
fn centre(fb: &mut Framebuffer, text: &mut TextRenderer, theme: &Theme, y: i32, s: &str) {
    let w = text.measure_width(s) as i32;
    text.draw(fb, theme.screen.x + (theme.screen.w - w) / 2, y, s, false);
}

/// The lines under the headline, for a store at `mark`.
///
/// An empty `mark` names a store that has read no log line.
pub fn note(mark: &str, s: &Strings) -> Vec<String> {
    match mark.is_empty() {
        true => vec![s.first_run_1.into(), s.first_run_2.into()],
        false => vec![s.catching_up.into()],
    }
}
