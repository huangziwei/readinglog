//! Turning a stream of log lines into sittings. A session is a run of page
//! events on one book, ended by a close, a change of book, a gap over
//! [`SESSION_GAP_SECS`], or midnight. Its duration is the counter's span.

use super::line;
use super::line::{
    Moment, Observation, end_position, observation, opened_at_counter, payloads, stamp,
    toc_and_book,
};
use super::metric::{Metric, cde_key, metric};
use super::power::{Awake, is_state_change};

/// The gap between two reader events that cuts a session.
pub const SESSION_GAP_SECS: i64 = 30 * 60;

/// How far a session's opening counter may outrun the wall clock.
const SEED_SLACK_SECS: i64 = 60;

/// The longest a page may stand awake and count.
const IDLE_SECS: i64 = 600;

/// The least a page must stand to count.
const FLOOR_SECS: i64 = 3;

/// The most a page counts whose words vouch for none of its standing time.
const CAP_SECS: i64 = 120;

/// The rate at which a page's words stop vouching for the time it stood.
const WPM_MIN: i64 = 40;

/// How much of `standing_ms` counts as reading, in milliseconds: nothing under
/// [`FLOOR_SECS`], the whole of it where `words` reach [`WPM_MIN`], and
/// [`CAP_SECS`] at most where they do not.
fn credited_ms(standing_ms: i64, words: i64) -> i64 {
    let secs = standing_ms / 1000;
    if secs < FLOOR_SECS {
        return 0;
    }
    // `words` a minute against `WPM_MIN`, both sides multiplied out.
    match words * 60_000 >= WPM_MIN * standing_ms {
        true => standing_ms,
        false => secs.min(CAP_SECS) * 1000,
    }
}

/// How a session's seconds were arrived at, ranked best first.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Measure {
    /// The span of the device's own `TotalTime` counter.
    #[default]
    Counted,
    /// What each turn line's `IntervalTime` credits, including the lines
    /// `SkipAvgReason` keeps out of `TotalTime`.
    Timed,
    /// How long each `ereader_book_consume_content` page was open.
    Paged,
    /// The `ACTIVE` stretches [`Awake::between`] sums with the book open.
    Awake,
}

impl Measure {
    /// The word a stored row carries.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Counted => "counted",
            Self::Timed => "timed",
            Self::Paged => "paged",
            Self::Awake => "awake",
        }
    }

    /// Read a stored row's word. A row a build before v0.3.3 wrote says
    /// `dwell` for the same measure.
    pub fn from_stored(s: &str) -> Self {
        match s {
            "timed" => Self::Timed,
            "paged" | "dwell" => Self::Paged,
            "awake" => Self::Awake,
            _ => Self::Counted,
        }
    }
}

/// One parsed sitting.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Session {
    /// `YYYY-MM-DDTHH:MM:SS`, device-local.
    pub started_at: String,
    pub ended_at: String,
    /// The book's own end position, this sitting's fingerprint for its book.
    pub end_position: i64,
    pub seconds: i64,
    /// Screens advanced, at whatever font size the device was set to.
    pub page_turns: i64,
    pub words: i64,
    /// Seconds read per clock hour, ascending, summing to [`Self::seconds`].
    pub hours: Vec<(u8, i64)>,
    /// Where [`Self::seconds`] came from.
    pub measure: Measure,
    /// The catalog key a reader-shell record named during the run.
    pub asin: Option<String>,
    /// How far into the book the sitting ended, as a fraction, off `%Left`.
    pub progress: Option<f64>,
    /// The seconds the device stated were left in the book as it closed, off
    /// `NewTimeLeft`.
    pub time_left: Option<i64>,
    /// The rate the device stated for this book as the sitting closed, off
    /// `TotalWPM`, in whole words a minute.
    pub stated_wpm: Option<i64>,
    /// Seconds this run's turn lines credit, each page's own `IntervalTime`
    /// through [`credited_ms`]. Zero where no turn line states an interval.
    pub timed_seconds: i64,
    /// The words on the pages that credited time, off `IntervalWords`.
    pub timed_words: i64,
    /// Seconds this run spanned, less the sleeps [`Awake`] states inside it.
    pub awake_seconds: i64,
    /// Seconds this run's pages credit, whatever [`Measure`] won. Zero where
    /// no `ereader_book_consume_content` record brackets a page.
    pub paged_seconds: i64,
    /// The words on those pages. `TotalWords` counts only the intervals the
    /// firmware's rate band accepted, so on a faster reader `Session::words`
    /// states a fraction of these.
    pub paged_words: i64,
    /// The book's own reading counter where this run began and where it was
    /// last seen. Both or neither: a run the device never counted has none.
    pub start_counter_ms: Option<i64>,
    pub end_counter_ms: Option<i64>,
    /// The same two readings of the device's own word counter. `end_words`
    /// beside `end_counter_ms` is the pair a sidecar's `timer.model` states.
    pub start_words: Option<i64>,
    pub end_words: Option<i64>,
    /// Seconds the device's clock stood ahead of UTC when this sitting was
    /// read out of the log, set by `Store::absorb` and unstated on a sitting
    /// no pass caught up with. No log line carries a zone.
    pub tz_offset_s: Option<i64>,
}

impl Session {
    /// Take `fresh`'s figures over this sitting's own, keeping every field
    /// `fresh` states nothing for. Answers whether anything moved.
    pub fn remeasure(&mut self, fresh: &Session) -> bool {
        let was = self.clone();
        self.seconds = fresh.seconds;
        self.page_turns = fresh.page_turns;
        self.words = fresh.words;
        self.hours = fresh.hours.clone();
        self.measure = fresh.measure;
        self.timed_seconds = fresh.timed_seconds;
        self.timed_words = fresh.timed_words;
        self.awake_seconds = fresh.awake_seconds;
        self.paged_seconds = fresh.paged_seconds;
        self.paged_words = fresh.paged_words;
        self.start_counter_ms = fresh.start_counter_ms.or(self.start_counter_ms);
        self.end_counter_ms = fresh.end_counter_ms.or(self.end_counter_ms);
        self.start_words = fresh.start_words.or(self.start_words);
        self.end_words = fresh.end_words.or(self.end_words);
        self.asin = fresh.asin.clone().or_else(|| self.asin.clone());
        self.progress = fresh.progress.or(self.progress);
        self.time_left = fresh.time_left.or(self.time_left);
        self.stated_wpm = fresh.stated_wpm.or(self.stated_wpm);
        self.tz_offset_s = fresh.tz_offset_s.or(self.tz_offset_s);
        was != *self
    }
}

/// The share of a counter's advance that falls before a boundary inside the
/// interval it was measured over, in proportion to the wall clock either side.
fn share(advance: i64, elapsed: i64, before: i64) -> i64 {
    if elapsed <= 0 || advance <= 0 {
        return 0;
    }
    advance * before.clamp(0, elapsed) / elapsed
}

/// How a run in progress ends when a new observation cannot join it.
enum Break {
    /// A gap, a close, or another book. The run ends where it was last seen.
    Left,
    /// Midnight inside an interval. The run is cut *at* the boundary, with
    /// `counter_ms` and `words` interpolated there.
    Midnight(Start),
}

/// An `OpenBook` unattached to a session.
struct Opened {
    counter_ms: i64,
    at: Moment,
}

/// Where a session begins: the counters it resumes from and the instant it
/// started at — an `OpenBook` that vouched for them, or a midnight cut.
/// `counter_ms` is absent on a book the device does not time; `at` is not.
struct Start {
    counter_ms: Option<i64>,
    /// The word counter at that same instant, where it is known.
    words: Option<i64>,
    at: Moment,
}

/// The highest `(EndPos, TotalTime, TotalWords)` each book was logged at.
struct Counters(Vec<(i64, i64, i64)>);

impl Counters {
    /// The rows [`parse_sessions`] was handed, as its own copy.
    fn held(rows: &[(i64, i64, i64)]) -> Self {
        Self(rows.to_vec())
    }

    /// The `TotalWords` standing beside `counter_ms` for the book at
    /// `position`. `StoredBookData` states whole seconds; the match is on the
    /// second.
    fn words_at(&self, position: i64, counter_ms: i64) -> Option<i64> {
        self.0
            .iter()
            .find(|(ep, ms, _)| *ep == position && *ms / 1000 == counter_ms / 1000)
            .map(|(_, _, words)| *words)
    }

    /// Hold what an observation states, where its counter outruns the one held.
    fn learn(&mut self, obs: &Observation) {
        let (Some(total_ms), Some(words)) = (obs.total_ms, obs.words) else {
            return;
        };
        match self.0.iter_mut().find(|(ep, _, _)| *ep == obs.position) {
            Some(held) if held.1 < total_ms => *held = (obs.position, total_ms, words),
            Some(_) => {}
            None => self.0.push((obs.position, total_ms, words)),
        }
    }
}

impl Opened {
    /// This open as a session start, or `None` when it cannot be vouched for:
    /// `counter_ms` must sit at or under the first observation's, and the
    /// reading it adds inside the wall clock since the open.
    fn vouch(self, now: &Moment, obs: &Observation, counters: &Counters) -> Option<Start> {
        let total = obs.total_ms?;
        let elapsed = now.abs.checked_sub(self.at.abs).filter(|e| *e >= 0)?;
        (self.at.day == now.day
            && self.counter_ms <= total
            && total - self.counter_ms <= (elapsed + SEED_SLACK_SECS) * 1000)
            .then(|| Start {
                counter_ms: Some(self.counter_ms),
                words: counters.words_at(obs.position, self.counter_ms),
                at: self.at,
            })
    }

    /// This open as the instant a run began, for a run with no counter to
    /// vouch it against. `at` stands at most [`SESSION_GAP_SECS`] before `now`,
    /// and [`Awake`] bounds the figure the run produces.
    fn opened_run(&self, now: &Moment) -> Option<Start> {
        let elapsed = now.abs - self.at.abs;
        (self.at.day == now.day && (0..=SESSION_GAP_SECS).contains(&elapsed)).then(|| Start {
            counter_ms: None,
            words: None,
            at: self.at.clone(),
        })
    }
}

/// A session under construction.
struct Open {
    end_position: i64,
    started_at: String,
    ended_at: String,
    time_lo: Option<i64>,
    time_hi: i64,
    words_lo: Option<i64>,
    words_hi: i64,
    page_turns: i64,
    /// The last observation's instant and counters.
    last: Moment,
    last_time: Option<i64>,
    last_words: Option<i64>,
    /// Milliseconds of counted reading booked against each hour of the day.
    hours_ms: [i64; 24],
    /// The first instant this run was seen at.
    began: Moment,
    /// Forward turns from the `fastmetrics` records, apart from `page_turns`.
    metric_turns: i64,
    /// Words off the pages that counted. A fixed-layout page has none.
    metric_words: i64,
    /// Whether an `OpenBook` vouched this run, making the first observation's
    /// `IntervalTime` a measure of this run and not of what came before.
    vouched: bool,
    /// How many observations this run has folded in.
    seen: i64,
    /// Milliseconds the turn lines credit, and the words beside them.
    interval_total_ms: i64,
    interval_words: i64,
    interval_hours_ms: [i64; 24],
    /// Milliseconds the pages were open, beside the open page.
    paged_total_ms: i64,
    paged_hours_ms: [i64; 24],
    open_page: Option<(Moment, i64, i64)>,
    /// The catalog key a reader-shell record named for this run.
    asin: Option<String>,
    /// The last `%Left` a line stated for this book.
    progress: Option<f64>,
    /// The last `NewTimeLeft` a line stated for this book.
    time_left: Option<i64>,
    /// The last `TotalWPM` a line stated for this book.
    stated_wpm: Option<i64>,
}

impl Open {
    /// `start` is where the book was opened — an `OpenBook` that vouched for
    /// it, or the midnight that cut the run before it. Without one, `now` is
    /// the floor.
    fn new(end_position: i64, now: &Moment, start: Option<Start>) -> Self {
        let vouched = start.is_some();
        let (time_lo, words_lo, from) = match start {
            Some(s) => (s.counter_ms, s.words, s.at),
            None => (None, None, now.clone()),
        };
        Self {
            end_position,
            started_at: from.at.clone(),
            ended_at: now.at.clone(),
            time_lo,
            time_hi: time_lo.unwrap_or(0),
            words_lo,
            words_hi: words_lo.unwrap_or(0),
            page_turns: 0,
            // `last_time` opens at the vouched counter, making the stretch to
            // the first observation an interval like any other.
            last_time: time_lo,
            last_words: words_lo,
            hours_ms: [0; 24],
            began: from.clone(),
            last: from,
            metric_turns: 0,
            metric_words: 0,
            vouched,
            seen: 0,
            interval_total_ms: 0,
            interval_words: 0,
            interval_hours_ms: [0; 24],
            paged_total_ms: 0,
            paged_hours_ms: [0; 24],
            open_page: None,
            asin: None,
            progress: None,
            time_left: None,
            stated_wpm: None,
        }
    }

    /// Book an interval's counted reading against the clock hours it ran
    /// through, `from` and `to` being seconds into the day. Evenly across the
    /// interval, which is a page turn wide.
    fn credit(hours_ms: &mut [i64; 24], from: i64, to: i64, advance_ms: i64) {
        if advance_ms <= 0 {
            return;
        }
        let hour = |secs: i64| ((secs / 3600) as usize).min(23);
        let span = to - from;
        if span <= 0 {
            hours_ms[hour(from)] += advance_ms;
            return;
        }
        let mut placed = 0;
        for h in (from / 3600)..=((to - 1) / 3600) {
            let overlap = to.min((h + 1) * 3600) - from.max(h * 3600);
            if overlap <= 0 {
                continue;
            }
            let share = advance_ms * overlap / span;
            placed += share;
            hours_ms[hour(h * 3600)] += share;
        }
        // The division's remainder to the hour the interval began in.
        hours_ms[hour(from)] += advance_ms - placed;
    }

    fn observe(&mut self, now: &Moment, obs: &Observation) {
        if let (Some(from), Some(to)) = (self.last_time, obs.total_ms) {
            Self::credit(&mut self.hours_ms, self.last.secs, now.secs, to - from);
        }
        self.ended_at = now.at.clone();
        self.last = now.clone();
        if obs.page_turn {
            self.page_turns += 1;
        }
        // `IntervalTime` stands on its own line, apart from `total_ms`, which a
        // `SkipAvgReason` line repeats unchanged. The first observation of a
        // run no `OpenBook` vouched measures the page before the run.
        self.seen += 1;
        let first_unvouched = self.seen == 1 && !self.vouched;
        let interval_words = obs.interval_words.unwrap_or(0).max(0);
        if let Some(counts) = obs
            .interval_ms
            .filter(|_| !first_unvouched)
            .map(|ms| credited_ms(ms, interval_words))
            .filter(|ms| *ms > 0)
        {
            self.interval_total_ms += counts;
            self.interval_words += interval_words;
            Self::credit(
                &mut self.interval_hours_ms,
                self.last.secs,
                now.secs,
                counts,
            );
        }
        if let Some(t) = obs.total_ms {
            self.time_lo = Some(self.time_lo.map_or(t, |lo| lo.min(t)));
            self.time_hi = self.time_hi.max(t);
            self.last_time = Some(t);
        }
        if let Some(w) = obs.words {
            self.words_lo = Some(self.words_lo.map_or(w, |lo| lo.min(w)));
            self.words_hi = self.words_hi.max(w);
            self.last_words = Some(w);
        }
        // `Open` holds one run's counters; a reopen starts another.
    }

    /// Fold one `fastmetrics` record into the run. `Metric::Page` closes the
    /// interval the page before it opened and [`page_ms`] says how much
    /// counts; neither `last` nor `ended_at` moves.
    fn observe_metric(&mut self, now: &Moment, m: &Metric, awake: &Awake) {
        match m {
            Metric::Forward => self.metric_turns += 1,
            Metric::Back => {}
            Metric::Close => self.open_page = None,
            Metric::Page { words, start } => {
                if let Some((from, from_words, from_start)) = self.open_page.take() {
                    // `Awake::bound`: the interval less the sleeps it states.
                    let awake_secs = awake.bound(from.abs, now.abs);
                    let counts = match awake_secs > IDLE_SECS {
                        true => 0,
                        false => credited_ms(awake_secs * 1000, from_words),
                    };
                    self.paged_total_ms += counts;
                    // A page under the floor counts no time, and its words are
                    // not read either. A redraw repeats `start` and its words
                    // stand on the screen the record before it already counted.
                    let redraw = *start >= 0 && *start == from_start;
                    if counts > 0 && !redraw {
                        self.metric_words += from_words;
                    }
                    credit_awake(&mut self.paged_hours_ms, awake, &from, now, counts);
                }
                self.open_page = Some((now.clone(), *words, *start));
            }
        }
    }

    /// Whether this observation ends the run, and how.
    fn broken_by(&self, now: &Moment, obs: &Observation, gapped: bool) -> Option<Break> {
        if self.end_position != obs.position || gapped {
            return Some(Break::Left);
        }
        if self.last.day == now.day {
            return None;
        }
        // Over midnight, `counter_ms` and `words` are interpolated at the
        // boundary. `Break::Left` where either side states none.
        let (Some(from), Some(to)) = (self.last_time, obs.total_ms) else {
            return Some(Break::Left);
        };
        let elapsed = now.abs - self.last.abs;
        let before = now.abs - now.secs - self.last.abs;
        Some(Break::Midnight(Start {
            counter_ms: Some(from + share(to - from, elapsed, before)),
            words: self
                .last_words
                .zip(obs.words)
                .map(|(from, to)| from + share(to - from, elapsed, before)),
            at: Moment {
                day: now.day.clone(),
                secs: 0,
                abs: now.abs - now.secs,
                at: format!("{}T00:00:00", now.day),
            },
        }))
    }

    /// Close the run at the midnight it was cut at, crediting this day the
    /// share of the unfinished interval before the boundary. The stored end is
    /// one second short of it: `T00:00:00` belongs to the next day.
    fn finish_at(mut self, boundary: &Start, awake: &Awake) -> Session {
        // `boundary.counter_ms` is absent where either side stated none.
        let at_boundary = boundary.counter_ms.unwrap_or(self.time_hi);
        if let Some(from) = self.last_time {
            // `86_400` is midnight in this day's seconds, second zero in the
            // next.
            Self::credit(
                &mut self.hours_ms,
                self.last.secs,
                86_400,
                at_boundary - from,
            );
        }
        self.time_hi = self.time_hi.max(at_boundary);
        if let Some(w) = boundary.words {
            self.words_hi = self.words_hi.max(w);
        }
        self.ended_at = format!("{}T23:59:59", self.last.day);
        self.finish(awake)
    }

    /// The run as a session. [`Measure`] names the first of the four its
    /// records support, and none of the four keeps the zero.
    fn finish(self, awake: &Awake) -> Session {
        let counted = (self.time_hi - self.time_lo.unwrap_or(self.time_hi)) / 1000;
        let timed = self.interval_total_ms / 1000;
        let paged = self.paged_total_ms / 1000;
        let witnessed = awake.between(self.began.abs, self.last.abs);
        let (seconds, measure) = match (counted, timed, paged) {
            (c, _, _) if c > 0 => (c, Measure::Counted),
            (_, t, _) if t > 0 => (t, Measure::Timed),
            (_, _, d) if d > 0 => (d, Measure::Paged),
            _ if awake.is_empty() => (0, Measure::Counted),
            _ => (witnessed, Measure::Awake),
        };
        Session {
            hours: match measure {
                Measure::Counted => hours_in_seconds(&self.hours_ms, seconds),
                Measure::Timed => hours_in_seconds(&self.interval_hours_ms, seconds),
                Measure::Paged => hours_in_seconds(&self.paged_hours_ms, seconds),
                Measure::Awake => spread(awake, &self.began, &self.last, seconds),
            },
            started_at: self.started_at,
            ended_at: self.ended_at,
            end_position: self.end_position,
            seconds,
            // `page_turns` where that stack names any, `metric_turns` where it
            // names none.
            page_turns: match self.page_turns {
                0 => self.metric_turns,
                named => named,
            },
            // The timer's own span where it stated one, else the pages'
            // own counts, as `page_turns` above takes the metric records'.
            words: match self.words_hi - self.words_lo.unwrap_or(self.words_hi) {
                0 => self.metric_words,
                span => span,
            },
            measure,
            asin: self.asin,
            progress: self.progress,
            time_left: self.time_left,
            stated_wpm: self.stated_wpm,
            tz_offset_s: None,
            timed_seconds: timed,
            timed_words: self.interval_words,
            awake_seconds: awake.bound(self.began.abs, self.last.abs),
            paged_seconds: paged,
            paged_words: self.metric_words,
            start_counter_ms: self.time_lo,
            end_counter_ms: self.time_lo.map(|_| self.time_hi),
            start_words: self.words_lo,
            end_words: self.words_lo.map(|_| self.words_hi),
        }
    }
}

/// Book `advance_ms` against the clock hours the device was awake in. `from`
/// and `to` bracket an interval a page was open across; the advance splits
/// between [`Awake`]'s stretches in it, or takes the whole where it names none.
fn credit_awake(
    hours_ms: &mut [i64; 24],
    awake: &Awake,
    from: &Moment,
    to: &Moment,
    advance_ms: i64,
) {
    if advance_ms <= 0 {
        return;
    }
    let spans = awake.spans_between(from.abs, to.abs);
    let total: i64 = spans.iter().map(|(a, b)| b - a).sum();
    if total <= 0 {
        Open::credit(hours_ms, from.secs, to.secs, advance_ms);
        return;
    }
    // `Moment::secs` is `abs` less the midnight it stands after.
    let midnight = from.abs - from.secs;
    let clip = |at: i64| (at - midnight).clamp(0, 86_400);
    let mut placed = 0;
    for (start, end) in &spans {
        let share = advance_ms * (end - start) / total;
        Open::credit(hours_ms, clip(*start), clip(*end), share);
        placed += share;
    }
    if let Some((start, _)) = spans.first() {
        Open::credit(hours_ms, clip(*start), clip(*start), advance_ms - placed);
    }
}

/// A bounded total across the hours the device was awake for it.
fn spread(awake: &Awake, from: &Moment, to: &Moment, seconds: i64) -> Vec<(u8, i64)> {
    let mut hours_ms = [0; 24];
    credit_awake(&mut hours_ms, awake, from, to, seconds * 1000);
    hours_in_seconds(&hours_ms, seconds)
}

/// The hours a session's milliseconds fall in, as whole seconds summing to
/// exactly `seconds`. The running total truncates, and a last correction covers
/// what the milliseconds cannot account for.
fn hours_in_seconds(hours_ms: &[i64; 24], seconds: i64) -> Vec<(u8, i64)> {
    let mut out = Vec::new();
    let (mut running, mut placed) = (0, 0);
    for (hour, ms) in hours_ms.iter().enumerate() {
        running += ms;
        let secs = running / 1000 - placed;
        placed += secs;
        if secs > 0 {
            out.push((hour as u8, secs));
        }
    }
    if placed != seconds
        && let Some(busiest) = out
            .iter_mut()
            .max_by_key(|(_, s)| *s)
            .filter(|(_, s)| *s + seconds - placed > 0)
    {
        busiest.1 += seconds - placed;
    }
    out
}

/// Turn an ordered, de-duplicated event stream into sessions. `counters` seeds
/// [`Counters`], where `Open::words_lo` reads the `TotalWords` an `OpenBook`
/// leaves unstated.
pub fn parse_sessions<'a>(
    events: impl IntoIterator<Item = &'a str>,
    counters: &[(i64, i64, i64)],
) -> Vec<Session> {
    // [`Awake`] reads the whole stream before the first sitting closes.
    let lines: Vec<&str> = events.into_iter().collect();
    // Which lines are reading-timer lines, read once: the readers below each
    // want one side or the other, and asking separately is a whole-line search
    // apiece over the largest stream a launch reads.
    let timer: Vec<bool> = lines
        .iter()
        .map(|line| crate::log::is_timer(line))
        .collect();
    // `toc` holds the positions only ever stated as a chapter's start.
    let mut toc: Vec<i64> = Vec::new();
    let mut book: Vec<i64> = Vec::new();
    for line in lines.iter().copied().zip(&timer).filter(|(_, t)| **t) {
        let (t, b) = toc_and_book(line.0);
        toc.extend(t);
        book.extend(b);
    }
    toc.sort_unstable();
    toc.dedup();
    book.sort_unstable();
    book.dedup();
    let chapters: Vec<i64> = toc
        .into_iter()
        .filter(|p| book.binary_search(p).is_err())
        .collect();
    // [`Awake::witnessed`] reads its instants here.
    let read_at: Vec<i64> = lines
        .iter()
        .copied()
        .zip(&timer)
        .filter(|(line, timer)| match timer {
            true => observation(line).is_some(),
            false => metric(line).is_some(),
        })
        .filter_map(|(line, _)| Some(stamp(line)?.abs))
        .collect();
    // Only `powerd` states a change, so the timer lines are not offered one.
    let woke = lines
        .iter()
        .copied()
        .zip(&timer)
        .filter_map(|(line, timer)| (!timer).then_some(line));
    let awake = Awake::from_events(woke).witnessed(&read_at);

    let mut out = Vec::new();
    let mut open: Option<Open> = None;
    let mut prev_abs: Option<i64> = None;
    let mut opened: Option<Opened> = None;
    let mut counters = Counters::held(counters);
    // `gapped` holds a break until an observation acts on it.
    let mut gapped = false;
    // The catalog key most recently named, and when, for the run it belongs to.
    let mut named: Option<(i64, String)> = None;
    // Records no open run reached, drained into the run that opens over them.
    let mut pending: Vec<(Moment, Metric)> = Vec::new();

    for (line, &timer) in lines.iter().copied().zip(&timer) {
        let Some(now) = stamp(line) else {
            continue;
        };
        // `gapped` counts the stretch between two lines `is_state_change`
        // rejects. A reading-timer line is never one of them.
        if timer || !is_state_change(line) {
            gapped |= prev_abs.is_some_and(|prev| now.abs - prev > SESSION_GAP_SECS);
            prev_abs = Some(now.abs);
        }

        if timer && let Some(counter_ms) = opened_at_counter(line) {
            opened = Some(Opened {
                counter_ms,
                at: now.clone(),
            });
        }

        // `live` is a run reaching [`SESSION_GAP_SECS`] past its own newest
        // observation. A record beyond it waits for the next run.
        let live = open
            .as_ref()
            .is_some_and(|cur| now.abs - cur.last.abs <= SESSION_GAP_SECS);

        if !timer && let Some(key) = cde_key(line) {
            named = Some((now.abs, key.to_string()));
            if live && let Some(cur) = open.as_mut() {
                cur.asin = Some(key.to_string());
            }
        }

        let Some(obs) = timer
            .then(|| observation(line))
            .flatten()
            .filter(|o| chapters.binary_search(&o.position).is_err())
        else {
            // `pending` holds a record no open run reaches.
            if !timer && let Some(m) = metric(line) {
                match open.as_mut().filter(|_| live) {
                    Some(cur) => cur.observe_metric(&now, &m, &awake),
                    None => pending.push((now.clone(), m)),
                }
            }
            continue;
        };
        // `gapped` is consumed here, at the second of two observations.
        let gapped = std::mem::take(&mut gapped);

        // `opened` is read at the first observation after it, whether or not
        // that observation carries the counter to vouch for it.
        let mut seed = match obs.total_ms {
            Some(_) => opened.take().and_then(|o| o.vouch(&now, &obs, &counters)),
            None => {
                let start = opened.as_ref().and_then(|o| o.opened_run(&now));
                // `opened` is spent on a run it starts, and held where `start`
                // is `None`.
                opened = opened.filter(|_| start.is_none());
                start
            }
        };

        match open
            .as_ref()
            .and_then(|cur| cur.broken_by(&now, &obs, gapped))
        {
            None => {}
            Some(Break::Left) => {
                out.push(open.take().expect("a run to break").finish(&awake));
            }
            Some(Break::Midnight(boundary)) => {
                out.push(
                    open.take()
                        .expect("a run to cut")
                        .finish_at(&boundary, &awake),
                );
                // `boundary` is the start `seed` carries into the next run.
                seed = Some(boundary);
            }
        }
        let fresh = open.is_none();
        let cur = open.get_or_insert_with(|| Open::new(obs.position, &now, seed.take()));
        if fresh {
            // `named` and `pending` in order, from `cur.began`.
            let from = cur.began.abs;
            cur.asin = named
                .as_ref()
                .filter(|(at, _)| *at >= from)
                .map(|(_, key)| key.clone());
            for (at, m) in std::mem::take(&mut pending) {
                if at.abs >= from {
                    cur.observe_metric(&at, &m, &awake);
                }
            }
        }
        // `cur.time_lo` and `cur.words_lo` take the counters an open vouched
        // for, where the run opened without them.
        if let Some(start) = seed.take() {
            if let Some(counter) = start.counter_ms
                && cur.time_lo.is_none()
            {
                cur.time_lo = Some(counter);
                cur.time_hi = counter;
                cur.last_time = Some(counter);
            }
            if let Some(words) = start.words
                && cur.words_lo.is_none()
            {
                cur.words_lo = Some(words);
                cur.words_hi = words;
                cur.last_words = Some(words);
            }
        }
        if let Some(left) = percent_left(line, cur.end_position) {
            cur.progress = Some(1.0 - left);
        }
        if let Some(secs) = time_left(line, cur.end_position) {
            cur.time_left = Some(secs);
        }
        if let Some(rate) = stated_wpm(line, cur.end_position) {
            cur.stated_wpm = Some(rate);
        }
        cur.observe(&now, &obs);
        counters.learn(&obs);
        if obs.closes {
            out.push(open.take().expect("a run to close").finish(&awake));
        }
    }
    out.extend(open.map(|cur| cur.finish(&awake)));
    out.retain(|s| s.seconds > 0);
    out
}

/// A book's percentage read, off the reading-timer lines. `%Left` is the
/// fraction ahead, on the payload naming the book; read only off a line whose
/// book `end_position` names.
fn percent_left(line: &str, book: i64) -> Option<f64> {
    payloads(line).find_map(|p| {
        if end_position(p)? != book {
            return None;
        }
        let rest = &p[p.find("%Left:")? + "%Left:".len()..];
        let end = rest
            .find(|c: char| !c.is_ascii_digit() && c != '.' && c != '-')
            .unwrap_or(rest.len());
        let left: f64 = rest[..end].parse().ok()?;
        (0.0..=1.0).contains(&left).then_some(left)
    })
}

/// The seconds the device states are left in `book`, off the payload naming it.
/// [`line::time_left`] reads the book's own figure past the chapter's.
fn time_left(line: &str, book: i64) -> Option<i64> {
    payloads(line).find_map(|p| match end_position(p) {
        Some(at) if at == book => line::time_left(p),
        _ => None,
    })
}

/// `TotalWPM` for `book`, off the payload naming it. It carries a fraction, and
/// [`line::field`] reads the whole words a minute ahead of the point.
fn stated_wpm(line: &str, book: i64) -> Option<i64> {
    payloads(line).find_map(|p| match end_position(p) {
        Some(at) if at == book => line::field(p, "TotalWPM").filter(|rate| *rate > 0),
        _ => None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// `cvm` lines, each naming its event first.
    const CVM: [&str; 2] = [
        "260807:101501 cvm[6144]: I ReadingTimerController:Information::NextPage,Verdict:Processed,PageStartPos:YJPosition: AfQJAAAAAAAA:54205,IntervalTime:39890,IntervalWords:320,TotalTime:7390020,TotalWords:49583,CurrentPos:YJPosition: AfQJAAAAAAAA:54205,EndPos:YJPosition: AbcVAAAPAAAA:148207,PosLeft:94002,%Left:0.6450299914310198,NextTOCEntryPosition:YJPosition: AT4KAAAAAAAA:56499,NextTOCEntryLength:10,CurrentPos:YJPosition: AfQJAAAAAAAA:54205,EndPos:YJPosition: AT4KAAAAAAAA:56499,PosLeft:2294,%Left:0.01585261353898887;",
        "260807:101543 cvm[6144]: I ReadingTimerController:Information::NextPage,Verdict:Processed,PageStartPos:YJPosition: Af8JAAAAAAAA:54507,IntervalTime:41443,IntervalWords:294,TotalTime:7431463,TotalWords:49877,CurrentPos:YJPosition: Af8JAAAAAAAA:54507,EndPos:YJPosition: AbcVAAAPAAAA:148207,PosLeft:93700,%Left:0.6426735218508998,NextTOCEntryPosition:YJPosition: AT4KAAAAAAAA:56499,NextTOCEntryLength:10,CurrentPos:YJPosition: Af8JAAAAAAAA:54507,EndPos:YJPosition: AT4KAAAAAAAA:56499,PosLeft:1992,%Left:0.01349614395886889;",
    ];

    #[test]
    fn two_page_events_measure_the_span_of_the_counter_between_them() {
        let out = parse_sessions(CVM, &[]);
        assert_eq!(out.len(), 1);
        // 7431463 - 7390020 = 41443 ms.
        assert_eq!(out[0].seconds, 41);
        assert_eq!(out[0].end_position, 148_207);
        assert_eq!(out[0].page_turns, 2);
        assert_eq!(out[0].words, 294);
        assert_eq!(out[0].measure, Measure::Counted);
        assert_eq!(out[0].started_at, "2026-08-07T10:15:01");
        assert_eq!(out[0].ended_at, "2026-08-07T10:15:43");
    }

    /// An `OpenBook` and two `NextPage` lines stating `HTMLPosition` places.
    const MOBI8: [&str; 3] = [
        "260906:192401 java[1]: I ReadingTimerController:Information::OpenBook,StoredBookData:TimeRead:329 sec. WPM:0. Version:0,Title:<private>;",
        "260906:192404 cvm[6144]: I ReadingTimerController:Information::NextPage,Verdict:Processed,PageStartPos:HTMLPosition:7731097,IntervalTime:785,IntervalWords:12,TotalTime:329785,TotalWords:1905,CurrentPos:HTMLPosition:7731097,EndPos:HTMLPosition:19886489,PosLeft:12155392,%Left:0.6112;",
        "260906:192425 cvm[6144]: I ReadingTimerController:Information::NextPage,Verdict:Processed,PageStartPos:HTMLPosition:7731725,IntervalTime:21217,IntervalWords:172,TotalTime:351002,TotalWords:2077,CurrentPos:HTMLPosition:7731725,EndPos:HTMLPosition:19886489,PosLeft:12154764,%Left:0.6111;",
    ];

    #[test]
    fn a_mobi8_run_is_measured_the_way_a_kfx_one_is() {
        let out = parse_sessions(MOBI8, &[]);
        assert_eq!(out.len(), 1);
        // `vouch` seeds 329000 ms; 351002 stands at the last turn.
        assert_eq!(out[0].seconds, 22);
        assert_eq!(out[0].end_position, 19_886_489);
        assert_eq!(out[0].page_turns, 2);
        assert_eq!(out[0].words, 172);
        assert_eq!(out[0].measure, Measure::Counted);
        assert_eq!(out[0].started_at, "2026-09-06T19:24:01");
        assert_eq!(out[0].ended_at, "2026-09-06T19:24:25");
        assert_eq!(out[0].progress, Some(1.0 - 0.6111));
    }

    /// A turn at `hhmmss` with the counter at `total_ms`, stating `book`
    /// seconds left in the book and `chapter` seconds left in the chapter.
    fn turn_left(hhmmss: &str, total_ms: i64, book: i64, chapter: i64) -> String {
        format!(
            "260906:{hhmmss} cvm[1]: I ReadingTimerController:Information::NextPage,\
             Verdict:Processed,TotalTime:{total_ms},TotalWords:1905,\
             CurrentPos:HTMLPosition:7731097,EndPos:HTMLPosition:19886489,PosLeft:12155392,\
             %Left:0.6112,NewTimeLeft:{book},OldTimeLeft:9999,\
             NextTOCEntryPosition:HTMLPosition:7800000,NextTOCEntryLength:10,\
             CurrentPos:HTMLPosition:7731097,EndPos:HTMLPosition:7800000,PosLeft:68903,\
             NewTimeLeft:{chapter};"
        )
    }

    #[test]
    fn a_run_holds_the_time_left_the_last_line_stated_for_its_book() {
        let lines = [
            turn_left("192404", 329_785, 4_200, 300),
            turn_left("192425", 351_002, 3_900, 240),
        ];
        let out = parse_sessions(lines.iter().map(String::as_str), &[]);
        assert_eq!(out.len(), 1);
        assert_eq!(out[0].end_position, 19_886_489);
        assert_eq!(out[0].time_left, Some(3_900), "the chapter's figure stood");
    }

    /// A `CloseBook` stating `TotalTime` and `TotalWords` together, the one
    /// counted line a KPP stack writes for a whole sitting.
    fn kpp_close(hhmmss: &str, total_ms: i64, words: i64) -> String {
        format!(
            "260906:{hhmmss} java[5878]: I ReadingTimerController:Information::\
             CloseBook,Title:<private>,Asin:<private>,\
             PageStartPos:YJPosition: AeUAAAAwAAAA:10671,IntervalTime:91293,IntervalWords:187,\
             TotalTime:{total_ms},TotalWords:{words},TotalWPM:217.248,\
             CurrentPos:YJPosition: AeUAAAAwAAAA:10671,\
             EndPos:YJPosition: AbIWAADQAAAA:289095,PosLeft:278424,%Left:0.9622;"
        )
    }

    /// An `OpenBook` restating the counter. `time_read` is whole seconds,
    /// thousands-separated, and no word count stands beside it.
    fn kpp_open(hhmmss: &str, time_read: &str) -> String {
        format!(
            "260906:{hhmmss} java[5878]: I ReadingTimerController:Information::\
             OpenBook,CurrentVersionUsed:0,StoredBookData:TimeRead:{time_read} sec. \
             WPM:217.248. Version:0,Title:<private>,Asin:<private>;"
        )
    }

    #[test]
    fn a_resumed_run_takes_the_words_that_stood_beside_the_counter_it_opened_at() {
        let lines = [
            kpp_close("024945", 1_592_301, 5_548),
            kpp_open("024957", "1,592"),
            kpp_close("140506", 10_194_302, 33_906),
        ];
        let out = parse_sessions(lines.iter().map(String::as_str), &[]);
        assert_eq!(out.len(), 1, "the first close spans nothing of its own");
        // 10194302 ms against the 1592000 the open vouched for.
        assert_eq!(out[0].seconds, 8_602);
        assert_eq!(out[0].words, 33_906 - 5_548);
        assert_eq!(out[0].start_words, Some(5_548));
        assert_eq!(out[0].end_words, Some(33_906));
    }

    #[test]
    fn a_pass_that_never_read_the_close_takes_its_words_off_the_record() {
        let lines = [
            kpp_open("024957", "1,592"),
            kpp_close("140506", 10_194_302, 33_906),
        ];
        let held = [(289_095, 1_592_301, 5_548)];
        let out = parse_sessions(lines.iter().map(String::as_str), &held);
        assert_eq!(out.len(), 1);
        assert_eq!(out[0].words, 33_906 - 5_548);
    }

    #[test]
    fn an_open_no_counter_answers_for_leaves_the_words_unstated() {
        let lines = [
            kpp_open("024957", "1,592"),
            kpp_close("140506", 10_194_302, 33_906),
        ];
        // `held` names another book's pairing, and this book's at a counter
        // `kpp_open` does not restate.
        let held = [
            (19_886_489, 1_592_301, 5_548),
            (289_095, 10_194_302, 33_906),
        ];
        let out = parse_sessions(lines.iter().map(String::as_str), &held);
        assert_eq!(out[0].seconds, 8_602, "the seconds still came off the open");
        assert_eq!(out[0].words, 0, "a word count was invented");
    }

    /// A `cde_key` record at `hhmmss`.
    fn keyed(hhmmss: &str, key: &str) -> String {
        format!(
            "260906:{hhmmss} fastmetrics[1]: D fastmetrics: \
             SchemaName[ereader_reader_page_turn_latency_ops], Fields[{{ \
             \"action\" : \"PageTurnTotalTime\", \"cde_key\" : \"{key}\" }} ]. :"
        )
    }

    /// A lone `CurrentPos`/`EndPos` group at `hhmmss` over `end`, the shape a
    /// chapter's progress and an untimed sitting share.
    fn lone(hhmmss: &str, end: i64) -> String {
        format!(
            "260906:{hhmmss} java[1]: I ReadingTimerController:Information::\
             CurrentPos:HTMLPosition: 7340,EndPos:HTMLPosition: {end},PosLeft:21488,%Left:0.013;"
        )
    }

    #[test]
    fn a_run_gone_quiet_does_not_take_the_next_books_key() {
        let mut lines = vec![page("101501", 7_390_020), page("101543", 7_431_463)];
        // A month on, another book's key, its open, and its own turns.
        lines.push(MOBI8[0].replace("260906:192401", "260906:121001"));
        lines.push(keyed("121002", "B00NEXTONE"));
        lines.push(MOBI8[1].replace("260906:192404", "260906:121004"));
        lines.push(MOBI8[2].replace("260906:192425", "260906:121025"));

        let out = parse_sessions(lines.iter().map(String::as_str), &[]);
        assert_eq!(out.len(), 2);
        assert_eq!(out[0].end_position, 148_207);
        assert_eq!(out[0].asin, None, "the first run took a later book's key");
        assert_eq!(out[1].end_position, 19_886_489);
        assert_eq!(out[1].asin.as_deref(), Some("B00NEXTONE"));
    }

    #[test]
    fn a_chapter_the_toc_names_is_no_book_of_its_own() {
        // 19886489 leads the group and 28828 is the `NextTOCEntry` position,
        // which states itself alone on the last line.
        let toc = format!(
            "{},NextTOCEntryPosition:HTMLPosition: 28828,NextTOCEntryLength:20405,\
             CurrentPos:HTMLPosition: 7340,EndPos:HTMLPosition: 28828,PosLeft:21488;",
            MOBI8[1].trim_end_matches(';')
        );
        let lines = [
            MOBI8[0].to_string(),
            toc,
            MOBI8[2].to_string(),
            lone("192500", 28_828),
        ];
        let out = parse_sessions(lines.iter().map(String::as_str), &[]);
        assert_eq!(out.len(), 1, "a chapter opened a sitting of its own");
        assert_eq!(out[0].end_position, 19_886_489);
    }

    #[test]
    fn the_hours_of_a_session_add_back_up_to_the_session() {
        let out = parse_sessions(CVM, &[]);
        let summed: i64 = out[0].hours.iter().map(|(_, s)| s).sum();
        assert_eq!(summed, out[0].seconds);
        assert_eq!(out[0].hours, vec![(10, 41)]);
    }

    /// A `NextPage` at `hhmmss` with the counter at `total_ms`.
    fn page(hhmmss: &str, total_ms: i64) -> String {
        format!(
            "260807:{hhmmss} cvm[6144]: I ReadingTimerController:Information::NextPage,\
             Verdict:Processed,PageStartPos:YJPosition: AfQJAAAAAAAA:54205,\
             IntervalTime:39890,IntervalWords:320,TotalTime:{total_ms},TotalWords:49583,\
             CurrentPos:YJPosition: AfQJAAAAAAAA:54205,\
             EndPos:YJPosition: AbcVAAAPAAAA:148207,PosLeft:94002,%Left:0.645;"
        )
    }

    /// A turn line stating no `IntervalTime`, leaving the run to the pages.
    fn untimed_page(hhmmss: &str, total_ms: i64) -> String {
        format!(
            "260807:{hhmmss} cvm[6144]: I ReadingTimerController:Information::NextPage,\
             Verdict:Processed,PageStartPos:YJPosition: AfQJAAAAAAAA:54205,\
             IntervalWords:320,TotalTime:{total_ms},TotalWords:49583,\
             CurrentPos:YJPosition: AfQJAAAAAAAA:54205,\
             EndPos:YJPosition: AbcVAAAPAAAA:148207,PosLeft:94002,%Left:0.645;"
        )
    }

    #[test]
    fn a_night_of_suspends_between_page_events_ends_the_sitting() {
        let mut lines = vec![page("020000", 7_390_020), page("020040", 7_430_020)];
        // `suspending` every twenty minutes, under `SESSION_GAP_SECS`.
        let mut at = 2 * 3600 + 30 * 60;
        while at < 11 * 3600 + 40 * 60 {
            let (h, m) = (at / 3600, (at % 3600) / 60);
            lines.push(format!(
                "260807:{h:02}{m:02}00 powerd[1]: I lipc:evts:name=suspending, \
                 origin=com.lab126.powerd"
            ));
            at += 20 * 60;
        }
        lines.push(page("115000", 7_431_020));
        lines.push(page("115040", 7_471_020));

        let out = parse_sessions(lines.iter().map(String::as_str), &[]);
        assert_eq!(
            out.len(),
            2,
            "a book left open over a sleep is two sittings"
        );
        assert_eq!(out[0].started_at, "2026-08-07T02:00:00");
        assert_eq!(out[0].ended_at, "2026-08-07T02:00:40");
        assert_eq!(out[1].started_at, "2026-08-07T11:50:00");
        assert_eq!(out[1].ended_at, "2026-08-07T11:50:40");
        // `seconds` at each end is the counter's own span.
        assert_eq!(out[0].seconds, 40);
        assert_eq!(out[1].seconds, 40);
    }

    /// An `ereader_book_consume_content` record at `hhmmss`, stating `words`.
    fn worded_page(hhmmss: &str, words: i64) -> String {
        format!(
            "260807:{hhmmss} fastmetrics[9842]: D fastmetrics: Emitting a new record. \
             SchemaName[ereader_book_consume_content], Fields[{{ \"words_count\" : {words} }} ]. :"
        )
    }

    /// The same record, stating where the screen began.
    fn placed_page(hhmmss: &str, words: i64, start: i64) -> String {
        format!(
            "260807:{hhmmss} fastmetrics[9842]: D fastmetrics: Emitting a new record. \
             SchemaName[ereader_book_consume_content], Fields[{{ \"start_position\" : {start}, \
             \"words_count\" : {words} }} ]. :"
        )
    }

    /// A redraw repeats `start_position`, and its words stand on the screen the
    /// record before it counted.
    #[test]
    fn a_redrawn_screen_counts_its_words_once() {
        let lines = [
            page("101500", 7_390_020),
            placed_page("101500", 300, 1000),
            placed_page("101530", 300, 1000),
            placed_page("101600", 250, 1301),
            placed_page("101630", 100, 1552),
        ];
        let out = parse_sessions(lines.iter().map(String::as_str), &[]);
        assert_eq!(out.len(), 1);
        // Three closed intervals of 30 s. The record at 101630 stays open.
        assert_eq!(out[0].paged_seconds, 90);
        // 300 once for the screen redrawn at 101530, then 250.
        assert_eq!(out[0].paged_words, 550, "a redraw counted its words twice");
    }

    #[test]
    fn a_run_the_timer_states_no_words_for_counts_the_pages_own() {
        // `TotalTime` never moves on a book the timer declines to count, and
        // the word span with it.
        let lines = [
            power("105000", "outOfScreenSaver"),
            page("105005", 7_390_020),
            worded_page("105010", 300),
            worded_page("105110", 300),
            worded_page("105210", 300),
            page("105220", 7_390_020),
        ];
        let out = parse_sessions(lines.iter().map(String::as_str), &[]);
        assert_eq!(out.len(), 1);
        // Two pages close an interval; the third closes none.
        assert_eq!(out[0].words, 600, "the pages' own counts went unread");
        assert_ne!(out[0].measure, Measure::Counted);
    }

    #[test]
    fn a_fixed_layout_run_states_no_words_and_none_are_invented() {
        // Every page of an image-only book states zero words.
        let lines = [
            power("105000", "outOfScreenSaver"),
            page("105005", 7_390_020),
            wordless_page("105010"),
            wordless_page("105110"),
            wordless_page("105210"),
            page("105220", 7_390_020),
        ];
        let out = parse_sessions(lines.iter().map(String::as_str), &[]);
        assert_eq!(out.len(), 1);
        assert_eq!(out[0].words, 0, "an image-only book was credited words");
    }

    #[test]
    fn a_page_flipped_past_too_fast_to_read_carries_none_of_its_words() {
        // One page held a hundred seconds, one flipped past in a second. The
        // first counts and the second does not; the words follow it.
        let lines = [
            power("105000", "outOfScreenSaver"),
            page("105005", 7_390_020),
            worded_page("105010", 300),
            worded_page("105150", 300),
            worded_page("105151", 300),
            page("105220", 7_390_020),
        ];
        let out = parse_sessions(lines.iter().map(String::as_str), &[]);
        assert_eq!(out.len(), 1);
        assert_eq!(out[0].words, 300, "a flipped page was counted as read");
    }

    /// An `ereader_book_consume_content` record at `hhmmss`, no words on it.
    fn wordless_page(hhmmss: &str) -> String {
        format!(
            "260807:{hhmmss} fastmetrics[9842]: D fastmetrics: Emitting a new record. \
             SchemaName[ereader_book_consume_content], Fields[{{ \"words_count\" : 0 }} ]. :"
        )
    }

    /// A `powerd` LIPC event at `hhmmss`.
    fn power(hhmmss: &str, name: &str) -> String {
        format!("260807:{hhmmss} powerd[1]: I lipc:evts:name={name}, origin=com.lab126.powerd")
    }

    #[test]
    fn a_page_held_across_a_short_sleep_counts_only_where_the_device_was_awake() {
        // `TotalTime` never moves on a book the timer declines to count.
        let lines = [
            power("105000", "outOfScreenSaver"),
            untimed_page("105005", 7_390_020),
            wordless_page("105010"),
            untimed_page("105020", 7_390_020),
            power("105200", "goingToScreenSaver"),
            power("111000", "outOfScreenSaver"),
            wordless_page("111500"),
            untimed_page("111510", 7_390_020),
            power("111600", "goingToScreenSaver"),
        ];
        let out = parse_sessions(lines.iter().map(String::as_str), &[]);
        assert_eq!(out.len(), 1);
        assert_eq!(out[0].measure, Measure::Paged);
        // `ACTIVE` over 10:50:00-10:52:00 and 11:10:00-11:15:00. `hours`
        // splits between the two in proportion and names no hour between.
        assert_eq!(out[0].hours, vec![(10, 32), (11, 88)]);
        let summed: i64 = out[0].hours.iter().map(|(_, s)| s).sum();
        assert_eq!(summed, out[0].seconds);
    }

    #[test]
    fn a_run_no_power_line_brackets_still_states_the_wall_clock_it_ran_over() {
        // Three `page` lines, and no `power` line above or between them.
        let lines = [
            page("105000", 7_390_020),
            page("105500", 7_420_020),
            page("110000", 7_450_020),
        ];
        let out = parse_sessions(lines.iter().map(String::as_str), &[]);
        assert_eq!(out.len(), 1);
        assert_eq!(out[0].measure, Measure::Counted);
        assert_eq!(out[0].seconds, 60, "the counter credited a minute of it");
        assert_eq!(out[0].awake_seconds, 600);
    }

    #[test]
    fn a_run_broken_by_a_sleep_states_the_wall_clock_less_the_sleep() {
        let lines = [
            page("105000", 7_390_020),
            power("105500", "goingToScreenSaver"),
            power("110500", "outOfScreenSaver"),
            page("111000", 7_420_020),
        ];
        let out = parse_sessions(lines.iter().map(String::as_str), &[]);
        assert_eq!(out.len(), 1);
        // 10:50:00 to 11:10:00, asleep 10:55:00 to 11:05:00.
        assert_eq!(out[0].awake_seconds, 600);
    }

    #[test]
    fn remeasure_takes_the_figures_and_keeps_what_the_fresh_parse_states_none_of() {
        let mut held = Session {
            seconds: 9,
            awake_seconds: 0,
            measure: Measure::Paged,
            asin: Some("B00OKPCRLG".into()),
            tz_offset_s: Some(10_800),
            stated_wpm: Some(240),
            ..Session::default()
        };
        let fresh = Session {
            seconds: 41,
            awake_seconds: 600,
            measure: Measure::Counted,
            asin: None,
            tz_offset_s: None,
            stated_wpm: Some(260),
            ..Session::default()
        };
        assert!(held.remeasure(&fresh));
        assert_eq!(held.seconds, 41);
        assert_eq!(held.awake_seconds, 600);
        assert_eq!(held.measure, Measure::Counted);
        assert_eq!(held.stated_wpm, Some(260));
        assert_eq!(held.asin.as_deref(), Some("B00OKPCRLG"));
        assert_eq!(held.tz_offset_s, Some(10_800));
        assert!(!held.remeasure(&held.clone()), "an unchanged row counted");
    }

    /// An `OpenBook` line with `StoredBookData:null`.
    fn open_book(hhmmss: &str) -> String {
        format!(
            "260807:{hhmmss} java[1]: I ReadingTimerController:Information::OpenBook,\
             StoredBookData:null;"
        )
    }

    /// A reading-timer line stating a position and no counter.
    fn position(hhmmss: &str) -> String {
        format!(
            "260807:{hhmmss} cvm[6144]: I ReadingTimerController:Information::\
             CurrentPos:YJPosition: AfQJAAAAAAAA:54205,\
             EndPos:YJPosition: AbcVAAAPAAAA:148207,PosLeft:94002,%Left:0.645;"
        )
    }

    #[test]
    fn the_device_waking_each_hour_of_a_night_draws_no_hour_of_reading() {
        let mut lines = vec![
            power("024000", "outOfScreenSaver"),
            open_book("024552"),
            wordless_page("024552"),
            position("024553"),
            power("024630", "goingToScreenSaver"),
        ];
        // `power` pairs two seconds apart, at the top of each hour.
        for hour in 3..=11 {
            lines.push(power(&format!("{hour:02}0000"), "outOfScreenSaver"));
            lines.push(power(&format!("{hour:02}0002"), "goingToScreenSaver"));
        }
        lines.push(power("113800", "outOfScreenSaver"));
        lines.push(wordless_page("113815"));
        lines.push(position("113818"));
        lines.push(power("114500", "goingToScreenSaver"));

        let out = parse_sessions(lines.iter().map(String::as_str), &[]);
        assert_eq!(out.len(), 1, "the night held one sitting, at its head");
        assert_eq!(out[0].started_at, "2026-08-07T02:45:52");
        assert_eq!(out[0].ended_at, "2026-08-07T02:45:53");
        assert_eq!(out[0].hours, vec![(2, 1)]);
        assert!(
            out.iter()
                .flat_map(|s| &s.hours)
                .all(|(hour, _)| !(3..=11).contains(hour)),
            "a wake the device took by itself was drawn as reading"
        );
    }

    #[test]
    fn an_open_a_gap_old_does_not_start_the_run_that_follows_it() {
        let lines = [
            power("100000", "outOfScreenSaver"),
            open_book("100000"),
            position("100001"),
            power("100100", "goingToScreenSaver"),
            power("120000", "outOfScreenSaver"),
            position("120005"),
            position("120010"),
            power("120100", "goingToScreenSaver"),
        ];
        let out = parse_sessions(lines.iter().map(String::as_str), &[]);
        assert_eq!(out.len(), 2);
        // `open_book` stands a second before the run it starts.
        assert_eq!(out[0].started_at, "2026-08-07T10:00:00");
        // Two hours on, `started_at` is the observation's own.
        assert_eq!(out[1].started_at, "2026-08-07T12:00:05");
    }

    #[test]
    fn a_page_between_the_floor_and_the_cap_counts_whole() {
        // `words` 0: [`FLOOR_SECS`] and [`CAP_SECS`] decide.
        assert_eq!(credited_ms(3_000, 0), 3_000);
        assert_eq!(credited_ms(55_000, 0), 55_000);
        assert_eq!(credited_ms(120_000, 0), 120_000);
    }

    #[test]
    fn a_page_standing_past_the_cap_counts_the_cap() {
        assert_eq!(credited_ms(600_000, 0), 120_000);
        // 80 words over 230 s is 21 a minute: the words vouch for none of it.
        assert_eq!(credited_ms(230_000, 80), 120_000);
    }

    #[test]
    fn a_page_swiped_past_counts_nothing() {
        assert_eq!(credited_ms(2_999, 0), 0);
        assert_eq!(credited_ms(0, 0), 0);
        // Under [`FLOOR_SECS`] whatever `words` states.
        assert_eq!(credited_ms(500, 100), 0);
    }

    /// Past [`FLOOR_SECS`], a page whose words reach [`WPM_MIN`] counts whole.
    #[test]
    fn a_page_its_words_vouch_for_counts_whole() {
        // 214 words over 257 s is 50 a minute, over [`WPM_MIN`].
        assert_eq!(credited_ms(257_000, 214), 257_000);
        // Exactly `WPM_MIN`: 40 words in 60 s.
        assert_eq!(credited_ms(60_000, 40), 60_000);
        // A hair under [`WPM_MIN`]: [`CAP_SECS`] takes it.
        assert_eq!(credited_ms(60_001, 40), 60_000);
        assert_eq!(credited_ms(600_000, 399), 120_000);
    }

    #[test]
    fn a_sitting_that_measured_nothing_is_not_a_sitting() {
        assert!(parse_sessions([CVM[0]], &[]).is_empty());
    }

    #[test]
    fn a_percentage_is_read_off_the_book_payload_and_not_the_chapter() {
        assert_eq!(percent_left(CVM[0], 148_207), Some(0.6450299914310198));
        // `percent_left` answers nothing for the chapter's own end position.
        assert_eq!(percent_left(CVM[0], 56_499), None);
    }
}
