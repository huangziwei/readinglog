//! Handing a book back to the Kindle's own reader.
//!
//! [`ask`] writes a `file://` URI built from the catalog's `p_location` into
//! [`REQUEST`], and `bin/readinglog.sh` files it with `com.lab126.appmgrd`
//! once this process has left the screen.

use std::io;
use std::path::Path;

/// The file a launch is asked for through, under [`crate::store::STORE_DIR`]:
/// the URI, then [`At::GOTO`] for [`At::Beginning`], then `location`.
pub const REQUEST: &str = "open";

/// Which end of the book [`ask`] asks for.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum At {
    /// The place [`uri`] alone lands on.
    #[default]
    Left,
    /// The book's own start, which `bin/readinglog.sh` opens at.
    Beginning,
}

impl At {
    /// The second line [`At::Beginning`] writes.
    pub const GOTO: &'static str = "goto";

    /// The second line of [`REQUEST`] for this end.
    fn line(self) -> &'static str {
        match self {
            At::Left => "",
            At::Beginning => At::GOTO,
        }
    }
}

/// The ASCII characters escaped inside a path. Every other printable
/// character, and every byte above ASCII, travels as itself.
const ESCAPED: &str = " \"#%<>?[\\]^`{|}";

/// `path` as a `file://` URI.
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

/// Ask for `location` to be opened `at`, once this process exits, by writing
/// [`REQUEST`] under `dir`.
///
/// Through a `.partial` sibling and a rename.
pub fn ask(dir: &Path, location: &str, at: At) -> io::Result<()> {
    let target = dir.join(REQUEST);
    let partial = target.with_extension("partial");
    let said = format!("{}\n{}\n{}\n", uri(location), at.line(), location);
    std::fs::write(&partial, said)?;
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
        // These fifteen ASCII characters, and no others.
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
        ask(&dir, "/mnt/us/documents/first.kfx", At::Left).expect("a written request");
        ask(&dir, "/mnt/us/documents/second one.kfx", At::Left).expect("a written request");
        let said = std::fs::read_to_string(dir.join(REQUEST)).expect("the request");
        assert_eq!(
            said,
            "file:///mnt/us/documents/second%20one.kfx\n\n/mnt/us/documents/second one.kfx\n"
        );
        // The sibling is renamed over the target, never left beside it.
        assert!(!dir.join("open.partial").exists());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_beginning_names_itself_on_the_second_line() {
        let dir = std::env::temp_dir().join("readinglog-open-goto");
        std::fs::create_dir_all(&dir).expect("a directory to ask in");
        ask(&dir, "/mnt/us/documents/a.kfx", At::Beginning).expect("a written request");
        let said = std::fs::read_to_string(dir.join(REQUEST)).expect("the request");
        assert_eq!(
            said,
            "file:///mnt/us/documents/a.kfx\ngoto\n/mnt/us/documents/a.kfx\n"
        );
        // The URI keeps the first line whichever end is asked for.
        assert_eq!(said.lines().next(), Some("file:///mnt/us/documents/a.kfx"));
        let _ = std::fs::remove_dir_all(&dir);
    }
}
