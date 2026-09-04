//! The run loop: what is drawn, and what a touch does to it. [`App::draw`]
//! redraws the whole screen and presents it in one
//! [`Framebuffer::send_update`].

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc;
use std::time::{Duration, Instant};

use anyhow::Result;

use crate::date;
use crate::eink::fb::{Framebuffer, MxcfbRect, WAVEFORM_MODE_GC16};
use crate::eink::input::{Input, InputEvent};
use crate::eink::screenshot;
use crate::eink::touch::{SwipeDir, TouchEvent, classify_swipe};
use crate::lang::Lang;
use crate::settings::Settings;
use crate::stats::Stats;
use crate::ui::chrome::{self, Tab};
use crate::ui::cover::Covers;
use crate::ui::text::TextRenderer;
use crate::ui::theme::Theme;
use crate::update::{self, Doing, Outcome};
use crate::view::{self, Ctx, Hit, State};

/// The shortest time between two repaints of the update banner. Every one is a
/// whole-screen [`WAVEFORM_MODE_GC16`] and a percentage moves several times a
/// second; this is what keeps that from flashing the panel.
const BANNER_REDRAW: Duration = Duration::from_millis(700);

/// How long an update's last word stays up before the settings come back. A
/// tap ends it sooner.
const OUTCOME_LINGER: Duration = Duration::from_secs(12);

pub struct App {
    theme: Theme,
    /// The language drawn in: [`Settings::language`], else [`App::detected`].
    lang: Lang,
    settings: Settings,
    text: TextRenderer,
    covers: Covers,
    store: crate::store::Store,
    stats: Stats,
    state: State,
    today: i64,
    now: i64,
    /// What `eink::fb::has_cfa` answered at startup.
    colour: bool,
    /// Where every touchable thing was on the last frame.
    hits: Vec<(Hit, crate::ui::paint::Rect)>,
}

impl App {
    pub fn new(store: crate::store::Store, theme: Theme, text: TextRenderer) -> Self {
        let (today, now) = date::now();
        let settings = Settings::load(Lang::detect());
        let stats = Stats::build(&store, today, settings.show_unnamed);
        let colour = crate::eink::fb::has_cfa();
        eprintln!(
            "panel: {}",
            match colour {
                true => "colour filter present, schemes offered",
                false => "no colour filter, drawing grey",
            }
        );
        Self {
            theme,
            lang: settings.language,
            settings,
            text,
            covers: Covers::default(),
            store,
            stats,
            colour,
            state: State::new(today),
            today,
            now,
            hits: Vec::new(),
        }
    }

    /// What the stats hold, for the launch line.
    pub fn counted(&self, s: &crate::lang::Strings) -> String {
        format!(
            "{} books drawn, {} on {} unnamed, {} on no book",
            self.stats.books.len(),
            date::duration(self.stats.unnamed_seconds, s),
            self.stats.unnamed_books(),
            date::duration(self.stats.skipped_seconds, s),
        )
    }

    /// Total the store again, at the day and the settings held.
    fn rebuild(&mut self) {
        self.stats = Stats::build(&self.store, self.today, self.settings.show_unnamed);
    }

    /// Draw at `size`, whatever is stored.
    pub fn set_text_size(&mut self, size: crate::settings::TextSize) {
        self.settings.text_size = size;
        self.theme = Theme::sized(self.theme.screen.w as u32, self.theme.screen.h as u32, size);
    }

    /// Draw in `lang`, whatever the device says.
    pub fn set_language(&mut self, lang: Lang) {
        self.lang = lang;
        self.settings.language = lang;
    }

    /// Count the sittings no record names, or leave them out of every total.
    pub fn set_unnamed(&mut self, show: bool) {
        self.settings.show_unnamed = show;
        self.rebuild();
    }

    /// Open the week on `start`, whatever is stored.
    pub fn set_week_start(&mut self, start: crate::settings::WeekStart) {
        self.settings.week_start = start;
    }

    /// Draw in `scheme`, whatever is stored.
    pub fn set_color_scheme(&mut self, scheme: crate::settings::ColorScheme) {
        self.settings.color_scheme = scheme;
    }

    /// Draw with `colour`, whatever `eink::fb::has_cfa` answered.
    pub fn set_colour(&mut self, colour: bool) {
        self.colour = colour;
    }

    /// Draw as though the device's clock read `now` seconds into `today`.
    pub fn set_clock(&mut self, today: i64, now: i64) {
        self.today = today;
        self.now = now;
        self.state.day = today;
        self.rebuild();
    }

    /// Set `state.tab` and `state.book`.
    pub fn show(&mut self, tab: Tab, book: Option<usize>) {
        self.state.tab = tab;
        self.state.book = book;
    }

    /// Draw All Time at `page`, whatever it was left on.
    pub fn set_alltime_page(&mut self, page: usize) {
        self.state.alltime_page = page;
    }

    /// Draw Rhythm at `span`, whatever it was left on.
    pub fn set_span(&mut self, span: crate::view::Span) {
        self.state.span = span;
        self.state.picked = false;
    }

    /// List the books in `order`.
    pub fn set_sort(&mut self, order: crate::view::Sort) {
        self.state.sort = order;
        self.state.books_from = 0;
    }

    /// List the books on `shelf`, whatever the Books screen was left on.
    pub fn set_shelf(&mut self, shelf: crate::view::Shelf) {
        self.state.shelf = shelf;
        self.state.books_from = 0;
    }

    /// Draw Rhythm at `day`, with no day picked off the grid.
    pub fn set_day(&mut self, day: i64) {
        self.state.day = day;
        self.state.picked = false;
    }

    /// Draw Rhythm with `day` picked off the grid.
    pub fn open_day(&mut self, day: i64) {
        self.state.day = day;
        self.state.picked = true;
    }

    /// Open a book list at `from`, held inside the list by the screen drawing
    /// it.
    pub fn open_list(&mut self, from: usize) {
        self.state.list_from = from;
    }

    /// Open the Books screen's own list at `from`, held the same way.
    pub fn open_books(&mut self, from: usize) {
        self.state.books_from = from;
    }

    /// Where every touchable thing was on the frame last drawn.
    pub fn hits(&self) -> &[(Hit, crate::ui::paint::Rect)] {
        &self.hits
    }

    /// The language every screen is drawn in.
    pub fn language(&self) -> Lang {
        self.lang
    }

    /// The tab, day, span and open book the screens are drawn at.
    pub fn state(&self) -> &State {
        &self.state
    }

    /// Draw the whole screen and present it.
    pub fn draw(&mut self, fb: &mut Framebuffer) -> Result<()> {
        let settings = self.settings.clone();
        let colour = self.colour;
        let state = self.state.clone();
        self.frame(fb, &mut |cx, area| match state.book {
            Some(index) => view::book::draw(cx, area, index),
            None => match state.tab {
                Tab::Config => view::config::draw(cx, area, &settings, colour),
                Tab::Home => view::home::draw(cx, area, state.list_from),
                Tab::Rhythm => view::rhythm::draw(cx, area, &state),
                Tab::Books => view::books::draw(cx, area, &state),
            },
        })
    }

    /// `body` in the content box, under the tab strip, presented in one update.
    ///
    /// [`App::draw`] fills it with the screen `state` names.
    pub fn frame(
        &mut self,
        fb: &mut Framebuffer,
        body: &mut dyn FnMut(&mut Ctx, crate::ui::paint::Rect),
    ) -> Result<()> {
        chrome::clear(fb, &self.theme);
        let area = chrome::content_box(&self.theme);

        let mut cx = Ctx {
            fb,
            text: &mut self.text,
            covers: &mut self.covers,
            theme: &self.theme,
            lang: self.lang,
            week: self.settings.week_start,
            palette: crate::ui::paint::Palette::for_panel(self.settings.color_scheme, self.colour),
            stats: &self.stats,
            today: self.today,
            now: self.now,
            hits: Vec::new(),
        };
        body(&mut cx, area);
        self.hits = std::mem::take(&mut cx.hits);
        let (exit, tabs) = chrome::tabs(fb, &mut self.text, &self.theme, self.lang, self.state.tab);
        self.hits.push((Hit::Exit, exit));
        for (tab, area) in tabs {
            self.hits.push((Hit::Tab(tab), area));
        }
        fb.send_update(
            MxcfbRect {
                top: 0,
                left: 0,
                width: self.theme.screen.w as u32,
                height: self.theme.screen.h as u32,
            },
            WAVEFORM_MODE_GC16,
        )?;
        Ok(())
    }

    /// Go looking for a newer release, over the whole screen. The transfer
    /// blocks a worker thread while this drains `input`, repaints the banner as
    /// steps arrive, and sets the flag the download reads between chunks.
    fn update(&mut self, fb: &mut Framebuffer, input: &mut Input) -> Result<()> {
        let cancel = Arc::new(AtomicBool::new(false));
        let (tx, rx) = mpsc::channel::<Doing>();
        let flag = Arc::clone(&cancel);
        let worker = std::thread::spawn(move || {
            update::run(&flag, &|doing| {
                let _ = tx.send(doing);
            })
        });

        let mut doing = Doing::Asking;
        self.doing(fb, doing, true)?;
        let mut painted = Instant::now();
        let mut stale = false;
        // Once the screen says it is stopping, nothing draws over that.
        let mut stopping = false;

        loop {
            while let Ok(step) = rx.try_recv() {
                doing = step;
                stale = !stopping;
            }
            if stale && painted.elapsed() >= BANNER_REDRAW {
                self.doing(fb, doing, false)?;
                painted = Instant::now();
                stale = false;
            }
            // Every step sent is drained above before this is read, so nothing
            // the worker said is lost by leaving here.
            if worker.is_finished() {
                break;
            }
            // Waking when the next repaint is due rather than a whole
            // interval from here: a step that arrived a moment ago would
            // otherwise wait out most of a second interval before it is drawn.
            let due = match stale {
                true => painted + BANNER_REDRAW,
                false => Instant::now() + BANNER_REDRAW,
            };
            // A tap anywhere stops it: the banner is the whole screen and has
            // no room for a button that would only ever be pressed once.
            if let InputEvent::Touch(TouchEvent::Up { .. }) = input.next_deadline(Some(due))?
                && doing.stoppable()
                && !cancel.swap(true, Ordering::Relaxed)
            {
                let headline = self.lang.strings().update_row;
                let said = vec![self.lang.strings().update_stopped.to_string()];
                self.banner(fb, headline, &said, "", true)?;
                (painted, stale, stopping) = (Instant::now(), false, true);
            }
        }

        // A worker that panicked left nothing in place: moving the new copy in
        // is the last thing it does, and every step of that answers rather
        // than unwinds.
        let outcome = worker
            .join()
            .unwrap_or(Outcome::Failed(update::Failure::NotPlaced));
        let (headline, note) = outcome.banner(self.lang.strings());
        eprintln!("update: {outcome:?}");
        self.banner(fb, &headline, &note, "", true)?;
        self.hold(input, OUTCOME_LINGER)
    }

    /// One frame of an update banner, over the whole screen. `headline` and
    /// `note` are what [`Doing::banner`] and [`Outcome::banner`] answer; `step`
    /// is the way out while there is one. Public for the preview.
    pub fn banner(
        &mut self,
        fb: &mut Framebuffer,
        headline: &str,
        note: &[String],
        step: &str,
        first: bool,
    ) -> Result<()> {
        let said = crate::ui::splash::Words {
            script: crate::font::Script::of_language(self.lang.language_tag()),
            headline,
            note,
            step,
        };
        crate::ui::splash::show(fb, &mut self.text, &self.theme, &said, first)
    }

    /// [`App::banner`] at a step of the update running now. The way out is
    /// offered only while [`Doing::stoppable`] says there is one.
    fn doing(&mut self, fb: &mut Framebuffer, doing: Doing, first: bool) -> Result<()> {
        let (headline, note) = doing.banner(self.lang.strings());
        let step = match doing.stoppable() {
            true => self.lang.strings().update_tap_to_stop,
            false => "",
        };
        self.banner(fb, &headline, &note, step, first)
    }

    /// Leave what is on screen up for `linger`, or until it is tapped.
    fn hold(&self, input: &mut Input, linger: Duration) -> Result<()> {
        let until = Instant::now() + linger;
        while Instant::now() < until {
            if let InputEvent::Touch(TouchEvent::Up { .. }) = input.next_deadline(Some(until))? {
                return Ok(());
            }
        }
        Ok(())
    }

    /// Run until [`Action::Quit`].
    pub fn run(&mut self, fb: &mut Framebuffer, input: &mut Input) -> Result<()> {
        self.draw(fb)?;
        let mut down: Option<(u32, u32)> = None;
        loop {
            match input.event()? {
                InputEvent::Touch(TouchEvent::Down { x, y }) => down = Some((x, y)),
                InputEvent::Touch(TouchEvent::Up { x, y }) => {
                    let from = down.take();
                    let swipe = from.and_then(|(x0, y0)| {
                        classify_swipe(x0, y0, x, y, self.theme.screen.w as u32)
                    });
                    let acted = match swipe {
                        Some(dir) => self.swiped(dir),
                        None => self.tapped(x as i32, y as i32),
                    };
                    match acted {
                        Action::Redraw => self.draw(fb)?,
                        Action::Quit => return Ok(()),
                        // Blocking: the banner is the screen until it ends.
                        Action::Update => {
                            self.update(fb, input)?;
                            self.draw(fb)?;
                        }
                        Action::Nothing => {}
                    }
                }
                // `eink::touch` raises this under an `EVIOCGRAB`.
                InputEvent::Touch(TouchEvent::Screenshot) => {
                    down = None;
                    match screenshot::capture(fb) {
                        Ok(path) => eprintln!("screenshot: {}", path.display()),
                        Err(err) => eprintln!("screenshot: {err:#}"),
                    }
                }
                InputEvent::Page(_) => {
                    if let Action::Redraw = self.paged(1) {
                        self.draw(fb)?;
                    }
                }
                // `pump_events` reports a repaint request.
                InputEvent::Tick if fb.pump_events() => self.draw(fb)?,
                _ => {}
            }
        }
    }

    /// What a tap at `(x, y)` does.
    fn tapped(&mut self, x: i32, y: i32) -> Action {
        // `hits` in reverse: the last box drawn wins.
        let Some(hit) = self
            .hits
            .iter()
            .rev()
            .find(|(_, r)| r.contains(x, y))
            .map(|(h, _)| *h)
        else {
            return Action::Nothing;
        };
        match hit {
            Hit::Tab(tab) => {
                if !self.state.go(tab) {
                    return Action::Nothing;
                }
            }
            Hit::Exit => return Action::Quit,
            Hit::Language(pick) => {
                if self.settings.language == pick {
                    return Action::Nothing;
                }
                self.settings.language = pick;
                self.lang = pick;
                self.settings.save();
            }
            Hit::WeekStart(pick) => {
                if self.settings.week_start == pick {
                    return Action::Nothing;
                }
                self.settings.week_start = pick;
                self.settings.save();
            }
            Hit::TextSize(pick) => {
                if self.settings.text_size == pick {
                    return Action::Nothing;
                }
                self.settings.text_size = pick;
                // Every size on screen comes off `theme`.
                self.theme =
                    Theme::sized(self.theme.screen.w as u32, self.theme.screen.h as u32, pick);
                self.settings.save();
            }
            Hit::ColorScheme(pick) => {
                if self.settings.color_scheme == pick {
                    return Action::Nothing;
                }
                self.settings.color_scheme = pick;
                self.settings.save();
            }
            Hit::ShowUnnamed(pick) => {
                if self.settings.show_unnamed == pick {
                    return Action::Nothing;
                }
                self.settings.show_unnamed = pick;
                // Every total on every screen comes off `stats`.
                self.rebuild();
                self.state.books_from = 0;
                self.state.list_from = 0;
                self.settings.save();
            }
            Hit::Book(index) => self.state.book = Some(index),
            // A second tap on the day picked drops it again.
            Hit::Day(day) => {
                self.state.picked = !(self.state.picked && self.state.day == day);
                self.state.day = day;
                self.state.opened_day = false;
                self.state.list_from = 0;
            }
            Hit::OpenDay => {
                if self.state.opened_day {
                    return Action::Nothing;
                }
                self.state.opened_day = true;
                self.state.list_from = 0;
            }
            Hit::Sorted(order) => {
                if self.state.sort == order {
                    return Action::Nothing;
                }
                self.state.sort = order;
                self.state.books_from = 0;
            }
            // A shelf is reached from the board, and lands on the Books tab.
            Hit::Shelved(shelf) => {
                if self.state.tab == Tab::Books && self.state.shelf == shelf {
                    return Action::Nothing;
                }
                self.state.tab = Tab::Books;
                self.state.shelf = shelf;
                self.state.book = None;
                self.state.books_from = 0;
            }
            Hit::ListPage(at) => {
                if at == self.state.list_from {
                    return Action::Nothing;
                }
                self.state.list_from = at;
            }
            Hit::Span(span) => {
                if self.state.span == span && !self.state.picked {
                    return Action::Nothing;
                }
                self.state.span = span;
                self.state.picked = false;
                self.state.opened_day = false;
                self.state.list_from = 0;
                self.state.alltime_page = 0;
            }
            Hit::Now => {
                if self.state.day == self.today && !self.state.picked {
                    return Action::Nothing;
                }
                self.state.day = self.today;
                self.state.picked = false;
                self.state.opened_day = false;
                self.state.list_from = 0;
            }
            Hit::BooksPage(at) => {
                if at == self.state.books_from {
                    return Action::Nothing;
                }
                self.state.books_from = at;
            }
            Hit::Update => return Action::Update,
            Hit::Prev => return self.paged(-1),
            Hit::Next => return self.paged(1),
        }
        Action::Redraw
    }

    /// `dir` through [`App::paged`].
    fn swiped(&mut self, dir: SwipeDir) -> Action {
        match dir {
            SwipeDir::Next => self.paged(1),
            SwipeDir::Prev => self.paged(-1),
        }
    }

    /// One step forward or back: a span on Rhythm, a page of the list, the
    /// next book.
    fn paged(&mut self, by: i64) -> Action {
        if self.state.book.is_some() {
            let count = self.stats.books.len() as i64;
            if count == 0 {
                return Action::Nothing;
            }
            let at = self.state.book.unwrap_or(0) as i64;
            let next = (at + by).rem_euclid(count);
            self.state.book = Some(next as usize);
            return Action::Redraw;
        }
        match self.state.tab {
            // Neither has anything to page through.
            Tab::Config | Tab::Home => Action::Nothing,
            Tab::Rhythm => {
                if !self.state.shift(by) {
                    return Action::Nothing;
                }
                self.state.list_from = 0;
                Action::Redraw
            }
            Tab::Books => {
                // `rows_per_page` states the step.
                let chips = !self.stats.books.is_empty();
                let area =
                    view::books::list_box(&self.theme, chrome::content_box(&self.theme), chips);
                let count =
                    view::books::listed(&self.stats, self.state.shelf, self.state.sort).len();
                let step = view::books::rows_per_page(&self.theme, area) as i64;
                let last = view::books::last_page_at(&self.theme, area, count);
                let from = self.state.books_from as i64 + by * step;
                let capped = from.clamp(0, last as i64) as usize;
                if capped == self.state.books_from {
                    return Action::Nothing;
                }
                self.state.books_from = capped;
                Action::Redraw
            }
        }
    }
}

/// What the loop does with an event.
enum Action {
    Redraw,
    Nothing,
    Quit,
    /// Go looking for a newer release, over the whole screen.
    Update,
}
