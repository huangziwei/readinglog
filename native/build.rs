//! `READINGLOG_BUILD` carries the compile instant and the commit into the
//! binary. The `cargo:rerun-if-changed` lines name `build.rs`, `src`, and
//! every path `watched` returns.

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

/// The `.git` files that move on a commit and that exist: `HEAD`, and the
/// loose or packed file holding the ref `HEAD` names. Empty where `..` holds
/// no `.git`.
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
    // `ref: refs/heads/main` — `at` is the ref, held by a loose file under
    // `.git/` or by a line in `packed-refs`.
    if let Some(at) = said.trim().strip_prefix("ref: ") {
        out.push(git.join(at));
        out.push(git.join("packed-refs"));
    }
    out.retain(|path| path.exists());
    out
}

/// The compile instant as `YYYY-MM-DDTHH:MM:SSZ`, or `?` where `date` fails.
fn at() -> String {
    run("date", &["-u", "+%Y-%m-%dT%H:%M:%SZ"]).unwrap_or_else(|| "?".into())
}

/// `git rev-parse --short HEAD`, with `+` where `git status --porcelain`
/// states any line. `?` where `git rev-parse` fails.
fn commit() -> String {
    let Some(short) = run("git", &["rev-parse", "--short", "HEAD"]) else {
        return "?".into();
    };
    match run("git", &["status", "--porcelain"]) {
        Some(changes) if !changes.is_empty() => format!("{short}+"),
        _ => short,
    }
}

/// `program`'s trimmed stdout, or `None` where `Command` or `status` fails.
fn run(program: &str, args: &[&str]) -> Option<String> {
    let out = Command::new(program).args(args).output().ok()?;
    out.status
        .success()
        .then(|| String::from_utf8_lossy(&out.stdout).trim().to_string())
}
