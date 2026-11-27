//! [`kindle`] and [`backup`] keep what they read under the `backups/` of the Kindle its cut
//! serial names, its record the latest whole but the `w`, `r` and `d` rows of both; a [`folder`]
//! or [`backup`] naming no Kindle folds by `backup::restore`; `keep_latest` leaves one in each.
//! A Kindle the workspace does not take stays out whole, as [`Elsewhere`].

use std::path::{Path, PathBuf};
use std::sync::{Mutex, PoisonError};

use anyhow::{Context, Result, anyhow};

use crate::device::mass_storage::transport::MassStorageTransport;
use crate::device::{DeviceInfo, TPath, Transport};
use crate::metadata::{self, links};
use crate::workspace::{BACKUPS, RECORD, Workspace, write_file, writing};
use readinglog_native::backup::{self, About, Kind};
use readinglog_native::paper;
use readinglog_native::record::{self, PARTIAL_FILE, Record};
use readinglog_native::update::archive::{self, Archive, Source};

/// The record's name inside a backup, and in the Kindle's own folder.
const RECORD_NAME: &str = "sessions.tsv";

/// The manifest's name inside a backup.
const ABOUT_NAME: &str = "about.tsv";

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

/// What one sync of a Kindle did: its import, and whether its own
/// `sessions.tsv` was written.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct Synced {
    pub imported: Imported,
    pub put: bool,
}

/// Why a Kindle's record stays out of a workspace: the workspace holds another
/// Kindle, and takes no second one.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Elsewhere {
    /// The cut serial of the Kindle kept out.
    pub serial: String,
}

impl std::fmt::Display for Elsewhere {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{} is not this workspace's Kindle", self.serial)
    }
}

impl std::error::Error for Elsewhere {}

/// `Ok` where `ws` takes the Kindle the cut serial `serial` names, else [`Elsewhere`].
fn admit(ws: &Workspace, serial: &str) -> Result<()> {
    match ws.takes(serial) {
        true => Ok(()),
        false => Err(Elsewhere {
            serial: serial.to_string(),
        }
        .into()),
    }
}

/// Import the backup at `from`, a `readinglog-*.zip` or `book-*.zip`: under
/// the Kindle its `about.tsv` names, or where no Kindle is named.
pub fn backup(ws: &Workspace, from: &Path) -> Result<Imported> {
    // `backup::peek` refuses a file carrying no record before it is kept.
    backup::peek(from)
        .map_err(|err| anyhow!("{} is not a Reading Log backup: {err}", from.display()))?;
    let serial = backup::about(from).map(|a| a.serial).unwrap_or_default();
    if !serial.is_empty() {
        admit(ws, &serial)?;
    }
    let bytes = std::fs::read(from).with_context(|| format!("reading {}", from.display()))?;
    let named = from
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_else(|| "backup.zip".into());
    let (group, fold) = match serial.is_empty() {
        true => (ws.dir.clone(), Fold::Restore),
        false => (ws.kindle_dir(&serial), Fold::Latest),
    };
    let backups = group.join(BACKUPS);
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
    settle(ws, &group, kept, again, fold)
}

/// One sync of a Kindle at a time.
static SYNCING: Mutex<()> = Mutex::new(());

/// Sync the Kindle `device` names over one session, after any sync running:
/// import its record and covers under its cut serial, call `putting`, then
/// [`put_back`] the notes, corrections and metadata the workspace's copy holds.
pub fn kindle(ws: &Workspace, device: &DeviceInfo, putting: impl FnOnce()) -> Result<Synced> {
    let serial = backup::cut_serial(&device.serial);
    admit(ws, &serial)?;
    let _one = SYNCING.lock().unwrap_or_else(PoisonError::into_inner);
    let transport = device.open_transport()?;
    let imported = take(ws, Some(&serial), transport.as_ref())?;
    putting();
    let put = put_back(ws, &serial, transport.as_ref())?;
    Ok(Synced { imported, put })
}

/// Write the workspace copy's `w`, `r`, `d`, `p` and `q` rows for `serial`, and
/// the workspace's `l` rows, into the Kindle's `sessions.tsv` as [`record_on`]
/// reads it, every other line as it stands, where a row moves or `record_on`
/// reads the partial; [`put_covers`] first. Answers whether it wrote.
fn put_back(ws: &Workspace, serial: &str, transport: &dyn Transport) -> Result<bool> {
    let app = app_folder(transport)?;
    let ours = ws.kindle_dir(serial).join(RECORD);
    let written = Record::load(&ours);
    let covers = put_covers(&ours, &written, transport, &app)?;
    let (bytes, in_place) = record_on(transport, &app)?;
    let held = String::from_utf8_lossy(&bytes).into_owned();
    let mut merged = Record::from_text(&held);
    let mut rows = metadata::rows(&held);
    let mut linked = links::rows(&held);
    let notes_moved = merged.take_written(&written);
    let rows_moved = metadata::take(&mut rows, &metadata::read(&ours));
    let links_moved = links::take(&mut linked, &links::of(ws));
    let text = match notes_moved || rows_moved || links_moved {
        true => metadata::splice(&held, &merged, &rows, &linked),
        false if in_place => return Ok(covers),
        false => held,
    };
    transport.write_atomic(&app.join(RECORD_NAME), text.as_bytes())?;
    Ok(true)
}

/// Write onto the Kindle each cover a paper book standing in `written`, the
/// record in `ours`, names that the Kindle holds none of, ahead of the row that
/// names it: a name is never given twice, so a name held is that picture.
/// Answers whether it wrote one.
fn put_covers(
    ours: &Path,
    written: &Record,
    transport: &dyn Transport,
    app: &TPath,
) -> Result<bool> {
    let mut put = false;
    for book in written.paper_books.iter().filter(|b| !b.deleted) {
        let Some(name) = Path::new(&book.cover).file_name().and_then(|n| n.to_str()) else {
            continue;
        };
        let at = app.join(PICTURES).join(name);
        let local = ours.join(PICTURES).join(name);
        if !paper::is_cover(name) || !local.is_file() || transport.exists(&at)? {
            continue;
        }
        let bytes =
            std::fs::read(&local).with_context(|| format!("reading {}", local.display()))?;
        transport.write_atomic(&at, &bytes)?;
        put = true;
    }
    Ok(put)
}

/// The folder on `transport` holding the app's record, as `sessions.tsv` or
/// `sessions.tsv.partial`: its own, or the root of a folder dropped as one.
fn app_folder(transport: &dyn Transport) -> Result<TPath> {
    [TPath::parse(APP_DIR), TPath::new()]
        .into_iter()
        .find(|at| {
            [RECORD_NAME, PARTIAL_FILE]
                .iter()
                .any(|name| transport.exists(&at.join(name)).unwrap_or(false))
        })
        .ok_or_else(|| no_record(transport))
}

/// The record `app` on `transport` holds, as bytes, and whether `sessions.tsv`
/// holds them: a `sessions.tsv.partial` beside it where `record::partial_stands`
/// says it stands for the record, as `Record::load` takes one.
fn record_on(transport: &dyn Transport, app: &TPath) -> Result<(Vec<u8>, bool)> {
    let read = |name: &str| -> Result<Option<Vec<u8>>> {
        let at = app.join(name);
        match transport.exists(&at)? {
            true => transport.read(&at).map(Some),
            false => Ok(None),
        }
    };
    let file = read(RECORD_NAME)?;
    match read(PARTIAL_FILE)?.filter(|p| record::partial_stands(file.as_deref(), p)) {
        Some(whole) => Ok((whole, false)),
        None => file
            .map(|bytes| (bytes, true))
            .ok_or_else(|| no_record(transport)),
    }
}

/// The error [`app_folder`] and [`record_on`] answer for a Kindle holding no record.
fn no_record(transport: &dyn Transport) -> anyhow::Error {
    anyhow!(
        "{} holds no Reading Log record",
        transport.display_path(&TPath::parse(APP_DIR))
    )
}

/// Import the folder `dir` dropped on the window: a Kindle's volume, or the
/// app's own folder on one. The volume `plugged` is mounted at is kept under
/// that Kindle's cut serial; any other folder names no Kindle.
pub fn folder(ws: &Workspace, dir: &Path, plugged: Option<&DeviceInfo>) -> Result<Imported> {
    let serial = named_by(dir, plugged);
    if let Some(serial) = &serial {
        admit(ws, serial)?;
    }
    take(
        ws,
        serial.as_deref(),
        &MassStorageTransport::new(dir.to_path_buf()),
    )
}

/// The cut serial of the Kindle `plugged` names where it is mounted at `dir`, the
/// one Kindle a dropped folder names.
pub fn named_by(dir: &Path, plugged: Option<&DeviceInfo>) -> Option<String> {
    plugged
        .filter(|d| d.mass_storage_mount().as_deref() == Some(dir))
        .map(|d| backup::cut_serial(&d.serial))
}

/// The record and covers `transport` reads, kept in one backup laid out as the
/// Kindle lays its own and folded: under `serial`'s Kindle, its `about.tsv`
/// naming it, or where no Kindle is named.
fn take(ws: &Workspace, serial: Option<&str>, transport: &dyn Transport) -> Result<Imported> {
    let app = app_folder(transport)?;
    let (bytes, _) = record_on(transport, &app)?;
    let (group, fold) = match serial {
        Some(serial) => (ws.kindle_dir(serial), Fold::Latest),
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
    let stamp = backup::stamped_now();
    let about = serial
        .map(|serial| {
            let read = Record::from_text(&String::from_utf8_lossy(&bytes));
            About {
                firmware: transport.firmware().unwrap_or_default(),
                serial: serial.to_string(),
                ..About::of(&read, &stamp)
            }
            .text()
        })
        .unwrap_or_default();
    let mut entries: Vec<(String, Source<'_>)> = vec![(RECORD_NAME.into(), Source::Bytes(&bytes))];
    if !about.is_empty() {
        entries.push((ABOUT_NAME.into(), Source::Bytes(about.as_bytes())));
    }
    entries.extend(
        pictures
            .iter()
            .map(|(name, picture)| (format!("{PICTURES}/{name}"), Source::Bytes(picture))),
    );
    std::fs::create_dir_all(&backups)?;
    let named = backup::name(Kind::Record, &stamp, "kindle");
    let at = free(&backups, &named);
    archive::write(&at, &entries).with_context(|| format!("writing {}", at.display()))?;
    settle(ws, &group, at, false, fold)
}

/// How a backup folds into the record beside it.
enum Fold {
    /// `backup::restore`, then [`keep_latest`]: the backups naming no Kindle.
    Restore,
    /// [`latest`] with the `w`, `r` and `d` rows of both, then [`keep_latest`]: one Kindle's backups.
    Latest,
}

/// Fold the backup kept at `kept` into the record under `group`, read and saved
/// under [`writing`] and saved only where it moved, and count what the record
/// the screens draw gained.
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
    let about = backup::about(&kept).unwrap_or_default();
    match fold {
        Fold::Restore => {
            let _held = writing();
            let mut record = Record::load(&dir);
            let was = record.text();
            let mut rows = metadata::read(&dir);
            let mut linked = links::read(&dir);
            let carried = inside(&kept)
                .map(|bytes| String::from_utf8_lossy(&bytes).into_owned())
                .unwrap_or_default();
            let rows_moved = metadata::take(&mut rows, &metadata::rows(&carried));
            let links_moved = links::take(&mut linked, &links::rows(&carried));
            backup::restore(&dir, &kept, &mut record, &mut |_, _| {})
                .with_context(|| format!("folding in {}", kept.display()))?;
            if record.text() != was || rows_moved || links_moved {
                let text = metadata::splice(&record.text(), &record, &rows, &linked);
                write_file(&Record::file(&dir), text.as_bytes())
                    .with_context(|| format!("saving the record in {}", dir.display()))?;
            }
            keep_latest(group, &newest(group, &kept), &record);
        }
        Fold::Latest => {
            let (picked, text, mut fresh) =
                latest(group, &kept).with_context(|| format!("folding in {}", kept.display()))?;
            let _held = writing();
            fresh.take_written(&Record::load(&dir));
            let mut rows = metadata::rows(&text);
            metadata::take(&mut rows, &metadata::read(&dir));
            let mut linked = links::rows(&text);
            links::take(&mut linked, &links::read(&dir));
            let merged = metadata::splice(&text, &fresh, &rows, &linked);
            let file = Record::file(&dir);
            if std::fs::read_to_string(&file).ok().as_deref() != Some(merged.as_str()) {
                write_file(&file, merged.as_bytes())
                    .with_context(|| format!("saving the record in {}", dir.display()))?;
            }
            keep_latest(group, &picked, &fresh);
        }
    }
    links::spread(ws)?;
    let after = ws.record();
    Ok(Imported {
        about,
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

/// [`newest`], and its record as its file reads and as parsed; each cover `at`
/// carries goes to the record's folder where it holds none.
fn latest(group: &Path, at: &Path) -> Result<(PathBuf, String, Record)> {
    take_pictures(&group.join(RECORD), at)?;
    let pick = newest(group, at);
    backup::peek(&pick).map_err(|err| anyhow!("reading {}: {err}", pick.display()))?;
    let text = inside(&pick)
        .map(|bytes| String::from_utf8_lossy(&bytes).into_owned())
        .ok_or_else(|| anyhow!("{} holds no record", pick.display()))?;
    let record = Record::from_text(&text);
    Ok((pick, text, record))
}

/// The newest whole-record backup under `group` by its name's stamp, `at` among
/// those sharing it; `at` where `group` holds none.
fn newest(group: &Path, at: &Path) -> PathBuf {
    let records: Vec<backup::Backup> = backup::list(group)
        .into_iter()
        .filter(|b| b.kind == Kind::Record)
        .collect();
    let top = records.first().map(|b| b.stamp.clone());
    records
        .iter()
        .find(|b| b.path == at && Some(&b.stamp) == top.as_ref())
        .or(records.first())
        .map_or_else(|| at.to_path_buf(), |b| b.path.clone())
}

/// Remove each backup under `group` but `picked` that `record` holds all of, by
/// `backup::holds`; one holding a row `record` lacks stays.
fn keep_latest(group: &Path, picked: &Path, record: &Record) {
    for held in backup::list(group) {
        if held.path != picked
            && backup::peek(&held.path).is_ok_and(|inside| backup::holds(record, &inside))
        {
            let _ = backup::remove(&held.path);
        }
    }
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

/// `named` under `dir`, stepping `-2`, `-3` while a file stands there.
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
    use crate::tests::{book, over_mtp, record_of, scratch, sitting};

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

    /// A backup as the Kindle writes one, named for `stamp`, its `about.tsv`
    /// naming `serial` where one is given.
    fn kindle_backup(name: &str, record: &Record, stamp: &str, serial: Option<&str>) -> PathBuf {
        let at = scratch(name).join(backup::name(Kind::Record, stamp, ""));
        let text = record.text();
        let about = serial
            .map(|s| format!("written\t{stamp}\nserial\t{s}\n"))
            .unwrap_or_default();
        let mut entries = vec![(RECORD_NAME.to_string(), Source::Bytes(text.as_bytes()))];
        if !about.is_empty() {
            entries.push((ABOUT_NAME.to_string(), Source::Bytes(about.as_bytes())));
        }
        archive::write(&at, &entries).unwrap();
        at
    }

    fn one() -> Record {
        record_of(
            vec![book(100, "B1", "One")],
            vec![sitting("2026-09-01T09:00:00", 100, 600)],
        )
    }

    /// Every file under `dir`, by its path, with its bytes.
    fn files(dir: &Path) -> Vec<(PathBuf, Vec<u8>)> {
        let mut out = Vec::new();
        for entry in std::fs::read_dir(dir).into_iter().flatten().flatten() {
            let path = entry.path();
            match path.is_dir() {
                true => out.extend(files(&path)),
                false => out.push((path.clone(), std::fs::read(&path).unwrap())),
            }
        }
        out.sort();
        out
    }

    /// Whether `err` is [`Elsewhere`] for the Kindle `serial`.
    fn kept_out(err: &anyhow::Error, serial: &str) -> bool {
        err.downcast_ref::<Elsewhere>()
            == Some(&Elsewhere {
                serial: serial.into(),
            })
    }

    #[test]
    fn a_kindle_is_kept_whole_under_its_cut_serial_and_twice_adds_nothing() {
        let ws = Workspace::create(&scratch("import-kindle")).unwrap();
        let root = kindle_dir("import-kindle-volume", &one());
        let first = kindle(&ws, &plugged(&root, "G003H21234567890"), || {})
            .unwrap()
            .imported;
        assert!(!first.again);
        assert_eq!((first.sittings, first.books, first.pictures), (1, 1, 1));
        let kept = ws.kindle_dir("G003H2-7890").join(BACKUPS);
        assert!(first.kept.starts_with(&kept));
        assert_eq!(
            first.about.serial, "G003H2-7890",
            "the backup names its Kindle"
        );
        let stated = &first.about;
        assert_eq!(
            (stated.sittings, stated.first.as_str(), stated.last.as_str()),
            (1, "2026-09-01", "2026-09-01"),
            "and states what it holds, as the Kindle's own backups do"
        );
        assert_eq!(ws.record().sittings.len(), 1);
        assert!(!ws.backups().exists());

        let second = kindle(&ws, &plugged(&root, "G003H21234567890"), || {})
            .unwrap()
            .imported;
        assert!(second.again);
        assert_eq!(second.kept, first.kept);
        assert_eq!((second.sittings, second.books, second.pictures), (0, 0, 0));
        assert_eq!(std::fs::read_dir(kept).unwrap().count(), 1);
    }

    #[test]
    fn a_kindle_keeps_one_backup_each_sync_taking_the_place_of_one_the_record_holds() {
        let ws = Workspace::create(&scratch("import-one")).unwrap();
        let mut on = one();
        let root = kindle_dir("import-one-volume", &on);
        let dir = root.join(APP_DIR);
        let synced = |ws: &Workspace| {
            kindle(ws, &plugged(&root, "G000K1"), || {})
                .unwrap()
                .imported
        };
        synced(&ws);
        for (n, day) in [(2, "2026-09-02"), (3, "2026-09-03")] {
            on.sittings
                .push(sitting(&format!("{day}T09:00:00"), 100, 600));
            on.save(&dir).unwrap();
            let next = synced(&ws);
            let kept = backup::list(&ws.kindle_dir("G000K1"));
            assert_eq!(kept.len(), 1, "{kept:?}");
            assert_eq!(kept[0].path, next.kept);
            assert_eq!(backup::peek(&next.kept).unwrap().sittings.len(), n);
        }
        assert_eq!(ws.record().sittings.len(), 3);
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
        let root = kindle_dir("import-again-volume", &on);
        kindle(&ws, &plugged(&root, "G000K1"), || {}).unwrap();

        // `on` as the Kindle holds it since: restarted and read past the
        // restart, off the Kindle, its timer moved on.
        let again = &mut on.books[0];
        again.finished = false;
        again.restarted_on.push(20_336);
        again.percent = 2.0;
        again.on_kindle = false;
        again.timer_ms = 60_000;
        again.timer_words = 300;
        again.timer_covered = 0.02;
        on.sittings.push(sitting("2026-09-05T09:00:00", 100, 60));
        on.save(&root.join("extensions").join("readinglog"))
            .unwrap();
        kindle(&ws, &plugged(&root, "G000K1"), || {}).unwrap();

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
    fn a_kindles_record_is_its_latest_backup_whole() {
        let ws = Workspace::create(&scratch("import-latest")).unwrap();
        let older = record_of(
            vec![book(100, "B1", "One")],
            vec![
                sitting("2026-08-20T09:00:00", 100, 300),
                sitting("2026-09-01T09:00:00", 100, 600),
            ],
        );
        let dropped = kindle_backup(
            "import-latest-older",
            &older,
            "260901:100000",
            Some("G000K1"),
        );
        let first = backup(&ws, &dropped).unwrap();
        assert!(
            first
                .kept
                .starts_with(ws.kindle_dir("G000K1").join(BACKUPS))
        );
        assert_eq!(ws.record().sittings.len(), 2);

        // `one()`, the Kindle's record since, a sitting fewer, read over USB.
        kindle(
            &ws,
            &plugged(&kindle_dir("import-latest-volume", &one()), "G000K1"),
            || {},
        )
        .unwrap();
        assert_eq!(ws.record().sittings.len(), 1, "the latest backup stands");
        assert_eq!(
            backup::list(&ws.kindle_dir("G000K1")).len(),
            2,
            "`dropped` holds a sitting the record lacks, and stays"
        );

        // `dropped` again: held, and the latest stands.
        let again = backup(&ws, &dropped).unwrap();
        assert!(again.again);
        assert_eq!(ws.record().sittings.len(), 1);
    }

    #[test]
    fn a_whole_partial_stands_for_a_record_missing_or_cut_and_goes_back_in_its_place() {
        let ws = Workspace::create(&scratch("import-partial")).unwrap();
        let serial = "G000P1";
        let root = kindle_dir("import-partial-volume", &one());
        let dir = root.join(APP_DIR);
        let (file, partial) = (Record::file(&dir), dir.join(PARTIAL_FILE));
        let whole = std::fs::read_to_string(&file).unwrap();
        // `file` missing beside a whole `partial`.
        std::fs::rename(&file, &partial).unwrap();
        let done = kindle(&ws, &plugged(&root, serial), || {}).unwrap();
        assert_eq!((done.imported.sittings, done.put), (1, true));
        assert_eq!(std::fs::read_to_string(&file).unwrap(), whole);
        assert!(!partial.exists());
        // `file` cut short beside a whole `partial`.
        std::fs::write(&partial, &whole).unwrap();
        std::fs::write(&file, &whole[..whole.len() / 2]).unwrap();
        assert!(kindle(&ws, &plugged(&root, serial), || {}).unwrap().put);
        assert_eq!(std::fs::read_to_string(&file).unwrap(), whole);
        assert!(!partial.exists());
        // `partial` cut itself, or whole and no fuller copy of `file`: `file` stands.
        let mut two = one();
        two.sittings.push(sitting("2026-09-02T09:00:00", 100, 600));
        for stale in [whole[..whole.len() - 3].to_string(), two.text()] {
            std::fs::write(&partial, &stale).unwrap();
            assert!(!kindle(&ws, &plugged(&root, serial), || {}).unwrap().put);
            assert_eq!(std::fs::read_to_string(&file).unwrap(), whole);
            assert_eq!(ws.record().sittings.len(), 1);
        }
    }

    #[test]
    fn a_kindle_over_mtp_holding_a_whole_partial_alone_takes_its_record_back_and_the_notes() {
        let ws = Workspace::create(&scratch("import-mtp")).unwrap();
        let serial = "G000M1";
        let root = kindle_dir("import-mtp-storage", &one());
        let dir = root.join(APP_DIR);
        let (file, partial) = (Record::file(&dir), dir.join(PARTIAL_FILE));
        let whole = std::fs::read_to_string(&file).unwrap();
        // `file` missing beside a whole `partial`.
        std::fs::rename(&file, &partial).unwrap();
        let device = over_mtp(&root, serial);
        let done = kindle(&ws, &device, || {}).unwrap();
        assert_eq!((done.imported.sittings, done.put), (1, true));
        assert_eq!(std::fs::read_to_string(&file).unwrap(), whole);
        assert!(!partial.exists());
        let on = crate::notes::On::Book {
            key: "B1".into(),
            extent: 100,
        };
        crate::notes::write(&ws, serial, &on, "written here").unwrap();
        assert!(kindle(&ws, &device, || {}).unwrap().put);
        let on_kindle = std::fs::read_to_string(&file).unwrap();
        assert_eq!(Record::from_text(&on_kindle).notes.len(), 1);
        assert!(record::closed(&on_kindle));
        assert!(!partial.exists());
        assert!(!kindle(&ws, &device, || {}).unwrap().put);
    }

    #[test]
    fn a_sync_puts_the_notes_written_here_into_the_kindles_own_record_and_nothing_else() {
        let ws = Workspace::create(&scratch("import-put")).unwrap();
        let serial = "G000K1";
        let root = kindle_dir("import-put-volume", &one());
        let file = Record::file(&root.join(APP_DIR));
        // `later` stands for a row a later build of Reading Log writes.
        let later = "y\tfrom a later build";
        let held = format!("{}{later}\n", std::fs::read_to_string(&file).unwrap());
        std::fs::write(&file, &held).unwrap();
        let first = kindle(&ws, &plugged(&root, serial), || {}).unwrap();
        assert!(!first.put, "nothing is written here yet");
        let on = crate::notes::On::Book {
            key: "B1".into(),
            extent: 100,
        };
        crate::notes::write(&ws, serial, &on, "written here").unwrap();
        // `since`: the Kindle's own record moved on before the sync reads it again.
        let since = held.replace(later, &format!("{later}\ny\tsince"));
        std::fs::write(&file, &since).unwrap();
        assert!(put_back(&ws, serial, &MassStorageTransport::new(root.clone())).unwrap());
        let on_kindle = std::fs::read_to_string(&file).unwrap();
        assert_eq!(Record::from_text(&on_kindle).notes.len(), 1);
        assert!(
            on_kindle.contains("y\tsince\n"),
            "the Kindle's own lines stand"
        );
        let other = |text: &str| -> Vec<String> {
            text.lines()
                .filter(|l| !matches!(l.split('\t').next(), Some("w" | "r" | "end")))
                .map(String::from)
                .collect()
        };
        assert_eq!(
            other(&on_kindle),
            other(&since),
            "byte for byte but the written rows"
        );
        assert!(
            !root
                .join(APP_DIR)
                .join(readinglog_native::record::PARTIAL_FILE)
                .exists(),
            "no partial left"
        );
        // `kindle` once more: nothing to put.
        assert!(!kindle(&ws, &plugged(&root, serial), || {}).unwrap().put);
    }

    #[test]
    fn a_note_written_here_outlasts_the_next_import_and_takes_a_later_edit_from_it() {
        let ws = Workspace::create(&scratch("import-written")).unwrap();
        let serial = "G000K1";
        kindle(
            &ws,
            &plugged(&kindle_dir("import-written-volume", &one()), serial),
            || {},
        )
        .unwrap();
        let on = crate::notes::On::Book {
            key: "B1".into(),
            extent: 100,
        };
        let written = crate::notes::write(&ws, serial, &on, "written here").unwrap();
        // `since`, the Kindle's record a sitting on, holding none of the rows written here.
        let mut since = one();
        since
            .sittings
            .push(sitting("2026-09-02T09:00:00", 100, 600));
        kindle(
            &ws,
            &plugged(&kindle_dir("import-written-since", &since), serial),
            || {},
        )
        .unwrap();
        let held = Record::load(&ws.kindle_dir(serial).join(RECORD));
        assert_eq!(held.sittings.len(), 2, "the latest backup stands");
        assert_eq!(held.notes.len(), 1, "and the note with it");
        // `carried` holds a later edit of the note: the later stands.
        let mut carried = since.clone();
        carried.take_written(&held);
        carried
            .reword_note(&written, "reworded", "2099-01-01T00:00:00")
            .unwrap();
        kindle(
            &ws,
            &plugged(&kindle_dir("import-written-carried", &carried), serial),
            || {},
        )
        .unwrap();
        let held = Record::load(&ws.kindle_dir(serial).join(RECORD));
        assert_eq!(held.notes[0].text, "reworded");
    }

    #[test]
    fn metadata_set_here_outlasts_the_next_import_and_goes_onto_the_kindle() {
        let ws = Workspace::create(&scratch("import-metadata")).unwrap();
        let serial = "G000K1";
        kindle(
            &ws,
            &plugged(&kindle_dir("import-metadata-volume", &one()), serial),
            || {},
        )
        .unwrap();
        let named = metadata::Named {
            kindle: serial.into(),
            key: "B1".into(),
            extent: 100,
        };
        let patch = metadata::Patch {
            title: Some(metadata::Change::To("Retitled".into())),
            ..metadata::Patch::default()
        };
        metadata::edit(&ws, &[named], &patch, "2026-10-31T10:00:00").unwrap();
        // `since`, the Kindle's record a sitting on, holding none of the rows set here.
        let mut since = one();
        since
            .sittings
            .push(sitting("2026-09-02T09:00:00", 100, 600));
        let root = kindle_dir("import-metadata-since", &since);
        assert!(kindle(&ws, &plugged(&root, serial), || {}).unwrap().put);
        let title = |dir: &Path| metadata::Set::of(&metadata::read(dir), "B1", 100).title;
        let ours = ws.kindle_dir(serial).join(RECORD);
        assert_eq!(
            Record::load(&ours).sittings.len(),
            2,
            "the latest backup stands"
        );
        assert_eq!(
            title(&ours).as_deref(),
            Some("Retitled"),
            "and the row with it"
        );
        let on_kindle = root.join(APP_DIR);
        assert_eq!(title(&on_kindle).as_deref(), Some("Retitled"));
        assert_eq!(Record::load(&on_kindle).sittings.len(), 2);
        // Once more: the Kindle holds the row, nothing to put.
        assert!(!kindle(&ws, &plugged(&root, serial), || {}).unwrap().put);
    }

    /// A paper book written here, over mass storage and over MTP: a sync puts its
    /// rows onto the Kindle, the Kindle's own save keeps them, the next import keeps
    /// them, and a sitting deleted here stays deleted past the Kindle's older copy.
    #[test]
    fn a_paper_book_written_here_goes_onto_the_kindle_and_outlasts_every_import() {
        for over in ["volume", "mtp"] {
            let ws = Workspace::create(&scratch(&format!("import-paper-{over}"))).unwrap();
            let serial = "G000K1";
            let root = kindle_dir(&format!("import-paper-{over}-storage"), &one());
            let device = match over {
                "mtp" => over_mtp(&root, serial),
                _ => plugged(&root, serial),
            };
            kindle(&ws, &device, || {}).unwrap();
            let new = crate::paper::NewBook {
                title: "On Paper".into(),
                author: "Someone".into(),
                language: "en".into(),
                pages: 200,
                cover: None,
            };
            let key = crate::paper::add_book(&ws, serial, &new).unwrap();
            let ours = ws.kindle_dir(serial).join(RECORD);
            let added = Record::load(&ours).paper_books[0].added.clone();
            let read = crate::paper::Read {
                day: "2026-09-02".into(),
                from: "21:00".into(),
                to: "21:30".into(),
                from_page: 0,
                to_page: 30,
            };
            let logged = crate::paper::log(&ws, serial, &added, &read).unwrap();
            assert!(kindle(&ws, &device, || {}).unwrap().put, "{over}");
            let app = root.join(APP_DIR);
            let on = Record::load(&app);
            assert_eq!(
                (on.paper_books.len(), on.paper_sittings.len()),
                (1, 1),
                "{over}"
            );
            // `Record::keep`: the Kindle's own pass, a sitting of its own on.
            let mut pass = Record::load(&app);
            pass.sittings.push(sitting("2026-09-03T09:00:00", 100, 600));
            pass.keep(&app, &mut None).unwrap();
            let older = std::fs::read_to_string(Record::file(&app)).unwrap();
            assert_eq!(Record::from_text(&older).paper_sittings.len(), 1);
            kindle(&ws, &device, || {}).unwrap();
            let held = Record::load(&ours);
            assert_eq!(held.sittings.len(), 2, "{over}: the Kindle's pass came in");
            assert_eq!(
                held.paper_sittings.len(),
                1,
                "{over}: and the paper rows stand"
            );
            let drawn = held.drawn().record;
            assert!(drawn.books.iter().any(|b| b.cde_key == key), "{over}");
            crate::paper::delete(&ws, serial, &logged).unwrap();
            // `older`, the Kindle's copy still holding the sitting live, comes in again.
            std::fs::write(Record::file(&app), &older).unwrap();
            assert!(kindle(&ws, &device, || {}).unwrap().put, "{over}");
            assert!(Record::load(&ours).paper_sittings[0].deleted, "{over}");
            assert!(Record::load(&app).paper_sittings[0].deleted, "{over}");
        }
    }

    /// A paper book's cover set here, over mass storage and over MTP: a sync puts
    /// the picture onto the Kindle with the row naming it, the Kindle's own cover
    /// pass leaves it standing, and the window draws it off its copy here.
    #[test]
    fn a_paper_cover_set_here_goes_onto_the_kindle_and_stays_there() {
        for over in ["volume", "mtp"] {
            let ws = Workspace::create(&scratch(&format!("import-cover-{over}"))).unwrap();
            let serial = "G000K1";
            let root = kindle_dir(&format!("import-cover-{over}-storage"), &one());
            let device = match over {
                "mtp" => over_mtp(&root, serial),
                _ => plugged(&root, serial),
            };
            kindle(&ws, &device, || {}).unwrap();
            let picture = scratch(&format!("import-cover-{over}-picture")).join("front.png");
            image::RgbImage::from_pixel(300, 450, image::Rgb([40, 90, 160]))
                .save(&picture)
                .unwrap();
            let new = crate::paper::NewBook {
                title: "On Paper".into(),
                author: String::new(),
                language: "en".into(),
                pages: 200,
                cover: Some(picture.to_string_lossy().into_owned()),
            };
            crate::paper::add_book(&ws, serial, &new).unwrap();
            assert!(kindle(&ws, &device, || {}).unwrap().put, "{over}");
            let app = root.join(APP_DIR);
            let on = Record::load(&app);
            let named = Path::new(&on.paper_books[0].cover).to_path_buf();
            let file = app.join(PICTURES).join(named.file_name().unwrap());
            assert!(readinglog_native::covers::drawable(&file), "{over}");
            // `Record::keep_covers_from`: the Kindle's own pass, sweeping what no book of its names.
            let mut pass = Record::load(&app);
            pass.keep_covers_from(&app, &scratch(&format!("import-cover-{over}-none")));
            assert!(file.exists(), "{over}: the Kindle's sweep spares it");
            assert!(app.join(PICTURES).join("B1.jpg").exists());
            assert!(
                !kindle(&ws, &device, || {}).unwrap().put,
                "{over}: nothing more to put"
            );
            // `shown`: the window draws the paper book's cover off its copy here.
            let today = readinglog_native::date::parse_day("2026-09-02").unwrap();
            let shown = crate::screen::Shown::of(&ws, today).unwrap();
            let at = shown.stats.books.iter().position(|b| b.is_paper()).unwrap();
            let drawn = shown.covers[at].clone().unwrap();
            assert_eq!(drawn.file_name(), named.file_name(), "{over}");
            assert!(drawn.starts_with(ws.kindle_dir(serial)), "{over}");
        }
    }

    #[test]
    fn metadata_set_on_a_record_naming_no_kindle_outlasts_a_restore() {
        let ws = Workspace::create(&scratch("import-metadata-unnamed")).unwrap();
        let zip = kindle_backup("import-metadata-unnamed-zip", &one(), "260901:100000", None);
        backup(&ws, &zip).unwrap();
        let named = metadata::Named {
            kindle: String::new(),
            key: "B1".into(),
            extent: 100,
        };
        let patch = metadata::Patch {
            publisher: Some(metadata::Change::To("Example Press".into())),
            ..metadata::Patch::default()
        };
        metadata::edit(&ws, &[named], &patch, "2026-10-31T10:00:00").unwrap();
        let mut two = one();
        two.sittings.push(sitting("2026-09-02T09:00:00", 100, 600));
        let later = kindle_backup("import-metadata-unnamed-later", &two, "260902:100000", None);
        assert_eq!(
            backup(&ws, &later).unwrap().sittings,
            1,
            "the later backup folded in"
        );
        let rows = metadata::read(&ws.record_dir());
        assert_eq!(
            metadata::Set::of(&rows, "B1", 100).publisher.as_deref(),
            Some("Example Press")
        );
    }

    #[test]
    fn a_backup_naming_no_kindle_is_kept_where_no_kindle_is_named() {
        let ws = Workspace::create(&scratch("import-unnamed")).unwrap();
        let zip = kindle_backup("import-unnamed-zip", &one(), "260901:100000", None);
        let added = backup(&ws, &zip).unwrap();
        assert!(added.kept.starts_with(ws.backups()));
        assert!(ws.kindles().is_empty());
        assert_eq!((added.sittings, added.books), (1, 1));
        assert_eq!(
            std::fs::read(&added.kept).unwrap(),
            std::fs::read(&zip).unwrap()
        );
        let again = backup(&ws, &zip).unwrap();
        assert!(again.again);
        assert_eq!(again.sittings, 0);
        // `later`, a second backup naming no Kindle, stands alone once `zip`'s rows are held.
        let mut two = one();
        two.sittings.push(sitting("2026-09-02T09:00:00", 100, 600));
        let later = kindle_backup("import-unnamed-later", &two, "260902:100000", None);
        let next = backup(&ws, &later).unwrap();
        let kept = backup::list(&ws.dir);
        assert_eq!(kept.len(), 1, "{kept:?}");
        assert_eq!(kept[0].path, next.kept);
        assert_eq!(ws.record().sittings.len(), 2);
    }

    #[test]
    fn a_second_kindle_is_kept_out_and_the_workspace_left_as_it_was() {
        let ws = Workspace::create(&scratch("import-second")).unwrap();
        kindle(
            &ws,
            &plugged(&kindle_dir("import-second-a", &one()), "G000K1"),
            || {},
        )
        .unwrap();
        let before = files(&ws.dir);
        let other = kindle_dir("import-second-b", &one());
        for device in [plugged(&other, "G000K2"), over_mtp(&other, "G000K2")] {
            let err = kindle(&ws, &device, || panic!("nothing is put")).unwrap_err();
            assert!(kept_out(&err, "G000K2"), "{err:#}");
            assert_eq!(files(&ws.dir), before, "nothing kept");
        }
        assert_eq!(ws.kindles(), vec![ws.kindle_dir("G000K1")]);
        assert_eq!(
            std::fs::read_to_string(Record::file(&other.join(APP_DIR))).unwrap(),
            one().text(),
            "nothing put on it"
        );
    }

    #[test]
    fn another_kindles_volume_dropped_is_kept_out() {
        let ws = Workspace::create(&scratch("import-dropped-second")).unwrap();
        kindle(
            &ws,
            &plugged(&kindle_dir("import-dropped-own", &one()), "G000K1"),
            || {},
        )
        .unwrap();
        let before = files(&ws.dir);
        let root = kindle_dir("import-dropped-other", &one());
        let err = folder(&ws, &root, Some(&plugged(&root, "G000K2"))).unwrap_err();
        assert!(kept_out(&err, "G000K2"), "{err:#}");
        assert_eq!(files(&ws.dir), before);
    }

    #[test]
    fn a_backup_joins_its_own_kindle_and_another_kindles_is_kept_out() {
        let ws = Workspace::create(&scratch("import-zip")).unwrap();
        kindle(
            &ws,
            &plugged(&kindle_dir("import-zip-volume", &one()), "G000K1"),
            || {},
        )
        .unwrap();

        // `more`, the Kindle's record since, kept as the window keeps one in a
        // workspace of its own: one more sitting, on another book.
        let more = record_of(
            vec![book(100, "B1", "One"), book(200, "B2", "Two")],
            vec![
                sitting("2026-09-01T09:00:00", 100, 600),
                sitting("2026-09-03T09:00:00", 200, 900),
            ],
        );
        let kept_by = |name: &str, serial: &str| {
            let elsewhere = Workspace::create(&scratch(name)).unwrap();
            let volume = kindle_dir(&format!("{name}-volume"), &more);
            kindle(&elsewhere, &plugged(&volume, serial), || {})
                .unwrap()
                .imported
                .kept
        };
        let own = kept_by("import-zip-own", "G000K1");
        let added = backup(&ws, &own).unwrap();
        assert!(!added.again);
        assert!(
            added
                .kept
                .starts_with(ws.kindle_dir("G000K1").join(BACKUPS))
        );
        assert_eq!(
            std::fs::read(&added.kept).unwrap(),
            std::fs::read(&own).unwrap()
        );
        assert_eq!((added.sittings, added.books), (1, 1));
        assert_eq!(ws.record().sittings.len(), 2, "the latest backup stands");

        let before = files(&ws.dir);
        let other = kept_by("import-zip-other", "G000K2");
        let err = backup(&ws, &other).unwrap_err();
        assert!(kept_out(&err, "G000K2"), "{err:#}");
        assert_eq!(files(&ws.dir), before, "nothing kept");
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
        assert_eq!(
            ws.record().sittings.len(),
            2,
            "the Kindle's, and the backup's"
        );

        let app = root.join("extensions").join("readinglog");
        let alone = Workspace::create(&scratch("import-dropped-app")).unwrap();
        assert_eq!(folder(&alone, &app, None).unwrap().sittings, 1);
    }

    #[test]
    fn a_file_that_is_no_backup_is_refused_and_not_kept() {
        let ws = Workspace::create(&scratch("import-refused")).unwrap();
        let stray = scratch("import-refused-file").join("notes.zip");
        std::fs::write(&stray, "not an archive").unwrap();
        assert!(backup(&ws, &stray).is_err());
        assert!(!ws.backups().exists());
        assert!(folder(&ws, &scratch("import-refused-empty"), None).is_err());
    }
}
