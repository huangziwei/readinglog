//! Four shapes: a month grid, a row of columns, a stack of labelled bars, and
//! one day's sittings laid along the clock.
//!
//! [`month_cells`] states where a cell lands, apart from any draw.

use crate::date;
use crate::eink::fb::Framebuffer;

use super::paint::{self, DARK, INK, LIGHT, MID, PALE, Rect, WHITE};
use super::text::TextRenderer;
use super::theme::Theme;

/// One day of a month grid: which day, and the box it occupies.
///
/// Monday first, and always six rows of seven.
pub fn month_cells(area: Rect, year: i64, month: i64, gap: i32) -> Vec<(i64, Rect)> {
    let first = date::days_from_civil(year, month, 1);
    let lead = date::weekday(first) as i64;
    let cols = area.columns(7, gap);
    let rows = area.rows(6, gap);
    let mut out = Vec::new();
    for d in 1..=date::days_in_month(year, month) {
        let slot = lead + d - 1;
        let (col, row) = ((slot % 7) as usize, (slot / 7) as usize);
        if let (Some(c), Some(r)) = (cols.get(col), rows.get(row)) {
            out.push((first + d - 1, Rect::new(c.x, r.y, c.w, r.h)));
        }
    }
    out
}

/// A month, each day inked by how long was read on it. Answers with the hit box
/// of every day drawn.
///
/// `duration_tight` sits inside the cell beside the day number.
#[allow(clippy::too_many_arguments)]
pub fn month(
    fb: &mut Framebuffer,
    text: &mut TextRenderer,
    theme: &Theme,
    area: Rect,
    year: i64,
    month: i64,
    seconds_of: impl Fn(i64) -> i64,
    today: i64,
    selected: Option<i64>,
) -> Vec<(i64, Rect)> {
    let head_h = theme.small_px as i32 * 2;
    let (head, grid) = area.split_top(head_h);
    text.set_px(theme.small_px);
    for (name, cell) in date::WEEKDAYS_SHORT.iter().zip(head.columns(7, theme.gap)) {
        let w = text.measure_width(name) as i32;
        text.draw(
            fb,
            cell.x + (cell.w - w) / 2,
            head.y + text.line_height() as i32,
            name,
            false,
        );
    }

    let cells = month_cells(grid, year, month, theme.gap);
    let busiest = cells
        .iter()
        .map(|(day, _)| seconds_of(*day))
        .max()
        .unwrap_or(0);

    for (day, cell) in &cells {
        let secs = seconds_of(*day);
        let ink = paint::ink_step(secs, busiest);
        match paint::ink_step_rgb(secs, busiest) {
            Some(rgb) => paint::fill_rgb(fb, *cell, rgb),
            None => {
                paint::fill(fb, *cell, WHITE);
                paint::stroke(fb, *cell, PALE, 1);
            }
        }
        // `inverted` on the two darkest steps.
        let inverted = matches!(ink, Some(DARK) | Some(INK));
        if *day == today {
            paint::stroke(fb, *cell, if inverted { WHITE } else { INK }, 2);
        }
        if Some(*day) == selected {
            paint::stroke(fb, cell.inset(2), if inverted { WHITE } else { INK }, 3);
        }

        let (_, _, dom) = date::civil_from_days(*day);
        text.set_px(theme.small_px);
        let pad = theme.gap / 2 + 2;
        text.draw(
            fb,
            cell.x + pad,
            cell.y + pad + text.cap_height() as i32,
            &dom.to_string(),
            inverted,
        );
        if secs > 0 {
            let label = date::duration_tight(secs);
            let w = text.measure_width(&label) as i32;
            text.draw(
                fb,
                cell.right() - pad - w,
                cell.bottom() - pad,
                &label,
                inverted,
            );
        }
    }
    cells
}

/// A row of columns, one per entry in `values`.
///
/// `every` labels one bucket in that many.
#[allow(clippy::too_many_arguments)]
pub fn columns(
    fb: &mut Framebuffer,
    text: &mut TextRenderer,
    theme: &Theme,
    area: Rect,
    values: &[i64],
    label: impl Fn(usize) -> String,
    every: usize,
    highlight: Option<usize>,
) {
    if values.is_empty() {
        return;
    }
    text.set_px(theme.small_px);
    let label_h = text.line_height() as i32 + theme.gap / 2;
    let (plot, axis) = area.split_top(area.h - label_h);
    let max = values.iter().copied().max().unwrap_or(0).max(1);
    let gap = if values.len() > 16 { 2 } else { theme.gap / 2 };
    let cells = plot.columns(values.len() as i32, gap);

    paint::hline(fb, plot.x, plot.bottom(), plot.w, LIGHT, 1);
    for (i, (value, cell)) in values.iter().zip(&cells).enumerate() {
        let h = (plot.h as i64 * value / max) as i32;
        if h > 0 {
            let ink = if highlight == Some(i) { INK } else { MID };
            // Three quarters of the cell, and no wider than `theme.row_h`.
            let w = (cell.w * 3 / 4).min(theme.row_h).max(1);
            let x = cell.x + (cell.w - w) / 2;
            paint::fill(fb, Rect::new(x, cell.bottom() - h, w, h), ink);
        }
        if i % every.max(1) == 0 {
            let name = label(i);
            let w = text.measure_width(&name) as i32;
            text.draw(
                fb,
                cell.x + (cell.w - w) / 2,
                axis.y + text.line_height() as i32,
                &name,
                false,
            );
        }
    }
}

/// A stack of named bars with the figure at the right.
pub fn bars(
    fb: &mut Framebuffer,
    text: &mut TextRenderer,
    theme: &Theme,
    area: Rect,
    rows: &[(String, i64)],
) {
    if rows.is_empty() {
        return;
    }
    let max = rows.iter().map(|(_, v)| *v).max().unwrap_or(0).max(1);
    let each = (area.h / rows.len() as i32).min(theme.row_h);
    text.set_px(theme.body_px);
    for (i, (name, value)) in rows.iter().enumerate() {
        let row = Rect::new(area.x, area.y + i as i32 * each, area.w, each);
        let figure = date::duration(*value);
        let fw = text.measure_width(&figure) as i32;
        let baseline = row.center_y() + text.cap_height() as i32 / 2;

        // The bar draws behind `name`.
        let track = Rect::new(
            row.x,
            row.y + theme.gap / 2,
            row.w - fw - theme.gap * 2,
            each - theme.gap,
        );
        let filled = (track.w as i64 * value / max) as i32;
        paint::fill(fb, Rect::new(track.x, track.y, filled, track.h), PALE);
        let clipped = text.wrap_and_clamp(name, (track.w - theme.gap) as u32, 1);
        text.draw(
            fb,
            row.x + theme.gap / 2,
            baseline,
            clipped.first().map(String::as_str).unwrap_or(name),
            false,
        );
        text.draw(fb, row.right() - fw, baseline, &figure, false);
    }
}

/// One day laid along the clock, each of `spans` a block where it happened.
///
/// The strip covers all 24 hours whatever `spans` holds.
pub fn timeline(
    fb: &mut Framebuffer,
    text: &mut TextRenderer,
    theme: &Theme,
    area: Rect,
    spans: &[(i64, i64)],
    now: Option<i64>,
) {
    text.set_px(theme.small_px);
    let label_h = text.line_height() as i32 + theme.gap / 2;
    let (strip, axis) = area.split_top(area.h - label_h);
    paint::stroke(fb, strip, LIGHT, 1);

    let at = |secs: i64| strip.x + (strip.w as i64 * secs.clamp(0, 86_400) / 86_400) as i32;
    for hour in (0..=24).step_by(6) {
        let x = at(hour as i64 * 3600);
        paint::vline(fb, x, strip.y, strip.h, PALE, 1);
        let name = format!("{hour:02}");
        let w = text.measure_width(&name) as i32;
        text.draw(
            fb,
            (x - w / 2).clamp(area.x, area.right() - w),
            axis.y + text.line_height() as i32,
            &name,
            false,
        );
    }
    for (from, to) in spans {
        let (x0, x1) = (at(*from), at(*to));
        // A span under 3 px wide draws 3 px.
        let w = (x1 - x0).max(3);
        paint::fill(fb, Rect::new(x0, strip.y + 1, w, strip.h - 2), INK);
    }
    // `now` down the strip.
    if let Some(secs) = now {
        let x = at(secs);
        paint::vline(fb, x, strip.y - theme.gap / 2, strip.h + theme.gap, DARK, 2);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_month_starts_on_the_weekday_it_really_starts_on() {
        // 1 August 2026 is a Saturday, the sixth column.
        let area = Rect::new(0, 0, 700, 600);
        let cells = month_cells(area, 2026, 8, 0);
        assert_eq!(cells.len(), 31);
        let (first, rect) = cells[0];
        assert_eq!(date::weekday(first), 5);
        assert_eq!(rect.x, 5 * 100);
        assert_eq!(rect.y, 0);
    }

    #[test]
    fn the_days_of_a_month_run_in_order_across_and_down() {
        let cells = month_cells(Rect::new(0, 0, 700, 600), 2026, 8, 0);
        for pair in cells.windows(2) {
            assert_eq!(pair[1].0, pair[0].0 + 1);
        }
        // The 2nd is a Sunday, ending the first row.
        assert_eq!(cells[1].1.x, 6 * 100);
        // The 3rd is a Monday, opening the second.
        assert_eq!(cells[2].1.x, 0);
        assert_eq!(cells[2].1.y, 100);
    }

    #[test]
    fn every_cell_of_every_month_stays_inside_the_grid() {
        let area = Rect::new(10, 20, 700, 600);
        for year in [2024, 2026] {
            for month in 1..=12 {
                for (_, cell) in month_cells(area, year, month, 4) {
                    assert!(cell.x >= area.x, "{year}-{month}");
                    assert!(cell.right() <= area.right(), "{year}-{month}");
                    assert!(cell.y >= area.y, "{year}-{month}");
                    assert!(cell.bottom() <= area.bottom(), "{year}-{month}");
                }
            }
        }
    }

    #[test]
    fn a_leap_february_gets_its_extra_day() {
        assert_eq!(month_cells(Rect::new(0, 0, 700, 600), 2024, 2, 0).len(), 29);
        assert_eq!(month_cells(Rect::new(0, 0, 700, 600), 2026, 2, 0).len(), 28);
    }

    #[test]
    fn no_two_days_of_a_month_share_a_box() {
        let cells = month_cells(Rect::new(0, 0, 700, 600), 2026, 8, 2);
        for (i, (_, a)) in cells.iter().enumerate() {
            for (_, b) in &cells[i + 1..] {
                assert!(a != b, "{a:?}");
            }
        }
    }
}
