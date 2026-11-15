//! [`Breakdown::of`]: All Time's Breakdown, its cards (Authors, Publishers, the language card,
//! one card per type) laid over pages; [`draw`] draws one page of them and [`whole`] one card's
//! every bar, paged; [`Hit::Bar`] opens Books narrowed to a bar's value.

use std::ops::RangeInclusive;

use crate::date;
use crate::font::Script;
use crate::lang::{Lang, Strings, counted};
use crate::metadata::By;
use crate::metadata::breakdown::{self, Bar, Languages, Ranked, Ranks};
use crate::settings::StatsFrom;
use crate::stats::Stats;
use crate::ui::paint::{self, DARK, LIGHT, PALE, Rect};
use crate::ui::text::TextRenderer;
use crate::ui::theme::Theme;
use crate::ui::{charts, chrome};

use super::{Ctx, Hit, pager};

/// `TOP`: the bars a card shows where every card fits one page at it, or over several pages.
const TOP: usize = 10;
/// `SHORT`: the bars a card shows where every card fits one page at it and not at [`TOP`].
const SHORT: usize = 5;

/// `NOTE_LINES`: the most lines a card's note wraps to.
const NOTE_LINES: usize = 3;

/// `CHART_ROWS`: the rows the months' columns take on the language card, the axis among them.
const CHART_ROWS: i32 = 5;

/// `COLUMN_ROWS`: the rows a type of numbers' columns take, the axis among them.
const COLUMN_ROWS: i32 = 6;

/// `NUMBERS`: the most columns a type of numbers draws.
const NUMBERS: usize = 41;

/// `Card`: one card: ranked bars, or the language card.
pub enum Card {
    Ranked(Ranked),
    Languages(Languages),
}

/// `Whole`: the card at `card` open on a page of its own, at its page `page`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Whole {
    pub card: usize,
    pub page: usize,
}

/// `Breakdown`: the cards, Authors, Publishers, the language card, then each type; `total`, the
/// seconds the books read; `order`, the languages by time read; `months`, the record's months;
/// `shown`, the bars a card shows on a page; `pages`, each page's cards by their place.
#[derive(Default)]
pub struct Breakdown {
    cards: Vec<Card>,
    total: i64,
    order: Vec<String>,
    months: Vec<RangeInclusive<i64>>,
    shown: usize,
    pages: Vec<Vec<usize>>,
}

/// `Item`: one row of a card opened whole: a bar, the bar of books holding no value, or a
/// line of its note.
enum Item {
    Bar(usize),
    Unset,
    Note(String),
}

impl Breakdown {
    /// `of`: `stats`' cards laid over pages of `body`, read off `from`'s seconds up to `today`
    /// and measured in `lang`; no card and no page where the record holds no `d` or `l` row.
    pub fn of(
        text: &mut TextRenderer,
        theme: &Theme,
        lang: Lang,
        stats: &Stats,
        from: StatsFrom,
        today: i64,
        body: Rect,
    ) -> Breakdown {
        if !stats.has_metadata() {
            return Breakdown::default();
        }
        let seconds = |at: usize| stats.books[at].read_seconds(from);
        let read = breakdown::read(stats, None, seconds, |at| stats.values(at));
        let every: Vec<_> = (0..stats.books.len()).map(|at| stats.values(at)).collect();
        let months = months(stats, today);
        let (types, named): (Vec<Ranked>, Vec<Ranked>) = breakdown::ranked(&read, &stats.linked)
            .into_iter()
            .partition(|card| matches!(card.ranks, Ranks::Type(_)));
        let mut cards: Vec<Card> = named.into_iter().map(Card::Ranked).collect();
        let languages = breakdown::languages(stats, &read, &every, &stats.linked, &months);
        cards.extend(languages.map(Card::Languages));
        cards.extend(types.into_iter().map(Card::Ranked));
        let mut out = Breakdown {
            cards,
            total: read.iter().map(|(_, secs, _)| secs).sum(),
            order: breakdown::language_order(&every, &stats.linked, seconds),
            months,
            shown: TOP,
            pages: Vec::new(),
        };
        out.lay(text, theme, lang, body);
        out
    }

    /// `pages`: how many pages the cards take.
    pub fn pages(&self) -> usize {
        self.pages.len()
    }

    /// `name`: the head line of the page `at`, numbered where there are several.
    pub fn name(&self, s: &Strings, at: usize) -> String {
        let name = s.breakdown.to_uppercase();
        match self.pages.len() {
            0 | 1 => name,
            n => format!("{name} {}/{n}", at + 1),
        }
    }

    /// `by`: the value the bar `bar` of the card at `card` stands for, `None` the card's books
    /// holding no value; `None` where the card holds no such bar.
    pub fn by(&self, card: usize, bar: Option<usize>) -> Option<By> {
        match self.cards.get(card)? {
            Card::Ranked(c) => {
                let name = match bar {
                    Some(n) => Some(c.bars.get(n)?.name.clone()?),
                    None => {
                        c.unset.as_ref()?;
                        None
                    }
                };
                Some(match &c.ranks {
                    Ranks::Authors => By::Author(name),
                    Ranks::Publishers => By::Publisher(name),
                    Ranks::Type(t) => By::Tag(t.clone(), name),
                })
            }
            Card::Languages(l) => {
                let (tag, _) = l.parts.get(bar?)?;
                let tag = tag.as_ref().map(|t| t.tag.clone());
                Some(By::Tag(l.r#type.clone(), tag))
            }
        }
    }

    /// `whole_pages`: the pages the card at `card` takes opened whole in `page`.
    pub fn whole_pages(
        &self,
        text: &mut TextRenderer,
        theme: &Theme,
        lang: Lang,
        card: usize,
        page: Rect,
    ) -> usize {
        let Some(Card::Ranked(c)) = self.cards.get(card) else {
            return 0;
        };
        let deep = whole_rows(text, theme, page);
        items(text, theme, lang, c, page.w).len().div_ceil(deep)
    }

    /// `lay` sets `shown` and `pages` for cards drawn into `body`: every card on one page at
    /// [`TOP`] bars where they fit, else at [`SHORT`], else whole cards over pages at [`TOP`].
    fn lay(&mut self, text: &mut TextRenderer, theme: &Theme, lang: Lang, body: Rect) {
        if self.cards.is_empty() {
            return;
        }
        let air = theme.gap * 3;
        for k in [TOP, SHORT] {
            let heights = self.heights(text, theme, lang, k, body.w);
            let sum: i32 = heights.iter().sum::<i32>() + air * (heights.len() as i32 - 1);
            if sum <= body.h {
                self.shown = k;
                self.pages = vec![(0..heights.len()).collect()];
                return;
            }
        }
        self.shown = TOP;
        let mut pages: Vec<Vec<usize>> = vec![Vec::new()];
        let mut y = 0;
        for (at, h) in self
            .heights(text, theme, lang, TOP, body.w)
            .into_iter()
            .enumerate()
        {
            let page = pages.last_mut().expect("a page");
            if !page.is_empty() && y + h > body.h {
                pages.push(vec![at]);
                y = h + air;
            } else {
                page.push(at);
                y += h + air;
            }
        }
        self.pages = pages;
    }

    /// `heights`: each card's height at `k` bars across `w`.
    fn heights(
        &self,
        text: &mut TextRenderer,
        theme: &Theme,
        lang: Lang,
        k: usize,
        w: i32,
    ) -> Vec<i32> {
        self.cards
            .iter()
            .map(|card| self.card_height(text, theme, lang, card, k, w))
            .collect()
    }

    /// `card_height`: `card`'s height at `k` bars across `w`: its heading, its rows, its note,
    /// and on the language card the share strip and the months.
    fn card_height(
        &self,
        text: &mut TextRenderer,
        theme: &Theme,
        lang: Lang,
        card: &Card,
        k: usize,
        w: i32,
    ) -> i32 {
        let s = lang.strings();
        let ui = Script::of_language(lang.language_tag());
        let head = chrome::section_height(text, theme);
        let row = row_height(text, theme);
        let doubled = |text: &mut TextRenderer, c: &Ranked| match c.doubled {
            true => note(text, theme, ui, s.counts_each, w).len(),
            false => 0,
        };
        match card {
            Card::Ranked(c) if c.numbers => {
                let rows = usize::from(c.unset.is_some()) + doubled(text, c);
                head + row * COLUMN_ROWS + rows as i32 * row
            }
            Card::Ranked(c) => {
                let rows = c.bars.len().min(k) + usize::from(c.unset.is_some());
                let more = usize::from(c.bars.len() > k);
                head + (rows + more + doubled(text, c)) as i32 * row
            }
            Card::Languages(l) => {
                let note = match l.split {
                    true => note(text, theme, ui, s.split_each, w).len(),
                    false => 0,
                };
                let chart = match self.months.len() > 1 {
                    true => row * CHART_ROWS + theme.gap * 2,
                    false => 0,
                };
                head + strip_height(theme) + (l.parts.len() + note) as i32 * row + chart
            }
        }
    }
}

/// `months`: the months from the record's first day to `today`'s, each cut to that run.
fn months(stats: &Stats, today: i64) -> Vec<RangeInclusive<i64>> {
    let first = stats.opened(today);
    let (mut y, mut m, _) = date::civil_from_days(first);
    let (last_y, last_m, _) = date::civil_from_days(today);
    let mut out = Vec::new();
    while (y, m) <= (last_y, last_m) {
        let start = date::days_from_civil(y, m, 1);
        let end = date::days_from_civil(y, m, date::days_in_month(y, m));
        out.push(start.max(first)..=end.min(today));
        (y, m) = match m {
            12 => (y + 1, 1),
            _ => (y, m + 1),
        };
    }
    out
}

/// `row_height`: one row of a card, set at [`Theme::small_px`].
fn row_height(text: &mut TextRenderer, theme: &Theme) -> i32 {
    text.set_px(theme.small_px);
    text.line_height() as i32 + theme.gap / 2
}

/// `strip_height`: the language card's share strip and the air under it.
fn strip_height(theme: &Theme) -> i32 {
    theme.small_px as i32 + theme.gap
}

/// `note`: `said` wrapped across `w`, at most [`NOTE_LINES`] lines.
fn note(text: &mut TextRenderer, theme: &Theme, ui: Script, said: &str, w: i32) -> Vec<String> {
    text.set_px(theme.small_px);
    text.wrap_and_clamp_in(ui, said, w.max(1) as u32, NOTE_LINES)
}

/// `draw`: the Breakdown page `at` of `held` into `body`, each card a [`TOP`] or [`SHORT`] bars,
/// the cards [`Theme::gap`] three times apart.
pub fn draw(cx: &mut Ctx, body: Rect, at: usize, held: &Breakdown) {
    let air = cx.theme.gap * 3;
    let mut y = body.y;
    for n in held.pages.get(at).into_iter().flatten().copied() {
        let card = &held.cards[n];
        let h = held.card_height(cx.text, cx.theme, cx.lang, card, held.shown, body.w);
        let box_ = Rect::new(body.x, y, body.w, h);
        match card {
            Card::Ranked(c) if c.numbers => numbers_card(cx, box_, n, c, held),
            Card::Ranked(c) => ranked_card(cx, box_, n, c, held),
            Card::Languages(l) => languages_card(cx, box_, n, l, held),
        }
        y += h + air;
    }
}

/// `numbers_card`: the card at `n` of `held`, a type whose every value is a whole number, in
/// `area`: a column a number from the lowest (1 at most) to the highest, at most [`NUMBERS`],
/// each holding books a [`Hit::Bar`], the unset bar stated under them, then the note.
fn numbers_card(cx: &mut Ctx, area: Rect, n: usize, card: &Ranked, held: &Breakdown) {
    let theme: &Theme = cx.theme;
    let s = cx.s();
    let most_name = card
        .bars
        .first()
        .and_then(|b| b.name.as_deref())
        .unwrap_or_default();
    let said = stated(held.total, most_name, s);
    let inner =
        chrome::section_stating(cx.fb, cx.text, theme, area, &heading(card, s), Some(&said));
    let numbered: Vec<(i64, usize)> = card
        .bars
        .iter()
        .enumerate()
        .filter_map(|(at, b)| Some((b.name.as_deref()?.parse::<i64>().ok()?, at)))
        .collect();
    let low = numbered.iter().map(|(v, _)| *v).min().unwrap_or(0).min(1);
    let high = numbered.iter().map(|(v, _)| *v).max().unwrap_or(0);
    let high = high.min(low + NUMBERS as i64 - 1).max(low);
    let bar_of = |v: i64| numbered.iter().find(|(m, _)| *m == v).map(|(_, at)| *at);
    let values: Vec<i64> = (low..=high)
        .map(|v| bar_of(v).map_or(0, |at| card.bars[at].secs))
        .collect();
    let busiest = values
        .iter()
        .enumerate()
        .max_by_key(|(at, secs)| (**secs, std::cmp::Reverse(*at)))
        .filter(|(_, secs)| **secs > 0)
        .map(|(at, _)| at);
    let row = row_height(cx.text, theme);
    let chart = Rect::new(inner.x, inner.y, inner.w, row * COLUMN_ROWS);
    let count = values.len() as i32;
    let gap = charts::cell_gap(theme, values.len());
    cx.text.set_px(theme.small_px);
    let line = cx.text.line_height() as i32;
    let widest = cx.text.measure_width(&high.to_string()) as i32;
    let cell_w = ((chart.w - gap * (count - 1)) / count.max(1)).max(1);
    let every = ((widest + theme.gap) / (cell_w + gap) + 1).max(1) as usize;
    charts::columns(
        cx.fb,
        cx.text,
        theme,
        cx.palette,
        chart,
        &values,
        |at| (low + at as i64).to_string(),
        &|secs| super::alltime::duration_rows(secs, s),
        every,
        busiest,
        None,
        &charts::Breaks::NONE,
    );
    let band = chart.split_top((chart.h - line - theme.gap / 2).max(1)).0;
    for (at, cell) in band.columns(count, gap).iter().enumerate() {
        if let Some(bar) = bar_of(low + at as i64) {
            let tap = Rect::new(cell.x, chart.y, cell.w, chart.h);
            cx.hit(Hit::Bar(n, Some(bar)), tap);
        }
    }
    let ui = cx.ui_script();
    let cap = cx.text.cap_height() as i32;
    let mut y = chart.bottom();
    if let Some(bar) = &card.unset {
        let books = counted(s.n_books, bar.books as i64);
        let line = format!("{} · {} · {books}", s.not_set, date::duration(bar.secs, s));
        let baseline = y + row / 2 + cap / 2;
        cx.text.draw_in(ui, cx.fb, inner.x, baseline, &line, false);
        cx.hit(Hit::Bar(n, None), Rect::new(inner.x, y, inner.w, row));
        y += row;
    }
    if card.doubled {
        for line in note(cx.text, theme, ui, s.counts_each, inner.w) {
            let baseline = y + row / 2 + cap / 2;
            cx.text
                .draw_inked(ui, cx.fb, inner.x, baseline, &line, DARK);
            y += row;
        }
    }
}

/// `heading`: the name a ranked card is headed by.
fn heading(card: &Ranked, s: &Strings) -> String {
    match &card.ranks {
        Ranks::Authors => s.authors_head.to_string(),
        Ranks::Publishers => s.publishers_head.to_string(),
        Ranks::Type(t) => t.to_uppercase(),
    }
}

/// `stated`: what a card's heading states at its right: the time read and the most read name.
fn stated(total: i64, most: &str, s: &Strings) -> String {
    format!("{} · {} {most}", date::duration(total, s), s.most)
}

/// `script_of`: the script `bar`'s name is set in: an author's first book's language, read
/// with the name, and the interface's for any other value.
fn script_of(cx: &Ctx, bar: &Bar, ranks: &Ranks) -> Script {
    let hint = match ranks {
        Ranks::Authors => Script::of_language(&cx.stats.books[bar.first].language),
        _ => cx.ui_script(),
    };
    Script::resolve(hint, bar.name.as_deref().unwrap_or_default())
}

/// `Ink`: what a bar is filled with: the scheme's mark on the most read, its bar colour on the
/// rest, [`LIGHT`] on the books holding no value.
#[derive(Clone, Copy)]
enum Ink {
    Most,
    Bar,
    Unset,
}

/// `Line`: one ranked row: `name` in `script`, its seconds and books, and its [`Ink`].
struct Line<'a> {
    name: &'a str,
    script: Script,
    secs: i64,
    books: usize,
    ink: Ink,
}

/// `ranked_row`: `line` across `row`: its name in the first 30 %, its bar to `most`'s scale,
/// its time, and its books at the right in [`DARK`].
fn ranked_row(cx: &mut Ctx, row: Rect, line: &Line, most: i64) {
    let theme: &Theme = cx.theme;
    let s = cx.s();
    cx.text.set_px(theme.small_px);
    let books_col = cx.text.measure_width(&counted(s.n_books, 888)) as i32;
    let time_col = cx
        .text
        .measure_width(&date::duration(888 * 3600 + 59 * 60, s)) as i32;
    let name_w = row.w * 30 / 100;
    let room = row.w - name_w - time_col - books_col - theme.gap * 4;
    let baseline = row.center_y() + cx.text.cap_height() as i32 / 2;
    let cut = cx
        .text
        .wrap_and_clamp_in(line.script, line.name, name_w.max(1) as u32, 1);
    let cut = cut.first().map(String::as_str).unwrap_or_default();
    cx.text
        .draw_in(line.script, cx.fb, row.x, baseline, cut, false);
    let bh = (cx.text.cap_height() as i32 * 4 / 3).min(row.h - 2);
    let bw = ((room.max(1) as i64 * line.secs / most.max(1)) as i32).max(1);
    let bar = Rect::new(row.x + name_w + theme.gap, row.center_y() - bh / 2, bw, bh);
    match line.ink {
        Ink::Most => paint::fill_rgb(cx.fb, bar, cx.palette.mark),
        Ink::Bar => paint::fill_rgb(cx.fb, bar, cx.palette.bar()),
        Ink::Unset => paint::fill(cx.fb, bar, LIGHT),
    }
    let time = date::duration(line.secs, s);
    let tw = cx.text.measure_width(&time) as i32;
    let books_x = row.right() - books_col;
    cx.text
        .draw(cx.fb, books_x - theme.gap * 2 - tw, baseline, &time, false);
    let books = counted(s.n_books, line.books as i64);
    let w = cx.text.measure_width(&books) as i32;
    cx.text.draw_inked(
        Script::Unknown,
        cx.fb,
        row.right() - w,
        baseline,
        &books,
        DARK,
    );
}

/// `most_of`: the longest of `card`'s bars and its unset one, the scale its bars are drawn to.
fn most_of(card: &Ranked) -> i64 {
    card.bars
        .iter()
        .chain(&card.unset)
        .map(|b| b.secs)
        .max()
        .unwrap_or(1)
}

/// `bar_line`: the bar at `n` of `card` as a [`Line`], the first in [`Ink::Most`].
fn bar_line<'a>(cx: &Ctx, card: &'a Ranked, n: usize) -> Line<'a> {
    let bar = &card.bars[n];
    Line {
        name: bar.name.as_deref().unwrap_or_default(),
        script: script_of(cx, bar, &card.ranks),
        secs: bar.secs,
        books: bar.books,
        ink: match n {
            0 => Ink::Most,
            _ => Ink::Bar,
        },
    }
}

/// `unset_line`: `card`'s bar of books holding no value as a [`Line`], named `s.not_set`.
fn unset_line<'a>(cx: &Ctx, bar: &Bar, s: &'a Strings) -> Line<'a> {
    Line {
        name: s.not_set,
        script: cx.ui_script(),
        secs: bar.secs,
        books: bar.books,
        ink: Ink::Unset,
    }
}

/// `ranked_card`: the card at `n` of `held` in `area`: its heading, its first `held.shown`
/// bars and its unset one, each a [`Hit::Bar`], `s.show_all` opening it whole where it holds
/// more, and its note.
fn ranked_card(cx: &mut Ctx, area: Rect, n: usize, card: &Ranked, held: &Breakdown) {
    let theme: &Theme = cx.theme;
    let s = cx.s();
    let most_name = card
        .bars
        .first()
        .and_then(|b| b.name.as_deref())
        .unwrap_or_default();
    let said = stated(held.total, most_name, s);
    let inner =
        chrome::section_stating(cx.fb, cx.text, theme, area, &heading(card, s), Some(&said));
    let row = row_height(cx.text, theme);
    let most = most_of(card);
    let mut y = inner.y;
    for at in 0..card.bars.len().min(held.shown) {
        let r = Rect::new(inner.x, y, inner.w, row);
        let line = bar_line(cx, card, at);
        ranked_row(cx, r, &line, most);
        cx.hit(Hit::Bar(n, Some(at)), r);
        y += row;
    }
    if let Some(bar) = &card.unset {
        let r = Rect::new(inner.x, y, inner.w, row);
        let line = unset_line(cx, bar, s);
        ranked_row(cx, r, &line, most);
        cx.hit(Hit::Bar(n, None), r);
        y += row;
    }
    let ui = cx.ui_script();
    cx.text.set_px(theme.small_px);
    let cap = cx.text.cap_height() as i32;
    if card.bars.len() > held.shown {
        let more = format!(
            "{} ›",
            s.show_all.replace("{d}", &card.bars.len().to_string())
        );
        let baseline = y + row / 2 + cap / 2;
        cx.text.draw_in(ui, cx.fb, inner.x, baseline, &more, false);
        let w = cx.text.measure_width_in(ui, &more) as i32;
        paint::hline(
            cx.fb,
            inner.x,
            baseline + theme.gap / 2,
            w,
            LIGHT,
            theme.rule(),
        );
        cx.hit(Hit::Whole(n), Rect::new(inner.x, y, inner.w, row));
        y += row;
    }
    if card.doubled {
        for line in note(cx.text, theme, ui, s.counts_each, inner.w) {
            let baseline = y + row / 2 + cap / 2;
            cx.text
                .draw_inked(ui, cx.fb, inner.x, baseline, &line, DARK);
            y += row;
        }
    }
}

/// `language_ink`: the ink of the language `words`: the scheme's steps, darkest first, by
/// `order`, the record's languages in time read; `None` for the books holding none.
fn language_ink(cx: &Ctx, words: Option<&str>, order: &[String]) -> Option<[u8; 3]> {
    let at = order.iter().position(|w| Some(w.as_str()) == words)?;
    Some(cx.palette.steps[4usize.saturating_sub(at).max(1)])
}

/// `swatch` fills `at` with `ink`, [`PALE`] where it is `None`.
fn swatch(cx: &mut Ctx, at: Rect, ink: Option<[u8; 3]>) {
    match ink {
        Some(rgb) => paint::fill_rgb(cx.fb, at, rgb),
        None => paint::fill(cx.fb, at, PALE),
    }
}

/// `languages_card`: the language card at `n` of `held` in `area`: the share strip, a legend
/// row per language (each a [`Hit::Bar`]), the months as columns split by language, its note.
fn languages_card(cx: &mut Ctx, area: Rect, n: usize, card: &Languages, held: &Breakdown) {
    let theme: &Theme = cx.theme;
    let s = cx.s();
    let top = card
        .parts
        .iter()
        .find_map(|(t, _)| t.as_ref().map(|t| t.tag.as_str()))
        .unwrap_or_default();
    let said = stated(held.total, top, s);
    let inner = chrome::section_stating(
        cx.fb,
        cx.text,
        theme,
        area,
        &card.r#type.to_uppercase(),
        Some(&said),
    );
    let ink_of = |cx: &Ctx, at: usize| {
        let words = card.parts[at].0.as_ref().map(|t| t.tag.as_str());
        language_ink(cx, words, &held.order)
    };
    let strip_h = theme.small_px as i32;
    let sum: i64 = card.parts.iter().map(|(_, secs)| secs).sum::<i64>().max(1);
    let mut x = inner.x;
    for (at, (_, secs)) in card.parts.iter().enumerate() {
        let w = match at + 1 == card.parts.len() {
            true => inner.right() - x,
            false => (inner.w as i64 * secs / sum) as i32,
        };
        let ink = ink_of(cx, at);
        swatch(cx, Rect::new(x, inner.y, w.max(1), strip_h), ink);
        x += w;
    }
    let row = row_height(cx.text, theme);
    let mut y = inner.y + strip_height(theme);
    cx.text.set_px(theme.small_px);
    let cap = cx.text.cap_height() as i32;
    let share_col = cx.text.measure_width("100%") as i32;
    let ui = cx.ui_script();
    for (at, ((tag, secs), share)) in card.parts.iter().zip(&card.shares).enumerate() {
        let baseline = y + row / 2 + cap / 2;
        let ink = ink_of(cx, at);
        swatch(cx, Rect::new(inner.x, baseline - cap, cap, cap), ink);
        let name = tag.as_ref().map_or(s.not_set, |t| t.tag.as_str());
        let script = Script::resolve(ui, name);
        cx.text.draw_in(
            script,
            cx.fb,
            inner.x + cap + theme.gap * 2,
            baseline,
            name,
            false,
        );
        let pct = format!("{share}%");
        let w = cx.text.measure_width(&pct) as i32;
        cx.text.draw_inked(
            Script::Unknown,
            cx.fb,
            inner.right() - w,
            baseline,
            &pct,
            DARK,
        );
        let time = date::duration(*secs, s);
        let tw = cx.text.measure_width(&time) as i32;
        let time_x = inner.right() - share_col - theme.gap * 3 - tw;
        cx.text.draw(cx.fb, time_x, baseline, &time, false);
        cx.hit(Hit::Bar(n, Some(at)), Rect::new(inner.x, y, inner.w, row));
        y += row;
    }
    if held.months.len() > 1 {
        let chart = Rect::new(inner.x, y + theme.gap, inner.w, row * CHART_ROWS);
        months_chart(cx, chart, card, held);
        y = chart.bottom() + theme.gap;
    }
    if card.split {
        for line in note(cx.text, theme, ui, s.split_each, inner.w) {
            let baseline = y + row / 2 + cap / 2;
            cx.text
                .draw_inked(ui, cx.fb, inner.x, baseline, &line, DARK);
            y += row;
        }
    }
}

/// `months_chart`: `card`'s months in `chart`, a column a month split by language, its time over
/// each shrunk as far as 0.45 of [`Theme::small_px`] and then every `thin`th stated, both ends
/// among them; every `every`th month named under its column, January by its year.
fn months_chart(cx: &mut Ctx, chart: Rect, card: &Languages, held: &Breakdown) {
    let theme: &Theme = cx.theme;
    let s = cx.s();
    let row = row_height(cx.text, theme);
    let (stood, axis) = chart.split_top(chart.h - row);
    let months = &held.months;
    let step = stood.w / months.len() as i32;
    let bw = (step * 3 / 4).max(1);
    let sums: Vec<i64> = card.months.iter().map(|p| p.iter().sum::<i64>()).collect();
    let tallest = sums.iter().copied().max().unwrap_or(1).max(1);
    let figures: Vec<String> = sums.iter().map(|secs| date::duration(*secs, s)).collect();
    let floor = theme.small_px * 0.45;
    let mut px = theme.small_px;
    loop {
        cx.text.set_px(px);
        let widest = figures
            .iter()
            .map(|f| cx.text.measure_width(f) as i32)
            .max()
            .unwrap_or(0);
        if widest <= step - theme.gap / 2 || px <= floor {
            break;
        }
        px = (px * (step - theme.gap / 2) as f32 / widest.max(1) as f32)
            .min(px - 1.0)
            .max(floor);
    }
    let widest = figures
        .iter()
        .map(|f| cx.text.measure_width(f) as i32)
        .max()
        .unwrap_or(0);
    let thin = ((widest + theme.gap) / step.max(1) + 1).max(1) as usize;
    let figure_h = cx.text.line_height() as i32;
    let plot = Rect::new(stood.x, stood.y + figure_h, stood.w, stood.h - figure_h);
    for (at, figure) in figures.iter().enumerate() {
        let last = at + 1 == figures.len();
        if sums[at] <= 0 || !(at % thin == 0 || last) {
            continue;
        }
        let h = (plot.h as i64 * sums[at] / tallest) as i32;
        let w = cx.text.measure_width(figure) as i32;
        let x = plot.x + at as i32 * step + step / 2 - w / 2;
        cx.text
            .draw(cx.fb, x, plot.bottom() - h - theme.gap / 3, figure, false);
    }
    cx.text.set_px(theme.small_px);
    let cap = cx.text.cap_height() as i32;
    let every = ((cx.text.measure_width("Mmm") as i32 + theme.gap) / step.max(1) + 1) as usize;
    let ui = cx.ui_script();
    for (at, (parts, span)) in card.months.iter().zip(months).enumerate() {
        let x = plot.x + at as i32 * step + (step - bw) / 2;
        let mut top = plot.bottom();
        for ((tag, _), secs) in card.parts.iter().zip(parts) {
            let h = (plot.h as i64 * secs / tallest) as i32;
            if h <= 0 {
                continue;
            }
            let words = tag.as_ref().map(|t| t.tag.as_str());
            let ink = language_ink(cx, words, &held.order);
            swatch(cx, Rect::new(x, top - h, bw, h), ink);
            top -= h;
        }
        if at % every == 0 || at + 1 == months.len() {
            let (y, m, _) = date::civil_from_days(*span.start());
            let name = match m {
                1 => format!("{y}"),
                _ => s.months_short[(m - 1) as usize].to_string(),
            };
            let w = cx.text.measure_width_in(ui, &name) as i32;
            let baseline = axis.y + cap + theme.gap / 2;
            cx.text
                .draw_in(ui, cx.fb, x + bw / 2 - w / 2, baseline, &name, false);
        }
    }
    paint::hline(cx.fb, plot.x, plot.bottom(), plot.w, LIGHT, 1);
}

/// `whole_rows`: the rows a card opened whole holds a page in `page`, under its heading and
/// over the pager, never fewer than one.
fn whole_rows(text: &mut TextRenderer, theme: &Theme, page: Rect) -> usize {
    let row = row_height(text, theme);
    let inner = page.h - pager::height(theme) - chrome::section_height(text, theme);
    (inner / row.max(1)).max(1) as usize
}

/// `items`: the rows of `card` opened whole across `w`: every bar, the unset one, then its note.
fn items(text: &mut TextRenderer, theme: &Theme, lang: Lang, card: &Ranked, w: i32) -> Vec<Item> {
    let mut out: Vec<Item> = (0..card.bars.len()).map(Item::Bar).collect();
    if card.unset.is_some() {
        out.push(Item::Unset);
    }
    if card.doubled {
        let ui = Script::of_language(lang.language_tag());
        let said = lang.strings().counts_each;
        out.extend(note(text, theme, ui, said, w).into_iter().map(Item::Note));
    }
    out
}

/// `whole`: the card `open.card` of `held` opened whole in `page`, at its page `open.page`:
/// its heading, as many rows as the page holds, and along the foot `s.breakdown` back to the
/// page under it beside the card's own pager.
pub fn whole(cx: &mut Ctx, page: Rect, open: Whole, held: &Breakdown) {
    let Some(Card::Ranked(card)) = held.cards.get(open.card) else {
        return;
    };
    let theme: &Theme = cx.theme;
    let s = cx.s();
    let deep = whole_rows(cx.text, theme, page);
    let items = items(cx.text, theme, cx.lang, card, page.w);
    let pages = items.len().div_ceil(deep).max(1);
    let shown = open.page.min(pages - 1);
    let (_, body) = page.split_bottom(pager::height(theme));
    let most_name = card
        .bars
        .first()
        .and_then(|b| b.name.as_deref())
        .unwrap_or_default();
    let said = stated(held.total, most_name, s);
    let inner =
        chrome::section_stating(cx.fb, cx.text, theme, body, &heading(card, s), Some(&said));
    let row = row_height(cx.text, theme);
    let most = most_of(card);
    let ui = cx.ui_script();
    let from = shown * deep;
    let to = (from + deep).min(items.len());
    let mut y = inner.y;
    for item in &items[from..to] {
        let r = Rect::new(inner.x, y, inner.w, row);
        match item {
            Item::Bar(at) => {
                let line = bar_line(cx, card, *at);
                ranked_row(cx, r, &line, most);
                cx.hit(Hit::Bar(open.card, Some(*at)), r);
            }
            Item::Unset => {
                let Some(bar) = &card.unset else { continue };
                let line = unset_line(cx, bar, s);
                ranked_row(cx, r, &line, most);
                cx.hit(Hit::Bar(open.card, None), r);
            }
            Item::Note(line) => {
                cx.text.set_px(theme.small_px);
                let baseline = y + row / 2 + cx.text.cap_height() as i32 / 2;
                cx.text.draw_inked(ui, cx.fb, inner.x, baseline, line, DARK);
            }
        }
        y += row;
    }
    let counted = card.bars.len() + usize::from(card.unset.is_some());
    let label = match pages {
        1 => String::new(),
        _ => format!(
            "{}–{} {} {counted}",
            (from + 1).min(counted),
            to.min(counted),
            s.of
        ),
    };
    pager::draw_back(
        cx,
        pager::foot(theme, page),
        (s.breakdown, Hit::WholeClose),
        &label,
        [shown > 0, shown + 1 < pages],
    );
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::metadata::links::{Link, Linked, Links};
    use crate::metadata::{Set, Tag};
    use crate::stats::BookStat;
    use crate::ui::theme::Theme;

    /// `genre`: a `Set` holding the one tag `tag` of the type Genre.
    fn genre(tag: &str) -> Set {
        let mut set = Set::default();
        set.tags = vec![Tag {
            r#type: "Genre".into(),
            tag: tag.into(),
        }];
        set
    }

    /// `tagged`: three ten-minute books: Mystery, ミステリー linked with it, and none.
    fn tagged() -> Stats {
        let books = (0..3)
            .map(|at| BookStat {
                cde_key: format!("KEY{at}"),
                title: format!("Book {at}"),
                seconds: 600,
                sittings: 1,
                ..BookStat::default()
            })
            .collect();
        let mut stats = Stats {
            books,
            sets: vec![genre("Mystery"), genre("ミステリー"), Set::default()],
            ..Stats::default()
        };
        let values: Vec<_> = (0..3).map(|at| stats.values(at)).collect();
        let link = Link {
            edited: "2026-11-14T09:00:00".into(),
            linked: Linked::Tag,
            r#type: "Genre".into(),
            first: "Mystery".into(),
            names: Some(vec!["ミステリー".into()]),
        };
        stats.linked = Links::new(&[link], &values);
        stats
    }

    #[test]
    fn a_bar_stands_for_its_value_and_the_books_holding_none() {
        let Ok(mut text) = TextRenderer::load(24.0) else {
            return;
        };
        let theme = Theme::for_screen(1264, 1680);
        let body = chrome::content_box(&theme);
        let today = date::days_from_civil(2026, 11, 14);
        let stats = tagged();
        let held = Breakdown::of(
            &mut text,
            &theme,
            Lang::English,
            &stats,
            StatsFrom::default(),
            today,
            body,
        );
        assert_eq!(held.pages(), 1);
        let mystery = By::Tag("Genre".into(), Some("Mystery".into()));
        assert_eq!(held.by(0, Some(0)), Some(mystery));
        assert_eq!(held.by(0, None), Some(By::Tag("Genre".into(), None)));
        assert_eq!(held.by(0, Some(1)), None, "the card holds one bar");
        assert_eq!(held.by(1, Some(0)), None, "the page holds one card");
    }

    #[test]
    fn a_record_holding_no_row_of_the_desktops_has_no_breakdown_page() {
        let Ok(mut text) = TextRenderer::load(24.0) else {
            return;
        };
        let theme = Theme::for_screen(1264, 1680);
        let body = chrome::content_box(&theme);
        let mut stats = tagged();
        stats.sets.clear();
        let today = date::days_from_civil(2026, 11, 14);
        let held = Breakdown::of(
            &mut text,
            &theme,
            Lang::English,
            &stats,
            StatsFrom::default(),
            today,
            body,
        );
        assert_eq!(held.pages(), 0);
    }

    #[test]
    fn a_type_of_numbers_is_drawn_as_columns_and_its_bars_stand_for_its_numbers() {
        let Ok(mut text) = TextRenderer::load(24.0) else {
            return;
        };
        let theme = Theme::for_screen(1264, 1680);
        let body = chrome::content_box(&theme);
        let mut stats = tagged();
        for (set, rating) in stats.sets.iter_mut().zip(["4", "2", "4"]) {
            set.tags = vec![Tag {
                r#type: "Rating".into(),
                tag: rating.into(),
            }];
        }
        let today = date::days_from_civil(2026, 11, 14);
        let held = Breakdown::of(
            &mut text,
            &theme,
            Lang::English,
            &stats,
            StatsFrom::default(),
            today,
            body,
        );
        let Some(Card::Ranked(card)) = held.cards.first() else {
            panic!("a card of the Rating type");
        };
        assert!(card.numbers);
        let four = By::Tag("Rating".into(), Some("4".into()));
        assert_eq!(
            held.by(0, Some(0)),
            Some(four),
            "4, the most read, ranks first"
        );
        assert_eq!(held.by(0, None), None, "every book holds a rating");
    }
}
