//! The annotation merge across a change of clock. Each fixture holds one
//! fortnight of reading in both frames, differing only in the offset the device
//! stood on. Skipped unless `READINGLOG_TZ_FIXTURES` names the fixture tree.

use std::path::{Path, PathBuf};

use readinglog_native::clock::Clock;
use readinglog_native::identify::Witness;
use readinglog_native::log::session::{Measure, Session};
use readinglog_native::store::{BookRecord, Named, Store};
use readinglog_native::{annotate, clippings, clock, sidecar};

/// The book both fixtures hold, by the name its `.sdr` carries.
const STEM: &str = "[Greg Egan] Diaspora (2017).0cdbcecc";
const EXTENT: i64 = 743_115;

/// The clocks the fixtures were written on.
const HOME: i64 = 2 * 3600;
const AWAY: i64 = 6 * 3600;

/// The fixtures directory, or `None` where the run is not asked to read one.
fn fixtures() -> Option<PathBuf> {
    match std::env::var("READINGLOG_TZ_FIXTURES") {
        Ok(at) => Some(PathBuf::from(at)),
        Err(_) => {
            eprintln!("skipped: set READINGLOG_TZ_FIXTURES to a directory of fixture trees");
            None
        }
    }
}

/// One fixture folded the way a launch folds it: the clocks measured off the
/// two files, then the merge over them.
fn folded(at: &Path) -> (Store, annotate::Merge) {
    let documents = at.join("documents");
    let clips = documents.join("My Clippings.txt");
    let records = clippings::read(&clips);
    let shelf = sidecar::read(&documents, false);
    let survey = sidecar::survey(&documents);
    let mut store = Store {
        books: vec![BookRecord {
            extent: EXTENT,
            cde_key: "B00DIASPORA".into(),
            title: "Diaspora".into(),
            author: "Greg Egan".into(),
            location: format!("/mnt/us/documents/{STEM}.kfx"),
            ..BookRecord::default()
        }],
        ..Store::default()
    };
    store.clock.observe(annotate::clocks_seen(&records, &shelf));
    let merge = annotate::fold(&mut store, &clips, &records, &shelf, &survey);
    (store, merge)
}

/// Every hour of the day a mark stands at, ascending.
fn hours(store: &Store) -> Vec<u32> {
    let mut out: Vec<u32> = store
        .marks
        .iter()
        .filter_map(|m| m.at.get(11..13)?.parse().ok())
        .collect();
    out.sort_unstable();
    out.dedup();
    out
}

/// What one fold came to, and the reader's own hours, as a report states them.
fn report(name: &str, store: &Store, merge: &annotate::Merge) {
    let paired = store
        .marks
        .iter()
        .filter(|m| m.start >= 0 && !m.body.is_empty())
        .count();
    println!(
        "{name:>6}: clippings {:3} sidecar {:3} held {:3} retired {:3} paired {paired:3}\n        \
         clocks {:?}  hours {:?}",
        merge.clippings,
        merge.sidecars,
        merge.held,
        merge.retired,
        store.clock.offsets(),
        hours(store),
    );
}

/// Both fixtures come to the same thing: the reader read at the same hour in
/// both, and the device's offset is not something a mark may show.
fn same_either_way(at: &Path, name: &str, clock: i64) -> Store {
    let (store, merge) = folded(&at.join(name));
    report(name, &store, &merge);
    assert_eq!(merge.clippings, 70, "every write the file holds");
    assert_eq!(merge.sidecars, 60, "what the book still holds");
    assert_eq!(merge.retired, 10, "only the marks the reader cleared");
    assert_eq!(merge.held, 60, "one row per record the book still holds");
    assert_eq!(
        store
            .marks
            .iter()
            .filter(|m| m.start >= 0 && !m.body.is_empty())
            .count(),
        60,
        "every sidecar record met the clipping that wrote it, words and all"
    );
    assert_eq!(hours(&store), vec![0, 22, 23]);
    assert_eq!(
        store.clock.offsets(),
        vec![clock],
        "the clock the two files state between them"
    );
    store
}

#[test]
fn a_fortnight_read_at_home_keeps_the_hour_it_was_read_at() {
    let Some(at) = fixtures() else { return };
    same_either_way(&at, "home", HOME);
}

#[test]
fn a_fortnight_read_four_hours_east_keeps_it_too() {
    let Some(at) = fixtures() else { return };
    same_either_way(&at, "away", AWAY);
}

// ---- a vocab lookup against the sitting it was made in --------------------

/// Three nights at the reader's hour, each one sitting on a class no record
/// names, as the log wrote them on the device's own clock.
fn three_nights(offset: i64) -> Store {
    let sitting = |at: &str, to: &str, end: i64| Session {
        started_at: at.into(),
        ended_at: to.into(),
        end_position: end,
        seconds: 3_600,
        measure: Measure::Counted,
        tz_offset_s: Some(offset),
        ..Session::default()
    };
    Store {
        sessions: vec![
            sitting("2026-08-24T22:50:00", "2026-08-25T00:40:00", 148_207),
            sitting("2026-08-25T22:55:00", "2026-08-26T00:35:00", 500_100),
            sitting("2026-08-26T23:05:00", "2026-08-27T00:20:00", 700_200),
        ],
        ..Store::default()
    }
}

/// The true instant a device standing `offset` ahead of UTC called `at`.
fn epoch_of(at: &str, offset: i64) -> i64 {
    let day = readinglog_native::date::parse_day(readinglog_native::date::day_of(at)).unwrap();
    day * 86_400 + readinglog_native::date::secs_of(at) - offset
}

/// The witness `vocab::read_from` makes of a lookup at that instant.
fn lookup(clock: &Clock, epoch: i64) -> Witness {
    Witness {
        at: clock::stamp(clock, epoch),
        title: "Diaspora".into(),
        author: "Greg Egan".into(),
        key: "B00DIASPORA".into(),
        pos: -1,
    }
}

/// A lookup made mid-sitting, on a device standing at `offset`, read back by a
/// record whose clock has seen that offset. It has to land in its own sitting.
fn names_its_book(offset: i64) {
    let mut store = three_nights(offset);
    store.clock.observe(vec![
        (epoch_of("2026-08-24T22:50:00", offset), offset),
        (epoch_of("2026-08-26T23:05:00", offset), offset),
    ]);
    let said = [lookup(
        &store.clock,
        epoch_of("2026-08-25T23:30:00", offset),
    )];
    println!("  {offset:>6}s: witness {}", said[0].at);
    assert_eq!(
        said[0].at, "2026-08-25T23:30:00",
        "the lookup reads on the clock the sitting was logged on"
    );
    let named = store.name_from(&said, Named::Vocab, &mut Vec::new());
    assert_eq!(named, 1, "the lookup fell inside its own sitting");
    assert_eq!(
        store.book_for(500_100, None).map(|b| b.title.clone()),
        Some("Diaspora".to_string())
    );
}

#[test]
fn a_lookup_made_at_home_names_its_book() {
    names_its_book(HOME);
}

#[test]
fn a_lookup_made_four_hours_east_names_it_too() {
    names_its_book(AWAY);
}

// ---- the clock a real device's own two files state ------------------------

/// A device capture named by `READINGLOG_CAPTURE`: `My Clippings.txt`, a
/// `Sidle` shelf and `sessions.tsv`. The measured offset must be the one that
/// device's own `m` row recorded, which the measurement never reads.
#[test]
fn a_real_captures_two_files_state_the_clock_its_store_recorded() {
    let Ok(at) = std::env::var("READINGLOG_CAPTURE") else {
        eprintln!("skipped: set READINGLOG_CAPTURE to a device capture directory");
        return;
    };
    let at = PathBuf::from(at);
    let clips = at.join("My Clippings.txt");
    let records = clippings::read(&clips);
    let shelf = sidecar::read(&at.join("Sidle"), false);
    let seen = annotate::clocks_seen(&records, &shelf);
    let mut clock = Clock::default();
    clock.observe(seen.clone());
    let stated: Vec<&str> = std::fs::read_to_string(at.join("sessions.tsv"))
        .unwrap_or_default()
        .lines()
        .filter(|l| l.starts_with("m\t"))
        .map(|l| l.split('\t').nth(2).unwrap_or_default())
        .collect::<Vec<_>>()
        .iter()
        .map(|s| Box::leak(s.to_string().into_boxed_str()) as &str)
        .collect();
    println!(
        "capture: clippings {} sidecar records {} readings {} clocks {:?}, store's m row says {stated:?}",
        records.len(),
        shelf
            .rosters
            .iter()
            .map(|r| r.annotations.len())
            .sum::<usize>(),
        seen.len(),
        clock.offsets(),
    );
    assert!(!seen.is_empty(), "the two files agreed about nothing");
    for offset in clock.offsets() {
        assert!(clock::plausible(offset), "{offset} is not a clock");
    }
    assert_eq!(
        clock
            .offsets()
            .iter()
            .map(|o| o.to_string())
            .collect::<Vec<_>>(),
        stated,
        "the measured clock against the one that device's own store recorded"
    );
}

// ---- one record holding a trip and the weeks either side ------------------

/// A fortnight at +6 between two weeks at home. The record has to hold both
/// clocks, and every mark has to keep the hour it was made at.
#[test]
fn a_record_holding_a_trip_keeps_both_clocks_and_one_habit() {
    let Some(at) = fixtures() else { return };
    let trip = at.join("trip");
    if !trip.is_dir() {
        eprintln!("skipped: no trip fixture; run make_fixtures.py --leave 7 --home 21");
        return;
    }
    let (store, merge) = folded(&trip);
    report("trip", &store, &merge);
    assert_eq!(merge.clippings, 140, "every write the file holds");
    assert_eq!(merge.sidecars, 120, "what the book still holds");
    assert_eq!(merge.retired, 20, "only the marks the reader cleared");
    assert_eq!(
        store
            .marks
            .iter()
            .filter(|m| m.start >= 0 && !m.body.is_empty())
            .count(),
        120,
        "every sidecar record met the clipping that wrote it, across both clocks"
    );
    let mut offsets = store.clock.offsets();
    offsets.sort_unstable();
    assert_eq!(offsets, vec![HOME, AWAY]);
    assert_eq!(
        store.clock.rows().len(),
        3,
        "out, back, and home either side"
    );
    assert_eq!(hours(&store), vec![0, 22, 23]);
}

/// Re-merging a capture's own two files gives back the marks its record
/// already holds: on one clock the join moves nothing.
#[test]
fn re_merging_a_real_captures_files_moves_none_of_its_marks() {
    let Ok(at) = std::env::var("READINGLOG_CAPTURE") else {
        eprintln!("skipped: set READINGLOG_CAPTURE to a device capture directory");
        return;
    };
    let at = PathBuf::from(at);
    let Ok(text) = std::fs::read_to_string(at.join("sessions.tsv")) else {
        eprintln!("skipped: the capture holds no sessions.tsv");
        return;
    };
    let mut store = Store::from_text(&text);
    let held = store.marks.clone();
    assert!(!held.is_empty(), "the capture states no marks");

    let clips = at.join("My Clippings.txt");
    let records = clippings::read(&clips);
    let documents = at.join("Sidle");
    let shelf = sidecar::read(&documents, false);
    let survey = sidecar::survey(&documents);
    store.clock.observe(annotate::clocks_seen(&records, &shelf));
    let merge = annotate::fold(&mut store, &clips, &records, &shelf, &survey);
    assert!(merge.read, "the gate held the merge back");

    let moved: Vec<(&str, &str, &str)> = held
        .iter()
        .zip(&store.marks)
        .filter(|(was, now)| was.at != now.at)
        .map(|(was, now)| (was.title.as_str(), was.at.as_str(), now.at.as_str()))
        .collect();
    println!(
        "capture: {} marks held, {} after the re-merge, clocks {:?}, {} moved",
        held.len(),
        store.marks.len(),
        store.clock.offsets(),
        moved.len(),
    );
    for (title, was, now) in &moved {
        println!("    {title}: {was} -> {now}");
    }
    assert_eq!(
        store.marks.len(),
        held.len(),
        "the re-merge changed the count"
    );
    assert!(moved.is_empty(), "marks moved on one clock");
    assert_eq!(store.marks, held, "a row moved that is not its stamp");
}
