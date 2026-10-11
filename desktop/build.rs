use std::hash::{Hash, Hasher};
use std::path::Path;

fn main() {
    // `generate_context!` embeds `web/`, which Cargo does not watch: the
    // tree's fingerprint goes into a rustc env var.
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    fingerprint_dir(Path::new("web"), &mut hasher);
    println!(
        "cargo:rustc-env=READINGLOG_WEB_FINGERPRINT={:x}",
        hasher.finish()
    );

    tauri_build::build()
}

fn fingerprint_dir(dir: &Path, hasher: &mut impl Hasher) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    let mut paths: Vec<_> = entries.flatten().map(|e| e.path()).collect();
    // One tree, one fingerprint: the paths in a fixed order.
    paths.sort();
    for path in paths {
        println!("cargo:rerun-if-changed={}", path.display());
        if path.is_dir() {
            fingerprint_dir(&path, hasher);
        } else if let Ok(bytes) = std::fs::read(&path) {
            path.to_string_lossy().hash(hasher);
            bytes.hash(hasher);
        }
    }
}
