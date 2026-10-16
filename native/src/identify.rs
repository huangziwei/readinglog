//! Identifying a sitting's book from `vocab.db`, `My Clippings.txt` and the `.sdr`
//! directories, where the library cannot. Each writes the rows the library
//! writes: `Record::slot_for` reaches them unchanged.
use std::path::Path;

use crate::clippings;
use crate::record::{IdentifiedBy, Record, Sources};
use crate::sidecar;
use crate::vocab;

/// One source's statement that a book was open at an instant.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct Witness {
    /// `YYYY-MM-DDTHH:MM:SS` on the Kindle's own clock, as a sitting stores
    /// its own.
    pub at: String,
    /// The book's title, as the source states it. This is what
    /// two claims on one end position have to agree on.
    pub title: String,
    /// The authors, a joined list whose separator differs by reader
    /// stack. Corroboration, never a key.
    pub author: String,
    /// The content key, where the source states one. Empty where it states none.
    pub key: String,
    /// A position on the `extent` axis, where the source states one. Negative
    /// where it states none. A witness past its end position is refused.
    pub pos: i64,
}

/// `title` as two claims are compared on: trimmed, its whitespace collapsed,
/// and case-folded.
pub fn normalise(title: &str) -> String {
    title
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .to_lowercase()
}

/// What one pass over the sources came to.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct Identified {
    /// Witnesses each source produced. Zero where it was not read at all.
    pub lookups: usize,
    pub clippings: usize,
    pub sidecars: usize,
    /// End positions each source identified.
    pub by_vocab: usize,
    pub by_clippings: usize,
    pub by_sidecars: usize,
    /// End positions two sources named two books for, which stay unidentified on purpose.
    pub contested: usize,
    /// End positions wanting a book once the last source had spoken.
    pub unidentified: usize,
}

impl Identified {
    /// End positions the three sources identified between them.
    pub fn found(&self) -> usize {
        self.by_vocab + self.by_clippings + self.by_sidecars
    }
}

/// Ask every source that has something to say, strongest first. Only the
/// end positions `Record::remember` left unidentified are on offer.
pub fn from_sources(
    record: &mut Record,
    clips: &[clippings::Clipping],
    sidecars: &sidecar::Sidecars,
) -> Identified {
    from_sources_at(record, Path::new(vocab::VOCAB_DB), clips, sidecars)
}

/// Every sidecar under `documents`, which both this module and
/// [`crate::highlights`] stand on: the frequent half where `counters`, the
/// rare half where `rosters` names the folder its rosters are kept in.
pub fn walk(documents: &Path, counters: bool, rosters: Option<&Path>) -> sidecar::Sidecars {
    sidecar::read(documents, counters, rosters)
}

/// The rules an identifying pass reads its sources under. A `g` row an older build
/// wrote states a lower number, and the sources are asked again.
pub const IDENTIFY_RULES: u32 = 1;

/// What the sources stand at. [`Sources`] carries what this pass reads
/// and nothing else: a pass finding the same again writes nothing new.
pub fn gate(record: &Record, db: &Path, clips: &Path, survey: &sidecar::Survey) -> Sources {
    let (vocab_len, vocab_mtime) = stat(db);
    let (clips_len, clips_mtime) = stat(clips);
    Sources {
        vocab_len,
        vocab_mtime,
        clips_len,
        clips_mtime,
        sidecars: survey.stamp,
        dirs: survey.dirs,
        record: record.identify_stamp(),
        rules: IDENTIFY_RULES,
    }
}

/// A file's length and modification time, both zero where it is not there.
fn stat(at: &Path) -> (u64, i64) {
    let Ok(held) = std::fs::metadata(at) else {
        return (0, 0);
    };
    (
        held.len(),
        held.modified()
            .ok()
            .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
            .map_or(0, |d| d.as_secs() as i64),
    )
}

/// Whether the identifying pass and the highlights pass are worth making, taken
/// before a sidecar is opened. `None` and `false` mean both gates hold and the
/// sidecars need not be parsed at all.
pub fn asked(
    record: &Record,
    db: &Path,
    clips: &Path,
    documents: &Path,
) -> (sidecar::Survey, Option<Sources>, bool) {
    let survey = sidecar::survey(documents);
    let now = gate(record, db, clips, &survey);
    let identifying = (record.sources != Some(now)).then_some(now);
    let highlights = crate::highlights::wants(record, clips, &survey);
    (survey, identifying, highlights)
}

/// The sidecars, parsed, for a pass that has something to do. The frequent half
/// is [`Record::recover`]'s alone and only while an end position is unidentified;
/// the rare half is the highlights pass's, kept in `rosters`.
pub fn sidecars_for(
    record: &Record,
    documents: &Path,
    identifying: bool,
    rosters: Option<&Path>,
) -> sidecar::Sidecars {
    walk(
        documents,
        identifying && record.wants_identifying(IdentifiedBy::Sidecar),
        rosters,
    )
}

/// [`from_sources`] over the three sources named, `clips` the parsed
/// `My Clippings.txt` that [`crate::highlights::fold`] reads too.
pub fn from_sources_at(
    record: &mut Record,
    db: &Path,
    clips: &[clippings::Clipping],
    sidecars: &sidecar::Sidecars,
) -> Identified {
    let mut out = Identified::default();
    // An end position a stronger source found two titles for holds two books, and the
    // next source down having seen only one of them does not settle it.
    let mut contested: Vec<i64> = Vec::new();

    if record.wants_identifying(IdentifiedBy::Vocab) {
        let lookups = vocab::read_from(db, &record.clock);
        out.lookups = lookups.len();
        let said: Vec<Witness> = lookups.iter().map(witness_of_lookup).collect();
        out.by_vocab = record.identify_from(&said, IdentifiedBy::Vocab, &mut contested);
    }

    if record.wants_identifying(IdentifiedBy::Clippings) {
        out.clippings = clips.len();
        let said: Vec<Witness> = clips.iter().map(witness_of_clipping).collect();
        out.by_clippings = record.identify_from(&said, IdentifiedBy::Clippings, &mut contested);
    }
    out.contested = contested.len();

    // The counters name only an end position nothing else could, and `recover`
    // runs over an empty list all the same: it is also what drops a pairing the record has
    // since contradicted.
    let counters: &[sidecar::Counter] = match record.wants_identifying(IdentifiedBy::Sidecar) {
        true => &sidecars.counters,
        false => &[],
    };
    out.sidecars = counters.len();
    out.by_sidecars = record.recover(counters);
    record.note_timers(&sidecars.counters);

    out.unidentified = record.end_positions_wanting(IdentifiedBy::Sidecar).len();
    out
}

/// A dictionary lookup as a witness. `BOOK_INFO` states the content key and
/// `LOOKUPS.pos` a real position: this is the only source whose claim can
/// be refused for landing past the book's end.
fn witness_of_lookup(lookup: &vocab::Lookup) -> Witness {
    Witness {
        at: lookup.at.clone(),
        title: lookup.title.clone(),
        author: lookup.author.clone(),
        key: lookup.key.clone(),
        pos: lookup.pos,
    }
}

/// A clipping as a witness. The label's number is a display location and not a
/// position: nothing here goes in `pos`.
fn witness_of_clipping(clip: &clippings::Clipping) -> Witness {
    Witness {
        at: clip.at.clone(),
        title: clip.title.clone(),
        author: clip.author.clone(),
        key: String::new(),
        pos: -1,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn two_titles_are_one_claim_when_only_their_spacing_differs() {
        assert_eq!(normalise("  The   Long Way "), "the long way");
        assert_eq!(normalise("THE LONG WAY"), normalise("the long way"));
        assert_eq!(normalise("無職転生\u{3000}19"), "無職転生 19");
    }

    #[test]
    fn the_sources_are_ranked_cheapest_and_best_first() {
        let mut ranked = [
            IdentifiedBy::Sidecar,
            IdentifiedBy::Clippings,
            IdentifiedBy::Library,
            IdentifiedBy::Vocab,
        ];
        ranked.sort();
        assert_eq!(
            ranked,
            [
                IdentifiedBy::Library,
                IdentifiedBy::Vocab,
                IdentifiedBy::Clippings,
                IdentifiedBy::Sidecar
            ]
        );
    }
    /// A scratch `documents` tree with one `.sdr` in it.
    fn documents_at(tag: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!("readinglog-identify-{tag}"));
        let _ = std::fs::remove_dir_all(&dir);
        let sdr = dir.join("A Book.sdr");
        std::fs::create_dir_all(&sdr).expect("a scratch sidecar directory");
        std::fs::write(sdr.join("A Book.yjr"), b"rare").expect("a rare sidecar");
        std::fs::write(sdr.join("A Book.yjf"), b"frequent").expect("a frequent sidecar");
        dir
    }

    fn stored(dir: &std::path::Path) -> Record {
        let mut record = Record::default();
        record.sittings.push(crate::log::sitting::Sitting {
            started_at: "2026-08-08T10:00:00".into(),
            ended_at: "2026-08-08T10:30:00".into(),
            end_position: 148_207,
            seconds: 1800,
            ..Default::default()
        });
        let db = dir.join("vocab.db");
        let clips = dir.join("My Clippings.txt");
        record.sources = Some(gate(&record, &db, &clips, &sidecar::survey(dir)));
        record
    }

    /// The gate stands where nothing it is stamped from moved.
    #[test]
    fn a_launch_that_moved_nothing_asks_the_sources_nothing() {
        let dir = documents_at("quiet");
        let record = stored(&dir);
        let (_, identifying, _) = asked(
            &record,
            &dir.join("vocab.db"),
            &dir.join("My Clippings.txt"),
            &dir,
        );
        assert!(
            identifying.is_none(),
            "nothing moved and the sources were asked"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// The gate opens on each thing it is stamped from moving.
    #[test]
    fn anything_an_identifying_pass_reads_opens_the_gate() {
        let dir = documents_at("moved");
        let db = dir.join("vocab.db");
        let clips = dir.join("My Clippings.txt");
        let opened = |record: &Record| asked(record, &db, &clips, &dir).1.is_some();

        // A word looked up.
        let mut record = stored(&dir);
        std::fs::write(&db, b"a lookup").expect("a vocab db");
        assert!(opened(&record), "vocab.db moved");

        // A highlight made.
        record = stored(&dir);
        std::fs::write(&clips, b"a clipping").expect("a clippings file");
        assert!(opened(&record), "the clippings file moved");

        // A book put back on the Kindle.
        record = stored(&dir);
        let back = dir.join("Another Book.sdr");
        std::fs::create_dir_all(&back).expect("another sidecar directory");
        std::fs::write(back.join("Another Book.yjr"), b"rare").expect("another sidecar");
        assert!(opened(&record), "the sidecars moved");
        let _ = std::fs::remove_dir_all(&back);

        // A sitting read.
        record = stored(&dir);
        record.sittings.push(crate::log::sitting::Sitting {
            started_at: "2026-08-09T10:00:00".into(),
            ended_at: "2026-08-09T10:30:00".into(),
            end_position: 500_100,
            seconds: 1800,
            ..Default::default()
        });
        assert!(opened(&record), "a sitting was read");

        // A book the library lists.
        record = stored(&dir);
        record.books.push(crate::record::Book {
            extent: 148_207,
            cde_key: "B00OKPCRLG".into(),
            title: "A Book".into(),
            ..Default::default()
        });
        assert!(opened(&record), "the library identified a book");

        // A build that reads the sources differently.
        record = stored(&dir);
        if let Some(held) = record.sources.as_mut() {
            held.rules -= 1;
        }
        assert!(opened(&record), "an older build wrote the row");
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// A `.yjf` rewritten opens the identifying gate, and `highlights::wants`
    /// answers false until the `.yjr` moves.
    #[test]
    fn a_page_turn_opens_no_highlights_pass() {
        let dir = documents_at("turned");
        let db = dir.join("vocab.db");
        let clips = dir.join("My Clippings.txt");
        let mut record = stored(&dir);
        record.gate = Some(crate::highlights::gate(&clips, &sidecar::survey(&dir)));
        std::fs::write(dir.join("A Book.sdr/A Book.yjf"), b"frequent, a turn on").expect("a turn");
        let (_, identifying, highlights) = asked(&record, &db, &clips, &dir);
        assert!(identifying.is_some(), "the counters moved");
        assert!(!highlights, "no highlight was made, edited or deleted");
        std::fs::write(dir.join("A Book.sdr/A Book.yjr"), b"rare, a note saved")
            .expect("a note saved");
        assert!(asked(&record, &db, &clips, &dir).2, "the rare half moved");
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// A `g` row has to mean the same thing to the build that reads it back.
    #[test]
    fn the_gate_goes_to_the_record_and_comes_back() {
        let dir = documents_at("row");
        let record = stored(&dir);
        let read = Record::from_text(&record.text());
        assert_eq!(read.sources, record.sources);
        // And an older record, carrying no row at all, asks again.
        let older: String = record
            .text()
            .lines()
            .filter(|l| !l.starts_with("g\t"))
            .map(|l| format!("{l}\n"))
            .collect();
        assert_eq!(Record::from_text(&older).sources, None);
        let _ = std::fs::remove_dir_all(&dir);
    }
}
