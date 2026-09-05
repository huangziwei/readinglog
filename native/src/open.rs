//! Handing a book back to the Kindle's own reader.
//!
//! The framework opens a downloaded book by writing a `file://` URI built from
//! the catalog's `p_location` to `com.lab126.appmgrd`'s `start` property;
//! `appmgrd` resolves the extension to a mimetype and the mimetype to the
//! reader booklet, which reads the path back off the URI. This module writes
//! that URI into a request file, and `bin/readinglog.sh` performs the call once
//! this process has left the screen.

use std::io;
use std::path::Path;

/// The file a launch is asked for through, under [`crate::store::STORE_DIR`].
pub const REQUEST: &str = "open";

/// The ASCII characters the framework escapes inside a path. Everything else
/// printable is left alone, and so is every byte above ASCII: a location
/// holding Han or kana travels as itself.
const ESCAPED: &str = " \"#%<>?[\\]^`{|}";

/// `path` as the `file://` URI the reader is started with.
///
/// `path` is absolute, as `p_location` always is.
pub fn uri(path: &str) -> String {
    let mut out = String::from("file://");
    for c in path.chars() {
        match c.is_ascii() && (c < ' ' || c == '\u{7f}' || ESCAPED.contains(c)) {
            true => out.push_str(&format!("%{:02X}", c as u8)),
            false => out.push(c),
        }
    }
    out
}

/// Ask for `location` to be opened once this process exits, by writing
/// [`REQUEST`] under `dir`.
///
/// Through a `.partial` sibling and a rename: the user partition is FAT, and a
/// truncated request would name no book.
pub fn ask(dir: &Path, location: &str) -> io::Result<()> {
    let target = dir.join(REQUEST);
    let partial = target.with_extension("partial");
    std::fs::write(&partial, format!("{}\n", uri(location)))?;
    std::fs::rename(&partial, &target)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_plain_path_is_its_own_uri() {
        assert_eq!(
            uri("/mnt/us/documents/bible.kfx"),
            "file:///mnt/us/documents/bible.kfx"
        );
    }

    #[test]
    fn the_characters_the_framework_escapes_are_the_ones_escaped() {
        assert_eq!(
            uri("/mnt/us/documents/My Clippings.txt"),
            "file:///mnt/us/documents/My%20Clippings.txt"
        );
        // Measured against the five-argument `java.net.URI` constructor the
        // firmware builds this with: these fifteen and no others.
        assert_eq!(
            uri("/x !\"#$%&'()*+,-./09:;<=>?@AZ[\\]^_`az{|}~"),
            "file:///x%20!%22%23$%25&'()*+,-./09:;%3C=%3E%3F@AZ%5B%5C%5D%5E_%60az%7B%7C%7D~"
        );
    }

    #[test]
    fn a_title_outside_ascii_travels_unescaped() {
        assert_eq!(
            uri("/mnt/us/documents/Sidle/[村上 春樹] 世界.kfx"),
            "file:///mnt/us/documents/Sidle/%5B村上%20春樹%5D%20世界.kfx"
        );
    }

    #[test]
    fn a_request_is_the_uri_and_replaces_the_one_before_it() {
        let dir = std::env::temp_dir().join("readinglog-open-request");
        std::fs::create_dir_all(&dir).expect("a directory to ask in");
        ask(&dir, "/mnt/us/documents/first.kfx").expect("a written request");
        ask(&dir, "/mnt/us/documents/second one.kfx").expect("a written request");
        let said = std::fs::read_to_string(dir.join(REQUEST)).expect("the request");
        assert_eq!(said, "file:///mnt/us/documents/second%20one.kfx\n");
        // The sibling is renamed over the target, never left beside it.
        assert!(!dir.join("open.partial").exists());
        let _ = std::fs::remove_dir_all(&dir);
    }
}
