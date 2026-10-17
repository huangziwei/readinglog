//! `READINGLOG_BUILD` carries the compile instant and the last commit to touch
//! this package into the binary. The `cargo:rerun-if-changed` lines name
//! `build.rs` and every path `watched` returns.

use std::path::PathBuf;
use std::process::Command;

fn main() {
    println!("cargo:rerun-if-changed=build.rs");
    for path in watched() {
        println!("cargo:rerun-if-changed={}", path.display());
    }
    println!("cargo:rustc-env=READINGLOG_BUILD={} {}", at(), commit());
}

/// Every entry of `src` other than `src/bin`: the files the lib and
/// `src/main.rs` compile from.
fn watched() -> Vec<PathBuf> {
    let Ok(entries) = std::fs::read_dir("src") else {
        return Vec::new();
    };
    let mut out: Vec<PathBuf> = entries
        .flatten()
        .map(|entry| entry.path())
        .filter(|path| !path.ends_with("bin"))
        .collect();
    out.sort();
    out
}

/// The compile instant as `YYYY-MM-DDTHH:MM:SSZ`, or `?` where `date` fails.
fn at() -> String {
    run("date", &["-u", "+%Y-%m-%dT%H:%M:%SZ"]).unwrap_or_else(|| "?".into())
}

/// `git log -1 --format=%h -- .`, the last commit to touch this package, with
/// `+` where `git status --porcelain -- .` states any line. `?` where `git log`
/// names no commit.
fn commit() -> String {
    let short = run("git", &["log", "-1", "--format=%h", "--", "."]).unwrap_or_default();
    if short.is_empty() {
        return "?".into();
    }
    match run("git", &["status", "--porcelain", "--", "."]) {
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
