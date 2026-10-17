//! The window: [`page`] draws the route its hash names through
//! [`screen::render`]; the other commands import, set a setting, correct a
//! highlight, write a note, delete a backup or everything, and ask.

use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::SystemTime;

use serde::Serialize;
use tauri::ipc::Channel;
use tauri::{AppHandle, Manager, State};
use tauri_plugin_dialog::{DialogExt, MessageDialogButtons, MessageDialogKind};
use tauri_plugin_opener::OpenerExt;

use readinglog_native::backup::{self, cut_serial};
use readinglog_native::covers::COVERS_DIR;
use readinglog_native::date;
use readinglog_native::lang::counted;
use readinglog_native::record::Record;
use readinglog_native::view::config;

use crate::device::mtp::transport::InUse;
use crate::device::{self, DeviceInfo};
use crate::edits::{self, HighlightId};
use crate::import::{self, Imported};
use crate::notes;
use crate::screen::{self, Covers, Page, Route, Shown};
use crate::workspace::{CONFIG_FILE, Workspace};

/// The folder the window shows, and the record as last built off it.
#[derive(Default)]
struct Open {
    dir: Mutex<Option<PathBuf>>,
    shown: Mutex<Option<(Stamp, Arc<Shown>)>>,
    /// The covers folders the asset protocol serves.
    served: Mutex<Vec<PathBuf>>,
}

/// When each record and `CONFIG_FILE` were last written, with the records'
/// sizes, and the clock's day.
type Stamp = (
    PathBuf,
    Vec<(Option<SystemTime>, Option<u64>)>,
    Option<SystemTime>,
    i64,
);

/// When the file at `path` was last written, and its size.
fn written(path: &Path) -> (Option<SystemTime>, Option<u64>) {
    let held = std::fs::metadata(path).ok();
    (
        held.as_ref().and_then(|m| m.modified().ok()),
        held.as_ref().map(|m| m.len()),
    )
}

fn stamp(ws: &Workspace, today: i64) -> Stamp {
    let records = ws
        .record_dirs()
        .iter()
        .map(|dir| written(&Record::file(dir)))
        .collect();
    (
        ws.dir.clone(),
        records,
        written(&ws.dir.join(CONFIG_FILE)).0,
        today,
    )
}

/// [`DeviceInfo`] of the Kindle plugged in that `import::kindle` reads: one
/// mounted with a record under `extensions/readinglog`, or one over MTP, which
/// shows nothing without a session.
fn plugged() -> Option<DeviceInfo> {
    let found = device::detect::detect()?;
    match found.mass_storage_mount() {
        Some(mount) => Record::file(&mount.join("extensions").join("readinglog"))
            .is_file()
            .then_some(found),
        None => Some(found),
    }
}

/// The file under the app's config folder naming the folder last opened.
const LAST: &str = "workspace";

/// The folder a first run keeps its record in, under Documents.
const FOLDER: &str = "Reading Log";

/// What a command answers: its value, or why not, in words.
type Answer<T> = Result<T, String>;

fn said(err: anyhow::Error) -> String {
    format!("{err:#}")
}

/// The folder the window shows: the one held, else the one last opened, else
/// `FOLDER` under Documents, made by the first import into it.
fn folder(app: &AppHandle, open: &State<'_, Open>) -> Answer<PathBuf> {
    let mut held = open
        .dir
        .lock()
        .map_err(|_| "the record's folder could not be read".to_string())?;
    if let Some(dir) = held.as_ref() {
        return Ok(dir.clone());
    }
    let dir = match last(app) {
        Some(dir) => dir,
        None => app
            .path()
            .document_dir()
            .map_err(|e| e.to_string())?
            .join(FOLDER),
    };
    *held = Some(dir.clone());
    Ok(dir)
}

/// The folder last opened, where it holds a `CONFIG_FILE`.
fn last(app: &AppHandle) -> Option<PathBuf> {
    let file = app.path().app_config_dir().ok()?.join(LAST);
    let dir = PathBuf::from(std::fs::read_to_string(file).ok()?.trim());
    dir.join(CONFIG_FILE).is_file().then_some(dir)
}

fn remember(app: &AppHandle, dir: &Path) -> Answer<()> {
    let config = app.path().app_config_dir().map_err(|e| e.to_string())?;
    std::fs::create_dir_all(&config).map_err(|e| e.to_string())?;
    std::fs::write(config.join(LAST), dir.to_string_lossy().as_bytes()).map_err(|e| e.to_string())
}

/// The record at `ws` built for the screens as of the clock's day, built again
/// only where its [`stamp`] has moved.
fn shown(open: &State<'_, Open>, ws: &Workspace) -> Answer<Option<Arc<Shown>>> {
    let (today, _) = date::now();
    let now = stamp(ws, today);
    let mut held = open
        .shown
        .lock()
        .map_err(|_| "the record could not be read".to_string())?;
    if let Some((at, built)) = held.as_ref()
        && *at == now
    {
        return Ok(Some(built.clone()));
    }
    let built = Shown::of(ws, today).map(Arc::new);
    *held = built.clone().map(|b| (now, b));
    Ok(built)
}

/// `route` drawn `width` wide, its covers named for the asset protocol.
#[tauri::command]
async fn page(app: AppHandle, open: State<'_, Open>, route: String, width: f64) -> Answer<Page> {
    let ws = Workspace::open(&folder(&app, &open)?).map_err(said)?;
    let built = shown(&open, &ws)?;
    let mut served = open
        .served
        .lock()
        .map_err(|_| "the covers could not be served".to_string())?;
    if built.is_some() {
        for covers in ws.record_dirs().iter().map(|dir| dir.join(COVERS_DIR)) {
            if served.contains(&covers) {
                continue;
            }
            app.asset_protocol_scope()
                .allow_directory(&covers, false)
                .map_err(|e| e.to_string())?;
            served.push(covers);
        }
    }
    drop(served);
    let kindle = plugged().map(|d| cut_serial(&d.serial));
    Ok(screen::render(
        built.as_deref(),
        &ws,
        &Route::parse(&route),
        width as f32,
        Covers::Window,
        kindle.as_deref(),
    ))
}

/// The cut serial of the Kindle [`plugged`] answers.
#[tauri::command]
async fn kindle() -> Option<String> {
    plugged().map(|d| cut_serial(&d.serial))
}

/// The clock's day, the one [`page`] draws as today.
#[tauri::command]
fn today() -> i64 {
    date::now().0
}

/// Import each of `paths`: a backup, or a Kindle's volume or its folder. Answers
/// what the record gained, in the window's words.
#[tauri::command]
async fn import(app: AppHandle, open: State<'_, Open>, paths: Vec<String>) -> Answer<String> {
    let dir = folder(&app, &open)?;
    let ws = Workspace::create(&dir).map_err(said)?;
    remember(&app, &dir)?;
    let kindle = plugged();
    let mut done = Vec::new();
    for path in &paths {
        let from = Path::new(path);
        done.push(
            match from.is_dir() {
                true => import::folder(&ws, from, kindle.as_ref()),
                false => import::backup(&ws, from),
            }
            .map_err(said)?,
        );
    }
    Ok(gained(&ws, &done))
}

/// What a sync says: `flash`, what the record gained in the window's words,
/// empty where it says nothing, and `card`, the Kindle card's line.
#[derive(Serialize)]
struct Synced {
    flash: String,
    card: String,
}

/// Sync the Kindle `serial` names off the window's thread, as `import::kindle`
/// does, `phase` told the card's line as each half starts; `quiet` flashes nothing
/// where nothing came in. Fails with the card's line naming what stopped it.
#[tauri::command]
async fn sync_kindle(
    app: AppHandle,
    open: State<'_, Open>,
    serial: String,
    quiet: bool,
    phase: Channel<String>,
) -> Answer<Synced> {
    let dir = folder(&app, &open)?;
    let ws = Workspace::create(&dir).map_err(said)?;
    remember(&app, &dir)?;
    let w = screen::Words::of(ws.config.lang());
    let stopped = |why: &str| w.not_synced.replace("{why}", why);
    let device = plugged()
        .filter(|d| cut_serial(&d.serial) == serial)
        .ok_or_else(|| stopped(w.unplugged))?;
    let _ = phase.send(w.syncing_in.to_string());
    let at = ws.clone();
    let done = tauri::async_runtime::spawn_blocking(move || {
        import::kindle(&at, &device, || {
            let _ = phase.send(w.syncing_out.to_string());
        })
    })
    .await
    .map_err(|e| stopped(&e.to_string()))?
    .map_err(|err| stopped(&stopped_by(&err, w)))?;
    let (_, secs) = date::now();
    let card = w.synced.replace(
        "{t}",
        &format!("{:02}:{:02}", secs / 3600, secs % 3600 / 60),
    );
    let came = &done.imported;
    let flash = match quiet && came.sittings + came.books + came.highlights == 0 {
        true => String::new(),
        false => gained(&ws, &[done.imported]),
    };
    Ok(Synced { flash, card })
}

/// What stopped a sync, `err`: `w.in_use` where another app holds the Kindle.
fn stopped_by(err: &anyhow::Error, w: &screen::Words) -> String {
    match err.chain().any(|e| e.downcast_ref::<InUse>().is_some()) {
        true => w.in_use.to_string(),
        false => format!("{err:#}"),
    }
}

/// What `done` added to the record, in `ws`'s language.
fn gained(ws: &Workspace, done: &[Imported]) -> String {
    let sittings: usize = done.iter().map(|d| d.sittings).sum();
    let books: usize = done.iter().map(|d| d.books).sum();
    let highlights: usize = done.iter().map(|d| d.highlights).sum();
    let lang = ws.config.lang();
    let (s, w) = (lang.strings(), screen::Words::of(lang));
    let counts: Vec<String> = [
        (s.n_sittings, sittings),
        (s.n_books, books),
        (s.n_highlights, highlights),
    ]
    .into_iter()
    .filter(|(_, n)| *n > 0)
    .map(|(said, n)| counted(said, n as i64))
    .collect();
    match counts.is_empty() {
        true => w.nothing_new.to_string(),
        false => format!("{}: {}", w.added, counts.join(", ")),
    }
}

/// Set `key` in `Workspace::config` to `value`, as the Kindle's settings file spells it.
#[tauri::command]
fn set_setting(app: AppHandle, open: State<'_, Open>, key: String, value: String) -> Answer<()> {
    let mut ws = Workspace::create(&folder(&app, &open)?).map_err(said)?;
    let field = match key.as_str() {
        "language" => &mut ws.config.language,
        "week_start" => &mut ws.config.week_start,
        "color_scheme" => &mut ws.config.color_scheme,
        "figures" => &mut ws.config.figures,
        "sitting_floor" => &mut ws.config.sitting_floor,
        "show_unnamed" => &mut ws.config.show_unnamed,
        "show_uncovered" => &mut ws.config.show_uncovered,
        other => return Err(format!("{other} is no setting")),
    };
    *field = value;
    ws.save_config().map_err(said)
}

/// Show the folder picked in a dialog from here on. Answers whether one was.
#[tauri::command]
async fn choose_folder(app: AppHandle, open: State<'_, Open>) -> Answer<bool> {
    let Some(picked) = app.dialog().file().blocking_pick_folder() else {
        return Ok(false);
    };
    let dir = picked.into_path().map_err(|e| e.to_string())?;
    remember(&app, &dir)?;
    *open
        .dir
        .lock()
        .map_err(|_| "the record's folder could not be held".to_string())? = Some(dir);
    Ok(true)
}

/// A backup picked in a dialog, `None` where the dialog is cancelled.
#[tauri::command]
async fn pick_backup(app: AppHandle) -> Option<String> {
    app.dialog()
        .file()
        .add_filter("Reading Log", &["zip"])
        .blocking_pick_file()
        .map(|p| p.to_string())
}

/// One highlight's words as the window sets them; `text` `None` gives the
/// Kindle's own back.
#[derive(Debug, serde::Deserialize)]
struct Correction {
    id: HighlightId,
    text: Option<String>,
}

/// Set the words of each highlight `changes` names in the record of the
/// Kindle whose cut serial is `kindle`, empty for the backups naming none.
#[tauri::command]
fn edit_highlight(
    app: AppHandle,
    open: State<'_, Open>,
    kindle: String,
    changes: Vec<Correction>,
) -> Answer<()> {
    let ws = Workspace::open(&folder(&app, &open)?).map_err(said)?;
    let changes: Vec<(HighlightId, Option<String>)> =
        changes.into_iter().map(|c| (c.id, c.text)).collect();
    edits::correct_words(&ws, &kindle, &changes).map_err(said)
}

/// Write `text` as a new note of the Kindle whose cut serial is `kindle`, on
/// `on`. Answers the moment that names it.
#[tauri::command]
fn write_note(
    app: AppHandle,
    open: State<'_, Open>,
    kindle: String,
    on: notes::On,
    text: String,
) -> Answer<String> {
    let ws = Workspace::open(&folder(&app, &open)?).map_err(said)?;
    notes::write(&ws, &kindle, &on, &text).map_err(said)
}

/// Set the words of the note `written` names in the Kindle `kindle`'s notes.
#[tauri::command]
fn reword_note(
    app: AppHandle,
    open: State<'_, Open>,
    kindle: String,
    written: String,
    text: String,
) -> Answer<()> {
    let ws = Workspace::open(&folder(&app, &open)?).map_err(said)?;
    notes::reword(&ws, &kindle, &written, &text).map_err(said)
}

/// Delete the note `written` names in the Kindle `kindle`'s notes, once a
/// dialog is answered yes. Answers whether it went.
#[tauri::command]
async fn delete_note(
    app: AppHandle,
    open: State<'_, Open>,
    kindle: String,
    written: String,
) -> Answer<bool> {
    let ws = Workspace::open(&folder(&app, &open)?).map_err(said)?;
    let lang = ws.config.lang();
    let (s, w) = (lang.strings(), screen::Words::of(lang));
    if !ask(
        &app,
        w.delete_note_ask,
        w.delete_note_said.to_string(),
        s.delete_do,
        s.cancel,
    ) {
        return Ok(false);
    }
    notes::remove(&ws, &kindle, &written).map_err(said)?;
    Ok(true)
}

/// Show `path` in its folder.
#[tauri::command]
fn reveal(app: AppHandle, path: String) -> Answer<()> {
    app.opener()
        .reveal_item_in_dir(path)
        .map_err(|e| e.to_string())
}

/// Whether `asked` is answered `yes` in a warning dialog stating `note`.
fn ask(app: &AppHandle, asked: &str, note: String, yes: &str, no: &str) -> bool {
    app.dialog()
        .message(note)
        .title(asked)
        .kind(MessageDialogKind::Warning)
        .buttons(MessageDialogButtons::OkCancelCustom(yes.into(), no.into()))
        .blocking_show()
}

/// Delete the backup at `path`, one of `Workspace::every_backup`, once a
/// dialog stating what goes is answered yes. Answers whether it went.
#[tauri::command]
async fn delete_backup(app: AppHandle, open: State<'_, Open>, path: String) -> Answer<bool> {
    let ws = Workspace::open(&folder(&app, &open)?).map_err(said)?;
    let Some((_, held)) = ws
        .every_backup()
        .into_iter()
        .find(|(_, held)| held.path == Path::new(&path))
    else {
        return Err(format!("{path} is no backup in {}", ws.dir.display()));
    };
    let lang = ws.config.lang();
    let (s, w) = (lang.strings(), screen::Words::of(lang));
    let named = held.path.file_name().unwrap_or_default().to_string_lossy();
    let note = w
        .delete_note
        .replace("{file}", &named)
        .replace("{size}", &config::bytes(held.bytes));
    if !ask(&app, s.delete_ask, note, s.delete_do, s.cancel) {
        return Ok(false);
    }
    backup::remove(&held.path).map_err(|e| format!("{}: {e}", held.path.display()))?;
    Ok(true)
}

/// Delete every one of `Workspace::every_backup`, once a dialog stating what
/// goes is answered yes. Answers whether they went.
#[tauri::command]
async fn delete_backups(app: AppHandle, open: State<'_, Open>) -> Answer<bool> {
    let ws = Workspace::open(&folder(&app, &open)?).map_err(said)?;
    let every = ws.every_backup();
    if every.is_empty() {
        return Ok(false);
    }
    let lang = ws.config.lang();
    let (s, w) = (lang.strings(), screen::Words::of(lang));
    let weight = every.iter().map(|(_, held)| held.bytes).sum();
    let note = w
        .delete_all_note
        .replace("{n}", &every.len().to_string())
        .replace("{size}", &config::bytes(weight));
    if !ask(&app, w.delete_all_ask, note, w.delete_all_do, s.cancel) {
        return Ok(false);
    }
    for (_, held) in &every {
        backup::remove(&held.path).map_err(|e| format!("{}: {e}", held.path.display()))?;
    }
    Ok(true)
}

/// Remove everything the app keeps, once a dialog stating what goes is
/// answered yes: [`Workspace::reset`], then the file naming the folder last
/// opened. Answers what the window says after, empty where nothing went.
#[tauri::command]
async fn reset(app: AppHandle, open: State<'_, Open>) -> Answer<String> {
    let dir = folder(&app, &open)?;
    let ws = Workspace::open(&dir).map_err(said)?;
    if ws.kept().is_empty() {
        return Ok(String::new());
    }
    let lang = ws.config.lang();
    let (s, w) = (lang.strings(), screen::Words::of(lang));
    let (sittings, books) = shown(&open, &ws)?.map_or((0, 0), |held| {
        (held.record.sittings.len(), held.stats.book_count())
    });
    let what = format!(
        "{} · {}",
        counted(s.n_sittings, sittings as i64),
        counted(s.n_books, books as i64)
    );
    let note = w
        .reset_note
        .replace("{what}", &what)
        .replace("{folder}", &dir.to_string_lossy())
        .replace("{size}", &config::bytes(ws.kept_bytes()));
    if !ask(&app, w.reset_ask, note, w.reset_do, s.cancel) {
        return Ok(String::new());
    }
    ws.reset().map_err(said)?;
    forget(&app)?;
    Ok(w.reset_done.to_string())
}

/// Take away the file naming the folder last opened, where one stands.
fn forget(app: &AppHandle) -> Answer<()> {
    let config = app.path().app_config_dir().map_err(|e| e.to_string())?;
    match std::fs::remove_file(config.join(LAST)) {
        Err(err) if err.kind() != std::io::ErrorKind::NotFound => Err(err.to_string()),
        _ => Ok(()),
    }
}

/// Open the window.
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_opener::init())
        .manage(Open::default())
        .invoke_handler(tauri::generate_handler![
            page,
            kindle,
            today,
            import,
            sync_kindle,
            set_setting,
            edit_highlight,
            write_note,
            reword_note,
            delete_note,
            choose_folder,
            pick_backup,
            reveal,
            delete_backup,
            delete_backups,
            reset,
        ])
        .run(tauri::generate_context!())
        .expect("the window could not open");
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_correction_reads_as_the_window_sends_it() {
        // What `app.js` sends for a note saved and for a row's words given back.
        let saved: Vec<Correction> = serde_json::from_str(
            r#"[{"id":{"at":"2026-10-07T09:46:44","kind":"note","title":"A Therapeutic Journey","location":42,"start":-1},"text":"letting everyone down?"}]"#,
        )
        .unwrap();
        assert_eq!(saved[0].id.at, "2026-10-07T09:46:44");
        assert_eq!((saved[0].id.location, saved[0].id.start), (42, -1));
        assert_eq!(saved[0].text.as_deref(), Some("letting everyone down?"));
        let note: notes::On =
            serde_json::from_str(r#"{"book":{"key":"B00KEY","extent":7}}"#).unwrap();
        assert_eq!(
            note,
            notes::On::Book {
                key: "B00KEY".into(),
                extent: 7
            }
        );
        let given_back: Vec<Correction> = serde_json::from_str(
            r#"[{"id":{"at":"2026-10-07T09:46:51","kind":"highlight","title":"A Therapeutic Journey","location":42,"start":-1},"text":null}]"#,
        )
        .unwrap();
        assert_eq!(given_back[0].text, None);
    }
}
