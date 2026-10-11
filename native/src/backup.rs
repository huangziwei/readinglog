//! Backups under [`BACKUPS_DIR`]. Each holds a `sessions.tsv` and the files
//! `covers::COVERS_DIR` held when [`keep_record`] wrote it. [`restore`] merges
//! one into a [`Record`].

use std::path::{Path, PathBuf};

use crate::covers;
use crate::record::Record;
use crate::update::archive::{self, Archive, Source};

/// The directory holding them, under `dir`.
pub const BACKUPS_DIR: &str = "backups";

/// The record's name inside a backup.
const RECORD: &str = "sessions.tsv";

/// The manifest's name inside a backup.
const ABOUT: &str = "about.tsv";

/// The Kindle's own model line.
const MODEL: &str = "/proc/device-tree/model";

/// The firmware's version line.
const FIRMWARE: &str = "/etc/prettyversion.txt";

/// What a backup holds.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    /// The whole record, with every cover held when it was written.
    Record,
    /// One book: its own row, its sittings, and its cover.
    Book,
}

impl Kind {
    /// What a backup of this kind is named, before its stamp.
    fn stem(self) -> &'static str {
        match self {
            Kind::Record => "readinglog",
            Kind::Book => "book",
        }
    }
}

/// One backup on disk.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Backup {
    pub path: PathBuf,
    /// `YYMMDD-HHMMSS`, as the name spells it.
    pub stamp: String,
    pub kind: Kind,
    pub bytes: u64,
}

/// Where the backups live under `dir`.
pub fn dir(dir: &Path) -> PathBuf {
    dir.join(BACKUPS_DIR)
}

/// The Kindle's clock as the `YYMMDD:HHMMSS` a backup is named for.
pub fn stamped_now() -> String {
    let (days, secs) = crate::date::now();
    crate::log::line::log_stamp(&crate::date::stamp(days, secs)).unwrap_or_default()
}

/// The `stamp` a backup's name carries: `YYMMDD-HHMMSS`, which
/// [`Backup::stamp`] reads back.
const STAMP_LEN: usize = 13;

/// A backup's name: `stamp` with every `:` written as `-`, and `tag` where
/// one names the book. Two backups of one book at one second differ in
/// nothing else, and [`free`] steps them apart.
pub fn name(kind: Kind, stamp: &str, tag: &str) -> String {
    let stamp = stamp.replace(':', "-");
    match tag.is_empty() {
        true => format!("{}-{stamp}.zip", kind.stem()),
        false => format!("{}-{stamp}-{tag}.zip", kind.stem()),
    }
}

/// `named` under `dir`, stepping `-2`, `-3` while a file stands at it.
/// `archive::write` renames onto its path, and a backup is never the file
/// that goes.
fn free(dir: &Path, named: &str) -> PathBuf {
    let at = dir.join(named);
    if !at.exists() {
        return at;
    }
    let stem = named.strip_suffix(".zip").unwrap_or(named);
    (2..)
        .map(|n| dir.join(format!("{stem}-{n}.zip")))
        .find(|next| !next.exists())
        .unwrap_or(at)
}

/// What a backup states about itself, beside the record it carries. A
/// backup written without an [`ABOUT`] entry answers every field empty.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct About {
    /// The clock the backup was written at, `YYMMDD:HHMMSS`.
    pub written: String,
    /// `update::VERSION`.
    pub app: String,
    /// [`MODEL`], one line.
    pub kindle: String,
    /// [`FIRMWARE`], one line.
    pub firmware: String,
    /// The length of `Record::sittings`.
    pub sittings: usize,
    /// The day the first and the last sitting started on, `YYYY-MM-DD`.
    pub first: String,
    pub last: String,
}

impl About {
    /// Whether a backup stated any of this.
    pub fn is_empty(&self) -> bool {
        *self == About::default()
    }
}

/// One line per field, the name and the value tab apart.
fn about_text(record: &Record, stamp: &str) -> String {
    let day = |at: Option<&crate::log::sitting::Sitting>| {
        at.map(|s| crate::date::day_of(&s.started_at).to_string())
            .unwrap_or_default()
    };
    [
        ("written", stamp.to_string()),
        ("app", crate::update::VERSION.to_string()),
        ("device", one_line(MODEL)),
        ("firmware", one_line(FIRMWARE)),
        ("sittings", record.sittings.len().to_string()),
        ("first", day(record.sittings.first())),
        ("last", day(record.sittings.last())),
    ]
    .iter()
    .map(|(name, value)| format!("{name}\t{value}\n"))
    .collect()
}

/// The first line of the file at `at`, without its control characters. Empty
/// where there is no file to read.
fn one_line(at: &str) -> String {
    std::fs::read_to_string(at)
        .unwrap_or_default()
        .lines()
        .next()
        .unwrap_or_default()
        .trim_matches(|c: char| c.is_control() || c.is_whitespace())
        .to_string()
}

pub fn about(at: &Path) -> archive::Result<About> {
    let mut open = Archive::open(at)?;
    let Some(entry) = open.entries().iter().find(|e| e.path == ABOUT).cloned() else {
        return Ok(About::default());
    };
    let bytes = open.read(&entry)?;
    Ok(read_about(&String::from_utf8_lossy(&bytes)))
}

/// [`about_text`] read back.
fn read_about(text: &str) -> About {
    let mut out = About::default();
    for line in text.lines() {
        let Some((name, value)) = line.split_once('\t') else {
            continue;
        };
        match name {
            "written" => out.written = value.to_string(),
            "app" => out.app = value.to_string(),
            "device" => out.kindle = value.to_string(),
            "firmware" => out.firmware = value.to_string(),
            "sittings" => out.sittings = value.parse().unwrap_or(0),
            "first" => out.first = value.to_string(),
            "last" => out.last = value.to_string(),
            _ => {}
        }
    }
    out
}

/// Write a backup of `record` and every cover held, taking nothing away.
/// Answers where it landed.
pub fn back_up(dir: &Path, record: &Record) -> archive::Result<PathBuf> {
    keep_record(dir, record, &stamped_now(), true)
}

/// Write `record`, and under `covers` every file in `covers::COVERS_DIR`,
/// into a backup named for `stamp`. Answers where it landed.
pub fn keep_record(
    dir: &Path,
    record: &Record,
    stamp: &str,
    with_covers: bool,
) -> archive::Result<PathBuf> {
    let text = record.text();
    let said = about_text(record, stamp);
    let mut entries: Vec<(String, Source<'_>)> = vec![
        (RECORD.to_string(), Source::Bytes(text.as_bytes())),
        (ABOUT.to_string(), Source::Bytes(said.as_bytes())),
    ];
    let held = match with_covers {
        true => covers_under(dir),
        false => Vec::new(),
    };
    for (name, path) in &held {
        entries.push((format!("{}/{name}", covers::COVERS_DIR), Source::File(path)));
    }
    let at = free(&self::dir(dir), &name(Kind::Record, stamp, ""));
    archive::write(&at, &entries)?;
    Ok(at)
}

/// Write `one` and the cover `covers::path` names, into a backup named for
/// `stamp` and for the book. Answers where it landed.
pub fn keep_book(dir: &Path, one: &Record, stamp: &str) -> archive::Result<PathBuf> {
    let text = one.text();
    let mut entries: Vec<(String, Source<'_>)> =
        vec![(RECORD.to_string(), Source::Bytes(text.as_bytes()))];
    let cover = one
        .books
        .first()
        .map(|b| covers::path(dir, &b.cde_key))
        .filter(|p| p.is_file());
    if let Some(path) = &cover {
        let named = path
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_default();
        entries.push((
            format!("{}/{named}", covers::COVERS_DIR),
            Source::File(path),
        ));
    }
    let tag: String = one
        .books
        .first()
        .map(|b| b.cde_key.as_str())
        .unwrap_or_default()
        .chars()
        .filter(|c| c.is_ascii_alphanumeric())
        .collect();
    let at = free(&self::dir(dir), &name(Kind::Book, stamp, &tag));
    archive::write(&at, &entries)?;
    Ok(at)
}

/// What a whole-record reset keeps.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Keep {
    /// A backup of the record and the covers in `covers::COVERS_DIR`.
    Backup,
    /// Nothing. `covers::sweep` empties `covers::COVERS_DIR`.
    Nothing,
}

/// [`Record::reset`] `record` and save it under `dir`. [`Keep::Backup`] writes
/// a backup first and answers `Err` where that write fails. Answers the
/// backup written.
pub fn reset(dir: &Path, record: &mut Record, keep: Keep) -> archive::Result<Option<PathBuf>> {
    if record.watermark.is_empty() {
        return Ok(None);
    }
    let kept = match keep {
        Keep::Backup => Some(keep_record(dir, record, &stamped_now(), true)?),
        Keep::Nothing => None,
    };
    record.reset();
    record.save(dir)?;
    if keep == Keep::Nothing {
        // After `save`: `Record::keep_covers` sweeps what a crash here leaves.
        covers::sweep(dir, &[]);
    }
    Ok(kept)
}

/// [`Record::drop_nested`] `record` and save it under `dir`, writing a backup
/// first. Answers the sittings dropped and the backup written, `None` where
/// [`Record::nested`] counts none. `record` stands whole where either write fails.
pub fn drop_nested(dir: &Path, record: &mut Record) -> archive::Result<Option<(usize, PathBuf)>> {
    if record.nested() == 0 {
        return Ok(None);
    }
    let kept = keep_record(dir, record, &stamped_now(), true)?;
    let mut fixed = record.clone();
    let went = fixed.drop_nested();
    fixed.save(dir)?;
    *record = fixed;
    Ok(Some((went, kept)))
}

/// Every backup under `dir`, newest first. A name no [`Kind::stem`] opens is
/// left out.
pub fn list(dir: &Path) -> Vec<Backup> {
    let Ok(entries) = std::fs::read_dir(self::dir(dir)) else {
        return Vec::new();
    };
    let mut out: Vec<Backup> = entries
        .flatten()
        .filter_map(|e| {
            let path = e.path();
            let named = path.file_name()?.to_str()?;
            let stem = named.strip_suffix(".zip")?;
            let kind = [Kind::Record, Kind::Book]
                .into_iter()
                .find(|k| stem.starts_with(&format!("{}-", k.stem())))?;
            // Past the stamp stands the book a `Kind::Book` name carries and
            // the step `free` took, neither of them part of the stamp.
            let rest = &stem[kind.stem().len() + 1..];
            Some(Backup {
                stamp: rest.get(..STAMP_LEN).unwrap_or(rest).to_string(),
                kind,
                bytes: e.metadata().map(|m| m.len()).unwrap_or_default(),
                path,
            })
        })
        .collect();
    out.sort_by(|a, b| b.stamp.cmp(&a.stamp).then(a.path.cmp(&b.path)));
    out
}

/// The record the backup at `at` holds, through [`Record::from_backup`].
pub fn peek(at: &Path) -> archive::Result<Record> {
    let mut open = Archive::open(at)?;
    let entry = open
        .entries()
        .iter()
        .find(|e| e.path == RECORD)
        .cloned()
        .ok_or_else(|| archive::Error::NoMarker(RECORD.to_string()))?;
    let bytes = open.read(&entry)?;
    Ok(Record::from_backup(&String::from_utf8_lossy(&bytes)))
}

/// What a [`restore`] merged.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Merged {
    /// Sittings [`Record::merge`] added.
    pub added: usize,
    /// Whether `record` holds every row of the backup, and every cover it
    /// carried is a file on disk.
    pub whole: bool,
}

/// Fold the backup at `at` into `record`, and write each cover it carries
/// that `covers::COVERS_DIR` does not hold. Answers what landed, reporting
/// files read and files to read as it goes.
pub fn restore(
    dir: &Path,
    at: &Path,
    record: &mut Record,
    on: &mut dyn FnMut(usize, usize),
) -> archive::Result<Merged> {
    let mut open = Archive::open(at)?;
    let entries: Vec<_> = open
        .entries()
        .iter()
        .filter(|e| !e.is_dir())
        .cloned()
        .collect();
    let total = entries.len();
    let mut added = 0;
    let mut inside = Record::default();
    let mut covers: Vec<PathBuf> = Vec::new();
    // `done` counts the entries behind this one; `on(total, total)` closes it.
    for (done, entry) in entries.iter().enumerate() {
        on(done, total);
        if entry.path == RECORD {
            let bytes = open.read(entry)?;
            inside = Record::from_backup(&String::from_utf8_lossy(&bytes));
            added = record.fold_in(&inside);
            continue;
        }
        let Some(named) = entry.path.strip_prefix(&format!("{}/", covers::COVERS_DIR)) else {
            continue;
        };
        // A name, never a path: `out` stays under `covers::COVERS_DIR`.
        if named.is_empty() || named.contains('/') || named.starts_with('.') {
            continue;
        }
        let out = dir.join(covers::COVERS_DIR).join(named);
        covers.push(out.clone());
        if out.exists() {
            continue;
        }
        let bytes = open.read(entry)?;
        if let Some(parent) = out.parent() {
            std::fs::create_dir_all(parent)?;
        }
        std::fs::write(&out, &bytes)?;
    }
    on(total, total);
    Ok(Merged {
        added,
        whole: holds(record, &inside) && covers.iter().all(|j| j.is_file()),
    })
}

/// Whether `record` holds every sitting, end and book row of `inside`: a
/// sitting under its start, or under its close from a start no later, as
/// `Record::merge` keeps one.
pub fn holds(record: &Record, inside: &Record) -> bool {
    let sitting = |a: &crate::log::sitting::Sitting| {
        record.sittings.iter().any(|s| {
            s.end_position == a.end_position
                && (s.started_at == a.started_at
                    || (s.ended_at == a.ended_at && s.started_at <= a.started_at))
        })
    };
    inside.sittings.iter().all(sitting)
        && inside.ends.iter().all(|e| record.ends.contains(e))
        && inside.books.iter().all(|b| {
            record
                .books
                .iter()
                .any(|h| h.extent == b.extent && h.cde_key == b.cde_key)
        })
}

/// Take the backup at `at` off disk. The one call that deletes one.
pub fn remove(at: &Path) -> std::io::Result<()> {
    std::fs::remove_file(at)
}

/// How many bytes the cover cache and the backups take under `dir`.
pub fn sizes(dir: &Path) -> (u64, u64) {
    (
        weight(&dir.join(covers::COVERS_DIR)),
        weight(&self::dir(dir)),
    )
}

/// The files directly under `at`, added up.
fn weight(at: &Path) -> u64 {
    let Ok(entries) = std::fs::read_dir(at) else {
        return 0;
    };
    entries
        .flatten()
        .filter_map(|e| e.metadata().ok())
        .filter(|m| m.is_file())
        .map(|m| m.len())
        .sum()
}

/// Each `.jpg` under `covers::COVERS_DIR`, as its name and its path.
fn covers_under(dir: &Path) -> Vec<(String, PathBuf)> {
    let Ok(entries) = std::fs::read_dir(dir.join(covers::COVERS_DIR)) else {
        return Vec::new();
    };
    let mut out: Vec<(String, PathBuf)> = entries
        .flatten()
        .filter(|e| e.metadata().is_ok_and(|m| m.is_file()))
        .filter_map(|e| {
            let path = e.path();
            let named = path.file_name()?.to_str()?.to_string();
            named.ends_with(".jpg").then_some((named, path))
        })
        .collect();
    out.sort();
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::record::HEADER;

    fn scratch(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("readinglog-backup-{name}"));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn a_backup_is_named_without_a_colon() {
        assert_eq!(
            name(Kind::Record, "260906:010231", ""),
            "readinglog-260906-010231.zip"
        );
        assert_eq!(
            name(Kind::Book, "260906:010231", "B00OKPCRLG"),
            "book-260906-010231-B00OKPCRLG.zip"
        );
    }

    /// The backup was named for `Record::watermark`, the newest log line read. Two
    /// written under one watermark landed on one path, and `archive::write` renames
    /// onto it.
    #[test]
    fn two_backups_of_one_second_are_two_files() {
        let dir = scratch("collide");
        let record = read_one(&dir);
        let first = keep_record(&dir, &record, "260810:120000", true).unwrap();
        let second = keep_record(&dir, &record, "260810:120000", true).unwrap();
        assert_ne!(first, second);
        assert_eq!(list(&dir).len(), 2);
        assert!(peek(&first).is_ok() && peek(&second).is_ok());

        // Two books cleared under one stamp: the name carries the book.
        let one = record.one_book(148_207, "B00OKPCRLG");
        let other = Record::from_text(&format!(
            "{HEADER}\n\
             b\t999\tB0OTHER0001\tAnother\tSomeone\t\t\t10.000000\t0\t\t\t0\t\t-1\tEBOK\n"
        ));
        let a = keep_book(&dir, &one, "260810:120000").unwrap();
        let b = keep_book(&dir, &other, "260810:120000").unwrap();
        assert_ne!(a, b);
        assert_eq!(peek(&a).unwrap().books[0].title, "A Book");
        assert_eq!(peek(&b).unwrap().books[0].title, "Another");
        assert_eq!(list(&dir).len(), 4);
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// A name a `Kind::Book` tag or a [`free`] step made longer reads back as
    /// the stamp alone, and one no stamp fits reads as itself.
    #[test]
    fn a_stamp_is_read_out_of_whatever_the_name_carries() {
        let dir = scratch("stamps");
        std::fs::create_dir_all(self::dir(&dir)).unwrap();
        for named in [
            "readinglog-260901-120000.zip",
            "readinglog-260901-120000-2.zip",
            "book-260905-090000-B00OKPCRLG.zip",
            "readinglog-nonsense.zip",
        ] {
            std::fs::write(self::dir(&dir).join(named), b"x").unwrap();
        }
        let held = list(&dir);
        assert_eq!(
            held.iter().map(|b| b.stamp.as_str()).collect::<Vec<_>>(),
            [
                "nonsense",
                "260905-090000",
                "260901-120000",
                "260901-120000",
            ]
        );
        assert_eq!(held[1].kind, Kind::Book);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn the_list_reads_both_kinds_newest_first() {
        let dir = scratch("listed");
        std::fs::create_dir_all(self::dir(&dir)).unwrap();
        for named in [
            "readinglog-260901-120000.zip",
            "book-260906-010231.zip",
            "readinglog-260905-090000.zip",
            "notes.txt",
        ] {
            std::fs::write(self::dir(&dir).join(named), b"x").unwrap();
        }
        let held = list(&dir);
        assert_eq!(
            held.iter().map(|b| b.stamp.as_str()).collect::<Vec<_>>(),
            ["260906-010231", "260905-090000", "260901-120000"]
        );
        assert_eq!(held[0].kind, Kind::Book);
        assert_eq!(held[1].kind, Kind::Record);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn the_list_of_a_directory_that_is_not_there_is_empty() {
        assert!(list(Path::new("/nowhere/at/all")).is_empty());
    }

    /// A record of one book with one sitting, and a cover on disk for it.
    fn read_one(dir: &Path) -> Record {
        let record = Record::from_text(&format!(
            "{HEADER}\n\
             m\t260810:120000\n\
             b\t148207\tB00OKPCRLG\tA Book\tAn Author\t\t\t62.000000\t1\t\t\t0\t\t-1\tEBOK\n\
             s\t2026-08-07T10:15:01\t2026-08-07T10:55:43\t148207\t2400\t40\t0\ttimed\t\t\t\n"
        ));
        std::fs::create_dir_all(dir.join(covers::COVERS_DIR)).unwrap();
        // 64 bytes opening as a JPEG, which is what `covers::held` asks.
        let mut cover = b"\xff\xd8\xff\xe0\x00\x10JFIF\0".to_vec();
        cover.resize(64, 0);
        std::fs::write(covers::path(dir, "B00OKPCRLG"), &cover).unwrap();
        record.save(dir).unwrap();
        record
    }

    #[test]
    fn backing_up_first_keeps_the_record_and_its_covers() {
        let dir = scratch("kept");
        let mut record = read_one(&dir);
        let at = reset(&dir, &mut record, Keep::Backup)
            .expect("a backup")
            .expect("a path");

        assert!(record.sittings.is_empty(), "the record was not emptied");
        assert_eq!(record.reset_at, "260810:120000");
        assert!(
            covers::held(&dir, "B00OKPCRLG"),
            "the covers were kept on disk"
        );

        let back = peek(&at).expect("a readable backup");
        assert_eq!(back.sittings.len(), 1);
        assert_eq!(back.books[0].title, "A Book");
        // `Record::load` reads the emptied record.
        assert!(Record::load(&dir).sittings.is_empty());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn resetting_without_a_backup_takes_the_covers_and_writes_nothing() {
        let dir = scratch("nobackup");
        let mut record = read_one(&dir);
        assert_eq!(
            reset(&dir, &mut record, Keep::Nothing).expect("a reset"),
            None
        );

        assert!(record.sittings.is_empty());
        assert!(list(&dir).is_empty(), "a backup was written anyway");
        assert!(!covers::held(&dir, "B00OKPCRLG"), "a cover was left");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_reset_that_cannot_write_its_backup_does_not_happen() {
        let dir = scratch("blocked");
        let mut record = read_one(&dir);
        let before = record.clone();
        // A file at the path `archive::write` needs a directory for.
        std::fs::write(self::dir(&dir), b"in the way").unwrap();

        assert!(reset(&dir, &mut record, Keep::Backup).is_err());
        assert_eq!(record, before, "the record was emptied anyway");
        assert_eq!(Record::load(&dir).sittings.len(), 1);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_record_that_has_read_nothing_is_not_reset() {
        let dir = scratch("unread");
        let mut record = Record::default();
        assert_eq!(
            reset(&dir, &mut record, Keep::Backup).expect("nothing"),
            None
        );
        assert!(list(&dir).is_empty());
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// A record holding two rows of one book closing on one second, on disk.
    fn read_nested(dir: &Path) -> Record {
        let record = Record::from_text(&format!(
            "{HEADER}\n\
             s\t2026-09-05T18:03:38\t2026-09-05T18:21:43\t226229\t776\t10\t0\ttimed\t\t\t\n\
             s\t2026-09-05T18:15:02\t2026-09-05T18:21:43\t226229\t131\t2\t0\ttimed\t\t\t\n"
        ));
        record.save(dir).unwrap();
        record
    }

    #[test]
    fn nested_sittings_are_backed_up_then_dropped_once() {
        let dir = scratch("nested");
        let mut record = read_nested(&dir);
        let (went, at) = drop_nested(&dir, &mut record)
            .expect("a backup")
            .expect("a drop");
        assert_eq!(went, 1);
        assert_eq!(peek(&at).expect("a readable backup").sittings.len(), 2);
        assert_eq!(
            Record::load(&dir).sittings.len(),
            1,
            "the drop was not saved"
        );
        assert_eq!(record.sittings[0].seconds, 776);

        // A second `drop_nested` finds none, and writes no backup.
        assert_eq!(drop_nested(&dir, &mut record).expect("nothing"), None);
        assert_eq!(list(&dir).len(), 1);

        // `restore` of that backup takes no nested row back.
        let merged = restore(&dir, &at, &mut record, &mut |_, _| {}).expect("a merge");
        assert_eq!((merged.added, merged.whole), (0, true));
        assert_eq!(record.sittings.len(), 1);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn nested_sittings_stand_where_the_backup_cannot_be_written() {
        let dir = scratch("nested-blocked");
        let mut record = read_nested(&dir);
        let before = record.clone();
        // A file at the path `archive::write` needs a directory for.
        std::fs::write(self::dir(&dir), b"in the way").unwrap();

        assert!(drop_nested(&dir, &mut record).is_err());
        assert_eq!(record, before, "a row went without its backup");
        assert_eq!(Record::load(&dir).sittings.len(), 2);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn restoring_a_backup_twice_restores_it_once() {
        let dir = scratch("twice");
        let mut record = read_one(&dir);
        let at = reset(&dir, &mut record, Keep::Backup).unwrap().unwrap();

        let merged = restore(&dir, &at, &mut record, &mut |_, _| {}).expect("a merge");
        assert_eq!((merged.added, merged.whole), (1, true));
        assert_eq!(record.sittings.len(), 1);
        assert_eq!(record.books.len(), 1);

        // The same rows, from a copy of the backup.
        let again = dir.join("again.zip");
        std::fs::copy(&at, &again).unwrap();
        let merged = restore(&dir, &again, &mut record, &mut |_, _| {}).expect("a merge");
        assert_eq!((merged.added, merged.whole), (0, true));
        assert_eq!(record.sittings.len(), 1, "the sitting came back twice");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_backup_of_a_superseded_record_gives_back_its_sittings() {
        let dir = scratch("superseded");
        let _ = read_one(&dir);
        // `HEADER` replaced by one this build does not open.
        let text = std::fs::read_to_string(Record::file(&dir)).unwrap();
        let stamped = text.replacen(HEADER, "#readinglog\t0", 1);
        let at = dir
            .join(BACKUPS_DIR)
            .join(name(Kind::Record, "260810:120000", ""));
        archive::write(
            &at,
            &[(RECORD.to_string(), Source::Bytes(stamped.as_bytes()))],
        )
        .expect("a backup");

        // `peek` reads every row, through `Record::from_backup`.
        assert!(peek(&at).expect("a readable backup").sittings.len() == 1);
        let mut empty = Record::default();
        let merged = restore(&dir, &at, &mut empty, &mut |_, _| {}).expect("a merge");
        assert_eq!(merged.added, 1, "the era stayed in the file");
        assert!(merged.whole, "and could never be told from a whole one");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn holds_answers_for_a_record_carrying_the_rows_and_one_carrying_none() {
        let dir = scratch("partial");
        let mut record = read_one(&dir);
        let at = reset(&dir, &mut record, Keep::Backup).unwrap().unwrap();

        // `holds` over a record carrying the rows, and over one carrying none.
        let inside = peek(&at).expect("a readable backup");
        assert!(holds(
            &{
                let mut whole = Record::default();
                whole.merge(&inside);
                whole
            },
            &inside
        ));
        assert!(
            !holds(&Record::default(), &inside),
            "an empty record holds none of it"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn backing_up_writes_a_backup_and_takes_nothing_away() {
        let dir = scratch("back-up");
        let record = read_one(&dir);
        let at = back_up(&dir, &record).expect("a backup");

        assert_eq!(peek(&at).expect("a readable backup").sittings.len(), 1);
        assert!(covers::held(&dir, "B00OKPCRLG"), "a cover went");
        let back = Record::load(&dir);
        assert_eq!(back.sittings.len(), 1, "the record was emptied");
        assert_eq!(back.reset_at, record.reset_at, "the reset stamp moved");
        assert_eq!(back.watermark, record.watermark, "the watermark moved");
    }

    #[test]
    fn a_backup_states_the_clock_it_was_written_at_and_what_it_holds() {
        let dir = scratch("about");
        let record = read_one(&dir);
        let at = keep_record(&dir, &record, "260913:103226", true).expect("a backup");

        let said = about(&at).expect("a readable backup");
        assert_eq!(said.written, "260913:103226");
        assert_eq!(said.app, crate::update::VERSION);
        assert_eq!(said.sittings, 1);
        assert_eq!(said.first, "2026-08-07");
        assert_eq!(said.last, "2026-08-07");
        assert!(!said.is_empty());

        // A backup carrying no `ABOUT` entry.
        let older = self::dir(&dir).join("readinglog-260101-000000.zip");
        archive::write(
            &older,
            &[(RECORD.to_string(), Source::Bytes(record.text().as_bytes()))],
        )
        .expect("a backup");
        assert!(about(&older).expect("a readable backup").is_empty());
        assert_eq!(peek(&older).expect("a readable backup").sittings.len(), 1);
    }

    #[test]
    fn a_restored_backup_stands_on_disk() {
        let dir = scratch("kept-after");
        let mut record = read_one(&dir);
        let at = reset(&dir, &mut record, Keep::Backup).unwrap().unwrap();

        let merged = restore(&dir, &at, &mut record, &mut |_, _| {}).expect("a merge");
        assert!(merged.whole, "the record took every row");
        assert!(at.is_file(), "the backup went");
        assert_eq!(list(&dir).len(), 1);
    }

    #[test]
    fn restoring_a_backup_puts_a_missing_cover_back() {
        let dir = scratch("covers");
        let mut record = read_one(&dir);
        let at = reset(&dir, &mut record, Keep::Backup).unwrap().unwrap();
        std::fs::remove_file(covers::path(&dir, "B00OKPCRLG")).unwrap();

        restore(&dir, &at, &mut record, &mut |_, _| {}).expect("a merge");
        assert!(covers::held(&dir, "B00OKPCRLG"), "the cover stayed gone");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_cover_already_held_is_not_written_over() {
        let dir = scratch("held-cover");
        let mut record = read_one(&dir);
        let at = reset(&dir, &mut record, Keep::Backup).unwrap().unwrap();
        std::fs::write(
            covers::path(&dir, "B00OKPCRLG"),
            b"\xff\xd8\xffthe newer copy",
        )
        .unwrap();

        restore(&dir, &at, &mut record, &mut |_, _| {}).expect("a merge");
        assert_eq!(
            std::fs::read(covers::path(&dir, "B00OKPCRLG")).unwrap(),
            b"\xff\xd8\xffthe newer copy"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn one_books_backup_holds_that_book_and_its_cover() {
        let dir = scratch("one-book");
        let mut record = read_one(&dir);
        let one = record.one_book(148_207, "B00OKPCRLG");
        let at = keep_book(&dir, &one, &record.watermark.clone()).expect("a backup");

        assert_eq!(at.file_name().unwrap(), "book-260810-120000-B00OKPCRLG.zip");
        let open = Archive::open(&at).expect("a readable backup");
        assert_eq!(
            open.entries()
                .iter()
                .map(|e| e.path.as_str())
                .collect::<Vec<_>>(),
            ["sessions.tsv", "covers/B00OKPCRLG.jpg"]
        );

        // `restore` after `clear_book` puts the book back.
        record.clear_book(148_207, "B00OKPCRLG");
        assert!(record.sittings.is_empty());
        assert_eq!(
            restore(&dir, &at, &mut record, &mut |_, _| {})
                .expect("a merge")
                .added,
            1
        );
        assert_eq!(record.sittings.len(), 1);
        assert_eq!(record.books[0].title, "A Book");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn the_sizes_are_what_the_two_directories_weigh() {
        let dir = scratch("weighed");
        let mut record = read_one(&dir);
        let (covers, backups) = sizes(&dir);
        assert_eq!(covers, 64);
        assert_eq!(backups, 0);

        let at = reset(&dir, &mut record, Keep::Backup).unwrap().unwrap();
        let (covers, backups) = sizes(&dir);
        assert_eq!(covers, 64, "the covers stayed");
        assert_eq!(backups, std::fs::metadata(&at).unwrap().len());
        let _ = std::fs::remove_dir_all(&dir);
    }
}
