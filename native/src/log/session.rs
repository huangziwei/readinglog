//! Turning a stream of log lines into sittings. A [`Session`] is a run of page
//! events on one book, ended by a close, a change of book, a gap over
//! [`SESSION_GAP_SECS`], or midnight; [`Session::seconds`] is the counter's span.

use super::line;
use super::line::{
    Moment, Observation, end_position, last_event, observation, opened_at_counter, payloads,
    resumed, stamp, toc_and_book,
};
use super::metric::{Metric, cde_key, metric};
use super::power::{Awake, is_state_change};

/// The gap between two reader events that cuts a session.
pub const SESSION_GAP_SECS: i64 = 30 * 60;

/// How far the wall clock may step back inside a run before the run is cut.
const CLOCK_BACK_SECS: i64 = 60;

/// How far a session's opening counter may outrun the wall clock.
const SEED_SLACK_SECS: i64 = 60;

/// How far a `powerd` record of `ACTIVE` may trail a `Reading_Resumed`.
const WAKE_LAG_SECS: i64 = 5;

/// The words a minute under which a page stood longer than its words account for.
const RATE_MIN: f64 = 40.0;

/// The least a page must stand to count, in seconds.
const PAGE_FLOOR_SECS: i64 = 3;
/// The most a page counts however long it stood, in seconds.
const PAGE_CAP_SECS: i64 = 120;

/// A page's words a minute, in the `f64` steps the log's `IntervalWPM` prints.
fn page_rate(words: i64, standing_ms: i64) -> f64 {
    words as f64 / (standing_ms as f64 / 1000.0) * 60.0
}

/// What one turn line's page contributes.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
struct Credit {
    /// Milliseconds and words the page added to `TotalTime` and `TotalWords`.
    counted_ms: i64,
    counted_words: i64,
    /// Milliseconds of a page left out of `TotalTime` whose rate reaches [`RATE_MIN`].
    refused_ms: i64,
}

impl Credit {
    /// Milliseconds the page contributes to [`Measure::Timed`]. `counted_ms`
    /// and `refused_ms` are never both set.
    fn timed_ms(self) -> i64 {
        self.counted_ms + self.refused_ms
    }
}

/// Read the page `obs` turned. `refused_ms` takes a page `TotalTime` left out
/// wherever its own rate reaches [`RATE_MIN`], whatever the event.
fn credits(obs: &Observation) -> Credit {
    let Some(standing_ms) = obs.interval_ms.filter(|ms| *ms > 0) else {
        return Credit::default();
    };
    // `obs` stating no `TotalTime` states an interval no counter reached.
    if obs.total_ms.is_none() {
        return Credit::default();
    }
    let words = obs.interval_words.unwrap_or(0).max(0);
    // `closes`: an interval `TotalTime` never takes.
    if !obs.refused && !obs.closes {
        return Credit {
            counted_ms: standing_ms,
            counted_words: words,
            refused_ms: 0,
        };
    }
    match page_rate(words, standing_ms) >= RATE_MIN {
        true => Credit {
            refused_ms: standing_ms,
            ..Credit::default()
        },
        false => Credit::default(),
    }
}

/// How much of a page's standing time counts, in seconds.
fn paged_secs(standing_secs: i64) -> i64 {
    match standing_secs {
        secs if secs < PAGE_FLOOR_SECS => 0,
        secs => secs.min(PAGE_CAP_SECS),
    }
}

/// How a session's seconds were arrived at, ranked best first.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Measure {
    /// The span of `TotalTime`.
    #[default]
    Counted,
    /// What `TotalTime` credits, plus the pages it left out reaching [`RATE_MIN`].
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

    /// Read a stored row's word, `dwell` the same measure as `paged`.
    pub fn from_stored(s: &str) -> Self {
        match s {
            "timed" => Self::Timed,
            "paged" | "dwell" => Self::Paged,
            "awake" => Self::Awake,
            _ => Self::Counted,
        }
    }
}

/// One run of a book: the place it opened at and the place it closed at, each
/// a fraction of `Session::end_position`. `to` under `from` runs backwards.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Stretch {
    pub from: f64,
    pub to: f64,
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
    /// Screens advanced, at the font size each was drawn at.
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
    /// The runs this sitting covered, in reading order, empty where no line stated a place.
    pub stretches: Vec<Stretch>,
    /// Seconds left in the book as it closed, off `NewTimeLeft`.
    pub time_left: Option<i64>,
    /// The rate off `TotalWPM` as the sitting closed, in whole words a minute.
    pub stated_wpm: Option<i64>,
    /// Seconds the run's `IntervalTime`s credit through [`credits`], zero where none is stated.
    pub timed_seconds: i64,
    /// The words on the pages that credited time, off `IntervalWords`.
    pub timed_words: i64,
    /// Seconds this run spanned, less the sleeps [`Awake`] states inside it.
    pub awake_seconds: i64,
    /// Seconds the run's `ereader_book_consume_content` pages credit, whatever [`Measure`] won.
    pub paged_seconds: i64,
    /// The words on those pages, inside [40, 900] a minute or not.
    pub paged_words: i64,
    /// The share of the book those same pages cover, summed page by page.
    pub covered: f64,
    /// The book's reading counter where this run began, `None` with `end_counter_ms`.
    pub start_counter_ms: Option<i64>,
    /// The same counter where the run was last seen.
    pub end_counter_ms: Option<i64>,
    /// `start_counter_ms`'s reading of `TotalWords`.
    pub start_words: Option<i64>,
    /// `end_counter_ms`'s reading of `TotalWords`.
    pub end_words: Option<i64>,
    /// Seconds ahead of UTC at the pass that read this sitting, set by `Store::absorb`.
    pub tz_offset_s: Option<i64>,
}

impl Session {
    /// Take `fresh`'s figures over this sitting's own, keeping every field
    /// `fresh` states nothing for. Answers whether anything moved.
    pub fn remeasure(&mut self, fresh: &Session) -> bool {
        let was = self.clone();
        // `fresh` reading more of the run states its earlier start and later close.
        if fresh.started_at < self.started_at {
            self.started_at = fresh.started_at.clone();
        }
        if fresh.ended_at > self.ended_at {
            self.ended_at = fresh.ended_at.clone();
        }
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
        self.covered = fresh.covered;
        self.start_counter_ms = fresh.start_counter_ms.or(self.start_counter_ms);
        self.end_counter_ms = fresh.end_counter_ms.or(self.end_counter_ms);
        self.start_words = fresh.start_words.or(self.start_words);
        self.end_words = fresh.end_words.or(self.end_words);
        self.asin = fresh.asin.clone().or_else(|| self.asin.clone());
        self.progress = fresh.progress.or(self.progress);
        // `Store::heal_from` is the one caller, parsing from an empty
        // watermark, and `fresh` opens where this sitting opens.
        if !fresh.stretches.is_empty() {
            self.stretches = fresh.stretches.clone();
        }
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
    /// A gap, a close, or another book, the run ending where it was last seen.
    Left,
    /// Midnight inside an interval, `counter_ms` and `words` interpolated at it.
    Midnight(Box<Start>),
}

/// An `OpenBook` unattached to a session.
struct Opened {
    counter_ms: i64,
    at: Moment,
}

/// A `Reading_Resumed` unattached to a session.
struct Woke {
    at: Moment,
    /// Whether a `Metric` page or turn followed `at`.
    read: bool,
    /// Whether `stepped` held after `read`.
    lapsed: bool,
}

/// Where a session begins: an `Opened`, a `Woke` or a `Break::Midnight`.
struct Start {
    /// `None` on an untimed book.
    counter_ms: Option<i64>,
    /// `TotalWords` beside `counter_ms`.
    words: Option<i64>,
    at: Moment,
    /// Milliseconds of the page open across `at` the run before took.
    credited_ms: i64,
    /// A counter for the run's first counted line to settle.
    floor: Option<Floor>,
    /// The `Floor` that settled `counter_ms`, and what it left to `owed` runs.
    settled: Option<(Floor, Settled)>,
}

/// A counter a run opens at, stated ahead of the run's first counted line.
#[derive(Debug, Clone, Copy, PartialEq)]
struct Floor {
    counter_ms: i64,
    words: Option<i64>,
    /// 1000 off `StoredBookData`'s whole seconds, 1 off `TotalTime`.
    step_ms: i64,
    /// The instant a line stated `counter_ms`.
    stated: i64,
}

/// What `Floor::settle` finds.
#[derive(Debug, Clone, Copy, PartialEq)]
struct Settled {
    counter_ms: i64,
    words: Option<i64>,
    /// The gain past `Floor::counter_ms` left to the `owed` runs.
    owed_ms: i64,
    owed_words: Option<i64>,
}

impl Floor {
    /// `obs` settles the gain past `counter_ms` no line stated: `after` seconds
    /// awake take their share against `owed`. `None` where the gain is negative
    /// or outruns both; `unseen` under `step_ms` is rounding.
    fn settle(&self, obs: &Observation, credit: Credit, owed: i64, after: i64) -> Option<Settled> {
        let total = obs.total_ms?;
        let unseen = total - credit.counted_ms - self.counter_ms;
        if unseen < 0 || unseen > (owed + after + SEED_SLACK_SECS) * 1000 {
            return None;
        }
        let rounded = unseen < self.step_ms;
        let ours = |unseen: i64| match (rounded, owed + after) {
            (true, _) => 0,
            (false, 0) => unseen,
            (false, span) => unseen * after / span,
        };
        let left = |unseen: i64| match rounded {
            true => 0,
            false => unseen - ours(unseen),
        };
        let words = self.words.zip(obs.words).and_then(|(from, to)| {
            let unseen = to - credit.counted_words - from;
            (unseen >= 0).then_some((to - credit.counted_words - ours(unseen), left(unseen)))
        });
        Some(Settled {
            counter_ms: total - credit.counted_ms - ours(unseen),
            words: words.map(|(at, _)| at),
            owed_ms: left(unseen),
            owed_words: words.map(|(_, owed)| owed),
        })
    }
}

/// The highest `(EndPos, TotalTime, TotalWords)` each book was logged at, and
/// apart from it the latest pair a line of this batch stated, with when.
struct Counters {
    highest: Vec<(i64, i64, i64)>,
    latest: Vec<(i64, i64, i64, i64)>,
}

impl Counters {
    /// The rows [`parse_sessions`] was handed, as its own copy.
    fn held(rows: &[(i64, i64, i64)]) -> Self {
        Self {
            highest: rows.to_vec(),
            latest: Vec::new(),
        }
    }

    /// The `TotalWords` standing beside `counter_ms` for the book at `position`,
    /// matched to the second `StoredBookData` states.
    fn words_at(&self, position: i64, counter_ms: i64) -> Option<i64> {
        self.highest
            .iter()
            .find(|(ep, ms, _)| *ep == position && *ms / 1000 == counter_ms / 1000)
            .map(|(_, _, words)| *words)
    }

    /// The one book whose counter, `latest` where held and `highest` where
    /// not, stands at a `counter_ms` above zero to the second.
    fn named_by(&self, counter_ms: i64) -> Option<i64> {
        let standing = |&(position, highest, _): &(i64, i64, i64)| {
            let latest = self.latest.iter().find(|row| row.0 == position);
            (position, latest.map_or(highest, |row| row.1))
        };
        let mut named = self
            .highest
            .iter()
            .map(standing)
            .filter(|(_, ms)| counter_ms > 0 && ms / 1000 == counter_ms / 1000);
        match (named.next(), named.next()) {
            (Some((position, _)), None) => Some(position),
            _ => None,
        }
    }

    /// The `latest` pair of `position`, as a `Floor`.
    fn stated(&self, position: i64) -> Option<Floor> {
        let &(_, counter_ms, words, stated) = self.latest.iter().find(|row| row.0 == position)?;
        Some(Floor {
            counter_ms,
            words: Some(words),
            step_ms: 1,
            stated,
        })
    }

    /// Hold what an observation at `at` states: as the highest where its
    /// counter outruns the one held, and as the latest whatever it states.
    fn learn(&mut self, obs: &Observation, at: i64) {
        let (Some(total_ms), Some(words)) = (obs.total_ms, obs.words) else {
            return;
        };
        match self
            .highest
            .iter_mut()
            .find(|(ep, _, _)| *ep == obs.position)
        {
            Some(held) if held.1 < total_ms => *held = (obs.position, total_ms, words),
            Some(_) => {}
            None => self.highest.push((obs.position, total_ms, words)),
        }
        let latest = (obs.position, total_ms, words, at);
        match self.latest.iter_mut().find(|row| row.0 == obs.position) {
            Some(held) => *held = latest,
            None => self.latest.push(latest),
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
                credited_ms: 0,
                floor: None,
                settled: None,
            })
    }

    /// `at` as the start of a run `obs` opens stating no counter, within
    /// [`SESSION_GAP_SECS`]; `counter_ms` waits on the first counted line.
    fn opened_run(&self, now: &Moment, obs: &Observation, counters: &Counters) -> Option<Start> {
        let elapsed = now.abs - self.at.abs;
        (self.at.day == now.day && (0..=SESSION_GAP_SECS).contains(&elapsed))
            .then(|| self.carried(obs.position, counters))
    }

    /// The run of the book `Counters::named_by` names from `at`, over the
    /// `pending` records from `at`, where they close a page.
    fn named_run(
        &self,
        pending: &mut Vec<(Moment, Metric)>,
        named: Option<&(i64, String)>,
        counters: &Counters,
        awake: &Awake,
    ) -> Option<Open> {
        let book = counters.named_by(self.counter_ms)?;
        let mut run = Open::new(book, &self.at, Some(self.carried(book, counters)));
        run.asin = named
            .filter(|(at, _)| *at >= self.at.abs)
            .map(|(_, key)| key.clone());
        for (at, m) in pending.iter().filter(|(at, _)| at.abs >= self.at.abs) {
            run.observe_metric(at, m, awake);
        }
        if run.paged_total_ms == 0 {
            return None;
        }
        pending.retain(|(at, _)| at.abs < self.at.abs);
        Some(run)
    }

    /// `at` as the start of a run of `position`, `counter_ms` its `Floor`.
    fn carried(&self, position: i64, counters: &Counters) -> Start {
        Start {
            counter_ms: None,
            words: None,
            at: self.at.clone(),
            credited_ms: 0,
            floor: Some(Floor {
                counter_ms: self.counter_ms,
                words: counters.words_at(position, self.counter_ms),
                step_ms: 1000,
                stated: self.at.abs,
            }),
            settled: None,
        }
    }
}

impl Woke {
    /// `Counters::stated` for `position`, else the `held` row opening at `at`.
    fn floor(&self, position: i64, counters: &Counters, held: &[Session]) -> Option<Floor> {
        counters.stated(position).or_else(|| {
            let stored = held
                .iter()
                .find(|s| s.started_at == self.at.at && s.end_position == position)?;
            Some(Floor {
                counter_ms: stored.start_counter_ms?,
                words: stored.start_words,
                step_ms: 1,
                stated: self.at.abs,
            })
        })
    }

    /// The `end_position` of the `held` row opening at `at`.
    fn held_book(&self, held: &[Session]) -> Option<i64> {
        held.iter()
            .find(|s| s.started_at == self.at.at)
            .map(|s| s.end_position)
    }

    /// Whether `at` can start a run at `now`: the same day, no `lapsed`, no
    /// `Awake::slept_after` it, and asleep at most [`WAKE_LAG_SECS`] past it.
    fn stands(&self, now: &Moment, awake: &Awake) -> bool {
        let span = now.abs - self.at.abs;
        !self.lapsed
            && self.at.day == now.day
            && span >= 0
            && !awake.slept_after(self.at.abs, now.abs)
            && span - awake.bound(self.at.abs, now.abs) <= WAKE_LAG_SECS
    }

    /// `at` as the start of the run `obs` opens: `floor` settles at `obs` where
    /// `obs` states a counter, else waits on the run's first counted line.
    fn start(
        self,
        now: &Moment,
        obs: &Observation,
        floor: Option<Floor>,
        owed: i64,
        awake: &Awake,
    ) -> Option<Start> {
        if !self.stands(now, awake) {
            return None;
        }
        let after = awake.bound(self.at.abs, now.abs);
        let settled = match obs.total_ms {
            Some(_) => floor.and_then(|f| Some((f, f.settle(obs, credits(obs), owed, after)?))),
            None => None,
        };
        Some(Start {
            counter_ms: settled.map(|(_, s)| s.counter_ms),
            words: settled.and_then(|(_, s)| s.words),
            at: self.at,
            credited_ms: 0,
            floor: floor.filter(|_| obs.total_ms.is_none()),
            settled,
        })
    }

    /// `at` as the start of a run a page record at `now` opens, on `floor`.
    fn begin(self, now: &Moment, floor: Option<Floor>, awake: &Awake) -> Option<Start> {
        if !self.stands(now, awake) {
            return None;
        }
        Some(Start {
            counter_ms: None,
            words: None,
            floor,
            at: self.at,
            credited_ms: 0,
            settled: None,
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
    /// The last observation stating `TotalTime`.
    stated_at: Option<Moment>,
    /// The newest observation or `Metric` other than `Metric::Close`.
    seen_at: Moment,
    /// Milliseconds of counted reading booked against each hour of the day.
    hours_ms: [i64; 24],
    began: Moment,
    /// `Metric::Forward` turns, apart from `page_turns`.
    metric_turns: i64,
    /// `Metric::Page` words on the pages `paged_secs` counted.
    metric_words: i64,
    /// Whether a `Start` made the first observation's `IntervalTime` this run's.
    vouched: bool,
    /// A `Start::floor` waiting on the first counted line.
    floor: Option<Floor>,
    /// `Start::settled`, until `parse_restating` shares `Settled::owed_ms`.
    settled: Option<(Floor, Settled)>,
    /// `Start::credited_ms`.
    credited_before: i64,
    /// Observations `observe` has folded in.
    seen: i64,
    /// Milliseconds `credits` admits, and the `IntervalWords` beside them.
    interval_total_ms: i64,
    interval_words: i64,
    interval_hours_ms: [i64; 24],
    /// Milliseconds `paged_secs` admits.
    paged_total_ms: i64,
    paged_hours_ms: [i64; 24],
    /// `share_of` summed over the pages `paged_secs` counted.
    covered: f64,
    open_page: Option<Page>,
    /// The last `cde_key` named while the run was open.
    asin: Option<String>,
    /// The last `%Left` a line stated for `end_position`.
    progress: Option<f64>,
    /// The `Stretch` runs closed, and the one open with whether it turned.
    stretches: Vec<Stretch>,
    open_stretch: Option<(f64, f64, bool)>,
    /// The last `NewTimeLeft` a line stated for `end_position`.
    time_left: Option<i64>,
    /// The last `TotalWPM` a line stated for `end_position`.
    stated_wpm: Option<i64>,
}

impl Open {
    /// A run of `end_position` from `start`, else from `now` with no floor.
    fn new(end_position: i64, now: &Moment, start: Option<Start>) -> Self {
        let vouched = start.is_some();
        let credited_before = start.as_ref().map_or(0, |s| s.credited_ms);
        let (time_lo, words_lo, from, floor, settled) = match start {
            Some(s) => (s.counter_ms, s.words, s.at, s.floor, s.settled),
            None => (None, None, now.clone(), None, None),
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
            // `last_time` opens at `time_lo`, the first observation booking the span from it.
            last_time: time_lo,
            last_words: words_lo,
            stated_at: None,
            hours_ms: [0; 24],
            began: from.clone(),
            seen_at: from.clone(),
            last: from,
            metric_turns: 0,
            metric_words: 0,
            vouched,
            floor,
            settled,
            credited_before,
            seen: 0,
            interval_total_ms: 0,
            interval_words: 0,
            interval_hours_ms: [0; 24],
            paged_total_ms: 0,
            paged_hours_ms: [0; 24],
            covered: 0.0,
            open_page: None,
            asin: None,
            progress: None,
            stretches: Vec::new(),
            open_stretch: None,
            time_left: None,
            stated_wpm: None,
        }
    }

    /// Fold `at` into the runs: `jumped` closes the open run and opens one at
    /// `at`, else `at` extends the open run and marks it turned.
    /// [`Open::covered`] drops a run holding no turn.
    fn place(&mut self, at: f64, jumped: bool) {
        match self.open_stretch {
            // `to` holds the place of the line before this one, which is the
            // page left; `at` is the destination.
            Some((from, to, turned)) if jumped => {
                if turned {
                    self.stretches.push(Stretch { from, to });
                }
                self.open_stretch = Some((at, at, false));
            }
            Some((from, _, _)) => self.open_stretch = Some((from, at, true)),
            None => self.open_stretch = Some((at, at, !jumped)),
        }
    }

    /// [`Open::stretches`] with the open run closed at its last place, where it turned.
    fn covered(&self) -> Vec<Stretch> {
        let mut out = self.stretches.clone();
        if let Some((from, to, true)) = self.open_stretch {
            out.push(Stretch { from, to });
        }
        out
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

    /// Fold `obs` in. `owed` is the seconds awake `Floor::settle` weighs the
    /// `owed` runs by.
    fn observe(&mut self, now: &Moment, obs: &Observation, awake: &Awake, owed: i64) {
        // `credit` is nothing for an unvouched first observation, its page before the run.
        self.seen += 1;
        let credit = match self.seen == 1 && !self.vouched {
            true => Credit::default(),
            false => credits(obs),
        };
        // `credited_before` stays with the run a midnight cut.
        let timed = match self.seen {
            1 => (credit.timed_ms() - self.credited_before).max(0),
            _ => credit.timed_ms(),
        };
        // `floor` settles at the first counted line, its gain booked from
        // `began`.
        if self.time_lo.is_none()
            && let Some(total) = obs.total_ms
            && let Some(floor) = self.floor.take()
            && let Some(settled) =
                floor.settle(obs, credit, owed, awake.bound(self.began.abs, now.abs))
        {
            let counter = settled.counter_ms;
            credit_awake(&mut self.hours_ms, awake, &self.began, now, total - counter);
            self.time_lo = Some(counter);
            self.time_hi = counter;
            self.last_time = Some(total);
            if let Some(words) = settled.words
                && self.words_lo.is_none()
            {
                self.words_lo = Some(words);
                self.words_hi = words;
                self.last_words = Some(words);
            }
            self.settled = Some((floor, settled));
        }
        if obs.total_ms.is_some() {
            self.stated_at = Some(now.clone());
        }
        // `time_lo` and `words_lo` unset take a page under `total_ms` and `words`.
        if let (None, Some(t)) = (self.time_lo, obs.total_ms) {
            self.time_lo = Some(t - credit.counted_ms);
            self.time_hi = t - credit.counted_ms;
            self.last_time = Some(t - credit.counted_ms);
        }
        if let (None, Some(w)) = (self.words_lo, obs.words) {
            self.words_lo = Some(w - credit.counted_words);
            self.words_hi = w - credit.counted_words;
            self.last_words = Some(w - credit.counted_words);
        }
        if let (Some(from), Some(to)) = (self.last_time, obs.total_ms) {
            Self::credit(&mut self.hours_ms, self.last.secs, now.secs, to - from);
        }
        // `timed` off `IntervalTime`, apart from `total_ms`.
        if timed > 0 {
            self.interval_total_ms += timed;
            self.interval_words += obs.interval_words.unwrap_or(0).max(0);
            Self::credit(&mut self.interval_hours_ms, self.last.secs, now.secs, timed);
        }
        self.ended_at = now.at.clone();
        self.last = now.clone();
        self.seen_at = now.clone();
        if obs.page_turn {
            self.page_turns += 1;
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
    }

    /// The last reading line: `seen_at` where `page_turns` is zero, else `last`.
    fn end(&self) -> &Moment {
        match self.page_turns {
            0 if self.seen_at.abs > self.last.abs => &self.seen_at,
            _ => &self.last,
        }
    }

    /// Whether `TotalTime` can have moved past `stated_at` on lines no
    /// observation is.
    fn owing(&self) -> bool {
        self.page_turns == 0
            && self
                .stated_at
                .as_ref()
                .is_none_or(|at| self.seen_at.abs > at.abs)
    }

    /// Raise the counters from `from` by `gain`, booked from `stated_at` to
    /// `end`; `words` is the same pair for `TotalWords`.
    fn top_up(&mut self, from: i64, gain: i64, words: Option<(i64, i64)>, awake: &Awake) {
        if gain <= 0 {
            return;
        }
        let since = self.stated_at.clone().unwrap_or_else(|| self.began.clone());
        let end = self.end().clone();
        credit_awake(&mut self.hours_ms, awake, &since, &end, gain);
        let lo = *self.time_lo.get_or_insert(from);
        self.time_hi = self.time_hi.max(lo) + gain;
        if let Some((from, gain)) = words {
            let lo = *self.words_lo.get_or_insert(from);
            self.words_hi = self.words_hi.max(lo) + gain;
        }
    }

    /// Fold one `fastmetrics` record in. `Metric::Page` closes the page
    /// before it; `ended_at` moves where `page_turns` is zero.
    fn observe_metric(&mut self, now: &Moment, m: &Metric, awake: &Awake) {
        if *m != Metric::Close && now.abs > self.seen_at.abs {
            self.seen_at = now.clone();
            if self.page_turns == 0 {
                self.ended_at = now.at.clone();
            }
        }
        match m {
            Metric::Forward => self.metric_turns += 1,
            Metric::Back => {}
            Metric::Close => self.open_page = None,
            Metric::Page { words, start, end } => {
                if let Some(page) = self.open_page.take() {
                    // `Awake::bound`: the interval less the sleeps it states.
                    let counts = paged_secs(awake.bound(page.at.abs, now.abs)) * 1000;
                    self.paged_total_ms += counts;
                    // No words where `counts` is zero or `redraw` repeats `start`.
                    let redraw = *start >= 0 && *start == page.start;
                    if counts > 0 && !redraw {
                        self.metric_words += page.words;
                        self.covered += share_of(page.start, page.end, self.end_position);
                    }
                    credit_awake(&mut self.paged_hours_ms, awake, &page.at, now, counts);
                }
                self.open_page = Some(Page {
                    at: now.clone(),
                    words: *words,
                    start: *start,
                    end: *end,
                });
            }
        }
    }

    /// Whether `obs` ends the run, and how.
    fn broken_by(&self, now: &Moment, obs: &Observation, gapped: bool) -> Option<Break> {
        if self.end_position != obs.position || gapped {
            return Some(Break::Left);
        }
        if self.last.day == now.day {
            return None;
        }
        // `share` interpolates the counters at midnight; `Break::Left` where
        // either side states none.
        let (Some(from), Some(to)) = (self.last_time, obs.total_ms) else {
            return Some(Break::Left);
        };
        let elapsed = now.abs - self.last.abs;
        let before = now.abs - now.secs - self.last.abs;
        let credit = credits(obs);
        Some(Break::Midnight(Box::new(Start {
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
            credited_ms: share(credit.timed_ms(), elapsed, before),
            floor: None,
            settled: None,
        })))
    }

    /// The run cut at the midnight ahead of `now`, and the run of
    /// `end_position` from it, carrying `time_hi` or `floor` as its `Floor`.
    fn past_midnight(mut self, now: &Moment) -> (Open, Open) {
        let midnight = Moment {
            day: now.day.clone(),
            secs: 0,
            abs: now.abs - now.secs,
            at: format!("{}T00:00:00", now.day),
        };
        let floor = match self.time_lo {
            Some(_) => Some(Floor {
                counter_ms: self.time_hi,
                words: self.words_lo.map(|_| self.words_hi),
                step_ms: 1,
                stated: self.stated_at.as_ref().map_or(self.last.abs, |at| at.abs),
            }),
            None => self.floor,
        };
        let start = |floor| Start {
            counter_ms: None,
            words: None,
            at: midnight.clone(),
            credited_ms: 0,
            floor,
            settled: None,
        };
        let mut next = Open::new(self.end_position, &midnight, Some(start(floor)));
        next.asin = self.asin.clone();
        self.cut_at(&start(None));
        (self, next)
    }

    /// Close the run at midnight on `boundary`'s counters, `ended_at` the
    /// second before it.
    fn cut_at(&mut self, boundary: &Start) {
        let at_boundary = boundary.counter_ms.unwrap_or(self.time_hi);
        if let Some(from) = self.last_time {
            // `86_400`: midnight in `last`'s day.
            Self::credit(
                &mut self.hours_ms,
                self.last.secs,
                86_400,
                at_boundary - from,
            );
        }
        // `credited_ms`: the share of the page across `boundary` before it.
        if boundary.credited_ms > 0 {
            self.interval_total_ms += boundary.credited_ms;
            Self::credit(
                &mut self.interval_hours_ms,
                self.last.secs,
                86_400,
                boundary.credited_ms,
            );
        }
        self.time_hi = self.time_hi.max(at_boundary);
        if let Some(w) = boundary.words {
            self.words_hi = self.words_hi.max(w);
        }
        self.ended_at = format!("{}T23:59:59", self.last.day);
    }

    /// [`Self::cut_at`], then [`Self::finish`].
    fn finish_at(mut self, boundary: &Start, awake: &Awake) -> Session {
        self.cut_at(boundary);
        self.finish(awake)
    }

    /// The run as a session, its [`Measure`] the first of the four with a
    /// figure.
    fn finish(self, awake: &Awake) -> Session {
        let stretches = self.covered();
        let counted = (self.time_hi - self.time_lo.unwrap_or(self.time_hi)) / 1000;
        let timed = self.interval_total_ms / 1000;
        let paged = self.paged_total_ms / 1000;
        let last = self.end().clone();
        let last = &last;
        let witnessed = awake.between(self.began.abs, last.abs);
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
                Measure::Awake => spread(awake, &self.began, last, seconds),
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
            // The `TotalWords` span where stated, else `metric_words`, as `page_turns`
            // takes `metric_turns`.
            words: match self.words_hi - self.words_lo.unwrap_or(self.words_hi) {
                0 => self.metric_words,
                span => span,
            },
            measure,
            asin: self.asin,
            progress: self.progress,
            stretches,
            time_left: self.time_left,
            stated_wpm: self.stated_wpm,
            tz_offset_s: None,
            timed_seconds: timed,
            timed_words: self.interval_words,
            awake_seconds: awake.bound(self.began.abs, last.abs),
            paged_seconds: paged,
            paged_words: self.metric_words,
            covered: self.covered,
            start_counter_ms: self.time_lo,
            end_counter_ms: self.time_lo.map(|_| self.time_hi),
            start_words: self.words_lo,
            end_words: self.words_lo.map(|_| self.words_hi),
        }
    }
}

/// The screen a page record opened, held until the record after it closes it.
#[derive(Debug, Clone)]
struct Page {
    at: Moment,
    words: i64,
    /// The page's own span, which `share_of` reads as its share of the book.
    start: i64,
    end: i64,
}

/// The share of `extent` one page's own span covers, zero where `extent` or
/// either place is unstated.
fn share_of(from: i64, to: i64, extent: i64) -> f64 {
    match extent > 0 && from >= 0 && to >= 0 {
        true => (to - from).abs() as f64 / extent as f64,
        false => 0.0,
    }
}

/// Whether the wall clock moved further than one run may hold.
fn stepped(now: i64, prev: i64) -> bool {
    now - prev > SESSION_GAP_SECS || prev - now > CLOCK_BACK_SECS
}

/// Book `advance_ms` against the clock hours [`Awake`] holds. `from` and `to`
/// bracket an interval a page was open across; `advance_ms` splits between
/// [`Awake`]'s stretches in it, or goes whole where it names none.
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

/// A bounded total across the hours [`Awake`] holds.
fn spread(awake: &Awake, from: &Moment, to: &Moment, seconds: i64) -> Vec<(u8, i64)> {
    let mut hours_ms = [0; 24];
    credit_awake(&mut hours_ms, awake, from, to, seconds * 1000);
    hours_in_seconds(&hours_ms, seconds)
}

/// The hours a session's milliseconds fall in, as whole seconds summing to
/// exactly `seconds`. `running` truncates, and the busiest hour takes the rest.
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
    parse_restating(events, counters, &[])
}

/// [`parse_sessions`] over a batch restating the sittings in `held`: a `held`
/// row opening at a wake lends its run the counter and book the batch lacks.
pub fn parse_restating<'a>(
    events: impl IntoIterator<Item = &'a str>,
    counters: &[(i64, i64, i64)],
    held: &[Session],
) -> Vec<Session> {
    // [`Awake`] reads the whole stream before the first sitting closes.
    let lines: Vec<&str> = events.into_iter().collect();
    // Which lines are reading-timer lines, read once.
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
    // `powerd` lines alone state a change, `timer` lines filtered out.
    let woke = lines
        .iter()
        .copied()
        .zip(&timer)
        .filter_map(|(line, timer)| (!timer).then_some(line));
    let awake = Awake::from_events(woke).witnessed(&read_at);

    let mut out = Vec::new();
    let mut open: Option<Open> = None;
    // `Open::owing` runs, waiting on a statement of their book to share its gain.
    let mut owed: Vec<Open> = Vec::new();
    let mut prev_abs: Option<i64> = None;
    let mut opened: Option<Opened> = None;
    let mut wake: Option<Woke> = None;
    let mut counters = Counters::held(counters);
    // `gapped` holds a `stepped` until an observation acts on it.
    let mut gapped = false;
    // The newest `cde_key` and its instant.
    let mut named: Option<(i64, String)> = None;
    // `Metric` records no `live` run took.
    let mut pending: Vec<(Moment, Metric)> = Vec::new();
    // The last observation's `position`, cleared at a close or an `OpenBook`.
    let mut left_open: Option<i64> = None;

    for (line, &timer) in lines.iter().copied().zip(&timer) {
        let Some(now) = stamp(line) else {
            continue;
        };
        // `stepped` reads every line `is_state_change` rejects.
        if timer || !is_state_change(line) {
            let step = prev_abs.is_some_and(|prev| stepped(now.abs, prev));
            gapped |= step;
            if step && let Some(woke) = wake.as_mut() {
                woke.lapsed |= woke.read;
            }
            prev_abs = Some(now.abs);
        }

        if timer && let Some(counter_ms) = opened_at_counter(line) {
            if let Some(run) = opened
                .take()
                .and_then(|o| o.named_run(&mut pending, named.as_ref(), &counters, &awake))
            {
                retire(run, &mut owed, &mut out, &awake);
            }
            opened = Some(Opened {
                counter_ms,
                at: now.clone(),
            });
            wake = None;
            left_open = None;
        }
        if timer && resumed(line) {
            wake = Some(Woke {
                at: now.clone(),
                read: false,
                lapsed: false,
            });
        }

        // `live`: `open` within [`SESSION_GAP_SECS`] of `seen_at`, with no
        // `opened` pending.
        let live = opened.is_none()
            && open
                .as_ref()
                .is_some_and(|cur| !stepped(now.abs, cur.seen_at.abs));

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
            if !timer && let Some(m) = metric(line) {
                let reading = m != Metric::Close;
                if reading && let Some(woke) = wake.as_mut() {
                    woke.read = true;
                }
                // `reading` after `gapped` and `wake`, ahead of any observation,
                // opens a run of `left_open`, or of the `held` row at the wake.
                if reading
                    && (gapped || open.is_none())
                    && !live
                    && let Some(book) = left_open.or_else(|| wake.as_ref()?.held_book(held))
                    && let Some(woke) = wake.take()
                    && let Some(start) = {
                        let floor = woke.floor(book, &counters, held);
                        woke.begin(&now, floor, &awake)
                    }
                {
                    if let Some(cur) = open.take() {
                        retire(cur, &mut owed, &mut out, &awake);
                    }
                    gapped = false;
                    let at = start.at.clone();
                    let mut cur = Open::new(book, &at, Some(start));
                    cur.asin = named
                        .as_ref()
                        .filter(|(named_at, _)| *named_at >= at.abs)
                        .map(|(_, key)| key.clone());
                    for (pended, m) in std::mem::take(&mut pending) {
                        if pended.abs >= at.abs {
                            cur.observe_metric(&pended, &m, &awake);
                        }
                    }
                    cur.observe_metric(&now, &m, &awake);
                    open = Some(cur);
                    continue;
                }
                // `reading` past midnight cuts the `live` run there.
                if reading
                    && live
                    && let Some(cur) = open.take_if(|cur| cur.seen_at.day != now.day)
                {
                    let (cut, next) = cur.past_midnight(&now);
                    retire(cut, &mut owed, &mut out, &awake);
                    open = Some(next);
                }
                match open.as_mut().filter(|_| live) {
                    Some(cur) => cur.observe_metric(&now, &m, &awake),
                    None => pending.push((now.clone(), m)),
                }
            }
            continue;
        };
        let gapped = std::mem::take(&mut gapped);

        // `reopened`: `opened`'s counter where it starts the run.
        let mut reopened: Option<i64> = None;
        let mut seed = match obs.total_ms {
            Some(_) => opened.take().and_then(|o| {
                let read = o.counter_ms;
                let start = o.vouch(&now, &obs, &counters);
                reopened = start.as_ref().map(|_| read);
                start
            }),
            None => {
                let start = opened
                    .as_ref()
                    .and_then(|o| o.opened_run(&now, &obs, &counters));
                reopened = start.as_ref().and(opened.as_ref()).map(|o| o.counter_ms);
                opened = opened.filter(|_| start.is_none());
                start
            }
        };
        // `TimeRead` restates the counter at the book's last close: the gain
        // past `Counters::stated` goes to its `owed` runs.
        if let Some(read) = reopened
            && let Some(floor) = counters.stated(obs.position)
        {
            let span = owed_awake(&owed, obs.position, floor.stated, &awake);
            let gain = read - floor.counter_ms;
            let gain = match gain >= 1000 && gain <= (span + SEED_SLACK_SECS) * 1000 {
                true => gain,
                false => 0,
            };
            share_out(
                &mut owed,
                &mut out,
                obs.position,
                &floor,
                gain,
                None,
                &awake,
            );
        }
        let woken = wake.take();

        match open
            .as_ref()
            .and_then(|cur| cur.broken_by(&now, &obs, gapped))
        {
            None => {}
            Some(Break::Left) => {
                let cur = open.take().expect("a run to break");
                retire(cur, &mut owed, &mut out, &awake);
            }
            Some(Break::Midnight(boundary)) => {
                out.push(
                    open.take()
                        .expect("a run to cut")
                        .finish_at(&boundary, &awake),
                );
                seed = Some(*boundary);
            }
        }
        let fresh = open.is_none();
        if fresh && seed.is_none() {
            seed = woken.and_then(|w| {
                let floor = w.floor(obs.position, &counters, held);
                let span = floor.map_or(0, |f| owed_awake(&owed, obs.position, f.stated, &awake));
                w.start(&now, &obs, floor, span, &awake)
            });
        }
        let cur = open.get_or_insert_with(|| Open::new(obs.position, &now, seed.take()));
        // `fresh` takes `named` and `pending` from `began`, a run going on every
        // record `pending` holds.
        let from = match fresh {
            true => cur.began.abs,
            false => i64::MIN,
        };
        if fresh {
            cur.asin = named
                .as_ref()
                .filter(|(at, _)| *at >= from)
                .map(|(_, key)| key.clone());
        }
        for (at, m) in std::mem::take(&mut pending) {
            if at.abs >= from {
                cur.observe_metric(&at, &m, &awake);
            }
        }
        // `seed`'s counters go to a run `time_lo` and `words_lo` leave open.
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
            cur.place(1.0 - left, obs.jumped);
        }
        if let Some(secs) = time_left(line, cur.end_position) {
            cur.time_left = Some(secs);
        }
        if let Some(rate) = stated_wpm(line, cur.end_position) {
            cur.stated_wpm = Some(rate);
        }
        let span = cur
            .floor
            .map_or(0, |f| owed_awake(&owed, cur.end_position, f.stated, &awake));
        cur.observe(&now, &obs, &awake, span);
        // A settled `Floor` shares `owed_ms` out; any other counter finishes
        // the `owed` runs as they stand.
        match cur.settled.take() {
            Some((floor, settled)) => {
                let book = cur.end_position;
                let words = settled.owed_words;
                share_out(
                    &mut owed,
                    &mut out,
                    book,
                    &floor,
                    settled.owed_ms,
                    words,
                    &awake,
                );
            }
            None if obs.total_ms.is_some() => {
                share_out(&mut owed, &mut out, obs.position, &FLAT, 0, None, &awake);
            }
            None => {}
        }
        counters.learn(&obs, now.abs);
        left_open = Some(obs.position);
        if obs.closes {
            let cur = open.take().expect("a run to close");
            retire(cur, &mut owed, &mut out, &awake);
            left_open = None;
        }
    }
    if let Some(cur) = open {
        retire(cur, &mut owed, &mut out, &awake);
    }
    if let Some(run) =
        opened.and_then(|o| o.named_run(&mut pending, named.as_ref(), &counters, &awake))
    {
        retire(run, &mut owed, &mut out, &awake);
    }
    out.extend(owed.into_iter().map(|run| run.finish(&awake)));
    out.retain(|s| s.seconds > 0);
    out
}

/// A `Floor` `share_out` chains nothing from.
const FLAT: Floor = Floor {
    counter_ms: 0,
    words: None,
    step_ms: 1,
    stated: 0,
};

/// `run` into `owed` while `Open::owing`, else finished into `out`.
fn retire(run: Open, owed: &mut Vec<Open>, out: &mut Vec<Session>, awake: &Awake) {
    match run.owing() {
        true => owed.push(run),
        false => out.push(run.finish(awake)),
    }
}

/// Seconds awake across the `owed` runs of `book` past `stated`.
fn owed_awake(owed: &[Open], book: i64, stated: i64, awake: &Awake) -> i64 {
    owed.iter()
        .filter(|run| run.end_position == book)
        .map(|run| awake.bound(run.began.abs.max(stated), run.end().abs))
        .sum()
}

/// The `owed` runs of `book`, `gain` shared over them by `owed_awake` from
/// `floor`, finished into `out`; `words` is the same for `TotalWords`.
fn share_out(
    owed: &mut Vec<Open>,
    out: &mut Vec<Session>,
    book: i64,
    floor: &Floor,
    gain: i64,
    words: Option<i64>,
    awake: &Awake,
) {
    let (runs, rest): (Vec<Open>, Vec<Open>) = std::mem::take(owed)
        .into_iter()
        .partition(|run| run.end_position == book);
    *owed = rest;
    let weights: Vec<i64> = runs
        .iter()
        .map(|run| awake.bound(run.began.abs.max(floor.stated), run.end().abs))
        .collect();
    let total: i64 = weights.iter().sum();
    let part = |whole: i64, given: i64, at: usize, weight: i64| match (at + 1 == runs.len(), total)
    {
        (true, _) => whole - given,
        (false, 0) => 0,
        (false, total) => whole * weight / total,
    };
    let (mut counter, mut given) = (floor.counter_ms, 0);
    let (mut at_words, mut words_given) = (floor.words, 0);
    let shares: Vec<(i64, Option<i64>)> = weights
        .iter()
        .enumerate()
        .map(|(at, weight)| {
            let share = part(gain, given, at, *weight);
            given += share;
            let words_share = words.map(|w| {
                let share = part(w, words_given, at, *weight);
                words_given += share;
                share
            });
            (share, words_share)
        })
        .collect();
    for (mut run, (share, words_share)) in runs.into_iter().zip(shares) {
        run.top_up(counter, share, at_words.zip(words_share), awake);
        counter += share;
        at_words = at_words.zip(words_share).map(|(at, share)| at + share);
        out.push(run.finish(awake));
    }
}

/// A book's percentage read, off the reading-timer lines. `%Left` is the
/// fraction ahead, on the payload naming the book; read only off a line whose
/// book `end_position` names.
fn percent_left(line: &str, book: i64) -> Option<f64> {
    payloads(line).map(last_event).find_map(|p| {
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

/// The seconds left in `book`, off the payload naming it.
/// [`line::time_left`] reads the book's own figure past the chapter's.
fn time_left(line: &str, book: i64) -> Option<i64> {
    payloads(line)
        .map(last_event)
        .find_map(|p| match end_position(p) {
            Some(at) if at == book => line::time_left(p),
            _ => None,
        })
}

/// `TotalWPM` for `book`, off the payload naming it, in the whole words a
/// minute [`line::field`] reads ahead of its point.
fn stated_wpm(line: &str, book: i64) -> Option<i64> {
    payloads(line)
        .map(last_event)
        .find_map(|p| match end_position(p) {
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

    /// One `cvm` line at `hhmmss` naming `event`, standing `left` of the book,
    /// its `TotalTime` the clock's.
    fn placed(hhmmss: &str, event: &str, left: f64) -> String {
        let secs: i64 = hhmmss[0..2].parse::<i64>().unwrap() * 3600
            + hhmmss[2..4].parse::<i64>().unwrap() * 60
            + hhmmss[4..6].parse::<i64>().unwrap();
        format!(
            "260906:{hhmmss} cvm[6144]: I ReadingTimerController:Information::{event},\
             Verdict:Processed,PageStartPos:HTMLPosition:7731097,IntervalTime:785,\
             IntervalWords:12,TotalTime:{},TotalWords:1905,CurrentPos:HTMLPosition:7731097,\
             EndPos:HTMLPosition:19886489,PosLeft:12155392,%Left:{left};",
            secs * 1000,
        )
    }

    fn covered(lines: &[String]) -> Vec<(i64, i64)> {
        let refs: Vec<&str> = lines.iter().map(String::as_str).collect();
        let out = parse_sessions(refs, &[]);
        assert_eq!(out.len(), 1, "one sitting");
        out[0]
            .stretches
            .iter()
            .map(|r| {
                (
                    (r.from * 100.0).round() as i64,
                    (r.to * 100.0).round() as i64,
                )
            })
            .collect()
    }

    /// 0 to 10, back to 5 and on to 8, then 10 to 12: three runs, and no run
    /// spanning 8 to 10.
    #[test]
    fn a_jump_cuts_the_run_of_the_book_and_a_page_turn_opens_one() {
        assert_eq!(
            covered(&[
                placed("100000", "NextPage", 1.0),
                placed("100100", "NextPage", 0.9),
                placed("100200", "GoToPosition", 0.95),
                placed("100300", "NextPage", 0.92),
                placed("100400", "GoToPosition", 0.9),
                placed("100500", "NextPage", 0.88),
            ]),
            [(0, 10), (5, 8), (10, 12)],
        );
        // `NextPage` turns make one stretch however far they reach.
        assert_eq!(
            covered(&[
                placed("100000", "NextPage", 1.0),
                placed("100100", "NextPage", 0.9),
                placed("100200", "PreviousPage", 0.93),
                placed("100300", "NextPage", 0.8),
            ]),
            [(0, 20)],
        );
    }

    /// `GoToPosition` twice running, and `TapOnFooter` on the way, cover no ground.
    #[test]
    fn a_run_no_page_was_turned_in_is_not_a_run_of_the_book() {
        assert_eq!(
            covered(&[
                placed("100000", "NextPage", 1.0),
                placed("100100", "NextPage", 0.9),
                placed("100200", "TapOnFooter", 0.9),
                placed("100300", "GoToPosition", 0.5),
                placed("100400", "GoToPosition", 0.3),
                placed("100500", "NextPage", 0.28),
            ]),
            [(0, 10), (70, 72)],
        );
        // `GoToPosition` and `TapOnFooter` alone state no run.
        assert!(
            covered(&[
                placed("100000", "GoToPosition", 0.5),
                placed("100100", "TapOnFooter", 0.4),
            ])
            .is_empty()
        );
    }

    /// A [`Stretch`] states the run it covered, backwards included.
    #[test]
    fn a_run_read_backwards_states_the_ground_it_covered() {
        assert_eq!(
            covered(&[
                placed("100000", "NextPage", 0.2),
                placed("100100", "PreviousPage", 0.25),
            ]),
            [(80, 75)],
        );
    }

    #[test]
    fn a_mobi8_run_is_measured_the_way_a_kfx_one_is() {
        let out = parse_sessions(MOBI8, &[]);
        assert_eq!(out.len(), 1);
        // `vouch` seeds 329000 ms; 351002 stands at the last turn.
        assert_eq!(out[0].seconds, 22);
        assert_eq!(out[0].end_position, 19_886_489);
        assert_eq!(out[0].page_turns, 2);
        // 2077 - 1893, 1893 being 1905 less the 12 words of the first page.
        assert_eq!(out[0].words, 184);
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
    /// thousands-separated, with no word count beside it.
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

    /// `start_position` repeated is a redraw, its words those the record before
    /// it counted.
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
        // Three closed intervals of 30 s, the one from 101630 left open.
        assert_eq!(out[0].paged_seconds, 90);
        // 300 once for the screen redrawn at 101530, then 250.
        assert_eq!(out[0].paged_words, 550, "a redraw counted its words twice");
    }

    /// A page record stating the span it covers.
    fn spanned_page(hhmmss: &str, words: i64, start: i64, end: i64) -> String {
        format!(
            "260807:{hhmmss} fastmetrics[9842]: D fastmetrics: Emitting a new record. \
             SchemaName[ereader_book_consume_content], Fields[{{ \"start_position\" : {start}, \
             \"end_position\" : {end}, \"words_count\" : {words} }} ]. :"
        )
    }

    /// `covered` is the share of the book the pages crediting words cover,
    /// against the run's own `end_position`.
    #[test]
    fn the_pages_that_count_carry_the_share_of_the_book() {
        let lines = [
            page("101500", 7_390_020),
            spanned_page("101500", 300, 1000, 1301),
            spanned_page("101530", 250, 1301, 1552),
            // A jump, landing on a page 251 wide like the rest.
            spanned_page("101600", 100, 9000, 9251),
            spanned_page("101630", 120, 9251, 9500),
        ];
        let out = parse_sessions(lines.iter().map(String::as_str), &[]);
        // Three closed pages, each its own span over the book's own 148207.
        let want = (301.0 + 251.0 + 251.0) / 148_207.0;
        assert!(
            (out[0].covered - want).abs() < 1e-12,
            "covered {} against {want}",
            out[0].covered
        );
        // The 7448 positions skipped between 1552 and 9000 are no part of it.
        let gapped = (301.0 + 251.0 + 7_448.0) / 148_207.0;
        assert!(
            (out[0].covered - gapped).abs() > 1e-9,
            "a jump between two pages counted as reading"
        );
    }

    /// `worded_page` names no `start_position`: `share_of` reads -1 for both
    /// places, and `covered` stays zero.
    #[test]
    fn pages_stating_no_place_carry_no_share() {
        let lines = [
            page("101500", 7_390_020),
            worded_page("101500", 300),
            worded_page("101530", 300),
            worded_page("101600", 250),
        ];
        let out = parse_sessions(lines.iter().map(String::as_str), &[]);
        assert!(out[0].paged_words > 0, "the pages counted their words");
        assert_eq!(out[0].covered, 0.0);
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
        // 105010 to 105110 and 105110 to 105210 close; 105210 closes none.
        assert_eq!(out[0].words, 600, "the pages' own counts went unread");
        assert_ne!(out[0].measure, Measure::Counted);
    }

    #[test]
    fn a_fixed_layout_run_states_no_words_and_none_are_invented() {
        // `wordless_page` states zero words, as an image-only book does.
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
        // 105010 stands 100 s and counts; 105150 stands 1 s and does not.
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
        // `ACTIVE` 10:50:00-10:52:00 and 11:10:00-11:15:00, `hours` split in proportion.
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

    /// A turn line stating `words` on a page that stood `ms`, carrying a
    /// `SkipAvgReason` where `refused`.
    fn turn(ms: i64, words: i64, refused: bool) -> Observation {
        Observation {
            position: 148_207,
            total_ms: Some(7_390_020),
            words: Some(49_583),
            interval_ms: Some(ms),
            interval_words: Some(words),
            refused,
            page_turn: true,
            closes: false,
            jumped: false,
        }
    }

    /// A counted page: `(ms, words)` as a `Credit`.
    fn counted(ms: i64, words: i64) -> Credit {
        Credit {
            counted_ms: ms,
            counted_words: words,
            refused_ms: 0,
        }
    }

    /// A `Credit` refusing `ms` for rate, its words counted nowhere.
    fn refused(ms: i64) -> Credit {
        Credit {
            refused_ms: ms,
            ..Credit::default()
        }
    }

    #[test]
    fn a_page_the_device_counted_counts_the_whole_of_its_interval() {
        // 3 words in 4 s and one page standing an hour both count whole.
        assert_eq!(
            credits(&turn(3_600_000, 900, false)),
            counted(3_600_000, 900)
        );
        assert_eq!(credits(&turn(4_000, 3, false)), counted(4_000, 3));
    }

    /// 346 Han characters in 8.85 s reads 2,345 a minute, and
    /// [`Measure::Timed`] takes it whole.
    #[test]
    fn a_page_refused_for_reading_too_fast_counts_the_whole_of_its_interval() {
        assert_eq!(credits(&turn(8_850, 346, true)), refused(8_850));
        assert_eq!(credits(&turn(100, 400, true)), refused(100));
    }

    #[test]
    fn a_page_refused_for_standing_too_long_counts_nothing() {
        // 3 words in 10 minutes is 0.5 a minute, under [`RATE_MIN`].
        assert_eq!(credits(&turn(600_000, 3, true)), Credit::default());
        assert_eq!(credits(&turn(600_000, 0, true)), Credit::default());
        // 40 words in 60 s is [`RATE_MIN`] exactly, and counts.
        assert_eq!(credits(&turn(60_000, 40, true)), refused(60_000));
    }

    /// `PreviousPage` and `CloseBook` carry `SkipAvgReason` whatever their rate,
    /// and `credits` reads the rate.
    #[test]
    fn a_page_refused_on_the_event_alone_counts_on_its_rate() {
        // 346 words in 8.85 s, and 200 in 60 s: both reach [`RATE_MIN`].
        assert_eq!(credits(&turn(8_850, 346, true)), refused(8_850));
        assert_eq!(credits(&turn(60_000, 200, true)), refused(60_000));
        // 300 words over 888 s is 20 a minute: a book left open at a close.
        let mut idle = turn(888_000, 300, true);
        idle.closes = true;
        assert_eq!(credits(&idle), Credit::default());
    }

    /// `CloseBook` states an interval `TotalTime` never takes, reason or none.
    #[test]
    fn a_close_stating_no_reason_counts_on_its_rate_too() {
        let mut close = turn(91_293, 187, false);
        close.closes = true;
        assert_eq!(credits(&close), refused(91_293));
    }

    /// `untimed` states `IntervalTime` and no `TotalTime`, as an untimed book's line does.
    #[test]
    fn a_page_of_a_book_the_timer_never_counted_counts_nothing() {
        let mut untimed = turn(2_141, 0, false);
        untimed.total_ms = None;
        untimed.words = None;
        assert_eq!(credits(&untimed), Credit::default());
    }

    #[test]
    fn a_page_between_the_floor_and_the_cap_counts_whole() {
        assert_eq!(paged_secs(3), 3);
        assert_eq!(paged_secs(55), 55);
        assert_eq!(paged_secs(120), 120);
    }

    #[test]
    fn a_page_standing_past_the_cap_counts_the_cap() {
        assert_eq!(paged_secs(600), 120);
        assert_eq!(paged_secs(86_400), 120);
    }

    #[test]
    fn a_page_swiped_past_counts_nothing() {
        assert_eq!(paged_secs(2), 0);
        assert_eq!(paged_secs(0), 0);
    }

    /// A turn on `yymmdd` at `hhmmss`, its page having stood `interval_ms` to
    /// bring the book's counter to `total_ms`.
    fn dated_page(yymmdd: &str, hhmmss: &str, total_ms: i64, interval_ms: i64) -> String {
        format!(
            "{yymmdd}:{hhmmss} cvm[6144]: I ReadingTimerController:Information::NextPage,\
             Verdict:Processed,PageStartPos:YJPosition: AfQJAAAAAAAA:54205,\
             IntervalTime:{interval_ms},IntervalWords:320,TotalTime:{total_ms},TotalWords:49583,\
             CurrentPos:YJPosition: AfQJAAAAAAAA:54205,\
             EndPos:YJPosition: AbcVAAAPAAAA:148207,PosLeft:94002,%Left:0.645;"
        )
    }

    /// `seconds` counts the page that brought the run its first `TotalTime`.
    #[test]
    fn a_run_counts_the_page_that_brought_it_its_first_counter() {
        let lines = [
            open_book("101500"),
            // `position` states no counter, and `open_book` starts the run unvouched.
            position("101501"),
            page("101540", 7_390_020),
            page("101620", 7_429_910),
        ];
        let out = parse_sessions(lines.iter().map(String::as_str), &[]);
        assert_eq!(out.len(), 1);
        // 39.89 s for each page, both the run's.
        assert_eq!(out[0].seconds, 79);
        assert_eq!(out[0].timed_seconds, 79);
    }

    /// `seconds` and `timed_seconds` split a page open across midnight between
    /// the days in proportion.
    #[test]
    fn a_page_open_across_midnight_falls_half_in_each_day() {
        let lines = [
            dated_page("260807", "235900", 7_000_000, 20_000),
            dated_page("260807", "235940", 7_040_000, 40_000),
            dated_page("260808", "000020", 7_080_000, 40_000),
        ];
        let out = parse_sessions(lines.iter().map(String::as_str), &[]);
        assert_eq!(out.len(), 2);
        // 40 s stood wholly on the 7th, then 40 s split 20/20 over midnight.
        assert_eq!((out[0].seconds, out[0].timed_seconds), (60, 60));
        assert_eq!((out[1].seconds, out[1].timed_seconds), (20, 20));
        assert_eq!(out[0].hours, vec![(23, 60)]);
        assert_eq!(out[1].hours, vec![(0, 20)]);
    }

    /// `dated_page` stepping back four hours mid-run, a zone change, opens a
    /// second sitting booking no hour the first ran in.
    #[test]
    fn a_clock_stepping_back_mid_run_cuts_the_run() {
        let lines = [
            dated_page("260924", "104500", 7_000_000, 20_000),
            dated_page("260924", "104540", 7_040_000, 40_000),
            // +06:00 to +02:00 on landing: 10:46 becomes 06:46.
            dated_page("260924", "064620", 7_080_000, 40_000),
            dated_page("260924", "064700", 7_120_000, 40_000),
        ];
        let out = parse_sessions(lines.iter().map(String::as_str), &[]);
        assert_eq!(out.len(), 2, "one sitting either side of the step");
        assert_eq!(out[0].started_at, "2026-09-24T10:45:00");
        assert_eq!(out[0].ended_at, "2026-09-24T10:45:40");
        assert_eq!(out[0].hours, vec![(10, 40)]);
        assert_eq!(out[1].started_at, "2026-09-24T06:46:20");
        assert_eq!(
            out[1].hours,
            vec![(6, 40)],
            "and nothing in the hour it left"
        );
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

    /// A reading-event line at `hhmmss`: `Reading_Resumed,Reason:2` and the
    /// like.
    fn event(hhmmss: &str, event: &str) -> String {
        format!("260807:{hhmmss} cvm[6144]: I ReadingTimerController:Information::{event};")
    }

    /// A KPP flush at a sleep: the tail of a page event of chapter 300000,
    /// then the page event stating the counter beside book 289095.
    fn flushed(hhmmss: &str, total_ms: i64, words: i64) -> String {
        format!(
            "260807:{hhmmss} java[5878]: I ReadingTimerController:Information::\
             NewTimeLeft:60,NextTOCEntryPosition:YJPosition: Ac0DAAAAAAAA:300000,\
             NextTOCEntryLength:11,CurrentPos:YJPosition: AUFRAAAgAAAA:280637,\
             EndPos:YJPosition: Ac0DAAAAAAAA:300000,PosLeft:1584,\
             PageStartPos:YJPosition: AeUAAAAwAAAA:10671,IntervalTime:5920,IntervalWords:123,\
             SkipAvgReason:RTC_PrevPage,TotalTime:{total_ms},TotalWords:{words},\
             CurrentPos:YJPosition: AeUAAAAwAAAA:10671,EndPos:YJPosition: AbIWAADQAAAA:289095,\
             PosLeft:278424,%Left:0.9622;Reading_Interrupted,Reason:1;"
        )
    }

    /// [`kpp_close`] on the day the other lines here stand on.
    fn closed(hhmmss: &str, total_ms: i64, words: i64) -> String {
        kpp_close(hhmmss, total_ms, words).replacen("260906:", "260807:", 1)
    }

    /// A `CloseBook` of book 148207 at `hhmmss` with the counter at `total_ms`.
    fn close_at(hhmmss: &str, total_ms: i64) -> String {
        format!(
            "260807:{hhmmss} cvm[6144]: I ReadingTimerController:Information::CloseBook,\
             PageStartPos:YJPosition: AfQJAAAAAAAA:54205,IntervalTime:30000,IntervalWords:200,\
             TotalTime:{total_ms},TotalWords:49583,CurrentPos:YJPosition: AfQJAAAAAAAA:54205,\
             EndPos:YJPosition: AbcVAAAPAAAA:148207,PosLeft:94002,%Left:0.645;"
        )
    }

    /// A wake 34 minutes after the last page, a wake and sleep a minute long,
    /// then the first page, its `IntervalTime` from the second wake.
    #[test]
    fn a_page_turned_after_a_long_sleep_belongs_to_the_run_it_opens() {
        let lines = [
            dated_page("260807", "120312", 12_722_410, 38_255),
            dated_page("260807", "120356", 12_766_659, 44_249),
            event("120435", "Reading_Interrupted,Reason:1"),
            power("120435", "goingToScreenSaver"),
            power("120609", "suspending"),
            event("123842", "Reading_Resumed,Reason:2"),
            power("123842", "outOfScreenSaver"),
            event("123924", "Reading_Interrupted,Reason:1"),
            power("123924", "goingToScreenSaver"),
            event("124035", "Reading_Resumed,Reason:2"),
            power("124035", "outOfScreenSaver"),
            dated_page("260807", "124052", 12_783_344, 16_685),
            dated_page("260807", "124130", 12_821_344, 38_000),
        ];
        let out = parse_sessions(lines.iter().map(String::as_str), &[]);
        assert_eq!(out.len(), 2);
        assert_eq!(out[1].started_at, "2026-08-07T12:40:35");
        assert_eq!(
            out[1].start_counter_ms,
            Some(12_766_659),
            "the first page was dropped"
        );
        assert_eq!((out[1].seconds, out[1].timed_seconds), (54, 54));
    }

    /// A KPP book left open across an hour's sleep, read on and closed: `closed`
    /// is the stretch's only counted line, and the last `flushed` before the
    /// sleep states the counter three seconds of reading short of it.
    #[test]
    fn a_stretch_read_after_a_wake_opens_at_the_wake_on_the_counter_the_sleep_left() {
        let lines = [
            kpp_open("221813", "18,648").replacen("260906:", "260807:", 1),
            flushed("221823", 18_648_522, 166_639),
            flushed("223127", 18_963_538, 169_911),
            worded_page("223130", 200),
            event("223131", "Reading_Interrupted,Reason:1"),
            power("223131", "goingToScreenSaver"),
            power("223337", "suspending"),
            event("233443", "Reading_Resumed,Reason:2"),
            power("233443", "outOfScreenSaver"),
            closed("234505", 19_285_414, 172_147),
        ];
        let out = parse_sessions(lines.iter().map(String::as_str), &[]);
        assert_eq!(out.len(), 2, "the stretch after the wake was lost");
        assert_eq!(out[0].seconds, 317);
        assert_eq!(out[0].end_counter_ms, out[1].start_counter_ms);
        assert_eq!(out[1].started_at, "2026-08-07T23:34:43");
        // 321.876 s gained: 3 s awake before the sleep, 622 s after the wake.
        assert_eq!(
            out[1].start_counter_ms,
            Some(19_285_414 - 321_876 * 622 / 625)
        );
        assert_eq!(out[1].seconds, 320);
        assert_eq!(out[1].measure, Measure::Counted);
        // `healed`: the log read under `t` rows holding the counter it ends at.
        let healed = parse_sessions(
            lines.iter().map(String::as_str),
            &[(289_095, 19_285_414, 172_147)],
        );
        assert_eq!(healed, out);
    }

    /// A wake, a page record, then eleven hours of silence: a hole in the log,
    /// the wake starting no run.
    #[test]
    fn a_wake_the_log_falls_silent_after_starts_no_run() {
        let lines = [
            dated_page("260807", "080000", 7_390_020, 39_890),
            event("080030", "Reading_Interrupted,Reason:1"),
            power("080030", "goingToScreenSaver"),
            event("081000", "Reading_Resumed,Reason:2"),
            power("081000", "outOfScreenSaver"),
            worded_page("081001", 300),
            // Eleven hours on, the counter five thousand seconds on.
            dated_page("260807", "190000", 12_390_020, 39_890),
            dated_page("260807", "190040", 12_430_020, 40_000),
        ];
        let out = parse_sessions(lines.iter().map(String::as_str), &[]);
        assert!(
            out.iter().all(|s| s.seconds < 100),
            "the hole was read as a sitting: {out:?}"
        );
        assert!(out.iter().all(|s| s.started_at != "2026-08-07T08:10:00"));
    }

    /// No page record between a wake and the sleep after it, `flushed` counting
    /// the run from the wake however long the silence.
    #[test]
    fn a_silent_stretch_the_counter_brackets_is_read_from_its_wake() {
        let lines = [
            flushed("120144", 30_470_613, 170_000),
            power("120144", "goingToScreenSaver"),
            power("144014", "outOfScreenSaver"),
            event("144015", "Reading_Resumed,Reason:2"),
            flushed("151739", 32_412_005, 180_000),
            power("151739", "goingToScreenSaver"),
        ];
        let out = parse_sessions(lines.iter().map(String::as_str), &[]);
        assert_eq!(out.len(), 1);
        assert_eq!(out[0].started_at, "2026-08-07T14:40:15");
        assert_eq!(out[0].start_counter_ms, Some(30_470_613));
        assert_eq!(out[0].seconds, 1_941);
    }

    /// `open_book`, then `position` stating no counter: `close_at` settles the
    /// run on the counter `open_book` restates.
    #[test]
    fn an_open_its_first_line_cannot_vouch_for_still_floors_the_run() {
        let lines = [
            open_book("180404"),
            position("180406"),
            close_at("182833", 10_201),
        ];
        let out = parse_sessions(lines.iter().map(String::as_str), &[]);
        assert_eq!(out.len(), 1);
        assert_eq!(out[0].start_counter_ms, Some(0));
        assert_eq!(out[0].seconds, 10);
    }

    /// `StoredBookData` states whole seconds: `dated_page`, under one past it,
    /// floors the run a page under itself.
    #[test]
    fn a_counter_stated_in_whole_seconds_gains_nothing_under_one() {
        let lines = [
            "260807:101500 java[1]: I ReadingTimerController:Information::OpenBook,\
             StoredBookData:TimeRead:5,421 sec. WPM:0. Version:0,Title:<private>;"
                .to_string(),
            position("101501"),
            dated_page("260807", "101541", 5_421_525 + 39_890, 39_890),
        ];
        let out = parse_sessions(lines.iter().map(String::as_str), &[]);
        assert_eq!(out[0].start_counter_ms, Some(5_421_525));
        assert_eq!(out[0].seconds, 39);
    }

    /// Page records alone after a wake, the sleep's flush naming no book: pages
    /// of the book `flushed` left open.
    #[test]
    fn pages_read_after_a_wake_with_no_line_naming_a_book_are_the_book_left_open() {
        let lines = [
            flushed("100000", 6_066_717, 53_689),
            power("100000", "goingToScreenSaver"),
            power("120000", "outOfScreenSaver"),
            event("120000", "Reading_Resumed,Reason:2"),
            worded_page("120010", 300),
            worded_page("120100", 300),
            worded_page("120200", 300),
            event("120500", "Reading_Interrupted,Reason:1"),
            power("120500", "goingToScreenSaver"),
        ];
        let out = parse_sessions(lines.iter().map(String::as_str), &[]);
        assert_eq!(out.len(), 1, "the pages after the wake were lost");
        assert_eq!(out[0].end_position, 289_095);
        assert_eq!(out[0].started_at, "2026-08-07T12:00:00");
        assert_eq!(out[0].ended_at, "2026-08-07T12:02:00");
        assert_eq!(
            (out[0].measure, out[0].paged_seconds),
            (Measure::Paged, 110)
        );
    }

    /// `record`s past midnight cut the run they carry at midnight.
    #[test]
    fn a_run_its_records_carry_past_midnight_is_cut_there() {
        let record = |day: &str, hhmmss: &str| worded_page(hhmmss, 300).replacen("260807:", day, 1);
        let lines = [
            flushed("230000", 6_066_717, 53_689),
            power("230000", "goingToScreenSaver"),
            power("235000", "outOfScreenSaver"),
            event("235000", "Reading_Resumed,Reason:2"),
            record("260807:", "235010"),
            record("260807:", "235500"),
            record("260808:", "000100"),
            record("260808:", "000500"),
        ];
        let out = parse_sessions(lines.iter().map(String::as_str), &[]);
        let spans: Vec<(&str, &str)> = out
            .iter()
            .map(|s| (s.started_at.as_str(), s.ended_at.as_str()))
            .collect();
        assert_eq!(
            spans,
            [
                ("2026-08-07T23:50:00", "2026-08-07T23:59:59"),
                ("2026-08-08T00:00:00", "2026-08-08T00:05:00"),
            ]
        );
    }

    /// The book `HTMLPosition` 19886489 turned a page at `hhmmss`.
    fn other(hhmmss: &str, total_ms: i64) -> String {
        format!(
            "260807:{hhmmss} cvm[6144]: I ReadingTimerController:Information::NextPage,\
             Verdict:Processed,PageStartPos:HTMLPosition:7731097,IntervalTime:21217,\
             IntervalWords:172,TotalTime:{total_ms},TotalWords:2077,\
             CurrentPos:HTMLPosition:7731097,EndPos:HTMLPosition:19886489,PosLeft:12154764,\
             %Left:0.6111;"
        )
    }

    /// `worded_page`s after `open_book` wait for the line naming the opened book.
    #[test]
    fn records_after_an_open_wait_for_the_line_naming_its_book() {
        let lines = [
            dated_page("260807", "100000", 7_390_020, 39_890),
            worded_page("100010", 300),
            open_book("100500"),
            worded_page("100510", 300),
            worded_page("100610", 300),
            other("100700", 60_000),
        ];
        let out = parse_sessions(lines.iter().map(String::as_str), &[]);
        let opened = out
            .iter()
            .find(|s| s.end_position == 19_886_489)
            .expect("the opened book's run");
        assert_eq!(opened.started_at, "2026-08-07T10:05:00");
        assert_eq!(opened.paged_seconds, 60);
        assert!(
            out.iter()
                .all(|s| s.end_position != 148_207 || s.paged_seconds == 0)
        );

        // `again`: the same book opened, its run going on and taking them.
        let again = [
            dated_page("260807", "100000", 7_390_020, 39_890),
            worded_page("100010", 300),
            "260807:100500 java[1]: I ReadingTimerController:Information::OpenBook,\
             StoredBookData:TimeRead:7,390 sec. WPM:0. Version:0,Title:<private>;"
                .to_string(),
            worded_page("100510", 300),
            worded_page("100610", 300),
            dated_page("260807", "100700", 7_430_020, 40_000),
        ];
        let out = parse_sessions(again.iter().map(String::as_str), &[]);
        assert_eq!(out.len(), 1);
        // 10:00:10 to 10:05:10 at the cap, then a minute.
        assert_eq!(out[0].paged_seconds, 180);
    }

    /// A pass re-reading a sitting from its wake, the counter the sleep left
    /// stated before the batch: `held` opens the run where it opened.
    #[test]
    fn a_pass_reading_from_the_wake_opens_the_run_where_the_record_did() {
        let lines = [
            event("233443", "Reading_Resumed,Reason:2"),
            power("233443", "outOfScreenSaver"),
            closed("234505", 19_285_414, 172_147),
        ];
        let held = [Session {
            started_at: "2026-08-07T23:34:43".into(),
            ended_at: "2026-08-07T23:45:05".into(),
            end_position: 289_095,
            start_counter_ms: Some(18_965_084),
            start_words: Some(169_950),
            ..Session::default()
        }];
        let out = parse_restating(lines.iter().map(String::as_str), &[], &held);
        assert_eq!(out.len(), 1);
        assert_eq!(out[0].start_counter_ms, Some(18_965_084));
        assert_eq!(out[0].seconds, 320);
        // `alone`: no `held` row, no counter at the wake.
        let alone = parse_sessions(lines.iter().map(String::as_str), &[]);
        assert_eq!(alone[0].measure, Measure::Timed);
    }

    /// `kpp_open` on the day the other lines here stand on.
    fn opened(hhmmss: &str, time_read: &str) -> String {
        kpp_open(hhmmss, time_read).replacen("260906:", "260807:", 1)
    }

    /// A KPP book reopened, its close a flush naming nothing: `TimeRead` names
    /// the run's book, and the next `opened` counts it.
    #[test]
    fn pages_after_an_open_no_line_names_are_the_book_its_counter_names() {
        let lines = [
            closed("221440", 589_913, 1_322),
            opened("222110", "589"),
            worded_page("222112", 300),
            worded_page("222544", 300),
            worded_page("222822", 300),
            worded_page("223127", 300),
            event("223135", "OldTimeLeft:1156"),
            opened("234000", "1,003"),
            closed("234020", 1_003_773, 1_834),
        ];
        let out = parse_sessions(lines.iter().map(String::as_str), &[]);
        let run = &out[0];
        assert_eq!(
            (run.started_at.as_str(), run.ended_at.as_str()),
            ("2026-08-07T22:21:10", "2026-08-07T22:31:27")
        );
        assert_eq!(run.end_position, 289_095);
        assert_eq!(run.paged_seconds, 360);
        assert_eq!(
            (run.start_counter_ms, run.end_counter_ms),
            (Some(589_913), Some(1_003_000))
        );
        assert_eq!((run.seconds, run.measure), (413, Measure::Counted));

        // `early` ends at the flush, its run `Measure::Paged`.
        let early = parse_sessions(lines[..7].iter().map(String::as_str), &[]);
        assert_eq!(early.len(), 1);
        assert_eq!((early[0].seconds, early[0].measure), (360, Measure::Paged));

        // `TimeRead` 590 names no book.
        let unnamed = lines[..7]
            .iter()
            .map(|line| line.replace(":589 sec", ":590 sec"));
        let unnamed: Vec<String> = unnamed.collect();
        assert!(parse_sessions(unnamed.iter().map(String::as_str), &[]).is_empty());
    }

    /// `Reading_Resumed` at 10:00:00, the page record at 10:01:00, under
    /// three `powerd` streams.
    #[test]
    fn a_resume_stands_where_powerd_woke_at_it() {
        let woke = Woke {
            at: stamp(&event("100000", "Reading_Resumed,Reason:2")).unwrap(),
            read: false,
            lapsed: false,
        };
        let now = stamp(&worded_page("100100", 300)).unwrap();
        let stands = |sleep: (&str, &str)| {
            let lines = [
                power(sleep.0, "goingToScreenSaver"),
                power(sleep.1, "outOfScreenSaver"),
            ];
            woke.stands(&now, &Awake::from_events(lines.iter().map(String::as_str)))
        };
        // `ACTIVE` [`WAKE_LAG_SECS`] or less past the resume.
        assert!(stands(("090000", "100001")));
        assert!(!stands(("090000", "100030")), "a resume asleep stood");
        assert!(!stands(("100030", "100050")), "a sleep after it stood");
    }

    /// A wake, then pages only past a silence over [`SESSION_GAP_SECS`]
    /// awake, up to the flush counting the stretch.
    #[test]
    fn pages_a_silence_after_their_wake_open_the_run_at_it() {
        let lines = [
            flushed("070000", 9_105_240, 50_000),
            power("070000", "goingToScreenSaver"),
            power("081359", "outOfScreenSaver"),
            event("081359", "Reading_Resumed,Reason:2"),
            worded_page("090000", 300),
            worded_page("091000", 300),
            worded_page("092000", 300),
            worded_page("093000", 300),
            flushed("093339", 12_038_162, 60_000),
            power("093339", "goingToScreenSaver"),
        ];
        let out = parse_sessions(lines.iter().map(String::as_str), &[]);
        assert_eq!(out.len(), 1);
        assert_eq!(out[0].started_at, "2026-08-07T08:13:59");
        assert_eq!(out[0].start_counter_ms, Some(9_105_240));
        assert_eq!((out[0].seconds, out[0].measure), (2_932, Measure::Counted));
    }

    #[test]
    fn remeasure_takes_the_earlier_start_a_fuller_pass_states() {
        let mut held = Session {
            started_at: "2026-08-07T10:40:40".into(),
            ended_at: "2026-08-07T10:41:20".into(),
            ..Session::default()
        };
        let fresh = Session {
            started_at: "2026-08-07T10:40:00".into(),
            ..held.clone()
        };
        assert!(held.remeasure(&fresh));
        assert_eq!(held.started_at, "2026-08-07T10:40:00");
        let later = Session {
            started_at: "2026-08-07T10:40:50".into(),
            ..fresh
        };
        held.remeasure(&later);
        assert_eq!(
            held.started_at, "2026-08-07T10:40:00",
            "a later start replaced an earlier one"
        );
    }
}
