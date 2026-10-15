//! Every Kindle's record laid together as the one the screens draw: a sitting
//! two records hold counts once, a book's state is the Kindle's that read it
//! furthest, and the record no Kindle was named for never outranks a Kindle's.

use std::collections::HashMap;

use readinglog_native::record::{Book, Record};

/// Every book row of `records`, by `(extent, cde_key)`, in their order.
fn rows_of<'a>(
    records: impl Iterator<Item = &'a Record>,
) -> HashMap<(i64, &'a str), Vec<&'a Book>> {
    let mut out: HashMap<(i64, &str), Vec<&Book>> = HashMap::new();
    for book in records.flat_map(|r| &r.books) {
        out.entry((book.extent, book.cde_key.as_str()))
            .or_default()
            .push(book);
    }
    out
}

/// `kindles`' records and `unnamed`, the backups no Kindle was named for, as
/// one record:
///
/// - every sitting once per `(started_at, end_position)`, as `Record::merge`
///   keeps one;
/// - every book once per `(extent, cde_key)`. Where Kindles hold it, the row of
///   the one that read furthest stands whole, finished and kept where any of
///   them says so, with every restart's day, and on a Kindle where any holds
///   it. `unnamed`'s row stands only where no Kindle holds the book;
/// - every highlight once per `Highlight::identity`.
///
/// Where two records state one sitting or book alike, the one whose watermark
/// is newer stands: a backup restored onto a second Kindle leaves both holding
/// a sitting only one of them goes on measuring. A text field the standing row
/// states none of is taken from another row, and the newest watermark stands.
pub fn together(kindles: &[Record], unnamed: &Record) -> Record {
    let mut kindles: Vec<&Record> = kindles.iter().collect();
    kindles.sort_by(|a, b| b.watermark.cmp(&a.watermark));
    let every = || kindles.iter().copied().chain(std::iter::once(unnamed));
    let mut out = Record::default();
    for record in every() {
        out.merge(record);
    }
    let held = rows_of(kindles.iter().copied());
    let none = rows_of(std::iter::once(unnamed));
    for book in &mut out.books {
        let key = (book.extent, book.cde_key.as_str());
        let (held, none) = (
            held.get(&key).map_or(&[][..], Vec::as_slice),
            none.get(&key).map_or(&[][..], Vec::as_slice),
        );
        let rows = match held.is_empty() {
            true => none,
            false => held,
        };
        let Some(furthest) = rows
            .iter()
            .copied()
            .reduce(|a, b| if b.percent > a.percent { b } else { a })
        else {
            continue;
        };
        let mut row = furthest.clone();
        for other in rows {
            row.finished |= other.finished;
            row.kept |= other.kept;
            row.on_kindle |= other.on_kindle;
            for day in &other.restarted_on {
                if !row.restarted_on.contains(day) {
                    row.restarted_on.push(*day);
                }
            }
        }
        row.restarted_on.sort_unstable();
        for other in held.iter().chain(none) {
            for (field, stated) in [
                (&mut row.cde_type, &other.cde_type),
                (&mut row.title, &other.title),
                (&mut row.author, &other.author),
                (&mut row.thumbnail, &other.thumbnail),
                (&mut row.language, &other.language),
                (&mut row.cover, &other.cover),
            ] {
                if field.is_empty() {
                    field.clone_from(stated);
                }
            }
        }
        *book = row;
    }
    out.watermark = every()
        .map(|r| r.watermark.as_str())
        .max()
        .unwrap_or_default()
        .to_string();
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tests::{book, record_of, sitting};

    /// A book at `percent` with its timer at `ms`, on the Kindle or not.
    fn at(percent: f64, ms: i64, on: bool) -> Book {
        Book {
            percent,
            timer_ms: ms,
            on_kindle: on,
            ..book(100, "B1", "One")
        }
    }

    #[test]
    fn a_book_two_kindles_hold_is_the_one_read_furthest() {
        let near = record_of(
            vec![at(10.0, 60_000, true)],
            vec![sitting("2026-09-01T09:00:00", 100, 600)],
        );
        let mut far_book = at(60.0, 3_600_000, false);
        far_book.restarted_on = vec![20_300];
        let far = record_of(
            vec![far_book],
            vec![sitting("2026-09-02T09:00:00", 100, 900)],
        );
        let one = together(&[near, far], &Record::default());
        assert_eq!(one.books.len(), 1);
        let held = &one.books[0];
        assert_eq!((held.percent, held.timer_ms), (60.0, 3_600_000));
        assert!(held.on_kindle, "on the Kindle that holds it");
        assert_eq!(held.restarted_on, vec![20_300]);
        assert_eq!(one.sittings.len(), 2);
    }

    #[test]
    fn a_sitting_two_kindles_hold_counts_once() {
        let first = record_of(
            vec![book(100, "B1", "One")],
            vec![sitting("2026-09-01T09:00:00", 100, 600)],
        );
        let restored = record_of(
            vec![book(100, "B1", "One")],
            vec![
                sitting("2026-09-01T09:00:00", 100, 600),
                sitting("2026-09-03T09:00:00", 100, 300),
            ],
        );
        let one = together(&[first, restored], &Record::default());
        assert_eq!(one.sittings.len(), 2);
    }

    #[test]
    fn a_sitting_two_kindles_hold_is_the_one_the_newer_record_states() {
        let mut older = record_of(
            vec![book(100, "B1", "One")],
            vec![sitting("2026-09-01T09:00:00", 100, 600)],
        );
        older.sittings[0].seconds = 6;
        older.watermark = "260907:100000".into();
        let mut newer = older.clone();
        newer.sittings[0].seconds = 106;
        newer.watermark = "260910:100000".into();
        for (first, second) in [(&older, &newer), (&newer, &older)] {
            let one = together(&[first.clone(), second.clone()], &Record::default());
            assert_eq!(one.sittings.len(), 1);
            assert_eq!(one.sittings[0].seconds, 106);
        }
    }

    #[test]
    fn a_record_no_kindle_was_named_for_never_outranks_a_kindles() {
        let mut stale = at(100.0, 3_600_000, true);
        stale.finished = true;
        let mut stale_record = record_of(vec![stale], vec![]);
        stale_record.books.push(book(200, "B2", "Only in a backup"));
        let mut restarted = at(2.0, 60_000, false);
        restarted.restarted_on = vec![20_336];
        restarted.title = String::new();
        let kindle = record_of(vec![restarted], vec![]);

        let one = together(&[kindle], &stale_record);
        let held = &one.books[0];
        assert!(!held.finished);
        assert_eq!((held.percent, held.timer_ms), (2.0, 60_000));
        assert!(!held.on_kindle);
        assert_eq!(held.title, "One", "a title the Kindle's row states none of");
        assert_eq!(one.books[1].title, "Only in a backup");
    }
}
