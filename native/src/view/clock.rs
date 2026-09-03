//! When reading happens: the same total cut by hour of day, by weekday and by
//! month.
//!
//! `cx.stats.hours` totals a sitting per clock hour of its own day.

use crate::date;
use crate::lang::Strings;
use crate::settings::WeekStart;

/// A bucket's name, in the interface's own words.
type Naming = fn(usize, &Strings) -> String;
use crate::ui::chrome;
use crate::ui::paint::{self, INK, LIGHT, Rect};
use crate::ui::{charts, theme::Theme};

use super::{Ctx, Cut, Hit, State};

pub fn draw(cx: &mut Ctx, area: Rect, state: &State) {
    let theme: &Theme = cx.theme;
    let (bar, rest) = area.split_top(theme.row_h);
    picker(cx, bar, state.cut);

    let (values, label, every): (Vec<i64>, Naming, usize) = match state.cut {
        Cut::Hour => (cx.stats.hours.to_vec(), hour_label, 3),
        // `stats.weekdays` counts from Monday; the bars are drawn from
        // whichever day the week is set to start on.
        Cut::Weekday => (
            (0..7)
                .map(|column| cx.stats.weekdays[cx.week.day_in(column)])
                .collect(),
            weekday_label,
            1,
        ),
        Cut::Month => (cx.stats.months.to_vec(), month_label, 1),
    };
    let s = cx.s();
    let week = cx.week;
    // A weekday index is a column here, not a day: the two differ wherever
    // the week does not start on Monday.
    let named = |i: usize, s: &Strings| match state.cut {
        Cut::Weekday => weekday_label_from(i, s, week),
        _ => label(i, s),
    };
    let busiest = values
        .iter()
        .enumerate()
        .max_by_key(|(_, v)| **v)
        .filter(|(_, v)| **v > 0)
        .map(|(i, _)| i);

    // `facts_h` takes four rows and a heading; `plot` takes the rest, less
    // `air` either side.
    let air = theme.gap * 2;
    let facts_h = chrome::section_height(cx.text, theme) + theme.row_h * 4;
    let (below, plot) = rest.split_bottom(facts_h);
    let plot = Rect::new(plot.x, plot.y + air, plot.w, (plot.h - air * 2).max(1));
    charts::columns(
        cx.fb,
        cx.text,
        theme,
        plot,
        &values,
        |i| named(i, s),
        every,
        busiest,
    );

    let inner = chrome::section(cx.fb, cx.text, theme, below, s.shape_of_it);
    let total: i64 = values.iter().sum();
    let rows = inner.rows(4, 0);
    let lines = [
        (s.busiest, busiest.map_or("—".into(), |i| named(i, s))),
        (s.then, second(&values).map_or("—".into(), |i| named(i, s))),
        (
            s.in_the_busiest,
            busiest.map_or("—".into(), |i| share(values[i], total)),
        ),
        (s.counted_over, date::duration(total, s)),
    ];
    for ((key, value), row) in lines.iter().zip(&rows) {
        chrome::row(cx.fb, cx.text, theme, *row, key, value);
    }
}

/// The three cuts as a segmented control, each its own hit box.
fn picker(cx: &mut Ctx, area: Rect, active: Cut) {
    let theme: &Theme = cx.theme;
    // `inner` insets vertically; the sides keep `area`'s margin.
    let inner = Rect::new(
        area.x,
        area.y + theme.gap / 2,
        area.w,
        (area.h - theme.gap).max(1),
    );
    let cells = inner.columns(Cut::ALL.len() as i32, 0);
    cx.text.set_px(theme.body_px);
    let baseline = area.center_y() + cx.text.cap_height() as i32 / 2;
    for (cut, cell) in Cut::ALL.iter().zip(cells) {
        let on = *cut == active;
        if on {
            paint::fill(cx.fb, cell, INK);
        } else {
            paint::stroke(cx.fb, cell, LIGHT, 1);
        }
        let label = cut.label(cx.lang);
        let w = cx.text.measure_width(label) as i32;
        cx.text
            .draw(cx.fb, cell.x + (cell.w - w) / 2, baseline, label, on);
        cx.hit(Hit::Cut(*cut), cell);
    }
}

/// The runner-up bucket.
fn second(values: &[i64]) -> Option<usize> {
    let mut order: Vec<usize> = (0..values.len()).filter(|i| values[*i] > 0).collect();
    order.sort_by_key(|i| -values[*i]);
    order.get(1).copied()
}

fn share(part: i64, total: i64) -> String {
    match total {
        0 => "—".into(),
        t => format!("{}%", part * 100 / t),
    }
}

fn hour_label(i: usize, _s: &Strings) -> String {
    format!("{i:02}")
}

fn weekday_label(i: usize, s: &Strings) -> String {
    s.weekdays_short[i.min(6)].to_string()
}

/// The same, for a week that does not start on Monday.
fn weekday_label_from(column: usize, s: &Strings, week: WeekStart) -> String {
    s.weekdays_short[week.day_in(column.min(6))].to_string()
}

fn month_label(i: usize, s: &Strings) -> String {
    s.months_short[i.min(11)].to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_runner_up_is_the_second_busiest_and_never_an_empty_one() {
        assert_eq!(second(&[5, 9, 1, 0]), Some(0));
        assert_eq!(second(&[0, 9, 0, 0]), None, "only one bucket has anything");
        assert_eq!(second(&[]), None);
    }

    #[test]
    fn a_share_of_nothing_is_not_a_division() {
        assert_eq!(share(0, 0), "—");
        assert_eq!(share(25, 100), "25%");
    }
}
