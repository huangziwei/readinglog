//! The `fastmetrics` records the reader shell writes beside the reading timer.
//!
//! The reading timer counts words and a WPM, and times only a book it can count
//! words in. These records come from the reader shell and are written for every
//! book: `ereader_book_consume_content` spans a page with its `words_count`,
//! `ereader_book_page_turn` and `ereader_book_linear_page_actions` name a turn,
//! `ereader_open_book` and `ereader_close_book` bracket a book, and
//! `ereader_reader_latency_ops` and `ereader_reader_page_turn_latency_ops`
//! carry a `cde_key`.
//!
//! Bracketed in the marker strings: `ereader_open_book` is a prefix of
//! `ereader_open_book_failure_backup`.

use super::line::{field_num, field_text};

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

/// What one record contributes to the run open around it.
///
/// The records name no book. They state a page and a turn for whatever the
/// reader had open, which is the run the reading-timer lines are already
/// tracking — and those lines still open and position a book the timer declines
/// to count.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Metric {
    /// `ereader_book_consume_content`: a page, with the words on it.
    Page { words: i64 },
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
        });
    }
    if line.contains(METRIC_MARKERS[1]) {
        return Some(Metric::Close);
    }
    if line.contains(METRIC_MARKERS[3]) || line.contains(METRIC_MARKERS[4]) {
        // The two records carrying an `action_id`, one per reader stack:
        // `ereader_book_page_turn` names `NextPageTurnWithSWIPE` and
        // `ereader_book_linear_page_actions` names `NextPageWithTap`. The
        // `ereader_content_point` beside them carries a `point_type` and no
        // action, being a chapter boundary rather than a turn.
        return match field_text(line, "action_id") {
            Some(a) if a.starts_with("Next") => Some(Metric::Forward),
            Some(a) if a.starts_with("Prev") => Some(Metric::Back),
            _ => None,
        };
    }
    None
}

/// The `cde_key` a reader-shell record states for the book it is about.
///
/// The reading-timer lines redact the book on every one of them
/// (`Title:<private>,Asin:<private>`), and these do not: the latency records
/// name it outright, one per open, per close and per page turn. It is the
/// catalog's own `p_cdeKey`, which makes it a second way into `cc.db` beside
/// the book's end position.
///
/// `N/A` is the reader shell's own filler for a book it has no key for.
pub fn cde_key(line: &str) -> Option<&str> {
    if !METRIC_MARKERS.iter().any(|m| line.contains(m)) {
        return None;
    }
    match field_text(line, "cde_key") {
        Some(k) if !k.is_empty() && k != "N/A" => Some(k),
        _ => None,
    }
}

/// The widest a page's dwell may run past what its words justify, and the
/// narrowest it may fall short. Below the floor the page was skipped past;
/// above the ceiling the reader was idle on it, and the ceiling is what counts.
const DWELL_FLOOR: f64 = 0.5;
const DWELL_CEILING: f64 = 1.5;

/// The WPM band inside which a rate is usable.
const WPM_MIN: f64 = 0.0;
const WPM_MAX: f64 = 500.0;

/// What a page with no usable rate may count, in seconds. A fixed-layout page
/// states no words, so no rate applies to it and only the dwell itself remains.
const WORDLESS_FLOOR: f64 = 3.0;
const WORDLESS_CEILING: f64 = 120.0;

/// How much of a page's dwell counts as reading, in milliseconds.
///
/// The device's own rule, from `PageHeuristicsImpl` in
/// `ReadingDataAggregatorService.jar` with the `KFTResources` defaults. Applied
/// verbatim: it has a defined answer for a page carrying no words, which is
/// exactly the content the reading timer refuses, and matching it keeps these
/// figures comparable with the ones the device shows for itself.
pub fn dwell_ms(wpm: Option<f64>, words: i64, dwell_ms: i64) -> i64 {
    let secs = dwell_ms as f64 / 1000.0;
    match wpm {
        Some(wpm) if wpm > WPM_MIN && wpm < WPM_MAX && words > 0 => {
            let expected = words as f64 / (wpm / 60.0);
            if secs < DWELL_FLOOR * expected {
                0
            } else {
                (secs.min(DWELL_CEILING * expected) * 1000.0) as i64
            }
        }
        _ if secs < WORDLESS_FLOOR => 0,
        _ => (secs.min(WORDLESS_CEILING) * 1000.0) as i64,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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
    fn a_page_record_carries_the_words_on_it() {
        assert_eq!(metric(&page(217)), Some(Metric::Page { words: 217 }));
        // A fixed-layout page states none, and zero is the answer, not absence.
        assert_eq!(metric(&page(0)), Some(Metric::Page { words: 0 }));
    }

    /// Two schemas carry a turn, one per reader stack, and a device that writes
    /// one writes none of the other.
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

    #[test]
    fn a_page_read_at_about_its_own_rate_counts_whole() {
        // 200 words at 200 wpm is a 60 s page; 55 s sits inside the band.
        assert_eq!(dwell_ms(Some(200.0), 200, 55_000), 55_000);
    }

    #[test]
    fn a_page_skipped_past_counts_nothing_and_one_idled_on_counts_its_ceiling() {
        // Under half of the 60 s the words justify.
        assert_eq!(dwell_ms(Some(200.0), 200, 20_000), 0);
        // Over 1.5x it, so 90 s is what counts of the 10 minutes.
        assert_eq!(dwell_ms(Some(200.0), 200, 600_000), 90_000);
    }

    #[test]
    fn a_page_with_no_rate_falls_back_to_its_own_floor_and_ceiling() {
        assert_eq!(dwell_ms(None, 0, 2_000), 0);
        assert_eq!(dwell_ms(None, 0, 40_000), 40_000);
        assert_eq!(dwell_ms(None, 0, 600_000), 120_000);
        // A rate outside the band is no rate.
        assert_eq!(dwell_ms(Some(900.0), 200, 40_000), 40_000);
    }
}
