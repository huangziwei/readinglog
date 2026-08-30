//! Reading sittings out of the Kindle's own system log.
//!
//! Three families of line carry them, and a device writes some or all of three:
//!
//! - `ReadingTimerController` — the reading timer's own accounting. Counts
//!   words and a WPM, and times only a book it can count words in.
//! - the `fastmetrics` reader-shell records — a page and its word count, a
//!   turn, an open and a close, written for every book including the ones the
//!   timer refuses.
//! - `powerd`'s state changes — whether the device was awake, the last bound
//!   available on a sitting neither of the above measured.
//!
//! [`session::parse_sessions`] reads all three at once and reports which one
//! answered, as [`session::Measure`].

pub mod line;
pub mod metric;
pub mod power;
pub mod session;
pub mod source;

pub use metric::METRIC_MARKERS;
pub use power::POWER_MARKERS;
pub use session::parse_sessions;

/// Every tag worth keeping a line for; the prefilter ahead of everything else.
///
/// Measured against one device-day: 186 reader-shell records at 45 KB beside 43
/// reading-timer lines at 31 KB, and about a hundred power lines. The whole
/// syslog for that day is two orders of magnitude larger.
pub const MARKERS: [&str; 13] = {
    let (m, p) = (METRIC_MARKERS, POWER_MARKERS);
    [
        line::TIMER_MARKER,
        m[0],
        m[1],
        m[2],
        m[3],
        m[4],
        m[5],
        m[6],
        m[7],
        p[0],
        p[1],
        p[2],
        p[3],
    ]
};
