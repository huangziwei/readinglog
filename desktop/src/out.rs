//! A built site written into its folder, which may be a clone of a repository.
//! The folder's manifest names each file the last write put there and what it
//! made public; a write touches those files and its own, and nothing else.

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Component, Path, PathBuf};

use anyhow::{Context, Result, bail};

use crate::workspace::write_file;

/// The manifest's name in the folder.
pub const MANIFEST: &str = ".readinglog-site";

/// The manifest's first line.
const HEAD: &str =
    "# Written by Reading Log: each file it wrote here, then each thing made public.";

/// What opens a manifest line naming a thing made public, not a file.
const ITEM: &str = "@ ";

/// What a manifest holds.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct Manifest {
    /// The files the last write put in the folder.
    pub files: BTreeSet<String>,
    /// What that write made public, a line each.
    pub items: BTreeSet<String>,
}

/// What one write did.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct Written {
    /// Files written whose bytes moved, or that were not there.
    pub changed: Vec<String>,
    /// Files the last write put here and this one did not, taken away.
    pub removed: Vec<String>,
    /// Files holding the bytes this write puts there.
    pub unchanged: usize,
}

/// Write `files` into `dir`, each path `/`-separated under it, and `items` into
/// its manifest. A folder with no manifest is taken only where it holds no more
/// than a repository's own files; a file no earlier write wrote is never replaced.
pub fn write(
    dir: &Path,
    files: &BTreeMap<String, Vec<u8>>,
    items: &BTreeSet<String>,
) -> Result<Written> {
    for path in files.keys() {
        if !owned(path) {
            bail!("{path} is not a path a site writes");
        }
    }
    if let Some(item) = items.iter().find(|i| i.contains('\n')) {
        bail!("{item:?} is not one line");
    }
    let Manifest {
        files: held,
        items: shown,
    } = match manifest(dir)? {
        Some(held) => held,
        None => {
            claim(dir)?;
            Manifest::default()
        }
    };
    for (path, bytes) in files {
        let at = under(dir, path);
        if held.contains(path) || !at.exists() {
            continue;
        }
        if std::fs::read(&at).ok().as_ref() != Some(bytes) {
            bail!(
                "{} stands where the site writes a file, and no earlier build wrote it",
                at.display()
            );
        }
    }
    // Every file this write may leave behind, named before any is written: a
    // write cut short leaves no file the next one does not own.
    let mut both = held.clone();
    both.extend(files.keys().cloned());
    write_manifest(dir, &both, &shown)?;

    let mut out = Written::default();
    for (path, bytes) in files {
        let at = under(dir, path);
        if std::fs::read(&at).ok().as_ref() == Some(bytes) {
            out.unchanged += 1;
            continue;
        }
        write_file(&at, bytes)?;
        out.changed.push(path.clone());
    }
    for path in held.iter().filter(|p| !files.contains_key(*p)) {
        let at = under(dir, path);
        match std::fs::remove_file(&at) {
            Ok(()) => out.removed.push(path.clone()),
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => {}
            Err(err) => return Err(err).with_context(|| format!("taking {} away", at.display())),
        }
        prune(dir, &at);
    }
    write_manifest(dir, &files.keys().cloned().collect(), items)?;
    Ok(out)
}

/// The manifest in `dir`, `None` where there is none. A line naming a path no
/// site writes is passed over.
pub fn manifest(dir: &Path) -> Result<Option<Manifest>> {
    let file = dir.join(MANIFEST);
    let text = match std::fs::read_to_string(&file) {
        Ok(text) => text,
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(err) => return Err(err).with_context(|| format!("reading {}", file.display())),
    };
    let mut out = Manifest::default();
    for line in text.lines() {
        if let Some(item) = line.strip_prefix(ITEM) {
            out.items.insert(item.to_string());
            continue;
        }
        let line = line.trim();
        if !line.is_empty() && !line.starts_with('#') && owned(line) {
            out.files.insert(line.to_string());
        }
    }
    Ok(Some(out))
}

fn write_manifest(dir: &Path, paths: &BTreeSet<String>, items: &BTreeSet<String>) -> Result<()> {
    let mut text = format!("{HEAD}\n");
    for path in paths {
        text.push_str(path);
        text.push('\n');
    }
    for item in items {
        text.push_str(ITEM);
        text.push_str(item);
        text.push('\n');
    }
    write_file(&dir.join(MANIFEST), text.as_bytes())
}

/// Take `dir`, which carries no manifest: it may be missing, empty, or hold
/// no more than a repository's own files.
fn claim(dir: &Path) -> Result<()> {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return Ok(());
    };
    let mut foreign: Vec<String> = entries
        .flatten()
        .map(|e| e.file_name().to_string_lossy().into_owned())
        .filter(|name| !a_repositorys_own(name))
        .collect();
    if foreign.is_empty() {
        return Ok(());
    }
    foreign.sort();
    foreign.truncate(3);
    bail!(
        "{} holds {} and no {MANIFEST}, so no build wrote it. Pick an empty folder, \
         or a clone of the site's repository.",
        dir.display(),
        foreign.join(", ")
    )
}

/// Whether a file named `name` at the top of a folder is the repository's own
/// and not a page: anything hidden, `CNAME`, a README or a licence.
fn a_repositorys_own(name: &str) -> bool {
    let upper = name.to_ascii_uppercase();
    name.starts_with('.')
        || name == "CNAME"
        || ["README", "LICENSE", "LICENCE", "COPYING"]
            .iter()
            .any(|own| upper.starts_with(own))
}

/// Whether `path` is one a site may write and take away: relative, every
/// part a plain name, and outside `.git`, `CNAME` and the manifest itself.
fn owned(path: &str) -> bool {
    let parts: Vec<Component<'_>> = Path::new(path).components().collect();
    !path.is_empty()
        && !path.contains('\\')
        && path != MANIFEST
        && path != "CNAME"
        && !path.starts_with('@')
        && parts.iter().all(|c| matches!(c, Component::Normal(_)))
        && !matches!(parts.first(), Some(Component::Normal(first)) if first.to_string_lossy().starts_with(".git"))
}

fn under(dir: &Path, path: &str) -> PathBuf {
    path.split('/')
        .fold(dir.to_path_buf(), |at, part| at.join(part))
}

/// Take away every folder between `at` and `dir` left empty.
fn prune(dir: &Path, at: &Path) {
    let mut up = at.parent();
    while let Some(folder) = up {
        if folder == dir || !folder.starts_with(dir) || std::fs::remove_dir(folder).is_err() {
            return;
        }
        up = folder.parent();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tests::scratch;

    fn files(pairs: &[(&str, &str)]) -> BTreeMap<String, Vec<u8>> {
        pairs
            .iter()
            .map(|(p, b)| (p.to_string(), b.as_bytes().to_vec()))
            .collect()
    }

    /// [`super::write`] making nothing public.
    fn write(dir: &Path, files: &BTreeMap<String, Vec<u8>>) -> Result<Written> {
        super::write(dir, files, &BTreeSet::new())
    }

    #[test]
    fn a_manifest_names_what_its_write_made_public() {
        let dir = scratch("out-items");
        let items: BTreeSet<String> = [
            "book B1",
            "highlight 2026-09-01T09:05:00\thighlight\tA\t120",
        ]
        .iter()
        .map(|i| i.to_string())
        .collect();
        super::write(&dir, &files(&[("index.html", "a")]), &items).unwrap();
        let held = manifest(&dir).unwrap().unwrap();
        assert_eq!(held.items, items);
        assert_eq!(held.files, ["index.html".to_string()].into());
        let two: BTreeSet<String> = ["one\ntwo".to_string()].into();
        assert!(super::write(&dir, &files(&[("index.html", "a")]), &two).is_err());
    }

    #[test]
    fn a_write_replaces_its_own_files_and_takes_away_only_those() {
        let dir = scratch("out");
        let first = write(
            &dir,
            &files(&[("index.html", "a"), ("books/one/index.html", "b")]),
        )
        .unwrap();
        assert_eq!(first.changed.len(), 2);
        // A file no build wrote, beside the site, and the repository's `CNAME`.
        std::fs::write(dir.join("notes.html"), "mine").unwrap();
        std::fs::write(dir.join("CNAME"), "reading.example").unwrap();

        let second = write(
            &dir,
            &files(&[("index.html", "a2"), ("books/two/index.html", "c")]),
        )
        .unwrap();
        assert_eq!(second.changed, ["books/two/index.html", "index.html"]);
        assert_eq!(second.removed, ["books/one/index.html"]);
        assert!(
            !dir.join("books/one").exists(),
            "an emptied folder goes with its file"
        );
        assert_eq!(
            std::fs::read_to_string(dir.join("notes.html")).unwrap(),
            "mine"
        );
        assert_eq!(
            std::fs::read_to_string(dir.join("CNAME")).unwrap(),
            "reading.example"
        );

        let third = write(
            &dir,
            &files(&[("index.html", "a2"), ("books/two/index.html", "c")]),
        )
        .unwrap();
        assert!(third.changed.is_empty() && third.removed.is_empty());
        assert_eq!(third.unchanged, 2);
    }

    #[test]
    fn a_folder_no_build_wrote_is_refused_unless_it_is_a_bare_repository() {
        let mine = scratch("out-mine");
        std::fs::write(mine.join("thesis.pdf"), "years of work").unwrap();
        let refused = write(&mine, &files(&[("index.html", "a")]));
        assert!(refused.is_err());
        assert!(!mine.join("index.html").exists());

        let clone = scratch("out-clone");
        std::fs::create_dir_all(clone.join(".git")).unwrap();
        std::fs::write(clone.join("README.md"), "# reading").unwrap();
        std::fs::write(clone.join("CNAME"), "reading.example").unwrap();
        assert!(write(&clone, &files(&[("index.html", "a")])).is_ok());

        let missing = scratch("out-missing").join("site");
        assert!(write(&missing, &files(&[("index.html", "a")])).is_ok());
    }

    #[test]
    fn a_file_no_build_wrote_is_never_replaced() {
        let dir = scratch("out-collide");
        write(&dir, &files(&[("index.html", "a")])).unwrap();
        std::fs::create_dir_all(dir.join("books")).unwrap();
        std::fs::write(dir.join("books/index.html"), "mine").unwrap();
        assert!(
            write(
                &dir,
                &files(&[("index.html", "a"), ("books/index.html", "b")])
            )
            .is_err()
        );
        assert_eq!(
            std::fs::read_to_string(dir.join("books/index.html")).unwrap(),
            "mine"
        );
    }

    #[test]
    fn a_manifest_reaches_nothing_outside_the_sites_own_paths() {
        let dir = scratch("out-reach");
        std::fs::create_dir_all(dir.join(".git")).unwrap();
        std::fs::write(dir.join(".git/config"), "kept").unwrap();
        std::fs::write(dir.join("CNAME"), "kept").unwrap();
        let outside = scratch("out-outside").join("kept.txt");
        std::fs::write(&outside, "kept").unwrap();
        let lines = format!(
            "{HEAD}\n.git/config\nCNAME\n../{}\n/etc/hosts\n",
            outside.display()
        );
        std::fs::write(dir.join(MANIFEST), lines).unwrap();
        let written = write(&dir, &files(&[("index.html", "a")])).unwrap();
        assert!(written.removed.is_empty());
        assert!(dir.join(".git/config").exists() && dir.join("CNAME").exists());
        assert!(outside.exists());
        assert!(write(&dir, &files(&[("../escape.html", "a")])).is_err());
        assert!(write(&dir, &files(&[(".git/hooks/x", "a")])).is_err());
    }
}
