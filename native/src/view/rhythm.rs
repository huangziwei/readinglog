//! The calendar, at whichever zoom the picker is set to.
//!
//! A week is seven columns of hours, a month a grid of weeks naming the books
//! read on each day, a year one column a week. Under a week and a year stands
//! what was read over it, which a day picked off the grid narrows to that day.

use crate::date;
use crate::font::Script;
use crate::settings::WeekStart;
use crate::ui::paint::{self, BAR_RGB, INK, LIGHT, MARK_RGB, PALE, Rect, WHITE};
use crate::ui::{charts, chrome, theme::Theme};

use super::{Ctx, Hit, Span, State, daybooks};

/// Books a day of a month names at the most, before the rest are counted in
/// `+n`. A cell with room for fewer draws fewer.
const LANES: usize = 4;

/// Rows of `theme.row_h` the average day takes under its heading.
const HOURS_ROWS: i32 = 2;

/// The bar the picker and the span's name each stand in.
fn bar_height(theme: &Theme) -> i32 {
    theme.row_h * 3 / 4
}

/// The height the average day takes, its heading included.
fn hours_height(theme: &Theme, head: i32) -> i32 {
    head + theme.row_h * HOURS_ROWS
}

/// The five bands of the page, top to bottom: the picker, the span's name
/// between its arrows, the grid, the average day, and the books.
///
/// `grid` is what the span asks for. Where it asks for the page there is no
/// list, and the grid keeps everything the average day leaves.
fn bands(area: Rect, theme: &Theme, head: i32, grid: i32, listed: bool) -> [Rect; 5] {
    let air = theme.gap * 2;
    let bar = bar_height(theme);
    let (picker, rest) = area.split_top(bar + air);
    let (nav, rest) = rest.split_top(bar + air);

    let hours = hours_height(theme, head).min(rest.h);
    let body = (rest.h - hours - air).max(0);
    let under = rest.y + body + air;
    let grid = match listed {
        true => grid.clamp(0, body),
        false => body,
    };
    // `list` stands between the grid and the average day, which takes the foot.
    let list = (body - grid - air).max(0) * listed as i32;
    [
        Rect::new(picker.x, picker.y, picker.w, bar),
        Rect::new(nav.x, nav.y, nav.w, bar),
        Rect::new(rest.x, rest.y, rest.w, grid),
        Rect::new(rest.x, under, rest.w, hours.min(rest.bottom() - under)),
        Rect::new(rest.x, rest.y + grid + air, rest.w, list),
    ]
}

/// Whether a span states its books under the grid. A month names them in the
/// cells themselves.
fn lists_books(span: Span) -> bool {
    !matches!(span, Span::Month)
}

/// The height the grid for one span asks for.
fn grid_height(span: Span, area: Rect, theme: &Theme, day: i64, week: WeekStart) -> i32 {
    match span {
        Span::Month => area.h,
        Span::Week => week_height(theme),
        Span::Year => {
            let (year, _, _) = date::civil_from_days(day);
            let box_ = Rect::new(area.x, area.y, area.w - gutter(theme), area.h);
            year_map(box_, year, theme, week).height
        }
    }
}

/// The bands a year's weeks are cut into, stacked down the page. Two of them
/// double the cell, which is what makes a day of it tappable.
const YEAR_BLOCKS: i32 = 2;

/// The column the weekday names stand in beside a year.
fn gutter(theme: &Theme) -> i32 {
    theme.small_px as i32 * 3
}

pub fn draw(cx: &mut Ctx, area: Rect, state: &State) {
    let theme: &Theme = cx.theme;
    // A month has no room to state a day under its grid, and opens one whole.
    if state.picked && !lists_books(state.span) {
        let (_, rest) = area.split_top(bar_height(theme) + theme.gap * 2);
        picker(
            cx,
            Rect::new(area.x, area.y, area.w, bar_height(theme)),
            state,
        );
        day_page(cx, rest, state.day);
        return;
    }
    span_page(cx, area, state);
}

/// The span holding `state.day`: its name, its grid, its average day, and the
/// books read over it.
fn span_page(cx: &mut Ctx, area: Rect, state: &State) {
    let theme: &Theme = cx.theme;
    let s = cx.s();
    let (span, day) = (state.span, state.day);
    let days = span.days(day, cx.week);

    let head = chrome::section_height(cx.text, theme);
    let listed = lists_books(span);
    let want = grid_height(span, area, theme, day, cx.week);
    let [bar, nav, grid, hours, list] = bands(area, theme, head, want, listed);

    picker(cx, bar, state);
    let name = span.name(day, cx.week, s);
    let total = date::duration(cx.stats.span_seconds(days.clone()), s);
    span_nav(cx, nav, &format!("{name} · {total}"));

    match span {
        Span::Week => week_columns(cx, grid, day, state.picked),
        Span::Month => month_grid(cx, grid, day),
        Span::Year => year_heatmap(cx, grid, day, state.picked),
    }
    average_of(cx, hours, state, days.clone());
    if listed {
        book_list(cx, list, state, days);
    }
}

/// One day in full: its name between the arrows that step it, its timeline,
/// and every book read on it.
fn day_page(cx: &mut Ctx, area: Rect, day: i64) {
    let theme: &Theme = cx.theme;
    let s = cx.s();
    let air = theme.gap * 2;
    let title = format!(
        "{} · {}",
        date::long_day(day, s),
        date::duration(cx.stats.day_seconds(day), s)
    );
    let (nav, rest) = area.split_top(bar_height(theme) + air);
    span_nav(
        cx,
        Rect::new(nav.x, nav.y, nav.w, bar_height(theme)),
        &title,
    );

    let (strip, list) = rest.split_top(theme.row_h + air);
    let spans = cx.stats.day_blocks(day);
    let now = (day == cx.today).then_some(cx.now);
    charts::timeline(
        cx.fb,
        cx.text,
        theme,
        Rect::new(strip.x, strip.y, strip.w, theme.row_h),
        &spans,
        now,
    );

    let read = cx.stats.book_totals(day..=day);
    let shown = daybooks::fits(theme, list.h, read.len());
    daybooks::draw(cx, list, day, &read[..shown]);
}

/// The three spans as a segmented control, each its own hit box. A day open
/// over the calendar lights none of them, and a tap on one closes it.
fn picker(cx: &mut Ctx, area: Rect, state: &State) {
    let theme: &Theme = cx.theme;
    let cells = area.columns(Span::ALL.len() as i32, 0);
    cx.text.set_px(theme.body_px);
    let baseline = area.center_y() + cx.text.cap_height() as i32 / 2;
    let script = cx.ui_script();
    let lit = state.span;
    let dark = state.picked && !lists_books(state.span);
    for (span, cell) in Span::ALL.iter().zip(cells) {
        let on = *span == lit && !dark;
        match on {
            true => paint::fill(cx.fb, cell, INK),
            false => paint::stroke(cx.fb, cell, LIGHT, 1),
        }
        let label = span.label(cx.lang);
        let w = cx.text.measure_width_in(script, label) as i32;
        cx.text.draw_in(
            script,
            cx.fb,
            cell.x + (cell.w - w) / 2,
            baseline,
            label,
            on,
        );
        cx.hit(Hit::Span(*span), cell);
    }
}

/// `title` between two arrows, each its own hit box, over a rule.
fn span_nav(cx: &mut Ctx, area: Rect, title: &str) {
    let theme: &Theme = cx.theme;
    cx.text.set_px(theme.head_px);
    let baseline = area.center_y() + cx.text.cap_height() as i32 / 2;
    let script = cx.ui_script();
    let w = cx.text.measure_width_in(script, title) as i32;
    cx.text.draw_in(
        script,
        cx.fb,
        area.x + (area.w - w) / 2,
        baseline,
        title,
        false,
    );
    paint::hline(cx.fb, area.x, area.bottom(), area.w, PALE, 1);

    // The hit box is `area.w / 6`, past the arrow's own width.
    let reach = area.w / 6;
    cx.text.draw(cx.fb, area.x, baseline, "‹", false);
    cx.hit(Hit::Prev, Rect::new(area.x, area.y, reach, area.h));
    let next = cx.text.measure_width("›") as i32;
    cx.text
        .draw(cx.fb, area.right() - next, baseline, "›", false);
    cx.hit(
        Hit::Next,
        Rect::new(area.right() - reach, area.y, reach, area.h),
    );
}

/// Rows of `theme.row_h` a week gives the day totals and the hours under them.
const WEEK_BARS: (i32, i32) = (3, 2);
const WEEK_HOURS: (i32, i32) = (2, 3);

/// The three rows a week's grid stacks: the dates, one bar to a day, and each
/// day's own hours under its bar.
fn week_rows(area: Rect, theme: &Theme) -> [Rect; 3] {
    let head = theme.small_px as i32 * 2;
    let bars = theme.row_h * WEEK_BARS.0 / WEEK_BARS.1;
    let (head, rest) = area.split_top(head);
    let (bars, hours) = rest.split_top(bars);
    [head, bars, hours]
}

/// The height [`week_rows`] asks for.
fn week_height(theme: &Theme) -> i32 {
    theme.small_px as i32 * 2
        + theme.row_h * WEEK_BARS.0 / WEEK_BARS.1
        + theme.row_h * WEEK_HOURS.0 / WEEK_HOURS.1
}

/// The week holding `day`: its dates, how long was read on each, and the
/// hours that reading fell in.
fn week_columns(cx: &mut Ctx, area: Rect, day: i64, picked: bool) {
    let theme: &Theme = cx.theme;
    let [head, bars, hours] = week_rows(area, theme);
    let first = day - cx.week.column_of(date::weekday(day)) as i64;
    let most = peak_of(cx, first..=first + 6);
    let tallest = hour_peak(cx, first..=first + 6);

    let names = charts::week_cells(head, first, theme.gap);
    let plots = charts::week_cells(bars, first, theme.gap);
    let clocks = charts::week_cells(hours, first, theme.gap);
    for ((at, name), (_, plot), (_, clock)) in names
        .into_iter()
        .zip(plots)
        .zip(clocks)
        .map(|((a, b), c)| (a, b, c))
    {
        let on = picked && at == day;
        let column = Rect::new(name.x, name.y, name.w, clock.bottom() - name.y);
        if on {
            paint::fill(cx.fb, column, PALE);
        }
        day_head(cx, name, at);
        day_bar(cx, plot, at, most, on);
        // The hours stand on a ground of their own, which reads as a strip
        // of the day.
        let counted = cx.stats.hours_over(at..=at);
        let ground = clock.inset(theme.gap / 2);
        paint::fill(cx.fb, ground, PALE);
        charts::hour_shape(cx.fb, ground, &counted, tallest);
        cx.hit(Hit::Day(at), column);
    }
}

/// A day's weekday and date, centred over its column.
fn day_head(cx: &mut Ctx, area: Rect, day: i64) {
    let theme: &Theme = cx.theme;
    let (_, _, dom) = date::civil_from_days(day);
    let name = format!("{} {dom}", cx.s().weekdays_short[date::weekday(day)]);
    let script = cx.ui_script();
    cx.text.set_px(theme.small_px);
    let w = cx.text.measure_width_in(script, &name) as i32;
    let baseline = area.y + cx.text.line_height() as i32;
    let today = day == cx.today;
    cx.text.draw_in(
        script,
        cx.fb,
        area.x + (area.w - w) / 2,
        baseline,
        &name,
        false,
    );
    if today {
        paint::hline(cx.fb, area.x, area.bottom() - 2, area.w, INK, 2);
    }
}

/// One day's total, as a bar up its own column with the figure over it.
fn day_bar(cx: &mut Ctx, area: Rect, day: i64, most: i64, on: bool) {
    let theme: &Theme = cx.theme;
    let secs = cx.stats.day_seconds(day);
    paint::hline(cx.fb, area.x, area.bottom(), area.w, LIGHT, 1);
    if secs <= 0 {
        return;
    }
    cx.text.set_px(theme.small_px);
    let label = date::duration_tight(secs, cx.s());
    let lw = cx.text.measure_width(&label) as i32;
    let label_h = cx.text.line_height() as i32;

    // The bar takes what the figure over it leaves.
    let room = (area.h - label_h).max(1);
    let h = ((room as i64 * secs / most.max(1)) as i32).max(2);
    let w = (area.w * 3 / 4).max(2);
    let x = area.x + (area.w - w) / 2;
    let bar = Rect::new(x, area.bottom() - h, w, h);
    paint::fill_rgb(cx.fb, bar, if on { MARK_RGB } else { BAR_RGB });
    cx.text.draw(
        cx.fb,
        area.x + (area.w - lw) / 2,
        bar.y - theme.gap / 3,
        &label,
        false,
    );
}

/// The month holding `day`, in weeks of seven, each day naming its own books.
fn month_grid(cx: &mut Ctx, area: Rect, day: i64) {
    let theme: &Theme = cx.theme;
    let (year, month, _) = date::civil_from_days(day);
    let (head, grid) = area.split_top(charts::weekday_head_height(theme));
    charts::weekday_head(cx.fb, cx.text, theme, cx.s(), head, cx.week);

    let first = date::days_from_civil(year, month, 1);
    let last = first + date::days_in_month(year, month) - 1;
    let peak = peak_of(cx, first..=last);
    let tallest = hour_peak(cx, first..=last);
    let cells = charts::month_cells(grid, year, month, theme.gap, cx.week);

    // A week of the grid shares its lanes, so a book read two days running
    // draws one bar across them.
    for row in cells.chunk_by(|a, b| a.1.y == b.1.y) {
        let days: Vec<Vec<usize>> = row.iter().map(|(day, _)| books_on(cx, *day)).collect();
        let depth = lane_count(theme, row[0].1.inset(theme.gap / 2));
        let lanes = charts::lanes(&days, depth);
        for (day, cell) in row.iter() {
            let (_, _, dom) = date::civil_from_days(*day);
            head_line(cx, *cell, *day, &dom.to_string(), peak);
            let inner = cell.inset(theme.gap / 2);
            let hours = cx.stats.hours_over(*day..=*day);
            charts::hour_shape(cx.fb, shape_box(theme, inner), &hours, tallest);
            cx.hit(Hit::Day(*day), *cell);
        }
        // A run reaches past its own cell, so every bar of the week is drawn
        // over the cells the whole week has laid down.
        for (column, (_, cell)) in row.iter().enumerate() {
            let inner = cell.inset(theme.gap / 2);
            let step = cell.w + theme.gap;
            for (lane, run) in lanes[column].iter().enumerate() {
                let Some(run) = run else { continue };
                if run.start != column {
                    continue;
                }
                let width = inner.w + step * (run.span as i32 - 1);
                book_bar(cx, lane_box(theme, inner, lane, width), run.book);
            }
            let named = lanes[column].iter().flatten().count();
            more_books(
                cx,
                inner,
                named.saturating_sub(1),
                days[column].len() - named,
            );
        }
    }
}

/// A year's blocks inside `area`, the month names over each one.
fn year_map(area: Rect, year: i64, theme: &Theme, week: WeekStart) -> charts::Heatmap {
    charts::heatmap(
        area,
        year,
        theme.gap / 2,
        week,
        YEAR_BLOCKS,
        theme.small_px as i32 * 2,
        theme.gap * 2,
    )
}

/// The year holding `day`, one column to a week.
fn year_heatmap(cx: &mut Ctx, area: Rect, day: i64, picked: bool) {
    let theme: &Theme = cx.theme;
    let (year, _, _) = date::civil_from_days(day);
    let (side, grid) = area.split_left(gutter(theme));
    let map = year_map(grid, year, theme, cx.week);

    cx.text.set_px(theme.small_px);
    for (month, box_) in &map.months {
        let name = cx.s().months_short[(*month - 1).clamp(0, 11) as usize];
        let baseline = box_.y + cx.text.line_height() as i32;
        cx.text.draw(cx.fb, box_.x, baseline, name, false);
    }
    // Monday, Wednesday and Friday are named; the rows between have no room.
    for block in 0..YEAR_BLOCKS as usize {
        for weekday in [0usize, 2, 4] {
            let Some(box_) = map.rows.get(block * 7 + cx.week.column_of(weekday)) else {
                continue;
            };
            let name = cx.s().weekdays_short[weekday];
            let w = cx.text.measure_width(name) as i32;
            let baseline = box_.center_y() + cx.text.cap_height() as i32 / 2;
            cx.text
                .draw(cx.fb, side.right() - w - theme.gap, baseline, name, false);
        }
    }

    let peak = map
        .cells
        .iter()
        .map(|(day, _)| cx.stats.day_seconds(*day))
        .max()
        .unwrap_or(0);
    for (at, cell) in &map.cells {
        let secs = cx.stats.day_seconds(*at);
        match charts::level_rgb(charts::level(secs, peak)) {
            Some(rgb) => paint::fill_rgb(cx.fb, *cell, rgb),
            None => {
                paint::fill(cx.fb, *cell, WHITE);
                paint::stroke(cx.fb, *cell, PALE, 1);
            }
        }
        if *at == cx.today {
            paint::stroke(cx.fb, *cell, INK, 2);
        }
        if picked && *at == day {
            paint::stroke(cx.fb, cell.inset(-2), INK, 2);
        }
        cx.hit(Hit::Day(*at), *cell);
    }
}

/// The date across the head of a day's cell, with the total against it.
///
/// A `peak` above zero draws a rule along the top edge at the day's own level.
fn head_line(cx: &mut Ctx, cell: Rect, day: i64, date: &str, peak: i64) {
    let theme: &Theme = cx.theme;
    let secs = cx.stats.day_seconds(day);
    paint::fill(cx.fb, cell, WHITE);
    paint::stroke(cx.fb, cell, if day == cx.today { INK } else { PALE }, 1);
    // A mark beside the date crowds a duration set in Japanese.
    if let Some(rgb) = charts::level_rgb(charts::level(secs, peak)) {
        let rule = (theme.gap / 3).max(2);
        paint::fill_rgb(cx.fb, Rect::new(cell.x, cell.y, cell.w, rule), rgb);
    }

    let inner = cell.inset(theme.gap / 2);
    let script = cx.ui_script();
    cx.text.set_px(theme.small_px);
    let baseline = inner.y + cx.text.cap_height() as i32;
    cx.text
        .draw_in(script, cx.fb, inner.x, baseline, date, false);
    if secs <= 0 {
        return;
    }
    let total = date::duration_tight(secs, cx.s());
    let w = cx.text.measure_width(&total) as i32;
    cx.text
        .draw(cx.fb, inner.right() - w, baseline, &total, false);
}

/// `over` more books than the lanes named, set against the right of the last
/// of them where the head line has no room for it.
fn more_books(cx: &mut Ctx, inner: Rect, lane: usize, over: usize) {
    if over == 0 {
        return;
    }
    let theme: &Theme = cx.theme;
    let bar = lane_box(theme, inner, lane, inner.w);
    let more = format!("+{over}");
    cx.text.set_px(theme.small_px);
    let w = cx.text.measure_width(&more) as i32;
    let baseline = bar.center_y() + cx.text.cap_height() as i32 / 2;
    paint::fill(
        cx.fb,
        Rect::new(bar.right() - w - theme.gap, bar.y, w + theme.gap, bar.h),
        WHITE,
    );
    cx.text.draw(cx.fb, bar.right() - w, baseline, &more, false);
}

/// What was read over `days`, longest first, each row a hit box onto its book.
///
/// `picked` narrows the whole list to one day of it. The list is a page deep
/// at the most, and the chips at the right of its heading step through the
/// rest: what fills the foot of the page is never what the page is about.
fn book_list(cx: &mut Ctx, area: Rect, state: &State, days: std::ops::RangeInclusive<i64>) {
    let theme: &Theme = cx.theme;
    let s = cx.s();
    let picked = state.picked.then_some(state.day);
    let (title, totals) = match picked {
        Some(day) => (
            format!(
                "{} — {}",
                date::long_day(day, s).to_uppercase(),
                date::duration(cx.stats.day_seconds(day), s)
            ),
            cx.stats.book_totals(day..=day),
        ),
        None => (
            format!(
                "{} — {}",
                s.what_was_read,
                date::duration(cx.stats.span_seconds(days.clone()), s)
            ),
            cx.stats.book_totals(days),
        ),
    };
    let head = Rect::new(
        area.x,
        area.y,
        area.w,
        chrome::section_height(cx.text, theme),
    );
    let inner = chrome::section(cx.fb, cx.text, theme, area, &title);
    if totals.is_empty() {
        cx.text.set_px(theme.body_px);
        let baseline = inner.y + cx.text.line_height() as i32;
        let script = cx.ui_script();
        cx.text
            .draw_in(script, cx.fb, inner.x, baseline, s.nothing_read, false);
        return;
    }

    // `from` holds at the last page of the list.
    let deep = ((inner.h / theme.row_h).max(1) as usize).min(totals.len());
    let last = totals.len().saturating_sub(deep);
    let from = state.list_from.min(last);
    let page = &totals[from..(from + deep).min(totals.len())];
    if totals.len() > deep {
        let of = format!(
            "{}–{} {} {}",
            from + 1,
            from + page.len(),
            s.of,
            totals.len()
        );
        heading_chips(
            cx,
            head,
            [
                ("‹", Hit::ListPage(-(deep as i64)), false),
                ("›", Hit::ListPage(deep as i64), false),
            ],
        );
        cx.text.set_px(theme.small_px);
        let w = cx.text.measure_width(&of) as i32;
        let baseline = head.y + cx.text.cap_height() as i32;
        let at = head.right() - w - theme.row_h * 3 / 2;
        cx.text.draw(cx.fb, at, baseline, &of, false);
    }

    let rows: Vec<(Script, String, i64)> = page
        .iter()
        .map(|(book, secs)| {
            let book = &cx.stats.books[*book];
            (
                Script::of_language(&book.language),
                book.title.clone(),
                *secs,
            )
        })
        .collect();
    charts::bars(cx.fb, cx.text, theme, s, inner, &rows);

    let each = (inner.h / page.len().max(1) as i32).min(theme.row_h);
    for (slot, (book, _)) in page.iter().enumerate() {
        let row = Rect::new(inner.x, inner.y + slot as i32 * each, inner.w, each);
        cx.hit(Hit::Book(*book), row);
    }
}

/// A pair of chips at the right of a heading, the one in use filled.
///
/// The controls belong to the section they head, and not to the page.
fn heading_chips(cx: &mut Ctx, head: Rect, of: [(&str, Hit, bool); 2]) {
    let theme: &Theme = cx.theme;
    let script = cx.ui_script();
    cx.text.set_px(theme.small_px);
    let pad = theme.gap;
    let widths: Vec<i32> = of
        .iter()
        .map(|(label, _, _)| cx.text.measure_width_in(script, label) as i32 + pad * 2)
        .collect();
    let h = (cx.text.line_height() as i32 + theme.gap / 2).min(head.h.max(1));
    let mut x = head.right() - widths.iter().sum::<i32>() - theme.gap;
    for ((label, hit, on), w) in of.iter().zip(&widths) {
        let chip = Rect::new(x, head.y, *w, h);
        match on {
            true => paint::fill(cx.fb, chip, INK),
            false => paint::stroke(cx.fb, chip, LIGHT, 1),
        }
        let tw = cx.text.measure_width_in(script, label) as i32;
        let baseline = chip.center_y() + cx.text.cap_height() as i32 / 2;
        cx.text.draw_in(
            script,
            cx.fb,
            chip.x + (chip.w - tw) / 2,
            baseline,
            label,
            *on,
        );
        cx.hit(*hit, chip);
        x += w;
    }
}

/// The books read on one day, longest first.
fn books_on(cx: &Ctx, day: i64) -> Vec<usize> {
    cx.stats
        .book_totals(day..=day)
        .into_iter()
        .map(|(book, _)| book)
        .collect()
}

/// The busiest day of a span, which every level on the page is banded against.
fn peak_of(cx: &Ctx, days: std::ops::RangeInclusive<i64>) -> i64 {
    days.map(|day| cx.stats.day_seconds(day)).max().unwrap_or(0)
}

/// The busiest single hour of any day of a span.
///
/// One scale under the whole grid: a day's shape says how much was read in
/// that hour, and not merely which of its own hours was the fullest.
fn hour_peak(cx: &Ctx, days: std::ops::RangeInclusive<i64>) -> i64 {
    days.filter_map(|day| cx.stats.hours_over(day..=day).into_iter().max())
        .max()
        .unwrap_or(0)
}

/// Lanes a cell `inner` tall has room for, of [`LANES`].
fn lane_count(theme: &Theme, inner: Rect) -> usize {
    let room = shape_box(theme, inner).y - lane_box(theme, inner, 0, 1).y;
    ((room / lane_height(theme)).max(1) as usize).min(LANES)
}

/// The height one lane takes, its air under it included.
fn lane_height(theme: &Theme) -> i32 {
    theme.small_px as i32 * 6 / 5
}

/// Where one lane's bar sits inside a cell, `width` across.
fn lane_box(theme: &Theme, inner: Rect, lane: usize, width: i32) -> Rect {
    let head = theme.small_px as i32 * 3 / 2;
    let each = lane_height(theme);
    Rect::new(
        inner.x,
        inner.y + head + lane as i32 * each,
        width,
        each - 2,
    )
}

/// The hours strip across the foot of a cell.
fn shape_box(theme: &Theme, inner: Rect) -> Rect {
    let h = theme.small_px as i32 * 2 / 3;
    Rect::new(inner.x, inner.bottom() - h, inner.w, h)
}

/// One book's bar: its title on a pale ground, clipped to the bar.
fn book_bar(cx: &mut Ctx, bar: Rect, index: usize) {
    let theme: &Theme = cx.theme;
    let book = &cx.stats.books[index];
    let script = Script::of_language(&book.language);
    let title = book.title.clone();
    paint::fill(cx.fb, bar, PALE);
    cx.text.set_px(theme.small_px);
    let room = (bar.w - theme.gap).max(1) as u32;
    let lines = cx.text.wrap_and_clamp_in(script, &title, room, 1);
    let baseline = bar.center_y() + cx.text.cap_height() as i32 / 2;
    cx.text.draw_in(
        script,
        cx.fb,
        bar.x + theme.gap / 2,
        baseline,
        lines.first().map(String::as_str).unwrap_or_default(),
        false,
    );
}

/// The columns one span cuts itself into, and how many of them have gone by
/// over `days`.
///
/// A week is read hour by hour, a month weekday by weekday, a year month by
/// month: the shape a reader is after at that zoom.
fn buckets(cx: &Ctx, span: Span, days: std::ops::RangeInclusive<i64>) -> (Vec<i64>, i64, usize) {
    let over = (cx.today.min(*days.end()) - *days.start() + 1).max(1);
    match span {
        Span::Week => (cx.stats.hours_over(days).to_vec(), over, 3),
        // `weekdays_over` counts from Monday; the columns run from whichever
        // day the week is set to start on.
        Span::Month => {
            let counted = cx.stats.weekdays_over(days);
            let week = cx.week;
            (
                (0..7).map(|at| counted[week.day_in(at)]).collect(),
                over.div_euclid(7).max(1),
                1,
            )
        }
        Span::Year => (
            cx.stats.months_over(days).to_vec(),
            over.div_euclid(30).max(1),
            1,
        ),
    }
}

/// What each column of a span is called, and the heading over them.
fn bucket_names(cx: &Ctx, span: Span) -> (&'static str, fn(usize, &Ctx) -> String) {
    let s = cx.s();
    match span {
        Span::Week => (s.an_average_day, |at, _| format!("{at:02}")),
        Span::Month => (s.an_average_week, |at, cx| {
            cx.s().weekdays_short[cx.week.day_in(at.min(6))].to_string()
        }),
        Span::Year => (s.an_average_month, |at, cx| {
            cx.s().months_short[at.min(11)].to_string()
        }),
    }
}

/// The reading of a span cut into its own columns, under a figure dividing it
/// by however many of them have gone by.
///
/// The columns keep the seconds themselves: an hour of a year divides to
/// nothing. The chips at the right swap the span showing for every span of its
/// width.
fn average_of(cx: &mut Ctx, area: Rect, state: &State, days: std::ops::RangeInclusive<i64>) {
    let theme: &Theme = cx.theme;
    let s = cx.s();
    let span = state.span;
    let all = state.average_all;
    let over = match all {
        true => cx.stats.days.first().map_or(cx.today, |(d, _)| *d)..=cx.today,
        false => days,
    };
    let (counted, elapsed, every) = buckets(cx, span, over);
    let (heading, name) = bucket_names(cx, span);
    let busiest = counted
        .iter()
        .enumerate()
        .max_by_key(|(_, secs)| **secs)
        .filter(|(_, secs)| **secs > 0)
        .map(|(at, _)| at);

    let each = date::duration(counted.iter().sum::<i64>() / elapsed, s);
    let title = match busiest {
        Some(at) => format!("{heading} — {each} · {} {}", s.most, name(at, cx)),
        None => format!("{heading} — {each}"),
    };
    let head = Rect::new(
        area.x,
        area.y,
        area.w,
        chrome::section_height(cx.text, theme),
    );
    let inner = chrome::section(cx.fb, cx.text, theme, area, &title);
    heading_chips(
        cx,
        head,
        [
            (span.label(cx.lang), Hit::Average(false), !all),
            (s.every, Hit::Average(true), all),
        ],
    );
    // `name` reads `cx`, which the draw below borrows.
    let labels: Vec<String> = (0..counted.len()).map(|at| name(at, cx)).collect();
    charts::columns(
        cx.fb,
        cx.text,
        theme,
        inner,
        &counted,
        |at| labels[at].clone(),
        every,
        busiest,
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A stand-in for `section_height`.
    const HEAD: i32 = 40;

    /// The panels this screen has to hold up on.
    const PANELS: [(i32, i32); 3] = [(1264, 1680), (1272, 1696), (1860, 2480)];

    fn page(w: i32, h: i32) -> (Theme, Rect) {
        let theme = Theme::for_screen(w as u32, h as u32);
        let area = chrome::content(&theme, Rect::new(0, 0, w, h));
        (theme, area)
    }

    /// What `span_page` asks `bands` for at this span.
    fn wanted(span: Span, area: Rect, theme: &Theme) -> i32 {
        let day = date::days_from_civil(2026, 9, 3);
        grid_height(span, area, theme, day, WeekStart::Monday)
    }

    #[test]
    fn the_bands_run_in_order_down_the_page() {
        for (w, h) in PANELS {
            let (theme, area) = page(w, h);
            for span in Span::ALL {
                let listed = lists_books(span);
                let out = bands(area, &theme, HEAD, wanted(span, area, &theme), listed);
                let [picker, nav, grid, hours, list] = out;
                assert_eq!(picker.y, area.y, "{w}x{h} {span:?}");
                let order = match listed {
                    true => [picker, nav, grid, list, hours],
                    false => [picker, nav, grid, hours, hours],
                };
                for pair in order.windows(2).filter(|p| p[0] != p[1]) {
                    let air = pair[1].y - pair[0].bottom();
                    assert!(air >= theme.gap, "{w}x{h} {span:?}: bands touch, {air}");
                }
                assert_eq!(hours.h, hours_height(&theme, HEAD), "{w}x{h} {span:?}");
                assert!(hours.bottom() <= area.bottom(), "{w}x{h} {span:?}");
                assert_eq!(list.h > 0, listed, "{w}x{h} {span:?}");
            }
        }
    }

    #[test]
    fn a_month_gives_its_grid_the_page_and_a_short_chart_the_foot() {
        for (w, h) in PANELS {
            let (theme, area) = page(w, h);
            let [_, nav, grid, hours, list] = bands(area, &theme, HEAD, area.h, false);
            assert_eq!(list.h, 0, "{w}x{h}");
            assert_eq!(hours.bottom(), area.bottom(), "{w}x{h}");
            assert!(
                grid.h > (nav.bottom() - area.y) * 2,
                "{w}x{h}: the grid takes {} of {}",
                grid.h,
                area.h
            );
            // The chart never takes a quarter of the page off the calendar.
            assert!(hours.h * 4 < area.h, "{w}x{h}: {} px of chart", hours.h);
        }
    }

    #[test]
    fn a_week_and_a_year_leave_room_for_books_under_them() {
        for (w, h) in PANELS {
            let (theme, area) = page(w, h);
            for span in [Span::Week, Span::Year] {
                let want = wanted(span, area, &theme);
                let [_, _, grid, _, list] = bands(area, &theme, HEAD, want, true);
                assert_eq!(grid.h, want, "{w}x{h} {span:?}: the grid was cut");
                let rows = (list.h - HEAD) / theme.row_h;
                let want = match span {
                    Span::Year => 2,
                    _ => 3,
                };
                assert!(rows >= want, "{w}x{h} {span:?}: room for {rows} books");
            }
        }
    }

    #[test]
    fn a_week_column_is_mostly_the_hours_of_its_day() {
        for (w, h) in PANELS {
            let (theme, area) = page(w, h);
            let want = wanted(Span::Week, area, &theme);
            let [_, _, grid, _, _] = bands(area, &theme, HEAD, want, true);
            let (_, cells) = grid.split_top(charts::weekday_head_height(&theme));
            let laid = charts::week_cells(cells, 0, theme.gap);
            assert_eq!(laid.len(), 7, "{w}x{h}");
            let inner = laid[0].1.inset(theme.gap / 2);
            let (_, plot) = inner.split_top(theme.small_px as i32 * 3 / 2);
            assert!(
                plot.h > inner.h / 2,
                "{w}x{h}: the histogram gets {} of {}",
                plot.h,
                inner.h
            );
            assert!(
                laid[0].1.w >= 24,
                "{w}x{h}: a column of 24 hours is too narrow"
            );
        }
    }

    #[test]
    fn a_month_cell_stacks_its_date_its_lanes_and_its_hours() {
        for (w, h) in PANELS {
            let (theme, area) = page(w, h);
            let [_, _, grid, _, _] = bands(area, &theme, HEAD, area.h, false);
            let (_, cells) = grid.split_top(charts::weekday_head_height(&theme));
            let laid = charts::month_cells(cells, 2026, 8, theme.gap, WeekStart::Monday);
            let inner = laid[0].1.inset(theme.gap / 2);

            let depth = lane_count(&theme, inner);
            assert!(depth >= 2, "{w}x{h}: a cell holds {depth} books");
            assert!(
                lane_box(&theme, inner, 0, inner.w).y >= inner.y + theme.small_px as i32,
                "{w}x{h}: the first lane sits on the date"
            );
            assert!(
                lane_box(&theme, inner, depth - 1, inner.w).bottom() <= shape_box(&theme, inner).y,
                "{w}x{h}: lane {} runs into the hours",
                depth - 1
            );
        }
    }

    #[test]
    fn a_day_page_holds_the_timeline_and_more_than_one_book() {
        for (w, h) in PANELS {
            let (theme, area) = page(w, h);
            let air = theme.gap * 2;
            let (_, rest) = area.split_top(bar_height(&theme) + air);
            let (_, rest) = rest.split_top(bar_height(&theme) + air);
            let (_, list) = rest.split_top(theme.row_h + air);
            let shown = daybooks::fits(&theme, list.h, 9);
            assert!(shown >= 3, "{w}x{h}: room for {shown} books");
        }
    }

    #[test]
    fn a_page_too_short_for_every_band_keeps_them_all_on_it() {
        let theme = Theme::for_screen(1264, 1680);
        let area = Rect::new(0, 0, 1186, 300);
        for listed in [true, false] {
            let out = bands(area, &theme, HEAD, 900, listed);
            for band in out {
                assert!(band.h >= 0, "{band:?}");
                assert!(band.y >= area.y, "{band:?} starts above the page");
                assert!(band.bottom() <= area.bottom(), "{band:?} runs off the page");
            }
        }
    }
}
