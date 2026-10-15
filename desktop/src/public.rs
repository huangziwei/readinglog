//! The public record: the record less every book, sitting and highlight `Edits`
//! hides. `Public::stats` is `Stats::build` over it: one population for a total
//! and its parts, holding none of a hidden book's reading.

use std::path::PathBuf;

use readinglog_native::date;
use readinglog_native::highlights::Highlight;
use readinglog_native::record::{self, Record};
use readinglog_native::settings::Settings;
use readinglog_native::stats::Stats;

use crate::edits::Edits;
use crate::workspace::NewBooks;

/// The public record and its `Stats`.
pub struct Public {
    pub record: Record,
    pub stats: Stats,
    /// The newest public sitting's day, the `today` of `stats`; `None` with none.
    pub as_of: Option<i64>,
}

impl Public {
    /// `record` less what `edits` hides, its covers found in `record_dirs` by
    /// `kindles::cover`, stated under `settings`.
    pub fn of(
        record: &Record,
        record_dirs: &[PathBuf],
        edits: &Edits,
        settings: &Settings,
        new_books: NewBooks,
    ) -> Self {
        let shown =
            |b: &record::Book| b.is_book() && edits.book_shown(&b.cde_key, b.extent, new_books);
        // `sittings`: each on the book `Stats::build` places it on, an
        // unidentified one on none.
        let sittings = record
            .sittings
            .iter()
            .filter(|s| {
                let book = record.book_for(record.extent_of(s.end_position), s.asin.as_deref());
                book.is_some_and(shown) && edits.sitting_shown(s)
            })
            .cloned()
            .collect();
        let books = record
            .books
            .iter()
            .filter(|b| shown(b))
            .map(|b| on_this_computer(b, record_dirs))
            .collect();
        let public = Record {
            sittings,
            books,
            highlights: highlights_shown(record, edits, settings, new_books),
            ..record.clone()
        };
        let as_of = public
            .sittings
            .iter()
            .filter_map(|s| date::parse_day(date::day_of(&s.started_at)))
            .max();
        let stats = Stats::build(
            &public,
            as_of.unwrap_or(0),
            false,
            settings.stats_from,
            settings.sitting_floor,
        );
        Self {
            record: public,
            stats,
            as_of,
        }
    }
}

/// `book` with its cover pointing at the copy `kindles::cover` finds under
/// `record_dirs`, at none where no copy there draws, and no `thumbnail`.
fn on_this_computer(book: &record::Book, record_dirs: &[PathBuf]) -> record::Book {
    let cover = crate::kindles::cover(record_dirs, &book.cde_key)
        .map(|at| at.to_string_lossy().into_owned())
        .unwrap_or_default();
    record::Book {
        cover,
        thumbnail: String::new(),
        ..book.clone()
    }
}

/// The highlights shown, in the record's order, each held by the book
/// `Stats::build` hands it to over the whole record: a hidden book's highlight
/// never lands on a shown book sharing its title.
fn highlights_shown(
    record: &Record,
    edits: &Edits,
    settings: &Settings,
    new_books: NewBooks,
) -> Vec<Highlight> {
    let whole = Stats::build(
        record,
        0,
        false,
        settings.stats_from,
        settings.sitting_floor,
    );
    let mut kept = vec![false; whole.highlights.len()];
    for book in &whole.books {
        if !edits.book_shown(&book.cde_key, book.extent, new_books) {
            continue;
        }
        for &at in &book.highlights {
            kept[at] = edits.highlight_shown(&whole.highlights[at], &book.cde_key, book.extent);
        }
    }
    whole
        .highlights
        .iter()
        .zip(kept)
        .filter(|(_, kept)| *kept)
        .map(|(highlight, _)| highlight.clone())
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tests::{book, highlight, record_of, sitting};
    use readinglog_native::lang::Lang;
    use readinglog_native::settings::StatsFrom;

    fn settings() -> Settings {
        Settings::new(Lang::English)
    }

    #[test]
    fn every_book_shown_states_what_the_kindle_states() {
        let record = record_of(
            vec![book(100, "B1", "One"), book(200, "B2", "Two")],
            vec![
                sitting("2026-09-01T09:00:00", 100, 600),
                sitting("2026-09-02T09:00:00", 200, 1200),
                // `300` names no book: an unidentified sitting.
                sitting("2026-09-03T09:00:00", 300, 900),
            ],
        );
        let public = Public::of(
            &record,
            &[PathBuf::from("/nowhere")],
            &Edits::default(),
            &settings(),
            NewBooks::Shown,
        );
        let kindle = Stats::build(
            &record,
            public.as_of.unwrap(),
            false,
            StatsFrom::App,
            settings().sitting_floor,
        );
        assert_eq!(public.stats.total_seconds, kindle.total_seconds);
        assert_eq!(public.stats.days, kindle.days);
        assert_eq!(public.stats.books.len(), kindle.books.len());
        // `public.as_of` is the last shown sitting's day, before the unidentified one.
        assert_eq!(public.as_of, date::parse_day("2026-09-02"));
    }

    #[test]
    fn a_hidden_book_reaches_no_figure() {
        let record = record_of(
            vec![book(100, "B1", "One"), book(200, "B2", "Two")],
            vec![
                sitting("2026-09-01T09:00:00", 100, 600),
                sitting("2026-09-02T09:00:00", 200, 1200),
                sitting("2026-09-02T21:00:00", 100, 300),
            ],
        );
        let mut edits = Edits::default();
        edits.book_mut("B2", 200).shown = Some(false);
        let public = Public::of(
            &record,
            &[PathBuf::from("/nowhere")],
            &edits,
            &settings(),
            NewBooks::Shown,
        );
        assert_eq!(public.stats.books.len(), 1);
        assert_eq!(public.stats.total_seconds, 900);
        let day = date::parse_day("2026-09-02").unwrap();
        assert_eq!(public.stats.day_seconds(day), 300);
        assert_eq!(public.stats.hours_over(day..=day)[9], 0);

        // `B1`, hidden by default, shown by its own row.
        let mut edits = Edits::default();
        edits.book_mut("B1", 100).shown = Some(true);
        let public = Public::of(
            &record,
            &[PathBuf::from("/nowhere")],
            &edits,
            &settings(),
            NewBooks::Hidden,
        );
        assert_eq!(public.stats.total_seconds, 900);
    }

    #[test]
    fn a_hidden_sitting_leaves_its_book_standing() {
        let record = record_of(
            vec![book(100, "B1", "One")],
            vec![
                sitting("2026-09-01T09:00:00", 100, 600),
                sitting("2026-09-02T09:00:00", 100, 1200),
            ],
        );
        let mut edits = Edits::default();
        edits.sitting_mut(&record.sittings[1]).shown = Some(false);
        let public = Public::of(
            &record,
            &[PathBuf::from("/nowhere")],
            &edits,
            &settings(),
            NewBooks::Shown,
        );
        assert_eq!(public.stats.total_seconds, 600);
        assert_eq!(public.stats.books[0].sittings, 1);
        assert_eq!(public.as_of, date::parse_day("2026-09-01"));
    }

    #[test]
    fn a_highlight_shows_only_where_the_reader_shows_it() {
        let mut record = record_of(
            vec![book(100, "B1", "One"), book(200, "B2", "One")],
            vec![
                sitting("2026-09-01T09:00:00", 100, 600),
                sitting("2026-09-02T09:00:00", 200, 600),
            ],
        );
        // `B1` and `B2` share a title, `B2` hidden: its highlight is its own,
        // never `B1`'s.
        record.highlights = vec![
            highlight(100, "One", "2026-09-01T09:10:00"),
            highlight(200, "One", "2026-09-02T09:10:00"),
        ];
        let mut edits = Edits::default();
        edits.book_mut("B2", 200).shown = Some(false);
        let shown = |edits: &Edits| {
            Public::of(
                &record,
                &[PathBuf::from("/nowhere")],
                edits,
                &settings(),
                NewBooks::Shown,
            )
            .record
            .highlights
        };
        assert!(shown(&edits).is_empty());
        edits.book_mut("B2", 200).highlights = Some(true);
        assert!(shown(&edits).is_empty());
        edits.book_mut("B1", 100).highlights = Some(true);
        let both = shown(&edits);
        assert_eq!(both.len(), 1);
        assert_eq!(both[0].extent, 100);
        let first = record.highlights[0].clone();
        edits.highlight_mut(&first).shown = Some(false);
        assert!(shown(&edits).is_empty());
    }
}
