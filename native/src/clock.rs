//! The offsets the device's clock has stood on, over time. A firmware keeps one
//! zone file and replaces it whole, so [`crate::zone`] answers only for today;
//! placing a true epoch on the device's clock needs the offset that stood then.

/// The furthest from UTC a device clock is ever set. Past this a difference
/// between two stamps is two different instants, not one instant in two
/// frames.
pub const BOUND_SECS: i64 = 14 * 3600;

/// Offsets are whole minutes. A Kindle set by hand sits at `+02:01`, so a
/// whole-*hour* test would refuse a real clock.
pub const STEP_SECS: i64 = 60;

pub fn plausible(offset: i64) -> bool {
    offset.abs() <= BOUND_SECS && offset % STEP_SECS == 0
}

/// The offsets the device has stood on, as `(the instant it was first seen
/// at, the offset)`. Ascending by instant, and never two rows running at one
/// offset.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct Clock {
    seen: Vec<(i64, i64)>,
}

impl Clock {
    pub fn rows(&self) -> &[(i64, i64)] {
        &self.seen
    }

    pub fn is_empty(&self) -> bool {
        self.seen.is_empty()
    }

    /// The offset standing at `epoch`, and `None` where nothing is observed.
    /// An instant under the first row takes that row's offset.
    pub fn offset_at(&self, epoch: i64) -> Option<i64> {
        let at = self.seen.partition_point(|(from, _)| *from <= epoch);
        match at {
            0 => self.seen.first().map(|(_, offset)| *offset),
            _ => self.seen.get(at - 1).map(|(_, offset)| *offset),
        }
    }

    /// The distinct offsets, in the order they were first stood on: what a join
    /// tries before it falls back to the zone file.
    pub fn offsets(&self) -> Vec<i64> {
        let mut out: Vec<i64> = Vec::new();
        for (_, offset) in &self.seen {
            if !out.contains(offset) {
                out.push(*offset);
            }
        }
        out
    }

    /// Fold `seen` in, sorted and collapsed so a run at one offset is one row,
    /// answering whether anything moved. Observations are only ever added: a
    /// pass that saw nothing is not the device changing zone.
    pub fn observe(&mut self, mut seen: Vec<(i64, i64)>) -> bool {
        seen.extend_from_slice(&self.seen);
        seen.retain(|(_, offset)| plausible(*offset));
        seen.sort_unstable();
        let mut held: Vec<(i64, i64)> = Vec::with_capacity(seen.len());
        for (epoch, offset) in seen {
            if held.last().is_some_and(|(_, last)| *last == offset) {
                continue;
            }
            held.push((epoch, offset));
        }
        let moved = self.seen != held;
        self.seen = held;
        moved
    }

    pub fn learn(&mut self, epoch: i64, offset: i64) -> bool {
        if !plausible(offset) || self.offset_at(epoch) == Some(offset) {
            return false;
        }
        self.observe(vec![(epoch, offset)])
    }
}

/// [`Clock::offset_at`], falling back to the zone file standing now where the
/// record has seen nothing — which is right for a device that has not moved.
pub fn offset_at(clock: &Clock, epoch: i64) -> Option<i64> {
    clock
        .offset_at(epoch)
        .or_else(|| crate::zone::offset_at(epoch))
}

/// An epoch second as the `YYYY-MM-DDTHH:MM:SS` the device would have written
/// for it, on the clock it stood on then. Empty where no day can be named.
pub fn stamp(clock: &Clock, epoch: i64) -> String {
    let placed = match offset_at(clock, epoch) {
        Some(offset) => crate::date::local_at(epoch, Some(offset)),
        None => crate::date::local_of(epoch),
    };
    match placed {
        Some((days, secs)) => crate::date::stamp(days, secs),
        None => String::new(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A fortnight at +6 between two stretches at +2.
    fn travelled() -> Clock {
        let mut clock = Clock::default();
        assert!(clock.observe(vec![
            (1_786_000_000, 7200),
            (1_786_100_000, 7200),
            (1_787_600_000, 21_600),
            (1_787_700_000, 21_600),
            (1_788_900_000, 7200),
        ]));
        clock
    }

    #[test]
    fn a_run_at_one_offset_is_one_row() {
        let clock = travelled();
        assert_eq!(
            clock.rows(),
            [
                (1_786_000_000, 7200),
                (1_787_600_000, 21_600),
                (1_788_900_000, 7200)
            ]
        );
        assert_eq!(clock.offsets(), vec![7200, 21_600]);
    }

    #[test]
    fn an_instant_takes_the_offset_standing_over_it() {
        let clock = travelled();
        assert_eq!(clock.offset_at(1_786_050_000), Some(7200));
        assert_eq!(
            clock.offset_at(1_787_600_000),
            Some(21_600),
            "the row's own instant"
        );
        assert_eq!(clock.offset_at(1_787_650_000), Some(21_600));
        assert_eq!(clock.offset_at(1_788_950_000), Some(7200));
        // Under the first row, the earliest clock the device is known to have
        // stood on rather than the one it stands on today.
        assert_eq!(clock.offset_at(1_700_000_000), Some(7200));
        assert_eq!(Clock::default().offset_at(1_786_050_000), None);
    }

    #[test]
    fn only_a_clock_a_device_could_stand_on_is_taken() {
        assert!(plausible(7260), "+02:01, set a minute fast by hand");
        assert!(plausible(-4 * 3600 - 30 * 60));
        assert!(!plausible(15 * 3600), "past the bound");
        assert!(!plausible(7201), "not a whole minute");
        let mut clock = Clock::default();
        assert!(!clock.observe(vec![(1_786_000_000, 7201), (1_786_100_000, 15 * 3600)]));
        assert!(clock.is_empty());
    }

    #[test]
    fn a_pass_that_saw_nothing_takes_nothing_away() {
        let mut clock = travelled();
        let held = clock.rows().to_vec();
        assert!(!clock.observe(Vec::new()));
        assert_eq!(clock.rows(), held);
    }

    #[test]
    fn an_observation_the_record_already_states_moves_nothing() {
        let mut clock = travelled();
        assert!(!clock.learn(1_786_050_000, 7200));
        assert!(
            clock.learn(1_789_000_000, 3600),
            "a clock that stepped back"
        );
        assert_eq!(clock.offset_at(1_789_100_000), Some(3600));
    }

    #[test]
    fn a_stamp_reads_on_the_clock_of_its_own_instant() {
        let clock = travelled();
        // Inside the fortnight at +6, and before it at +2.
        assert_eq!(stamp(&clock, 1_787_650_000), "2026-08-25T15:26:40");
        assert_eq!(stamp(&clock, 1_786_050_000), "2026-08-06T23:00:00");
        assert_eq!(
            crate::date::local_at(1_787_650_000, Some(7200)).map(|(d, s)| crate::date::stamp(d, s)),
            Some("2026-08-25T11:26:40".into()),
            "the same instant on the clock the device stands on today"
        );
    }
}
