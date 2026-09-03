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
use crate::stats::Stats;
use crate::ui::chrome::{self, Tab};
use crate::ui::cover::Covers;
use crate::ui::text::TextRenderer;
use crate::ui::theme::Theme;
use crate::view::{self, Ctx, Cut, Hit, State};

pub struct App {
    theme: Theme,
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
        eprintln!("fonts: {}", text.chain_description());
        let (today, now) = date::now();
        Self {
            theme,
            text,
            covers: Covers::default(),
            stats,
            state: State::new(today),
            today,
            now,
            hits: Vec::new(),
        }
    }

    /// Set `state.tab` and `state.book`.
    #[cfg(test)]
    pub fn show(&mut self, tab: Tab, book: Option<usize>) {
        self.state.tab = tab;
        self.state.book = book;
    }

    /// Draw the whole screen and present it.
    pub fn draw(&mut self, fb: &mut Framebuffer) -> Result<()> {
        chrome::clear(fb, &self.theme);
        let area = chrome::content_box(&self.theme);

        let mut cx = Ctx {
            fb,
            text: &mut self.text,
            covers: &mut self.covers,
            theme: &self.theme,
            stats: &self.stats,
            today: self.today,
            now: self.now,
            hits: Vec::new(),
        };
        match self.state.book {
            Some(index) => view::book::draw(&mut cx, area, index),
            None => match self.state.tab {
                Tab::Home => view::home::draw(&mut cx, area),
                Tab::Calendar => view::calendar::draw(&mut cx, area, &self.state),
                Tab::Books => view::books::draw(&mut cx, area, &self.state),
                Tab::Clock => view::clock::draw(&mut cx, area, &self.state),
            },
        }
        self.hits = std::mem::take(&mut cx.hits);
        let (exit, tabs) = chrome::tabs(fb, &mut self.text, &self.theme, self.state.tab);
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
            match input.next()? {
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
            Hit::Book(index) => self.state.book = Some(index),
            Hit::Day(day) => {
                // A second tap on `state.day` clears it.
                self.state.day = match self.state.day == Some(day) {
                    true => None,
                    false => Some(day),
                };
            }
            Hit::Cut(cut) => self.state.cut = cut,
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

    /// One step forward or back: a month on the calendar, a page of the list,
    /// the next book.
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
            Tab::Calendar => {
                self.state.shift_month(by);
                self.state.day = None;
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
            Tab::Clock => {
                let at = Cut::ALL
                    .iter()
                    .position(|c| *c == self.state.cut)
                    .unwrap_or(0) as i64;
                let next = (at + by).rem_euclid(Cut::ALL.len() as i64) as usize;
                self.state.cut = Cut::ALL[next];
                Action::Redraw
            }
            Tab::Home => Action::Nothing,
        }
    }
}

/// What the loop does with an event.
enum Action {
    Redraw,
    Nothing,
    Quit,
}
