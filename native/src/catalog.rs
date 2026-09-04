//! The device's content catalog, `cc.db`, read through the `sqlite3` binary the
//! firmware ships. `p_contentSize` equals a reading line's
//! `BookEndPosition.FromBook`, and `p_cdeKey` a reader-shell record's key.

use std::path::{Path, PathBuf};
use std::process::Command;

/// The catalog, newest firmware first. `/var/local` is a symlink to
/// `/var/base-local`, and one device answers to more than one of these.
const CATALOG_PATHS: [&str; 3] = [
    "/var/base-local/metadata/cc.db",
    "/var/local/metadata/cc.db",
    "/var/local/cc.db",
];

/// Column separator and row separator for the `sqlite3` output. Neither can
/// occur in a title, an author or a path.
const COL: &str = "\u{1}";
const ROW: &str = "\u{2}";

/// `p_location` is empty on a cloud row, which also states no `p_contentSize`
/// and no `p_percentFinished`. `p_contentState` is 1 on a store book, 0 on a
/// sideload. A `*`-prefixed `p_cdeKey` is kept — see [`Book::is_book`].
const QUERY: &str = "select coalesce(p_contentSize, 0), p_cdeKey, p_cdeType, \
     coalesce(p_titles_0_nominal, ''), coalesce(p_credits_0_name_collation, ''), \
     coalesce(p_percentFinished, -1), coalesce(p_thumbnail, ''), \
     coalesce(p_lastAccess, 0), coalesce(p_languages_0, ''), \
     replace(coalesce(j_credits, ''), char(10), ' '), \
     (p_location is not null and p_location <> '') \
     from Entries \
     where p_cdeKey is not null and p_cdeKey <> '' \
       and p_cdeType in ('EBOK', 'PDOC', 'MAGZ')";

/// One book the catalog names, on the device or in the library.
#[derive(Debug, Clone, PartialEq)]
pub struct Book {
    /// `p_contentSize`, the number a sitting is keyed by. Zero on a cloud row,
    /// which states none.
    pub extent: i64,
    /// `p_cdeKey`, the key the reader-shell records name this book by.
    pub cde_key: String,
    /// `EBOK` for a store book or a sideload, `PDOC` for a personal document.
    pub cde_type: String,
    pub title: String,
    pub author: String,
    /// `p_percentFinished`, 0 through 100. Negative where the catalog states
    /// none.
    pub percent: f64,
    /// `p_thumbnail`, a path under `/mnt/us/system/thumbnails/`. Empty where
    /// the catalog names none.
    pub thumbnail: String,
    /// `p_lastAccess`, epoch seconds.
    pub last_access: i64,
    /// `p_languages_0`, a BCP-47 tag. Empty where the catalog states none.
    /// `font::Script::of_language` reads it.
    pub language: String,
    /// False on a `*`-prefixed `cde_key`: a scriptlet, `My Clippings.txt`, a
    /// hotfix runner. Each carries reading time and `Stats::build` drops it.
    pub is_book: bool,
    /// Whether `p_location` names a file.
    pub on_device: bool,
}

/// Every book the catalog names. Empty when no `CATALOG_PATHS` entry exists.
pub fn read() -> Vec<Book> {
    match CATALOG_PATHS.iter().map(Path::new).find(|p| p.exists()) {
        Some(db) => read_from(db),
        None => Vec::new(),
    }
}

/// [`read`] against a named file. Opened plain: on a WAL database `mode=ro`
/// and `-readonly` fail for want of a `-shm` file, and `immutable=1` reads
/// pre-WAL state.
pub fn read_from(db: &Path) -> Vec<Book> {
    let out = match Command::new("sqlite3")
        .arg("-separator")
        .arg(COL)
        .arg("-newline")
        .arg(ROW)
        .arg(db)
        .arg(QUERY)
        .output()
    {
        Ok(out) => out,
        Err(err) => {
            eprintln!("catalog: sqlite3 would not run: {err}");
            return Vec::new();
        }
    };
    // `sqlite3` writes a refused query to stderr.
    let complaint = String::from_utf8_lossy(&out.stderr);
    if !complaint.trim().is_empty() {
        eprintln!("catalog: sqlite3 {}: {}", db.display(), complaint.trim());
    }
    String::from_utf8_lossy(&out.stdout)
        .split(ROW)
        .filter_map(parse_row)
        .collect()
}

fn parse_row(row: &str) -> Option<Book> {
    let mut f = row.split(COL);
    let mut next = || f.next().unwrap_or_default();
    let extent: i64 = next().trim().parse().ok()?;
    let cde_key = next().to_string();
    if cde_key.is_empty() {
        return None;
    }
    let is_book = !cde_key.starts_with('*');
    let cde_type = next().to_string();
    let title = next().to_string();
    let collation = next().to_string();
    let percent = next().trim().parse().unwrap_or(-1.0);
    let thumbnail = next().to_string();
    let last_access = next().trim().parse().unwrap_or(0);
    let language = next().to_string();
    let author = author_name(next(), &collation);
    let on_device = next().trim() == "1";
    Some(Book {
        extent,
        cde_key,
        cde_type,
        title,
        author,
        percent,
        thumbnail,
        last_access,
        language,
        is_book,
        on_device,
    })
}

/// The `display` name in `j_credits`, else `collation` unpadded.
fn author_name(j_credits: &str, collation: &str) -> String {
    if let Some(display) = json_string(j_credits, "display")
        && !display.is_empty()
    {
        return display;
    }
    unpadded(collation).to_string()
}

/// The value of the first `"<key>":"…"` in `json`, with `\"` and `\\` undone.
fn json_string(json: &str, key: &str) -> Option<String> {
    let rest = json.split_once(&format!("\"{key}\":\""))?.1;
    let mut out = String::new();
    let mut chars = rest.chars();
    while let Some(c) = chars.next() {
        match c {
            '"' => return Some(out),
            '\\' => out.push(chars.next()?),
            c => out.push(c),
        }
    }
    None
}

/// `collation` without a leading run of three or more of one character.
/// An ASCII alphanumeric first character is kept.
fn unpadded(collation: &str) -> &str {
    let Some(first) = collation.chars().next() else {
        return collation;
    };
    if first.is_alphanumeric() && first.is_ascii() {
        return collation;
    }
    let run = collation.chars().take_while(|c| *c == first).count();
    match run >= 3 {
        true => collation.trim_start_matches(first),
        false => collation,
    }
}

/// The first `CATALOG_PATHS` entry that exists.
pub fn path() -> Option<PathBuf> {
    CATALOG_PATHS.iter().map(PathBuf::from).find(|p| p.exists())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A store book, a sideload, a magazine, a cloud row, a loose file and an
    /// audiobook.
    fn fixture(name: &str) -> PathBuf {
        let path = std::env::temp_dir().join(format!("readinglog-catalog-{name}.db"));
        let _ = std::fs::remove_file(&path);
        // `p_contentState` is 1 on the store book and 0 on the sideload.
        let sql = "create table Entries (\
                p_contentSize integer, p_cdeKey text, p_cdeType text, \
                p_titles_0_nominal text, p_credits_0_name_collation text, \
                p_percentFinished real, p_thumbnail text, p_lastAccess integer, \
                p_languages_0 text, p_contentState integer, p_location text, \
                j_credits text);\
            insert into Entries values \
              (22782698, 'B00OKPCRLG', 'EBOK', 'The Jewish Study Bible', 'Adele Berlin', \
               76.12611, '/mnt/us/system/thumbnails/thumbnail_B00OKPCRLG_EBOK_portrait.jpg', \
               1786186690, 'en-GB', 1, '/mnt/us/documents/bible.kfx', \
               '[{\"name\":{\"display\":\"Adele Berlin\"},\"kind\":\"Author\"}]'), \
              (938018, 'CR!ABC123', 'PDOC', 'A Sideloaded Document', '', \
               null, '', 1786100000, 'ja-JP', 0, '/mnt/us/documents/sideload.kfx', ''), \
              (491227, 'CR!DEF456', 'MAGZ', 'A Fixed-Layout Magazine', 'A Publisher', \
               11.5, '', 1786100001, 'en', 0, '/mnt/us/documents/magazine.azw', ''), \
              (null, 'B01CLOUD01', 'EBOK', 'Never Downloaded', 'Someone', null, \
               '/mnt/us/system/thumbnails/thumbnail_B01CLOUD01_EBOK_portrait.jpg', \
               1783251156, '', 0, null, ''), \
              (22222, '*8e4f2a', 'PDOC', 'My Clippings.txt', '', 0.0, '', 0, '', 0, \
               '/mnt/us/documents/My Clippings.txt', ''), \
              (33333, 'B02AUDIO01', 'AUDI', 'An Audiobook', 'A Narrator', 0.0, '', 0, '', 1, \
               '/mnt/us/audible/book.aax', ''), \
              (777, '*aa11bb22', 'PDOC', 'Reading Log', '', 0.0, '', 0, '', 0, \
               '/mnt/us/documents/ReadingLog.sh', '');";
        let out = Command::new("sqlite3")
            .arg(&path)
            .arg(sql)
            .output()
            .expect("sqlite3 on PATH");
        assert!(out.status.success(), "fixture: {:?}", out);
        path
    }

    #[test]
    fn every_book_row_is_read_and_marked_for_where_it_sits() {
        let db = fixture("filters");
        let books = read_from(&db);
        // Three books held, the clippings file, the scriptlet, and one book in
        // the library alone.
        assert_eq!(books.len(), 6, "{books:#?}");
        assert_eq!(books.iter().filter(|b| b.on_device).count(), 5);
        assert!(!books.iter().any(|b| b.cde_type == "AUDI"));
        let _ = std::fs::remove_file(&db);
    }

    #[test]
    fn a_book_in_the_library_alone_carries_its_name_and_no_extent() {
        let db = fixture("cloud");
        let books = read_from(&db);
        let cloud = books
            .iter()
            .find(|b| b.cde_key == "B01CLOUD01")
            .expect("the book no longer on the device");
        assert!(!cloud.on_device);
        assert_eq!(cloud.title, "Never Downloaded");
        assert_eq!(cloud.author, "Someone");
        // `p_contentSize` is the one column a library row does not state.
        assert_eq!(cloud.extent, 0);
        assert!(cloud.percent < 0.0);
        let _ = std::fs::remove_file(&db);
    }

    #[test]
    fn a_sideload_is_read_though_its_content_state_is_zero() {
        let db = fixture("sideload");
        let books = read_from(&db);
        let doc = books
            .iter()
            .find(|b| b.cde_key == "CR!ABC123")
            .expect("the sideloaded book");
        assert_eq!(doc.title, "A Sideloaded Document");
        assert_eq!(doc.extent, 938_018);
        assert!(books.iter().any(|b| b.cde_type == "MAGZ"));
        let _ = std::fs::remove_file(&db);
    }

    #[test]
    fn a_book_carries_the_number_a_sitting_is_keyed_by() {
        let db = fixture("fields");
        let books = read_from(&db);
        let bible = books
            .iter()
            .find(|b| b.cde_key == "B00OKPCRLG")
            .expect("the downloaded book");
        assert_eq!(bible.extent, 22_782_698);
        assert_eq!(bible.title, "The Jewish Study Bible");
        assert_eq!(bible.author, "Adele Berlin");
        assert!((bible.percent - 76.12611).abs() < 1e-6);
        assert!(bible.percent >= 0.0);
        assert!(bible.thumbnail.ends_with("_EBOK_portrait.jpg"));
        assert_eq!(bible.last_access, 1_786_186_690);
        let _ = std::fs::remove_file(&db);
    }

    #[test]
    fn a_book_the_catalog_states_no_progress_for_says_so() {
        let db = fixture("nopercent");
        let books = read_from(&db);
        let doc = books
            .iter()
            .find(|b| b.cde_type == "PDOC")
            .expect("the sideloaded document");
        assert!(doc.percent < 0.0, "the catalog states none");
        assert_eq!(doc.author, "");
        assert_eq!(doc.thumbnail, "");
        let _ = std::fs::remove_file(&db);
    }

    #[test]
    fn a_cjk_author_is_read_without_its_sort_padding() {
        let ja = r#"[{"name":{"display":"TARO REIREI","collation":"ぁぁぁレイレイ タロウ","language":"ja"},"kind":"Author"}]"#;
        assert_eq!(author_name(ja, "ぁぁぁレイレイ タロウ"), "TARO REIREI");
        // An empty `j_credits` falls back to `collation`.
        assert_eq!(author_name("", "ぁぁぁレイレイ タロウ"), "レイレイ タロウ");
        assert_eq!(author_name("", "阿阿阿lilei"), "lilei");
    }

    #[test]
    fn a_latin_author_is_left_exactly_as_it_is() {
        assert_eq!(author_name("", "Ada Sprocket"), "Ada Sprocket");
        assert_eq!(author_name("", ""), "");
        // A run of two is kept.
        assert_eq!(author_name("", "ああ書店"), "ああ書店");
    }

    #[test]
    fn a_display_name_keeps_its_escaped_characters() {
        let json = r#"[{"name":{"display":"O\"Brien \\ Sons"},"kind":"Author"}]"#;
        assert_eq!(author_name(json, "x"), "O\"Brien \\ Sons");
        // An unterminated value falls back to `collation`.
        assert_eq!(author_name(r#"{"display":"unclosed"#, "ぁぁぁA"), "A");
    }

    #[test]
    fn a_loose_file_is_read_and_marked_no_book() {
        let db = fixture("loose");
        let books = read_from(&db);
        let script = books
            .iter()
            .find(|b| b.title == "Reading Log")
            .expect("the scriptlet");
        assert!(!script.is_book);
        let clippings = books
            .iter()
            .find(|b| b.title == "My Clippings.txt")
            .expect("the clippings file");
        assert!(!clippings.is_book);
        assert!(books.iter().filter(|b| b.is_book).count() == 4);
        let _ = std::fs::remove_file(&db);
    }

    #[test]
    fn a_catalog_that_is_not_there_is_an_empty_one() {
        assert!(read_from(Path::new("/nonexistent/cc.db")).is_empty());
    }
}
