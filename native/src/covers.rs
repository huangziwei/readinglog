//! Book covers under [`COVERS_DIR`], copied from the paths `cc.db` states in
//! `p_thumbnail`.

use std::path::{Path, PathBuf};

/// The directory holding the copies, under the store's own.
pub const COVERS_DIR: &str = "covers";

/// The largest file [`keep`] copies, in bytes.
const MAX_BYTES: u64 = 2 * 1024 * 1024;

/// `key` with every character outside `[A-Za-z0-9]` replaced, plus `.jpg`.
fn file_name(key: &str) -> String {
    let stem: String = key
        .chars()
        .map(|c| match c.is_ascii_alphanumeric() {
            true => c,
            false => '_',
        })
        .collect();
    format!("{stem}.jpg")
}

/// The copy's path under `dir`, whether or not it exists.
pub fn path(dir: &Path, key: &str) -> PathBuf {
    dir.join(COVERS_DIR).join(file_name(key))
}

/// Copy `source` to [`path`], through a `.partial` sibling and a rename.
///
/// `Err` on a `source` of zero bytes or over [`MAX_BYTES`].
pub fn keep(dir: &Path, key: &str, source: &Path) -> std::io::Result<PathBuf> {
    let bytes = std::fs::metadata(source)?.len();
    if bytes == 0 || bytes > MAX_BYTES {
        return Err(std::io::Error::other(format!(
            "{} is {bytes} bytes",
            source.display()
        )));
    }
    let dest = path(dir, key);
    if let Some(parent) = dest.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let partial = dest.with_extension("partial");
    std::fs::copy(source, &partial)?;
    std::fs::rename(&partial, &dest)?;
    Ok(dest)
}

/// Whether [`path`] exists with a non-zero length.
pub fn held(dir: &Path, key: &str) -> bool {
    std::fs::metadata(path(dir, key)).is_ok_and(|m| m.len() > 0)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn scratch(tag: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("readinglog-covers-{tag}"));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("a scratch directory");
        dir
    }

    fn thumbnail(dir: &Path, name: &str, bytes: &[u8]) -> PathBuf {
        let path = dir.join(name);
        std::fs::write(&path, bytes).expect("a written thumbnail");
        path
    }

    #[test]
    fn a_cover_is_copied_and_read_back_from_the_stores_own_directory() {
        let dir = scratch("keep");
        let source = thumbnail(&dir, "thumbnail_B00OKPCRLG_EBOK_portrait.jpg", b"jpegbytes");
        assert!(!held(&dir, "B00OKPCRLG"));

        let dest = keep(&dir, "B00OKPCRLG", &source).expect("a copied cover");
        assert!(held(&dir, "B00OKPCRLG"));
        assert_eq!(std::fs::read(&dest).unwrap(), b"jpegbytes");
        assert!(dest.starts_with(dir.join(COVERS_DIR)));

        std::fs::remove_file(&source).unwrap();
        assert!(held(&dir, "B00OKPCRLG"));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_key_naming_no_book_makes_a_legal_file_name() {
        // `*` is not a legal FAT filename character.
        assert_eq!(file_name("*aa11bb22"), "_aa11bb22.jpg");
        assert_eq!(file_name("B00OKPCRLG"), "B00OKPCRLG.jpg");
        assert_eq!(file_name("CR!ABC 123"), "CR_ABC_123.jpg");
    }

    #[test]
    fn an_empty_or_missing_source_is_not_kept() {
        let dir = scratch("empty");
        let empty = thumbnail(&dir, "empty.jpg", b"");
        assert!(keep(&dir, "B01", &empty).is_err());
        assert!(!held(&dir, "B01"));
        assert!(keep(&dir, "B02", &dir.join("nothing.jpg")).is_err());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn no_partial_survives_a_copy() {
        let dir = scratch("partial");
        let source = thumbnail(&dir, "t.jpg", b"x");
        keep(&dir, "B01", &source).expect("a copied cover");
        let left: Vec<_> = std::fs::read_dir(dir.join(COVERS_DIR))
            .expect("the covers directory")
            .flatten()
            .filter(|e| e.file_name().to_string_lossy().contains("partial"))
            .collect();
        assert!(left.is_empty());
        let _ = std::fs::remove_dir_all(&dir);
    }
}
