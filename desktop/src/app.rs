//! The window: [`page`] draws the route its hash names through
//! [`screen::render`]; the other commands import, set a setting, delete a
//! backup, every backup or everything, and open the dialogs and the folder.

use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::SystemTime;

use tauri::{AppHandle, Manager, State};
use tauri_plugin_dialog::{DialogExt, MessageDialogButtons, MessageDialogKind};
use tauri_plugin_opener::OpenerExt;

use readinglog_native::backup::{self, cut_serial};
use readinglog_native::covers::COVERS_DIR;
use readinglog_native::date;
use readinglog_native::lang::counted;
use readinglog_native::record::Record;
use readinglog_native::view::config;

use crate::device::{self, DeviceInfo};
use crate::import::{self, Imported};
use crate::screen::{self, Covers, Page, Route, Shown};
use crate::workspace::{SITE_FILE, Workspace};

/// The folder the window shows, and the record as last built off it.
#[derive(Default)]
struct Open {
    dir: Mutex<Option<PathBuf>>,
    shown: Mutex<Option<(Stamp, Arc<Shown>)>>,
    /// The covers folders the asset protocol serves.
    served: Mutex<Vec<PathBuf>>,
}

/// When each record and `SITE_FILE` were last written, each record's size,
/// and the clock's day.
type Stamp = (
    PathBuf,
    Vec<(Option<SystemTime>, Option<u64>)>,
    Option<SystemTime>,
    i64,
);

fn stamp(ws: &Workspace, today: i64) -> Stamp {
    let records = ws
        .record_dirs()
        .iter()
        .map(|dir| {
            let record = std::fs::metadata(Record::file(dir)).ok();
            (
                record.as_ref().and_then(|m| m.modified().ok()),
                record.as_ref().map(|m| m.len()),
            )
        })
        .collect();
    let site = std::fs::metadata(ws.dir.join(SITE_FILE)).ok();
    (
        ws.dir.clone(),
        records,
        site.and_then(|m| m.modified().ok()),
        today,
    )
}

/// The Kindle plugged in that Import can read: a mounted one holding the app's
/// record, or one over MTP, which shows nothing without a session.
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

/// The folder last opened, where it holds a `SITE_FILE`.
fn last(app: &AppHandle) -> Option<PathBuf> {
    let file = app.path().app_config_dir().ok()?.join(LAST);
    let dir = PathBuf::from(std::fs::read_to_string(file).ok()?.trim());
    dir.join(SITE_FILE).is_file().then_some(dir)
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

/// The cut serial of the Kindle plugged in that Import can read.
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

/// Import the Kindle plugged in that the cut serial `serial` names, off the
/// window's thread. Answers what the record gained, in the window's words.
#[tauri::command]
async fn import_kindle(app: AppHandle, open: State<'_, Open>, serial: String) -> Answer<String> {
    let dir = folder(&app, &open)?;
    let ws = Workspace::create(&dir).map_err(said)?;
    remember(&app, &dir)?;
    let device = plugged()
        .filter(|d| cut_serial(&d.serial) == serial)
        .ok_or_else(|| "the Kindle is no longer plugged in".to_string())?;
    let at = ws.clone();
    let done = tauri::async_runtime::spawn_blocking(move || import::kindle(&at, &device))
        .await
        .map_err(|e| e.to_string())?
        .map_err(said)?;
    Ok(gained(&ws, &[done]))
}

/// What `done` added to the record, in `ws`'s language.
fn gained(ws: &Workspace, done: &[Imported]) -> String {
    let sittings: usize = done.iter().map(|d| d.sittings).sum();
    let books: usize = done.iter().map(|d| d.books).sum();
    let highlights: usize = done.iter().map(|d| d.highlights).sum();
    let lang = ws.site.lang();
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

/// Set `key` in `Workspace::site` to `value`, as the Kindle's settings file spells it.
#[tauri::command]
fn set_setting(app: AppHandle, open: State<'_, Open>, key: String, value: String) -> Answer<()> {
    let mut ws = Workspace::create(&folder(&app, &open)?).map_err(said)?;
    let field = match key.as_str() {
        "language" => &mut ws.site.language,
        "week_start" => &mut ws.site.week_start,
        "color_scheme" => &mut ws.site.color_scheme,
        "figures" => &mut ws.site.figures,
        "sitting_floor" => &mut ws.site.sitting_floor,
        "show_unnamed" => &mut ws.site.show_unnamed,
        "show_uncovered" => &mut ws.site.show_uncovered,
        other => return Err(format!("{other} is no setting")),
    };
    *field = value;
    ws.save_site().map_err(said)
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
    let lang = ws.site.lang();
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
    let lang = ws.site.lang();
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
    let lang = ws.site.lang();
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
            import_kindle,
            set_setting,
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
