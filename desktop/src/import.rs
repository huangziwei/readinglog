//! A backup into the workspace: kept byte for byte under `backups/` first, then
//! folded into the record by `backup::restore`, the Kindle's own Restore. A fold
//! adds what the record does not hold; the same backup twice adds nothing.

use std::path::{Path, PathBuf};

use anyhow::{Context, Result, anyhow};

use readinglog_native::backup::{self, About, Kind};
use readinglog_native::record::Record;
use readinglog_native::update::archive::{self, Archive, Source};

use crate::workspace::Workspace;

/// The record's name inside a backup, and in the Kindle's own folder.
const RECORD_NAME: &str = "sessions.tsv";

/// The folder the Kindle keeps its cover copies in, inside a backup too.
const PICTURES: &str = readinglog_native::covers::COVERS_DIR;

/// What one import did.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct Imported {
    /// The backup, as kept under `backups/`.
    pub kept: PathBuf,
    /// Whether these bytes were imported before.
    pub again: bool,
    /// What the record gained.
    pub sittings: usize,
    pub books: usize,
    pub highlights: usize,
    pub pictures: usize,
    /// What the backup states about itself.
    pub about: About,
}

/// Import the backup at `from`, a `readinglog-*.zip` or `book-*.zip`.
pub fn backup(ws: &Workspace, from: &Path) -> Result<Imported> {
    // A file carrying no record is refused before it is kept.
    backup::peek(from)
        .map_err(|err| anyhow!("{} is not a Reading Log backup: {err}", from.display()))?;
    let bytes = std::fs::read(from).with_context(|| format!("reading {}", from.display()))?;
    let named = from
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_else(|| "backup.zip".into());
    let (kept, again) = match held(ws, |at| std::fs::read(at).ok().as_ref() == Some(&bytes)) {
        Some(at) => (at, true),
        None => {
            let at = free(&ws.backups(), &named);
            let partial = at.with_extension("partial");
            std::fs::create_dir_all(ws.backups())?;
            std::fs::write(&partial, &bytes)
                .with_context(|| format!("writing {}", partial.display()))?;
            std::fs::rename(&partial, &at).with_context(|| format!("writing {}", at.display()))?;
            (at, false)
        }
    };
    fold(ws, kept, again)
}

/// Import the record of a Kindle mounted at `root`: its volume, or the
/// app's own folder on it. The record and its covers go into one backup laid
/// out as the Kindle lays its own, which is kept and folded as any other.
pub fn kindle(ws: &Workspace, root: &Path) -> Result<Imported> {
    let dir = [
        root.join("extensions").join("readinglog"),
        root.to_path_buf(),
    ]
    .into_iter()
    .find(|dir| Record::file(dir).is_file())
    .ok_or_else(|| anyhow!("{} holds no Reading Log record", root.display()))?;
    let record = Record::file(&dir);
    let bytes = std::fs::read(&record).with_context(|| format!("reading {}", record.display()))?;
    let found = held(ws, |at| inside(at).as_ref() == Some(&bytes));
    if let Some(at) = found {
        return fold(ws, at, true);
    }
    let mut pictures: Vec<PathBuf> = std::fs::read_dir(dir.join(PICTURES))
        .map(|entries| {
            entries
                .flatten()
                .map(|e| e.path())
                .filter(|p| p.is_file() && p.extension().is_some_and(|x| x == "jpg"))
                .collect()
        })
        .unwrap_or_default();
    pictures.sort();
    let names: Vec<String> = pictures
        .iter()
        .map(|p| {
            format!(
                "{PICTURES}/{}",
                p.file_name().unwrap_or_default().to_string_lossy()
            )
        })
        .collect();
    let mut entries: Vec<(String, Source<'_>)> = vec![(RECORD_NAME.into(), Source::Bytes(&bytes))];
    entries.extend(
        names
            .into_iter()
            .zip(&pictures)
            .map(|(name, p)| (name, Source::File(p))),
    );
    let named = backup::name(Kind::Record, &backup::stamped_now(), "kindle");
    let at = free(&ws.backups(), &named);
    archive::write(&at, &entries).with_context(|| format!("writing {}", at.display()))?;
    fold(ws, at, false)
}

/// Fold the backup kept at `kept` into the workspace's record, saving the
/// record only where it moved.
fn fold(ws: &Workspace, kept: PathBuf, again: bool) -> Result<Imported> {
    let dir = ws.record_dir();
    let mut record = ws.record();
    let before = record.text();
    let (books, highlights, pictures) = (
        record.books.len(),
        record.highlights.len(),
        pictures_in(&dir),
    );
    let merged = backup::restore(&dir, &kept, &mut record, &mut |_, _| {})
        .with_context(|| format!("folding in {}", kept.display()))?;
    if record.text() != before {
        record
            .save(&dir)
            .with_context(|| format!("saving the record in {}", dir.display()))?;
    }
    Ok(Imported {
        about: backup::about(&kept).unwrap_or_default(),
        kept,
        again,
        sittings: merged.added,
        books: record.books.len().saturating_sub(books),
        highlights: record.highlights.len().saturating_sub(highlights),
        pictures: pictures_in(&dir).saturating_sub(pictures),
    })
}

/// The first backup under `backups/` that `same` answers true of.
fn held(ws: &Workspace, same: impl Fn(&Path) -> bool) -> Option<PathBuf> {
    let mut kept: Vec<PathBuf> = std::fs::read_dir(ws.backups())
        .ok()?
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.is_file() && p.extension().is_some_and(|x| x == "zip"))
        .collect();
    kept.sort();
    kept.into_iter().find(|at| same(at))
}

/// The record a backup at `at` carries, as bytes.
fn inside(at: &Path) -> Option<Vec<u8>> {
    let mut open = Archive::open(at).ok()?;
    let entry = open
        .entries()
        .iter()
        .find(|e| e.path == RECORD_NAME)?
        .clone();
    open.read(&entry).ok()
}

/// `named` under `dir`, stepping `-2`, `-3` while a file stands there: a
/// backup is never written over another.
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

/// How many cover copies the record's folder holds.
fn pictures_in(dir: &Path) -> usize {
    std::fs::read_dir(dir.join(PICTURES))
        .map(|entries| entries.flatten().filter(|e| e.path().is_file()).count())
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tests::{book, record_of, scratch, sitting};

    /// A Kindle's folder holding `record` and one cover, as the app keeps them.
    fn kindle_dir(name: &str, record: &Record) -> PathBuf {
        let root = scratch(name);
        let dir = root.join("extensions").join("readinglog");
        record.save(&dir).unwrap();
        std::fs::create_dir_all(dir.join(PICTURES)).unwrap();
        std::fs::write(
            dir.join(PICTURES).join("B1.jpg"),
            b"\xff\xd8\xff\xe0 a cover",
        )
        .unwrap();
        root
    }

    fn one() -> Record {
        record_of(
            vec![book(100, "B1", "One")],
            vec![sitting("2026-09-01T09:00:00", 100, 600)],
        )
    }

    #[test]
    fn a_kindle_is_kept_whole_then_folded_and_twice_adds_nothing() {
        let ws = Workspace::create(&scratch("import-kindle")).unwrap();
        let root = kindle_dir("import-kindle-volume", &one());
        let first = kindle(&ws, &root).unwrap();
        assert!(!first.again);
        assert_eq!((first.sittings, first.books, first.pictures), (1, 1, 1));
        assert!(first.kept.starts_with(ws.backups()));
        assert_eq!(ws.record().sittings.len(), 1);

        let second = kindle(&ws, &root).unwrap();
        assert!(second.again);
        assert_eq!(second.kept, first.kept);
        assert_eq!((second.sittings, second.books, second.pictures), (0, 0, 0));
        assert_eq!(std::fs::read_dir(ws.backups()).unwrap().count(), 1);
    }

    #[test]
    fn a_backup_is_kept_byte_for_byte_and_its_reading_added_to_whats_held() {
        let ws = Workspace::create(&scratch("import-zip")).unwrap();
        let root = kindle_dir("import-zip-volume", &one());
        let made = kindle(&ws, &root).unwrap();

        // A second Kindle's backup: one more sitting, on another book.
        let other = Workspace::create(&scratch("import-zip-other")).unwrap();
        let more = record_of(
            vec![book(100, "B1", "One"), book(200, "B2", "Two")],
            vec![
                sitting("2026-09-01T09:00:00", 100, 600),
                sitting("2026-09-03T09:00:00", 200, 900),
            ],
        );
        let zip = kindle(&other, &kindle_dir("import-zip-second", &more))
            .unwrap()
            .kept;
        let added = backup(&ws, &zip).unwrap();
        assert!(!added.again);
        assert_eq!((added.sittings, added.books), (1, 1));
        assert_eq!(
            std::fs::read(&added.kept).unwrap(),
            std::fs::read(&zip).unwrap()
        );
        assert_ne!(added.kept, made.kept);
        assert_eq!(ws.record().sittings.len(), 2);

        let again = backup(&ws, &zip).unwrap();
        assert!(again.again);
        assert_eq!(again.sittings, 0);
    }

    #[test]
    fn a_file_that_is_no_backup_is_refused_and_not_kept() {
        let ws = Workspace::create(&scratch("import-refused")).unwrap();
        let stray = scratch("import-refused-file").join("notes.zip");
        std::fs::write(&stray, "not an archive").unwrap();
        assert!(backup(&ws, &stray).is_err());
        assert_eq!(std::fs::read_dir(ws.backups()).unwrap().count(), 0);
        assert!(kindle(&ws, &scratch("import-refused-empty")).is_err());
    }
}
