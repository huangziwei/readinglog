//! `vocab.db`, the Vocabulary Builder's record of every word looked up while
//! reading. A lookup names its book by title and author and adds the content
//! key and a position; nothing here reaches the store.

use std::path::Path;

/// The database, one for the whole device. `VBSqlConstants.DATABASE_PATH`,
/// the same literal on 5.16, 5.18 and 5.19.
pub const VOCAB_DB: &str = "/mnt/us/system/vocabulary/vocab.db";

/// `LOOKUPS` joined to the book it names. `DictionaryCardProvider` writes one
/// row per in-book dictionary lookup and nothing rotates, caps or purges the
/// table, so it outlives the book.
const QUERY: &str = "select coalesce(b.title, ''), coalesce(b.authors, ''), \
     coalesce(b.asin, ''), coalesce(l.pos, ''), coalesce(l.timestamp, 0) \
     from LOOKUPS l join BOOK_INFO b on b.id = l.book_key \
     where l.timestamp > 0";

/// One dictionary lookup, and the book it was made in.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct Lookup {
    /// `LOOKUPS.timestamp` as `YYYY-MM-DDTHH:MM:SS` on the device's own clock,
    /// which is the form a sitting is stored under. Empty where the value
    /// names no instant.
    pub at: String,
    /// `BOOK_INFO.title`, which is `BookMetadata.getTitle()`.
    pub title: String,
    /// `BOOK_INFO.authors`, which is `BookMetadata.kv()`.
    pub author: String,
    /// `BOOK_INFO.asin`, the content key `BookRecord::cde_key` also holds.
    /// Empty where the book carried none.
    pub key: String,
    /// `LOOKUPS.pos`, on the `extent` axis. Negative where it named none.
    pub pos: i64,
}

/// Every lookup the device holds. Empty where there is no database.
pub fn read() -> Vec<Lookup> {
    read_from(Path::new(VOCAB_DB))
}

/// [`read`] against a named file, empty where there is none — asking
/// `sqlite3` for one would make it. The device's `VOCAB_BUILDER` switch gates
/// only *new* rows, so nothing here consults it.
pub fn read_from(db: &Path) -> Vec<Lookup> {
    if !db.exists() {
        return Vec::new();
    }
    let Some(rows) = crate::catalog::ask(db, QUERY, "vocab") else {
        return Vec::new();
    };
    rows.iter().filter_map(|row| parse_row(row)).collect()
}

fn parse_row(row: &str) -> Option<Lookup> {
    let mut f = row.split(crate::catalog::COL);
    let mut next = || f.next().unwrap_or_default();
    let title = next().to_string();
    if title.is_empty() {
        return None;
    }
    let author = next().to_string();
    let key = next().to_string();
    let pos = position(next());
    let ms: i64 = next().trim().parse().ok()?;
    Some(Lookup {
        at: stamp(ms),
        title,
        author,
        key,
        pos,
    })
}

/// `LOOKUPS.pos` as a number on the `extent` axis — the trailing integer of
/// `Position.oy()` whichever stack wrote it, and **not** the display location
/// a clipping's label carries. Negative where there is none.
fn position(pos: &str) -> i64 {
    pos.rsplit(':')
        .next()
        .and_then(|tail| tail.trim().parse().ok())
        .unwrap_or(-1)
}

/// `LOOKUPS.timestamp` — `new Date().getTime()`, epoch milliseconds — as the
/// device-local `YYYY-MM-DDTHH:MM:SS` a sitting is stored under. Empty where
/// the clock will not break the value down.
fn stamp(ms: i64) -> String {
    match crate::date::local_of(ms.div_euclid(1_000)) {
        Some((days, secs)) => crate::date::stamp(days, secs),
        None => String::new(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::process::Command;

    /// A `vocab.db` holding the tables `VBSqlConstants` names, built here
    /// rather than shipped, the way `catalog::tests::fixture` builds a
    /// catalog.
    fn fixture(name: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!("readinglog-vocab-{name}"));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("a scratch directory");
        let db = dir.join("vocab.db");
        let sql = "\
            create table BOOK_INFO (id text primary key, asin text, guid text, \
                lang text, title text, authors text);\n\
            create table LOOKUPS (id text primary key, word_key text, \
                book_key text, dict_key text, pos text, usage text, \
                timestamp integer);\n\
            insert into BOOK_INFO values \
                ('g1', 'B00OKPCRLG', 'g1', 'ja', 'A Book', 'An Author'),\n\
                ('g2', '', 'g2', 'en', 'A Sideload', ''),\n\
                ('g3', 'B00NOLOOKUP', 'g3', 'en', 'Never Opened', 'Nobody');\n\
            insert into LOOKUPS values \
                ('g1:1:4', 'ja:word', 'g1', 'D1', 'AQAAAAAAAAA:8410', '', 1757000000000),\n\
                ('g1:2:3', 'ja:more', 'g1', 'D1', '9902', '', 1757000600000),\n\
                ('g2:3:5', 'en:plain', 'g2', '', '', '', 1757001200000),\n\
                ('g4:4:2', 'en:orphan', 'g4', '', '55', '', 1757001800000);\n";
        let out = Command::new("sqlite3")
            .arg(&db)
            .arg(sql)
            .output()
            .expect("sqlite3 on PATH");
        assert!(
            out.stderr.is_empty(),
            "{}",
            String::from_utf8_lossy(&out.stderr)
        );
        db
    }

    #[test]
    fn a_lookup_carries_its_books_title_key_and_a_position() {
        let db = fixture("read");
        let got = read_from(&db);
        // The orphan names no `BOOK_INFO` row, so the join drops it.
        assert_eq!(got.len(), 3, "a lookup with no book was kept");
        assert_eq!(got[0].title, "A Book");
        assert_eq!(got[0].author, "An Author");
        assert_eq!(got[0].key, "B00OKPCRLG");
        assert_eq!(got[0].pos, 8_410, "the trailing integer of a KFX position");
        assert_eq!(got[1].pos, 9_902, "a mobi8 position is the bare integer");
        assert_eq!(got[2].key, "", "a sideload states no content key");
        assert_eq!(got[2].pos, -1, "a lookup that stated no position");
        let _ = std::fs::remove_dir_all(db.parent().unwrap());
    }

    #[test]
    fn a_timestamp_reads_as_the_local_wall_clock_a_sitting_is_stored_under() {
        let db = fixture("stamp");
        let got = read_from(&db);
        // The zone is the device's, so only the shape is fixed here.
        assert_eq!(got[0].at.len(), 19);
        assert_eq!(&got[0].at[4..5], "-");
        assert_eq!(&got[0].at[10..11], "T");
        assert_eq!(&got[0].at[13..14], ":");
        // Ten minutes apart, whatever the zone.
        let secs = |at: &str| crate::date::secs_of(at);
        assert_eq!(secs(&got[1].at) - secs(&got[0].at), 600);
        let _ = std::fs::remove_dir_all(db.parent().unwrap());
    }

    #[test]
    fn a_database_that_is_not_one_answers_no_lookups() {
        let dir = std::env::temp_dir().join("readinglog-vocab-notadb");
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("a scratch directory");
        let db = dir.join("vocab.db");
        std::fs::write(&db, b"not a database").expect("a written file");
        assert!(read_from(&db).is_empty());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_database_that_is_not_there_is_not_made() {
        let dir = std::env::temp_dir().join("readinglog-vocab-absent");
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("a scratch directory");
        let db = dir.join("vocab.db");
        assert!(read_from(&db).is_empty());
        assert!(!db.exists(), "a device file was made to read it");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_position_is_read_off_either_reader_stack() {
        assert_eq!(position("AQAAAAAAAAA:8410"), 8_410);
        assert_eq!(position("9902"), 9_902);
        assert_eq!(position(""), -1);
        assert_eq!(position("nothing:numeric"), -1);
    }
}
