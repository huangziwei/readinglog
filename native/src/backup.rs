//! Archives under [`BACKUPS_DIR`]. Each holds a `sessions.tsv` and the files
//! `covers::COVERS_DIR` held when [`keep_record`] wrote it. [`take`] merges
//! one into a [`Store`].

use std::path::{Path, PathBuf};

use crate::covers;
use crate::store::Store;
use crate::update::archive::{self, Archive, Source};

/// The directory holding them, under `dir`.
pub const BACKUPS_DIR: &str = "backups";

/// The record's name inside an archive.
const RECORD: &str = "sessions.tsv";

/// The manifest's name inside an archive.
const ABOUT: &str = "about.tsv";

/// The device's own model line.
const MODEL: &str = "/proc/device-tree/model";

/// The firmware's version line.
const FIRMWARE: &str = "/etc/prettyversion.txt";

/// What an archive holds.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    /// The whole record, with every jacket held when it was written.
    Record,
    /// One book: its own row, its sittings, and its jacket.
    Book,
}

impl Kind {
    /// What an archive of this kind is named, before its stamp.
    fn stem(self) -> &'static str {
        match self {
            Kind::Record => "readinglog",
            Kind::Book => "book",
        }
    }
}

/// One archive on disk.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Backup {
    pub path: PathBuf,
    /// `YYMMDD-HHMMSS`, as the name spells it.
    pub stamp: String,
    pub kind: Kind,
    pub bytes: u64,
}

/// Where the archives live under `dir`.
pub fn dir(dir: &Path) -> PathBuf {
    dir.join(BACKUPS_DIR)
}

/// The device's clock as the `YYMMDD:HHMMSS` an archive is named for.
pub fn stamped_now() -> String {
    let (days, secs) = crate::date::now();
    crate::log::line::log_stamp(&crate::date::stamp(days, secs)).unwrap_or_default()
}

/// The `stamp` an archive's name carries: `YYMMDD-HHMMSS`, which
/// [`Backup::stamp`] reads back.
const STAMP_LEN: usize = 13;

/// An archive's name: `stamp` with every `:` written as `-`, and `tag` where
/// one names the book. Two archives of one book at one second differ in
/// nothing else, and [`free`] steps them apart.
pub fn name(kind: Kind, stamp: &str, tag: &str) -> String {
    let stamp = stamp.replace(':', "-");
    match tag.is_empty() {
        true => format!("{}-{stamp}.zip", kind.stem()),
        false => format!("{}-{stamp}-{tag}.zip", kind.stem()),
    }
}

/// `named` under `dir`, stepping `-2`, `-3` while a file stands at it.
/// `archive::write` renames onto its path, and an archive is never the file
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

/// What an archive states about itself, beside the record it carries. An
/// archive written without an [`ABOUT`] entry answers every field empty.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct About {
    /// The clock the archive was written at, `YYMMDD:HHMMSS`.
    pub written: String,
    /// `update::VERSION`.
    pub app: String,
    /// [`MODEL`], one line.
    pub device: String,
    /// [`FIRMWARE`], one line.
    pub firmware: String,
    /// The length of `Store::sessions`.
    pub sittings: usize,
    /// The day the first and the last sitting started on, `YYYY-MM-DD`.
    pub first: String,
    pub last: String,
}

impl About {
    /// Whether an archive stated any of this.
    pub fn is_empty(&self) -> bool {
        *self == About::default()
    }
}

/// One line per field, the name and the value tab apart.
fn about_text(store: &Store, stamp: &str) -> String {
    let day = |at: Option<&crate::log::session::Session>| {
        at.map(|s| crate::date::day_of(&s.started_at).to_string())
            .unwrap_or_default()
    };
    [
        ("written", stamp.to_string()),
        ("app", crate::update::VERSION.to_string()),
        ("device", one_line(MODEL)),
        ("firmware", one_line(FIRMWARE)),
        ("sittings", store.sessions.len().to_string()),
        ("first", day(store.sessions.first())),
        ("last", day(store.sessions.last())),
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
            "device" => out.device = value.to_string(),
            "firmware" => out.firmware = value.to_string(),
            "sittings" => out.sittings = value.parse().unwrap_or(0),
            "first" => out.first = value.to_string(),
            "last" => out.last = value.to_string(),
            _ => {}
        }
    }
    out
}

/// Write an archive of `store` and every jacket held, taking nothing away.
/// Answers where it landed.
pub fn export(dir: &Path, store: &Store) -> archive::Result<PathBuf> {
    keep_record(dir, store, &stamped_now(), true)
}

/// Write `store`, and under `jackets` every file in `covers::COVERS_DIR`,
/// into an archive named for `stamp`. Answers where it landed.
pub fn keep_record(
    dir: &Path,
    store: &Store,
    stamp: &str,
    jackets: bool,
) -> archive::Result<PathBuf> {
    let text = store.text();
    let said = about_text(store, stamp);
    let mut entries: Vec<(String, Source<'_>)> = vec![
        (RECORD.to_string(), Source::Bytes(text.as_bytes())),
        (ABOUT.to_string(), Source::Bytes(said.as_bytes())),
    ];
    let held = match jackets {
        true => jackets_under(dir),
        false => Vec::new(),
    };
    for (name, path) in &held {
        entries.push((format!("{}/{name}", covers::COVERS_DIR), Source::File(path)));
    }
    let at = free(&self::dir(dir), &name(Kind::Record, stamp, ""));
    archive::write(&at, &entries)?;
    Ok(at)
}

/// Write `one` and the jacket `covers::path` names, into an archive named for
/// `stamp` and for the book. Answers where it landed.
pub fn keep_book(dir: &Path, one: &Store, stamp: &str) -> archive::Result<PathBuf> {
    let text = one.text();
    let mut entries: Vec<(String, Source<'_>)> =
        vec![(RECORD.to_string(), Source::Bytes(text.as_bytes()))];
    let jacket = one
        .books
        .first()
        .map(|b| covers::path(dir, &b.cde_key))
        .filter(|p| p.is_file());
    if let Some(path) = &jacket {
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
    /// An archive of the record and the jackets in `covers::COVERS_DIR`.
    Archive,
    /// Nothing. `covers::sweep` empties `covers::COVERS_DIR`.
    Nothing,
}

/// [`Store::wipe`] `store` and save it under `dir`. [`Keep::Archive`] writes
/// an archive first and answers `Err` where that write fails. Answers the
/// archive written.
pub fn reset(dir: &Path, store: &mut Store, keep: Keep) -> archive::Result<Option<PathBuf>> {
    if store.mark.is_empty() {
        return Ok(None);
    }
    let kept = match keep {
        Keep::Archive => Some(keep_record(dir, store, &stamped_now(), true)?),
        Keep::Nothing => None,
    };
    store.wipe();
    store.save(dir)?;
    if keep == Keep::Nothing {
        // After `save`: `Store::keep_covers` sweeps what a crash here leaves.
        covers::sweep(dir, &[]);
    }
    Ok(kept)
}

/// Every archive under `dir`, newest first. A name no [`Kind::stem`] opens is
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

/// The record the archive at `at` holds, through [`Store::from_archive`].
pub fn peek(at: &Path) -> archive::Result<Store> {
    let mut open = Archive::open(at)?;
    let entry = open
        .entries()
        .iter()
        .find(|e| e.path == RECORD)
        .cloned()
        .ok_or_else(|| archive::Error::NoMarker(RECORD.to_string()))?;
    let bytes = open.read(&entry)?;
    Ok(Store::from_archive(&String::from_utf8_lossy(&bytes)))
}

/// What taking an archive back did.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Taken {
    /// Sittings [`Store::merge`] added.
    pub added: usize,
    /// Whether `store` holds every row of the archive, and every jacket it
    /// carried is a file on disk.
    pub whole: bool,
}

/// Fold the archive at `at` into `store`, and write each jacket it carries
/// that `covers::COVERS_DIR` does not hold. Answers what landed, reporting
/// files read and files to read as it goes.
pub fn take(
    dir: &Path,
    at: &Path,
    store: &mut Store,
    on: &mut dyn FnMut(usize, usize),
) -> archive::Result<Taken> {
    let mut open = Archive::open(at)?;
    let entries: Vec<_> = open
        .entries()
        .iter()
        .filter(|e| !e.is_dir())
        .cloned()
        .collect();
    let total = entries.len();
    let mut added = 0;
    let mut inside = Store::default();
    let mut jackets: Vec<PathBuf> = Vec::new();
    // `done` counts the entries behind this one; `on(total, total)` closes it.
    for (done, entry) in entries.iter().enumerate() {
        on(done, total);
        if entry.path == RECORD {
            let bytes = open.read(entry)?;
            inside = Store::from_archive(&String::from_utf8_lossy(&bytes));
            added = store.fold_in(&inside);
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
        jackets.push(out.clone());
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
    Ok(Taken {
        added,
        whole: holds(store, &inside) && jackets.iter().all(|j| j.is_file()),
    })
}

/// Whether `store` holds every sitting, end and book row of `inside`, on the
/// identities `Store::sort` de-duplicates by.
pub fn holds(store: &Store, inside: &Store) -> bool {
    let sitting = |a: &crate::log::session::Session| {
        store.sessions.iter().any(|s| {
            s.started_at == a.started_at
                && s.end_position == a.end_position
                && s.ended_at == a.ended_at
        })
    };
    inside.sessions.iter().all(sitting)
        && inside.ends.iter().all(|e| store.ends.contains(e))
        && inside.books.iter().all(|b| {
            store
                .books
                .iter()
                .any(|h| h.extent == b.extent && h.cde_key == b.cde_key)
        })
}

/// Take the archive at `at` off disk. The one call that deletes one.
pub fn remove(at: &Path) -> std::io::Result<()> {
    std::fs::remove_file(at)
}

/// How many bytes the jacket cache and the archives take under `dir`.
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
fn jackets_under(dir: &Path) -> Vec<(String, PathBuf)> {
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
    use crate::store::HEADER;

    fn scratch(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("readinglog-backup-{name}"));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn an_archive_is_named_without_a_colon() {
        assert_eq!(
            name(Kind::Record, "260906:010231", ""),
            "readinglog-260906-010231.zip"
        );
        assert_eq!(
            name(Kind::Book, "260906:010231", "B00OKPCRLG"),
            "book-260906-010231-B00OKPCRLG.zip"
        );
    }

    /// The archive was named for `Store::mark`, the newest log line read. Two
    /// written under one mark landed on one path, and `archive::write` renames
    /// onto it.
    #[test]
    fn two_archives_of_one_second_are_two_files() {
        let dir = scratch("collide");
        let store = read_one(&dir);
        let first = keep_record(&dir, &store, "260810:120000", true).unwrap();
        let second = keep_record(&dir, &store, "260810:120000", true).unwrap();
        assert_ne!(first, second);
        assert_eq!(list(&dir).len(), 2);
        assert!(peek(&first).is_ok() && peek(&second).is_ok());

        // Two books cleared under one stamp: the name carries the book.
        let one = store.one_book(148_207, "B00OKPCRLG");
        let other = Store::from_text(&format!(
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

    /// A record of one book with one sitting, and a jacket on disk for it.
    fn read_one(dir: &Path) -> Store {
        let store = Store::from_text(&format!(
            "{HEADER}\n\
             m\t260810:120000\n\
             b\t148207\tB00OKPCRLG\tA Book\tAn Author\t\t\t62.000000\t1\t\t\t0\t\t-1\tEBOK\n\
             s\t2026-08-07T10:15:01\t2026-08-07T10:55:43\t148207\t2400\t40\t0\ttimed\t\t\t\n"
        ));
        std::fs::create_dir_all(dir.join(covers::COVERS_DIR)).unwrap();
        // 64 bytes opening as a JPEG, which is what `covers::held` asks.
        let mut jacket = b"\xff\xd8\xff\xe0\x00\x10JFIF\0".to_vec();
        jacket.resize(64, 0);
        std::fs::write(covers::path(dir, "B00OKPCRLG"), &jacket).unwrap();
        store.save(dir).unwrap();
        store
    }

    #[test]
    fn backing_up_first_keeps_the_record_and_its_jackets() {
        let dir = scratch("kept");
        let mut store = read_one(&dir);
        let at = reset(&dir, &mut store, Keep::Archive)
            .expect("an archive")
            .expect("a path");

        assert!(store.sessions.is_empty(), "the record was not emptied");
        assert_eq!(store.floor, "260810:120000");
        assert!(
            covers::held(&dir, "B00OKPCRLG"),
            "the jackets were kept on disk"
        );

        let back = peek(&at).expect("a readable archive");
        assert_eq!(back.sessions.len(), 1);
        assert_eq!(back.books[0].title, "A Book");
        // `Store::load` reads the emptied record.
        assert!(Store::load(&dir).sessions.is_empty());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn resetting_without_a_backup_takes_the_jackets_and_writes_nothing() {
        let dir = scratch("nobackup");
        let mut store = read_one(&dir);
        assert_eq!(
            reset(&dir, &mut store, Keep::Nothing).expect("a reset"),
            None
        );

        assert!(store.sessions.is_empty());
        assert!(list(&dir).is_empty(), "an archive was written anyway");
        assert!(!covers::held(&dir, "B00OKPCRLG"), "a jacket was left");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_reset_that_cannot_write_its_archive_does_not_happen() {
        let dir = scratch("blocked");
        let mut store = read_one(&dir);
        let before = store.clone();
        // A file at the path `archive::write` needs a directory for.
        std::fs::write(self::dir(&dir), b"in the way").unwrap();

        assert!(reset(&dir, &mut store, Keep::Archive).is_err());
        assert_eq!(store, before, "the record was emptied anyway");
        assert_eq!(Store::load(&dir).sessions.len(), 1);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_record_that_has_read_nothing_is_not_reset() {
        let dir = scratch("unread");
        let mut store = Store::default();
        assert_eq!(
            reset(&dir, &mut store, Keep::Archive).expect("nothing"),
            None
        );
        assert!(list(&dir).is_empty());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn taking_an_archive_back_twice_takes_it_back_once() {
        let dir = scratch("twice");
        let mut store = read_one(&dir);
        let at = reset(&dir, &mut store, Keep::Archive).unwrap().unwrap();

        let taken = take(&dir, &at, &mut store, &mut |_, _| {}).expect("a merge");
        assert_eq!((taken.added, taken.whole), (1, true));
        assert_eq!(store.sessions.len(), 1);
        assert_eq!(store.books.len(), 1);

        // The same rows, from a copy of the archive.
        let again = dir.join("again.zip");
        std::fs::copy(&at, &again).unwrap();
        let taken = take(&dir, &again, &mut store, &mut |_, _| {}).expect("a merge");
        assert_eq!((taken.added, taken.whole), (0, true));
        assert_eq!(store.sessions.len(), 1, "the sitting came back twice");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn an_archive_of_a_superseded_record_gives_back_its_sittings() {
        let dir = scratch("superseded");
        let _ = read_one(&dir);
        // `HEADER` replaced by one this build does not open.
        let text = std::fs::read_to_string(Store::file(&dir)).unwrap();
        let stamped = text.replacen(HEADER, "#readinglog\t0", 1);
        let at = dir
            .join(BACKUPS_DIR)
            .join(name(Kind::Record, "260810:120000", ""));
        archive::write(
            &at,
            &[(RECORD.to_string(), Source::Bytes(stamped.as_bytes()))],
        )
        .expect("an archive");

        // `peek` reads every row, through `Store::from_archive`.
        assert!(peek(&at).expect("a readable archive").sessions.len() == 1);
        let mut empty = Store::default();
        let taken = take(&dir, &at, &mut empty, &mut |_, _| {}).expect("a merge");
        assert_eq!(taken.added, 1, "the era stayed in the file");
        assert!(taken.whole, "and could never be told from a whole one");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn holds_answers_for_a_record_carrying_the_rows_and_one_carrying_none() {
        let dir = scratch("partial");
        let mut store = read_one(&dir);
        let at = reset(&dir, &mut store, Keep::Archive).unwrap().unwrap();

        // `holds` over a record carrying the rows, and over one carrying none.
        let inside = peek(&at).expect("a readable archive");
        assert!(holds(
            &{
                let mut whole = Store::default();
                whole.merge(&inside);
                whole
            },
            &inside
        ));
        assert!(
            !holds(&Store::default(), &inside),
            "an empty record holds none of it"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn an_export_writes_an_archive_and_takes_nothing_away() {
        let dir = scratch("export");
        let store = read_one(&dir);
        let at = export(&dir, &store).expect("an archive");

        assert_eq!(peek(&at).expect("a readable archive").sessions.len(), 1);
        assert!(covers::held(&dir, "B00OKPCRLG"), "a jacket went");
        let back = Store::load(&dir);
        assert_eq!(back.sessions.len(), 1, "the record was emptied");
        assert_eq!(back.floor, store.floor, "the floor moved");
        assert_eq!(back.mark, store.mark, "the mark moved");
    }

    #[test]
    fn an_archive_states_the_clock_it_was_written_at_and_what_it_holds() {
        let dir = scratch("about");
        let store = read_one(&dir);
        let at = keep_record(&dir, &store, "260913:103226", true).expect("an archive");

        let said = about(&at).expect("a readable archive");
        assert_eq!(said.written, "260913:103226");
        assert_eq!(said.app, crate::update::VERSION);
        assert_eq!(said.sittings, 1);
        assert_eq!(said.first, "2026-08-07");
        assert_eq!(said.last, "2026-08-07");
        assert!(!said.is_empty());

        // An archive carrying no `ABOUT` entry.
        let older = self::dir(&dir).join("readinglog-260101-000000.zip");
        archive::write(
            &older,
            &[(RECORD.to_string(), Source::Bytes(store.text().as_bytes()))],
        )
        .expect("an archive");
        assert!(about(&older).expect("a readable archive").is_empty());
        assert_eq!(peek(&older).expect("a readable archive").sessions.len(), 1);
    }

    #[test]
    fn an_archive_taken_back_stands_on_disk() {
        let dir = scratch("kept-after");
        let mut store = read_one(&dir);
        let at = reset(&dir, &mut store, Keep::Archive).unwrap().unwrap();

        let taken = take(&dir, &at, &mut store, &mut |_, _| {}).expect("a merge");
        assert!(taken.whole, "the record took every row");
        assert!(at.is_file(), "the archive went");
        assert_eq!(list(&dir).len(), 1);
    }

    #[test]
    fn taking_an_archive_back_puts_a_missing_jacket_back() {
        let dir = scratch("jackets");
        let mut store = read_one(&dir);
        let at = reset(&dir, &mut store, Keep::Archive).unwrap().unwrap();
        std::fs::remove_file(covers::path(&dir, "B00OKPCRLG")).unwrap();

        take(&dir, &at, &mut store, &mut |_, _| {}).expect("a merge");
        assert!(covers::held(&dir, "B00OKPCRLG"), "the jacket stayed gone");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_jacket_already_held_is_not_written_over() {
        let dir = scratch("held-jacket");
        let mut store = read_one(&dir);
        let at = reset(&dir, &mut store, Keep::Archive).unwrap().unwrap();
        std::fs::write(
            covers::path(&dir, "B00OKPCRLG"),
            b"\xff\xd8\xffthe newer copy",
        )
        .unwrap();

        take(&dir, &at, &mut store, &mut |_, _| {}).expect("a merge");
        assert_eq!(
            std::fs::read(covers::path(&dir, "B00OKPCRLG")).unwrap(),
            b"\xff\xd8\xffthe newer copy"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn one_books_archive_holds_that_book_and_its_jacket() {
        let dir = scratch("one-book");
        let mut store = read_one(&dir);
        let one = store.one_book(148_207, "B00OKPCRLG");
        let at = keep_book(&dir, &one, &store.mark.clone()).expect("an archive");

        assert_eq!(at.file_name().unwrap(), "book-260810-120000-B00OKPCRLG.zip");
        let open = Archive::open(&at).expect("a readable archive");
        assert_eq!(
            open.entries()
                .iter()
                .map(|e| e.path.as_str())
                .collect::<Vec<_>>(),
            ["sessions.tsv", "covers/B00OKPCRLG.jpg"]
        );

        // `take` after `clear_book` puts the book back.
        store.clear_book(148_207, "B00OKPCRLG");
        assert!(store.sessions.is_empty());
        assert_eq!(
            take(&dir, &at, &mut store, &mut |_, _| {})
                .expect("a merge")
                .added,
            1
        );
        assert_eq!(store.sessions.len(), 1);
        assert_eq!(store.books[0].title, "A Book");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn the_sizes_are_what_the_two_directories_weigh() {
        let dir = scratch("weighed");
        let mut store = read_one(&dir);
        let (jackets, archives) = sizes(&dir);
        assert_eq!(jackets, 64);
        assert_eq!(archives, 0);

        let at = reset(&dir, &mut store, Keep::Archive).unwrap().unwrap();
        let (jackets, archives) = sizes(&dir);
        assert_eq!(jackets, 64, "the jackets stayed");
        assert_eq!(archives, std::fs::metadata(&at).unwrap().len());
        let _ = std::fs::remove_dir_all(&dir);
    }
}
