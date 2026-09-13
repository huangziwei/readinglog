//! The archives, listed over the config page. A row is one tap target, two
//! lines tall: when the archive was written and what it weighs above what it
//! holds. Every row raises a question of its own.

use crate::ui::chrome;
use crate::ui::paint::{self, INK, LIGHT, Rect, WHITE};

use super::{Ctx, Hit};

/// The outline standing the list off the page, as `ui::dialog` draws one.
const BORDER: i32 = 3;

/// The most rows a page holds, whatever room the panel has.
const MOST_ROWS: usize = 8;

/// One archive as its two lines read, gathered before the page is drawn.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Row {
    /// When it was written, else the file's own name.
    pub when: String,
    pub size: String,
    /// What it holds, and whether the record holds all of it.
    pub holds: String,
}

/// What the list states on its heading: how many archives, and their weight.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Listed {
    pub rows: Vec<Row>,
    pub said: String,
}

/// The height one archive's two lines take, air included.
fn row_height(cx: &mut Ctx) -> i32 {
    let theme = cx.theme;
    cx.text.set_px(theme.body_px);
    let first = cx.text.line_height() as i32;
    cx.text.set_px(theme.small_px);
    let second = cx.text.line_height() as i32;
    (first + second + theme.gap * 2).max(chrome::chip_height(theme) + theme.gap)
}

/// How many rows one page of `area` holds.
pub fn per_page(cx: &mut Ctx, area: Rect) -> usize {
    let theme = cx.theme;
    let pad = theme.gap * 3;
    let step = row_height(cx);
    cx.text.set_px(cx.theme.head_px);
    let head_h = cx.text.line_height() as i32;
    let foot_h = chrome::chip_height(cx.theme);
    let room = area.h - pad * 2 - head_h - theme.gap * 4 - foot_h;
    (room / step).clamp(1, MOST_ROWS as i32) as usize
}

/// Draw `listed` over `area`, from the row at `from`.
pub fn draw(cx: &mut Ctx, area: Rect, listed: &Listed, from: usize) {
    let theme = cx.theme;
    cx.hit(Hit::BackupsClose, area);

    let pad = theme.gap * 3;
    let width = area.w - theme.gap * 6;
    let step = row_height(cx);
    let fits = per_page(cx, area);
    // Every page opens on a row `fits` divides, and none opens past the last.
    let last = listed.rows.len().saturating_sub(1) / fits * fits;
    let from = (from / fits * fits).min(last);
    let showing = listed.rows.len().saturating_sub(from).min(fits).max(1);

    cx.text.set_px(theme.head_px);
    let head_h = cx.text.line_height() as i32;
    let foot_h = chrome::chip_height(theme);
    let high = pad * 2 + head_h + theme.gap * 2 + step * showing as i32 + theme.gap * 2 + foot_h;
    let panel = Rect::new(
        area.x + (area.w - width) / 2,
        area.y + (area.h - high.min(area.h)) / 2,
        width,
        high.min(area.h),
    );
    paint::fill(cx.fb, panel, WHITE);
    paint::stroke(cx.fb, panel, INK, theme.px(BORDER));

    let inner = panel.inset(pad);
    let (heads, rest) = inner.split_top(head_h + theme.gap * 2);
    head(cx, heads, &listed.said);

    let (list, feet) = rest.split_top(step * showing as i32 + theme.gap * 2);
    for (at, row) in listed.rows.iter().skip(from).take(showing).enumerate() {
        let box_ = Rect::new(list.x, list.y + step * at as i32, list.w, step);
        archive(cx, box_, row);
        cx.hit(Hit::Restore(from + at), box_);
        if at + 1 < showing {
            let rule = cx.theme.rule();
            paint::hline(cx.fb, box_.x, box_.bottom(), box_.w, LIGHT, rule);
        }
    }
    foot(cx, feet, from, fits, listed.rows.len());
}

/// The heading, with the figure the list is measured in at its right end.
fn head(cx: &mut Ctx, box_: Rect, said: &str) {
    let theme = cx.theme;
    let script = cx.ui_script();
    let heading = cx.s().restore_row;
    cx.text.set_px(theme.head_px);
    let y = box_.y + cx.text.cap_height() as i32;
    cx.text.draw_in(script, cx.fb, box_.x, y, heading, false);
    cx.text.set_px(theme.body_px);
    let w = cx.text.measure_width_in(script, said) as i32;
    cx.text
        .draw_in(script, cx.fb, box_.right() - w, y, said, false);
    let rule = theme.rule();
    paint::hline(cx.fb, box_.x, box_.bottom() - theme.gap, box_.w, INK, rule);
}

/// The largest size at or under `opening` that sets `said` inside `room`.
fn fitted(cx: &mut Ctx, opening: f32, said: &str, room: i32) -> f32 {
    let script = cx.ui_script();
    let text = &mut *cx.text;
    chrome::shrink_to_fit(
        opening,
        opening * chrome::SHRINK_FLOOR,
        &[said],
        &|_| room,
        |px, line| {
            text.set_px(px);
            text.measure_width_in(script, line) as i32
        },
    )
}

/// One archive: when and how heavy above what it holds.
fn archive(cx: &mut Ctx, box_: Rect, row: &Row) {
    let script = cx.ui_script();
    let (gap, body, small) = (cx.theme.gap, cx.theme.body_px, cx.theme.small_px);
    cx.text.set_px(body);
    let y = box_.y + gap + cx.text.cap_height() as i32;
    let size = cx.text.measure_width_in(script, &row.size) as i32;

    let px = fitted(cx, body, &row.when, box_.w - size - gap * 2);
    cx.text.set_px(px);
    cx.text.draw_in(script, cx.fb, box_.x, y, &row.when, false);
    cx.text.set_px(body);
    cx.text
        .draw_in(script, cx.fb, box_.right() - size, y, &row.size, false);

    cx.text.set_px(small);
    let under = y + gap + cx.text.line_height() as i32;
    let px = fitted(cx, small, &row.holds, box_.w);
    cx.text.set_px(px);
    cx.text
        .draw_in(script, cx.fb, box_.x, under, &row.holds, false);
}

/// The pager, where a page is left, and the way out at the right end.
fn foot(cx: &mut Ctx, box_: Rect, from: usize, fits: usize, held: usize) {
    let theme = cx.theme;
    let script = cx.ui_script();
    let chip = chrome::chip_height(theme);
    let feet = Rect::new(box_.x, box_.bottom() - chip, box_.w, chip);
    let close = cx.s().close;

    cx.text.set_px(theme.body_px);
    let said = cx.text.measure_width_in(script, close) as i32;
    let w = said + chrome::chip_pad(theme) * 4;
    let out = Rect::new(feet.right() - w, feet.y, w, chip);
    paint::stroke(cx.fb, out, INK, theme.rule());
    let lift = cx.text.cap_height() as i32 / 2;
    cx.text.draw_in(
        script,
        cx.fb,
        out.x + (out.w - said) / 2,
        out.center_y() + lift,
        close,
        false,
    );
    cx.hit(Hit::BackupsClose, out);

    if held <= fits {
        return;
    }
    let page = from / fits + 1;
    let pages = held.div_ceil(fits);
    let steps: [(&str, Option<usize>); 3] = [
        ("‹", (from > 0).then(|| from.saturating_sub(fits))),
        (&format!("{page} / {pages}"), None),
        ("›", (from + fits < held).then_some(from + fits)),
    ];
    let cells = Rect::new(feet.x, feet.y, feet.w - w - theme.gap * 2, chip).columns(3, theme.gap);
    for ((said, to), cell) in steps.iter().zip(&cells) {
        let at = cx.text.measure_width_in(script, said) as i32;
        cx.text.draw_in(
            script,
            cx.fb,
            cell.x + (cell.w - at) / 2,
            cell.center_y() + lift,
            said,
            false,
        );
        if let Some(to) = to {
            cx.hit(Hit::BackupsPage(*to), *cell);
        }
    }
}
