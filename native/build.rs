//! Stamps `READINGLOG_BUILD` into the binary: the instant this crate was
//! compiled and the commit it came from.
//!
//! The stamp is what tells two builds of one version apart, in a log line and
//! in `journal`'s header block — and the block is written only when it differs
//! from the one standing, so a stale stamp means a new build never states
//! itself. Hence the `rerun-if-changed` lines: naming any of them replaces
//! cargo's default of watching the whole package, so the sources are named too,
//! and the git refs beside them, because a commit moves `HEAD` without
//! touching a file this package holds.

use std::path::{Path, PathBuf};
use std::process::Command;

fn main() {
    println!("cargo:rerun-if-changed=build.rs");
    println!("cargo:rerun-if-changed=src");
    for path in watched() {
        println!("cargo:rerun-if-changed={}", path.display());
    }
    println!("cargo:rustc-env=READINGLOG_BUILD={} {}", at(), commit());
}

/// The git files that move when the commit does: `HEAD`, and whatever ref it
/// names. Empty outside a checkout, or where `.git` is not a directory — a
/// worktree keeps a file there, and the stamp falls back to being right only
/// when a source file changes.
fn watched() -> Vec<PathBuf> {
    let git = Path::new("..").join(".git");
    if !git.is_dir() {
        return Vec::new();
    }
    let head = git.join("HEAD");
    let Ok(said) = std::fs::read_to_string(&head) else {
        return Vec::new();
    };
    let mut out = vec![head];
    // `ref: refs/heads/main` — the file that file names moves on a commit.
    // A detached HEAD names no ref and moves on its own.
    if let Some(at) = said.trim().strip_prefix("ref: ") {
        out.push(git.join(at));
        // A ref that has been packed away has no file of its own.
        out.push(git.join("packed-refs"));
    }
    out
}

/// The compile instant as `YYYY-MM-DDTHH:MM:SSZ`, or `?` where `date` is not
/// there to state one.
fn at() -> String {
    run("date", &["-u", "+%Y-%m-%dT%H:%M:%SZ"]).unwrap_or_else(|| "?".into())
}

/// The short commit, with `+` where the tree carries changes over it. `?`
/// outside a checkout.
fn commit() -> String {
    let Some(short) = run("git", &["rev-parse", "--short", "HEAD"]) else {
        return "?".into();
    };
    match run("git", &["status", "--porcelain"]) {
        Some(changes) if !changes.is_empty() => format!("{short}+"),
        _ => short,
    }
}

/// `program`'s trimmed stdout, or `None` where it did not run or failed.
fn run(program: &str, args: &[&str]) -> Option<String> {
    let out = Command::new(program).args(args).output().ok()?;
    out.status
        .success()
        .then(|| String::from_utf8_lossy(&out.stdout).trim().to_string())
}
