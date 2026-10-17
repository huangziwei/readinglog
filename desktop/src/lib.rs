//! [`import`] fills a [`workspace`] off backups and Kindles read over [`device`];
//! [`edits`] and [`notes`] write into each Kindle's record, [`kindles`] lays them
//! together, [`screen`] draws them, [`app`] is the window.

pub mod app;
pub mod device;
pub mod edits;
pub mod import;
pub mod kindles;
pub mod notes;
pub mod screen;
pub mod workspace;

#[cfg(test)]
pub(crate) mod tests {
    use std::path::PathBuf;

    use readinglog_native::clippings::Kind;
    use readinglog_native::date;
    use readinglog_native::highlights::Highlight;
    use readinglog_native::log::sitting::{Measure, Sitting};
    use readinglog_native::record::{Book, Record};

    /// A folder no earlier run has touched, named for the test and the clock.
    pub fn scratch(name: &str) -> PathBuf {
        let clock = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or_default();
        let dir = std::env::temp_dir().join(format!(
            "readinglog-desktop-{name}-{}-{clock}",
            std::process::id()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    /// A book the library listed at `extent` under `key`.
    pub fn book(extent: i64, key: &str, title: &str) -> Book {
        Book {
            extent,
            cde_key: key.into(),
            title: title.into(),
            author: "Someone".into(),
            language: "en".into(),
            percent: 40.0,
            on_kindle: true,
            ..Book::default()
        }
    }

    /// A sitting opening at `started_at` on `end_position`, `seconds` long, all
    /// of it in the hour it opened in.
    pub fn sitting(started_at: &str, end_position: i64, seconds: i64) -> Sitting {
        let opened = date::secs_of(started_at);
        let closed = opened + seconds;
        Sitting {
            started_at: started_at.into(),
            ended_at: format!(
                "{}T{:02}:{:02}:{:02}",
                date::day_of(started_at),
                closed / 3600,
                closed % 3600 / 60,
                closed % 60
            ),
            end_position,
            seconds,
            page_turns: 10,
            words: 2000,
            measure: Measure::Counted,
            hours: vec![((opened / 3600) as u8, seconds)],
            ..Sitting::default()
        }
    }

    /// A passage highlighted in the book at `extent`, at `at`.
    pub fn highlight(extent: i64, title: &str, at: &str) -> Highlight {
        Highlight {
            extent,
            title: title.into(),
            kind: Kind::Highlight,
            at: at.into(),
            location: 120,
            body: "Some words".into(),
            ..Highlight::default()
        }
    }

    /// A record of `books` and `sittings`, in the order a load puts rows in.
    pub fn record_of(books: Vec<Book>, sittings: Vec<Sitting>) -> Record {
        let record = Record {
            books,
            sittings,
            ..Record::default()
        };
        Record::from_text(&record.text())
    }
}
