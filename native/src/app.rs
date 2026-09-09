//! The run loop: what is drawn, and what a touch does to it. [`App::draw`]
//! redraws the whole screen and presents it in one
//! [`Framebuffer::send_update`].

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc;
use std::time::{Duration, Instant};

use anyhow::Result;

use crate::date;
use crate::eink::buttons::PageButton;
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

/// The shortest time between two repaints of the update banner.
const BANNER_REDRAW: Duration = Duration::from_millis(700);

/// How long an update's last word stays up. A tap ends it sooner.
const OUTCOME_LINGER: Duration = Duration::from_secs(12);

pub struct App {
    theme: Theme,
    /// The language drawn in, from [`Settings::language`].
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
    /// Where the record and the archives beside it live.
    dir: std::path::PathBuf,
    /// Where every touchable thing was on the last frame.
    hits: Vec<(Hit, crate::ui::paint::Rect)>,
    /// Whether the device's keyboard stands over the foot of the screen.
    /// [`App::reconcile_keyboard`] holds it against what the state asks for.
    keyboard: bool,
}

impl App {
    pub fn new(store: crate::store::Store, theme: Theme, text: TextRenderer) -> Self {
        let (today, now) = date::now();
        let settings = Settings::load(Lang::detect());
        let stats = Stats::build(&store, today, settings.show_unnamed, settings.figures);
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
            dir: std::path::PathBuf::from(crate::store::STORE_DIR),
            state: State::new(today),
            today,
            now,
            hits: Vec::new(),
            keyboard: false,
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
        self.stats = Stats::build(
            &self.store,
            self.today,
            self.settings.show_unnamed,
            self.settings.figures,
        );
    }

    /// Draw at `size`, whatever is stored.
    pub fn set_text_size(&mut self, size: crate::settings::TextSize) {
        self.settings.text_size = size;
        self.theme = Theme::sized(self.theme.screen.w as u32, self.theme.screen.h as u32, size);
    }

    /// Rebuild `theme` at `fb`'s size and the stored `text_size`.
    fn took_size(&mut self, fb: &Framebuffer) {
        self.theme = Theme::sized(fb.var.xres, fb.var.yres, self.settings.text_size);
    }

    /// Draw in `lang`, past `Lang::detect`.
    pub fn set_language(&mut self, lang: Lang) {
        self.lang = lang;
        self.settings.language = lang;
    }

    /// Count the sittings no record names, or leave them out of every total.
    pub fn set_unnamed(&mut self, show: bool) {
        self.settings.show_unnamed = show;
        self.rebuild();
    }

    /// List the books no jacket can be drawn for, or leave them off the lists.
    pub fn set_uncovered(&mut self, show: bool) {
        self.settings.show_uncovered = show;
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

    /// Draw against a clock at `now` seconds into `today`.
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

    /// Put a question up over the book it names, or take one down.
    pub fn ask(&mut self, asked: Option<(usize, view::Ask)>) {
        self.state.asked = asked;
    }

    /// Put one of the config page's questions up, gathering the figures it
    /// states, or take one down.
    pub fn ask_about(&mut self, about: Option<view::About>) {
        match about {
            Some(about) => {
                self.ask_question(about);
            }
            None => self.state.confirm = None,
        }
    }

    /// Draw the settings at `page`, whatever it was left on.
    pub fn set_config_page(&mut self, page: usize) {
        self.state.config_page = page;
    }

    /// Draw the open book at `tab`, whatever it was left on, with its marks
    /// opened `from` rows down.
    pub fn set_book_tab(&mut self, tab: view::BookTab, from: usize) {
        self.state.book_tab = tab;
        self.state.marks_from = from;
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

    /// Put a search up over the Books screen, or take one down.
    pub fn set_search(&mut self, search: Option<view::Search>) {
        self.state.search = search;
    }

    /// Take `keysyms` into the open search, answering whether the screen
    /// moved. Nothing is typed anywhere else: the keyboard only stands over a
    /// search.
    fn typed(&mut self, keysyms: &[u32]) -> bool {
        use crate::eink::keysym::{Typed, of_keysym};
        if keysyms.is_empty() || self.state.search.is_none() {
            return false;
        }
        let mut moved = false;
        for keysym in keysyms {
            let Some(said) = of_keysym(*keysym) else {
                continue;
            };
            // A key that leaves the search takes the rest of the batch with it.
            if said == Typed::Escape {
                self.state.search = None;
                return true;
            }
            let Some(search) = self.state.search.as_mut() else {
                return moved;
            };
            match said {
                Typed::Char(said) => {
                    search.typed(said);
                    moved = true;
                }
                Typed::Backspace => moved |= search.backspace(),
                // The query is finished: the keyboard goes and the results
                // take the whole page back.
                Typed::Enter => {
                    moved |= search.keyboard;
                    search.keyboard = false;
                }
                Typed::Escape => unreachable!("answered above"),
            }
        }
        moved
    }

    /// Whether a touch at `y` lands on the keyboard rather than on this app.
    ///
    /// Ungrabbed, the touchscreen still delivers to us everything the keyboard
    /// is being typed on, and a row drawn under it would answer a tap meant
    /// for a key.
    fn under_keyboard(&self, y: i32) -> bool {
        self.keyboard
            && y >= self.theme.screen.bottom() - crate::keyboard::height(self.theme.screen.h)
    }

    /// Raise or dismiss the device's keyboard, to match what the state asks
    /// for. It is another X client, so it is only ever spoken to from a frame
    /// that has a server behind it.
    fn reconcile_keyboard(&mut self, on_screen: bool) {
        let want = on_screen && self.state.search.as_ref().is_some_and(|s| s.keyboard);
        if want == self.keyboard {
            return;
        }
        self.keyboard = want;
        let _ = match want {
            true => crate::keyboard::open(),
            false => crate::keyboard::close(),
        };
    }

    /// List the books on `shelf`, whatever the Books screen was left on.
    pub fn set_shelf(&mut self, shelf: crate::view::Shelf) {
        self.state.shelf = shelf;
        self.state.books_from = 0;
    }

    /// List the books of `window`, whatever stretch the screen was left on.
    pub fn set_window(&mut self, window: Option<crate::view::Window>) {
        self.state.window = window;
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

    /// Everything the screens draw, for a caller placing itself against the
    /// record.
    pub fn stats(&self) -> &crate::stats::Stats {
        &self.stats
    }

    /// Draw the whole screen and present it.
    pub fn draw(&mut self, fb: &mut Framebuffer) -> Result<()> {
        self.reconcile_keyboard(fb.on_screen());
        let settings = self.settings.clone();
        let colour = self.colour;
        let state = self.state.clone();
        // `backup::list` runs on the frame that draws its row.
        let record = match state.tab == Tab::Config && state.book.is_none() {
            true => view::config::Record::of(
                &self.stats,
                &self.dir,
                !self.store.floor.is_empty(),
                self.lang,
            ),
            false => view::config::Record::default(),
        };
        self.frame(fb, &mut |cx, area| match state.book {
            Some(index) => {
                view::book::draw(cx, area, index, state.book_tab, state.marks_from);
                if let Some((at, ask)) = state.asked
                    && at == index
                {
                    view::book::asking(cx, area, ask, index);
                }
            }
            None => match state.tab {
                Tab::Config => {
                    view::config::draw(cx, area, &settings, colour, &record, state.config_page);
                    if let Some(confirm) = &state.confirm {
                        view::config::asking(cx, area, confirm);
                    }
                }
                Tab::Home => view::home::draw(cx, area, state.list_from),
                Tab::Rhythm => view::rhythm::draw(cx, area, &state),
                Tab::Books => match &state.search {
                    Some(search) => view::search::draw(cx, area, search),
                    None => view::books::draw(cx, area, &state),
                },
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
            figures: self.settings.figures,
            uncovered: self.settings.show_uncovered,
            palette: crate::ui::paint::Palette::for_panel(self.settings.color_scheme, self.colour),
            stats: &self.stats,
            today: self.today,
            now: self.now,
            floored: !self.store.floor.is_empty(),
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
        // While `stopping`, a later `doing` leaves `stale` false.
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
            // Every step sent is drained above before this is read: no step
            // the worker sent is lost by leaving here.
            if worker.is_finished() {
                break;
            }
            // `due` is when the next repaint falls, never a whole
            // `BANNER_REDRAW` from here.
            let due = match stale {
                true => painted + BANNER_REDRAW,
                false => Instant::now() + BANNER_REDRAW,
            };
            // A tap anywhere stops it: the banner is the whole screen.
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

        // `worker` moves the new copy in as its last step, and every step of
        // that answers.
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
    /// is the way out while there is one.
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

    /// [`App::banner`] at one step of a running update. The way out is
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

    /// Run until `Action::Quit`.
    pub fn run(&mut self, fb: &mut Framebuffer, input: &mut Input) -> Result<()> {
        fb.pump_events();
        self.took_size(fb);
        // A key the on-screen keyboard sends arrives on the X connection, not
        // on either input device.
        input.watch(fb.raw_fd());
        self.draw(fb)?;
        let mut down: Option<(u32, u32)> = None;
        loop {
            // An exclusive grab on the touchscreen blinds the X server, and
            // with it the keyboard: it would draw and never feel a tap.
            input.set_keyboard(self.keyboard);
            match input.event()? {
                // `follow_orientation_now` maps the `Up` this stroke ends on. A
                // `Down` behind a change is dropped, leaving a tap at its `Up`.
                InputEvent::Touch(TouchEvent::Down { x, y }) => {
                    down = match input.follow_orientation_now() || self.under_keyboard(y as i32) {
                        true => None,
                        false => Some((x, y)),
                    };
                }
                InputEvent::Touch(TouchEvent::Up { x, y }) => {
                    if self.under_keyboard(y as i32) {
                        down = None;
                        continue;
                    }
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
                        Action::Retry => {
                            self.retry(fb, input)?;
                            self.draw(fb)?;
                        }
                        Action::Heal => {
                            self.heal(fb, input)?;
                            self.draw(fb)?;
                        }
                        Action::Resetting(about) => {
                            self.resetting(fb, about)?;
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
                // `page` carries the orientation standing before this read; a
                // change drops it.
                InputEvent::Page(page) => {
                    if !input.follow_orientation_now()
                        && let Action::Redraw = self.pressed(page)
                    {
                        self.draw(fb)?;
                    }
                }
                InputEvent::Tick => {
                    let pump = fb.pump_events();
                    if let Some(covered) = pump.covered {
                        down = None;
                        input.set_covered(covered);
                    }
                    input.retake();
                    let typed = self.typed(&pump.typed);
                    if input.follow_orientation() {
                        down = None;
                    }
                    // `Pump::resized` rebuilds `theme` and draws the whole screen.
                    if pump.resized.is_some() {
                        self.took_size(fb);
                        self.draw(fb)?;
                        continue;
                    }
                    match pump.covered {
                        // Covered: nothing is drawn.
                        Some(true) => {}
                        // Uncovered: drawn without an `Expose`.
                        Some(false) => self.draw(fb)?,
                        None if pump.repaint || typed => self.draw(fb)?,
                        None => {}
                    }
                }
            }
        }
    }

    /// Write down a request for a book to be opened, and leave.
    /// `bin/readinglog.sh` makes the call once this process is gone, and a book
    /// `BookStat::can_open` refuses is never asked for.
    fn open(&mut self, index: usize, at: crate::open::At) -> Action {
        let Some(book) = self.stats.books.get(index).filter(|b| b.can_open()) else {
            return Action::Nothing;
        };
        match crate::open::ask(
            std::path::Path::new(crate::store::STORE_DIR),
            &book.location,
            at,
        ) {
            Ok(()) => Action::Quit,
            Err(err) => {
                eprintln!("open: {} would not be asked for: {err:#}", book.location);
                Action::Nothing
            }
        }
    }

    /// Set `BookRecord::finished` on the book at `index`, total the store again
    /// and write it out. A failed `save` leaves the mark in `stats` and off the
    /// disk.
    fn set_finished(&mut self, index: usize, on: bool) -> Action {
        let Some(book) = self.stats.books.get(index) else {
            return Action::Nothing;
        };
        let (extent, key, cde_type) = (book.extent, book.cde_key.clone(), book.cde_type.clone());
        if !self.store.set_finished(extent, &key, on) {
            return Action::Nothing;
        }
        self.hand_over_mark(extent, &key, &cde_type, on);
        self.rebuild();
        if let Err(err) = self
            .store
            .save(std::path::Path::new(crate::store::STORE_DIR))
        {
            eprintln!("finished: the mark did not reach the store: {err:#}");
        }
        Action::Redraw
    }

    /// Hand `read` to `mark::set`, and write down the `p_readState` a taken
    /// call leaves. A refused call leaves the record's own value standing.
    fn hand_over_mark(&mut self, extent: i64, key: &str, cde_type: &str, read: bool) {
        if crate::mark::set(key, cde_type, read) {
            self.store
                .note_mark(extent, key, crate::catalog::read_state_for(read));
        }
    }

    /// Put `ask` up over the book at `index`. A question standing there is
    /// left as it stands.
    fn put(&mut self, index: usize, ask: view::Ask) -> Action {
        if self.state.asked == Some((index, ask)) {
            return Action::Nothing;
        }
        self.state.asked = Some((index, ask));
        Action::Redraw
    }

    /// Carry out the question `State::asked` holds, taking it down.
    fn answer(&mut self) -> Action {
        let Some((index, ask)) = self.state.asked.take() else {
            return Action::Nothing;
        };
        match ask {
            view::Ask::Restart => self.restart(index),
            view::Ask::Mark(on) => self.set_finished(index, on),
            // Two answers of its own, each its own hit.
            view::Ask::Clear => Action::Redraw,
        }
    }

    /// The book at `index` gives up its place and its mark, then goes back to
    /// the Kindle's reader.
    fn restart(&mut self, index: usize) -> Action {
        if let Some(book) = self.stats.books.get(index) {
            let (extent, key, cde_type) =
                (book.extent, book.cde_key.clone(), book.cde_type.clone());
            if self.store.restart(extent, &key) {
                self.hand_over_mark(extent, &key, &cde_type, false);
                self.rebuild();
                if let Err(err) = self
                    .store
                    .save(std::path::Path::new(crate::store::STORE_DIR))
                {
                    eprintln!("restart: the record did not reach the store: {err:#}");
                }
            }
        }
        match self.open(index, crate::open::At::Beginning) {
            Action::Quit => Action::Quit,
            _ => Action::Redraw,
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
            Hit::Open(index) => return self.open(index, crate::open::At::Left),
            Hit::Finished(index, on) => return self.put(index, view::Ask::Mark(on)),
            Hit::Restart(index) => return self.put(index, view::Ask::Restart),
            Hit::Answer => return self.answer(),
            Hit::Dismiss => {
                // Whichever question stands: the book's, or the config page's.
                let asked = self.state.asked.take().is_some();
                let confirmed = self.state.confirm.take().is_some();
                if !asked && !confirmed {
                    return Action::Nothing;
                }
            }
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
            Hit::Figures(pick) => {
                if self.settings.figures == pick {
                    return Action::Nothing;
                }
                self.settings.figures = pick;
                // Every total on every screen comes off `stats`.
                self.rebuild();
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
            Hit::ShowUncovered(pick) => {
                if self.settings.show_uncovered == pick {
                    return Action::Nothing;
                }
                self.settings.show_uncovered = pick;
                // The lists are shorter or longer for it, and a page held
                // part way down one of them names other books now.
                self.state.books_from = 0;
                self.state.list_from = 0;
                self.settings.save();
            }
            Hit::Book(index) => self.state.open_book(index),
            Hit::BookTab(tab) => {
                if !self.state.go_in_book(tab) {
                    return Action::Nothing;
                }
            }
            Hit::MarksPage(from) => {
                if self.state.marks_from == from {
                    return Action::Nothing;
                }
                self.state.marks_from = from;
            }
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
            // A shelf is reached from a figure, and lands on the Books tab.
            Hit::Shelved(shelf, window) => {
                let on = self.state.shelf == shelf && self.state.window == window;
                if self.state.tab == Tab::Books && on {
                    return Action::Nothing;
                }
                self.state.tab = Tab::Books;
                self.state.shelf = shelf;
                self.state.window = window;
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
            Hit::Search => {
                self.state.search = Some(view::Search {
                    keyboard: true,
                    ..view::Search::default()
                });
            }
            Hit::SearchField => {
                let Some(search) = self.state.search.as_mut() else {
                    return Action::Nothing;
                };
                if search.keyboard {
                    return Action::Nothing;
                }
                search.keyboard = true;
            }
            Hit::SearchClear => {
                let Some(search) = self.state.search.as_mut() else {
                    return Action::Nothing;
                };
                match search.query.is_empty() {
                    // Nothing to take off, so the mark takes the search off.
                    true => self.state.search = None,
                    false => {
                        search.query.clear();
                        search.from = 0;
                    }
                }
            }
            Hit::SearchPage(at) => {
                let Some(search) = self.state.search.as_mut() else {
                    return Action::Nothing;
                };
                if at == search.from {
                    return Action::Nothing;
                }
                search.from = at;
            }
            Hit::ConfigPage(at) => {
                if at == self.state.config_page {
                    return Action::Nothing;
                }
                self.state.config_page = at;
            }
            Hit::Update => return Action::Update,
            Hit::Retry => return self.ask_question(view::About::Retry),
            Hit::Heal => return self.ask_question(view::About::Heal),
            Hit::Prev => return self.paged(-1),
            Hit::Next => return self.paged(1),
            Hit::Clear(index) => return self.put(index, view::Ask::Clear),
            Hit::ClearBook(index) => return self.clear_book(index, false),
            Hit::ForgetBook(index) => return self.clear_book(index, true),
            Hit::Wipe(keep) => {
                return self.ask_question(view::About::Reset(view::Reset::Wipe(keep)));
            }
            Hit::Restore(at) => {
                return self.ask_question(view::About::Reset(view::Reset::Restore(at)));
            }
            Hit::Rebuild => return self.ask_question(view::About::Reset(view::Reset::Rebuild)),
            // `App::resetting`, `App::retry` and `App::heal` carry these out,
            // holding `fb`.
            Hit::Wiped(keep) => return Action::Resetting(view::Reset::Wipe(keep)),
            Hit::Restored(at) => return Action::Resetting(view::Reset::Restore(at)),
            Hit::Rebuilt => return Action::Resetting(view::Reset::Rebuild),
            Hit::Retried => return Action::Retry,
            Hit::Healed => return Action::Heal,
        }
        Action::Redraw
    }

    /// Where the record lives.
    fn dir(&self) -> &std::path::Path {
        &self.dir
    }

    /// Draw and act against a record outside [`crate::store::STORE_DIR`].
    pub fn set_dir(&mut self, dir: std::path::PathBuf) {
        self.dir = dir;
    }

    /// Write the record, printing `what` and the error on a failed write.
    fn store_it(&self, what: &str) {
        if let Err(err) = self.store.save(self.dir()) {
            eprintln!("{what}: the record did not reach the store: {err:#}");
        }
    }

    /// Put one of the config page's questions up, with the figures it states
    /// gathered here: the dialog itself walks nothing.
    fn ask_question(&mut self, about: view::About) -> Action {
        let (jackets, archives) = crate::backup::sizes(self.dir());
        let confirm = match about {
            view::About::Heal | view::About::Retry => view::Confirm {
                about,
                sittings: self.stats.sittings.len(),
                books: self.stats.book_count(),
                bytes: 0,
                named: String::new(),
            },
            view::About::Reset(view::Reset::Wipe(keep)) => view::Confirm {
                about,
                sittings: self.stats.sittings.len(),
                books: self.stats.book_count(),
                // The archive's weight, or the room `covers::sweep` returns.
                bytes: match keep {
                    true => jackets + self.store.text().len() as u64,
                    false => jackets,
                },
                named: crate::backup::name(crate::backup::Kind::Record, &self.store.mark),
            },
            view::About::Reset(view::Reset::Restore(at)) => {
                let held = crate::backup::list(self.dir());
                let Some(backup) = held.get(at) else {
                    return Action::Nothing;
                };
                // `peek` once: the figures the question states. `inside` is
                // totalled whole, whatever `show_unnamed` stands at.
                let inside = crate::backup::peek(&backup.path).unwrap_or_default();
                let held =
                    crate::stats::Stats::build(&inside, self.today, true, self.settings.figures);
                view::Confirm {
                    about,
                    sittings: held.sittings.len(),
                    books: held.book_count(),
                    bytes: backup.bytes,
                    named: backup
                        .path
                        .file_name()
                        .map(|n| n.to_string_lossy().into_owned())
                        .unwrap_or_default(),
                }
            }
            view::About::Reset(view::Reset::Rebuild) => view::Confirm {
                about,
                sittings: 0,
                books: 0,
                bytes: archives,
                named: String::new(),
            },
        };
        if self.state.confirm.as_ref() == Some(&confirm) {
            return Action::Nothing;
        }
        self.state.confirm = Some(confirm);
        Action::Redraw
    }

    /// Carry `about` out under a banner over the whole screen, drawn from
    /// `Reset::doing`. `restore` and `reread` count their files onto it.
    fn resetting(&mut self, fb: &mut Framebuffer, about: view::Reset) -> Result<()> {
        self.state.confirm = None;
        let (headline, note) = about.doing(self.lang.strings());
        self.banner(fb, &headline, &note, "", true)?;
        match about {
            view::Reset::Wipe(keep) => self.wipe(keep),
            view::Reset::Restore(at) => self.restore(fb, &headline, &note, at),
            view::Reset::Rebuild => self.reread(fb, &headline, &note),
        }
        self.relearn();
        Ok(())
    }

    /// Read every source of identity again and state how many books that
    /// named, over a banner. No sitting is folded in. The sources come in
    /// `identify::rescue`'s order, the logs last, for the sidecar arm alone.
    fn retry(&mut self, fb: &mut Framebuffer, input: &mut Input) -> Result<()> {
        let before = self.unnamed_books();
        let (headline, doing) = view::Retrying::Files.banner(self.lang.strings());
        self.banner(fb, headline, &doing, "", true)?;
        // A book that arrives with its real title in place of a file name was
        // named before and after, so the count the sources report is the only
        // one that sees it.
        let mut rescued = self.relearn();
        // A sidecar is reached through the counter its class was logged with,
        // so the logs are read again only for a class that still names no book
        // and holds no counter. Nothing else this needs is in them.
        if self.store.wants_the_logs() {
            let (headline, doing) = view::Retrying::Logs.banner(self.lang.strings());
            self.banner(fb, headline, &doing, "", true)?;
            let learned =
                self.over_the_logs(fb, headline, &doing, |store, on| store.relearn_classes(on));
            eprintln!("retry: {learned} classes the record held no counter for");
            if learned > 0 {
                self.store_it("retry");
                rescued += self.relearn();
            }
        }
        let named = before.saturating_sub(self.unnamed_books()).max(rescued);
        eprintln!("retry: {before} books no record named, {named} named");
        let (headline, said) = view::Retrying::Named(named).banner(self.lang.strings());
        self.banner(fb, headline, &said, "", true)?;
        self.hold(input, OUTCOME_LINGER)
    }

    /// Measure every sitting the device's logs still reach again, over a
    /// banner, and state how many stored rows moved. A row older than the logs
    /// keeps what it holds, and no row is given up.
    fn heal(&mut self, fb: &mut Framebuffer, input: &mut Input) -> Result<()> {
        let (headline, doing) = view::Healing::Logs.banner(self.lang.strings());
        self.banner(fb, headline, &doing, "", true)?;
        let healed = self.over_the_logs(fb, headline, &doing, |store, on| store.heal(on));
        eprintln!("heal: {healed} sittings the logs measured again");
        if healed > 0 {
            self.store_it("heal");
            self.relearn();
        }
        let (headline, said) = view::Healing::Done(healed).banner(self.lang.strings());
        self.banner(fb, headline, &said, "", true)?;
        self.hold(input, OUTCOME_LINGER)
    }

    /// Books read that no record names, whatever the page is set to show.
    fn unnamed_books(&self) -> usize {
        crate::stats::Stats::build(&self.store, self.today, true, self.settings.figures)
            .unnamed_books()
    }

    /// `catalog::read` through `Store::remember`, then every source that names
    /// a book the catalog cannot, then `Store::keep_covers` and
    /// `Stats::build`. Answers the classes the sources past the catalog named.
    fn relearn(&mut self) -> usize {
        let books = crate::catalog::read();
        let dir = self.dir.clone();
        let stated = self.store.remember(&books);
        let shelf = crate::identify::walk(std::path::Path::new(crate::sidecar::DOCUMENTS_DIR));
        let rescue = crate::identify::rescue(&mut self.store, &shelf);
        let merge = crate::annotate::fold(
            &mut self.store,
            std::path::Path::new(crate::clippings::CLIPPINGS_FILE),
            &shelf,
        );
        let refreshed = stated + rescue.named() + self.store.keep_covers(&dir);
        eprintln!(
            "reset: {} catalog rows, {refreshed} book records refreshed, {} held",
            books.len(),
            self.store.books.len(),
        );
        if refreshed > 0 || merge.read {
            self.store_it("reset");
        }
        self.covers.forget();
        self.rebuild();
        rescue.named()
    }

    /// Empty the record, keeping an archive of it first under `keep`.
    fn wipe(&mut self, keep: bool) {
        let keeping = match keep {
            true => crate::backup::Keep::Archive,
            false => crate::backup::Keep::Nothing,
        };
        let dir = self.dir.clone();
        match crate::backup::reset(&dir, &mut self.store, keeping) {
            Ok(Some(at)) => eprintln!("reset: the record is kept at {}", at.display()),
            Ok(None) => {}
            // `reset` writes the archive first and leaves `store` on a failure.
            Err(err) => {
                eprintln!("reset: nothing was reset — {err}");
            }
        }
    }

    /// Fold the archive at `at` in the list back into the record, counting the
    /// files coming out of it onto the banner.
    fn restore(&mut self, fb: &mut Framebuffer, headline: &str, note: &[String], at: usize) {
        let held = crate::backup::list(self.dir());
        let Some(backup) = held.get(at) else {
            return;
        };
        let path = backup.path.clone();
        let dir = self.dir.clone();
        // `backup::take` holds `self.store`, `App::banner` holds `self`.
        let counting = self.lang.strings().step_files;
        let mut store = std::mem::take(&mut self.store);
        let mut painted = usize::MAX;
        let took = crate::backup::take(&dir, &path, &mut store, &mut |done, total| {
            if done == painted {
                return;
            }
            painted = done;
            let step = crate::ui::splash::step(counting, done, total);
            let _ = self.banner(fb, headline, note, &step, false);
        });
        self.store = store;
        match took {
            Ok(taken) => {
                eprintln!("restore: {} sittings from {}", taken.added, path.display());
                self.store_it("restore");
                // `Taken::whole`: `store` holds every row the archive carried.
                if taken.whole {
                    match std::fs::remove_file(&path) {
                        Ok(()) => eprintln!("restore: {} is now in the record", path.display()),
                        Err(err) => eprintln!("restore: {} stands — {err}", path.display()),
                    }
                } else {
                    eprintln!(
                        "restore: {} holds more than the record took",
                        path.display()
                    );
                }
            }
            Err(err) => eprintln!("restore: {} would not open — {err}", path.display()),
        }
    }

    /// One pass over the device's logs, counting the files it opens onto the
    /// banner. The pass holds `self.store` and `App::banner` holds `self`, and
    /// the record stands out of both for the length of it.
    fn over_the_logs(
        &mut self,
        fb: &mut Framebuffer,
        headline: &str,
        note: &[String],
        pass: impl FnOnce(&mut crate::store::Store, &mut dyn FnMut(usize, usize)) -> usize,
    ) -> usize {
        let counting = self.lang.strings().step_logs;
        let mut store = std::mem::take(&mut self.store);
        let mut painted = usize::MAX;
        let out = pass(&mut store, &mut |done, total| {
            if done == painted {
                return;
            }
            painted = done;
            let step = crate::ui::splash::step(counting, done, total);
            let _ = self.banner(fb, headline, note, &step, false);
        });
        self.store = store;
        out
    }

    /// `Store::rebuild`, counting the log files it opens onto the banner.
    fn reread(&mut self, fb: &mut Framebuffer, headline: &str, note: &[String]) {
        let added = self.over_the_logs(fb, headline, note, |store, on| store.rebuild(on));
        eprintln!("reread: {added} sittings the record did not hold");
        self.store_it("reread");
    }

    /// Put one book's reading back to zero, taking its record with it under
    /// `forget`. An archive of that book goes down first, and a book whose
    /// archive will not write is left alone.
    fn clear_book(&mut self, index: usize, forget: bool) -> Action {
        self.state.asked = None;
        let Some(book) = self.stats.books.get(index) else {
            return Action::Redraw;
        };
        let (extent, key) = (book.extent, book.cde_key.clone());
        let one = self.store.one_book(extent, &key);
        if let Err(err) = crate::backup::keep_book(self.dir(), &one, &self.store.mark.clone()) {
            eprintln!("clear: nothing was cleared — {err}");
            return Action::Redraw;
        }
        let went = match forget {
            true => self.store.forget_book(extent, &key),
            false => self.store.clear_book(extent, &key),
        };
        eprintln!("clear: {went} sittings, forget {forget}");
        self.store_it("clear");
        // `Stats::books` drops a book with no sittings.
        self.state.book = None;
        self.covers.forget();
        self.rebuild();
        Action::Redraw
    }

    /// `dir` through [`App::paged`].
    fn swiped(&mut self, dir: SwipeDir) -> Action {
        self.paged(dir.step())
    }

    /// One step from the bezel, [`App::swiped`]'s counterpart.
    fn pressed(&mut self, page: PageButton) -> Action {
        self.paged(page.step())
    }

    /// One step forward or back: a span on Rhythm, a page of the list, the
    /// next book.
    /// A step across the open book, which is one track: the statistics page,
    /// then each page of its marks in turn. A step off either end is no step —
    /// the book screen is left by a tab, not by stepping past it.
    fn through_book(&mut self, book: usize, by: i64) -> Action {
        let area = chrome::content_box(&self.theme);
        let (_, rest) = area.split_top(view::book::picker_height(&self.theme));
        let opens = view::marks::pages(&mut self.text, &self.theme, &self.stats, book, rest);
        let forward = by > 0;
        match self.state.book_tab {
            view::BookTab::Statistics if forward => {
                self.state.go_in_book(view::BookTab::Marks);
                Action::Redraw
            }
            view::BookTab::Statistics => Action::Nothing,
            view::BookTab::Marks => {
                let page = opens
                    .iter()
                    .rposition(|o| *o <= self.state.marks_from)
                    .unwrap_or(0);
                let next = match forward {
                    true => page + 1,
                    false => match page {
                        // The head of the list steps back onto the statistics.
                        0 => {
                            self.state.go_in_book(view::BookTab::Statistics);
                            return Action::Redraw;
                        }
                        _ => page - 1,
                    },
                };
                match opens.get(next) {
                    Some(opens) => {
                        self.state.marks_from = *opens;
                        Action::Redraw
                    }
                    None => Action::Nothing,
                }
            }
        }
    }

    fn paged(&mut self, by: i64) -> Action {
        if let Some(at) = self.state.book {
            return self.through_book(at, by);
        }
        match self.state.tab {
            // `config::draw` clamps `config_page` to the pages it has.
            Tab::Config => {
                let at = self.state.config_page as i64;
                self.state.config_page = (at + by).max(0) as usize;
                Action::Redraw
            }
            // Nothing to page through.
            Tab::Home => Action::Nothing,
            Tab::Rhythm => {
                if !self.state.shift(by) {
                    return Action::Nothing;
                }
                self.state.list_from = 0;
                Action::Redraw
            }
            Tab::Books if self.state.search.is_some() => {
                let Some(search) = self.state.search.clone() else {
                    return Action::Nothing;
                };
                let area = chrome::content_box(&self.theme);
                let count =
                    view::search::listed(&self.stats, &search.query, self.settings.show_uncovered)
                        .len();
                let box_ = view::search::results_box(&self.theme, area, search.keyboard);
                let step = view::books::rows_per_page(&self.theme, box_) as i64;
                let last = view::books::last_page_at(&self.theme, box_, count);
                let from = search.from as i64 + by * step;
                let capped = from.clamp(0, last as i64) as usize;
                if capped == search.from {
                    return Action::Nothing;
                }
                if let Some(search) = self.state.search.as_mut() {
                    search.from = capped;
                }
                Action::Redraw
            }
            Tab::Books => {
                // `rows_per_page` states the step.
                let chips = !self.stats.books.is_empty();
                let area =
                    view::books::list_box(&self.theme, chrome::content_box(&self.theme), chips);
                let over = self
                    .state
                    .window
                    .map(|window| window.days(self.settings.week_start));
                let count = view::books::listed(
                    &self.stats,
                    self.state.shelf,
                    self.state.sort,
                    over,
                    self.settings.show_uncovered,
                )
                .len();
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
    /// Name the books nothing names yet, over the whole screen.
    Retry,
    /// Measure every sitting the logs still reach again, over the whole
    /// screen.
    Heal,
    /// Carry out one of the config page's resets, over the whole screen.
    Resetting(view::Reset),
}

impl Drop for App {
    /// A keyboard left up outlives the app: the reader is handed back to the
    /// framework with a third of the screen taken by a keyboard nothing is
    /// listening to.
    fn drop(&mut self) {
        if self.keyboard {
            crate::keyboard::close();
        }
    }
}
