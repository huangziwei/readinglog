//! The app's own archives, under [`BACKUPS_DIR`] beside the record. One holds
//! a `sessions.tsv` and the jackets standing when it was written; taking it
//! back is a merge, so an archive can be folded in twice and in any order.
//!
//! Nothing here reaches outside the extension's own directory.

use std::path::{Path, PathBuf};

use crate::covers;
use crate::store::Store;
use crate::update::archive::{self, Archive, Source};

/// The directory holding them, under the `dir` the store lives in.
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

/// An archive's name. The stamp loses the mark's colon: the user partition is
/// FAT, which will not take one in a name.
pub fn name(kind: Kind, mark: &str) -> String {
    format!("{}-{}.zip", kind.stem(), mark.replace(':', "-"))
}

/// Write `store` and, under `covers`, every jacket it holds into an archive
/// named for `mark`. Answers where it landed.
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

/// Write one book's rows, and its jacket where it has one, into an archive
/// named for `mark`. Answers where it landed.
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

/// Write the bytes of a record this build will not read whole into an archive
/// of one entry. Answers where it landed.
pub fn keep_text(dir: &Path, text: &str, mark: &str) -> archive::Result<PathBuf> {
    let at = dir.join(BACKUPS_DIR).join(name(Kind::Record, mark));
    archive::write(&at, &[(RECORD.to_string(), Source::Bytes(text.as_bytes()))])?;
    Ok(at)
}

/// What a whole-record reset keeps.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Keep {
    /// An archive of the record and every jacket, written before the wipe. The
    /// jackets stay on disk as well.
    Archive,
    /// Nothing. The jacket cache goes with the record and the space comes
    /// back.
    Nothing,
}

/// Empty the record and write it. Under [`Keep::Archive`] the archive is
/// written first and a reset that cannot write one does not happen; under
/// [`Keep::Nothing`] the jacket cache is deleted after the record is safely
/// on disk. Answers the archive written, where one was.
///
/// Nothing outside `dir` is touched, and no log is opened at all.
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
        // After the record is down: a crash between the two leaves jackets
        // for books that are gone, which the next pass sweeps up anyway.
        covers::sweep(dir, &[]);
    }
    Ok(kept)
}

/// Every archive under `dir`, newest first. A file this does not recognise is
/// left out of the list and alone on disk.
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

/// The record an archive holds, without touching anything on disk.
pub fn peek(at: &Path) -> archive::Result<Store> {
    let mut open = Archive::open(at)?;
    let entry = open
        .entries()
        .iter()
        .find(|e| e.path == RECORD)
        .cloned()
        .ok_or_else(|| archive::Error::NoMarker(RECORD.to_string()))?;
    let bytes = open.read(&entry)?;
    Ok(Store::from_text(&String::from_utf8_lossy(&bytes)))
}

/// Fold the archive at `at` into `store` and put back any jacket the cache is
/// missing. Answers how many sittings the record did not already hold.
///
/// A jacket already on disk is left alone: it is the copy the app made for
/// itself, and it is no older than the archive's.
pub fn take(dir: &Path, at: &Path, store: &mut Store) -> archive::Result<usize> {
    let mut open = Archive::open(at)?;
    let entries = open.entries().to_vec();
    let mut added = 0;
    for entry in entries.iter().filter(|e| !e.is_dir()) {
        if entry.path == RECORD {
            let bytes = open.read(entry)?;
            added = store.merge(&Store::from_text(&String::from_utf8_lossy(&bytes)));
            continue;
        }
        let Some(named) = entry.path.strip_prefix(&format!("{}/", covers::COVERS_DIR)) else {
            continue;
        };
        // A name of its own, never a path: an archive cannot write outside the
        // jacket cache.
        if named.is_empty() || named.contains('/') || named.starts_with('.') {
            continue;
        }
        let out = dir.join(covers::COVERS_DIR).join(named);
        if out.exists() {
            continue;
        }
        let bytes = open.read(entry)?;
        if let Some(parent) = out.parent() {
            std::fs::create_dir_all(parent)?;
        }
        std::fs::write(&out, &bytes)?;
    }
    Ok(added)
}

/// How many bytes the jacket cache and the archives take under `dir`.
pub fn sizes(dir: &Path) -> (u64, u64) {
    (
        weight(&dir.join(covers::COVERS_DIR)),
        weight(&self::dir(dir)),
    )
}

/// The files directly under `at`, added up. Nothing recurses: neither
/// directory holds another.
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

/// Every jacket under `dir`, as the name it goes into an archive under and the
/// file it is copied from.
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
        // What is on disk is what was emptied, not what the store held before.
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
        // A file where the archive directory has to go.
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

        assert_eq!(take(&dir, &at, &mut store).expect("a merge"), 1);
        assert_eq!(store.sessions.len(), 1);
        assert_eq!(store.books.len(), 1);
        assert_eq!(take(&dir, &at, &mut store).expect("a merge"), 0);
        assert_eq!(store.sessions.len(), 1, "the sitting came back twice");
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

        // And it is the way back: clear the book, take the archive, it stands.
        store.clear_book(148_207, "B00OKPCRLG");
        assert!(store.sessions.is_empty());
        assert_eq!(take(&dir, &at, &mut store).expect("a merge"), 1);
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
