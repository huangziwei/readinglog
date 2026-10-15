//! An import into the workspace: a Kindle read over [`crate::device`], kept
//! under its serial, or a backup, kept where no Kindle is named. Each is kept
//! byte for byte under a `backups/` first, then folded into the record beside
//! it: a Kindle's newest record stands whole with its earlier imports folded
//! under it, and a backup naming no Kindle folds by `backup::restore`, the
//! Kindle's own Restore. The same import twice adds nothing.

use std::path::{Path, PathBuf};

use anyhow::{Context, Result, anyhow};

use readinglog_native::backup::{self, About, Kind};
use readinglog_native::record::Record;
use readinglog_native::update::archive::{self, Archive, Source};

use crate::device::mass_storage::transport::MassStorageTransport;
use crate::device::{DeviceInfo, TPath, Transport};
use crate::workspace::{BACKUPS, RECORD, Workspace};

/// The record's name inside a backup, and in the Kindle's own folder.
const RECORD_NAME: &str = "sessions.tsv";

/// The folder the Kindle keeps its cover copies in, inside a backup too.
const PICTURES: &str = readinglog_native::covers::COVERS_DIR;

/// The app's own folder on a Kindle, from the volume or storage root.
const APP_DIR: &str = "extensions/readinglog";

/// What one import did.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct Imported {
    /// The backup, as kept under a `backups/`.
    pub kept: PathBuf,
    /// Whether these bytes were imported before.
    pub again: bool,
    /// What the record the screens draw gained.
    pub sittings: usize,
    pub books: usize,
    pub highlights: usize,
    /// What the record beside the backup gained in cover copies.
    pub pictures: usize,
    /// What the backup states about itself.
    pub about: About,
}

/// Import the backup at `from`, a `readinglog-*.zip` or `book-*.zip`, where no
/// Kindle is named.
pub fn backup(ws: &Workspace, from: &Path) -> Result<Imported> {
    // A file carrying no record is refused before it is kept.
    backup::peek(from)
        .map_err(|err| anyhow!("{} is not a Reading Log backup: {err}", from.display()))?;
    let bytes = std::fs::read(from).with_context(|| format!("reading {}", from.display()))?;
    let named = from
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_else(|| "backup.zip".into());
    let backups = ws.backups();
    let (kept, again) = match held(&backups, |at| {
        std::fs::read(at).ok().as_ref() == Some(&bytes)
    }) {
        Some(at) => (at, true),
        None => {
            let at = free(&backups, &named);
            let partial = at.with_extension("partial");
            std::fs::create_dir_all(&backups)?;
            std::fs::write(&partial, &bytes)
                .with_context(|| format!("writing {}", partial.display()))?;
            std::fs::rename(&partial, &at).with_context(|| format!("writing {}", at.display()))?;
            (at, false)
        }
    };
    settle(ws, &ws.dir, kept, again, Fold::Restore)
}

/// Import the Kindle `device` names: its record and covers, read over its
/// transport, kept and folded under its serial. Nothing is written to it.
pub fn kindle(ws: &Workspace, device: &DeviceInfo) -> Result<Imported> {
    let transport = device.open_transport()?;
    take(ws, Some(&device.serial), transport.as_ref())
}

/// Import the folder `dir` dropped on the window: a Kindle's volume, or the
/// app's own folder on one. The volume `plugged` is mounted at is kept under
/// that Kindle's serial; any other folder names no Kindle.
pub fn folder(ws: &Workspace, dir: &Path, plugged: Option<&DeviceInfo>) -> Result<Imported> {
    let serial = plugged
        .filter(|d| d.mass_storage_mount().as_deref() == Some(dir))
        .map(|d| d.serial.as_str());
    take(ws, serial, &MassStorageTransport::new(dir.to_path_buf()))
}

/// The record and covers `transport` reads, kept in one backup laid out as the
/// Kindle lays its own and folded: under `serial`'s Kindle, or where no Kindle
/// is named.
fn take(ws: &Workspace, serial: Option<&str>, transport: &dyn Transport) -> Result<Imported> {
    let app = [TPath::parse(APP_DIR), TPath::new()]
        .into_iter()
        .find(|at| transport.exists(&at.join(RECORD_NAME)).unwrap_or(false))
        .ok_or_else(|| {
            anyhow!(
                "{} holds no Reading Log record",
                transport.display_path(&TPath::parse(APP_DIR))
            )
        })?;
    let bytes = transport.read(&app.join(RECORD_NAME))?;
    let (group, fold) = match serial {
        Some(serial) => (ws.kindle_dir(serial), Fold::Newest),
        None => (ws.dir.clone(), Fold::Restore),
    };
    let backups = group.join(BACKUPS);
    if let Some(at) = held(&backups, |at| inside(at).as_ref() == Some(&bytes)) {
        return settle(ws, &group, at, true, fold);
    }
    let mut pictures: Vec<(String, Vec<u8>)> = transport
        .read_files_in_children(&app, &|d| d == PICTURES, &|f| f.ends_with(".jpg"))?
        .into_iter()
        .flat_map(|(_, files)| files)
        .collect();
    pictures.sort();
    let mut entries: Vec<(String, Source<'_>)> = vec![(RECORD_NAME.into(), Source::Bytes(&bytes))];
    entries.extend(
        pictures
            .iter()
            .map(|(name, picture)| (format!("{PICTURES}/{name}"), Source::Bytes(picture))),
    );
    std::fs::create_dir_all(&backups)?;
    let named = backup::name(Kind::Record, &backup::stamped_now(), "kindle");
    let at = free(&backups, &named);
    archive::write(&at, &entries).with_context(|| format!("writing {}", at.display()))?;
    settle(ws, &group, at, false, fold)
}

/// How a backup folds into the record beside it.
enum Fold {
    /// `backup::restore`, the Kindle's own Restore, under which the record
    /// held keeps each book's state: for backups naming no Kindle, which may
    /// come from several.
    Restore,
    /// [`newest`]: for one Kindle's imports, the newest of which states what
    /// that Kindle holds now.
    Newest,
}

/// Fold the backup kept at `kept` into the record under `group`, saving it only
/// where it moved, and count what the record the screens draw gained.
fn settle(
    ws: &Workspace,
    group: &Path,
    kept: PathBuf,
    again: bool,
    fold: Fold,
) -> Result<Imported> {
    let dir = group.join(RECORD);
    let before = ws.record();
    let pictures = pictures_in(&dir);
    let mut record = Record::load(&dir);
    let was = record.text();
    match fold {
        Fold::Restore => {
            backup::restore(&dir, &kept, &mut record, &mut |_, _| {})
                .with_context(|| format!("folding in {}", kept.display()))?;
        }
        Fold::Newest => {
            newest(&dir, &kept, &mut record)
                .with_context(|| format!("folding in {}", kept.display()))?;
        }
    }
    if record.text() != was {
        record
            .save(&dir)
            .with_context(|| format!("saving the record in {}", dir.display()))?;
    }
    let after = ws.record();
    Ok(Imported {
        about: backup::about(&kept).unwrap_or_default(),
        kept,
        again,
        sittings: after.sittings.len().saturating_sub(before.sittings.len()),
        books: after.books.len().saturating_sub(before.books.len()),
        highlights: after
            .highlights
            .len()
            .saturating_sub(before.highlights.len()),
        pictures: pictures_in(&dir).saturating_sub(pictures),
    })
}

/// Fold the backup at `at` into `record`, a Kindle's record under `dir`. Of
/// the two, the one whose watermark is newer stands whole, its books' rows and
/// its `e` rows with it, and `Record::merge` folds the other under it, which
/// fills only what the standing one states none of: a Kindle's rows only widen
/// from one record to the next. Each cover the backup carries that `dir` does
/// not hold is written.
fn newest(dir: &Path, at: &Path, record: &mut Record) -> Result<()> {
    let text = inside(at).ok_or_else(|| anyhow!("{} carries no record", at.display()))?;
    let carried = Record::from_backup(&String::from_utf8_lossy(&text));
    let held = std::mem::take(record);
    let (mut newer, older) = match carried.watermark >= held.watermark {
        true => (carried, held),
        false => (held, carried),
    };
    let ends = newer.ends.clone();
    newer.merge(&older);
    for (key, end) in ends {
        if let Ok(i) = newer.ends.binary_search_by_key(&key, |(k, _)| *k) {
            newer.ends[i].1 = end;
        }
    }
    *record = newer;
    take_pictures(dir, at)
}

/// Write each cover the backup at `at` carries that `dir` does not hold, by its
/// name alone, never a path.
fn take_pictures(dir: &Path, at: &Path) -> Result<()> {
    let mut open = Archive::open(at)?;
    let entries: Vec<_> = open
        .entries()
        .iter()
        .filter(|e| !e.is_dir())
        .cloned()
        .collect();
    for entry in &entries {
        let Some(named) = entry.path.strip_prefix(&format!("{PICTURES}/")) else {
            continue;
        };
        if named.is_empty() || named.contains('/') || named.starts_with('.') {
            continue;
        }
        let out = dir.join(PICTURES).join(named);
        if out.exists() {
            continue;
        }
        let bytes = open.read(entry)?;
        std::fs::create_dir_all(dir.join(PICTURES))?;
        std::fs::write(&out, &bytes).with_context(|| format!("writing {}", out.display()))?;
    }
    Ok(())
}

/// The first backup under `dir` that `same` answers true of.
fn held(dir: &Path, same: impl Fn(&Path) -> bool) -> Option<PathBuf> {
    let mut kept: Vec<PathBuf> = std::fs::read_dir(dir)
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
    use crate::device::TransportKind;
    use crate::tests::{book, record_of, scratch, sitting};

    /// A Kindle's volume holding `record` and one cover, as the app keeps
    /// them, and the firmware's version line.
    fn kindle_dir(name: &str, record: &Record) -> PathBuf {
        let root = scratch(name);
        std::fs::create_dir_all(root.join("system")).unwrap();
        std::fs::write(
            root.join("system").join("version.txt"),
            "Kindle 5.16.2.1.1\n",
        )
        .unwrap();
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

    /// The Kindle `serial` names, mounted at `root`.
    fn plugged(root: &Path, serial: &str) -> DeviceInfo {
        DeviceInfo {
            serial: serial.into(),
            model: None,
            firmware: None,
            transport: TransportKind::MassStorage {
                mount: root.to_string_lossy().into_owned(),
            },
        }
    }

    fn one() -> Record {
        record_of(
            vec![book(100, "B1", "One")],
            vec![sitting("2026-09-01T09:00:00", 100, 600)],
        )
    }

    #[test]
    fn a_kindle_is_kept_whole_under_its_serial_and_twice_adds_nothing() {
        let ws = Workspace::create(&scratch("import-kindle")).unwrap();
        let root = kindle_dir("import-kindle-volume", &one());
        let first = kindle(&ws, &plugged(&root, "G000K1")).unwrap();
        assert!(!first.again);
        assert_eq!((first.sittings, first.books, first.pictures), (1, 1, 1));
        assert!(
            first
                .kept
                .starts_with(ws.kindle_dir("G000K1").join(BACKUPS))
        );
        assert_eq!(ws.record().sittings.len(), 1);
        assert_eq!(std::fs::read_dir(ws.backups()).unwrap().count(), 0);

        let second = kindle(&ws, &plugged(&root, "G000K1")).unwrap();
        assert!(second.again);
        assert_eq!(second.kept, first.kept);
        assert_eq!((second.sittings, second.books, second.pictures), (0, 0, 0));
        let kept = ws.kindle_dir("G000K1").join(BACKUPS);
        assert_eq!(std::fs::read_dir(kept).unwrap().count(), 1);
    }

    #[test]
    fn a_kindle_imported_again_states_what_it_holds_now() {
        let ws = Workspace::create(&scratch("import-again")).unwrap();
        let mut read = book(100, "B1", "One");
        read.percent = 100.0;
        read.finished = true;
        read.timer_ms = 3_600_000;
        read.timer_words = 20_000;
        read.timer_covered = 1.0;
        let mut on = record_of(vec![read], vec![sitting("2026-09-01T09:00:00", 100, 600)]);
        on.watermark = "260901:100000".into();
        let root = kindle_dir("import-again-volume", &on);
        kindle(&ws, &plugged(&root, "G000K1")).unwrap();

        // On the Kindle since: restarted and read past the restart, taken off
        // the Kindle, its timer moved on.
        let again = &mut on.books[0];
        again.finished = false;
        again.restarted_on.push(20_336);
        again.percent = 2.0;
        again.on_kindle = false;
        again.timer_ms = 60_000;
        again.timer_words = 300;
        again.timer_covered = 0.02;
        on.sittings.push(sitting("2026-09-05T09:00:00", 100, 60));
        on.watermark = "260905:100000".into();
        on.save(&root.join("extensions").join("readinglog"))
            .unwrap();
        kindle(&ws, &plugged(&root, "G000K1")).unwrap();

        let held = ws.record();
        let book = &held.books[0];
        assert_eq!(held.sittings.len(), 2);
        assert!(!book.finished, "restarted on the Kindle");
        assert_eq!(book.percent, 2.0);
        assert!(!book.on_kindle, "taken off the Kindle");
        assert_eq!(
            (book.timer_ms, book.timer_words, book.timer_covered),
            (60_000, 300, 0.02)
        );
        assert_eq!(book.restarted_on, vec![20_336]);
    }

    #[test]
    fn an_older_record_of_a_kindle_adds_only_what_the_newer_states_none_of() {
        let ws = Workspace::create(&scratch("import-older")).unwrap();
        let mut newer = one();
        newer.books[0].percent = 80.0;
        newer.watermark = "260910:100000".into();
        let mut older = record_of(
            vec![book(100, "B1", "One")],
            vec![
                sitting("2026-08-20T09:00:00", 100, 300),
                sitting("2026-09-01T09:00:00", 100, 600),
            ],
        );
        older.books[0].percent = 40.0;
        older.watermark = "260901:100000".into();
        kindle(&ws, &plugged(&kindle_dir("import-newer", &newer), "G000K1")).unwrap();
        kindle(&ws, &plugged(&kindle_dir("import-older", &older), "G000K1")).unwrap();

        let held = ws.record();
        assert_eq!(held.sittings.len(), 2, "the older one's sitting stays");
        assert_eq!(held.books[0].percent, 80.0, "the newer record's book");
        assert_eq!(held.watermark, "260910:100000");
    }

    #[test]
    fn two_kindles_are_kept_apart_and_drawn_together() {
        let ws = Workspace::create(&scratch("import-two")).unwrap();
        let mut near = one();
        near.books[0].percent = 10.0;
        let mut far = record_of(
            vec![book(100, "B1", "One")],
            vec![sitting("2026-09-02T09:00:00", 100, 900)],
        );
        far.books[0].percent = 60.0;
        kindle(&ws, &plugged(&kindle_dir("import-two-a", &near), "G000K1")).unwrap();
        let added = kindle(&ws, &plugged(&kindle_dir("import-two-b", &far), "G000K2")).unwrap();
        assert_eq!((added.sittings, added.books), (1, 0));
        assert_eq!(ws.kindles().len(), 2);
        let held = ws.record();
        assert_eq!(held.books.len(), 1);
        assert_eq!(held.books[0].percent, 60.0);
        assert_eq!(held.sittings.len(), 2);
    }

    #[test]
    fn a_dropped_volume_is_its_kindles_only_where_that_kindle_is_mounted_there() {
        let ws = Workspace::create(&scratch("import-dropped")).unwrap();
        let root = kindle_dir("import-dropped-volume", &one());
        let elsewhere = plugged(&scratch("import-dropped-other"), "G000K9");
        folder(&ws, &root, Some(&elsewhere)).unwrap();
        assert!(ws.kindles().is_empty(), "no Kindle is named");
        assert_eq!(std::fs::read_dir(ws.backups()).unwrap().count(), 1);

        folder(&ws, &root, Some(&plugged(&root, "G000K1"))).unwrap();
        assert_eq!(ws.kindles(), vec![ws.kindle_dir("G000K1")]);
        assert_eq!(ws.record().sittings.len(), 1);

        let app = root.join("extensions").join("readinglog");
        let alone = Workspace::create(&scratch("import-dropped-app")).unwrap();
        assert_eq!(folder(&alone, &app, None).unwrap().sittings, 1);
    }

    #[test]
    fn a_backup_is_kept_byte_for_byte_and_its_reading_added_to_whats_held() {
        let ws = Workspace::create(&scratch("import-zip")).unwrap();
        let root = kindle_dir("import-zip-volume", &one());
        let made = kindle(&ws, &plugged(&root, "G000K1")).unwrap();

        // A second Kindle's backup: one more sitting, on another book.
        let other = Workspace::create(&scratch("import-zip-other")).unwrap();
        let more = record_of(
            vec![book(100, "B1", "One"), book(200, "B2", "Two")],
            vec![
                sitting("2026-09-01T09:00:00", 100, 600),
                sitting("2026-09-03T09:00:00", 200, 900),
            ],
        );
        let zip = kindle(
            &other,
            &plugged(&kindle_dir("import-zip-second", &more), "G000K2"),
        )
        .unwrap()
        .kept;
        let added = backup(&ws, &zip).unwrap();
        assert!(!added.again);
        assert_eq!((added.sittings, added.books), (1, 1));
        assert_eq!(
            std::fs::read(&added.kept).unwrap(),
            std::fs::read(&zip).unwrap()
        );
        assert!(added.kept.starts_with(ws.backups()));
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
        assert!(folder(&ws, &scratch("import-refused-empty"), None).is_err());
    }
}
