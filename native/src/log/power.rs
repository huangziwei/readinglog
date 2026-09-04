//! The stretches `powerd` recorded as `ACTIVE`, in absolute seconds, out of
//! its state machine — `ACTIVE`, `SCREEN SAVER`, `READY TO SUSPEND`,
//! `SUSPENDED`, `HIBERNATE`. [`Awake::between`] bounds an uncounted sitting.

use super::line::{field_text, stamp};

/// The tags on the lines `powerd` writes at a change of state.
/// `ereader_powerd_state_change` transcribes the machine whole;
/// `outOfScreenSaver`/`goingToScreenSaver` bracket `ACTIVE`.
pub const POWER_MARKERS: [&str; 4] = [
    "ereader_powerd_state_change",
    "lipc:evts:name=outOfScreenSaver, origin=com.lab126.powerd",
    "lipc:evts:name=goingToScreenSaver, origin=com.lab126.powerd",
    "lipc:evts:name=suspending, origin=com.lab126.powerd",
];

/// Which family of line stated a power change.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Family {
    /// `ereader_powerd_state_change`, naming both ends of the move.
    Record,
    /// The LIPC event `powerd` fires as it makes the same move.
    Event,
}

/// Whether a power change left the device awake.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Woke {
    Active,
    Idle,
}

/// Read a line as a change into or out of `ACTIVE`.
/// `ereader_powerd_state_change` states both ends of a move, read on both;
/// `outOfScreenSaver` opens a span, `goingToScreenSaver`/`suspending` close.
fn change(line: &str) -> Option<(Family, Woke)> {
    if line.contains(POWER_MARKERS[0]) {
        return match (
            field_text(line, "curr_state").is_some_and(|s| s == "ACTIVE"),
            field_text(line, "prev_state").is_some_and(|s| s == "ACTIVE"),
        ) {
            (true, _) => Some((Family::Record, Woke::Active)),
            (_, true) => Some((Family::Record, Woke::Idle)),
            _ => None,
        };
    }
    if line.contains(POWER_MARKERS[1]) {
        return Some((Family::Event, Woke::Active));
    }
    if POWER_MARKERS[2..].iter().any(|m| line.contains(m)) {
        return Some((Family::Event, Woke::Idle));
    }
    None
}

/// Whether a line carries one of [`POWER_MARKERS`].
pub fn is_state_change(line: &str) -> bool {
    POWER_MARKERS.iter().any(|marker| line.contains(marker))
}

#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct Awake {
    spans: Vec<(i64, i64)>,
}

impl Awake {
    /// Read the power lines out of an event stream. A span opens at
    /// `Woke::Active` and closes at the next `Woke::Idle`; one left open is
    /// dropped. One `Family` at a time, since both state the same move.
    pub fn from_events<'a>(events: impl IntoIterator<Item = &'a str>) -> Self {
        let changes: Vec<(Family, i64, Woke)> = events
            .into_iter()
            .filter_map(|line| {
                let (family, woke) = change(line)?;
                Some((family, stamp(line)?.abs, woke))
            })
            .collect();
        let family = match changes.iter().any(|(f, ..)| *f == Family::Record) {
            true => Family::Record,
            false => Family::Event,
        };

        let mut spans = Vec::new();
        let mut open: Option<i64> = None;
        for (_, at, woke) in changes.iter().filter(|(f, ..)| *f == family) {
            match (woke, open) {
                (Woke::Active, None) => open = Some(*at),
                (Woke::Idle, Some(from)) if *at > from => {
                    spans.push((from, *at));
                    open = None;
                }
                (Woke::Idle, Some(_)) => open = None,
                _ => {}
            }
        }
        spans.sort_unstable();
        Self { spans }
    }

    /// Drop the `ACTIVE` spans that no reading line falls in.
    ///
    /// `read_at` names the instants a reading line stands at, ascending.
    pub fn witnessed(self, read_at: &[i64]) -> Self {
        let spans = self
            .spans
            .into_iter()
            .filter(|(from, to)| {
                let next = read_at.partition_point(|at| at < from);
                read_at.get(next).is_some_and(|at| at <= to)
            })
            .collect();
        Self { spans }
    }

    /// Seconds of `ACTIVE` between two instants.
    pub fn between(&self, from: i64, to: i64) -> i64 {
        self.spans_between(from, to)
            .iter()
            .map(|(s, e)| e - s)
            .sum()
    }

    /// The `ACTIVE` stretches between two instants, clipped to them.
    pub fn spans_between(&self, from: i64, to: i64) -> Vec<(i64, i64)> {
        self.spans
            .iter()
            .map(|(s, e)| (from.max(*s), to.min(*e)))
            .filter(|(s, e)| e > s)
            .collect()
    }

    /// Whether [`Awake`] holds any span.
    pub fn is_empty(&self) -> bool {
        self.spans.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// `ereader_powerd_state_change` records: `ACTIVE` at 11:19:00,
    /// `SCREEN SAVER` at 11:35:00.
    const RECORDS: [&str; 2] = [
        "260814:111900.726 fastmetrics[9842]: D fastmetrics:KindleFastMetricsPublisher:[24109.944489]: Emitting a new record. SchemaName[ereader_powerd_state_change], Fields[{ \t\"curr_state\" : \"ACTIVE\", \t\"prev_state\" : \"SCREEN SAVER\" } ]. :",
        "260814:113500.549 fastmetrics[9842]: D fastmetrics:KindleFastMetricsPublisher:[26548.733985]: Emitting a new record. SchemaName[ereader_powerd_state_change], Fields[{ \t\"curr_state\" : \"SCREEN SAVER\", \t\"prev_state\" : \"ACTIVE\" } ]. :",
    ];

    /// The same two transitions as `powerd` LIPC events.
    const EVENTS: [&str; 2] = [
        "260814:111900 powerd[4213]: I lipc:evts:name=outOfScreenSaver, origin=com.lab126.powerd, fparam=2:Event sent",
        "260814:113500 powerd[4213]: I lipc:evts:name=goingToScreenSaver, origin=com.lab126.powerd, fparam=3:Event sent",
    ];

    /// No `outOfScreenSaver` between them.
    const UNATTENDED: [&str; 3] = [
        "260814:112500 powerd[4213]: I lipc:evts:name=wakeupFromSuspend, origin=com.lab126.powerd, fparam=154:Event sent",
        "260814:112500 powerd[4213]: I lipc:evts:name=resuming, origin=com.lab126.powerd:Event sent",
        "260814:112830 powerd[4213]: I lipc:evts:name=suspending, origin=com.lab126.powerd, fparam=mem:Event sent",
    ];

    fn at(clock: &str) -> i64 {
        stamp(&format!("{clock} x")).expect("a stamped line").abs
    }

    #[test]
    fn a_wake_no_reading_line_falls_in_is_dropped() {
        let awake = Awake::from_events(EVENTS);
        assert_eq!(awake.between(at("260814:111900"), at("260814:113500")), 960);
        // One instant, inside the span.
        let held = awake.clone().witnessed(&[at("260814:112000")]);
        assert_eq!(held.between(at("260814:111900"), at("260814:113500")), 960);
        // No instant inside it.
        let dropped = awake.witnessed(&[at("260814:120000")]);
        assert!(dropped.is_empty());
        assert_eq!(Awake::default().witnessed(&[at("260814:112000")]).spans, []);
    }

    #[test]
    fn a_record_pair_brackets_one_awake_span() {
        let a = Awake::from_events(RECORDS);
        assert!(!a.is_empty());
        assert_eq!(a.between(at("260814:000000"), at("260814:235959")), 16 * 60);
    }

    #[test]
    fn a_device_writing_only_lipc_events_is_still_bounded() {
        let a = Awake::from_events(EVENTS);
        assert_eq!(a.between(at("260814:000000"), at("260814:235959")), 16 * 60);
    }

    #[test]
    fn a_device_writing_both_families_counts_the_span_once() {
        let both: Vec<&str> = RECORDS.iter().chain(EVENTS.iter()).copied().collect();
        let a = Awake::from_events(both);
        assert_eq!(a.between(at("260814:000000"), at("260814:235959")), 16 * 60);
    }

    #[test]
    fn a_wake_the_reader_had_no_part_in_opens_no_span() {
        let a = Awake::from_events(UNATTENDED);
        assert!(a.is_empty());
    }

    #[test]
    fn a_span_still_open_at_the_end_of_the_batch_is_dropped() {
        let a = Awake::from_events([RECORDS[0]]);
        assert!(a.is_empty());
    }

    #[test]
    fn a_stream_with_no_power_line_at_all_is_empty() {
        assert!(Awake::from_events(["260814:111900 cvm[1]: I something else"]).is_empty());
    }
}
