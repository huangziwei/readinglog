//! A question standing over whatever screen raised it: a scrim, a boxed
//! headline and note, and the answers along the foot.
//!
//! It is drawn from the ordinary `draw`, off state the screen holds, so every
//! question can be rendered off the device.

use crate::font::Script;
use crate::ui::chrome;
use crate::ui::paint::{self, INK, Rect, WHITE};
use crate::ui::theme::Theme;
use crate::view::{Ctx, Hit};

/// How many lines the headline and the note are each allowed. The note's
/// budget is what the longest question needs to state its whole case without a
/// clamp; `lang`'s own tests hold every question inside it, and hold the box
/// that many lines make inside the screen.
const HEAD_LINES: usize = 2;
const NOTE_LINES: usize = 8;

/// What a question says and what may be answered.
pub struct Question<'a> {
    pub heading: &'a str,
    /// One paragraph, or one per answer in the answers' own order.
    pub note: &'a str,
    /// Label and hit for each answer, the way out first.
    pub answers: &'a [(&'a str, Hit)],
}

/// Draw `question` over `area`.
///
/// `area` takes [`Hit::Dismiss`] before anything else, so a tap outside the
/// box takes the question down and a tap on an answer, pushed later, wins its
/// own box: `App::tapped` reads the hits in reverse.
pub fn draw(cx: &mut Ctx, area: Rect, question: &Question) {
    let theme: &Theme = cx.theme;
    let script = cx.ui_script();
    cx.hit(Hit::Dismiss, area);

    let pad = theme.gap * 3;
    let width = area.w - theme.gap * 6;
    let inner = width - pad * 2;

    cx.text.set_px(theme.head_px);
    let heading = cx
        .text
        .wrap_and_clamp_in(script, question.heading, inner as u32, HEAD_LINES);
    let head_h = heading.len() as i32 * cx.text.line_height() as i32;
    cx.text.set_px(theme.body_px);
    let note = cx
        .text
        .wrap_and_clamp_in(script, question.note, inner as u32, NOTE_LINES);
    let note_h = note.len() as i32 * cx.text.line_height() as i32;

    let said: Vec<i32> = question
        .answers
        .iter()
        .map(|(l, _)| {
            (cx.text.measure_width_in(script, l) as i32 + chrome::chip_pad() * 4).min(inner)
        })
        .collect();
    let chip = chrome::chip_height(theme);
    let abreast = abreast(&said, theme, inner);
    let rows = match abreast {
        true => 1,
        false => said.len() as i32,
    };
    let feet_h = chip * rows + theme.gap * (rows - 1);

    let high = (pad * 2 + head_h + theme.gap * 2 + note_h + theme.gap * 3 + feet_h).min(area.h);
    let panel = Rect::new(
        area.x + (area.w - width) / 2,
        area.y + (area.h - high) / 2,
        width,
        high,
    );
    paint::fill(cx.fb, panel, WHITE);
    paint::stroke(cx.fb, panel, INK, 3);

    let (heads, rest) = panel.inset(pad).split_top(head_h + theme.gap * 2);
    let (notes, feet) = rest.split_top(note_h + theme.gap * 3);
    cx.text.set_px(theme.head_px);
    lines(cx, heads, script, &heading);
    cx.text.set_px(theme.body_px);
    lines(cx, notes, script, &note);

    let boxes = match abreast {
        true => {
            let mut x =
                feet.right() - (said.iter().sum::<i32>() + theme.gap * 2 * said.len() as i32);
            said.iter()
                .map(|w| {
                    let box_ = Rect::new(x, feet.y, *w, chip);
                    x += w + theme.gap * 2;
                    box_
                })
                .collect()
        }
        false => feet.rows(said.len() as i32, theme.gap),
    };
    for ((said, hit), box_) in question.answers.iter().zip(&boxes) {
        chrome::outlined(cx, *box_, said);
        cx.hit(*hit, *box_);
    }
}

/// Whether the answers stand in one row, gaps and all.
fn abreast(said: &[i32], theme: &Theme, inner: i32) -> bool {
    said.iter().sum::<i32>() + theme.gap * 2 * said.len() as i32 <= inner
}

/// `said` set down `box_` from its top, at whatever size is set.
fn lines(cx: &mut Ctx, box_: Rect, script: Script, said: &[String]) {
    let step = cx.text.line_height() as i32;
    let mut y = box_.y + cx.text.cap_height() as i32;
    for line in said {
        cx.text.draw_in(script, cx.fb, box_.x, y, line, false);
        y += step;
    }
}
