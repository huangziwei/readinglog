//! [`draw`]: All Time's pages: the board of four rows of three figures, three of them carrying
//! a hit box, Trends folding the record onto a day, a week and a year, and [`Breakdown`]'s pages
//! where the record holds a `d` or `l` row; [`BACK`] and [`FORWARD`] step between them.

use crate::date;
use crate::lang::{Lang, Strings};
use crate::settings::{SittingFloor, StatsFrom, WeekStart};
use crate::stats::{Fold, SITTING_BANDS, SITTING_STEP_SECS, Stats};
use crate::ui::paint::{self, LIGHT, PALE, Rect};
use crate::ui::text::TextRenderer;
use crate::ui::theme::Theme;
use crate::ui::{charts, chrome};

use super::breakdown::{self, Breakdown};
use super::{Ctx, Hit, Shelf, State};

/// `PAGES`: All Time's pages before [`Breakdown`]'s: the board and Trends.
pub const PAGES: usize = 2;

/// `ROWS`: rows of three the board holds.
const ROWS: i32 = 4;

/// `BANDS`: Bands the Trends page stacks: the three folds and the sittings.
const BANDS: i32 = 4;

/// `Cell`: one figure of the board: what it states, what it is called, and the page it opens.
struct Cell {
    value: String,
    label: &'static str,
    opens: Option<Hit>,
}

/// `draw`: the page `state.alltime_page` in `area`, and the arrows onto the pages either side;
/// `state.whole`, a Breakdown card opened whole over them.
pub fn draw(cx: &mut Ctx, area: Rect, state: &State) {
    let (line, body) = area.split_top(edge_height(cx.theme));
    let held = breakdown_in(
        cx.text,
        cx.theme,
        cx.lang,
        cx.stats,
        cx.stats_from,
        cx.today,
        area,
    );
    if held.pages() == 0 {
        match state.alltime_page {
            0 => {
                span_line(cx, line);
                edges(cx, line, None, Some(FORWARD));
                let cells = cells(cx);
                board_of(cx, body, &cells);
            }
            _ => {
                let name = cx.s().trends.to_string();
                edges(cx, line, Some(BACK), Some(&name));
                trends(cx, body);
            }
        }
        return;
    }
    if let Some(open) = state.whole {
        breakdown::whole(cx, area, open, &held);
        return;
    }
    let pages = PAGES + held.pages();
    let page = state.alltime_page.min(pages - 1);
    let name = match page {
        0 => since(cx.stats, cx.today, cx.s()),
        1 => cx.s().trends.to_string(),
        at => held.name(cx.s(), at - PAGES),
    };
    centred(cx, line, &name, page > 0, page + 1 < pages);
    match page {
        0 => {
            let cells = cells(cx);
            board_of(cx, body, &cells);
        }
        1 => trends(cx, body),
        at => breakdown::draw(cx, body, at - PAGES, &held),
    }
}

/// `breakdown_in`: `stats`' [`Breakdown`] laid over the box under the head line of `area`, the
/// box an All Time page draws into.
pub fn breakdown_in(
    text: &mut TextRenderer,
    theme: &Theme,
    lang: Lang,
    stats: &Stats,
    from: StatsFrom,
    today: i64,
    area: Rect,
) -> Breakdown {
    let (_, body) = area.split_top(edge_height(theme));
    Breakdown::of(text, theme, lang, stats, from, today, body)
}

/// `centred`: `name` centred on `line`, ‹ at its left edge where `back` and › at its right
/// where `on`, each with a sixth of the line to tap.
fn centred(cx: &mut Ctx, line: Rect, name: &str, back: bool, on: bool) {
    let theme: &Theme = cx.theme;
    let script = cx.ui_script();
    cx.text.set_px(theme.small_px);
    let baseline = line.y + cx.text.cap_height() as i32;
    let w = cx.text.measure_width_in(script, name) as i32;
    let x = line.x + (line.w - w) / 2;
    cx.text.draw_in(script, cx.fb, x, baseline, name, false);
    cx.text.set_px(theme.head_px);
    let reach = line.w / 6;
    if back {
        cx.text
            .draw_in(script, cx.fb, line.x, baseline, BACK, false);
        cx.hit(Hit::Prev, Rect::new(line.x, line.y, reach, line.h));
    }
    if on {
        let w = cx.text.measure_width_in(script, FORWARD) as i32;
        cx.text
            .draw_in(script, cx.fb, line.right() - w, baseline, FORWARD, false);
        let at = line.right() - reach;
        cx.hit(Hit::Next, Rect::new(at, line.y, reach, line.h));
    }
}

/// `BACK`: the arrows onto the page either side, each its own hit box.
const BACK: &str = "‹";
const FORWARD: &str = "›";

/// `edge_height`: the height of the line at the head of a page.
fn edge_height(theme: &Theme) -> i32 {
    theme.small_px as i32 + theme.gap * 2
}

/// `edges`: a marker at each end of `line`: an arrow onto the page either side, and the page's own
/// name where it has one.
fn edges(cx: &mut Ctx, line: Rect, left: Option<&str>, right: Option<&str>) {
    let theme: &Theme = cx.theme;
    let script = cx.ui_script();
    // `baseline`: one for both ends, off the type the line is set in.
    cx.text.set_px(theme.small_px);
    let baseline = line.y + cx.text.cap_height() as i32;
    for (said, at_left) in [(left, true), (right, false)] {
        let Some(said) = said else { continue };
        // `arrow`: an arrow takes `Theme::head_px`; a name takes the line's own type.
        let arrow = said == BACK || said == FORWARD;
        cx.text.set_px(match arrow {
            true => theme.head_px,
            false => theme.small_px,
        });
        let w = cx.text.measure_width_in(script, said) as i32;
        let x = match at_left {
            true => line.x,
            false => line.right() - w,
        };
        cx.text.draw_in(script, cx.fb, x, baseline, said, false);
        if arrow {
            // `reach`: the hit box reaches past the arrow's own width.
            let reach = line.w / 6;
            let at = match at_left {
                true => line.x,
                false => line.right() - reach,
            };
            let hit = match at_left {
                true => Hit::Prev,
                false => Hit::Next,
            };
            cx.hit(hit, Rect::new(at, line.y, reach, line.h));
        }
    }
}

/// `trends`: the three folds over the sitting histogram, in equal bands down the page.
fn trends(cx: &mut Ctx, area: Rect) {
    let rows = area.rows(BANDS, cx.theme.gap * 3);

    let day = cx.stats.average_day(cx.today);
    let names: Vec<String> = (0..24).map(|at| format!("{at:02}")).collect();
    band(cx, rows[0], cx.s().an_average_day, &day, &names, 3, true);

    let week = cx.stats.average_week(cx.today, cx.week);
    let names: Vec<String> = (0..7)
        .map(|at| cx.s().weekdays_short[cx.week.day_in(at)].to_string())
        .collect();
    band(cx, rows[1], cx.s().an_average_week, &week, &names, 1, true);

    let year = cx.stats.by_month(cx.today);
    let names: Vec<String> = cx.s().months_short.iter().map(|m| m.to_string()).collect();
    band(cx, rows[2], cx.s().by_month, &year, &names, 1, true);

    sittings(cx, rows[3]);
}

/// `band`: one fold under its own heading: the name, what one turn of the cycle averages, and the
/// fullest bucket.
pub(super) fn band(
    cx: &mut Ctx,
    area: Rect,
    name: &str,
    fold: &Fold,
    axis: &[String],
    every: usize,
    rule: bool,
) {
    let theme: &Theme = cx.theme;
    let s = cx.s();
    let short = format!("{name} · {}", date::duration(fold.each, s));
    let title = match fold.busiest {
        Some(at) => format!("{short} · {} {}", s.most, axis[at]),
        None => short.clone(),
    };
    // `title` names the fullest bucket only where the row has the width for it.
    cx.text.set_px(theme.small_px);
    let title = match cx.text.measure_width(&title) as i32 <= area.w {
        true => title,
        false => short,
    };
    let inner = match rule {
        true => chrome::section(cx.fb, cx.text, theme, area, &title),
        false => chrome::heading_stating(cx.fb, cx.text, theme, area, &title, None),
    };
    let names = axis.to_vec();
    charts::columns(
        cx.fb,
        cx.text,
        theme,
        cx.palette,
        inner,
        &fold.values,
        |at| names[at].clone(),
        &|secs| duration_rows(secs, s),
        every,
        fold.busiest,
        None,
        &charts::Breaks::NONE,
    );
}

/// `duration_rows`: a duration over its bar, as the rows it is set in: the hour and minute parts
/// stack: `2h 22m` fits the width of `22m`; a bucket under a minute states nothing.
pub fn duration_rows(secs: i64, s: &Strings) -> Vec<String> {
    if secs < 60 {
        return Vec::new();
    }
    let space = if s.unit_space { " " } else { "" };
    // `hours`: Rounded, as [`date::duration`] is.
    let (hours, mins) = date::hours_and_minutes(secs);
    let hour = format!("{hours}{space}{}", s.hours);
    let min = format!("{mins}{space}{}", s.minutes);
    match (hours, mins) {
        (0, _) => vec![min],
        (_, 0) => vec![hour],
        _ => vec![hour, min],
    }
}

/// `sittings`: how many sittings of the record ran each length, five minutes to a band.
fn sittings(cx: &mut Ctx, area: Rect) {
    let theme: &Theme = cx.theme;
    let s = cx.s();
    let counted = cx.stats.sitting_bands();
    let total: i64 = counted.iter().sum();
    let (first, floor) = (cx.stats.first_band(), cx.stats.sitting_floor);
    let names: Vec<String> = (0..counted.len())
        .map(|at| sitting_name(at, first, floor, s))
        .collect();
    let busiest = counted
        .iter()
        .enumerate()
        .max_by_key(|(_, n)| **n)
        .filter(|(_, n)| **n > 0)
        .map(|(at, _)| at);
    let head = format!("{} · {total} {}", s.sitting_lengths, s.in_all);
    let title = match busiest {
        Some(at) => format!("{head} · {} {}", s.most, names[at]),
        None => head,
    };
    let inner = chrome::section(cx.fb, cx.text, theme, area, &title);
    let axis = names.clone();
    charts::columns(
        cx.fb,
        cx.text,
        theme,
        cx.palette,
        inner,
        &counted,
        |at| axis[at].clone(),
        &|n| match n {
            0 => Vec::new(),
            n => vec![n.to_string()],
        },
        6,
        busiest,
        None,
        &charts::Breaks::NONE,
    );
}

/// `sitting_name`: the length the band at `at` opens at, counting from `first`.
pub fn sitting_name(at: usize, first: usize, floor: SittingFloor, s: &Strings) -> String {
    // `floor.label` names the band the scale opens on.
    if at == 0 {
        return floor.label(s);
    }
    let band = first + at;
    let secs = band as i64 * SITTING_STEP_SECS;
    let opens = match secs % 3600 {
        0 => format!("{}{}", secs / 3600, s.hours),
        _ => date::duration_tight(secs, s),
    };
    match band + 1 == SITTING_BANDS {
        true => format!("{opens}+"),
        false => opens,
    }
}

/// `board`: the twelve figures of the board, row by row: what each states and what it is called;
/// the board states what the whole record came to by `today`.
pub fn board(
    stats: &Stats,
    today: i64,
    week: WeekStart,
    s: &Strings,
) -> [(String, &'static str); 12] {
    let all = stats.tally(stats.opened(today)..=today);
    let best = stats
        .busiest_day(i64::MIN..=i64::MAX)
        .map_or(0, |(_, secs)| secs);
    let sat = stats.longest_sitting().map_or(0, |(_, secs)| secs);
    [
        (date::duration_coarse(all.read, s), s.total_read),
        (all.days_read.to_string(), s.days_read),
        (date::duration(all.a_day, s), s.a_day),
        (stats.book_count().to_string(), s.book_count),
        (all.finished.to_string(), s.finished),
        (a_book(stats.a_book(), s), s.a_book),
        (stats.current_streak.to_string(), s.current_streak),
        (stats.longest_streak.to_string(), s.longest_streak),
        (
            stats.weeks_running(today, week).to_string(),
            s.weeks_running,
        ),
        (date::duration(best, s), s.best_day),
        (date::duration(sat, s), s.longest_sitting),
        (date::duration(stats.a_sitting(), s), s.a_sitting),
    ]
}

/// [`board`]'s figures, three of them opening a page.
fn cells(cx: &Ctx) -> Vec<Cell> {
    let finished = cx.stats.finished_in(i64::MIN..=i64::MAX).next().is_some();
    let (best_day, best_day_secs) = best_day(cx);
    let (sat_on, sat_secs) = longest_sitting(cx);
    // `opens`: the page each figure opens, in the board's order.
    let opens = [
        None,
        None,
        None,
        None,
        finished.then_some(Hit::Shelved(Shelf::Finished, None)),
        None,
        None,
        None,
        None,
        (best_day_secs > 0).then_some(Hit::Day(best_day)),
        (sat_secs > 0).then_some(Hit::Day(sat_on)),
        None,
    ];
    board(cx.stats, cx.today, cx.week, cx.s())
        .into_iter()
        .zip(opens)
        .map(|((value, label), opens)| Cell {
            value,
            label,
            opens,
        })
        .collect()
}

/// `a_book`: what a finished book took, off [`crate::stats::Stats::a_book`]; a record with no book
/// read through states no average.
fn a_book(secs: Option<i64>, s: &Strings) -> String {
    match secs {
        Some(secs) => date::duration_coarse(secs, s),
        None => "—".into(),
    }
}

/// `best_day`: the fullest day of the record, and what was read on it.
fn best_day(cx: &Ctx) -> (i64, i64) {
    cx.stats
        .busiest_day(i64::MIN..=i64::MAX)
        .unwrap_or((cx.today, 0))
}

/// `longest_sitting`: the longest single sitting of the record, and the day it fell on.
fn longest_sitting(cx: &Ctx) -> (i64, i64) {
    cx.stats.longest_sitting().unwrap_or((cx.today, 0))
}

/// `span_line`: what the record covers, over the board.
fn span_line(cx: &mut Ctx, area: Rect) {
    let line = since(cx.stats, cx.today, cx.s());
    cx.text.set_px(cx.theme.small_px);
    let baseline = area.y + cx.text.cap_height() as i32;
    let script = cx.ui_script();
    cx.text
        .draw_in(script, cx.fb, area.x, baseline, &line, false);
}

/// `since`: the month the record opens in and the days from its first to `today`, as the head of
/// the board states them.
pub fn since(stats: &Stats, today: i64, s: &Strings) -> String {
    let first = stats.opened(today);
    let over = (today - first + 1).max(1);
    let (year, month, _) = date::civil_from_days(first);
    crate::lang::counted(s.since_days, over)
        .replace("{m}", &date::month_name(year, month, s).to_uppercase())
}

/// `cells` in rows of three down `area`, each row at its own size; every figure is centred in a
/// fixed third, and the three column centres hold across rows of unequal size.
fn board_of(cx: &mut Ctx, area: Rect, cells: &[Cell]) {
    let theme: &Theme = cx.theme;
    let rows = area.rows(ROWS, 0);
    let column_w = area.w / 3 - theme.gap * 2;

    for (line, band) in cells.chunks(3).zip(&rows) {
        let px = fitting_px(cx, line, column_w);
        for (cell, box_) in line.iter().zip(band.columns(3, 0)) {
            figure(cx, box_, cell, px);
            if let Some(hit) = cell.opens {
                cx.hit(hit, box_);
            }
        }
    }
    for row in rows.iter().skip(1) {
        paint::hline(cx.fb, area.x, row.y, area.w, PALE, 1);
    }
}

/// `fitting_px`: the largest size at or under [`Theme::display_px`] that sets every value in
/// `cells` inside one column.
fn fitting_px(cx: &mut Ctx, cells: &[Cell], column_w: i32) -> f32 {
    let theme: &Theme = cx.theme;
    let mut px = theme.display_px;
    while px > theme.body_px {
        cx.text.set_px(px);
        let widest = cells
            .iter()
            .map(|cell| cx.text.measure_solid(&cell.value) as i32)
            .max()
            .unwrap_or(0);
        if widest <= column_w {
            break;
        }
        // `px`: a width is near enough proportional to `px` to land in one step; the pixel taken
        // off it settles the rounding.
        px = (px * column_w as f32 / widest.max(1) as f32)
            .min(px - 1.0)
            .max(theme.body_px);
    }
    px
}

/// `figure`: one figure centred in `box_`, its name under it; a name carrying [`Cell::opens`] is
/// underlined.
fn figure(cx: &mut Ctx, box_: Rect, cell: &Cell, px: f32) {
    let theme: &Theme = cx.theme;
    cx.text.set_px(px);
    let cap = cx.text.cap_height() as i32;
    cx.text.set_px(theme.small_px);
    let line = cx.text.line_height() as i32;
    let block = cap + theme.gap + line;
    let top = box_.y + (box_.h - block).max(0) / 2 + cap;
    let script = cx.ui_script();

    cx.text.set_px(px);
    let w = cx.text.measure_solid(&cell.value) as i32;
    cx.text
        .draw_solid(cx.fb, box_.x + (box_.w - w) / 2, top, &cell.value, false);

    cx.text.set_px(theme.small_px);
    let lw = cx.text.measure_width_in(script, cell.label) as i32;
    let at = box_.x + (box_.w - lw) / 2;
    let baseline = top + theme.gap + line;
    cx.text
        .draw_in(script, cx.fb, at, baseline, cell.label, false);
    if cell.opens.is_some() {
        paint::hline(cx.fb, at, baseline + theme.gap / 2, lw, LIGHT, theme.rule());
    }
}
