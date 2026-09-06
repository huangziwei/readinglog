//! Reading one syslog line. The reading-timer lines are a flat `key:value,`
//! list inside `;`-terminated payloads, each headed by an event name. The
//! `fastmetrics` records beside them carry a JSON-ish body.

use crate::date;

/// The tag every reading-timer line carries.
pub const TIMER_MARKER: &str = "ReadingTimerController";

/// One stamped moment in the log.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Moment {
    /// `YYYY-MM-DD`, the day the line fell on.
    pub day: String,
    /// Seconds into `day`.
    pub secs: i64,
    /// The same instant as one running count of seconds.
    pub abs: i64,
    /// `YYYY-MM-DDTHH:MM:SS` — the form a session stores.
    pub at: String,
}

/// `YYMMDD:HHMMSS` at the start of a syslog line.
pub fn stamp(line: &str) -> Option<Moment> {
    let raw = line.as_bytes();
    if raw.len() < 13 || raw[6] != b':' || !line[..6].bytes().all(|c| c.is_ascii_digit()) {
        return None;
    }
    let clock = &line[7..13];
    if !clock.bytes().all(|c| c.is_ascii_digit()) {
        return None;
    }
    let (y, mo, d) = (&line[0..2], &line[2..4], &line[4..6]);
    let (h, mi, s) = (&clock[0..2], &clock[2..4], &clock[4..6]);
    let (y, mo, d): (i64, i64, i64) = (
        2000 + y.parse::<i64>().ok()?,
        mo.parse().ok()?,
        d.parse().ok()?,
    );
    if !date::is_valid(y, mo, d) {
        return None;
    }
    let secs =
        h.parse::<i64>().ok()? * 3600 + mi.parse::<i64>().ok()? * 60 + s.parse::<i64>().ok()?;
    Some(Moment {
        day: format!("{y:04}-{mo:02}-{d:02}"),
        secs,
        abs: date::days_from_civil(y, mo, d) * 86_400 + secs,
        at: format!("{y:04}-{mo:02}-{d:02}T{h}:{mi}:{s}"),
    })
}

/// The `YYMMDD:HHMMSS` a line begins with, or `None`. The form a watermark
/// travels in: log prefixes and dump filenames are both this shape, and every
/// comparison is string ordering with no date arithmetic.
pub fn line_stamp(line: &str) -> Option<&str> {
    stamp(line).map(|_| &line[..13])
}

/// `YYYY-MM-DDTHH:MM:SS` back to the `YYMMDD:HHMMSS` a syslog line starts with.
pub fn log_stamp(iso: &str) -> Option<String> {
    let b = iso.as_bytes();
    if b.len() < 19 || b[4] != b'-' || b[7] != b'-' || b[10] != b'T' || b[13] != b':' {
        return None;
    }
    let out = format!(
        "{}{}{}:{}{}{}",
        &iso[2..4],
        &iso[5..7],
        &iso[8..10],
        &iso[11..13],
        &iso[14..16],
        &iso[17..19]
    );
    out.bytes()
        .all(|c| c.is_ascii_digit() || c == b':')
        .then_some(out)
}

/// A whole-number field of a reading-timer payload. The name must start a
/// field — the line's separator or a `,`. `Time` does not match inside
/// `IntervalTime`.
pub fn field(line: &str, name: &str) -> Option<i64> {
    let needle = format!("{name}:");
    let bytes = line.as_bytes();
    let at = line.match_indices(&needle).find_map(|(at, _)| {
        let before = at.checked_sub(1).map(|i| bytes[i]);
        matches!(before, None | Some(b',') | Some(b':')).then_some(at + needle.len())
    })?;
    let rest = &line[at..];
    let end = rest
        .find(|c: char| !c.is_ascii_digit())
        .unwrap_or(rest.len());
    rest[..end].parse().ok()
}

/// The payloads a line carries, each `<Event>,<fields>` with the event name
/// possibly missing. Fields must be read from one payload: reading across pairs
/// one event's counter with another's book.
pub fn payloads(line: &str) -> impl Iterator<Item = &str> {
    line.split_once("Information::")
        .map_or("", |(_, rest)| rest)
        .split(';')
        .filter(|p| !p.is_empty())
}

/// Each place `text` states a value for `name`, in order. `name` carries the
/// `:` ending it and starts a field: the byte before it is a `,`, a `:`, or
/// nothing.
fn fields<'a>(text: &'a str, name: &'static str) -> impl Iterator<Item = usize> + 'a {
    let bytes = text.as_bytes();
    text.match_indices(name).filter_map(move |(at, _)| {
        let before = at.checked_sub(1).map(|i| bytes[i]);
        matches!(before, None | Some(b',') | Some(b':')).then_some(at + name.len())
    })
}

/// The `kind` and the place a position field states, `at` past its `<name>:`.
/// `value` ends at the next `,` or `;`. Shapes: `YJPosition:<token>:<n>`,
/// `HTMLPosition:<n>`, `MobiPosition:<n>`, `MobiSerializedPosition:<n> <n>`.
fn stated(text: &str, at: usize) -> Option<(&str, i64)> {
    let rest = &text[at..];
    let value = &rest[..rest.find([',', ';']).unwrap_or(rest.len())];
    let (kind, tail) = value.split_once(':')?;
    if !kind.ends_with("Position") {
        return None;
    }
    // `from`..`end` is the last digit run of `tail`. It opens `tail` or follows
    // a `:` or a ` `.
    let end = tail.rfind(|c: char| c.is_ascii_digit())? + 1;
    let from = tail[..end]
        .rfind(|c: char| !c.is_ascii_digit())
        .map_or(0, |i| i + 1);
    matches!(tail[..from].chars().next_back(), None | Some(':' | ' '))
        .then(|| tail[from..end].parse().ok())
        .flatten()
        .map(|position| (kind, position))
}

/// The book's own end position within one payload: the **last** `EndPos` ahead
/// of the `NextTOCEntry` group, and the first where `payload` carries no group.
/// The `EndPos` inside that group is a chapter boundary.
pub fn end_position(payload: &str) -> Option<i64> {
    let (ahead, group) = match payload.find("NextTOCEntry") {
        Some(toc) => (&payload[..toc], true),
        None => (payload, false),
    };
    let mut found = fields(ahead, "EndPos:");
    let at = match group {
        true => found.last()?,
        false => found.next()?,
    };
    stated(ahead, at).map(|(_, position)| position)
}

/// The book a whole line is about, from whichever of its payloads names one.
pub fn book_position(line: &str) -> Option<i64> {
    payloads(line).find_map(end_position)
}

/// Every `NextTOCEntryPosition` `line` states, and the [`end_position`] of each
/// payload stating one.
pub fn toc_and_book(line: &str) -> (Vec<i64>, Vec<i64>) {
    let mut toc = Vec::new();
    let mut book = Vec::new();
    for payload in payloads(line) {
        if !payload.contains("NextTOCEntry") {
            continue;
        }
        toc.extend(fields(payload, "NextTOCEntryPosition:").filter_map(|at| {
            let (_, position) = stated(payload, at)?;
            Some(position)
        }));
        book.extend(end_position(payload));
    }
    (toc, book)
}

/// True when `line` names `event` as `<sep><Event>,`, `<sep>` being the
/// `Information::` prefix or the `;` ending the payload before it.
pub fn names(line: &str, event: &str) -> bool {
    let bytes = line.as_bytes();
    line.match_indices(event).any(|(at, _)| {
        let before = at.checked_sub(1).map(|i| bytes[i]);
        let after = bytes.get(at + event.len()).copied();
        matches!(before, Some(b':') | Some(b';')) && matches!(after, None | Some(b',') | Some(b';'))
    })
}

/// What one line says about the book it is on.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Observation {
    /// The book's own end position: this line's fingerprint for its book.
    pub position: i64,
    /// The book's running reading counter, in milliseconds.
    pub total_ms: Option<i64>,
    pub words: Option<i64>,
    pub page_turn: bool,
    pub closes: bool,
}

/// Read a line as an observation of some book's reading counter, or `None`. A
/// line qualifies on what it carries — a counter beside an end position — not
/// on a named event, which a mangled payload loses while keeping its fields.
pub fn observation(line: &str) -> Option<Observation> {
    let page_turn = names(line, "NextPage");
    let closes = names(line, "CloseBook");
    // A named page event with no counter marks a turn. `TotalTime` is absent
    // from the uncredited ones.
    let named = page_turn || closes || names(line, "PreviousPage") || names(line, "GoToPosition");
    // The payload holding the counter, or — for those uncredited events — any
    // that at least says which book.
    let chosen = payloads(line)
        .find(|p| field(p, "TotalTime").is_some() && end_position(p).is_some())
        .or_else(|| {
            named
                .then(|| payloads(line).find(|p| end_position(p).is_some()))
                .flatten()
        })
        // A payload carrying `CurrentPos` and an end position, with no
        // `TotalTime` and no name — the whole record of an untimed sitting.
        .or_else(|| {
            payloads(line).find(|p| {
                end_position(p).is_some()
                    && fields(p, "CurrentPos:").any(|at| stated(p, at).is_some())
            })
        })?;
    Some(Observation {
        position: end_position(chosen)?,
        total_ms: field(chosen, "TotalTime"),
        words: field(chosen, "TotalWords"),
        page_turn,
        closes,
    })
}

/// A book's reading counter when it was opened, in milliseconds, from an
/// `OpenBook` line's `StoredBookData`. `TimeRead:9,229 sec.` is whole seconds,
/// thousands-separated; `null` is a counter of zero, not an absent one.
pub fn opened_at_counter(line: &str) -> Option<i64> {
    let rest = line.split_once("StoredBookData:")?.1;
    if rest.starts_with("null") {
        return Some(0);
    }
    let digits = rest.strip_prefix("TimeRead:")?;
    let end = digits
        .find(|c: char| !c.is_ascii_digit() && c != ',')
        .unwrap_or(digits.len());
    digits[..end]
        .replace(',', "")
        .parse::<i64>()
        .ok()
        .map(|s| s * 1000)
}

/// Read `"<name>" : "<value>"` out of a metrics record's JSON-ish body.
pub fn field_text<'a>(line: &'a str, name: &str) -> Option<&'a str> {
    let at = line.find(&format!("\"{name}\""))? + name.len() + 2;
    let rest = &line[at..];
    let tail = &rest[rest.find('"')? + 1..];
    Some(&tail[..tail.find('"')?])
}

/// Read `"<name>" : <number>` out of the same body. Distinct from
/// [`field_text`], which reads past an unquoted value into the next field.
pub fn field_num(line: &str, name: &str) -> Option<i64> {
    let at = line.find(&format!("\"{name}\" : "))? + name.len() + 5;
    let rest = &line[at..];
    let end = rest
        .find(|c: char| !c.is_ascii_digit() && c != '-')
        .unwrap_or(rest.len());
    rest[..end].parse().ok()
}

/// The `BookEndPosition.FromBook` `line` states, as `p_contentSize` holds it.
/// An `HTMLPosition` book end is `p_contentSize - 1`; every other class states
/// `p_contentSize`.
pub fn from_book(line: &str) -> Option<i64> {
    let (kind, position) =
        fields(line, "BookEndPosition.FromBook:").find_map(|at| stated(line, at))?;
    Some(position + i64::from(kind == "HTMLPosition"))
}

/// Map each book's per-line `EndPos` fingerprint to its [`from_book`].
/// `pending` drops before an `OpenBook` and after a `CloseBook`.
pub fn frombook_map<'a>(events: impl IntoIterator<Item = &'a str>) -> Vec<(i64, i64)> {
    let mut map: Vec<(i64, i64)> = Vec::new();
    let mut pending: Option<i64> = None;
    for line in events {
        if names(line, "OpenBook") {
            pending = None;
        }
        if let Some(stated) = from_book(line) {
            pending = Some(stated);
        }
        if let (Some(from_book), Some(ep)) = (pending, book_position(line))
            && !map.iter().any(|(k, _)| *k == ep)
        {
            map.push((ep, from_book));
        }
        if names(line, "CloseBook") {
            pending = None;
        }
    }
    map
}

#[cfg(test)]
mod tests {
    use super::*;

    const PAGE: &str = "260807:101501 cvm[6144]: I ReadingTimerController:Information::NextPage,Verdict:Processed,PageStartPos:YJPosition: AfQJAAAAAAAA:54205,IntervalTime:39890,IntervalWords:320,TotalTime:7390020,TotalWords:49583,CurrentPos:YJPosition: AfQJAAAAAAAA:54205,EndPos:YJPosition: AbcVAAAPAAAA:148207,PosLeft:94002,NextTOCEntryPosition:YJPosition: AT4KAAAAAAAA:56499,NextTOCEntryLength:10,CurrentPos:YJPosition: AfQJAAAAAAAA:54205,EndPos:YJPosition: AT4KAAAAAAAA:56499,PosLeft:2294;";

    /// The `PAGE` event as `HTMLPosition` states it.
    const MOBI8: &str = "260906:192404 cvm[6144]: I ReadingTimerController:Information::NextPage,Verdict:Processed,PageStartPos:HTMLPosition:7731097,IntervalTime:785,IntervalWords:12,TotalTime:329785,TotalWords:1905,CurrentPos:HTMLPosition:7731097,EndPos:HTMLPosition:19886489,PosLeft:12155392,%Left:0.6112,NextTOCEntryPosition:HTMLPosition:7800000,NextTOCEntryLength:10,CurrentPos:HTMLPosition:7731097,EndPos:HTMLPosition:7800000,PosLeft:68903;";

    #[test]
    fn a_stamp_reads_its_day_and_its_clock() {
        let m = stamp(PAGE).expect("a stamped line");
        assert_eq!(m.day, "2026-08-07");
        assert_eq!(m.at, "2026-08-07T10:15:01");
        assert_eq!(m.secs, 10 * 3600 + 15 * 60 + 1);
        assert_eq!(m.abs, date::days_from_civil(2026, 8, 7) * 86_400 + m.secs);
    }

    #[test]
    fn a_prefix_naming_no_day_is_not_a_stamp() {
        // Six digits and a colon, but 2026-02-30 is not a day.
        assert!(stamp("260230:101501 cvm[1]: I x").is_none());
        assert!(stamp("not a log line").is_none());
        assert!(stamp("26080").is_none());
    }

    #[test]
    fn a_stamp_round_trips_through_the_stored_form() {
        let m = stamp(PAGE).unwrap();
        assert_eq!(log_stamp(&m.at).as_deref(), Some("260807:101501"));
        assert_eq!(line_stamp(PAGE), Some("260807:101501"));
        // And back again, through the shape a line carries.
        assert_eq!(stamp(&format!("{} x", log_stamp(&m.at).unwrap())), Some(m));
    }

    #[test]
    fn a_field_must_start_where_its_name_does() {
        // `IntervalTime` also ends in `Time`, and `Time` must not match it.
        assert_eq!(field(PAGE, "TotalTime"), Some(7_390_020));
        assert_eq!(field(PAGE, "IntervalTime"), Some(39_890));
        assert_eq!(field(PAGE, "TotalWords"), Some(49_583));
        assert_eq!(field(PAGE, "NoSuchField"), None);
    }

    #[test]
    fn the_book_position_is_the_one_ahead_of_the_toc_group() {
        // 148207 leads the NextTOCEntry group; 56499 is the chapter's.
        assert_eq!(book_position(PAGE), Some(148_207));
    }

    #[test]
    fn an_event_name_needs_its_separator() {
        assert!(names(PAGE, "NextPage"));
        assert!(!names(PAGE, "CloseBook"));
        // `PageStartPos` contains `Page` but does not name it.
        assert!(!names(PAGE, "Page"));
    }

    #[test]
    fn a_page_line_observes_its_book_and_its_counter() {
        let obs = observation(PAGE).expect("a page event");
        assert_eq!(obs.position, 148_207);
        assert_eq!(obs.total_ms, Some(7_390_020));
        assert_eq!(obs.words, Some(49_583));
        assert!(obs.page_turn);
        assert!(!obs.closes);
    }

    #[test]
    fn an_open_states_the_counter_it_resumes_from() {
        let null = "260811:072945 java[1]: I ReadingTimerController:Information::OpenBook,CurrentVersionUsed:0,StoredBookData:null,Title:<private>;";
        assert_eq!(opened_at_counter(null), Some(0));
        let read = "260811:072945 java[1]: I ReadingTimerController:Information::OpenBook,StoredBookData:TimeRead:9,229 sec. WPM:0. Version:0,Title:<private>;";
        assert_eq!(opened_at_counter(read), Some(9_229_000));
        assert_eq!(opened_at_counter(PAGE), None);
    }

    #[test]
    fn a_metrics_body_gives_up_quoted_and_unquoted_values() {
        let rec = "260814:111900 fastmetrics[1]: D fastmetrics: SchemaName[ereader_book_consume_content], Fields[{ \t\"context\" : \"Book:Reading\", \t\"words_count\" : 217, \t\"span_type\" : \"Text\" } ]. :";
        assert_eq!(field_text(rec, "context"), Some("Book:Reading"));
        assert_eq!(field_num(rec, "words_count"), Some(217));
        // `field_text` runs into the next field on an unquoted value.
        assert_eq!(field_num(rec, "context"), None);
    }

    #[test]
    fn a_book_end_maps_its_last_word_position_to_the_catalog_number() {
        let open = "260811:072945 java[1]: I ReadingTimerController:Information::OpenBook,StoredBookData:null;";
        let info = "260811:072948 java[1]: I ReadingTimerController:Information::BookEndPosition.FromBook:YJPosition: AZI/AAAAAAAA:938018,BookEndPosition.LastWordPos.override:YJPosition: Aag/AACDAQAA:938016,CurrentPos:YJPosition: AWUDAAAAAAAA:2,EndPos:YJPosition: Aag/AACDAQAA:938016,PosLeft:938014;";
        assert_eq!(frombook_map([open, info]), vec![(938_016, 938_018)]);
        assert_eq!(from_book(info), Some(938_018));
        assert_eq!(from_book(PAGE), None);
        // An open with no BookEndPosition of its own inherits nothing.
        assert_eq!(frombook_map([info, open, PAGE]), vec![(938_016, 938_018)]);
    }

    #[test]
    fn a_close_ends_the_reach_of_the_book_end_it_states() {
        let info = "260811:072948 java[1]: I ReadingTimerController:Information::BookEndPosition.FromBook:YJPosition: AZI/AAAAAAAA:938018,CurrentPos:YJPosition: AWUDAAAAAAAA:2,EndPos:YJPosition: Aag/AACDAQAA:938016,PosLeft:938014;";
        let close = "260811:073010 java[1]: I ReadingTimerController:Information::CloseBook,CurrentPos:YJPosition: AWUDAAAAAAAA:2,EndPos:YJPosition: Aag/AACDAQAA:938016,PosLeft:938014;";
        // The close states the closing book's own position and is paired.
        assert_eq!(frombook_map([info, close]), vec![(938_016, 938_018)]);
        // `PAGE` is another book at 148207, and takes nothing from the close.
        assert_eq!(
            frombook_map([info, close, PAGE]),
            vec![(938_016, 938_018)],
            "148207 reached 938018 across a close"
        );
    }

    #[test]
    fn a_mobi8_page_line_observes_its_book_and_its_counter() {
        // 19886489 leads the NextTOCEntry group; 7800000 is the chapter's.
        assert_eq!(book_position(MOBI8), Some(19_886_489));
        let obs = observation(MOBI8).expect("a page event");
        assert_eq!(obs.position, 19_886_489);
        assert_eq!(obs.total_ms, Some(329_785));
        assert_eq!(obs.words, Some(1_905));
        assert!(obs.page_turn);
    }

    #[test]
    fn a_mobi8_book_end_is_raised_to_the_extent_the_catalog_states() {
        let open = "260906:192401 java[1]: I ReadingTimerController:Information::OpenBook,StoredBookData:TimeRead:329 sec. WPM:0. Version:0,Title:<private>;";
        let info = "260906:192402 java[1]: I ReadingTimerController:Information::BookEndPosition.FromBook:HTMLPosition:19886521,BookEndPosition.LastWordPos.override:HTMLPosition:19886489,CurrentPos:HTMLPosition:7731097,EndPos:HTMLPosition:19886489,PosLeft:12155392;";
        // `FromBook` 19886521 against `p_contentSize` 19886522.
        assert_eq!(from_book(info), Some(19_886_522));
        assert_eq!(
            frombook_map([open, info, MOBI8]),
            vec![(19_886_489, 19_886_522)]
        );
    }

    #[test]
    fn a_place_read_back_out_of_a_sidecar_states_itself_last() {
        let close = "260906:193015 java[1]: I ReadingTimerController:Information::CloseBook,CurrentPos:MobiSerializedPosition:12 4507,EndPos:MobiSerializedPosition:9 938016,PosLeft:933509;";
        assert_eq!(book_position(close), Some(938_016));
        // `EndPos:938016` names no class.
        assert_eq!(
            book_position(
                "260906:193015 java[1]: I ReadingTimerController:Information::CloseBook,EndPos:938016,PosLeft:0;"
            ),
            None
        );
    }

    #[test]
    fn an_untimed_sitting_is_read_whatever_stack_wrote_it() {
        let line = "260906:192500 java[1]: I ReadingTimerController:Information::CurrentPos:HTMLPosition:7731097,EndPos:HTMLPosition:19886489,PosLeft:12155392;";
        let obs = observation(line).expect("a book and a place");
        assert_eq!(obs.position, 19_886_489);
        assert_eq!(obs.total_ms, None);
        assert!(!obs.page_turn);
    }
}
