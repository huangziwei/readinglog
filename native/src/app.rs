//! The run loop: what is drawn, and what a touch does to it. [`App::draw`]
//! redraws the whole screen and presents it in one
//! [`Framebuffer::send_update`].

use std::fmt::Write as _;
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
use crate::settings::{Scope, Settings, SittingFloor};
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
    record: crate::record::Record,
    stats: Stats,
    state: State,
    today: i64,
    now: i64,
    /// What `eink::fb::has_cfa` answered at startup.
    colour: bool,
    /// Where the record and the backups beside it live.
    dir: std::path::PathBuf,
    /// Where every touchable thing was on the last frame.
    hits: Vec<(Hit, crate::ui::paint::Rect)>,
    /// Whether the on-screen keyboard stands. Set by
    /// [`App::reconcile_keyboard`].
    keyboard: bool,
    /// The lipc service the keyboard sets its IME properties on, opened by
    /// [`App::reconcile_keyboard`] with the first keyboard.
    lipc: Option<crate::lipc::Service>,
}

impl App {
    pub fn new(record: crate::record::Record, theme: Theme, text: TextRenderer) -> Self {
        let (today, now) = date::now();
        let settings = Settings::load(Lang::detect());
        let settings_scope = settings.scope;
        let stats = Stats::build(
            &record,
            today,
            settings.show_unidentified,
            settings.stats_from,
            settings.sitting_floor,
        );
        // `colour` gates the schemes; the header block states it too.
        let colour = crate::eink::fb::has_cfa();
        Self {
            theme,
            lang: settings.language,
            settings,
            text,
            covers: Covers::default(),
            record,
            stats,
            colour,
            dir: std::path::PathBuf::from(crate::record::RECORD_DIR),
            state: State {
                // The search opens on the list it last showed, which outlives
                // the launch in `Settings::scope`.
                scope: settings_scope,
                ..State::new(today)
            },
            today,
            now,
            hits: Vec::new(),
            keyboard: false,
            lipc: None,
        }
    }

    /// What the stats hold, as the launch line's field: books drawn, then the
    /// reading on unidentified books, in seconds.
    pub fn drawn(&self) -> String {
        let mut out = format!("draw={}b", self.stats.books.len());
        if self.stats.unidentified_seconds > 0 {
            let _ = write!(
                out,
                "/{}u{}s",
                self.stats.unidentified_books(),
                self.stats.unidentified_seconds
            );
        }
        if self.stats.skipped_seconds > 0 {
            let _ = write!(out, "/{}x", self.stats.skipped_seconds);
        }
        out
    }

    /// Total the record again, at the day and the settings held.
    fn build_stats(&mut self) {
        self.stats = Stats::build(
            &self.record,
            self.today,
            self.settings.show_unidentified,
            self.settings.stats_from,
            self.settings.sitting_floor,
        );
    }

    /// Draw at `size`, whatever is stored.
    pub fn set_text_size(&mut self, size: crate::settings::TextSize) {
        self.settings.text_size = size;
        self.theme = Theme::sized(self.theme.screen.w as u32, self.theme.screen.h as u32, size);
    }

    /// Size `theme` again to `fb` and the stored `text_size`.
    fn took_size(&mut self, fb: &Framebuffer) {
        self.theme = Theme::sized(fb.var.xres, fb.var.yres, self.settings.text_size);
    }

    /// Draw in `lang`, past `Lang::detect`.
    pub fn set_language(&mut self, lang: Lang) {
        self.lang = lang;
        self.settings.language = lang;
    }

    /// Count the sittings on unidentified books, or leave them out of every total.
    pub fn set_unidentified(&mut self, show: bool) {
        self.settings.show_unidentified = show;
        self.build_stats();
    }

    /// Count a sitting of at least `floor` as reading.
    pub fn set_sitting_floor(&mut self, floor: SittingFloor) {
        self.settings.sitting_floor = floor;
        self.build_stats();
    }

    /// Take the stats from `from`, and total again.
    pub fn set_stats_from(&mut self, from: crate::settings::StatsFrom) {
        self.settings.stats_from = from;
        self.build_stats();
    }

    /// List the books no cover can be drawn for, or leave them off the lists.
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
        self.build_stats();
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
    pub fn ask_about(&mut self, about: Option<view::Act>) {
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

    /// List the backups over the config page, from the row at `from`, or take
    /// the list down.
    pub fn set_backups(&mut self, from: Option<usize>) {
        self.state.backups = from;
    }

    /// Draw the open book at `tab`, whatever it was left on, with its highlights
    /// opened `from` rows down.
    pub fn set_book_tab(&mut self, tab: view::BookTab, from: usize) {
        self.state.book_tab = tab;
        self.state.highlights_from = from;
    }

    /// Draw the open book's highlights from `order`'s own end.
    pub fn set_highlights_way(&mut self, order: view::Way) {
        self.state.highlights_way = order;
    }

    /// Draw the open book's Graphs tab on `reading`, counted from the first,
    /// with its list of readings opened to hold it.
    pub fn set_reading(&mut self, reading: Option<usize>) {
        self.state.reading = reading;
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

    /// Run that order `way`.
    pub fn set_sort_way(&mut self, way: crate::view::Way) {
        self.state.sort_way = way;
        self.state.books_from = 0;
    }

    /// Put a search up over the Books screen, or take one down.
    pub fn set_search(&mut self, search: Option<view::Search>) {
        self.state.search = search;
    }

    /// Put one up over the open book's own screen, or take it down.
    pub fn set_book_search(&mut self, search: Option<view::Search>) {
        self.state.book_search = search;
    }

    /// Which list that search names.
    pub fn set_scope(&mut self, scope: Scope) {
        self.state.scope = scope;
    }

    /// Takes `keysyms` into the search in play, answering whether it moved.
    fn typed(&mut self, keysyms: &[u32]) -> bool {
        use crate::eink::keysym::{Typed, of_keysym};
        if keysyms.is_empty() || self.state.searching().is_none() {
            return false;
        }
        let mut moved = false;
        for keysym in keysyms {
            let Some(said) = of_keysym(*keysym) else {
                continue;
            };
            // `Typed::Escape` drops the rest of `keysyms`.
            if said == Typed::Escape {
                *self.state.searching_slot() = None;
                return true;
            }
            let Some(search) = self.state.searching_mut() else {
                return moved;
            };
            match said {
                Typed::Char(said) => {
                    search.typed(said);
                    moved = true;
                }
                Typed::Backspace => moved |= search.backspace(),
                Typed::Enter => {
                    moved |= search.keyboard;
                    search.keyboard = false;
                }
                Typed::Escape => unreachable!("answered above"),
            }
        }
        moved
    }

    /// Whether a touch at `y` lands on the on-screen keyboard.
    fn under_keyboard(&self, y: i32) -> bool {
        self.keyboard
            && y >= self.theme.screen.bottom() - crate::keyboard::height(self.theme.screen.h)
    }

    /// Raises or dismisses the keyboard to match the search in play, where
    /// `on_screen`.
    fn reconcile_keyboard(&mut self, on_screen: bool) {
        let want = on_screen && self.state.searching().is_some_and(|s| s.keyboard);
        if want == self.keyboard {
            return;
        }
        self.keyboard = want;
        if want && self.lipc.is_none() {
            match crate::lipc::Service::open(crate::keyboard::CLIENT) {
                Ok(service) => {
                    eprintln!("lipc: {} is open", service.name());
                    self.lipc = Some(service);
                }
                // Every key the candidate engine does not eat arrives over X,
                // which needs none of this.
                Err(err) => eprintln!("?? lipc: {err:#} — Latin typing only"),
            }
        }
        let _ = match want {
            true => crate::keyboard::open(),
            false => crate::keyboard::close(),
        };
    }

    /// Takes what the keyboard set on this app's lipc service into the search
    /// in play, answering whether it moved.
    fn committed(&mut self) -> bool {
        let Some(service) = self.lipc.as_mut() else {
            return false;
        };
        let sets = service.drain();
        let Some(search) = self.state.searching_mut() else {
            return false;
        };
        let mut moved = false;
        for set in sets {
            moved |= search.set(&set.property, &set.value);
        }
        moved
    }

    /// List the books on `shelf`, whatever the Books screen was left on.
    pub fn set_shelf(&mut self, shelf: crate::view::Shelf) {
        self.state.shelf = shelf;
        self.state.books_from = 0;
    }

    /// List the books of `window`, whatever span the screen was left on.
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
        let summary = match state.tab == Tab::Config && state.book.is_none() {
            true => view::config::Summary::of(
                &self.record,
                &self.stats,
                &self.dir,
                !self.record.reset_at.is_empty(),
            ),
            false => view::config::Summary::default(),
        };
        // Every backup opened once, on the frame that lists them.
        let listed = match state.backups.is_some() {
            true => view::config::Summary::listed(&self.dir, &self.record, self.lang),
            false => view::backups::Listed::default(),
        };
        self.frame(fb, &mut |cx, area| match state.book {
            Some(index) => {
                view::book::draw(cx, area, index, &state);
                if let Some((at, ask)) = state.asked
                    && at == index
                {
                    view::book::asking(cx, area, ask, index);
                }
            }
            None => match state.tab {
                Tab::Config => {
                    view::config::draw(cx, area, &settings, colour, &summary, state.config_page);
                    if let Some(from) = state.backups {
                        view::backups::draw(cx, area, &listed, from);
                    }
                    if let Some(confirm) = &state.confirm {
                        view::config::asking(cx, area, confirm);
                    }
                }
                Tab::Home => view::home::draw(cx, area, state.list_from),
                Tab::Rhythm => view::rhythm::draw(cx, area, &state),
                Tab::Books => match &state.search {
                    Some(search) => view::search::draw(cx, area, search, state.scope),
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
            stats_from: self.settings.stats_from,
            uncovered: self.settings.show_uncovered,
            palette: crate::ui::paint::Palette::for_panel(self.settings.color_scheme, self.colour),
            stats: &self.stats,
            today: self.today,
            now: self.now,
            was_reset: !self.record.reset_at.is_empty(),
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
        match outcome {
            Outcome::Failed(_) => eprintln!("!! update: {outcome:?}"),
            _ => eprintln!("update: {outcome:?}"),
        }
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
        self.draw(fb)?;
        let mut down: Option<(u32, u32)> = None;
        loop {
            // `EVIOCGRAB` is exclusive against the X server.
            input.set_keyboard(self.keyboard);
            // A `KeyPress` arrives on the X connection and a commit on the
            // lipc socket, neither of them an input device.
            input.watch([fb.raw_fd(), self.lipc.as_ref().map(|s| s.raw_fd())]);
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
                        Action::Acting(act) => {
                            self.acting(fb, act)?;
                            self.draw(fb)?;
                        }
                        Action::BackingUp => {
                            self.backing_up(fb, input)?;
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
                        Err(err) => eprintln!("!! screenshot: {err:#}"),
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
                    let typed = self.typed(&pump.typed) | self.committed();
                    if input.follow_orientation() {
                        down = None;
                    }
                    // `Pump::resized` sizes `theme` again and draws the whole screen.
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
            std::path::Path::new(crate::record::RECORD_DIR),
            &book.location,
            at,
        ) {
            Ok(()) => Action::Quit,
            Err(err) => {
                eprintln!("!! open: {} would not be asked for: {err:#}", book.location);
                Action::Nothing
            }
        }
    }

    /// Set `Book::finished` on the book at `index`, total the record again
    /// and write it out. A failed `save` leaves the mark in `stats` and off the
    /// disk.
    fn set_finished(&mut self, index: usize, on: bool) -> Action {
        let Some(book) = self.stats.books.get(index) else {
            return Action::Nothing;
        };
        let (extent, key, cde_type) = (book.extent, book.cde_key.clone(), book.cde_type.clone());
        if !self.record.set_finished(extent, &key, on) {
            return Action::Nothing;
        }
        self.hand_over_mark(extent, &key, &cde_type, on);
        self.build_stats();
        if let Err(err) = self
            .record
            .save(std::path::Path::new(crate::record::RECORD_DIR))
        {
            eprintln!("!! finished: the mark did not reach the record: {err:#}");
        }
        Action::Redraw
    }

    /// Hand `read` to `mark::set`, and write down the `p_readState` a taken
    /// call leaves. A refused call leaves the record's own value standing.
    fn hand_over_mark(&mut self, extent: i64, key: &str, cde_type: &str, read: bool) {
        if crate::mark::set(key, cde_type, read) {
            self.record
                .note_mark(extent, key, crate::library::read_state_for(read));
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

    /// The book at `index` gives up its progress and its mark, then opens
    /// through `open::uri`.
    fn restart(&mut self, index: usize) -> Action {
        if let Some(book) = self.stats.books.get(index) {
            let (extent, key, cde_type) =
                (book.extent, book.cde_key.clone(), book.cde_type.clone());
            if self.record.restart(extent, &key, self.today) {
                self.hand_over_mark(extent, &key, &cde_type, false);
                self.build_stats();
                if let Err(err) = self
                    .record
                    .save(std::path::Path::new(crate::record::RECORD_DIR))
                {
                    eprintln!("!! restart: the record was not written: {err:#}");
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
            Hit::StatsFrom(pick) => {
                if self.settings.stats_from == pick {
                    return Action::Nothing;
                }
                self.settings.stats_from = pick;
                // Every total on every screen comes off `stats`.
                self.build_stats();
                self.settings.save();
            }
            Hit::SittingFloor(pick) => {
                if self.settings.sitting_floor == pick {
                    return Action::Nothing;
                }
                self.settings.sitting_floor = pick;
                // Every total on every screen comes off `stats`.
                self.build_stats();
                self.state.books_from = 0;
                self.state.list_from = 0;
                self.settings.save();
            }
            Hit::ShowUnidentified(pick) => {
                if self.settings.show_unidentified == pick {
                    return Action::Nothing;
                }
                self.settings.show_unidentified = pick;
                // Every total on every screen comes off `stats`.
                self.build_stats();
                self.state.books_from = 0;
                self.state.list_from = 0;
                self.settings.save();
            }
            Hit::ShowUncovered(pick) => {
                if self.settings.show_uncovered == pick {
                    return Action::Nothing;
                }
                self.settings.show_uncovered = pick;
                // `show_uncovered` makes both lists shorter or longer, and a
                // page held part way down one of them names other books.
                self.state.books_from = 0;
                self.state.list_from = 0;
                self.settings.save();
            }
            Hit::Book(index) => self.state.open_book(index),
            // A tap on the tab showing is no navigation; on the Highlights tab it
            // turns the list over.
            Hit::BookTab(tab) => {
                if !self.state.go_in_book(tab) {
                    match tab {
                        view::BookTab::Highlights => self.state.flip_highlights(),
                        _ => return Action::Nothing,
                    }
                }
            }
            Hit::HighlightsPage(from) => {
                if self.state.highlights_from == from {
                    return Action::Nothing;
                }
                self.state.highlights_from = from;
            }
            // A passage the search found opens the book it was highlighted in, at
            // its own place in that book's list.
            Hit::Highlight(book, at) => {
                self.state
                    .open_highlight(book, at, self.stats.highlights_held(book))
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
            Hit::Reading(at) => {
                if self.state.reading == Some(at) {
                    return Action::Nothing;
                }
                self.state.reading = Some(at);
            }
            Hit::Sorted(order) => {
                if self.state.sort == order {
                    return Action::Nothing;
                }
                self.state.sort = order;
                self.state.books_from = 0;
            }
            Hit::SortWay(way) => {
                if self.state.sort_way == way {
                    return Action::Nothing;
                }
                self.state.sort_way = way;
                self.state.books_from = 0;
            }
            Hit::Scoped(pick) => {
                if self.state.scope == pick {
                    return Action::Nothing;
                }
                self.state.scope = pick;
                // The next launch opens the search on the list this one was
                // last looking at.
                self.settings.scope = pick;
                self.settings.save();
                // The other scope is another list, and a page held part way
                // down this one names nothing in it.
                if let Some(search) = self.state.search.as_mut() {
                    search.from = 0;
                }
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
            // The open book's own search, or the Books tab's: one field
            // stands at a time, and `State::searching_slot` names which.
            Hit::Search => {
                *self.state.searching_slot() = Some(view::Search {
                    keyboard: true,
                    ..view::Search::default()
                });
            }
            Hit::SearchField => {
                let Some(search) = self.state.searching_mut() else {
                    return Action::Nothing;
                };
                if search.keyboard {
                    return Action::Nothing;
                }
                search.keyboard = true;
            }
            Hit::SearchClear => {
                let slot = self.state.searching_slot();
                let Some(search) = slot.as_mut() else {
                    return Action::Nothing;
                };
                if search.query.is_empty() {
                    // An empty `query` takes the search off.
                    *slot = None;
                } else {
                    search.query.clear();
                    search.from = 0;
                }
            }
            Hit::SearchPage(at) => {
                let Some(search) = self.state.searching_mut() else {
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
            Hit::Prev => return self.paged(-1),
            Hit::Next => return self.paged(1),
            Hit::Clear(index) => return self.put(index, view::Ask::Clear),
            Hit::ClearBook(index) => return self.clear_book(index, false),
            Hit::ForgetBook(index) => return self.clear_book(index, true),
            Hit::Reset(keep) => {
                return self.ask_question(view::Act::Reset(keep));
            }
            Hit::BackUp => return Action::BackingUp,
            Hit::Backups => self.state.backups = Some(0),
            Hit::BackupsPage(at) => self.state.backups = Some(at),
            Hit::BackupsClose => self.state.backups = None,
            Hit::Restore(at) => {
                return self.ask_question(view::Act::Restore(at));
            }
            Hit::Deleted(at) => {
                self.state.confirm = None;
                self.delete_backup(at);
            }
            Hit::Rebuild => return self.ask_question(view::Act::Rebuild),
            // `App::acting` carries these out, holding `fb`.
            Hit::ResetDone(keep) => return Action::Acting(view::Act::Reset(keep)),
            Hit::Restored(at) => return Action::Acting(view::Act::Restore(at)),
            Hit::Rebuilt => return Action::Acting(view::Act::Rebuild),
        }
        Action::Redraw
    }

    /// Where the record lives.
    fn dir(&self) -> &std::path::Path {
        &self.dir
    }

    /// Draw and act against a record outside [`crate::record::RECORD_DIR`].
    pub fn set_dir(&mut self, dir: std::path::PathBuf) {
        self.dir = dir;
    }

    /// Write the record, printing `what` and the error on a failed write.
    fn save_record(&self, what: &str) {
        if let Err(err) = self.record.save(self.dir()) {
            eprintln!("!! {what}: the record was not written: {err:#}");
        }
    }

    /// Put one of the config page's questions up, with the figures it states
    /// gathered here: the dialog itself walks nothing.
    fn ask_question(&mut self, about: view::Act) -> Action {
        let (covers, backups) = crate::backup::sizes(self.dir());
        // `Record::sittings` is what `Act::Reset` acts on.
        let held = self.record.sittings.len();
        let confirm = match about {
            view::Act::Reset(keep) => view::Confirm {
                about,
                sittings: held,
                books: self.stats.book_count(),
                // The backup's weight, the record's off its file, or the room
                // `covers::sweep` returns.
                bytes: match keep {
                    true => covers + record_bytes(self.dir()),
                    false => covers,
                },
                named: crate::backup::name(
                    crate::backup::Kind::Record,
                    &crate::backup::stamped_now(),
                    "",
                ),
            },
            view::Act::Restore(at) => {
                let held = crate::backup::list(self.dir());
                let Some(backup) = held.get(at) else {
                    return Action::Nothing;
                };
                // `peek` once: the figures the question states. `inside` is
                // totalled whole.
                let inside = crate::backup::peek(&backup.path).unwrap_or_default();
                let held = crate::stats::Stats::build(
                    &inside,
                    self.today,
                    true,
                    self.settings.stats_from,
                    SittingFloor::All,
                );
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
            view::Act::Rebuild => view::Confirm {
                about,
                sittings: 0,
                books: 0,
                bytes: backups,
                named: String::new(),
            },
        };
        if self.state.confirm.as_ref() == Some(&confirm) {
            return Action::Nothing;
        }
        self.state.confirm = Some(confirm);
        Action::Redraw
    }

    /// Carry `act` out under a banner over the whole screen, drawn from
    /// `Act::doing`. `restore` and `rebuild` count their files onto it.
    fn acting(&mut self, fb: &mut Framebuffer, act: view::Act) -> Result<()> {
        self.state.confirm = None;
        let (headline, note) = act.doing(self.lang.strings());
        self.banner(fb, &headline, &note, "", true)?;
        match act {
            view::Act::Reset(keep) => self.reset(keep),
            view::Act::Restore(at) => self.restore(fb, &headline, &note, at),
            view::Act::Rebuild => self.rebuild(fb, &headline, &note),
        }
        self.relearn();
        Ok(())
    }

    /// `library::read` through `Record::remember`, then every source that identifies
    /// a book the library cannot, then `Record::keep_covers` and
    /// `Stats::build`. Answers the end positions the sources past the library identified.
    fn relearn(&mut self) -> usize {
        let books = crate::library::read();
        let dir = self.dir.clone();
        let stated = self.record.remember(&books);
        // Neither gate holds this pass back: both are dropped and written
        // again from what it reads.
        let clips = std::path::Path::new(crate::clippings::CLIPPINGS_FILE);
        let documents = std::path::Path::new(crate::sidecar::DOCUMENTS_DIR);
        self.record.sources = None;
        self.record.gate = None;
        let survey = crate::sidecar::survey(documents);
        let held = crate::identify::gate(
            &self.record,
            std::path::Path::new(crate::vocab::VOCAB_DB),
            clips,
            &survey,
        );
        let sidecars = crate::identify::walk(documents, true, Some(&dir));
        // One read of the file, for both passes.
        let clippings = crate::clippings::read(clips);
        // Measured before either pass places an instant on a clock.
        self.record
            .clock
            .observe(crate::highlights::clocks_seen(&clippings, &sidecars));
        let identified = crate::identify::from_sources(&mut self.record, &clippings, &sidecars);
        self.record.sources = Some(held);
        let merge =
            crate::highlights::fold(&mut self.record, clips, &clippings, &sidecars, &survey);
        let refreshed = stated + identified.found() + self.record.keep_covers(&dir).kept;
        eprintln!(
            "reset: {} library rows, {refreshed} books refreshed, {} held",
            books.len(),
            self.record.books.len(),
        );
        if refreshed > 0 || merge.read {
            self.save_record("reset");
        }
        self.covers.forget();
        self.build_stats();
        identified.found()
    }

    /// Write a backup of the record under a banner, and state what it came
    /// to on an outcome banner `App::hold` keeps up.
    fn backing_up(&mut self, fb: &mut Framebuffer, input: &mut Input) -> Result<()> {
        let s = self.lang.strings();
        let headline = s.back_up.to_string();
        let doing = vec![s.back_up_doing.to_string()];
        self.banner(fb, &headline, &doing, "", true)?;
        let said = match crate::backup::back_up(&self.dir.clone(), &self.record) {
            Ok(at) => {
                eprintln!("backup: the record is at {}", at.display());
                let named = at
                    .file_name()
                    .map(|n| n.to_string_lossy().into_owned())
                    .unwrap_or_default();
                let bytes = std::fs::metadata(&at).map(|m| m.len()).unwrap_or_default();
                s.back_up_done
                    .replace("{file}", &named)
                    .replace("{size}", &view::config::bytes(bytes))
            }
            Err(err) => {
                eprintln!("!! backup: nothing was written — {err}");
                s.back_up_failed.to_string()
            }
        };
        self.banner(fb, &headline, &[said], "", true)?;
        self.hold(input, OUTCOME_LINGER)
    }

    /// Take one backup off disk, by its place in `backup::list`.
    fn delete_backup(&mut self, at: usize) {
        let held = crate::backup::list(self.dir());
        let Some(backup) = held.get(at) else {
            return;
        };
        match crate::backup::remove(&backup.path) {
            Ok(()) => eprintln!("backup: {} is gone", backup.path.display()),
            Err(err) => eprintln!("!! backup: {} stands — {err}", backup.path.display()),
        }
        // The last backup gone takes the list down.
        if held.len() <= 1 {
            self.state.backups = None;
        }
    }

    /// Empty the record, keeping a backup of it first under `keep`.
    fn reset(&mut self, keep: bool) {
        let keeping = match keep {
            true => crate::backup::Keep::Backup,
            false => crate::backup::Keep::Nothing,
        };
        let dir = self.dir.clone();
        match crate::backup::reset(&dir, &mut self.record, keeping) {
            Ok(Some(at)) => eprintln!("reset: the record is kept at {}", at.display()),
            Ok(None) => {}
            // `reset` writes the backup first and leaves `record` on a failure.
            Err(err) => {
                eprintln!("!! reset: nothing was reset — {err}");
            }
        }
    }

    /// Fold the backup at `at` in the list back into the record, counting the
    /// files coming out of it onto the banner.
    fn restore(&mut self, fb: &mut Framebuffer, headline: &str, note: &[String], at: usize) {
        let held = crate::backup::list(self.dir());
        let Some(backup) = held.get(at) else {
            return;
        };
        let path = backup.path.clone();
        let dir = self.dir.clone();
        // `backup::restore` holds `self.record`, `App::banner` holds `self`.
        let counting = self.lang.strings().step_files;
        let mut record = std::mem::take(&mut self.record);
        let mut painted = usize::MAX;
        let restored = crate::backup::restore(&dir, &path, &mut record, &mut |done, total| {
            if done == painted {
                return;
            }
            painted = done;
            let step = crate::ui::splash::step(counting, done, total);
            let _ = self.banner(fb, headline, note, &step, false);
        });
        self.record = record;
        match restored {
            Ok(merged) => {
                eprintln!("restore: {} sittings from {}", merged.added, path.display());
                self.save_record("restore");
                // `Merged::whole`: `record` holds every row the backup carried.
                // The file stands either way, and `backup::remove` is the way
                // out.
                eprintln!("restore: {} whole {}", path.display(), merged.whole);
            }
            Err(err) => eprintln!("!! restore: {} would not open — {err}", path.display()),
        }
    }

    /// One pass over the logs, counting the files it opens onto the
    /// banner. The pass holds `self.record` and `App::banner` holds `self`, and
    /// the record stands out of both for the length of it.
    fn over_the_logs(
        &mut self,
        fb: &mut Framebuffer,
        headline: &str,
        note: &[String],
        pass: impl FnOnce(&mut crate::record::Record, &mut dyn FnMut(usize, usize)) -> usize,
    ) -> usize {
        let counting = self.lang.strings().step_logs;
        let mut record = std::mem::take(&mut self.record);
        let mut painted = usize::MAX;
        let out = pass(&mut record, &mut |done, total| {
            if done == painted {
                return;
            }
            painted = done;
            let step = crate::ui::splash::step(counting, done, total);
            let _ = self.banner(fb, headline, note, &step, false);
        });
        self.record = record;
        out
    }

    /// `Record::rebuild`, counting the log files it opens onto the banner.
    fn rebuild(&mut self, fb: &mut Framebuffer, headline: &str, note: &[String]) {
        let added = self.over_the_logs(fb, headline, note, |record, on| record.rebuild(on));
        eprintln!("rebuild: {added} sittings the record did not hold");
        self.save_record("rebuild");
    }

    /// Put one book's reading back to zero, taking the book with it under
    /// `forget`. A backup of that book goes down first, and a book whose
    /// backup will not write is left alone.
    fn clear_book(&mut self, index: usize, forget: bool) -> Action {
        self.state.asked = None;
        let Some(book) = self.stats.books.get(index) else {
            return Action::Redraw;
        };
        let (extent, key) = (book.extent, book.cde_key.clone());
        let one = self.record.one_book(extent, &key);
        if let Err(err) = crate::backup::keep_book(self.dir(), &one, &crate::backup::stamped_now())
        {
            eprintln!("!! clear: nothing was cleared — {err}");
            return Action::Redraw;
        }
        let went = match forget {
            true => self.record.forget_book(extent, &key),
            false => self.record.clear_book(extent, &key),
        };
        eprintln!("clear: {went} sittings, forget {forget}");
        self.save_record("clear");
        // `Stats::books` drops a book with no sittings.
        self.state.book = None;
        self.covers.forget();
        self.build_stats();
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

    /// One step across the open book: `BookTab::Statistics`, `BookTab::Graphs`,
    /// then each page of `view::highlights::pages` in turn. A step off either end
    /// answers `Action::Nothing`.
    fn through_book(&mut self, book: usize, by: i64) -> Action {
        let area = chrome::content_box(&self.theme);
        let (_, rest) = area.split_top(view::book::picker_height(&self.theme));
        // A field standing over the book pages what it found.
        if let Some(open) = self.state.book_search.clone() {
            return self.through_found(book, rest, &open, by);
        }
        let opens = view::highlights::pages(
            &mut self.text,
            &self.theme,
            &self.stats,
            book,
            rest,
            self.state.highlights_way,
            None,
        );
        let forward = by > 0;
        match self.state.book_tab {
            view::BookTab::Statistics if forward => {
                self.state.go_in_book(view::BookTab::Graphs);
                Action::Redraw
            }
            view::BookTab::Statistics => Action::Nothing,
            view::BookTab::Graphs => {
                self.state.go_in_book(match forward {
                    true => view::BookTab::Highlights,
                    false => view::BookTab::Statistics,
                });
                Action::Redraw
            }
            view::BookTab::Highlights => {
                let page = opens
                    .iter()
                    .rposition(|o| *o <= self.state.highlights_from)
                    .unwrap_or(0);
                let next = match forward {
                    true => page + 1,
                    false => match page {
                        // The head of the list steps back onto the graphs.
                        0 => {
                            self.state.go_in_book(view::BookTab::Graphs);
                            return Action::Redraw;
                        }
                        _ => page - 1,
                    },
                };
                match opens.get(next) {
                    Some(opens) => {
                        self.state.highlights_from = *opens;
                        Action::Redraw
                    }
                    None => Action::Nothing,
                }
            }
        }
    }

    /// One step through the passages a search found in `book`, drawn into
    /// `rest`. A step off either end answers `Action::Nothing`.
    fn through_found(
        &mut self,
        book: usize,
        rest: crate::ui::paint::Rect,
        open: &view::Search,
        by: i64,
    ) -> Action {
        let opens = view::highlights::pages(
            &mut self.text,
            &self.theme,
            &self.stats,
            book,
            rest,
            self.state.highlights_way,
            Some(open),
        );
        let page = opens.iter().rposition(|o| *o <= open.from).unwrap_or(0) as i64;
        let last = opens.len().saturating_sub(1) as i64;
        let from = opens
            .get((page + by).clamp(0, last) as usize)
            .copied()
            .unwrap_or(0);
        if from == open.from {
            return Action::Nothing;
        }
        if let Some(search) = self.state.book_search.as_mut() {
            search.from = from;
        }
        Action::Redraw
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
                let capped = match self.state.scope {
                    Scope::Books => {
                        let count = view::search::listed(
                            &self.stats,
                            &search.query,
                            self.settings.show_uncovered,
                        )
                        .len();
                        let step =
                            view::search::rows_per_page(&self.theme, area, search.keyboard) as i64;
                        let last =
                            view::search::last_page_at(&self.theme, area, search.keyboard, count);
                        let from = search.from as i64 + by * step;
                        from.clamp(0, last as i64) as usize
                    }
                    // A page steps to the next entry of `opens`, every row
                    // being as tall as its own words.
                    Scope::Highlights => {
                        let opens = view::search::highlight_pages(
                            &mut self.text,
                            &self.theme,
                            &self.stats,
                            area,
                            &search,
                            self.settings.show_uncovered,
                        );
                        let page =
                            opens.iter().rposition(|o| *o <= search.from).unwrap_or(0) as i64;
                        let last = opens.len().saturating_sub(1) as i64;
                        let next = (page + by).clamp(0, last) as usize;
                        opens.get(next).copied().unwrap_or(0)
                    }
                };
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
                    self.state.sort_way,
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
    /// Carry out one of the config page's acts on the record, over the whole
    /// screen.
    Acting(view::Act),
    /// Write a backup of the record, over the whole screen.
    BackingUp,
}

impl Drop for App {
    /// Closes a keyboard left standing.
    fn drop(&mut self) {
        if self.keyboard {
            crate::keyboard::close();
        }
    }
}

/// What the record takes on disk, read off the file. Zero where there is none
/// to read, which is what a record never saved weighs.
fn record_bytes(dir: &std::path::Path) -> u64 {
    std::fs::metadata(crate::record::Record::file(dir))
        .map(|m| m.len())
        .unwrap_or(0)
}
