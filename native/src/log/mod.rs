//! Reading sittings out of the Kindle's own system log: the
//! `ReadingTimerController` lines, the `fastmetrics` reader-shell records, and
//! `powerd`'s state changes. [`session::parse_sessions`] reads all three.

pub mod line;
pub mod metric;
pub mod power;
pub mod session;
pub mod source;

pub use metric::METRIC_MARKERS;
pub use power::POWER_MARKERS;
pub use session::parse_sessions;

/// Every tag worth keeping a line for; the prefilter ahead of everything else.
/// One device-day is some 76 KB of these, against a syslog two orders of
/// magnitude larger.
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
