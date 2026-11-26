//! Paper books written here: `p` and `q` rows of the record of a Kindle this
//! workspace holds, which that Kindle takes at its next sync and draws as it
//! draws its own books. [`add_book`] … [`delete`] write them, each refusal a
//! `paper::Refusal` or [`NoKindle`] the window states in its own words.

use anyhow::{Result, anyhow};
use serde::Deserialize;

use readinglog_native::date;
use readinglog_native::paper::{self, Refusal};

use crate::kindles::Source;
use crate::metadata::suggested_language;
use crate::workspace::{Workspace, keep_written, now};

/// `NewBook`: a paper book as the window adds it; a blank `language` takes the
/// one its characters say, where they say one.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct NewBook {
    pub title: String,
    #[serde(default)]
    pub author: String,
    #[serde(default)]
    pub language: String,
    pub pages: i64,
}

/// `Read`: a sitting as the window logs it: its day (`YYYY-MM-DD`), the minute
/// it opened and the minute it closed (`HH:MM`), the next day's where that is
/// earlier, and the pages it opened and closed on.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct Read {
    pub day: String,
    pub from: String,
    pub to: String,
    pub from_page: i64,
    pub to_page: i64,
}

impl Read {
    /// `stamps`: when it opened and closed, as a record stamps them; `None`
    /// where `day`, `from` or `to` is no day or time.
    fn stamps(&self) -> Option<(String, String)> {
        let day = date::parse_day(self.day.trim()).filter(|_| self.day.trim().len() == 10)?;
        let minute = |at: &str| -> Option<i64> {
            let mut parts = at.trim().split(':');
            let mut next = || -> Option<i64> {
                let part = parts.next()?;
                let digits = part.len() == 2 && part.bytes().all(|b| b.is_ascii_digit());
                digits.then(|| part.parse().ok()).flatten()
            };
            let (h, m) = (next()?, next()?);
            (h < 24 && m < 60).then_some(h * 3600 + m * 60)
        };
        let (from, to) = (minute(&self.from)?, minute(&self.to)?);
        let closes = match to < from {
            true => day + 1,
            false => day,
        };
        Some((date::stamp(day, from), date::stamp(closes, to)))
    }
}

/// `NoKindle`: why a paper book has nowhere to go: the workspace holds no
/// Kindle's record for its rows to ride in.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct NoKindle;

impl std::fmt::Display for NoKindle {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("a paper book goes into a Kindle's record, and this folder holds none yet")
    }
}

impl std::error::Error for NoKindle {}

/// `kindles`: the cut serials of the Kindles whose records `ws` holds, which a
/// paper book can go into.
pub fn kindles(ws: &Workspace) -> Vec<String> {
    ws.sources().into_iter().filter_map(|s| s.serial).collect()
}

/// `source`: the record of the Kindle `kindle` names, the one Kindle `ws`
/// holds where `kindle` is empty; [`NoKindle`] where it holds none.
fn source(ws: &Workspace, kindle: &str) -> Result<Source> {
    let mut held: Vec<Source> = ws
        .sources()
        .into_iter()
        .filter(|s| s.serial.is_some())
        .collect();
    if held.is_empty() {
        return Err(NoKindle.into());
    }
    let at = match (kindle.is_empty(), held.len()) {
        (true, 1) => 0,
        _ => held
            .iter()
            .position(|s| s.serial.as_deref() == Some(kindle))
            .ok_or_else(|| anyhow!("no record of {kindle} in {}", ws.dir.display()))?,
    };
    Ok(held.swap_remove(at))
}

/// `add_book` adds `book` to the record of the Kindle `kindle` names, at the
/// clock's moment, and answers the key the book draws under.
pub fn add_book(ws: &Workspace, kindle: &str, book: &NewBook) -> Result<String> {
    let mut source = source(ws, kindle)?;
    let language = match book.language.trim() {
        "" => suggested_language(&book.title, &book.author).unwrap_or_default(),
        code => code,
    };
    let added =
        source
            .record
            .add_paper_book(&book.title, &book.author, language, book.pages, &now())?;
    keep_written(&source)?;
    Ok(paper::key(&added))
}

/// `set_pages` sets the pages of the book added at `added`.
pub fn set_pages(ws: &Workspace, kindle: &str, added: &str, pages: i64) -> Result<()> {
    let mut source = source(ws, kindle)?;
    source.record.set_paper_pages(added, pages, &now())?;
    keep_written(&source)
}

/// `sittings_in`: how many sittings stand in the book added at `added`, the ones
/// [`delete_book`] takes with it.
pub fn sittings_in(ws: &Workspace, kindle: &str, added: &str) -> Result<usize> {
    Ok(source(ws, kindle)?.record.paper_sittings_of(added).len())
}

/// `delete_book` deletes the book added at `added`, its sittings with it.
pub fn delete_book(ws: &Workspace, kindle: &str, added: &str) -> Result<()> {
    let mut source = source(ws, kindle)?;
    source.record.delete_paper_book(added, &now())?;
    keep_written(&source)
}

/// `log` logs `read` in the book added at `book`, and answers the moment that
/// names the sitting.
pub fn log(ws: &Workspace, kindle: &str, book: &str, read: &Read) -> Result<String> {
    let mut source = source(ws, kindle)?;
    let (from, to) = read.stamps().ok_or(Refusal::NotAnInstant)?;
    let logged = source.record.log_paper_sitting(
        book,
        (&from, &to),
        (read.from_page, read.to_page),
        &now(),
    )?;
    keep_written(&source)?;
    Ok(logged)
}

/// `edit` sets the sitting logged at `logged` to `read`.
pub fn edit(ws: &Workspace, kindle: &str, logged: &str, read: &Read) -> Result<()> {
    let mut source = source(ws, kindle)?;
    let (from, to) = read.stamps().ok_or(Refusal::NotAnInstant)?;
    source.record.edit_paper_sitting(
        logged,
        (&from, &to),
        (read.from_page, read.to_page),
        &now(),
    )?;
    keep_written(&source)
}

/// `delete` deletes the sitting logged at `logged`.
pub fn delete(ws: &Workspace, kindle: &str, logged: &str) -> Result<()> {
    let mut source = source(ws, kindle)?;
    source.record.delete_paper_sitting(logged, &now())?;
    keep_written(&source)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tests::{book, record_of, scratch, sitting};
    use readinglog_native::record::Record;

    /// `with_kindle`: a workspace holding the record of the Kindle `G000K1`, one book read once.
    fn with_kindle(name: &str) -> Workspace {
        let ws = Workspace::create(&scratch(name)).unwrap();
        let record = record_of(
            vec![book(100, "B1", "One")],
            vec![sitting("2026-09-01T09:00:00", 100, 600)],
        );
        record
            .save(&ws.kindle_dir("G000K1").join("record"))
            .unwrap();
        ws
    }

    fn read(day: &str, from: &str, to: &str, pages: (i64, i64)) -> Read {
        Read {
            day: day.into(),
            from: from.into(),
            to: to.into(),
            from_page: pages.0,
            to_page: pages.1,
        }
    }

    #[test]
    fn a_sitting_is_a_day_and_two_minutes_the_close_running_into_the_next_day() {
        let stamps = |r: Read| r.stamps();
        assert_eq!(
            stamps(read("2026-09-02", "21:10", "21:55", (0, 1))),
            Some(("2026-09-02T21:10:00".into(), "2026-09-02T21:55:00".into()))
        );
        assert_eq!(
            stamps(read("2026-09-02", "23:20", "00:30", (0, 1))),
            Some(("2026-09-02T23:20:00".into(), "2026-09-03T00:30:00".into()))
        );
        assert_eq!(
            stamps(read("2026-09-02", "21:10:30", "21:55", (0, 1))),
            Some(("2026-09-02T21:10:00".into(), "2026-09-02T21:55:00".into())),
            "a time naming its seconds keeps its minute"
        );
        for (day, from, to) in [
            ("2026-09-02", "", "21:55"),
            ("2026-9-2", "21:10", "21:55"),
            ("2026-09-02", "24:00", "01:00"),
            ("2026-09-02", "9:5", "10:00"),
        ] {
            assert_eq!(
                stamps(read(day, from, to, (0, 1))),
                None,
                "{day} {from} {to}"
            );
        }
    }

    #[test]
    fn a_paper_book_and_its_sittings_land_in_the_kindles_record() {
        let ws = with_kindle("paper-lands");
        let key = add_book(
            &ws,
            "",
            &NewBook {
                title: "こころ".into(),
                author: "夏目漱石".into(),
                language: String::new(),
                pages: 300,
            },
        )
        .unwrap();
        let held = Record::load(&ws.kindle_dir("G000K1").join("record"));
        assert_eq!(held.paper_books.len(), 1);
        assert_eq!(held.paper_books[0].language, "ja", "the kana say Japanese");
        assert_eq!(held.paper_books[0].key(), key);
        let added = held.paper_books[0].added.clone();
        let logged = log(
            &ws,
            "G000K1",
            &added,
            &read("2026-09-02", "21:10", "21:55", (0, 40)),
        )
        .unwrap();
        // `again` overlaps `logged`: refused in the record's own words.
        let again = log(
            &ws,
            "G000K1",
            &added,
            &read("2026-09-02", "21:30", "22:00", (40, 50)),
        )
        .unwrap_err();
        assert_eq!(again.downcast_ref::<Refusal>(), Some(&Refusal::Overlaps));
        edit(
            &ws,
            "G000K1",
            &logged,
            &read("2026-09-02", "21:10", "22:10", (0, 45)),
        )
        .unwrap();
        assert_eq!(
            set_pages(&ws, "G000K1", &added, 44)
                .unwrap_err()
                .downcast_ref::<Refusal>(),
            Some(&Refusal::UnderLogged)
        );
        set_pages(&ws, "G000K1", &added, 320).unwrap();
        let held = Record::load(&ws.kindle_dir("G000K1").join("record"));
        assert_eq!(held.paper_books[0].pages, 320);
        assert_eq!(held.paper_sittings.len(), 1);
        assert_eq!(held.paper_sittings[0].to, "2026-09-02T22:10:00");
        assert_eq!(held.sittings.len(), 1, "the Kindle's own rows stand");
        let drawn = held.drawn().record;
        assert_eq!(drawn.books.len(), 2);
        assert_eq!(drawn.sittings.len(), 2);
        // `on`: a note written on the paper book, laid in under it.
        let on = crate::notes::On::Book {
            key: key.clone(),
            extent: held.paper_books[0].extent(),
        };
        crate::notes::write(&ws, "G000K1", &on, "Read on the train").unwrap();
        let drawn = Record::load(&ws.kindle_dir("G000K1").join("record")).drawn();
        assert_eq!(drawn.written.len(), 1);
        let book = drawn
            .record
            .books
            .iter()
            .find(|b| b.cde_key == key)
            .unwrap();
        assert_eq!(drawn.record.highlights[0].extent, book.extent);
        delete(&ws, "G000K1", &logged).unwrap();
        delete_book(&ws, "G000K1", &added).unwrap();
        let held = Record::load(&ws.kindle_dir("G000K1").join("record"));
        assert!(held.paper_books[0].deleted && held.paper_sittings[0].deleted);
        assert_eq!(held.drawn().record.books.len(), 1);
    }

    #[test]
    fn a_folder_holding_no_kindle_has_nowhere_to_put_one_and_several_ask_which() {
        let ws = Workspace::create(&scratch("paper-nowhere")).unwrap();
        let new = NewBook {
            title: "A Book".into(),
            author: String::new(),
            language: "en".into(),
            pages: 100,
        };
        let err = add_book(&ws, "", &new).unwrap_err();
        assert_eq!(err.downcast_ref::<NoKindle>(), Some(&NoKindle));
        let ws = with_kindle("paper-several");
        record_of(vec![book(200, "B2", "Two")], Vec::new())
            .save(&ws.kindle_dir("G000K2").join("record"))
            .unwrap();
        assert_eq!(kindles(&ws), vec!["G000K1", "G000K2"]);
        assert!(add_book(&ws, "", &new).is_err(), "two Kindles: which one");
        add_book(&ws, "G000K2", &new).unwrap();
        let held = Record::load(&ws.kindle_dir("G000K2").join("record"));
        assert_eq!(held.paper_books.len(), 1);
    }
}
