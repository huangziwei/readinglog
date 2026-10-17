use std::hash::{Hash, Hasher};
use std::path::Path;

fn main() {
    // Every file under `web` and `icons`: a `rerun-if-changed`, its path and
    // bytes hashed into `READINGLOG_EMBEDDED_FINGERPRINT`.
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    fingerprint_dir(Path::new("web"), &mut hasher);
    fingerprint_dir(Path::new("icons"), &mut hasher);
    println!(
        "cargo:rustc-env=READINGLOG_EMBEDDED_FINGERPRINT={:x}",
        hasher.finish()
    );

    // `CodegenContext` writes the context `tauri_build_context!` includes, and its
    // copies of `web` and `icons`, into `OUT_DIR` ahead of the crate's compile.
    let attributes = tauri_build::Attributes::new().codegen(tauri_build::CodegenContext::new());
    if let Err(error) = tauri_build::try_build(attributes) {
        println!("{error:#}");
        std::process::exit(1);
    }
}

fn fingerprint_dir(dir: &Path, hasher: &mut impl Hasher) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    let mut paths: Vec<_> = entries.flatten().map(|e| e.path()).collect();
    // `paths` in name order, the order `hasher` takes them in.
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
