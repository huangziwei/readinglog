//! A month at a time, each day inked by how long was read on it, with the
//! day's own books listed under the grid.

use crate::date;
use crate::font;
use crate::ui::chrome;
use crate::ui::paint::Rect;
use crate::ui::{charts, theme::Theme};

use super::{Ctx, Hit, State};

pub fn draw(cx: &mut Ctx, area: Rect, state: &State) {
    let theme: &Theme = cx.theme;
    let (year, month) = state.month;

    let (nav, rest) = area.split_top(theme.row_h);
    month_nav(cx, nav, year, month);

    // The grid takes 58% of `rest`, less `theme.gap * 2` of air.
    let (grid, below) = rest.split_top(rest.h * 58 / 100);
    let grid = Rect::new(grid.x, grid.y, grid.w, (grid.h - theme.gap * 2).max(1));
    let days = charts::month(
        cx.fb,
        cx.text,
        cx.theme,
        grid,
        year,
        month,
        |day| cx.stats.day_seconds(day),
        cx.today,
        state.day,
    );
    for (day, cell) in days {
        cx.hit(Hit::Day(day), cell);
    }

    match state.day {
        Some(day) => day_books(cx, below, day),
        None => month_books(cx, below, year, month),
    }
}

/// The month's name between two arrows, each its own hit box.
fn month_nav(cx: &mut Ctx, area: Rect, year: i64, month: i64) {
    let theme: &Theme = cx.theme;
    let name = format!("{} {year}", date::MONTHS[(month - 1).clamp(0, 11) as usize]);
    cx.text.set_px(theme.head_px);
    let baseline = area.center_y() + cx.text.cap_height() as i32 / 2;
    let w = cx.text.measure_width(&name) as i32;
    cx.text
        .draw(cx.fb, area.x + (area.w - w) / 2, baseline, &name, false);

    // The hit box is `area.w / 6`, past the arrow's own width.
    let arrow_w = area.w / 6;
    cx.text.draw(cx.fb, area.x, baseline, "‹", false);
    cx.hit(Hit::Prev, Rect::new(area.x, area.y, arrow_w, area.h));
    let next_w = cx.text.measure_width("›") as i32;
    cx.text
        .draw(cx.fb, area.right() - next_w, baseline, "›", false);
    cx.hit(
        Hit::Next,
        Rect::new(area.right() - arrow_w, area.y, arrow_w, area.h),
    );
}

/// What was read on one day, longest first, each row opening its book.
fn day_books(cx: &mut Ctx, area: Rect, day: i64) {
    let theme: &Theme = cx.theme;
    let title = format!(
        "{} — {}",
        date::long_day(day).to_uppercase(),
        date::duration(cx.stats.day_seconds(day))
    );
    let inner = chrome::section(cx.fb, cx.text, theme, area, &title);

    let mut totals: Vec<(usize, i64)> = Vec::new();
    for s in cx.stats.sittings_on(day) {
        let Some(book) = s.book else { continue };
        match totals.iter_mut().find(|(b, _)| *b == book) {
            Some((_, secs)) => *secs += s.seconds,
            None => totals.push((book, s.seconds)),
        }
    }
    totals.sort_by_key(|(_, secs)| -secs);
    list(cx, inner, &totals);
}

/// The same, over a whole month, where `State::day` is `None`.
fn month_books(cx: &mut Ctx, area: Rect, year: i64, month: i64) {
    let theme: &Theme = cx.theme;
    let first = date::days_from_civil(year, month, 1);
    let last = first + date::days_in_month(year, month) - 1;
    let total: i64 = (first..=last).map(|d| cx.stats.day_seconds(d)).sum();
    let title = format!("THE MONTH — {}", date::duration(total));
    let inner = chrome::section(cx.fb, cx.text, theme, area, &title);

    let mut totals: Vec<(usize, i64)> = Vec::new();
    for s in cx
        .stats
        .sittings
        .iter()
        .filter(|s| (first..=last).contains(&s.day))
    {
        let Some(book) = s.book else { continue };
        match totals.iter_mut().find(|(b, _)| *b == book) {
            Some((_, secs)) => *secs += s.seconds,
            None => totals.push((book, s.seconds)),
        }
    }
    totals.sort_by_key(|(_, secs)| -secs);
    list(cx, inner, &totals);
}

/// Book bars, each row a hit box onto that book's own screen.
fn list(cx: &mut Ctx, area: Rect, totals: &[(usize, i64)]) {
    let theme: &Theme = cx.theme;
    if totals.is_empty() {
        cx.text.set_px(theme.body_px);
        let baseline = area.y + cx.text.line_height() as i32;
        cx.text
            .draw(cx.fb, area.x, baseline, "Nothing read.", false);
        return;
    }
    let rows: Vec<(font::Script, String, i64)> = totals
        .iter()
        .map(|(b, secs)| {
            let book = &cx.stats.books[*b];
            (
                font::Script::of_language(&book.language),
                book.title.clone(),
                *secs,
            )
        })
        .collect();
    let shown = rows.len().min((area.h / theme.row_h).max(1) as usize);
    charts::bars(cx.fb, cx.text, theme, area, &rows[..shown]);

    let each = (area.h / shown.max(1) as i32).min(theme.row_h);
    for (i, (book, _)) in totals.iter().take(shown).enumerate() {
        cx.hit(
            Hit::Book(*book),
            Rect::new(area.x, area.y + i as i32 * each, area.w, each),
        );
    }
}
