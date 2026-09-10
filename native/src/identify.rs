//! Naming a sitting's book: the four sources, in one order, and the one join
//! they share.
//!
//! A sitting is keyed by its `EndPos` class, and `Store::slot_for` looks for a
//! [`crate::store::BookRecord`] under it. Where the catalog names no such book
//! — because it was borrowed and returned, deleted, or re-downloaded under a
//! new content key — three more sources can, each writing the same rows the
//! catalog does so that `slot_for` reaches them unchanged.
//!
//! ## The order
//!
//! [`crate::store::Named`] ranks them, and the ranking is what each source
//! knows a book by:
//!
//! | source | names a book with | what the read costs |
//! | --- | --- | --- |
//! | `cc.db` | everything: title, author, jacket, content key, place | one query, made every pass anyway |
//! | `vocab.db` | a title, an author and a content key, per word looked up | one query |
//! | `My Clippings.txt` | a title and an author, per annotation | one file, parsed |
//! | the `.sdr` directories | the book's own file name | a walk of `/mnt/us/documents`, opening a sidecar per book |
//!
//! The order is what each source knows a book by, not what its read costs: a
//! source knows a book well exactly when the reader wrote it *with* the book's
//! metadata in hand. The first three are milliseconds each and none of them
//! grows with the library. The walk is last on both counts — it is the one
//! read that grows with the shelf, and its answer is a file name.
//!
//! Each source is asked only while [`crate::store::Store::wants_naming`]
//! answers for it, so a device whose catalog names everything reads none of
//! them. A source may take a class a weaker one named, which is how a book
//! that arrived as a file name gets its real title.
//!
//! ## The gate
//!
//! `wants_naming` alone is not enough. A class whose book was deleted wants a
//! name **for ever** — nothing on the device can give it one until the book
//! comes back — so a record holding one of those reads all three sources on
//! every launch, names nothing, and does it again next launch. Keeping the
//! class is right: that is what lets a re-downloaded book relink to its old
//! reading. Reading the sources again while nothing has moved is not.
//!
//! [`gate`] is everything a pass here reads, in one row: the two files by
//! their length and modification time, the shelf by
//! [`crate::sidecar::survey`], and the record by
//! [`crate::store::Store::naming_stamp`]. `Store::sources` holds what the last
//! pass stood at, and a pass finding the same again is skipped whole — which
//! also means the sidecars are never opened.
//!
//! ## The join
//!
//! Vocabulary lookups and clippings both carry a device-local instant, which
//! is the clock `Session::started_at` and `ended_at` are on. So a witness
//! bracketed by a sitting names that sitting's book, and everything after that
//! — the floor, the unanimity, the link or the new record — is
//! `Store::name_from`, written once. A [`Witness`] is all either source has
//! of its own.
//!
//! ## The walk, and its other half
//!
//! The sidecar walk answers two questions at once and is read once for both.
//! Naming a book is this module's; what the rare sidecar holds is
//! [`crate::annotate`]'s, and it wants the walk on every pass whether or not a
//! class here is still unnamed. [`walk`] runs it and [`rescue_from`] takes the
//! answer, so neither caller reads `documents` twice.

use std::path::Path;

use crate::clippings;
use crate::sidecar;
use crate::store::{Named, Sources, Store};
use crate::vocab;

/// One source's statement that a book was open at an instant.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct Witness {
    /// `YYYY-MM-DDTHH:MM:SS` on the device's own clock, as a sitting stores
    /// its own.
    pub at: String,
    /// The book's title, as `BookMetadata.getTitle()` states it. This is what
    /// two claims on one class have to agree on.
    pub title: String,
    /// `BookMetadata.kv()`, a joined list whose separator differs by reader
    /// stack. Corroboration, never a key.
    pub author: String,
    /// The content key, where the source states one. Empty otherwise.
    pub key: String,
    /// A position on the `extent` axis, where the source states one. Negative
    /// otherwise. A witness past its class's own end is refused.
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
pub struct Rescue {
    /// Witnesses each source produced. Zero where it was not read at all.
    pub lookups: usize,
    pub clippings: usize,
    pub sidecars: usize,
    /// Classes each source named.
    pub by_vocab: usize,
    pub by_clippings: usize,
    pub by_sidecars: usize,
    /// Classes two sources named two books, which stay unnamed on purpose.
    pub contested: usize,
    /// Classes still wanting a book once the last source had spoken.
    pub unnamed: usize,
}

impl Rescue {
    /// Classes the three sources named between them.
    pub fn named(&self) -> usize {
        self.by_vocab + self.by_clippings + self.by_sidecars
    }
}

/// Ask every source that has something to say, strongest first, and fold what
/// they say into `store`.
///
/// The catalog has spoken by the time this runs: `Store::remember` is what
/// writes the `b` rows, and only the classes it leaves unnamed are on offer
/// here.
pub fn rescue(store: &mut Store, clips: &[clippings::Clipping], shelf: &sidecar::Shelf) -> Rescue {
    rescue_from(store, Path::new(vocab::VOCAB_DB), clips, shelf)
}

/// Every sidecar under `documents`, which is the one read both this module and
/// [`crate::annotate`] stand on.
///
/// `counters` is the frequent half, which only [`Store::recover`] reads: a
/// shelf where no class wants a name has nothing to recover and does not pay
/// for it.
pub fn walk(documents: &Path, counters: bool) -> sidecar::Shelf {
    sidecar::read(documents, counters)
}

/// The rules a naming pass reads its sources under. A `g` row an older build
/// wrote states a lower number, and the sources are asked again.
pub const NAMING_RULES: u32 = 1;

/// What the sources stand at now, against `survey` of the shelf.
///
/// [`Sources`] carries what this pass reads and nothing else, so a pass
/// finding the same one again would write the rows already stored.
pub fn gate(store: &Store, db: &Path, clips: &Path, survey: &sidecar::Survey) -> Sources {
    let (vocab_len, vocab_mtime) = stat(db);
    let (clips_len, clips_mtime) = stat(clips);
    Sources {
        vocab_len,
        vocab_mtime,
        clips_len,
        clips_mtime,
        shelf: survey.stamp,
        dirs: survey.dirs,
        record: store.naming_stamp(),
        rules: NAMING_RULES,
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

/// The shelf a launch's naming and annotation passes stand on, and whether
/// either is worth making — taken before a single sidecar is opened.
///
/// `None` where both gates hold: nothing under `documents`, in either file, or
/// in the record has moved since the last pass, so neither pass can write a
/// row it has not written already and the shelf is never parsed.
///
/// The `bool` is whether [`rescue_from`] is the pass to make; the caller reads
/// [`crate::annotate::wants`] for the other, over the same [`sidecar::Survey`].
pub fn asked(
    store: &Store,
    db: &Path,
    clips: &Path,
    documents: &Path,
) -> (sidecar::Survey, Option<Sources>, bool) {
    let survey = sidecar::survey(documents);
    let now = gate(store, db, clips, &survey);
    let naming = (store.sources != Some(now)).then_some(now);
    let marks = crate::annotate::wants(store, clips, &survey);
    (survey, naming, marks)
}

/// The shelf, parsed, for a pass that has something to do. The frequent half
/// is [`Store::recover`]'s alone and only while a class wants a name.
pub fn shelf_for(store: &Store, documents: &Path, naming: bool) -> sidecar::Shelf {
    walk(documents, naming && store.wants_naming(Named::Sidecar))
}

/// [`rescue`] over the three sources named. `clips` is `My Clippings.txt`
/// already parsed: [`crate::annotate::fold`] wants the same records a few
/// lines later, and the launch reads the file once for both.
pub fn rescue_from(
    store: &mut Store,
    db: &Path,
    clips: &[clippings::Clipping],
    shelf: &sidecar::Shelf,
) -> Rescue {
    let mut out = Rescue::default();
    // A class a stronger source found two titles for holds two books, and the
    // next source down having seen only one of them does not settle it.
    let mut contested: Vec<i64> = Vec::new();

    if store.wants_naming(Named::Vocab) {
        let lookups = vocab::read_from(db);
        out.lookups = lookups.len();
        let said: Vec<Witness> = lookups.iter().map(witness_of_lookup).collect();
        out.by_vocab = store.name_from(&said, Named::Vocab, &mut contested);
    }

    if store.wants_naming(Named::Clippings) {
        out.clippings = clips.len();
        let said: Vec<Witness> = clips.iter().map(witness_of_clipping).collect();
        out.by_clippings = store.name_from(&said, Named::Clippings, &mut contested);
    }
    out.contested = contested.len();

    // The counters name only a class nothing else could, and `recover` still
    // runs over an empty list: it is also what drops a pairing a record has
    // since contradicted.
    let counters: &[sidecar::Counter] = match store.wants_naming(Named::Sidecar) {
        true => &shelf.counters,
        false => &[],
    };
    out.sidecars = counters.len();
    out.by_sidecars = store.recover(counters);

    out.unnamed = store.classes_wanting(Named::Sidecar).len();
    out
}

/// A dictionary lookup as a witness. `BOOK_INFO` states the content key and
/// `LOOKUPS.pos` a real position, so this is the only source whose claim can
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
/// position, so nothing here goes in `pos`.
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
            Named::Sidecar,
            Named::Clippings,
            Named::Catalog,
            Named::Vocab,
        ];
        ranked.sort();
        assert_eq!(
            ranked,
            [
                Named::Catalog,
                Named::Vocab,
                Named::Clippings,
                Named::Sidecar
            ]
        );
    }
    /// A scratch `documents` tree with one `.sdr` in it.
    fn shelf_at(tag: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!("readinglog-identify-{tag}"));
        let _ = std::fs::remove_dir_all(&dir);
        let sdr = dir.join("A Book.sdr");
        std::fs::create_dir_all(&sdr).expect("a scratch shelf");
        std::fs::write(sdr.join("A Book.yjr"), b"rare").expect("a rare sidecar");
        std::fs::write(sdr.join("A Book.yjf"), b"frequent").expect("a frequent sidecar");
        dir
    }

    fn stored(dir: &std::path::Path) -> Store {
        let mut store = Store::default();
        store.sessions.push(crate::log::session::Session {
            started_at: "2026-08-08T10:00:00".into(),
            ended_at: "2026-08-08T10:30:00".into(),
            end_position: 148_207,
            seconds: 1800,
            ..Default::default()
        });
        let db = dir.join("vocab.db");
        let clips = dir.join("My Clippings.txt");
        store.sources = Some(gate(&store, &db, &clips, &sidecar::survey(dir)));
        store
    }

    /// The gate is only worth having if a launch that changed nothing leaves
    /// it standing — that is the launch it exists for.
    #[test]
    fn a_launch_that_moved_nothing_asks_the_sources_nothing() {
        let dir = shelf_at("quiet");
        let store = stored(&dir);
        let (_, naming, _) = asked(
            &store,
            &dir.join("vocab.db"),
            &dir.join("My Clippings.txt"),
            &dir,
        );
        assert!(naming.is_none(), "nothing moved and the sources were asked");
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// And only safe if every one of the four things it stands on opens it.
    #[test]
    fn anything_a_naming_pass_reads_opens_the_gate() {
        let dir = shelf_at("moved");
        let db = dir.join("vocab.db");
        let clips = dir.join("My Clippings.txt");
        let opened = |store: &Store| asked(store, &db, &clips, &dir).1.is_some();

        // A word looked up.
        let mut store = stored(&dir);
        std::fs::write(&db, b"a lookup").expect("a vocab db");
        assert!(opened(&store), "vocab.db moved");

        // A highlight made.
        store = stored(&dir);
        std::fs::write(&clips, b"a clipping").expect("a clippings file");
        assert!(opened(&store), "the clippings file moved");

        // A book put back on the device.
        store = stored(&dir);
        let back = dir.join("Another Book.sdr");
        std::fs::create_dir_all(&back).expect("another sidecar directory");
        std::fs::write(back.join("Another Book.yjr"), b"rare").expect("another sidecar");
        assert!(opened(&store), "the shelf moved");
        let _ = std::fs::remove_dir_all(&back);

        // A sitting read.
        store = stored(&dir);
        store.sessions.push(crate::log::session::Session {
            started_at: "2026-08-09T10:00:00".into(),
            ended_at: "2026-08-09T10:30:00".into(),
            end_position: 500_100,
            seconds: 1800,
            ..Default::default()
        });
        assert!(opened(&store), "a sitting was read");

        // A book named by the catalog.
        store = stored(&dir);
        store.books.push(crate::store::BookRecord {
            extent: 148_207,
            cde_key: "B00OKPCRLG".into(),
            title: "A Book".into(),
            ..Default::default()
        });
        assert!(opened(&store), "the catalog named a book");

        // A build that reads the sources differently.
        store = stored(&dir);
        if let Some(held) = store.sources.as_mut() {
            held.rules -= 1;
        }
        assert!(opened(&store), "an older build wrote the row");
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// A `g` row has to mean the same thing to the build that reads it back.
    #[test]
    fn the_gate_goes_to_the_record_and_comes_back() {
        let dir = shelf_at("row");
        let store = stored(&dir);
        let read = Store::from_text(&store.text());
        assert_eq!(read.sources, store.sources);
        // And an older record, carrying no row at all, asks again.
        let older: String = store
            .text()
            .lines()
            .filter(|l| !l.starts_with("g\t"))
            .map(|l| format!("{l}\n"))
            .collect();
        assert_eq!(Store::from_text(&older).sources, None);
        let _ = std::fs::remove_dir_all(&dir);
    }
}
