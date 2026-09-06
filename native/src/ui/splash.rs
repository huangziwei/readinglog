//! A headline, a note and a step, centred on [`Theme::screen`]. Every line
//! wraps to the panel: these are translated five ways, and a line that runs off
//! the screen in one language cannot be read.

use crate::lang::Strings;
use anyhow::Result;

use crate::eink::fb::{Framebuffer, MxcfbRect, WAVEFORM_MODE_DU, WAVEFORM_MODE_GC16};
use crate::font::Script;
use crate::ui::paint::{self, WHITE};
use crate::ui::text::TextRenderer;
use crate::ui::theme::Theme;

/// Lines the headline takes, and lines each line of the note takes.
const HEADLINE_LINES: usize = 2;
const NOTE_LINES: usize = 3;

/// What a banner says: a headline, the lines under it, and the way out while
/// there is one. `script` is the convention the words are set in — 日本語 drawn
/// from a Simplified face is the defect carrying it here prevents.
pub struct Words<'a> {
    pub script: Script,
    pub headline: &'a str,
    pub note: &'a [String],
    pub step: &'a str,
}

/// Draw `said` centred, and present the screen.
///
/// `first` picks [`WAVEFORM_MODE_GC16`] over [`WAVEFORM_MODE_DU`].
pub fn show(
    fb: &mut Framebuffer,
    text: &mut TextRenderer,
    theme: &Theme,
    said: &Words,
    first: bool,
) -> Result<()> {
    let script = said.script;
    paint::fill(fb, theme.screen, WHITE);
    let room = (theme.screen.w - theme.pad * 2).max(1) as u32;
    let mut y = theme.screen.y + theme.screen.h / 3;

    text.set_px(theme.head_px);
    for (at, line) in text
        .wrap_and_clamp_in(script, said.headline, room, HEADLINE_LINES)
        .iter()
        .enumerate()
    {
        if at > 0 {
            y += text.line_height() as i32;
        }
        centre(fb, text, theme, script, y, line);
    }
    y += theme.head_px as i32;

    text.set_px(theme.body_px);
    for line in said.note {
        // An empty line takes one line of height.
        if line.is_empty() {
            y += text.line_height() as i32;
            continue;
        }
        for wrapped in text.wrap_and_clamp_in(script, line, room, NOTE_LINES) {
            y += text.line_height() as i32;
            centre(fb, text, theme, script, y, &wrapped);
        }
    }

    text.set_px(theme.small_px);
    let y = y + theme.head_px as i32;
    for line in text.wrap_and_clamp_in(script, said.step, room, 1) {
        centre(fb, text, theme, script, y, &line);
    }

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
fn centre(
    fb: &mut Framebuffer,
    text: &mut TextRenderer,
    theme: &Theme,
    script: Script,
    y: i32,
    s: &str,
) {
    let w = text.measure_width_in(script, s) as i32;
    let x = theme.screen.x + (theme.screen.w - w) / 2;
    text.draw_in(script, fb, x, y, s, false);
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
