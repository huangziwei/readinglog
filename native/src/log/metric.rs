//! The `fastmetrics` records written beside the reading timer, for every book
//! including the ones the timer declines to count. Bracketed in the marker
//! strings: `ereader_open_book` prefixes `..._failure_backup`.

use super::line::{field_num, field_text};

/// What every one of [`METRIC_MARKERS`] opens with: the `fastmetrics` record
/// head, which is what says a line carries a JSON-ish body at all.
pub const SCHEMA: &str = "SchemaName[ereader_";

pub const METRIC_MARKERS: [&str; 8] = [
    "SchemaName[ereader_open_book]",
    "SchemaName[ereader_close_book]",
    "SchemaName[ereader_book_consume_content]",
    "SchemaName[ereader_book_page_turn]",
    "SchemaName[ereader_book_linear_page_actions]",
    "SchemaName[ereader_content_point]",
    "SchemaName[ereader_reader_latency_ops]",
    "SchemaName[ereader_reader_page_turn_latency_ops]",
];

/// What one record contributes to the run open around it. The records name no
/// book: they state a page and a turn for the run the reading-timer lines
/// track.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Metric {
    /// `ereader_book_consume_content`: a page, with the words on it and where
    /// it began. A redraw repeats `start`; a record stating none reads -1.
    Page { words: i64, start: i64 },
    /// A forward turn.
    Forward,
    /// A backward turn, which advances no reading.
    Back,
    /// `ereader_close_book`.
    Close,
}

/// Read a line as a `fastmetrics` record.
pub fn metric(line: &str) -> Option<Metric> {
    if line.contains(METRIC_MARKERS[2]) {
        return Some(Metric::Page {
            words: field_num(line, "words_count").unwrap_or(0),
            start: field_num(line, "start_position").unwrap_or(-1),
        });
    }
    if line.contains(METRIC_MARKERS[1]) {
        return Some(Metric::Close);
    }
    if line.contains(METRIC_MARKERS[3]) || line.contains(METRIC_MARKERS[4]) {
        // The two records carrying an `action_id`, one per reader stack.
        // `ereader_content_point` carries a `point_type` and no action.
        return match field_text(line, "action_id") {
            Some(a) if a.starts_with("Next") => Some(Metric::Forward),
            Some(a) if a.starts_with("Prev") => Some(Metric::Back),
            _ => None,
        };
    }
    None
}

/// The `cde_key` a record states for the book it is about, which the
/// reading-timer lines redact. The catalog's own `p_cdeKey`; `N/A` stands
/// for a book with no key.
pub fn cde_key(line: &str) -> Option<&str> {
    // `SCHEMA` heads every one of `METRIC_MARKERS`, in one search.
    if !line.contains(SCHEMA) {
        return None;
    }
    match field_text(line, "cde_key") {
        Some(k) if !k.is_empty() && k != "N/A" => Some(k),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// `cde_key` stands on [`SCHEMA`] alone, which is only sound while every
    /// marker is written under it.
    #[test]
    fn every_metric_marker_opens_with_the_record_head() {
        for marker in METRIC_MARKERS {
            assert!(marker.starts_with(SCHEMA), "{marker}");
        }
    }

    fn page(words: i64) -> String {
        format!(
            r#"260814:112035 fastmetrics[9842]: D fastmetrics:KindleFastMetricsPublisher:[1.0]: Emitting a new record. SchemaName[ereader_book_consume_content], Fields[{{ 	"context" : "Book:Reading:MainContent", 	"end_position" : 4133, 	"span_type" : "Text", 	"start_position" : 3227, 	"words_count" : {words} }} ]. :"#
        )
    }

    const TURN: &str = r#"260814:112040 fastmetrics[9842]: D fastmetrics:KindleFastMetricsPublisher:[1.0]: Emitting a new record. SchemaName[ereader_book_linear_page_actions], Fields[{ 	"action_id" : "NextPageWithSwipe", 	"context" : "Book:Reading:MainContent" } ]. :"#;

    const BACK: &str = r#"260814:112044 fastmetrics[9842]: D fastmetrics:KindleFastMetricsPublisher:[1.0]: Emitting a new record. SchemaName[ereader_book_page_turn], Fields[{ 	"action_id" : "PrevPageTurnWithGESTURE_TAP_SWIPES", 	"context" : "Book:Reading:MainContent" } ]. :"#;

    const POINT: &str = r#"260814:112042 fastmetrics[9690]: D fastmetrics:KindleFastMetricsPublisher:[4937.8]: Emitting a new record. SchemaName[ereader_content_point], Fields[{ 	"context" : "Book:Reading:MainContent", 	"point_type" : "ChapterStart", 	"position" : 4205 } ]. :"#;

    const LATENCY: &str = r#"260814:112035 fastmetrics[9842]: D fastmetrics: Emitting a new record. SchemaName[ereader_reader_latency_ops], Fields[{ 	"cde_key" : "B00OKPCRLG", 	"op_name" : "OpenBook" } ]. :"#;

    const NO_KEY: &str = r#"260814:112035 fastmetrics[9842]: D fastmetrics: Emitting a new record. SchemaName[ereader_reader_latency_ops], Fields[{ 	"cde_key" : "N/A", 	"op_name" : "OpenBook" } ]. :"#;

    #[test]
    fn a_page_record_carries_the_words_on_it_and_where_it_began() {
        let read = |w| metric(&page(w));
        assert_eq!(
            read(217),
            Some(Metric::Page {
                words: 217,
                start: 3227
            })
        );
        // `words_count` of a fixed-layout page.
        assert_eq!(
            read(0),
            Some(Metric::Page {
                words: 0,
                start: 3227
            })
        );
    }

    /// [`Metric::Forward`] and [`Metric::Back`] off either turn schema.
    #[test]
    fn a_turn_reads_its_direction_off_the_action_on_either_stack() {
        assert_eq!(metric(TURN), Some(Metric::Forward));
        assert_eq!(metric(BACK), Some(Metric::Back));
    }

    #[test]
    fn a_chapter_boundary_sitting_among_the_turns_is_not_one() {
        assert_eq!(metric(POINT), None);
    }

    #[test]
    fn a_record_that_is_not_one_of_the_eight_contributes_nothing() {
        assert_eq!(metric(LATENCY), None);
        assert_eq!(metric("260814:112035 cvm[1]: I something else"), None);
    }

    #[test]
    fn a_latency_record_names_the_book_the_timer_redacts() {
        assert_eq!(cde_key(LATENCY), Some("B00OKPCRLG"));
        assert_eq!(cde_key(NO_KEY), None);
        assert_eq!(
            cde_key("260814:112035 cvm[1]: I cde_key not a record"),
            None
        );
    }
}
