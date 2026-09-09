//! The zone the device's clock stands in, read out of the file the firmware
//! writes.
//!
//! `/etc/localtime` is a symlink to `/var/local/system/tz`, a TZif blob the
//! firmware generates. A Kindle ships no zoneinfo database and `/etc/TZ` reads
//! `UTC` on every firmware, so this file is the only thing that states the
//! offset, and it is what the firmware's own daemons resolve their clock
//! through — `syslogd` among them, which stamps every line the store is built
//! from.
//!
//! Every one of these files ends in an empty POSIX-TZ footer. Past the last
//! transition musl reads that as UTC where glibc reads the last transition's
//! own offset, and a firmware with no timezone picker writes one fixed
//! transition — so on those devices every instant is past the table and
//! `libc::localtime_r` answers UTC while the log is stamped local. Reading the
//! file here, the way the daemons do, keeps the app on the log's own clock.

use std::sync::OnceLock;

/// The zone file, the symlink every firmware keeps first. `/var/local` is a
/// bind mount of the metadata partition, so the second path reaches the same
/// file on both layouts.
pub const ZONE_PATHS: [&str; 3] = [
    "/etc/localtime",
    "/var/local/system/tz",
    "/var/base-local/metadata/system/tz",
];

/// What a TZif file opens with.
const MAGIC: [u8; 4] = *b"TZif";

/// The length of a header, and of the second one a version 2 file carries.
const HEAD: usize = 44;

/// One offset the zone stands at.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Kind {
    /// Seconds ahead of UTC.
    pub offset: i64,
    /// Whether the file calls this one daylight saving.
    pub dst: bool,
    /// What the file calls it. The firmware writes its own labels rather than
    /// tzdata's: Berlin's +1 reads `CEST` and its +2 `EEDT`.
    pub name: String,
}

/// A parsed zone file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Zone {
    /// The instants the offset changes at, ascending.
    changes: Vec<i64>,
    /// The [`Kind`] each change moves to, one index into `kinds` per change.
    at: Vec<usize>,
    kinds: Vec<Kind>,
    /// Whether a POSIX rule stands past the last change. Every
    /// firmware-written file leaves it empty.
    ruled: bool,
}

impl Zone {
    /// Read a TZif file. `None` where it is not one, is cut short, or states a
    /// transition no type answers for.
    pub fn read(b: &[u8]) -> Option<Self> {
        // A version 2 file restates everything in a second block with 64-bit
        // transitions. The 32-bit block ahead of it is skipped whole.
        let wide = matches!(b.get(4)?, b'2' | b'3' | b'4');
        let width = if wide { 8 } else { 4 };
        let head = match wide {
            true => HEAD + block(&counts(b, 0)?, 4),
            false => 0,
        };
        let [isutc, isstd, leap, times, kinds, chars] = counts(b, head)?;
        if kinds == 0 {
            return None;
        }

        let mut at = head + HEAD;
        let mut changes = Vec::with_capacity(times);
        for i in 0..times {
            let raw = b.get(at + i * width..at + (i + 1) * width)?;
            changes.push(match wide {
                true => i64::from_be_bytes(raw.try_into().ok()?),
                false => i32::from_be_bytes(raw.try_into().ok()?) as i64,
            });
        }
        at += times * width;
        let index = b.get(at..at + times)?.to_vec();
        at += times;

        let names = b.get(at + kinds * 6..at + kinds * 6 + chars)?;
        let mut held = Vec::with_capacity(kinds);
        for i in 0..kinds {
            let o = at + i * 6;
            held.push(Kind {
                offset: i32::from_be_bytes(b.get(o..o + 4)?.try_into().ok()?) as i64,
                dst: *b.get(o + 4)? != 0,
                name: name(names, *b.get(o + 5)? as usize),
            });
        }
        at += kinds * 6 + chars + leap * (width + 4) + isstd + isutc;

        if index.iter().any(|i| usize::from(*i) >= kinds) {
            return None;
        }
        if changes.windows(2).any(|pair| pair[0] > pair[1]) {
            return None;
        }
        Some(Zone {
            changes,
            at: index.iter().map(|i| usize::from(*i)).collect(),
            kinds: held,
            ruled: wide && ruled(b.get(at..)?),
        })
    }

    /// The [`Kind`] standing at `epoch`, and `None` where the file leaves that
    /// to a POSIX rule this does not read.
    fn kind_at(&self, epoch: i64) -> Option<&Kind> {
        let Some(&last) = self.changes.last() else {
            return self.kinds.first();
        };
        if epoch >= last {
            // Past the table the file's own rule takes over. An empty footer
            // states none and the last change's offset stands, which is the
            // reading the firmware's daemons take.
            return match self.ruled {
                true => None,
                false => self.kinds.get(*self.at.last()?),
            };
        }
        if epoch < self.changes[0] {
            // Before the first change, the lowest-indexed kind that is not
            // daylight saving.
            return self.kinds.iter().find(|k| !k.dst).or(self.kinds.first());
        }
        let held = self.changes.partition_point(|change| *change <= epoch) - 1;
        self.kinds.get(*self.at.get(held)?)
    }

    /// Seconds the local clock stands ahead of UTC at `epoch`.
    pub fn offset_at(&self, epoch: i64) -> Option<i64> {
        self.kind_at(epoch).map(|k| k.offset)
    }

    /// What the file calls the offset standing at `epoch`.
    pub fn name_at(&self, epoch: i64) -> Option<&str> {
        self.kind_at(epoch).map(|k| k.name.as_str())
    }

    /// The next instant the offset changes at past `epoch`, and `None` where
    /// the file holds none: a clock that will not move on its own.
    pub fn next_change(&self, epoch: i64) -> Option<i64> {
        self.changes.iter().copied().find(|change| *change > epoch)
    }
}

/// The six counts a header ends with: `isutcnt`, `isstdcnt`, `leapcnt`,
/// `timecnt`, `typecnt`, `charcnt`. `None` where `head` opens no header.
fn counts(b: &[u8], head: usize) -> Option<[usize; 6]> {
    if b.get(head..head + 4)? != MAGIC {
        return None;
    }
    let mut out = [0usize; 6];
    for (i, count) in out.iter_mut().enumerate() {
        let at = head + 20 + i * 4;
        *count = u32::from_be_bytes(b.get(at..at + 4)?.try_into().ok()?) as usize;
    }
    Some(out)
}

/// The bytes the data block behind a header takes, transitions `width` wide.
fn block(counts: &[usize; 6], width: usize) -> usize {
    let [isutc, isstd, leap, times, kinds, chars] = *counts;
    times * (width + 1) + kinds * 6 + chars + leap * (width + 4) + isstd + isutc
}

/// The NUL-terminated designation at `from` in the abbreviation blob.
fn name(names: &[u8], from: usize) -> String {
    let rest = names.get(from..).unwrap_or_default();
    let end = rest.iter().position(|b| *b == 0).unwrap_or(rest.len());
    String::from_utf8_lossy(&rest[..end]).into_owned()
}

/// Whether the `\n`-delimited POSIX rule closing a version 2 file states
/// anything.
fn ruled(tail: &[u8]) -> bool {
    tail.strip_prefix(b"\n")
        .and_then(|rest| rest.strip_suffix(b"\n"))
        .is_some_and(|rule| !rule.is_empty())
}

/// A zone file, and which of [`ZONE_PATHS`] it was read from.
pub struct Held {
    pub path: &'static str,
    pub zone: Zone,
}

/// The zone file this device keeps, read once for the life of the process.
pub fn held() -> Option<&'static Held> {
    static HELD: OnceLock<Option<Held>> = OnceLock::new();
    HELD.get_or_init(|| load(&ZONE_PATHS)).as_ref()
}

/// The first of `paths` that opens and reads as a zone file.
fn load(paths: &[&'static str]) -> Option<Held> {
    paths.iter().find_map(|path| {
        let bytes = std::fs::read(path).ok()?;
        Some(Held {
            path,
            zone: Zone::read(&bytes)?,
        })
    })
}

/// Seconds the device's local clock stands ahead of UTC at `epoch`, and `None`
/// where no zone file answers and the caller must fall back to the C library.
pub fn offset_at(epoch: i64) -> Option<i64> {
    held()?.zone.offset_at(epoch)
}

/// An offset as `+02:01`.
pub fn hhmm(offset: i64) -> String {
    let sign = if offset < 0 { '-' } else { '+' };
    let mins = offset.abs() / 60;
    format!("{sign}{:02}:{:02}", mins / 60, mins % 60)
}

/// One line for the launch log: the zone file this device keeps, or what
/// stands in for it.
pub fn describe(epoch: i64) -> String {
    match held() {
        Some(held) => held.line(epoch),
        None => format!("no file at {}, on the C library's answer", ZONE_PATHS[0]),
    }
}

impl Held {
    /// The file, the offset standing at `epoch`, what the file calls it, and
    /// whether the clock will move on its own again.
    pub fn line(&self, epoch: i64) -> String {
        let Some(offset) = self.zone.offset_at(epoch) else {
            return format!("{}, a POSIX rule, on the C library's answer", self.path);
        };
        let name = self.zone.name_at(epoch).unwrap_or_default();
        let ahead = match self.zone.next_change(epoch) {
            // A clock with no change ahead of it is a hand-set offset: it does
            // not follow daylight saving and stands until someone moves it.
            None => "fixed, no change ahead".to_string(),
            // Read on the clock the change itself brings in, which is this
            // file's business and not the process zone's.
            Some(at) => {
                let local = at + self.zone.offset_at(at).unwrap_or(0);
                let stamp = crate::date::stamp(local.div_euclid(86_400), local.rem_euclid(86_400));
                format!("next change {stamp}")
            }
        };
        format!("{} {} {name}, {ahead}", self.path, hhmm(offset))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::date::days_from_civil;

    /// An instant, as the day count and clock a test names it by.
    fn at(y: i64, m: i64, d: i64, h: i64) -> i64 {
        days_from_civil(y, m, d) * 86_400 + h * 3600
    }

    /// A version 2 TZif over `changes` — `(instant, kind)` — and `kinds` —
    /// `(offset, dst, name)` — closed by `footer`. The 32-bit block carries
    /// the kinds and no transitions, the way a file the firmware writes is
    /// laid out.
    fn tzif(changes: &[(i64, usize)], kinds: &[(i32, bool, &str)], footer: &str) -> Vec<u8> {
        // The designation blob, and where each kind's name starts in it.
        let mut names: Vec<u8> = Vec::new();
        let mut starts: Vec<u8> = Vec::new();
        for (_, _, name) in kinds {
            starts.push(names.len() as u8);
            names.extend_from_slice(name.as_bytes());
            names.push(0);
        }
        let types = |out: &mut Vec<u8>| {
            for (i, (offset, dst, _)) in kinds.iter().enumerate() {
                out.extend_from_slice(&offset.to_be_bytes());
                out.push(u8::from(*dst));
                out.push(starts[i]);
            }
            out.extend_from_slice(&names);
        };
        let header = |out: &mut Vec<u8>, times: usize| {
            out.extend_from_slice(&MAGIC);
            out.push(b'2');
            out.extend_from_slice(&[0u8; 15]);
            for count in [0, 0, 0, times, kinds.len(), names.len()] {
                out.extend_from_slice(&(count as u32).to_be_bytes());
            }
        };

        let mut out = Vec::new();
        header(&mut out, 0);
        types(&mut out);
        header(&mut out, changes.len());
        for (instant, _) in changes {
            out.extend_from_slice(&instant.to_be_bytes());
        }
        for (_, kind) in changes {
            out.push(*kind as u8);
        }
        types(&mut out);
        out.push(b'\n');
        out.extend_from_slice(footer.as_bytes());
        out.push(b'\n');
        out
    }

    /// The shape a firmware with no timezone picker writes: one transition at
    /// the bottom of the calendar, one fixed offset, no rule behind it.
    fn fixed() -> Vec<u8> {
        tzif(
            &[(at(1900, 1, 1, 0), 0)],
            &[(7260, false, "GMT+2:1100")],
            "",
        )
    }

    /// The shape a firmware with one writes: a rule table running out at a
    /// last transition, and no rule behind it either.
    fn ruled_table() -> Vec<u8> {
        tzif(
            &[
                (at(2026, 3, 29, 1), 1),
                (at(2026, 10, 25, 1), 0),
                (at(2027, 3, 28, 1), 1),
            ],
            &[(3600, false, "CEST"), (7200, true, "EEDT")],
            "",
        )
    }

    #[test]
    fn a_fixed_offset_stands_past_its_only_transition() {
        let zone = Zone::read(&fixed()).expect("a zone file");
        // Every instant a device will ever ask about is past 1900, which is
        // where the C library the app is built against answers UTC instead.
        assert_eq!(zone.offset_at(at(2026, 9, 9, 11)), Some(7260));
        assert_eq!(zone.offset_at(at(2038, 9, 9, 11)), Some(7260));
        assert_eq!(zone.name_at(at(2026, 9, 9, 11)), Some("GMT+2:1100"));
        // Nothing will move it.
        assert_eq!(zone.next_change(at(2026, 9, 9, 11)), None);
    }

    #[test]
    fn a_rule_table_states_the_offset_the_instant_falls_in() {
        let zone = Zone::read(&ruled_table()).expect("a zone file");
        assert_eq!(zone.offset_at(at(2026, 9, 9, 11)), Some(7200));
        assert_eq!(zone.name_at(at(2026, 9, 9, 11)), Some("EEDT"));
        // The hour either side of a change.
        assert_eq!(zone.offset_at(at(2026, 10, 25, 0)), Some(7200));
        assert_eq!(zone.offset_at(at(2026, 10, 25, 1)), Some(3600));
        assert_eq!(
            zone.next_change(at(2026, 9, 9, 11)),
            Some(at(2026, 10, 25, 1))
        );
    }

    #[test]
    fn an_instant_past_the_table_takes_the_last_transitions_offset() {
        let zone = Zone::read(&ruled_table()).expect("a zone file");
        // 2027-03-28 is the last row, and 2030 is past it: the offset it moved
        // to stands, rather than the UTC an empty rule would read as.
        assert_eq!(zone.offset_at(at(2030, 1, 1, 0)), Some(7200));
    }

    #[test]
    fn a_stated_posix_rule_is_left_to_the_c_library() {
        let ruled = tzif(
            &[(at(2026, 3, 29, 1), 0)],
            &[(3600, false, "CET")],
            "CET-1CEST,M3.5.0,M10.5.0/3",
        );
        let zone = Zone::read(&ruled).expect("a zone file");
        // Inside the table the table answers; past it the rule does, and this
        // does not read one.
        assert_eq!(zone.offset_at(at(2026, 1, 1, 0)), Some(3600));
        assert_eq!(zone.offset_at(at(2027, 1, 1, 0)), None);
    }

    #[test]
    fn an_instant_under_the_table_takes_the_first_standard_offset() {
        let zone = Zone::read(&ruled_table()).expect("a zone file");
        // Kind 0 is the standard one, and it stands below the first change
        // whatever that change moved to.
        assert_eq!(zone.offset_at(at(1970, 1, 1, 0)), Some(3600));
    }

    #[test]
    fn a_file_that_is_not_one_answers_nothing() {
        assert!(Zone::read(b"").is_none());
        assert!(Zone::read(b"not a zone file at all, but long enough").is_none());
        // A header stating more than the file carries.
        let short = fixed();
        assert!(Zone::read(&short[..short.len() - 20]).is_none());
        // A transition naming a kind the file does not hold, and transitions
        // out of order: both leave a lookup with nothing to stand on.
        let kinds = [(3600, false, "CET")];
        assert!(Zone::read(&tzif(&[(at(2026, 1, 1, 0), 9)], &kinds, "")).is_none());
        let backwards = [(at(2026, 3, 1, 0), 0), (at(2026, 1, 1, 0), 0)];
        assert!(Zone::read(&tzif(&backwards, &kinds, "")).is_none());
    }

    #[test]
    fn a_file_with_no_transitions_stands_at_its_only_offset() {
        let zone = Zone::read(&tzif(&[], &[(0, false, "UTC")], "")).expect("a zone file");
        assert_eq!(zone.offset_at(at(2026, 9, 9, 11)), Some(0));
        assert_eq!(zone.next_change(at(2026, 9, 9, 11)), None);
    }

    #[test]
    fn the_launch_line_names_the_offset_and_what_moves_it_next() {
        let now = at(2026, 9, 9, 11);
        let fixed = Held {
            path: ZONE_PATHS[0],
            zone: Zone::read(&fixed()).expect("a zone file"),
        };
        assert_eq!(
            fixed.line(now),
            "/etc/localtime +02:01 GMT+2:1100, fixed, no change ahead"
        );
        let ruled = Held {
            path: ZONE_PATHS[1],
            zone: Zone::read(&ruled_table()).expect("a zone file"),
        };
        // The change is read on the clock it brings in: 01:00 UTC at +1.
        assert_eq!(
            ruled.line(now),
            "/var/local/system/tz +02:00 EEDT, next change 2026-10-25T02:00:00"
        );
    }

    #[test]
    fn an_offset_reads_as_hours_and_minutes() {
        assert_eq!(hhmm(7260), "+02:01");
        assert_eq!(hhmm(7200), "+02:00");
        assert_eq!(hhmm(0), "+00:00");
        assert_eq!(hhmm(-4 * 3600 - 30 * 60), "-04:30");
    }

    /// Zone files off real devices, named by `READINGLOG_ZONES` as a
    /// `:`-separated list of `path=offset` pairs — the offset in seconds that
    /// device's own `date` reported. Skipped where it is unset.
    #[test]
    fn a_real_zone_file_answers_what_the_device_answered() {
        let Ok(named) = std::env::var("READINGLOG_ZONES") else {
            eprintln!("skipped: set READINGLOG_ZONES=path=offset[:path=offset] to read real files");
            return;
        };
        let now = at(2026, 9, 9, 11);
        for entry in named.split(':').filter(|e| !e.is_empty()) {
            let (path, want) = entry.rsplit_once('=').expect("a path=offset pair");
            let want: i64 = want.parse().expect("an offset in seconds");
            let bytes = std::fs::read(path).expect("a readable zone file");
            let zone = Zone::read(&bytes).expect("a zone file");
            assert_eq!(zone.offset_at(now), Some(want), "{path}");
        }
    }
}
