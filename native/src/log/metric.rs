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
    // The head every one of `METRIC_MARKERS` is written under, in one search
    // rather than eight. This runs on every line of a whole syslog and, on the
    // stacks that state no key at all, finds nothing every time.
    if !line.contains(SCHEMA) {
        return None;
    }
    match field_text(line, "cde_key") {
        Some(k) if !k.is_empty() && k != "N/A" => Some(k),
        _ => None,
    }
}

/// A page open for less than this is navigation, not reading, whatever its
/// words say.
const FLOOR_SECS: f64 = 3.0;

/// A page may credit this many times what its words justify, never less than
/// [`CAP_SECS`]: a page of one word and a diagram would buy nothing.
const PAGE_CEILING: f64 = 1.5;

/// The ceiling where the words or the rate justify less.
/// `PageHeuristicsImpl` holds this, [`PAGE_CEILING`] and [`FLOOR_SECS`].
const CAP_SECS: f64 = 120.0;

/// A derived rate under this is not real, and would buy a page a ceiling of
/// many minutes. The firmware's matching upper bound is not applied here.
const WPM_MIN: f64 = 40.0;

/// How much of a page's open time counts as reading, in milliseconds. The
/// floor is flat and never scales with the rate: the firmware caps that rate
/// at 900, and a faster reader would have every page refused.
pub fn page_ms(wpm: Option<f64>, words: i64, open_ms: i64) -> i64 {
    let secs = open_ms as f64 / 1000.0;
    if secs < FLOOR_SECS {
        return 0;
    }
    let ceiling = match wpm {
        Some(wpm) if wpm > WPM_MIN && words > 0 => {
            (PAGE_CEILING * (words as f64 / (wpm / 60.0))).max(CAP_SECS)
        }
        _ => CAP_SECS,
    };
    (secs.min(ceiling) * 1000.0) as i64
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
        assert_eq!(page_ms(Some(200.0), 200, 55_000), 55_000);
    }

    #[test]
    fn a_page_idled_on_counts_only_its_ceiling() {
        // 600 words at 200 wpm is a 3-minute page; 1.5x it is 4m30s, and that
        // is what counts of the ten minutes it stood open.
        assert_eq!(page_ms(Some(200.0), 600, 600_000), 270_000);
    }

    #[test]
    fn a_page_of_one_word_is_not_held_to_what_one_word_justifies() {
        // 1.5x what one word justifies at 200 wpm is under half a second.
        assert_eq!(page_ms(Some(200.0), 1, 300_000), 120_000);
        assert_eq!(page_ms(Some(200.0), 200, 600_000), 120_000);
    }

    #[test]
    fn a_page_read_far_faster_than_its_stated_rate_still_counts() {
        // 200 words at 200 wpm is a 60 s page; 20 s is three times that rate.
        assert_eq!(page_ms(Some(200.0), 200, 20_000), 20_000);
        // And a page swiped past counts nothing, rate or no rate.
        assert_eq!(page_ms(Some(200.0), 200, 2_000), 0);
        assert_eq!(page_ms(None, 0, 2_000), 0);
    }

    #[test]
    fn a_page_with_no_rate_falls_back_to_its_own_cap() {
        assert_eq!(page_ms(None, 0, 40_000), 40_000);
        assert_eq!(page_ms(None, 0, 600_000), 120_000);
        // A rate the firmware would refuse for being too fast is used here.
        assert_eq!(page_ms(Some(1800.0), 200, 40_000), 40_000);
        // One too slow to be real is not: 250 words at 10 wpm is 25 minutes.
        assert_eq!(page_ms(Some(10.0), 250, 600_000), 120_000);
    }
}
