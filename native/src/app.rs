//! The run loop: what is drawn, and what a touch does to it.
//!
//! [`App::draw`] redraws the whole screen and presents it in one
//! [`Framebuffer::send_update`].

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
use crate::view::{self, Ctx, Hit, State};

pub struct App {
    theme: Theme,
    /// The language drawn in: the reader's pick, else [`App::detected`].
    lang: Lang,
    settings: Settings,
    text: TextRenderer,
    covers: Covers,
    stats: Stats,
    state: State,
    today: i64,
    now: i64,
    /// Where every touchable thing was on the last frame.
    hits: Vec<(Hit, crate::ui::paint::Rect)>,
}

impl App {
    pub fn new(stats: Stats, theme: Theme, text: TextRenderer) -> Self {
        let (today, now) = date::now();
        let settings = Settings::load(Lang::detect());
        Self {
            theme,
            lang: settings.language,
            settings,
            text,
            covers: Covers::default(),
            stats,
            state: State::new(today),
            today,
            now,
            hits: Vec::new(),
        }
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

    /// Open the week on `start`, whatever is stored.
    pub fn set_week_start(&mut self, start: crate::settings::WeekStart) {
        self.settings.week_start = start;
    }

    /// Draw as though the device's clock read `now` seconds into `today`.
    pub fn set_clock(&mut self, today: i64, now: i64) {
        self.today = today;
        self.now = now;
        self.state.day = today;
    }

    /// Set `state.tab` and `state.book`.
    pub fn show(&mut self, tab: Tab, book: Option<usize>) {
        self.state.tab = tab;
        self.state.book = book;
    }

    /// Draw Rhythm at `span`, whatever it was left on.
    pub fn set_span(&mut self, span: crate::view::Span) {
        self.state.span = span;
        self.state.picked = false;
    }

    /// Draw Rhythm with `day` picked off the grid.
    pub fn open_day(&mut self, day: i64) {
        self.state.day = day;
        self.state.picked = true;
    }

    /// Where the reader has navigated to.
    pub fn state(&self) -> &State {
        &self.state
    }

    /// Draw the whole screen and present it.
    pub fn draw(&mut self, fb: &mut Framebuffer) -> Result<()> {
        let settings = self.settings.clone();
        let state = self.state.clone();
        self.frame(fb, &mut |cx, area| match state.book {
            Some(index) => view::book::draw(cx, area, index),
            None => match state.tab {
                Tab::Config => view::config::draw(cx, area, &settings),
                Tab::Home => view::home::draw(cx, area),
                Tab::Rhythm => view::rhythm::draw(cx, area, &state),
                Tab::Books => view::books::draw(cx, area, &state),
            },
        })
    }

    /// `body` in the content box, under the tab strip, presented in one update.
    ///
    /// [`App::draw`] fills it with the screen `state` names. A caller with a
    /// screen of its own draws that instead, in the frame the device gives it.
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
                // Every size on screen comes off the theme, so it is rebuilt.
                self.theme =
                    Theme::sized(self.theme.screen.w as u32, self.theme.screen.h as u32, pick);
                self.settings.save();
            }
            Hit::Book(index) => self.state.book = Some(index),
            // A second tap on the day picked drops it again.
            Hit::Day(day) => {
                self.state.picked = !(self.state.picked && self.state.day == day);
                self.state.day = day;
                self.state.list_from = 0;
            }
            Hit::Average(all) => {
                if self.state.average_all == all {
                    return Action::Nothing;
                }
                self.state.average_all = all;
            }
            Hit::ListPage(by) => {
                let at = self.state.list_from as i64 + by;
                if at < 0 {
                    return Action::Nothing;
                }
                self.state.list_from = at as usize;
            }
            Hit::Span(span) => {
                if self.state.span == span && !self.state.picked {
                    return Action::Nothing;
                }
                self.state.span = span;
                self.state.picked = false;
                self.state.list_from = 0;
            }
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
                self.state.shift(by);
                self.state.list_from = 0;
                Action::Redraw
            }
            Tab::Books => {
                // `rows_per_page` states the step.
                let area = chrome::content_box(&self.theme);
                let step = view::books::rows_per_page(&self.theme, area) as i64;
                let last = view::books::last_page_at(&self.theme, area, self.stats.books.len());
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
}
