//! `update` against the release list GitHub actually serves. Skipped unless
//! `READINGLOG_NETWORK=1`: the only check that the release JSON still carries
//! the fields `ApiRelease` names, and that the asset is still called that.

use std::sync::atomic::AtomicBool;

use readinglog_native::update::{self, archive, http};

/// Whether to talk to GitHub at all.
fn allowed() -> bool {
    match std::env::var("READINGLOG_NETWORK").is_ok() {
        true => true,
        false => {
            eprintln!("skipped: set READINGLOG_NETWORK=1 to talk to GitHub");
            false
        }
    }
}

fn tmpdir(name: &str) -> std::path::PathBuf {
    let dir = std::env::temp_dir().join(format!("readinglog-real-{name}"));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

#[test]
fn the_release_list_still_names_the_archive_this_looks_for() {
    if !allowed() {
        return;
    }
    let release = update::available(&http::Client::new()).expect("a release carrying an archive");
    assert!(release.tag.starts_with('v'), "{}", release.tag);
    assert!(
        release.name.starts_with("readinglog-") && release.name.ends_with("-kindle.zip"),
        "{}",
        release.name
    );
    assert!(release.url.starts_with("https://"), "{}", release.url);
    // The workflow publishes a checksum beside it.
    assert!(
        release
            .sha
            .as_deref()
            .is_some_and(|s| s.ends_with(".sha256")),
        "{:?}",
        release.sha
    );
}

#[test]
fn the_published_archive_holds_the_tree_an_update_moves_into_place() {
    if !allowed() {
        return;
    }
    let client = http::Client::new();
    let release = update::available(&client).expect("a release carrying an archive");
    let dir = tmpdir("archive");
    let zip = dir.join(&release.name);
    client
        .download(&release.url, &zip, &AtomicBool::new(false), &|_, _| {})
        .expect("the archive downloads");

    // The digest the release publishes for it.
    let sidecar = release.sha.expect("a checksum");
    let text = client.text(&sidecar, "text/plain").expect("the checksum");
    assert!(
        update::digest_from(&text, &release.name).is_some(),
        "no digest for {} in {text:?}",
        release.name
    );

    // Everything under `extensions/readinglog/`, and nothing outside it.
    let dest = dir.join("readinglog.new");
    let written = archive::unpack(&zip, update::MARKER, &dest).expect("a readable archive");
    assert!(written >= 3, "{written} files");
    assert!(dest.join(update::MARKER).is_file(), "no binary inside");
    assert!(dest.join("config.xml").is_file());
    // The library tile sits outside the marker's own root and is left alone.
    assert!(!dest.join("documents").exists());
    let _ = std::fs::remove_dir_all(&dir);
}
