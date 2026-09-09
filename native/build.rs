//! Stamps `READINGLOG_BUILD` into the binary: the instant this crate was
//! compiled and the commit it came from.

use std::process::Command;

fn main() {
    println!("cargo:rustc-env=READINGLOG_BUILD={} {}", at(), commit());
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
