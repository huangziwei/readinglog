//! The record, at whichever zoom the picker is set to: `alltime` draws a board
//! of figures over the whole log, a week seven columns of hours, a month a grid
//! of weeks naming its books, a year one column a week.

use crate::date;
use crate::font::Script;
use crate::settings::WeekStart;
use crate::ui::paint::{self, BAR_RGB, INK, LIGHT, MARK_RGB, PALE, Rect, WHITE, WHITE_RGB};
use crate::ui::{charts, chrome, cover, theme::Theme};

use super::{Ctx, Hit, Span, State, alltime, daybooks, home};

/// Books a day of a month names, the rest counted in `+n`.
const LANES: usize = 4;

/// The bar the picker and the span's name each stand in.
fn bar_height(theme: &Theme) -> i32 {
    theme.row_h * 3 / 4
}

/// The five bands of the page, top to bottom: the picker, the span's name
/// between its arrows, what the span comes to as figures, the grid, and the
/// books. `grid` is what the span asks for, clamped to what the page holds.
fn bands(area: Rect, theme: &Theme, figures: i32, grid: i32, listed: bool) -> [Rect; 5] {
    let air = theme.gap * 2;
    let bar = bar_height(theme);
    let (picker, rest) = area.split_top(bar + air);
    let (nav, rest) = rest.split_top(bar + air);
    // The span's name stands clear of the figures under it: the two are a
    // heading and its body, not one block.
    let (stated, rest) = rest.split_top(figures + air * 2);

    let grid = match listed {
        true => grid.clamp(0, rest.h),
        false => rest.h,
    };
    let list = (rest.h - grid - air).max(0) * listed as i32;
    // A span listing no books still answers with a band, which stands at the
    // foot of the page rather than past it.
    let under = (rest.y + grid + air).min(rest.bottom());
    [
        Rect::new(picker.x, picker.y, picker.w, bar),
        Rect::new(nav.x, nav.y, nav.w, bar),
        Rect::new(stated.x, stated.y, stated.w, figures),
        Rect::new(rest.x, rest.y, rest.w, grid),
        Rect::new(rest.x, under, rest.w, list),
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
        Span::AllTime | Span::Month => area.h,
        // A week has seven columns and a shelf of covers under them, so the
        // grid takes a share of the page and never less than its own height.
        Span::Week => week_height(theme).max(area.h * 2 / 5),
        Span::Year => {
            let (year, _, _) = date::civil_from_days(day);
            let box_ = Rect::new(area.x, area.y, area.w - gutter(theme), area.h);
            year_map(box_, year, theme, week).height
        }
    }
}

/// The bands a year's weeks are cut into, stacked down the page.
const YEAR_BLOCKS: i32 = 2;

/// The column the weekday names stand in beside a year.
fn gutter(theme: &Theme) -> i32 {
    theme.small_px as i32 * 3
}

pub fn draw(cx: &mut Ctx, area: Rect, state: &State) {
    let theme: &Theme = cx.theme;
    if !opens_page_whole(state) {
        span_page(cx, area, state);
        return;
    }
    let (bar, rest) = area.split_top(bar_height(theme) + theme.gap * 2);
    picker(cx, Rect::new(bar.x, bar.y, bar.w, bar_height(theme)), state);
    match state.picked {
        true => day_page(cx, rest, state),
        false => alltime::draw(cx, rest, state.alltime_page),
    }
}

/// Whether `draw` gives the box under `picker` to one page: `Span::AllTime`,
/// or a day picked off a span whose grid has no room to state it.
fn opens_page_whole(state: &State) -> bool {
    state.span == Span::AllTime || (state.picked && (state.opened_day || !lists_books(state.span)))
}

/// The span holding `state.day`: its name, its grid, its average day, and the
/// books read over it.
fn span_page(cx: &mut Ctx, area: Rect, state: &State) {
    let theme: &Theme = cx.theme;
    let s = cx.s();
    let (span, day) = (state.span, state.day);
    let days = span.days(day, cx.week);

    let listed = lists_books(span);
    let figures = chrome::figure_height_at(cx.text, theme, span_figure_px(theme));
    let want = grid_height(span, area, theme, day, cx.week);
    let [bar, nav, stated, grid, list] = bands(area, theme, figures, want, listed);

    picker(cx, bar, state);
    // The chip stands only where there is somewhere to go: the span holding
    // today needs no way back to itself.
    let adrift = !days.contains(&cx.today);
    span_nav(cx, nav, &span.name(day, cx.week, s), adrift);
    span_figures(cx, stated, days.clone());

    match span {
        // `alltime::draw` takes the page ahead of `span_page`.
        Span::AllTime => {}
        Span::Week => week_columns(cx, grid, day, state.picked),
        Span::Month => month_grid(cx, grid, day),
        Span::Year => year_heatmap(cx, grid, day, state.picked),
    }
    // A year and a week both name their books by their jackets under the grid.
    // A month names them in its own cells and lists none.
    if listed {
        cover_grid(cx, list, state, days);
    }
}

/// One day in full on `home::bands`: its figures, its timeline under the day
/// it fell on, and the books read on it. A day of more books than the list
/// holds pages them from its heading.
fn day_page(cx: &mut Ctx, area: Rect, state: &State) {
    let theme: &Theme = cx.theme;
    let s = cx.s();
    let day = state.day;
    let figures = chrome::figure_height(cx.text, theme);
    let head = chrome::section_height(cx.text, theme);
    let [top, strip, list] = home::bands(area, theme, figures, head);

    let turns: i64 = cx.stats.sittings_on(day).map(|s| s.page_turns).sum();
    let longest = cx
        .stats
        .sittings_on(day)
        .map(|s| s.seconds)
        .max()
        .unwrap_or(0);
    let stated = [
        (date::duration(cx.stats.day_seconds(day), s), s.total_read),
        (turns.to_string(), s.pages_turned),
        (date::duration(longest, s), s.longest_sitting),
    ];
    chrome::figures(cx.fb, cx.text, theme, top, &stated);

    let named = date::long_day(day, s).to_uppercase();
    let inner = chrome::section(cx.fb, cx.text, theme, strip, &named);
    let spans = cx.stats.day_blocks(day);
    let now = (day == cx.today).then_some(cx.now);
    charts::timeline(cx.fb, cx.text, theme, inner, &spans, now);

    daybooks::paged(cx, list, day, state.list_from);
}

/// The four spans as a segmented control, each its own hit box. A day open
/// over the calendar lights none of them, and a tap on one closes it.
fn picker(cx: &mut Ctx, area: Rect, state: &State) {
    let theme: &Theme = cx.theme;
    let cells = area.columns(Span::ALL.len() as i32, 0);
    cx.text.set_px(theme.body_px);
    let baseline = area.center_y() + cx.text.cap_height() as i32 / 2;
    let script = cx.ui_script();
    let lit = state.span;
    let dark = state.picked && opens_page_whole(state);
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

/// The largest a span's figures are set, which is under
/// [`Theme::display_px`]: they stand as one band over a grid and a list, not
/// as the head of the page, and the band is sized to what they take.
fn span_figure_px(theme: &Theme) -> f32 {
    theme.head_px
}

/// What the span holding `days` came to: the reading, the days it fell on,
/// what one of those days averaged, and the books read through inside it.
fn span_figures(cx: &mut Ctx, area: Rect, days: std::ops::RangeInclusive<i64>) {
    let s = cx.s();
    let span = cx.stats.tally(days);
    let stated = [
        (date::duration_coarse(span.read, s), s.total_read),
        (span.days_read.to_string(), s.days_read),
        (date::duration(span.a_day, s), s.a_day),
        (span.finished.to_string(), s.finished),
    ];
    let ceiling = span_figure_px(cx.theme);
    chrome::figures_at(cx.fb, cx.text, cx.theme, area, &stated, ceiling);
}

/// `title` between two arrows, each its own hit box. `back` draws the chip
/// returning to the span that holds today, beside the name it replaces.
///
/// No rule under it: the air below is what separates the name from the
/// figures, and a line there closes the two into one block. The chip keeps
/// its distance from both arrows, a thumb's width and more, so a reach for
/// one cannot land on the other.
fn span_nav(cx: &mut Ctx, area: Rect, title: &str, back: bool) {
    let theme: &Theme = cx.theme;
    let script = cx.ui_script();
    cx.text.set_px(theme.head_px);
    let baseline = area.center_y() + cx.text.cap_height() as i32 / 2;
    let title_w = cx.text.measure_width_in(script, title) as i32;

    // The name is centred whatever else stands on the row: the chip beside it
    // is optional and must not move it.
    let at = area.x + (area.w - title_w) / 2;
    cx.text.draw_in(script, cx.fb, at, baseline, title, false);
    let chip = back.then(|| now_chip(cx, area, at + title_w + theme.gap * 3));

    cx.text.set_px(theme.head_px);
    let next = cx.text.measure_width("›") as i32;
    cx.text.draw(cx.fb, area.x, baseline, "‹", false);
    cx.text
        .draw(cx.fb, area.right() - next, baseline, "›", false);

    // The forward arrow takes a sixth of the row, or whatever the chip beside
    // a long name leaves it, so the two never share a pixel.
    let reach = area.w / 6;
    let opens = chip.map_or(area.right() - reach, |chip| {
        (chip.right() + theme.gap).max(area.right() - reach)
    });
    cx.hit(Hit::Prev, Rect::new(area.x, area.y, reach, area.h));
    cx.hit(
        Hit::Next,
        Rect::new(opens, area.y, area.right() - opens, area.h),
    );
}

/// The chip returning to the span holding today, opening at `x`. Answers the
/// box it took.
fn now_chip(cx: &mut Ctx, area: Rect, x: i32) -> Rect {
    let theme: &Theme = cx.theme;
    let script = cx.ui_script();
    let said = cx.s().now;
    let w = chip_width(cx, said);
    let h = cx.text.line_height() as i32 + theme.gap;
    let chip = Rect::new(x, area.center_y() - h / 2, w, h);
    paint::stroke(cx.fb, chip, LIGHT, 1);
    let tw = cx.text.measure_width_in(script, said) as i32;
    let baseline = chip.center_y() + cx.text.cap_height() as i32 / 2;
    cx.text
        .draw_in(script, cx.fb, chip.x + (w - tw) / 2, baseline, said, false);
    cx.hit(Hit::Now, chip);
    chip
}

/// Rows of `theme.row_h` a week's bars are never drawn shorter than.
const WEEK_BARS: (i32, i32) = (2, 1);

/// The share of a bar its own hours are cut into, at its foot, and the rows of
/// `theme.row_h` that share never passes.
const WEEK_CLOCK: (i32, i32) = (1, 3);
const WEEK_CLOCK_CAP: (i32, i32) = (2, 5);

/// The share of its column a day's bar is drawn at. Wide enough to carry the
/// day's hours inside it, and short of the whole so the columns read as seven
/// bars and not as one band broken by gaps.
const WEEK_BAR_WIDTH: (i32, i32) = (7, 8);

/// The two rows a week's grid stacks: the dates, and one bar to a day with
/// that day's own hours in its foot.
fn week_rows(area: Rect, theme: &Theme) -> [Rect; 2] {
    let (head, bars) = area.split_top(theme.small_px as i32 * 2);
    [head, bars]
}

/// The least [`week_rows`] is drawn into, which a taller page passes.
fn week_height(theme: &Theme) -> i32 {
    theme.small_px as i32 * 2 + theme.row_h * WEEK_BARS.0 / WEEK_BARS.1
}

/// The week holding `day`: its dates, and how long was read on each with the
/// hours that reading fell in drawn inside the bar.
fn week_columns(cx: &mut Ctx, area: Rect, day: i64, picked: bool) {
    let theme: &Theme = cx.theme;
    let [head, bars] = week_rows(area, theme);
    let first = day - cx.week.column_of(date::weekday(day)) as i64;
    let most = peak_of(cx, first..=first + 6);

    let names = charts::week_cells(head, first, theme.gap);
    let plots = charts::week_cells(bars, first, theme.gap);
    for ((at, name), (_, plot)) in names.into_iter().zip(plots) {
        let on = picked && at == day;
        let column = Rect::new(name.x, name.y, name.w, plot.bottom() - name.y);
        if on {
            paint::fill(cx.fb, column, PALE);
        }
        day_head(cx, name, at);
        day_bar(cx, plot, at, most, on);
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

/// The box a day's hours are cut into, inside its own bar.
///
/// A whole number of hours wide, so each hour has its own column of pixels and
/// a mark's distance along the box is a time. It is held clear of the bar's
/// own edges: a mark reaching an edge would open onto the page and read as a
/// bar broken in two rather than as an hour. An answer with no width or no
/// height is a bar with no room for its hours.
fn clock_box(theme: &Theme, bar: Rect) -> Rect {
    let pad = (theme.gap / 2).max(1);
    let w = ((bar.w - pad * 2).max(0) / 24) * 24;
    // A share of the bar, so a short day keeps a bar and not a row of notches,
    // and never past a strip, so a long one keeps a bar and not a comb.
    let cap = theme.row_h * WEEK_CLOCK_CAP.0 / WEEK_CLOCK_CAP.1;
    let h = (bar.h * WEEK_CLOCK.0 / WEEK_CLOCK.1).min(cap) - pad;
    Rect::new(bar.x + (bar.w - w) / 2, bar.bottom() - pad - h, w, h.max(0))
}

/// One day's total, as a bar up its own column with the figure over it and
/// the hours it was read in cut into its foot.
///
/// The bar's height says how much was read and is scaled against the week; the
/// foot says when, and is scaled against the day's own busiest hour so a quiet
/// day states its shape as plainly as a full one.
///
/// See [`clock_box`] for where the hours stand inside the bar.
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
    let bw = (area.w * WEEK_BAR_WIDTH.0 / WEEK_BAR_WIDTH.1).max(2);
    let bar = Rect::new(area.x + (area.w - bw) / 2, area.bottom() - h, bw, h);
    paint::fill_rgb(cx.fb, bar, if on { MARK_RGB } else { BAR_RGB });
    let hours = cx.stats.hours_over(day..=day);
    let busiest = hours.iter().copied().max().unwrap_or(0);
    // The hours are cut out of the bar rather than drawn over it: the bar
    // keeps its own weight on the page and the marks read as part of it.
    charts::hour_shape(cx.fb, clock_box(theme, bar), &hours, busiest, WHITE_RGB);
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

    // A week of the grid shares its lanes: a book read two days running draws
    // one bar across them.
    for row in cells.chunk_by(|a, b| a.1.y == b.1.y) {
        let days: Vec<Vec<usize>> = row.iter().map(|(day, _)| books_on(cx, *day)).collect();
        let depth = lane_count(theme, row[0].1.inset(theme.gap / 2));
        let lanes = charts::lanes(&days, depth);
        for (day, cell) in row.iter() {
            let (_, _, dom) = date::civil_from_days(*day);
            head_line(cx, *cell, *day, &dom.to_string(), peak);
            let inner = cell.inset(theme.gap / 2);
            let hours = cx.stats.hours_over(*day..=*day);
            charts::hour_shape(cx.fb, shape_box(theme, inner), &hours, tallest, BAR_RGB);
            cx.hit(Hit::Day(*day), *cell);
        }
        // A run reaches past its own cell; every bar of the week is drawn over
        // the cells the whole week laid down.
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
        // A day with nothing on it takes no tap. The cells are small enough
        // that an empty one standing between two read days would only steal
        // the reach meant for them, and it has nothing to state.
        if secs > 0 {
            cx.hit(Hit::Day(*at), *cell);
        }
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

/// The shortest a cover may be drawn: under this the jacket states nothing
/// and the grid is better off with fewer, larger ones.
const COVER_FLOOR: i32 = 3;

/// The air the cover grid keeps: between one jacket and the next, under the
/// heading before the first row, and under a jacket before its figures.
/// Jackets set shoulder to shoulder read as one band of colour, not a shelf.
fn cover_air(theme: &Theme) -> i32 {
    theme.gap * 2
}

/// Every book read over `days` as its cover, what was read of it under each.
///
/// As many to a row as the width holds and as many rows as the band has
/// height for; a grid too small for them all is paged from its heading. The
/// figures under a cover are set to the cover's own width.
fn cover_grid(cx: &mut Ctx, area: Rect, state: &State, days: std::ops::RangeInclusive<i64>) {
    let theme: &Theme = cx.theme;
    let s = cx.s();
    // A day picked off the heatmap narrows the grid to that day.
    let picked = state.picked.then_some(state.day);
    let over = picked.map_or(days, |day| day..=day);
    let total = date::duration(cx.stats.span_seconds(over.clone()), s);
    let title = match picked {
        Some(day) => format!("{} · {total}", date::long_day(day, s).to_uppercase()),
        None => format!("{} · {total}", s.what_was_read),
    };
    let head = Rect::new(
        area.x,
        area.y,
        area.w,
        chrome::section_height(cx.text, theme),
    );
    let whole = chrome::section(cx.fb, cx.text, theme, area, &title);
    // Most recently put down first, which is the order a shelf is read in.
    let read = cx.stats.book_totals_recent(over.clone());
    if picked.is_some() {
        open_day_chip(cx, head);
    }

    let (unnamed, seconds) = cx.stats.unnamed_over(over);
    let foot = (unnamed > 0) as i32 * daybooks::note_height(cx);
    let inner = Rect::new(whole.x, whole.y, whole.w, (whole.h - foot).max(0));

    if read.is_empty() {
        cx.text.set_px(theme.body_px);
        let baseline = inner.y + cx.text.line_height() as i32;
        let script = cx.ui_script();
        cx.text
            .draw_in(script, cx.fb, inner.x, baseline, s.nothing_read, false);
        daybooks::note(
            cx,
            Rect::new(whole.x, baseline + theme.gap * 2, whole.w, foot),
            unnamed,
            seconds,
        );
        return;
    }

    // The arrows stand at the ends of the grid's own row, where a thumb
    // reaches without covering a jacket. The grid keeps the middle, opening a
    // row of air below the heading.
    let reach = arrow_reach(cx);
    let air = cover_air(theme);
    let (_, inner) = inner.split_top(air.min(inner.h));
    let grid = Rect::new(
        inner.x + reach,
        inner.y,
        (inner.w - reach * 2).max(1),
        inner.h,
    );
    let (rows, columns, cell) = grid_of(cx, grid);
    let deep = (rows * columns).max(1) as usize;
    let from = state.list_from.min(super::last_page_at(read.len(), deep));
    let to = (from + deep).min(read.len());
    if read.len() > deep {
        counted(cx, head, from, to, read.len(), picked.is_some());
        let last = super::last_page_at(read.len(), deep);
        let rows_h = (to - from).div_ceil(columns as usize) as i32 * (cell.h + air);
        let band = Rect::new(inner.x, inner.y, inner.w, rows_h.min(inner.h));
        step_arrow(
            cx,
            band,
            true,
            (from > 0).then(|| from.saturating_sub(deep)),
        );
        step_arrow(
            cx,
            band,
            false,
            (to < read.len()).then(|| (from + deep).min(last)),
        );
    }

    for (slot, (book, secs)) in read[from..to].iter().enumerate() {
        let at = slot as i32;
        let box_ = Rect::new(
            grid.x + (at % columns) * (cell.w + air),
            grid.y + (at / columns) * (cell.h + air),
            cell.w,
            cell.h,
        );
        jacket(cx, box_, *book, *secs);
        cx.hit(Hit::Book(*book), box_);
    }
    let under = inner.y + (to - from).div_ceil(columns as usize) as i32 * (cell.h + air);
    daybooks::note(
        cx,
        Rect::new(whole.x, under, whole.w, whole.bottom() - under),
        unnamed,
        seconds,
    );
}

/// The width an arrow beside the cover grid takes, its air included.
fn arrow_reach(cx: &mut Ctx) -> i32 {
    let theme: &Theme = cx.theme;
    cx.text.set_px(theme.head_px);
    cx.text.measure_width("›") as i32 + theme.gap * 2
}

/// An arrow at one end of `band`, opening the list at `at`. Nothing is drawn
/// where there is no page that way.
fn step_arrow(cx: &mut Ctx, band: Rect, at_left: bool, at: Option<usize>) {
    let theme: &Theme = cx.theme;
    let Some(at) = at else { return };
    let reach = arrow_reach(cx);
    let said = match at_left {
        true => "‹",
        false => "›",
    };
    let box_ = match at_left {
        true => Rect::new(band.x, band.y, reach, band.h),
        false => Rect::new(band.right() - reach, band.y, reach, band.h),
    };
    cx.text.set_px(theme.head_px);
    let w = cx.text.measure_width(said) as i32;
    let baseline = box_.center_y() + cx.text.cap_height() as i32 / 2;
    cx.text
        .draw(cx.fb, box_.x + (box_.w - w) / 2, baseline, said, false);
    cx.hit(Hit::ListPage(at), box_);
}

/// `from`–`to` of `count` at the right of the heading the grid stands under,
/// left of the chip where one stands there.
fn counted(cx: &mut Ctx, head: Rect, from: usize, to: usize, count: usize, chipped: bool) {
    let theme: &Theme = cx.theme;
    let of = format!("{}–{to} {} {count}", from + 1, cx.s().of);
    cx.text.set_px(theme.small_px);
    let w = cx.text.measure_width(&of) as i32;
    let taken = chipped as i32 * (chip_width(cx, cx.s().open_day) + theme.gap);
    let row = chrome::heading_row(cx.text, theme, head);
    cx.text.set_px(theme.small_px);
    let baseline = row.center_y() + cx.text.cap_height() as i32 / 2;
    cx.text
        .draw(cx.fb, head.right() - taken - w, baseline, &of, false);
}

/// How wide a chip carrying `said` stands, its padding included.
fn chip_width(cx: &mut Ctx, said: &str) -> i32 {
    let theme: &Theme = cx.theme;
    let script = cx.ui_script();
    cx.text.set_px(theme.small_px);
    cx.text.measure_width_in(script, said) as i32 + theme.gap * 2
}

/// The chip opening the day the grid is narrowed to as its own page, at the
/// right of the heading naming that day.
fn open_day_chip(cx: &mut Ctx, head: Rect) {
    let theme: &Theme = cx.theme;
    let script = cx.ui_script();
    let said = cx.s().open_day;
    let w = chip_width(cx, said);
    let row = chrome::heading_row(cx.text, theme, head);
    let chip = Rect::new(row.right() - w, row.y, w, row.h);
    paint::stroke(cx.fb, chip, LIGHT, 1);
    let tw = cx.text.measure_width_in(script, said) as i32;
    let baseline = chip.center_y() + cx.text.cap_height() as i32 / 2;
    cx.text
        .draw_in(script, cx.fb, chip.x + (w - tw) / 2, baseline, said, false);
    cx.hit(Hit::OpenDay, chip);
}

/// How the grid cuts `inner`: the rows and columns it holds, and one cell.
///
/// A cell is a cover with two lines under it, and the cover keeps its own
/// two-to-three shape, so the columns follow from whatever height the rows
/// leave.
fn grid_of(cx: &mut Ctx, inner: Rect) -> (i32, i32, Rect) {
    let theme: &Theme = cx.theme;
    cx.text.set_px(theme.small_px);
    let air = cover_air(theme);
    let under = cx.text.line_height() as i32 * 2 + air;
    let least = theme.row_h * COVER_FLOOR + under;

    let rows = ((inner.h + air) / (least + air)).max(1);
    let cell_h = (inner.h - air * (rows - 1)) / rows;
    let cover_h = (cell_h - under).max(1);
    let cell_w = cover::width_for(cover_h);
    let columns = ((inner.w + air) / (cell_w + air)).max(1);
    (rows, columns, Rect::new(0, 0, cell_w, cell_h))
}

/// One book of the grid: its cover, the reading under it, and how far through
/// it stands.
fn jacket(cx: &mut Ctx, box_: Rect, book: usize, secs: i64) {
    let theme: &Theme = cx.theme;
    let s = cx.s();
    cx.text.set_px(theme.small_px);
    let line = cx.text.line_height() as i32;
    // The jacket keeps clear air under it before the figures start.
    let under_h = line * 2 + cover_air(theme);
    let (art, under) = box_.split_top((box_.h - under_h).max(1));
    cx.covers
        .draw(cx.fb, art, &cx.stats.books[book].thumbnail.clone());

    let read = date::duration_tight(secs, s);
    let stat = &cx.stats.books[book];
    let percent = match stat.has_percent() {
        true => format!("{}%", stat.percent_shown()),
        false => String::new(),
    };
    // Both lines are set to the cover's own width: a title read for a hundred
    // hours states a wider figure than the jacket beside it.
    let px = fitting_px(cx, &[&read, &percent], box_.w);
    cx.text.set_px(px);
    let mut baseline = under.y + theme.gap + cx.text.cap_height() as i32;
    for said in [&read, &percent] {
        if said.is_empty() {
            continue;
        }
        let w = cx.text.measure_width(said) as i32;
        cx.text
            .draw(cx.fb, box_.x + (box_.w - w) / 2, baseline, said, false);
        baseline += cx.text.line_height() as i32;
    }
}

/// The largest size at or under [`Theme::small_px`] that sets every one of
/// `said` inside `room`.
fn fitting_px(cx: &mut Ctx, said: &[&str], room: i32) -> f32 {
    let theme: &Theme = cx.theme;
    let floor = theme.small_px * 0.6;
    let mut px = theme.small_px;
    while px > floor {
        cx.text.set_px(px);
        let widest = said
            .iter()
            .map(|s| cx.text.measure_width(s) as i32)
            .max()
            .unwrap_or(0);
        if widest <= room {
            break;
        }
        px = (px * room as f32 / widest.max(1) as f32)
            .min(px - 1.0)
            .max(floor);
    }
    px
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

/// The busiest single hour of any day of `days`. One scale under the whole
/// grid: a day's shape states how much was read in that hour.
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

#[cfg(test)]
mod tests {
    use super::*;

    /// A stand-in for `section_height`.
    const HEAD: i32 = 40;

    /// A stand-in for `chrome::figure_height`.
    const FIGURES: i32 = 90;

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
            for span in Span::CALENDAR {
                let listed = lists_books(span);
                let [picker, nav, stated, grid, list] =
                    bands(area, &theme, FIGURES, wanted(span, area, &theme), listed);
                assert_eq!(picker.y, area.y, "{w}x{h} {span:?}");
                assert_eq!(stated.h, FIGURES, "{w}x{h} {span:?}");
                let order = match listed {
                    true => [picker, nav, stated, grid, list],
                    false => [picker, nav, stated, grid, grid],
                };
                for pair in order.windows(2).filter(|p| p[0] != p[1]) {
                    let air = pair[1].y - pair[0].bottom();
                    assert!(air >= theme.gap, "{w}x{h} {span:?}: bands touch, {air}");
                }
                assert!(list.bottom() <= area.bottom(), "{w}x{h} {span:?}");
                assert_eq!(list.h > 0, listed, "{w}x{h} {span:?}");
            }
        }
    }

    #[test]
    fn every_span_states_its_figures_over_the_grid() {
        for (w, h) in PANELS {
            let (theme, area) = page(w, h);
            for span in Span::CALENDAR {
                let want = wanted(span, area, &theme);
                let [_, nav, stated, grid, _] =
                    bands(area, &theme, FIGURES, want, lists_books(span));
                assert!(stated.y > nav.bottom(), "{w}x{h} {span:?}");
                assert!(stated.bottom() < grid.y, "{w}x{h} {span:?}");
                assert!(grid.h > stated.h, "{w}x{h} {span:?}: the grid is crowded");
            }
        }
    }

    #[test]
    fn a_month_gives_its_grid_the_whole_page() {
        for (w, h) in PANELS {
            let (theme, area) = page(w, h);
            let [_, nav, _, grid, list] = bands(area, &theme, FIGURES, area.h, false);
            assert_eq!(list.h, 0, "{w}x{h}");
            assert_eq!(grid.bottom(), area.bottom(), "{w}x{h}");
            assert!(
                grid.h > (nav.bottom() - area.y) * 2,
                "{w}x{h}: the grid takes {} of {}",
                grid.h,
                area.h
            );
        }
    }

    #[test]
    fn a_week_and_a_year_leave_room_for_books_under_them() {
        for (w, h) in PANELS {
            let (theme, area) = page(w, h);
            for span in [Span::Week, Span::Year] {
                let want = wanted(span, area, &theme);
                let [_, _, _, grid, list] = bands(area, &theme, FIGURES, want, true);
                assert_eq!(grid.h, want, "{w}x{h} {span:?}: the grid was cut");
                let room = list.h - HEAD;
                match span {
                    // A year names its books by their jackets: a cover and the
                    // two lines under it, which is three rows of type.
                    Span::Year => assert!(
                        room >= theme.row_h * 3,
                        "{w}x{h}: {room} px for a row of covers"
                    ),
                    // A week lists them: four rows and the note under them.
                    _ => {
                        let rows = room / theme.row_h;
                        assert!(rows >= 4, "{w}x{h}: room for {rows} books");
                    }
                }
            }
        }
    }

    #[test]
    fn a_day_picked_off_a_year_narrows_its_books_before_it_opens() {
        let mut state = State::new(date::days_from_civil(2026, 9, 16));
        state.span = Span::Year;
        state.picked = true;
        assert!(
            !opens_page_whole(&state),
            "a picked day narrows the covers to it"
        );
        state.opened_day = true;
        assert!(opens_page_whole(&state), "and opens as its own page on ask");

        // A span with no book list under it opens a picked day either way.
        state.span = Span::Month;
        state.opened_day = false;
        assert!(opens_page_whole(&state));
    }

    #[test]
    fn a_week_bar_holds_its_own_hours_inside_it() {
        for (w, h) in PANELS {
            let (theme, area) = page(w, h);
            let want = wanted(Span::Week, area, &theme);
            let [_, _, _, grid, _] = bands(area, &theme, FIGURES, want, true);
            let [dates, bars] = week_rows(grid, &theme);
            assert_eq!(dates.bottom(), bars.y, "{w}x{h}");
            assert_eq!(bars.bottom(), grid.bottom(), "{w}x{h}");

            let laid = charts::week_cells(bars, 0, theme.gap);
            assert_eq!(laid.len(), 7, "{w}x{h}");

            let cell = laid[0].1;
            let bw = cell.w * WEEK_BAR_WIDTH.0 / WEEK_BAR_WIDTH.1;
            assert!(bw < cell.w, "{w}x{h}: bar {bw} fills its column {}", cell.w);
            assert!(bw > cell.w * 3 / 4, "{w}x{h}: bar {bw} of {}", cell.w);
            for tall in [2, 8, 30, bars.h / 2, bars.h] {
                let bar = Rect::new(cell.x, cell.bottom() - tall, bw, tall);
                let clock = clock_box(&theme, bar);
                if clock.w <= 0 || clock.h <= 0 {
                    continue;
                }
                // Every hour of the day gets its own column of pixels, and
                // none of the box is left over past the last of them.
                assert_eq!(clock.w % 24, 0, "{w}x{h} {tall}: {} wide", clock.w);
                // Blue on all four sides, so no mark opens onto the page.
                assert!(clock.x > bar.x, "{w}x{h} {tall}");
                assert!(clock.right() < bar.right(), "{w}x{h} {tall}");
                assert!(clock.y > bar.y, "{w}x{h} {tall}");
                assert!(clock.bottom() < bar.bottom(), "{w}x{h} {tall}");
            }
            // A bar wide enough to state its hours at every height it reaches.
            let tallest = Rect::new(cell.x, cell.y, bw, bars.h);
            assert!(clock_box(&theme, tallest).w >= 24, "{w}x{h}: too narrow");
        }
    }

    /// A jacket is as tall as the band it stands in, so the week's grid must
    /// leave one deep enough to draw a cover at.
    #[test]
    fn a_week_leaves_its_covers_a_band_a_jacket_fits() {
        for (w, h) in PANELS {
            let (theme, area) = page(w, h);
            let want = wanted(Span::Week, area, &theme);
            let [_, _, _, grid, list] = bands(area, &theme, FIGURES, want, true);
            assert_eq!(grid.h, want, "{w}x{h}: the grid took what it was given");
            let under = theme.small_px as i32 * 2 + cover_air(&theme);
            let least = theme.row_h * COVER_FLOOR + under;
            assert!(
                list.h >= least,
                "{w}x{h}: {} under a grid of {}",
                list.h,
                grid.h
            );
        }
    }

    #[test]
    fn a_month_cell_stacks_its_date_its_lanes_and_its_hours() {
        for (w, h) in PANELS {
            let (theme, area) = page(w, h);
            let [_, _, _, grid, _] = bands(area, &theme, FIGURES, area.h, false);
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

    /// The list `day_page` draws the day's books into, on a `w` by `h` panel.
    fn day_list(w: i32, h: i32) -> (Theme, Rect) {
        let (theme, area) = page(w, h);
        let (_, rest) = area.split_top(bar_height(&theme) + theme.gap * 2);
        let [_, _, list] = home::bands(rest, &theme, FIGURES, HEAD);
        (
            theme,
            Rect::new(list.x, list.y + HEAD, list.w, list.h - HEAD),
        )
    }

    #[test]
    fn a_day_page_holds_more_than_one_book_under_its_timeline() {
        for (w, h) in PANELS {
            let (theme, list) = day_list(w, h);
            let shown = daybooks::fits(&theme, list.h, 9);
            assert!(shown >= 2, "{w}x{h}: room for {shown} books");
        }
    }

    #[test]
    fn paging_a_day_tiles_its_books_and_repeats_none() {
        for (w, h) in PANELS {
            let (theme, list) = day_list(w, h);
            for books in 0..=20usize {
                let deep = daybooks::fits(&theme, list.h, books);
                let last = super::super::last_page_at(books, deep);
                let (mut from, mut seen) = (0usize, 0usize);
                loop {
                    assert_eq!(from, seen, "{w}x{h}, {books} books: {from} repeats");
                    seen = (from + deep).min(books);
                    let next = (from + deep).min(last);
                    if next == from {
                        break;
                    }
                    from = next;
                }
                assert_eq!(seen, books, "{w}x{h}: {seen} of {books} books reached");
                assert_eq!(from, last, "{w}x{h}: paging stops short of {last}");
            }
        }
    }

    #[test]
    fn a_page_too_short_for_every_band_keeps_them_all_on_it() {
        let theme = Theme::for_screen(1264, 1680);
        let area = Rect::new(0, 0, 1186, 300);
        for listed in [true, false] {
            let out = bands(area, &theme, FIGURES, 900, listed);
            for band in out {
                assert!(band.h >= 0, "{band:?}");
                assert!(band.y >= area.y, "{band:?} starts above the page");
                assert!(band.bottom() <= area.bottom(), "{band:?} runs off the page");
            }
        }
    }
}
