//! Fetching a newer Reading Log from the project's own GitHub releases. The
//! extension folder also holds the reading record, so [`place`] moves the
//! archive's files over their counterparts and never swaps the folder.

pub mod archive;
pub mod http;

use std::fs;
use std::io::Read;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicBool, Ordering};

use crate::lang::Strings;

/// The version this build is, as `Cargo.toml` states it. What a release tag is
/// held against.
pub const VERSION: &str = env!("CARGO_PKG_VERSION");

/// The project, as GitHub names it. One spelling: the API URL, the page a
/// failure sends someone to, and the `User-Agent` all read it.
pub const REPO: &str = "huangziwei/readinglog";

/// Where to go on a computer when this cannot do it. Short enough to be typed
/// off a screen, which is the whole point of it.
pub const RELEASES_URL: &str = "github.com/huangziwei/readinglog/releases";

/// The folder this app installs into, and the folder it keeps its record in.
/// Only the files the archive carries are replaced; `sessions.tsv`, `settings`
/// and `covers/` are not in it and are never touched.
pub const EXTENSION_DIR: &str = "/mnt/us/extensions/readinglog";

/// An entry that names the archive's own root: `extensions/readinglog/bin/`
/// plus this. Also the last file [`place`] moves.
pub const MARKER: &str = "bin/readinglog";

/// Where the archive lands and where it is unpacked, both under the folder
/// they replace: `/mnt/us/extensions/` is what KUAL builds its menu from, and
/// a staging folder left there would stand in it as a second Reading Log.
const ARCHIVE: &str = ".new.zip";
const STAGING: &str = ".new";

/// Releases to look through, newest first. Well past the depth at which this
/// project last published an archive.
const RELEASES_PER_PAGE: u32 = 30;

/// Is this one of the archives a release publishes? The version rides the
/// filename, and the `.sha256` sidecar beside it must not match.
fn is_asset(name: &str) -> bool {
    name.starts_with("readinglog-") && name.ends_with("-kindle.zip")
}

/// The release an update would install.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Release {
    pub tag: String,
    /// The asset to download.
    pub url: String,
    pub name: String,
    /// The `.sha256` sidecar the release workflow publishes beside the zip.
    pub sha: Option<String>,
}

/// One release as the GitHub API serves it, cut to what [`pick_release`] reads.
#[derive(Debug, Clone, serde::Deserialize)]
pub struct ApiRelease {
    pub tag_name: String,
    #[serde(default)]
    pub draft: bool,
    #[serde(default)]
    pub prerelease: bool,
    #[serde(default)]
    pub assets: Vec<ApiAsset>,
}

#[derive(Debug, Clone, serde::Deserialize)]
pub struct ApiAsset {
    pub name: String,
    pub browser_download_url: String,
}

/// What the update is doing now, for the banner drawing it. No words: `lang`
/// owns every one this app draws, and this runs on a worker thread.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Doing {
    /// Asking GitHub what the newest release is.
    Asking,
    /// Fetching the archive.
    Downloading { got: u64, total: Option<u64> },
    /// Reading the archive out, and proving the binary in it runs here.
    Checking,
    /// Moving the new copy into place.
    Placing,
}

impl Doing {
    /// The banner: a headline, and the lines under it. The second is blank
    /// until there is a figure, so the verb over it holds still.
    pub fn banner(&self, s: &Strings) -> (String, Vec<String>) {
        let said = match self {
            Doing::Asking => s.update_asking,
            Doing::Downloading { .. } => s.update_downloading,
            Doing::Checking => s.update_checking,
            Doing::Placing => s.update_placing,
        };
        (s.update_row.into(), vec![said.into(), self.got()])
    }

    /// Can a tap still stop it? The flag is read while the release list is
    /// awaited and between chunks of the download; past that the copy is
    /// being moved into place, and stopping would leave it half replaced.
    pub fn stoppable(&self) -> bool {
        matches!(self, Doing::Asking | Doing::Downloading { .. })
    }

    /// How far through, where that can be said, and `""` where it cannot.
    pub fn got(&self) -> String {
        match self {
            Doing::Downloading { got, total } => transferred(*got, *total),
            _ => String::new(),
        }
    }
}

/// Why an update did not go through. One line each, and the banner says to
/// fetch it on a computer whichever it is.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Failure {
    /// GitHub could not be reached, or would not answer.
    NoAnswer,
    /// It answered, and no release it lists carries an archive.
    NoRelease,
    /// What arrived is not the archive the release published.
    BadDownload,
    /// The binary inside does not run on this Kindle.
    WrongBuild,
    /// It runs, and the folder would not take it.
    NotPlaced,
}

impl Failure {
    /// The one line that says what went wrong.
    pub fn line(self, s: &Strings) -> &'static str {
        match self {
            Failure::NoAnswer => s.update_no_answer,
            Failure::NoRelease => s.update_no_release,
            Failure::BadDownload => s.update_bad_download,
            Failure::WrongBuild => s.update_wrong_build,
            Failure::NotPlaced => s.update_not_placed,
        }
    }
}

/// How an update ended.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Outcome {
    /// No route off this Kindle. Said before anything is asked for: the
    /// resolver's own timeout is the difference between this and a hang.
    Offline,
    /// The newest release is the one already running.
    UpToDate,
    /// In place, at this version. Nothing on screen is running it yet.
    Installed(String),
    /// A tap on the banner, mid-transfer.
    Stopped,
    Failed(Failure),
}

impl Outcome {
    /// The banner: a headline, and the lines under it. Every ending that is
    /// not an update says where to get one by hand — there is no browser
    /// here, so the line is an address to type into a computer.
    pub fn banner(&self, s: &Strings) -> (String, Vec<String>) {
        match self {
            Outcome::UpToDate => (
                s.update_up_to_date.into(),
                vec![crate::lang::at_version(s.update_this_version, VERSION)],
            ),
            Outcome::Installed(version) => (
                crate::lang::at_version(s.update_installed, version),
                vec![s.update_reopen.into()],
            ),
            Outcome::Stopped => (s.update_stopped.into(), Vec::new()),
            Outcome::Offline => (s.update_failed.into(), by_hand(s.update_offline, s)),
            Outcome::Failed(why) => (s.update_failed.into(), by_hand(why.line(s), s)),
        }
    }
}

/// `why`, then where to go instead.
fn by_hand(why: &str, s: &Strings) -> Vec<String> {
    vec![why.into(), s.update_by_hand.into(), RELEASES_URL.into()]
}

//------------------------------------------------------------------------------
// Pure: what to fetch, and what to check it against
//------------------------------------------------------------------------------

/// The newest release in `releases` carrying an archive. Drafts and
/// prereleases are passed over: a prerelease is published to be tried on one
/// device, and offering it to every device is not what publishing one means.
pub fn pick_release(releases: &[ApiRelease]) -> Option<Release> {
    for release in releases {
        if release.draft || release.prerelease {
            continue;
        }
        let found = release.assets.iter().find(|a| is_asset(&a.name))?;
        let sidecar = format!("{}.sha256", found.name);
        return Some(Release {
            tag: release.tag_name.clone(),
            url: found.browser_download_url.clone(),
            name: found.name.clone(),
            sha: release
                .assets
                .iter()
                .find(|a| a.name == sidecar)
                .map(|a| a.browser_download_url.clone()),
        });
    }
    None
}

/// A version as its dotted numbers. A leading `v` and anything trailing a
/// number — `-rc1`, `+build` — are not read.
fn numbers(version: &str) -> Vec<u64> {
    version
        .trim()
        .trim_start_matches(['v', 'V'])
        .split('.')
        .map(|part| {
            part.chars()
                .take_while(char::is_ascii_digit)
                .collect::<String>()
                .parse()
                .unwrap_or(0)
        })
        .collect()
}

/// Is `offered` a later version than `running`? A missing part is a zero, and
/// equal is not later: an update is offered only when there is one.
pub fn newer(offered: &str, running: &str) -> bool {
    let (offered, running) = (numbers(offered), numbers(running));
    for at in 0..offered.len().max(running.len()) {
        let (a, b) = (
            offered.get(at).copied().unwrap_or(0),
            running.get(at).copied().unwrap_or(0),
        );
        if a != b {
            return a > b;
        }
    }
    false
}

/// The `sha256sum`-style line for `name`, or the whole file when it carries a
/// bare digest.
pub fn digest_from(text: &str, name: &str) -> Option<String> {
    for line in text.lines() {
        let line = line.trim();
        let mut parts = line.splitn(2, char::is_whitespace);
        let digest = parts.next().unwrap_or_default();
        if !is_sha256(digest) {
            continue;
        }
        match parts.next() {
            // A file holding nothing but the digest.
            None => return Some(digest.to_ascii_lowercase()),
            Some(rest) => {
                // `sha256sum -b` marks a binary with a `*`, and a digest taken
                // from a build directory names the file by its whole path.
                let named = rest.trim_start().trim_start_matches('*');
                if named == name || named.rsplit('/').next() == Some(name) {
                    return Some(digest.to_ascii_lowercase());
                }
            }
        }
    }
    None
}

fn is_sha256(s: &str) -> bool {
    s.len() == 64 && s.bytes().all(|b| b.is_ascii_hexdigit())
}

/// `got` against `total`, for the banner.
fn transferred(got: u64, total: Option<u64>) -> String {
    match total {
        Some(total) if total > 0 => format!("{}%", got * 100 / total),
        // No `Content-Length`: the count is all there is to say.
        _ => format!("{:.1} MB", got as f64 / (1024.0 * 1024.0)),
    }
}

//------------------------------------------------------------------------------
// The whole flow
//------------------------------------------------------------------------------

/// Ask, fetch and install, into [`EXTENSION_DIR`]. Blocking, and meant for a
/// worker thread: `cancel` is what a tap on the banner sets, and it is read
/// between chunks of the download.
pub fn run(cancel: &AtomicBool, say: &dyn Fn(Doing)) -> Outcome {
    run_into(Path::new(EXTENSION_DIR), cancel, say)
}

/// [`run`] against a named folder, which the tests give one of their own.
pub fn run_into(dest: &Path, cancel: &AtomicBool, say: &dyn Fn(Doing)) -> Outcome {
    if crate::net::is_offline() {
        return Outcome::Offline;
    }
    say(Doing::Asking);

    let client = http::Client::new();
    let release = match available(&client) {
        Ok(release) => release,
        Err(why) => return Outcome::Failed(why),
    };
    // A tap while GitHub was being waited on. Read here as well as between
    // chunks of the download, so a tap is answered the same way wherever it
    // lands while [`Doing::stoppable`] holds.
    if cancel.load(Ordering::Relaxed) {
        return Outcome::Stopped;
    }
    eprintln!("update: running {VERSION}, newest {}", release.tag);
    if !newer(&release.tag, VERSION) {
        return Outcome::UpToDate;
    }

    match fetch(&client, &release, dest, say, cancel) {
        Ok(()) => {
            eprintln!("update: {} into {}", release.tag, dest.display());
            Outcome::Installed(release.tag)
        }
        Err(None) => Outcome::Stopped,
        Err(Some(why)) => {
            eprintln!("update: {why:?}");
            Outcome::Failed(why)
        }
    }
}

/// The release an update would install.
pub fn available(client: &http::Client) -> Result<Release, Failure> {
    let url = format!("https://api.github.com/repos/{REPO}/releases?per_page={RELEASES_PER_PAGE}");
    let body = client
        .text(&url, "application/vnd.github+json")
        .map_err(|e| {
            eprintln!("update: {e}");
            Failure::NoAnswer
        })?;

    let releases: Vec<ApiRelease> = serde_json::from_str(&body).map_err(|e| {
        eprintln!("update: unreadable release list: {e}");
        Failure::NoAnswer
    })?;

    pick_release(&releases).ok_or(Failure::NoRelease)
}

/// Download, unpack, prove and move in. `Err(None)` is a tap on the banner.
/// Nothing in `dest` is touched until the staged copy has stated its own
/// version, so a failure before [`place`] leaves the install as it was.
fn fetch(
    client: &http::Client,
    release: &Release,
    dest: &Path,
    say: &dyn Fn(Doing),
    cancel: &AtomicBool,
) -> Result<(), Option<Failure>> {
    let staging = dest.join(STAGING);
    let zip = dest.join(ARCHIVE);
    let _ = fs::remove_dir_all(&staging);
    let _ = fs::remove_file(&zip);

    // Every step is a full-screen repaint and a percentage moves several times
    // a second; a mark that changes every other percent is enough to look
    // live without flashing the panel.
    let last = std::cell::Cell::new(u64::MAX);
    let progress = |got: u64, total: Option<u64>| {
        let mark = match total {
            Some(total) if total > 0 => got * 50 / total,
            _ => got / (512 * 1024),
        };
        if mark != last.get() {
            last.set(mark);
            say(Doing::Downloading { got, total });
        }
    };
    say(Doing::Downloading {
        got: 0,
        total: None,
    });
    if let Err(e) = client.download(&release.url, &zip, cancel, &progress) {
        eprintln!("update: {e}");
        return Err(match e {
            http::Error::Cancelled => None,
            _ => Some(Failure::BadDownload),
        });
    }

    say(Doing::Checking);
    if let Some(sidecar) = &release.sha
        && !matches(client, sidecar, &release.name, &zip)
    {
        let _ = fs::remove_file(&zip);
        return Err(Some(Failure::BadDownload));
    }

    let unpacked = archive::unpack(&zip, MARKER, &staging);
    let _ = fs::remove_file(&zip);
    match unpacked {
        Ok(written) => eprintln!("update: {written} files into {}", staging.display()),
        Err(e) => {
            eprintln!("update: {e}");
            let _ = fs::remove_dir_all(&staging);
            return Err(Some(Failure::BadDownload));
        }
    }

    // The archive may or may not carry Unix modes depending on what wrote it,
    // so nothing under bin/ is assumed to have come out executable.
    mark_executable(&staging.join("bin"));

    if !states_version(&staging.join(MARKER), &release.tag) {
        let _ = fs::remove_dir_all(&staging);
        return Err(Some(Failure::WrongBuild));
    }

    say(Doing::Placing);
    let placed = place(&staging, dest);
    let _ = fs::remove_dir_all(&staging);
    placed.map_err(Some)
}

/// `<path>.<suffix>`, which shares a parent with `path` so a rename between
/// the two stays on one filesystem.
fn beside(path: &Path, suffix: &str) -> PathBuf {
    PathBuf::from(format!("{}.{suffix}", path.display()))
}

/// Does the staged copy run on this Kindle, and is it the release it came
/// from? `--version` opens neither the display nor a log, so running it
/// settles the ABI, that it starts, and which release the archive holds.
fn states_version(exe: &Path, tag: &str) -> bool {
    let Ok(out) = Command::new(exe).arg("--version").output() else {
        eprintln!("update: the staged copy would not run");
        return false;
    };
    let said = String::from_utf8_lossy(&out.stdout).trim().to_string();
    // The same version by [`newer`]'s reckoning, which is the one the offer
    // was made under: neither is later than the other.
    let same = !newer(&said, tag) && !newer(tag, &said);
    if !out.status.success() || !same {
        eprintln!("update: staged copy says {said:?}, release says {tag:?}");
        return false;
    }
    true
}

/// Every file under `staging` over its counterpart in `dest`. [`MARKER`] goes
/// last: it is the file this process runs from, so a move that fails partway
/// leaves a runnable folder rather than an empty one.
fn place(staging: &Path, dest: &Path) -> Result<(), Failure> {
    let mut files = Vec::new();
    walk(staging, Path::new(""), &mut files);
    files.sort();
    files.sort_by_key(|rel| rel == Path::new(MARKER));

    for rel in &files {
        let to = dest.join(rel);
        if let Some(parent) = to.parent() {
            let _ = fs::create_dir_all(parent);
        }
        if !replace(&staging.join(rel), &to) {
            eprintln!("update: {} would not go into place", rel.display());
            return Err(Failure::NotPlaced);
        }
    }
    Ok(())
}

/// Every file under `dir`, as a path relative to `at`.
fn walk(dir: &Path, at: &Path, out: &mut Vec<PathBuf>) {
    let Ok(entries) = fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        let rel = at.join(entry.file_name());
        match path.is_dir() {
            true => walk(&path, &rel, out),
            false => out.push(rel),
        }
    }
}

/// `from` over `to`, whether or not anything stands at `to`. The old copy is
/// moved aside first: one of these is the running binary, and renaming a busy
/// file *away* is allowed on filesystems where renaming over it is not.
fn replace(from: &Path, to: &Path) -> bool {
    let aside = beside(to, "old");
    let _ = fs::remove_file(&aside);
    let stood = to.exists();
    if stood && fs::rename(to, &aside).is_err() {
        return false;
    }
    if fs::rename(from, to).is_ok() {
        // A copy still open cannot always be unlinked. Left where it is, the
        // next update clears it above.
        let _ = fs::remove_file(&aside);
        return true;
    }
    if stood {
        let _ = fs::rename(&aside, to);
    }
    false
}

/// The downloaded file against the digest the release publishes for it. A
/// sidecar that cannot be read is not a mismatch; the gate that holds either
/// way is [`states_version`].
fn matches(client: &http::Client, sidecar: &str, name: &str, zip: &Path) -> bool {
    let Ok(text) = client.text(sidecar, "text/plain") else {
        eprintln!("update: the checksum could not be read");
        return true;
    };
    let Some(want) = digest_from(&text, name) else {
        eprintln!("update: the checksum names no digest for this file");
        return true;
    };
    let Some(got) = digest_of(zip) else {
        eprintln!("update: the download could not be read back");
        return true;
    };
    if want != got {
        eprintln!("update: checksum wanted {want}, got {got}");
        return false;
    }
    true
}

/// SHA-256 of `path`, hex.
fn digest_of(path: &Path) -> Option<String> {
    use sha2::{Digest, Sha256};

    let mut file = fs::File::open(path).ok()?;
    let mut hasher = Sha256::new();
    let mut buf = vec![0u8; 64 * 1024];
    loop {
        let n = file.read(&mut buf).ok()?;
        if n == 0 {
            break;
        }
        hasher.update(&buf[..n]);
    }
    Some(
        hasher
            .finalize()
            .iter()
            .map(|b| format!("{b:02x}"))
            .collect(),
    )
}

/// Every file directly under `dir`, executable. Best effort: the partition is
/// FAT and its modes come from the mount as often as from the file.
fn mark_executable(dir: &Path) {
    let Ok(entries) = fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        let Ok(meta) = fs::metadata(&path) else {
            continue;
        };
        if meta.is_file() {
            let mut perms = meta.permissions();
            perms.set_mode(perms.mode() | 0o755);
            let _ = fs::set_permissions(&path, perms);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::lang::Lang;

    fn en() -> &'static Strings {
        Lang::English.strings()
    }

    fn tmpdir(name: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("readinglog-update-{name}"));
        let _ = fs::remove_dir_all(&d);
        fs::create_dir_all(&d).unwrap();
        d
    }

    fn asset(name: &str) -> ApiAsset {
        ApiAsset {
            name: name.into(),
            browser_download_url: format!("https://example.invalid/{name}"),
        }
    }

    fn release(tag: &str, assets: Vec<ApiAsset>) -> ApiRelease {
        ApiRelease {
            tag_name: tag.into(),
            draft: false,
            prerelease: false,
            assets,
        }
    }

    /// The list as the workflow leaves it: the zip and its sidecar.
    fn published(tag: &str) -> ApiRelease {
        release(
            tag,
            vec![
                asset(&format!("readinglog-{tag}-kindle.zip")),
                asset(&format!("readinglog-{tag}-kindle.zip.sha256")),
            ],
        )
    }

    #[test]
    fn the_archive_is_taken_by_pattern_rather_than_by_version() {
        let picked = pick_release(&[published("v0.2.0"), published("v0.1.0")]).unwrap();
        assert_eq!(picked.tag, "v0.2.0");
        assert_eq!(picked.name, "readinglog-v0.2.0-kindle.zip");
        assert!(picked.url.ends_with("readinglog-v0.2.0-kindle.zip"));
        // The workflow publishes a checksum beside it.
        assert_eq!(
            picked.sha.as_deref(),
            Some("https://example.invalid/readinglog-v0.2.0-kindle.zip.sha256")
        );
    }

    #[test]
    fn the_sidecar_is_never_taken_for_the_archive() {
        assert!(is_asset("readinglog-v0.1.0-kindle.zip"));
        assert!(is_asset("readinglog-v0.10.3-kindle.zip"));
        assert!(!is_asset("readinglog-v0.1.0-kindle.zip.sha256"));
        assert!(!is_asset("Source code (zip)"));
        assert!(!is_asset("readinglog-v0.1.0.zip"));
    }

    #[test]
    fn a_draft_or_a_prerelease_is_not_offered_to_every_device() {
        let mut draft = published("v0.3.0");
        draft.draft = true;
        let mut early = published("v0.2.0");
        early.prerelease = true;
        let list = vec![draft, early, published("v0.1.0")];
        assert_eq!(pick_release(&list).unwrap().tag, "v0.1.0");
        assert_eq!(pick_release(&[]), None);
    }

    #[test]
    fn a_tag_published_before_its_assets_is_passed_over() {
        let list = vec![release("v0.9.9", Vec::new()), published("v0.2.0")];
        // `pick_release` reads the newest release *carrying* an archive.
        assert_eq!(pick_release(&list), None);
        // A release with something else attached is not one either.
        let other = release("v0.9.9", vec![asset("Source code (zip)")]);
        assert_eq!(pick_release(&[other]), None);
    }

    #[test]
    fn a_version_is_later_only_when_it_is_later() {
        assert!(newer("v0.2.0", "0.1.0"));
        assert!(newer("v0.1.10", "0.1.9"));
        assert!(newer("v1.0.0", "0.99.99"));
        assert!(!newer("v0.1.0", "0.1.0"));
        assert!(!newer("v0.1.0", "0.2.0"));
        assert!(!newer("v0.1.9", "0.1.10"));
        // A missing part is a zero, either way round.
        assert!(!newer("v1", "1.0.0"));
        assert!(!newer("v1.0.0", "1"));
        assert!(newer("v1.0.1", "1"));
        // Nonsense is not later than something.
        assert!(!newer("", VERSION));
        assert!(!newer("latest", VERSION));
    }

    #[test]
    fn this_build_is_not_an_update_to_itself() {
        assert!(!newer(VERSION, VERSION));
        assert!(!newer(&format!("v{VERSION}"), VERSION));
    }

    #[test]
    fn a_digest_is_read_off_whichever_shape_the_sidecar_takes() {
        let d = "a".repeat(64);
        let name = "readinglog-v0.1.0-kindle.zip";
        assert_eq!(digest_from(&format!("{d}  {name}"), name), Some(d.clone()));
        // `sha256sum -b` marks a binary file.
        assert_eq!(digest_from(&format!("{d} *{name}"), name), Some(d.clone()));
        // A digest taken in a build directory names the whole path.
        assert_eq!(
            digest_from(&format!("{d}  /tmp/build/{name}"), name),
            Some(d.clone())
        );
        // A file holding nothing but the digest is for whatever it came with.
        assert_eq!(digest_from(&format!("{d}\n"), name), Some(d.clone()));
        assert_eq!(
            digest_from(&format!("{}  f.zip", "A".repeat(64)), "f.zip"),
            Some("a".repeat(64))
        );
        assert_eq!(digest_from("abc  f.zip", "f.zip"), None);
        assert_eq!(digest_from("", "f.zip"), None);
        // A sidecar for a different file is not this file's digest.
        assert_eq!(digest_from(&format!("{d}  other.zip"), name), None);
    }

    #[test]
    fn a_file_hashes_to_what_sha256sum_would_say() {
        let dir = tmpdir("digest");
        let path = dir.join("f.txt");
        fs::write(&path, b"abc").unwrap();
        assert_eq!(
            digest_of(&path).as_deref(),
            Some("ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad")
        );
        assert_eq!(digest_of(&dir.join("absent")), None);
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn the_transferred_line_prefers_a_percentage_and_falls_back_to_a_count() {
        assert_eq!(transferred(0, Some(200)), "0%");
        assert_eq!(transferred(100, Some(200)), "50%");
        assert_eq!(transferred(200, Some(200)), "100%");
        // No `Content-Length`, or one that says nothing.
        assert_eq!(transferred(1024 * 1024, None), "1.0 MB");
        assert_eq!(transferred(1024 * 1024, Some(0)), "1.0 MB");
        // Only a download has a figure to state.
        assert_eq!(Doing::Asking.got(), "");
        assert_eq!(Doing::Placing.got(), "");
        assert_eq!(
            Doing::Downloading {
                got: 50,
                total: Some(100)
            }
            .got(),
            "50%"
        );
    }

    /// An executable at `path` that prints `says` and exits `code`.
    fn stub(path: &Path, says: &str, code: i32) {
        fs::write(path, format!("#!/bin/sh\necho '{says}'\nexit {code}\n")).unwrap();
        let mut perms = fs::metadata(path).unwrap().permissions();
        perms.set_mode(0o755);
        fs::set_permissions(path, perms).unwrap();
    }

    #[test]
    fn the_staged_copy_has_to_state_the_version_it_came_from() {
        let dir = tmpdir("states");
        let exe = dir.join("readinglog");

        stub(&exe, "0.2.0", 0);
        assert!(states_version(&exe, "v0.2.0"), "the tag it came from");
        assert!(states_version(&exe, "0.2.0"), "with or without the v");
        assert!(!states_version(&exe, "v0.3.0"), "some other release");
        assert!(!states_version(&exe, "v0.1.0"), "an older release");
        // Read the way the offer was made: a missing part is a zero.
        stub(&exe, "1", 0);
        assert!(states_version(&exe, "v1.0.0"));

        // A build for another ABI does not start; one that fails is not it.
        stub(&exe, "0.2.0", 1);
        assert!(!states_version(&exe, "v0.2.0"));
        assert!(!states_version(&dir.join("absent"), "v0.2.0"));

        // And a copy that runs and says nothing useful is not it either.
        stub(&exe, "", 0);
        assert!(!states_version(&exe, "v0.2.0"));
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_tap_stops_it_only_while_stopping_leaves_nothing_half_done() {
        // The flag is read here and nowhere later.
        assert!(Doing::Asking.stoppable());
        assert!(
            Doing::Downloading {
                got: 0,
                total: None
            }
            .stoppable()
        );
        // Past the download the copy is being proved and moved.
        assert!(!Doing::Checking.stoppable());
        assert!(!Doing::Placing.stoppable());
    }

    #[test]
    fn the_new_copy_lands_and_the_record_beside_it_is_left_alone() {
        let dir = tmpdir("place");
        let (dest, staging) = (dir.join("readinglog"), dir.join("readinglog.new"));
        fs::create_dir_all(dest.join("bin")).unwrap();
        fs::create_dir_all(staging.join("bin")).unwrap();

        // What is installed, and what the app wrote beside it.
        fs::write(dest.join("bin/readinglog"), b"old binary").unwrap();
        fs::write(dest.join("config.xml"), b"<version>0.1.0</version>").unwrap();
        fs::write(dest.join("sessions.tsv"), b"the reading").unwrap();
        fs::write(dest.join("settings"), b"lang = e").unwrap();
        fs::create_dir_all(dest.join("covers")).unwrap();
        fs::write(dest.join("covers/a.jpg"), b"a jacket").unwrap();

        fs::write(staging.join("bin/readinglog"), b"new binary").unwrap();
        fs::write(staging.join("config.xml"), b"<version>0.2.0</version>").unwrap();

        place(&staging, &dest).unwrap();

        assert_eq!(
            fs::read(dest.join("bin/readinglog")).unwrap(),
            b"new binary"
        );
        assert_eq!(
            fs::read(dest.join("config.xml")).unwrap(),
            b"<version>0.2.0</version>"
        );
        // The record is not in the archive and is never touched.
        assert_eq!(fs::read(dest.join("sessions.tsv")).unwrap(), b"the reading");
        assert_eq!(fs::read(dest.join("settings")).unwrap(), b"lang = e");
        assert_eq!(fs::read(dest.join("covers/a.jpg")).unwrap(), b"a jacket");
        // Nothing is left lying beside what it replaced.
        assert!(!dest.join("bin/readinglog.old").exists());
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn the_binary_is_the_last_file_to_move() {
        let dir = tmpdir("order");
        let staging = dir.join("readinglog.new");
        fs::create_dir_all(staging.join("bin")).unwrap();
        for name in ["bin/readinglog", "bin/readinglog.sh", "config.xml"] {
            fs::write(staging.join(name), b"x").unwrap();
        }
        let mut files = Vec::new();
        walk(&staging, Path::new(""), &mut files);
        files.sort();
        files.sort_by_key(|rel| rel == Path::new(MARKER));

        assert_eq!(files.len(), 3);
        assert_eq!(files.last().unwrap(), Path::new(MARKER));
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_file_that_will_not_move_leaves_what_was_there() {
        let dir = tmpdir("replace");
        let (from, to) = (dir.join("new"), dir.join("live"));
        fs::write(&to, b"working").unwrap();
        // Nothing at `from`, so the second rename fails.
        assert!(!replace(&from, &to));
        assert_eq!(fs::read(&to).unwrap(), b"working");
        assert!(!dir.join("live.old").exists());

        // And with something to move, it lands.
        fs::write(&from, b"newer").unwrap();
        assert!(replace(&from, &to));
        assert_eq!(fs::read(&to).unwrap(), b"newer");
        // Nothing standing there is the ordinary first install.
        fs::write(&from, b"newest").unwrap();
        let fresh = dir.join("fresh");
        assert!(replace(&from, &fresh));
        assert_eq!(fs::read(&fresh).unwrap(), b"newest");
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn every_ending_that_is_not_an_update_says_where_to_get_one() {
        let endings = [
            Outcome::Offline,
            Outcome::Failed(Failure::NoAnswer),
            Outcome::Failed(Failure::NoRelease),
            Outcome::Failed(Failure::BadDownload),
            Outcome::Failed(Failure::WrongBuild),
            Outcome::Failed(Failure::NotPlaced),
        ];
        for ending in &endings {
            let (headline, note) = ending.banner(en());
            assert_eq!(headline, en().update_failed, "{ending:?}");
            assert!(
                note.iter().any(|l| l == RELEASES_URL),
                "{ending:?} does not say where to go: {note:?}"
            );
            assert!(note.len() == 3, "{ending:?}: {note:?}");
            assert!(note.iter().all(|l| !l.is_empty()), "{ending:?}");
        }
        // Each failure says something different about what went wrong.
        let mut said: Vec<&str> = endings
            .iter()
            .filter_map(|o| match o {
                Outcome::Failed(why) => Some(why.line(en())),
                _ => None,
            })
            .collect();
        said.sort_unstable();
        let count = said.len();
        said.dedup();
        assert_eq!(said.len(), count, "two failures read the same");
    }

    #[test]
    fn an_update_that_landed_says_what_to_do_next() {
        let (headline, note) = Outcome::Installed("v0.2.0".into()).banner(en());
        assert!(headline.contains("0.2.0"), "{headline}");
        assert_eq!(note, vec![en().update_reopen.to_string()]);
        // Nothing to do says the version it is staying on.
        let (headline, note) = Outcome::UpToDate.banner(en());
        assert_eq!(headline, en().update_up_to_date);
        assert!(note[0].contains(VERSION), "{note:?}");
        // And a tap that stopped it says only that.
        let (headline, note) = Outcome::Stopped.banner(en());
        assert_eq!(headline, en().update_stopped);
        assert!(note.is_empty());
    }

    #[test]
    fn every_language_has_the_words_for_every_ending() {
        for lang in Lang::ALL {
            let s = lang.strings();
            for ending in [
                Outcome::Offline,
                Outcome::UpToDate,
                Outcome::Stopped,
                Outcome::Installed("v9.9.9".into()),
                Outcome::Failed(Failure::WrongBuild),
            ] {
                let (headline, note) = ending.banner(s);
                assert!(!headline.is_empty(), "{lang:?}: {ending:?}");
                assert!(note.iter().all(|l| !l.is_empty()), "{lang:?}: {ending:?}");
            }
            // The version goes into both lines that carry one.
            assert!(crate::lang::at_version(s.update_installed, "9.9.9").contains("9.9.9"));
            assert!(crate::lang::at_version(s.update_this_version, "9.9.9").contains("9.9.9"));
        }
    }
}
