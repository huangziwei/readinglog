//! Copies of book covers under [`COVERS_DIR`], made by [`keep`] from the
//! `source` it is given and named by `file_name`.

use std::collections::HashMap;
use std::io::Read as _;
use std::path::{Path, PathBuf};

/// The directory holding the copies, under the `dir` [`path`] takes.
pub const COVERS_DIR: &str = "covers";

/// The device's own cache of book covers, which `catalog::Book::thumbnail`
/// names a file in.
pub const THUMBNAILS_DIR: &str = "/mnt/us/system/thumbnails";

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

/// Whether `path` opens a picture [`crate::ui::cover`] can draw: a JPEG, or a
/// PNG for whatever was dropped into the cache by hand.
///
/// The store's cover-art service answers a key it holds no artwork for with a
/// 60x40 GIF, written under the `.jpg` name a jacket would have had. The name
/// is the one thing about such a file that says JPEG.
pub fn drawable(path: &Path) -> bool {
    let mut head = [0u8; 8];
    std::fs::File::open(path)
        .and_then(|mut file| file.read_exact(&mut head))
        .is_ok()
        && (head.starts_with(b"\xff\xd8\xff") || head == *b"\x89PNG\r\n\x1a\n")
}

/// Copy `source` to [`path`], through a `.partial` sibling and a rename.
///
/// `Err` on a `source` of zero bytes or over `MAX_BYTES`, and one of
/// `ErrorKind::InvalidData` on a `source` [`drawable`] refuses.
pub fn keep(dir: &Path, key: &str, source: &Path) -> std::io::Result<PathBuf> {
    let bytes = std::fs::metadata(source)?.len();
    if bytes == 0 || bytes > MAX_BYTES {
        return Err(std::io::Error::other(format!(
            "{} is {bytes} bytes",
            source.display()
        )));
    }
    if !drawable(source) {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            format!("{} is neither a JPEG nor a PNG", source.display()),
        ));
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

/// Whether [`path`] holds a jacket: a file of non-zero length that
/// [`drawable`] answers for. A copy of something that will not draw counts for
/// nothing, so the next pass weighs it against the cache again.
pub fn held(dir: &Path, key: &str) -> bool {
    let at = path(dir, key);
    std::fs::metadata(&at).is_ok_and(|m| m.len() > 0) && drawable(&at)
}

/// The files in [`THUMBNAILS_DIR`] under `dir`, by the content key each names.
/// One `read_dir`, no file opened.
///
/// This reaches a book the catalog no longer states a thumbnail for: the
/// cache holds a jacket after the row naming it is gone, so a key is enough to
/// find one for a book the catalog cannot name at all.
pub fn cached(dir: &Path) -> HashMap<String, PathBuf> {
    let mut out = HashMap::new();
    let Ok(entries) = std::fs::read_dir(dir) else {
        return out;
    };
    for entry in entries.flatten() {
        let name = entry.file_name().to_string_lossy().into_owned();
        let Some(key) = keyed(&name) else {
            continue;
        };
        out.insert(key.to_string(), entry.path());
    }
    out
}

/// The content key `name` states, and `None` where it states none.
///
/// A jacket the store's cover-art service wrote is named
/// `thumbnail_<key>_<cdeType>_portrait.jpg`. One taken out of a book on the
/// device is named by `mkstemp` instead — six characters that reach neither
/// the book nor its key — and those are what this passes over.
fn keyed(name: &str) -> Option<&str> {
    let rest = name
        .strip_prefix("thumbnail_")?
        .strip_suffix("_portrait.jpg")?;
    let (key, _) = rest.rsplit_once('_')?;
    (!key.is_empty()).then_some(key)
}

/// Delete every file under [`COVERS_DIR`] that no key in `keys` names,
/// answering how many went. A `.partial` sibling goes with them.
pub fn sweep(dir: &Path, keys: &[&str]) -> usize {
    let names: std::collections::HashSet<String> = keys.iter().map(|k| file_name(k)).collect();
    let Ok(entries) = std::fs::read_dir(dir.join(COVERS_DIR)) else {
        return 0;
    };
    let mut gone = 0;
    for entry in entries.flatten() {
        if names.contains(&entry.file_name().to_string_lossy().into_owned()) {
            continue;
        }
        match std::fs::remove_file(entry.path()) {
            Ok(()) => gone += 1,
            Err(err) => eprintln!("!! covers: {} — {err}", entry.path().display()),
        }
    }
    gone
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

    /// A JPEG's opening bytes, which is all [`keep`] asks of a source.
    const JPEG: &[u8] = b"\xff\xd8\xff\xe0\x00\x10JFIF\0";

    /// The 60x40 GIF the store answers a key it holds no artwork for with,
    /// under the `.jpg` name a jacket would have had.
    const PLACEHOLDER: &[u8] = b"GIF89a\x3c\x00\x28\x00\x80\x00\x00";

    fn thumbnail(dir: &Path, name: &str, bytes: &[u8]) -> PathBuf {
        let path = dir.join(name);
        std::fs::write(&path, bytes).expect("a written thumbnail");
        path
    }

    #[test]
    fn a_cover_is_copied_and_read_back_from_the_stores_own_directory() {
        let dir = scratch("keep");
        let source = thumbnail(&dir, "thumbnail_B00OKPCRLG_EBOK_portrait.jpg", JPEG);
        assert!(!held(&dir, "B00OKPCRLG"));

        let dest = keep(&dir, "B00OKPCRLG", &source).expect("a copied cover");
        assert!(held(&dir, "B00OKPCRLG"));
        assert_eq!(std::fs::read(&dest).unwrap(), JPEG);
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
    fn the_device_cache_answers_for_a_key_and_not_for_a_name_with_none_in_it() {
        let dir = scratch("cached");
        for name in [
            "thumbnail_B00OKPCRLG_EBOK_portrait.jpg",
            "thumbnail_Entry:Item:ADC_Entry:Item:ADC_portrait.jpg",
            // Six random characters: a jacket taken out of a book on the
            // device, which nothing names.
            "thumbnail_iS79xE.jpg",
            "thumbnail__portrait.jpg",
        ] {
            thumbnail(&dir, name, JPEG);
        }
        std::fs::create_dir(dir.join("StoreSearchResults")).expect("a subdirectory");

        let found = cached(&dir);
        assert_eq!(found.len(), 2, "{found:?}");
        assert!(found["B00OKPCRLG"].ends_with("thumbnail_B00OKPCRLG_EBOK_portrait.jpg"));
        assert!(found.contains_key("Entry:Item:ADC"), "a key carrying no _");
        assert!(cached(&dir.join("nowhere")).is_empty());
        let _ = std::fs::remove_dir_all(&dir);
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
    fn the_stores_no_artwork_answer_is_not_a_jacket_whatever_it_is_named() {
        let dir = scratch("placeholder");
        let art = thumbnail(&dir, "thumbnail_B0053VMNY2_EBOK_portrait.jpg", PLACEHOLDER);
        assert!(!drawable(&art));

        let err = keep(&dir, "B0053VMNY2", &art).expect_err("a refused cover");
        assert_eq!(err.kind(), std::io::ErrorKind::InvalidData);
        assert!(!held(&dir, "B0053VMNY2"));

        // One already copied counts for nothing, so a jacket the store sends
        // later is not shadowed by it.
        std::fs::create_dir_all(dir.join(COVERS_DIR)).unwrap();
        std::fs::write(path(&dir, "B0053VMNY2"), PLACEHOLDER).unwrap();
        assert!(!held(&dir, "B0053VMNY2"));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn no_partial_survives_a_copy() {
        let dir = scratch("partial");
        let source = thumbnail(&dir, "t.jpg", JPEG);
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
