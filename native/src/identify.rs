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
//! **The answer decides the order, not the read.** The first three cost
//! milliseconds each and none of them grows with the library, so there is
//! nothing to win by shuffling them; what separates them is that a source
//! knows a book well exactly when the reader wrote it *with* the book's
//! metadata in hand. Cost speaks only at the bottom, where it agrees: the walk
//! is the one read that grows with the shelf, and its answer is a file name.
//!
//! Each source is asked only while [`crate::store::Store::wants_naming`]
//! answers for it, so a device whose catalog names everything reads none of
//! them — not even the walk. A source may take a class a weaker one named,
//! which is how a book that arrived as a file name gets its real title.
//!
//! ## The join
//!
//! Vocabulary lookups and clippings both carry a device-local instant, which
//! is the clock `Session::started_at` and `ended_at` are on. So a witness
//! bracketed by a sitting names that sitting's book, and everything after that
//! — the floor, the unanimity, the link or the new record — is
//! `Store::name_from`, written once. A [`Witness`] is all either source has
//! of its own.

use std::path::Path;

use crate::clippings;
use crate::sidecar;
use crate::store::{Named, Store};
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
pub fn rescue(store: &mut Store) -> Rescue {
    rescue_from(
        store,
        Path::new(vocab::VOCAB_DB),
        Path::new(clippings::CLIPPINGS_FILE),
        Path::new(sidecar::DOCUMENTS_DIR),
    )
}

/// [`rescue`] over the three sources named.
pub fn rescue_from(store: &mut Store, db: &Path, clips: &Path, documents: &Path) -> Rescue {
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
        let clips = clippings::read(clips);
        out.clippings = clips.len();
        let said: Vec<Witness> = clips.iter().map(witness_of_clipping).collect();
        out.by_clippings = store.name_from(&said, Named::Clippings, &mut contested);
    }
    out.contested = contested.len();

    // The walk is the dear one, so it only happens for a class nothing else
    // could name. `recover` still runs over an empty list: it is also what
    // drops a pairing a record has since contradicted.
    let sidecars = match store.wants_naming(Named::Sidecar) {
        true => sidecar::read(documents),
        false => Vec::new(),
    };
    out.sidecars = sidecars.len();
    out.by_sidecars = store.recover(&sidecars);

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
}
