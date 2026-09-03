//! Proleptic Gregorian date arithmetic on a day count from 1970-01-01.
//!
//! The device has no date library and this needs four things of one: a real
//! calendar to reject a stamp naming no day, a day number to measure elapsed
//! time on across midnight, a weekday, and month lengths for the calendar grid.

use crate::lang::Strings;

/// `(year, month, day)` as days since 1970-01-01, negative before it.
///
/// Howard Hinnant's `days_from_civil`, shifted to the Unix epoch. Exact for
/// every year this will ever see and for a wide margin either side.
pub fn days_from_civil(y: i64, m: i64, d: i64) -> i64 {
    let y = y - if m <= 2 { 1 } else { 0 };
    let era = if y >= 0 { y } else { y - 399 } / 400;
    let yoe = y - era * 400;
    let doy = (153 * (m + if m > 2 { -3 } else { 9 }) + 2) / 5 + d - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146_097 + doe - 719_468
}

/// The inverse: a day count back to `(year, month, day)`.
pub fn civil_from_days(z: i64) -> (i64, i64, i64) {
    let z = z + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = mp + if mp < 10 { 3 } else { -9 };
    (y + if m <= 2 { 1 } else { 0 }, m, d)
}

/// Whether `(y, m, d)` names a day that exists.
pub fn is_valid(y: i64, m: i64, d: i64) -> bool {
    (1..=12).contains(&m) && d >= 1 && d <= days_in_month(y, m)
}

pub fn is_leap(y: i64) -> bool {
    y % 4 == 0 && (y % 100 != 0 || y % 400 == 0)
}

pub fn days_in_month(y: i64, m: i64) -> i64 {
    match m {
        1 | 3 | 5 | 7 | 8 | 10 | 12 => 31,
        4 | 6 | 9 | 11 => 30,
        2 if is_leap(y) => 29,
        2 => 28,
        _ => 0,
    }
}

/// 0 = Monday through 6 = Sunday. 1970-01-01 was a Thursday.
pub fn weekday(days: i64) -> usize {
    (days + 3).rem_euclid(7) as usize
}

/// A `YYYY-MM-DD` key back to a day count, or `None` when it names no day.
pub fn parse_day(key: &str) -> Option<i64> {
    let b = key.as_bytes();
    if b.len() < 10 || b[4] != b'-' || b[7] != b'-' {
        return None;
    }
    let y: i64 = key[0..4].parse().ok()?;
    let m: i64 = key[5..7].parse().ok()?;
    let d: i64 = key[8..10].parse().ok()?;
    is_valid(y, m, d).then(|| days_from_civil(y, m, d))
}

/// The day part of a `YYYY-MM-DDTHH:MM:SS` instant.
pub fn day_of(at: &str) -> &str {
    at.get(..10).unwrap_or(at)
}

/// Seconds into the day of a `YYYY-MM-DDTHH:MM:SS` instant.
pub fn secs_of(at: &str) -> i64 {
    let Some(clock) = at.get(11..19) else {
        return 0;
    };
    let n = |r: std::ops::Range<usize>| clock[r].parse::<i64>().unwrap_or(0);
    n(0..2) * 3600 + n(3..5) * 60 + n(6..8)
}

/// Now, as `(day count, seconds into the day)` on the device's own clock.
///
/// Local and never UTC. Every stamp in the log is local wall clock with no zone
/// on it, so a reading log compared against a UTC "today" would file a late
/// night west of Greenwich a day early and one east of it a day late.
pub fn now() -> (i64, i64) {
    // SAFETY: `localtime_r` fills a caller-owned `tm` and takes the zone from
    // the process environment. No pointer outlives the call.
    unsafe {
        let clock = libc::time(std::ptr::null_mut());
        let mut tm: libc::tm = std::mem::zeroed();
        if libc::localtime_r(&clock, &mut tm).is_null() {
            // A clock the C library will not break down still orders events;
            // the epoch's own day is the honest answer.
            return (0, 0);
        }
        (
            days_from_civil(
                tm.tm_year as i64 + 1900,
                tm.tm_mon as i64 + 1,
                tm.tm_mday as i64,
            ),
            tm.tm_hour as i64 * 3600 + tm.tm_min as i64 * 60 + tm.tm_sec as i64,
        )
    }
}

/// "Aug 9" — enough to place a day at a glance. `9月9日` where the language
/// writes a date year first.
pub fn short_day(days: i64, s: &Strings) -> String {
    let (_, m, d) = civil_from_days(days);
    let month = s.months_short[(m - 1).clamp(0, 11) as usize];
    match s.date_ymd {
        true => format!("{month}{d}日"),
        false => format!("{month} {d}"),
    }
}

/// "Sun, 9 August 2026", or "2026年8月9日（日）".
pub fn long_day(days: i64, s: &Strings) -> String {
    let (y, m, d) = civil_from_days(days);
    let weekday = s.weekdays_short[weekday(days)];
    let month = s.months[(m - 1).clamp(0, 11) as usize];
    match s.date_ymd {
        true => format!("{y}年{}{d}日 {weekday}", (m - 1).clamp(0, 11) + 1),
        false => format!("{weekday}, {d} {month} {y}"),
    }
}

/// Durations read as "4h 12m", "37m", "2m" — and "4小时12分", "4 h 12 min".
/// Seconds are never shown: the counters behind these figures are not that
/// precise, and a reading log measured to the second would claim an accuracy
/// it does not have.
pub fn duration(secs: i64, s: &Strings) -> String {
    let sp = if s.unit_space { " " } else { "" };
    let (h, m) = (s.hours, s.minutes);
    // Nothing read is nothing, not "under a minute": a day with no reading on
    // it reads as a day with no reading on it.
    if secs <= 0 {
        return format!("0{sp}{m}");
    }
    if secs < 60 {
        return format!("<1{sp}{m}");
    }
    let hours = secs / 3600;
    let mins = (secs % 3600 + 30) / 60;
    let (hours, mins) = if mins == 60 {
        (hours + 1, 0)
    } else {
        (hours, mins)
    };
    match (hours, mins) {
        (0, mins) => format!("{mins}{sp}{m}"),
        (hours, 0) => format!("{hours}{sp}{h}"),
        (hours, mins) => format!("{hours}{sp}{h} {mins}{sp}{m}"),
    }
}

/// The same, narrowed for a cell with no room: "4h12", "37m".
pub fn duration_tight(secs: i64, s: &Strings) -> String {
    if secs < 60 {
        return "·".into();
    }
    let (hours, mins) = (secs / 3600, (secs % 3600) / 60);
    match hours {
        0 => format!("{mins}{}", s.minutes),
        _ => format!("{hours}{}{mins:02}", s.hours),
    }
}

/// Word counts read as "1.2M", "48k", "812".
pub fn words(n: i64) -> String {
    match n {
        n if n >= 1_000_000 => format!("{:.1}M", n as f64 / 1_000_000.0),
        n if n >= 1_000 => format!("{}k", (n as f64 / 1000.0).round() as i64),
        n => n.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::lang::Lang;

    /// English, which the assertions below are written in.
    fn en() -> &'static Strings {
        Lang::English.strings()
    }

    #[test]
    fn the_epoch_is_day_zero_and_a_thursday() {
        assert_eq!(days_from_civil(1970, 1, 1), 0);
        assert_eq!(weekday(0), 3);
        assert_eq!(civil_from_days(0), (1970, 1, 1));
    }

    #[test]
    fn a_day_count_round_trips_through_a_century() {
        for day in -30_000..30_000 {
            let (y, m, d) = civil_from_days(day);
            assert_eq!(days_from_civil(y, m, d), day, "{y}-{m}-{d}");
            assert!(is_valid(y, m, d));
        }
    }

    #[test]
    fn february_knows_its_leap_years() {
        assert_eq!(days_in_month(2024, 2), 29);
        assert_eq!(days_in_month(2026, 2), 28);
        assert_eq!(days_in_month(2000, 2), 29);
        assert_eq!(days_in_month(1900, 2), 28);
        assert!(!is_valid(2026, 2, 29));
        assert!(is_valid(2024, 2, 29));
    }

    #[test]
    fn a_day_key_reads_back_and_rejects_a_day_that_is_not_one() {
        let day = days_from_civil(2026, 8, 29);
        assert_eq!(parse_day("2026-08-29"), Some(day));
        assert_eq!(parse_day("2026-02-30"), None);
        assert_eq!(parse_day("2026-13-01"), None);
        assert_eq!(parse_day("not-a-day"), None);
    }

    #[test]
    fn an_instant_gives_up_its_day_and_its_clock() {
        assert_eq!(day_of("2026-08-29T21:03:07"), "2026-08-29");
        assert_eq!(secs_of("2026-08-29T21:03:07"), 21 * 3600 + 3 * 60 + 7);
        assert_eq!(secs_of("2026-08-29T00:00:00"), 0);
    }

    #[test]
    fn durations_round_to_the_minute_and_carry() {
        assert_eq!(duration(0, en()), "0m");
        assert_eq!(duration(59, en()), "<1m");
        assert_eq!(duration(1, en()), "<1m");
        assert_eq!(duration(120, en()), "2m");
        assert_eq!(duration(3600, en()), "1h");
        assert_eq!(duration(4 * 3600 + 12 * 60, en()), "4h 12m");
        // 59m30s rounds to 60 minutes, which is an hour and not "0h 60m".
        assert_eq!(duration(3570, en()), "1h");
        assert_eq!(duration_tight(4 * 3600 + 12 * 60, en()), "4h12");
        assert_eq!(duration_tight(30, en()), "·");
    }

    #[test]
    fn word_counts_shorten_by_magnitude() {
        assert_eq!(words(812), "812");
        assert_eq!(words(48_000), "48k");
        assert_eq!(words(1_200_000), "1.2M");
    }
}
