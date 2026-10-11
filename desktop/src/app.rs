//! The window: Tauri over the modules beside this one. A command takes what
//! the window sends, calls one of them, and hands back what it answers; the
//! dialogs and the dropped files are the window's own.

use std::path::{Path, PathBuf};
use std::sync::Mutex;

use serde::Serialize;
use tauri::{AppHandle, Manager, State};
use tauri_plugin_dialog::DialogExt;
use tauri_plugin_opener::OpenerExt;

use readinglog_native::date;

use crate::import;
use crate::out;
use crate::posts;
use crate::shelf::{
    self, BookChange, BookRow, HighlightChange, HighlightId, HighlightRow, PostRow,
};
use crate::site;
use crate::workspace::{self, SITE_FILE, Setting, SiteToml, Workspace};

/// The workspace the window has open.
#[derive(Default)]
struct Open(Mutex<Option<PathBuf>>);

/// The file under the app's config folder naming the workspace last opened.
const LAST: &str = "workspace";

/// What a command answers: its value, or why not, in words.
type Answer<T> = Result<T, String>;

fn said(err: anyhow::Error) -> String {
    format!("{err:#}")
}

/// The workspace open, its `site.toml` read from disk on every call.
fn current(open: &State<'_, Open>) -> Answer<Workspace> {
    let dir = open
        .0
        .lock()
        .map_err(|_| "the workspace could not be read".to_string())?
        .clone()
        .ok_or("no workspace is open")?;
    Workspace::open(&dir).map_err(said)
}

fn hold(open: &State<'_, Open>, dir: &Path) -> Answer<()> {
    *open
        .0
        .lock()
        .map_err(|_| "the workspace could not be held".to_string())? = Some(dir.to_path_buf());
    Ok(())
}

/// The workspace last opened, where its folder holds a `site.toml`.
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

/// A workspace as the window shows it.
#[derive(Serialize)]
struct WorkspaceView {
    dir: String,
    /// The folder the site is written to.
    out: String,
    site: SiteToml,
    /// What `site.toml` states that no Kindle settings file holds.
    problems: Vec<String>,
    backups: usize,
}

fn view(ws: &Workspace) -> WorkspaceView {
    let backups = std::fs::read_dir(ws.backups())
        .map(|entries| {
            entries
                .flatten()
                .filter(|e| e.path().extension().is_some_and(|x| x == "zip"))
                .count()
        })
        .unwrap_or(0);
    WorkspaceView {
        dir: ws.dir.to_string_lossy().into_owned(),
        out: ws.out().to_string_lossy().into_owned(),
        problems: ws.site.settings().1,
        site: ws.site.clone(),
        backups,
    }
}

/// The workspace open, else the one last opened.
#[tauri::command]
fn workspace(app: AppHandle, open: State<'_, Open>) -> Answer<Option<WorkspaceView>> {
    if let Ok(ws) = current(&open) {
        return Ok(Some(view(&ws)));
    }
    let Some(dir) = last(&app) else {
        return Ok(None);
    };
    let ws = Workspace::open(&dir).map_err(said)?;
    hold(&open, &dir)?;
    Ok(Some(view(&ws)))
}

/// Open the workspace at `dir`, making one there where none stands.
#[tauri::command]
fn open_workspace(app: AppHandle, open: State<'_, Open>, dir: String) -> Answer<WorkspaceView> {
    let ws = Workspace::create(Path::new(&dir)).map_err(said)?;
    remember(&app, &ws.dir)?;
    hold(&open, &ws.dir)?;
    Ok(view(&ws))
}

/// The settings `site.toml` takes, in the site's language.
#[tauri::command]
fn offered(open: State<'_, Open>) -> Vec<Setting> {
    let lang = current(&open).map(|ws| ws.site.lang()).unwrap_or_default();
    workspace::offered(lang)
}

#[tauri::command]
fn save_site(open: State<'_, Open>, site: SiteToml) -> Answer<WorkspaceView> {
    let mut ws = current(&open)?;
    ws.site = site;
    ws.save_site().map_err(said)?;
    Ok(view(&ws))
}

#[tauri::command]
fn books(open: State<'_, Open>) -> Answer<Vec<BookRow>> {
    shelf::books(&current(&open)?).map_err(said)
}

#[tauri::command]
fn change_book(open: State<'_, Open>, key: String, extent: i64, change: BookChange) -> Answer<()> {
    shelf::change_book(&current(&open)?, &key, extent, change).map_err(said)
}

#[tauri::command]
fn show_all(open: State<'_, Open>, shown: bool) -> Answer<()> {
    shelf::show_all(&current(&open)?, shown).map_err(said)
}

#[tauri::command]
fn highlights(open: State<'_, Open>, key: String, extent: i64) -> Answer<Vec<HighlightRow>> {
    shelf::highlights(&current(&open)?, &key, extent).map_err(said)
}

#[tauri::command]
fn change_highlight(open: State<'_, Open>, id: HighlightId, change: HighlightChange) -> Answer<()> {
    shelf::change_highlight(&current(&open)?, &id, change).map_err(said)
}

#[derive(Serialize)]
struct PostsView {
    rows: Vec<PostRow>,
    problems: Vec<String>,
}

#[tauri::command]
fn posts(open: State<'_, Open>) -> Answer<PostsView> {
    let (rows, problems) = shelf::post_rows(&current(&open)?);
    Ok(PostsView { rows, problems })
}

#[tauri::command]
fn read_post(open: State<'_, Open>, name: String) -> Answer<String> {
    shelf::read_post(&current(&open)?, &name).map_err(said)
}

#[tauri::command]
fn write_post(open: State<'_, Open>, name: String, text: String) -> Answer<()> {
    shelf::write_post(&current(&open)?, &name, &text).map_err(said)
}

/// A new draft titled `title`, dated today, naming `books`. Answers its name.
#[tauri::command]
fn new_post(open: State<'_, Open>, title: String, books: Vec<String>) -> Answer<String> {
    let (today, _) = date::now();
    shelf::new_post(&current(&open)?, &title, today, &books).map_err(said)
}

/// A post as its page states it, saved or not, draft or not: its title and its
/// words.
#[derive(Serialize)]
struct Rendered {
    title: String,
    html: String,
}

#[tauri::command]
fn render_post(name: String, text: String) -> Answer<Rendered> {
    let post = posts::read(&name, &text)?;
    Ok(Rendered {
        title: post.title,
        html: posts::html(&post.body),
    })
}

/// What an import did, as the window states it.
#[derive(Serialize)]
struct ImportView {
    file: String,
    again: bool,
    sittings: usize,
    books: usize,
    highlights: usize,
    pictures: usize,
    written: String,
    app: String,
    kindle: String,
    firmware: String,
    first: String,
    last: String,
}

/// Import `path`: a backup, or a Kindle's drive or its `readinglog` folder.
#[tauri::command]
fn import(open: State<'_, Open>, path: String) -> Answer<ImportView> {
    let ws = current(&open)?;
    let from = Path::new(&path);
    let done = match from.is_dir() {
        true => import::kindle(&ws, from),
        false => import::backup(&ws, from),
    }
    .map_err(said)?;
    Ok(ImportView {
        file: done
            .kept
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_default(),
        again: done.again,
        sittings: done.sittings,
        books: done.books,
        highlights: done.highlights,
        pictures: done.pictures,
        written: done.about.written,
        app: done.about.app,
        kindle: done.about.kindle,
        firmware: done.about.firmware,
        first: done.about.first,
        last: done.about.last,
    })
}

/// Where the preview stands, and the address of each page on it.
#[derive(Serialize)]
struct PreviewView {
    dir: String,
    books: Vec<PlacedView>,
    posts: Vec<String>,
    problems: Vec<String>,
}

#[derive(Serialize)]
struct PlacedView {
    key: String,
    extent: i64,
    slug: String,
}

/// Build the site into the app's own preview folder, which the window shows.
#[tauri::command]
fn preview(app: AppHandle, open: State<'_, Open>) -> Answer<PreviewView> {
    let ws = current(&open)?;
    let built = site::build(&ws).map_err(said)?;
    let dir = app
        .path()
        .app_cache_dir()
        .map_err(|e| e.to_string())?
        .join("preview");
    out::write(&dir, &built.files, &built.items()).map_err(said)?;
    app.asset_protocol_scope()
        .allow_directory(&dir, true)
        .map_err(|e| e.to_string())?;
    Ok(PreviewView {
        dir: dir.to_string_lossy().into_owned(),
        books: built
            .books
            .iter()
            .map(|p| PlacedView {
                key: p.key.clone(),
                extent: p.extent,
                slug: p.slug.clone(),
            })
            .collect(),
        posts: built.posts,
        problems: built.problems,
    })
}

/// What a write makes public that is not yet.
#[derive(Serialize)]
struct PlanView {
    out: String,
    first: bool,
    fresh: Vec<FreshView>,
    leaving: usize,
    problems: Vec<String>,
}

#[derive(Serialize)]
struct FreshView {
    /// `book`, `post` or `highlight`.
    kind: String,
    said: String,
    book: String,
}

#[tauri::command]
fn plan(open: State<'_, Open>) -> Answer<PlanView> {
    let planned = site::plan(&current(&open)?).map_err(said)?;
    Ok(PlanView {
        out: planned.out.to_string_lossy().into_owned(),
        first: planned.first,
        fresh: planned
            .fresh
            .into_iter()
            .map(|l| FreshView {
                kind: l.item.split(' ').next().unwrap_or_default().to_string(),
                said: l.said,
                book: l.book,
            })
            .collect(),
        leaving: planned.leaving,
        problems: planned.site.problems,
    })
}

/// What writing the site did.
#[derive(Serialize)]
struct PublishedView {
    out: String,
    index: String,
    changed: usize,
    removed: usize,
    unchanged: usize,
    problems: Vec<String>,
}

#[tauri::command]
fn publish(open: State<'_, Open>) -> Answer<PublishedView> {
    let ws = current(&open)?;
    let done = site::publish(&ws).map_err(said)?;
    Ok(PublishedView {
        out: ws.out().to_string_lossy().into_owned(),
        index: ws.out().join("index.html").to_string_lossy().into_owned(),
        changed: done.written.changed.len(),
        removed: done.written.removed.len(),
        unchanged: done.written.unchanged,
        problems: done.site.problems,
    })
}

// The dialogs and the opener are commands of this app. A blocking pick waits on
// the worker thread an async command runs on, never the main one.

/// A folder picked in a dialog, `None` where the dialog is cancelled.
#[tauri::command]
async fn pick_folder(app: AppHandle, title: String) -> Option<String> {
    app.dialog()
        .file()
        .set_title(title)
        .blocking_pick_folder()
        .map(|p| p.to_string())
}

/// A backup picked in a dialog, `None` where the dialog is cancelled.
#[tauri::command]
async fn pick_backup(app: AppHandle) -> Option<String> {
    app.dialog()
        .file()
        .set_title("A Reading Log backup")
        .add_filter("Backup", &["zip"])
        .blocking_pick_file()
        .map(|p| p.to_string())
}

/// Open `path` as the system opens it: a page in the browser.
#[tauri::command]
fn open_path(app: AppHandle, path: String) -> Answer<()> {
    app.opener()
        .open_path(path, None::<&str>)
        .map_err(|e| e.to_string())
}

/// Show `path` in its folder.
#[tauri::command]
fn reveal(app: AppHandle, path: String) -> Answer<()> {
    app.opener()
        .reveal_item_in_dir(path)
        .map_err(|e| e.to_string())
}

/// Open the window.
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_opener::init())
        .manage(Open::default())
        .invoke_handler(tauri::generate_handler![
            workspace,
            open_workspace,
            offered,
            save_site,
            books,
            change_book,
            show_all,
            highlights,
            change_highlight,
            posts,
            read_post,
            write_post,
            new_post,
            render_post,
            import,
            preview,
            plan,
            publish,
            pick_folder,
            pick_backup,
            open_path,
            reveal,
        ])
        .run(tauri::generate_context!())
        .expect("the window could not open");
}
