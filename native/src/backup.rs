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

/// An archive's name: `mark` with every `:` written as `-`.
pub fn name(kind: Kind, mark: &str) -> String {
    format!("{}-{}.zip", kind.stem(), mark.replace(':', "-"))
}

/// Write `store`, and under `jackets` every file in `covers::COVERS_DIR`,
/// into an archive named for `mark`. Answers where it landed.
pub fn keep_record(
    dir: &Path,
    store: &Store,
    mark: &str,
    jackets: bool,
) -> archive::Result<PathBuf> {
    let text = store.text();
    let mut entries: Vec<(String, Source<'_>)> =
        vec![(RECORD.to_string(), Source::Bytes(text.as_bytes()))];
    let held = match jackets {
        true => jackets_under(dir),
        false => Vec::new(),
    };
    for (name, path) in &held {
        entries.push((format!("{}/{name}", covers::COVERS_DIR), Source::File(path)));
    }
    let at = dir.join(BACKUPS_DIR).join(name(Kind::Record, mark));
    archive::write(&at, &entries)?;
    Ok(at)
}

/// Write `one` and the jacket `covers::path` names, into an archive named
/// for `mark`. Answers where it landed.
pub fn keep_book(dir: &Path, one: &Store, mark: &str) -> archive::Result<PathBuf> {
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
    let at = dir.join(BACKUPS_DIR).join(name(Kind::Book, mark));
    archive::write(&at, &entries)?;
    Ok(at)
}

/// Write `text` into an archive of one entry, named for `mark`. Answers
/// where it landed.
pub fn keep_text(dir: &Path, text: &str, mark: &str) -> archive::Result<PathBuf> {
    let at = dir.join(BACKUPS_DIR).join(name(Kind::Record, mark));
    archive::write(&at, &[(RECORD.to_string(), Source::Bytes(text.as_bytes()))])?;
    Ok(at)
}

/// What a whole-record reset keeps.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Keep {
    /// An archive of the record and every jacket. `covers::COVERS_DIR` keeps
    /// its files.
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
        Keep::Archive => Some(keep_record(dir, store, &store.mark.clone(), true)?),
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
            Some(Backup {
                stamp: stem[kind.stem().len() + 1..].to_string(),
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
/// that `covers::COVERS_DIR` does not hold. Answers what landed.
pub fn take(dir: &Path, at: &Path, store: &mut Store) -> archive::Result<Taken> {
    let mut open = Archive::open(at)?;
    let entries = open.entries().to_vec();
    let mut added = 0;
    let mut inside = Store::default();
    let mut jackets: Vec<PathBuf> = Vec::new();
    for entry in entries.iter().filter(|e| !e.is_dir()) {
        if entry.path == RECORD {
            let bytes = open.read(entry)?;
            inside = Store::from_archive(&String::from_utf8_lossy(&bytes));
            added = store.merge(&inside);
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
    Ok(Taken {
        added,
        whole: holds(store, &inside) && jackets.iter().all(|j| j.is_file()),
    })
}

/// Whether `store` holds every sitting, end and book row of `inside`, on the
/// identities `Store::sort` de-duplicates by.
fn holds(store: &Store, inside: &Store) -> bool {
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

    fn scratch(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("readinglog-backup-{name}"));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn an_archive_is_named_without_a_colon() {
        assert_eq!(
            name(Kind::Record, "260906:010231"),
            "readinglog-260906-010231.zip"
        );
        assert_eq!(name(Kind::Book, "260906:010231"), "book-260906-010231.zip");
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
        let store = Store::from_text(
            "#readinglog\t2\n\
             m\t260810:120000\n\
             b\t148207\tB00OKPCRLG\tA Book\tAn Author\t\t\t62.000000\t1\t\t\t0\t\t-1\tEBOK\n\
             s\t2026-08-07T10:15:01\t2026-08-07T10:55:43\t148207\t2400\t40\t0\ttimed\t\t\t\n",
        );
        std::fs::create_dir_all(dir.join(covers::COVERS_DIR)).unwrap();
        std::fs::write(covers::path(dir, "B00OKPCRLG"), vec![0xFFu8; 64]).unwrap();
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

        let taken = take(&dir, &at, &mut store).expect("a merge");
        assert_eq!((taken.added, taken.whole), (1, true));
        assert_eq!(store.sessions.len(), 1);
        assert_eq!(store.books.len(), 1);

        // The same rows, from a copy of the archive.
        let again = dir.join("again.zip");
        std::fs::copy(&at, &again).unwrap();
        let taken = take(&dir, &again, &mut store).expect("a merge");
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
        let at = keep_text(
            &dir,
            &text.replacen("#readinglog\t2", "#readinglog\t1", 1),
            "260810:120000",
        )
        .expect("an archive");

        // `peek` reads every row, through `Store::from_archive`.
        assert!(peek(&at).expect("a readable archive").sessions.len() == 1);
        let mut empty = Store::default();
        let taken = take(&dir, &at, &mut empty).expect("a merge");
        assert_eq!(taken.added, 1, "the era stayed in the file");
        assert!(taken.whole, "and could never be told from a whole one");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn an_archive_the_record_did_not_take_whole_is_not_the_apps_to_delete() {
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
    fn taking_an_archive_back_puts_a_missing_jacket_back() {
        let dir = scratch("jackets");
        let mut store = read_one(&dir);
        let at = reset(&dir, &mut store, Keep::Archive).unwrap().unwrap();
        std::fs::remove_file(covers::path(&dir, "B00OKPCRLG")).unwrap();

        take(&dir, &at, &mut store).expect("a merge");
        assert!(covers::held(&dir, "B00OKPCRLG"), "the jacket stayed gone");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_jacket_already_held_is_not_written_over() {
        let dir = scratch("held-jacket");
        let mut store = read_one(&dir);
        let at = reset(&dir, &mut store, Keep::Archive).unwrap().unwrap();
        std::fs::write(covers::path(&dir, "B00OKPCRLG"), b"the newer copy").unwrap();

        take(&dir, &at, &mut store).expect("a merge");
        assert_eq!(
            std::fs::read(covers::path(&dir, "B00OKPCRLG")).unwrap(),
            b"the newer copy"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn one_books_archive_holds_that_book_and_its_jacket() {
        let dir = scratch("one-book");
        let mut store = read_one(&dir);
        let one = store.one_book(148_207, "B00OKPCRLG");
        let at = keep_book(&dir, &one, &store.mark.clone()).expect("an archive");

        assert_eq!(at.file_name().unwrap(), "book-260810-120000.zip");
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
        assert_eq!(take(&dir, &at, &mut store).expect("a merge").added, 1);
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
