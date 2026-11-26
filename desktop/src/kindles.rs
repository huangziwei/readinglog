//! [`together`] lays each [`Source`]'s record beside the others' as one record, and
//! [`labels`] names each source's Kindle.

use std::path::{Path, PathBuf};

use readinglog_native::covers;
use readinglog_native::record::{self, Record};

/// `Source`: a record, the folder it was loaded from, and the Kindle it is from.
pub struct Source {
    pub record: Record,
    /// `dir`: the folder the record was loaded from, holding its covers.
    pub dir: PathBuf,
    /// `serial`: the Kindle's serial, cut; `None` for backups naming no Kindle.
    pub serial: Option<String>,
}

/// `SPACE`: the `n`th source's extents and end positions start at `(n + 1) * SPACE`.
const SPACE: i64 = 1 << 40;

/// `KEY_MARK`: between a `cde_key` and the number of the source [`together`] took it from.
const KEY_MARK: char = '@';

/// `sources` as one record, each through `Record::drop_nested`: one as it is, two
/// or more each in a space of its own — every extent, end position and key moved,
/// each highlight whole, line breaks and all — and the newest watermark standing.
pub fn together(sources: &[Source]) -> Record {
    let loaded = |record: &Record| {
        let mut out = record.clone();
        out.drop_nested();
        out
    };
    match sources {
        [] => Record::default(),
        [one] => loaded(&one.record),
        many => {
            let parts: Vec<Record> = many
                .iter()
                .enumerate()
                .map(|(n, source)| apart(&loaded(&source.record), n))
                .collect();
            let text: String = parts.iter().map(Record::text).collect();
            let mut out = Record::from_text(&text);
            out.highlights = parts
                .iter()
                .flat_map(|part| part.highlights.iter().cloned())
                .collect();
            out.sort_highlights();
            out.watermark = many
                .iter()
                .map(|s| s.record.watermark.as_str())
                .max()
                .unwrap_or_default()
                .to_string();
            out
        }
    }
}

/// `record` in the space of the `n`th source.
fn apart(record: &Record, n: usize) -> Record {
    let into = |value: i64| match value > 0 {
        true => value + (n as i64 + 1) * SPACE,
        false => value,
    };
    let mark = |key: &str| match key.is_empty() {
        true => String::new(),
        false => format!("{key}{KEY_MARK}{n}"),
    };
    let mut out = record.clone();
    for sitting in &mut out.sittings {
        sitting.end_position = into(sitting.end_position);
        sitting.asin = sitting.asin.as_deref().map(mark);
    }
    for book in &mut out.books {
        book.extent = into(book.extent);
        book.cde_key = mark(&book.cde_key);
    }
    for highlight in &mut out.highlights {
        highlight.extent = into(highlight.extent);
    }
    for end in &mut out.ends {
        *end = (into(end.0), into(end.1));
    }
    for counter in &mut out.counters {
        counter.0 = into(counter.0);
    }
    for (extent, key) in &mut out.keys {
        *extent = into(*extent);
        *key = mark(key);
    }
    for (extent, file) in &mut out.pairs {
        *extent = into(*extent);
        *file = mark(file);
    }
    for cleared in &mut out.cleared {
        cleared.extent = into(cleared.extent);
        cleared.key = mark(&cleared.key);
    }
    for note in &mut out.notes {
        note.extent = into(note.extent);
        note.key = mark(&note.key);
    }
    for row in &mut out.metadata {
        row.extent = into(row.extent);
        row.key = mark(&row.key);
    }
    out
}

/// `origin`: the source [`together`] took `extent` from, and the extent as that source states it;
/// `None` and `extent` itself for one it did not move.
pub fn origin(extent: i64) -> (Option<usize>, i64) {
    match extent > SPACE {
        true => (Some((extent / SPACE - 1) as usize), extent % SPACE),
        false => (None, extent),
    }
}

/// `key_origin`: the source [`together`] took `key` from, and the key as that source states it;
/// `None` and `key` itself for one it did not mark.
pub fn key_origin(key: &str) -> (Option<usize>, &str) {
    match key.rsplit_once(KEY_MARK) {
        Some((own, n)) if !n.is_empty() && n.bytes().all(|b| b.is_ascii_digit()) => {
            (n.parse().ok(), own)
        }
        _ => (None, key),
    }
}

/// `cover`: the copy of the cover `key` names: in its own source's folder for a key [`together`]
/// marked, else in the first of `dirs` holding one.
pub fn cover(dirs: &[PathBuf], key: &str) -> Option<PathBuf> {
    let held = |dir: &Path, key: &str| covers::held(dir, key).then(|| covers::path(dir, key));
    match key_origin(key) {
        (Some(n), own) => dirs.get(n).and_then(|dir| held(dir, own)),
        (None, own) => dirs.iter().find_map(|dir| held(dir, own)),
    }
}

/// `book` with its cover pointing at the copy [`cover`] finds under `dirs`, at
/// none where no copy there draws or `cde_key` is empty, and no `thumbnail`.
pub fn on_this_computer(book: &record::Book, dirs: &[PathBuf]) -> record::Book {
    let cover = match book.cde_key.is_empty() {
        true => None,
        false => cover(dirs, &book.cde_key),
    }
    .map(|at| at.to_string_lossy().into_owned())
    .unwrap_or_default();
    record::Book {
        cover,
        thumbnail: String::new(),
        ..book.clone()
    }
}

/// `model`: the model `serial` names, `Kindle` where no table holds its code.
pub fn model(serial: &str) -> &'static str {
    crate::device::model::of(serial).unwrap_or("Kindle")
}

/// `named`: what the Kindle `serial` names is called beside another of its model: its
/// [`model`] and the serial's last four.
pub fn named(serial: &str) -> String {
    format!("{} {}", model(serial), tail(serial))
}

/// `labels`: what each of `sources` is called: its Kindle's [`model`], with the serial's last four
/// where two share one; `unnamed` for the backups naming no Kindle.
pub fn labels(sources: &[Source], unnamed: &str) -> Vec<String> {
    let models: Vec<Option<&str>> = sources
        .iter()
        .map(|s| s.serial.as_deref().map(model))
        .collect();
    sources
        .iter()
        .zip(&models)
        .map(|(source, model)| match (model, &source.serial) {
            (Some(model), Some(serial)) => {
                match models.iter().filter(|m| *m == &Some(*model)).count() > 1 {
                    true => format!("{model} {}", tail(serial)),
                    false => (*model).to_string(),
                }
            }
            _ => unnamed.to_string(),
        })
        .collect()
}

/// `serial`'s last four characters.
fn tail(serial: &str) -> String {
    let chars: Vec<char> = serial.chars().collect();
    chars[chars.len().saturating_sub(4)..].iter().collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tests::{book, record_of, sitting};
    use readinglog_native::record::Book;
    use readinglog_native::stats::Stats;

    fn source(record: Record, serial: Option<&str>) -> Source {
        Source {
            record,
            dir: PathBuf::from("/nowhere"),
            serial: serial.map(String::from),
        }
    }

    /// `reading`: one book, `B1` at extent 100, at `percent`, read for `seconds` on the first of
    /// September.
    fn reading(percent: f64, seconds: i64) -> Record {
        record_of(
            vec![Book {
                percent,
                ..book(100, "B1", "One")
            }],
            vec![sitting("2026-09-01T09:00:00", 100, seconds)],
        )
    }

    #[test]
    fn one_source_stands_as_it_is() {
        let only = reading(40.0, 600);
        let one = together(&[source(only.clone(), Some("G003H2-7890"))]);
        assert_eq!(one.text(), only.text());
    }

    #[test]
    fn a_book_two_kindles_hold_is_two_books() {
        let sources = [
            source(reading(10.0, 600), Some("G003H2-7890")),
            source(reading(60.0, 900), Some("G0027J-ABCD")),
        ];
        let one = together(&sources);
        assert_eq!(one.books.len(), 2);
        assert_eq!(one.sittings.len(), 2, "each Kindle's sitting is its own");
        let stats = Stats::build(
            &one,
            readinglog_native::date::parse_day("2026-09-01").unwrap(),
            true,
            readinglog_native::settings::StatsFrom::App,
            readinglog_native::settings::SittingFloor::All,
        );
        assert_eq!(stats.books.len(), 2);
        let mut read: Vec<(Option<usize>, i64, i64)> = stats
            .books
            .iter()
            .map(|b| {
                let (from, extent) = origin(b.extent);
                (from, extent, b.seconds)
            })
            .collect();
        read.sort();
        assert_eq!(read, vec![(Some(0), 100, 600), (Some(1), 100, 900)]);
        for b in &stats.books {
            let (from, key) = key_origin(&b.cde_key);
            assert_eq!((from, key), (origin(b.extent).0, "B1"));
        }
    }

    #[test]
    fn a_note_keeps_its_line_breaks_where_two_kindles_meet() {
        use readinglog_native::clippings::Kind;
        use readinglog_native::highlights::Highlight;
        let mut first = reading(10.0, 600);
        first.highlights.push(Highlight {
            extent: 100,
            title: "One".into(),
            kind: Kind::Note,
            at: "2026-09-01T09:05:00".into(),
            body: "# A title\n\nIts words".into(),
            ..Highlight::default()
        });
        let sources = [
            source(first, Some("G003H2-7890")),
            source(reading(60.0, 900), Some("G0027J-ABCD")),
        ];
        let one = together(&sources);
        assert_eq!(one.highlights.len(), 1);
        assert_eq!(one.highlights[0].body, "# A title\n\nIts words");
        assert_eq!(origin(one.highlights[0].extent), (Some(0), 100));
    }

    /// `together` keeps each Kindle's `d` rows on its own book, the same title set on the same
    /// book of two Kindles two rows.
    #[test]
    fn a_book_two_kindles_retitle_keeps_each_kindles_title() {
        use readinglog_native::metadata::{Field, Metadata, Set};
        let titled = |title: &str| {
            let mut record = reading(10.0, 600);
            record.metadata.push(Metadata {
                edited: "2026-11-14T10:00:00".into(),
                key: "B1".into(),
                extent: 100,
                field: Field::Title,
                value: Some(title.into()),
                was: None,
            });
            record
        };
        let sources = [
            source(titled("First"), Some("G003H2-7890")),
            source(titled("Second"), Some("G0027J-ABCD")),
        ];
        let one = together(&sources);
        assert_eq!(one.metadata.len(), 2);
        for (at, title) in [(0, "First"), (1, "Second")] {
            let book = one
                .books
                .iter()
                .find(|b| origin(b.extent).0 == Some(at))
                .expect("each Kindle's book");
            let set = Set::of(&one.metadata, &book.cde_key, book.extent);
            assert_eq!(set.title.as_deref(), Some(title));
        }
    }

    /// `together` over two Kindles' drawn records, as `Shown::of` lays them: the paper book one
    /// holds comes through as a plain book, its key's prefix, its reading and its Kindle standing.
    #[test]
    fn a_paper_book_keeps_its_prefix_and_its_reading_where_two_kindles_meet() {
        use readinglog_native::settings::{SittingFloor, StatsFrom};
        let mut first = reading(10.0, 600);
        let added = first
            .add_paper_book("On Paper", "", "en", 100, "2026-08-30T08:00:00")
            .unwrap();
        first
            .log_paper_sitting(
                &added,
                ("2026-09-01T20:00:00", "2026-09-01T21:00:00"),
                (0, 50),
                "2026-09-02T00:00:00",
            )
            .unwrap();
        let sources = [
            source(first.drawn().record, Some("G003H2-7890")),
            source(reading(60.0, 900).drawn().record, Some("G0027J-ABCD")),
        ];
        let one = together(&sources);
        assert!(one.paper_books.is_empty(), "laid in as books already");
        let day = readinglog_native::date::parse_day("2026-09-02").unwrap();
        for from in [StatsFrom::App, StatsFrom::Kindle] {
            let stats = Stats::build(&one, day, true, from, SittingFloor::All);
            assert_eq!(stats.books.len(), 3);
            let paper = stats.books.iter().find(|b| b.is_paper()).unwrap();
            assert_eq!((paper.seconds, paper.page_turns), (3600, 50));
            assert_eq!(paper.percent_shown(), 50);
            assert_eq!(paper.time_left(from), Some(3600), "{from:?}");
            assert_eq!(origin(paper.extent).0, Some(0));
            let key = readinglog_native::paper::key(&added);
            assert_eq!(key_origin(&paper.cde_key), (Some(0), key.as_str()));
        }
    }

    #[test]
    fn a_key_or_extent_together_did_not_move_names_no_source() {
        assert_eq!(origin(100), (None, 100));
        assert_eq!(origin(0), (None, 0));
        assert_eq!(key_origin("B1"), (None, "B1"));
        assert_eq!(key_origin("name@home"), (None, "name@home"));
        assert_eq!(key_origin("*1a9c@3"), (Some(3), "*1a9c"));
    }

    #[test]
    fn a_kindle_is_called_by_its_model_and_twins_by_their_serials() {
        let named = |serials: &[Option<&str>]| {
            let sources: Vec<Source> = serials
                .iter()
                .map(|s| source(Record::default(), *s))
                .collect();
            labels(&sources, "Backup")
        };
        assert_eq!(
            named(&[Some("G003H2-7890"), Some("G0027J-ABCD"), None]),
            vec!["Kindle Colorsoft", "Kindle Scribe", "Backup"]
        );
        assert_eq!(
            named(&[Some("G003H2-7890"), Some("G003H4-1234")]),
            vec!["Kindle Colorsoft 7890", "Kindle Colorsoft 1234"]
        );
        assert_eq!(named(&[Some("G00ZZZ-0000")]), vec!["Kindle"]);
        assert_eq!(super::named("G003H2-7890"), "Kindle Colorsoft 7890");
    }
}
