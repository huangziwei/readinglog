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

/// Two substrings standing in for whole groups of [`MARKERS`]. With
/// [`line::TIMER_MARKER`] they cover all thirteen, which
/// `every_marker_holds_a_probe` pins.
const EREADER: &str = "ereader_";
const LIPC: &str = "lipc:evts:name=";

/// Which family of marker a line carries, and so what is worth reading off it.
/// The three groups come from three processes and no line of a real log
/// carries two.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Family {
    /// `ReadingTimerController`, off `cvm`: the reading counter, the book's
    /// positions, the chapter boundaries. Every payload reader wants one of
    /// these and nothing else.
    Timer,
    /// `fastmetrics`' `SchemaName[ereader_…]` records: the page and its words,
    /// the turn, and on the stacks that state it the book's `cde_key`.
    Metric,
    /// `powerd`'s state changes, stated as a metrics record or as the LIPC
    /// event that accompanies it.
    Power,
}

/// The family `line` carries, `None` for a line carrying no marker. Three
/// probes stand ahead of the exact thirteen, which matters: this runs on every
/// line of every log file a launch opens.
pub fn family(line: &str) -> Option<Family> {
    if line.contains(line::TIMER_MARKER) {
        return Some(Family::Timer);
    }
    if line.contains(EREADER) {
        if METRIC_MARKERS.iter().any(|m| line.contains(m)) {
            return Some(Family::Metric);
        }
        return line.contains(POWER_MARKERS[0]).then_some(Family::Power);
    }
    if line.contains(LIPC) && POWER_MARKERS[1..].iter().any(|m| line.contains(m)) {
        return Some(Family::Power);
    }
    None
}

/// Whether `line` is a reading-timer line, in one search. Sound only over a
/// stream [`family`] has already filtered, where there is no lookalike left to
/// refuse and every other reader tests its own markers.
pub fn is_timer(line: &str) -> bool {
    line.contains(line::TIMER_MARKER)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The probes are only sound while every marker holds one of them: a
    /// fourteenth marker that holds none would be read out of no log at all.
    #[test]
    fn every_marker_holds_a_probe() {
        for marker in MARKERS {
            assert!(
                marker.contains(line::TIMER_MARKER)
                    || marker.contains(EREADER)
                    || marker.contains(LIPC),
                "{marker} holds none of the three probes"
            );
            assert!(family(marker).is_some(), "{marker} names no family");
        }
    }

    #[test]
    fn a_line_is_read_as_the_process_that_wrote_it() {
        assert_eq!(
            family("260807:101501 cvm[6144]: I ReadingTimerController:Information::NextPage,"),
            Some(Family::Timer)
        );
        assert_eq!(
            family(
                "260807:101501 fastmetrics[1]: D Emitting. SchemaName[ereader_book_page_turn], Fields[{"
            ),
            Some(Family::Metric)
        );
        // `powerd`'s own record holds `ereader_` and is not a metric record.
        assert_eq!(
            family("260807:101501 powerd[1]: I ereader_powerd_state_change curr_state"),
            Some(Family::Power)
        );
        assert_eq!(
            family(
                "260807:101501 powerd[1]: I lipc:evts:name=suspending, origin=com.lab126.powerd"
            ),
            Some(Family::Power)
        );
    }

    #[test]
    fn a_line_holding_a_probe_and_no_marker_is_no_marker() {
        // Both probes, neither marker.
        assert_eq!(family("260807:101501 x[1]: I ereader_something_else"), None);
        assert_eq!(
            family(
                "260807:101501 x[1]: I lipc:evts:name=battLevelChanged, origin=com.lab126.powerd"
            ),
            None
        );
        assert_eq!(family("260807:101502 kernel: I mmc0: unrelated"), None);
    }

    /// [`family`] and [`MARKERS`] have to name the same lines. A line one
    /// keeps and the other refuses is a sitting that moves under a reader.
    #[test]
    fn the_probes_keep_exactly_what_the_thirteen_kept() {
        for line in [
            "260807:101501 cvm[6144]: I ReadingTimerController:Information::NextPage,EndPos:1;",
            "260807:101501 fastmetrics[1]: SchemaName[ereader_open_book], Fields[{",
            "260807:101501 fastmetrics[1]: SchemaName[ereader_reader_page_turn_latency_ops], F",
            "260807:101501 powerd[1]: ereader_powerd_state_change",
            "260807:101501 powerd[1]: lipc:evts:name=outOfScreenSaver, origin=com.lab126.powerd",
            "260807:101501 powerd[1]: lipc:evts:name=goingToScreenSaver, origin=com.lab126.powerd",
            "260807:101502 kernel: I mmc0: unrelated",
            "260807:101502 cvm[1]: I something with ereader_ in it",
            "",
        ] {
            assert_eq!(
                family(line).is_some(),
                MARKERS.iter().any(|m| line.contains(m)),
                "{line}"
            );
        }
    }
}
