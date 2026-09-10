//! `.sdr` sidecar directories: the KRDS container, `timer.model`'s counters,
//! and the annotations the rare sidecar holds.
//!
//! A `.sdr` carries two sidecars per book. The **frequent** one — `.yjf` and
//! its four siblings — is rewritten on every turn and holds `timer.model`.
//! The **rare** one, [`RARE`], is written when an annotation is made, and
//! holds `annotation.cache.object`: the roster of what the book still carries.
//! [`read`] walks for both at once, because the walk is the dear part.
//!
//! What the rare file states and `My Clippings.txt` cannot: a real position on
//! the `p_contentSize` axis, the colour, and — by an annotation's absence from
//! it — that the reader deleted one. What it cannot state and the clippings
//! file can: the words. [`crate::annotate`] joins the two.

use std::fmt;
use std::path::{Path, PathBuf};

use crate::clippings::Kind;

/// The directory holding book files and their `.sdr` directories.
pub const DOCUMENTS_DIR: &str = "/mnt/us/documents";

/// The suffix a sidecar directory carries.
const SDR: &str = ".sdr";

/// The frequently updated sidecar's suffix, one per reader stack: KFX, mobi8,
/// Mobipocket, Topaz, PDF. `timer.model` is in this half.
const FREQUENT: [&str; 5] = [".yjf", ".azw3f", ".mbs", ".tas", ".pdt"];

/// The rarely updated sidecar's suffix, the same five stacks in the same
/// order. `annotation.cache.object` is in this half.
const RARE: [&str; 5] = [".yjr", ".azw3r", ".mbp1", ".tal", ".pds"];

/// The suffix marking a superseded sidecar.
const SUPERSEDED: &str = ".bad_file";

/// The suffix on a sidecar half written.
const PARTIAL: &str = ".tmp";

/// How long the profile infix is: `BaseProfileImpl.cHj()` writes 32 lowercase
/// hex characters between the book's stem and the suffix.
const INFIX_LEN: usize = 32;

/// The record holding the annotations a book still carries.
const ANNOTATION_CACHE: &str = "annotation.cache.object";

/// The record inside it holding one kind's annotations.
const ANNOTATION_TREE: &str = "saved.avl.interval.tree";

/// What every annotation record's name opens with.
const ANNOTATION: &str = "annotation.personal.";

/// How deep under [`DOCUMENTS_DIR`] [`collect`] descends.
const MAX_DEPTH: usize = 4;

/// The record holding `TotalTime` and `TotalWords`.
const TIMER_MODEL: &str = "timer.model";

/// Magic bytes every sidecar opens with.
pub const SIGNATURE: [u8; 8] = [0x00, 0x00, 0x00, 0x00, 0x00, 0x1A, 0xB1, 0x26];

const T_BOOL: u8 = 0;
const T_INT: u8 = 1;
const T_LONG: u8 = 2;
const T_UTF: u8 = 3;
const T_DOUBLE: u8 = 4;
const T_SHORT: u8 = 5;
const T_FLOAT: u8 = 6;
const T_BYTE: u8 = 7;
const T_CHAR: u8 = 9;
const T_OBJECT_BEGIN: u8 = 0xFE;
const T_OBJECT_END: u8 = 0xFF;

/// What went wrong reading a sidecar.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum KrdsError {
    /// The file doesn't open with [`SIGNATURE`].
    NotASidecar,
    /// A value ran past the end of the buffer.
    Truncated { at: usize },
    /// A type byte this format doesn't define.
    UnknownType { byte: u8, at: usize },
    /// A string that isn't valid UTF-8.
    BadUtf8 { at: usize },
    /// Bytes left over after the declared object count was read.
    TrailingBytes { at: usize },
}

impl fmt::Display for KrdsError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NotASidecar => write!(f, "not a KRDS sidecar (bad signature)"),
            Self::Truncated { at } => write!(f, "value runs past end of buffer at {at}"),
            Self::UnknownType { byte, at } => write!(f, "unknown type byte {byte:#04x} at {at}"),
            Self::BadUtf8 { at } => write!(f, "invalid UTF-8 string at {at}"),
            Self::TrailingBytes { at } => write!(f, "trailing bytes after last object at {at}"),
        }
    }
}

impl std::error::Error for KrdsError {}

/// One typed value.
#[derive(Debug, Clone, PartialEq)]
pub enum Value {
    Bool(u8),
    Int(i32),
    Long(i64),
    /// `None` is the format's null string, which carries no length or payload.
    Utf8(Option<String>),
    Double([u8; 8]),
    Short(i16),
    Float([u8; 4]),
    Byte(u8),
    /// `DataOutputStream.writeChar` is a UTF-16 code unit: **two** bytes, on
    /// every firmware. Reading one shifts everything after it by a byte and
    /// costs the whole file.
    Char(u16),
    Object(Object),
}

impl Value {
    /// The number in an `Int`, widened.
    pub fn as_int(&self) -> Option<i64> {
        match self {
            Self::Int(v) => Some(i64::from(*v)),
            _ => None,
        }
    }

    /// The number in a `Long`.
    pub fn as_long(&self) -> Option<i64> {
        match self {
            Self::Long(v) => Some(*v),
            _ => None,
        }
    }

    /// The object in an `Object`.
    pub fn as_object(&self) -> Option<&Object> {
        match self {
            Self::Object(o) => Some(o),
            _ => None,
        }
    }
}

/// A named node: a record type, and the values that make it up.
#[derive(Debug, Clone, PartialEq)]
pub struct Object {
    pub name: String,
    pub values: Vec<Value>,
}

/// A parsed sidecar.
#[derive(Debug, Clone, PartialEq)]
pub struct Store {
    /// Format version from the header.
    pub version: i64,
    pub roots: Vec<Object>,
}

struct Reader<'a> {
    b: &'a [u8],
    i: usize,
}

impl<'a> Reader<'a> {
    fn take(&mut self, n: usize) -> Result<&'a [u8], KrdsError> {
        let end = self
            .i
            .checked_add(n)
            .ok_or(KrdsError::Truncated { at: self.i })?;
        let s = self
            .b
            .get(self.i..end)
            .ok_or(KrdsError::Truncated { at: self.i })?;
        self.i = end;
        Ok(s)
    }

    fn u8(&mut self) -> Result<u8, KrdsError> {
        Ok(self.take(1)?[0])
    }

    fn peek(&self) -> Option<u8> {
        self.b.get(self.i).copied()
    }

    /// A UTF-8 payload without its type byte: null flag, then length + bytes.
    fn utf_body(&mut self) -> Result<Option<String>, KrdsError> {
        if self.u8()? != 0 {
            return Ok(None);
        }
        let at = self.i;
        let n = u16::from_be_bytes(self.take(2)?.try_into().expect("2 bytes")) as usize;
        let raw = self.take(n)?;
        modified_utf8(raw)
            .map(Some)
            .ok_or(KrdsError::BadUtf8 { at })
    }

    fn value(&mut self) -> Result<Value, KrdsError> {
        let at = self.i;
        let t = self.u8()?;
        Ok(match t {
            T_BOOL => Value::Bool(self.u8()?),
            T_INT => Value::Int(i32::from_be_bytes(self.take(4)?.try_into().expect("4"))),
            T_LONG => Value::Long(i64::from_be_bytes(self.take(8)?.try_into().expect("8"))),
            T_UTF => Value::Utf8(self.utf_body()?),
            T_DOUBLE => Value::Double(self.take(8)?.try_into().expect("8")),
            T_SHORT => Value::Short(i16::from_be_bytes(self.take(2)?.try_into().expect("2"))),
            T_FLOAT => Value::Float(self.take(4)?.try_into().expect("4")),
            T_BYTE => Value::Byte(self.u8()?),
            T_CHAR => Value::Char(u16::from_be_bytes(self.take(2)?.try_into().expect("2"))),
            T_OBJECT_BEGIN => Value::Object(self.object()?),
            other => return Err(KrdsError::UnknownType { byte: other, at }),
        })
    }

    /// Reads an object's body, the `0xFE` opening it consumed.
    fn object(&mut self) -> Result<Object, KrdsError> {
        let name = self.utf_body()?.unwrap_or_default();
        let mut values = Vec::new();
        while self.peek() != Some(T_OBJECT_END) {
            values.push(self.value()?);
        }
        self.u8()?; // the terminator
        Ok(Object { name, values })
    }
}

/// `java.io.DataInputStream.readUTF`'s payload, which is **modified** UTF-8
/// and not the encoding `String::from_utf8` reads.
///
/// Two differences, both of which a note body reaches: `U+0000` is written as
/// `C0 80`, and anything above `U+FFFF` as a **surrogate pair** — two
/// three-byte sequences, six bytes, never the four-byte form.
///
/// A lone surrogate is a character Java can hold and Rust cannot; it becomes
/// `U+FFFD` rather than costing the file. A malformed sequence answers `None`.
fn modified_utf8(raw: &[u8]) -> Option<String> {
    let mut units: Vec<u16> = Vec::with_capacity(raw.len());
    let mut at = 0;
    while at < raw.len() {
        let b = raw[at];
        let unit = match b {
            0x00..=0x7F => {
                at += 1;
                u16::from(b)
            }
            0xC0..=0xDF => {
                let next = *raw.get(at + 1)?;
                at += 2;
                (u16::from(b & 0x1F) << 6) | u16::from(next & 0x3F)
            }
            0xE0..=0xEF => {
                let (b1, b2) = (*raw.get(at + 1)?, *raw.get(at + 2)?);
                at += 3;
                (u16::from(b & 0x0F) << 12) | (u16::from(b1 & 0x3F) << 6) | u16::from(b2 & 0x3F)
            }
            // The four-byte form and the continuation bytes are both out of
            // place at the head of a character.
            _ => return None,
        };
        units.push(unit);
    }
    // `from_utf16_lossy` pairs the surrogates and puts `U+FFFD` where one
    // stands alone.
    Some(String::from_utf16_lossy(&units))
}

impl Store {
    /// Read a sidecar.
    pub fn parse(bytes: &[u8]) -> Result<Self, KrdsError> {
        let mut r = Reader { b: bytes, i: 0 };
        if r.take(8)? != SIGNATURE {
            return Err(KrdsError::NotASidecar);
        }
        let version = r.value()?.as_long().unwrap_or(1);
        let count = r.value()?.as_int().unwrap_or(0).max(0) as usize;
        let mut roots = Vec::with_capacity(count);
        for _ in 0..count {
            if r.u8()? != T_OBJECT_BEGIN {
                return Err(KrdsError::UnknownType {
                    byte: r.b.get(r.i - 1).copied().unwrap_or(0),
                    at: r.i - 1,
                });
            }
            roots.push(r.object()?);
        }
        if r.i != bytes.len() {
            return Err(KrdsError::TrailingBytes { at: r.i });
        }
        Ok(Self { version, roots })
    }

    /// The first top-level object called `name`.
    pub fn root(&self, name: &str) -> Option<&Object> {
        self.roots.iter().find(|o| o.name == name)
    }
}

/// What the sixth field of an annotation record holds. It is the one field
/// the subclass writes, and which subclass wrote it is the record's own name.
///
/// Read off the 5.19 `ReaderSDK-impl` classes: the five that call
/// `isColorSupportedDevice` write a colour, `Note` writes the text the reader
/// typed, the three handwritten kinds write the id their ink is filed under,
/// and `Asterisk`, `ClipArticle` and `GraphicalHighlight` write no sixth field
/// at all.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Sixth {
    Colour,
    Body,
    /// An id naming ink or a note kept elsewhere. Held by nothing here.
    Elsewhere,
    None,
}

/// What `kind`'s subclass writes as its sixth field.
fn sixth(kind: Kind) -> Sixth {
    match kind {
        Kind::Bookmark | Kind::Highlight | Kind::Underline | Kind::Circle | Kind::Pin => {
            Sixth::Colour
        }
        Kind::Note => Sixth::Body,
        Kind::StickyNote | Kind::Handwriting | Kind::HandwritingOnContent => Sixth::Elsewhere,
        Kind::Article | Kind::Asterisk | Kind::GraphicalHighlight => Sixth::None,
    }
}

/// One annotation, as `AnnotationImpl.a(ReaderDataOutput)` wrote it: five
/// fields, and the sixth its subclass adds.
///
/// The anchors are held verbatim. `HTMLPosition`, `MobiPosition` and
/// `TopazPosition` write a bare integer, `YJPosition` writes
/// `<base64>:<position>`, and `PDFPosition` writes `<page> <x> <y> <nBounds>`
/// — so fields 1 and 2 are taken **by their place**, never by recognising
/// which stack wrote them.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct Annotation {
    pub kind: Kind,
    /// Field 1, the start anchor.
    pub start: String,
    /// Field 2, the end anchor.
    pub end: String,
    /// Field 3, `created`, epoch milliseconds on the device's own clock. This
    /// is the instant a clipping's `Added on` names, to the second.
    pub created: i64,
    /// Field 4, `modified`. Equal to `created` until the reader edits it.
    pub modified: i64,
    /// Field 5, `<tags>\u{FFFC}<flags>` verbatim. `0\u{FFFC}0` is the
    /// ordinary case: no tags and no flags.
    pub flags: String,
    /// Field 6 where the kind writes a colour — one of the eleven
    /// `AnnotationColor` names — and empty otherwise.
    pub colour: String,
    /// Field 6 where the kind writes the reader's own words, and empty
    /// otherwise. Never the book's text: no sidecar holds that.
    pub body: String,
}

impl Annotation {
    /// The position [`Self::start`] names on the book's `p_contentSize` axis,
    /// and `None` for an anchor that is on no such axis — a PDF's page and
    /// point.
    pub fn start_position(&self) -> Option<i64> {
        position(&self.start)
    }

    /// [`Self::start_position`] for the end anchor.
    pub fn end_position(&self) -> Option<i64> {
        position(&self.end)
    }

    /// The second [`Self::created`] falls in, which is the join to a clipping.
    pub fn second(&self) -> i64 {
        self.created.div_euclid(1_000)
    }
}

/// The position an anchor names, read by shape: the run after the last `:` for
/// a KFX anchor, the whole of a bare-integer one, and nothing for a PDF's
/// space-separated page and point.
fn position(anchor: &str) -> Option<i64> {
    let tail = match anchor.rsplit_once(':') {
        Some((_, tail)) => tail,
        None => anchor,
    };
    tail.parse().ok()
}

/// What a book's frequent sidecar states about the reading it has had.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Counter {
    /// The `.sdr` directory's name without its suffix: the book's own file.
    pub file: String,
    /// `timer.model`'s `TotalTime`, cumulative milliseconds against this book.
    pub total_ms: i64,
    /// `timer.model`'s `TotalWords`.
    pub words: i64,
}

/// `timer.model`'s `TotalTime` and `TotalWords`, its values 1 and 2.
fn counters(store: &Store) -> Option<(i64, i64)> {
    let model = store.root(TIMER_MODEL)?;
    let total_ms = model.values.get(1)?.as_long()?;
    let words = model.values.get(2)?.as_long()?;
    Some((total_ms, words))
}

/// What one `.sdr` directory's **rare** sidecar states about its book's marks:
/// the roster of what that book still carries.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct Roster {
    /// The `.sdr` directory's name without its suffix: the book's own file,
    /// which is `catalog::Book::location` with its last suffix cut.
    pub file: String,
    /// Whether the sidecar was in a position to say what still exists: it is
    /// there, it parses, and it names an `annotation.cache.object`.
    ///
    /// **An empty cache counts.** `AnnotationCacheObject.a` returns before
    /// writing its count when nothing is left — logging *"Notes and Highlights
    /// not found while saving annotations"* — so an empty record is the writer
    /// stating that the book carries none, not a torn file. Only that
    /// distinction lets an annotation's absence be read as a deletion.
    pub trusted: bool,
    /// Every annotation it holds, in the order the trees were written.
    pub annotations: Vec<Annotation>,
}

/// What one walk of `documents` found: both halves of every sidecar.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct Shelf {
    /// `timer.model`'s counters, ascending by file.
    pub counters: Vec<Counter>,
    /// What the rare sidecars hold, ascending by file. One entry per `.sdr`,
    /// whether or not it carried a rare file.
    pub rosters: Vec<Roster>,
}

/// What a walk of `documents` says without opening a single sidecar.
///
/// [`read`] opens and parses two files per book — on a large shelf, thousands
/// of reads off flash for an answer that is usually the one already stored.
/// Every byte of that answer comes from files whose length and modification
/// time say whether they have moved, and a walk can have both for the cost of
/// a `stat`. Two surveys that agree stand for two identical parses.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct Survey {
    /// The `.sdr` directories found.
    pub dirs: usize,
    /// Over each directory's name and the length and modification time of
    /// each sidecar in it, in walk order.
    pub stamp: u64,
}

/// [`Survey`] of `documents`: the same walk [`read`] makes, stopping at each
/// sidecar's metadata rather than its bytes.
pub fn survey(documents: &Path) -> Survey {
    let mut dirs = Vec::new();
    collect(documents, 0, &mut dirs);
    dirs.sort();
    let mut stamp = crate::stamp::Stamp::default();
    for (sdr, file) in &dirs {
        stamp.text(file);
        // Both halves, by name, so that a sidecar appearing or being replaced
        // moves the stamp even where the lengths happen to match.
        let mut found = files_in(sdr, &FREQUENT);
        found.extend(files_in(sdr, &RARE));
        found.sort();
        for (name, path) in found {
            stamp.text(&name);
            let Ok(held) = std::fs::metadata(&path) else {
                continue;
            };
            stamp.num(held.len() as i64);
            stamp.num(
                held.modified()
                    .ok()
                    .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
                    .map_or(0, |d| d.as_secs() as i64),
            );
        }
    }
    Survey {
        dirs: dirs.len(),
        stamp: stamp.done(),
    }
}

/// Every sidecar under `documents`, book file present or not. Empty where
/// `documents` does not exist; a sidecar that will not parse is printed and
/// skipped.
///
/// One walk answers both halves: the walk is what costs, and
/// [`crate::identify`] and [`crate::annotate`] each want one of them.
///
/// `counters` is the frequent half, which only [`crate::store::Store::recover`]
/// reads and only while some class still wants a name. A shelf whose catalog
/// names everything asks for it `false` and saves a file open and a parse per
/// book.
pub fn read(documents: &Path, counters: bool) -> Shelf {
    let mut dirs = Vec::new();
    collect(documents, 0, &mut dirs);
    // The profile the shelf belongs to is whichever infix most of its rare
    // sidecars carry — the device does not write it anywhere a reader can
    // ask. A directory holding one file of another profile then keeps its own.
    let profile = profile_of(&dirs);
    let mut out = Shelf::default();
    for (sdr, file) in dirs {
        if counters && let Some(counter) = read_counter(&sdr, &file) {
            out.counters.push(counter);
        }
        out.rosters
            .push(read_roster(&sdr, &file, profile.as_deref()));
    }
    out.counters.sort_by(|a, b| a.file.cmp(&b.file));
    out.rosters.sort_by(|a, b| a.file.cmp(&b.file));
    out
}

/// Every `.sdr` under `dir`, as `(the directory, the book's file name)`.
fn collect(dir: &Path, depth: usize, out: &mut Vec<(PathBuf, String)>) {
    if depth > MAX_DEPTH {
        return;
    }
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if !path.is_dir() {
            continue;
        }
        match path.file_name().and_then(|n| n.to_str()) {
            Some(name) if name.ends_with(SDR) => {
                let file = name[..name.len() - SDR.len()].to_string();
                out.push((path, file));
            }
            _ => collect(&path, depth + 1, out),
        }
    }
}

/// The counters in `sdr`'s frequent sidecar, or `None` where it holds none.
fn read_counter(sdr: &Path, file: &str) -> Option<Counter> {
    let at = frequent(sdr)?;
    let store = open_sidecar(&at)?;
    let (total_ms, words) = counters(&store)?;
    Some(Counter {
        file: file.to_string(),
        total_ms,
        words,
    })
}

/// What `sdr`'s rare sidecar holds. A directory carrying none, or one that
/// will not parse, answers [`Roster::trusted`] false and says nothing more.
fn read_roster(sdr: &Path, file: &str, profile: Option<&str>) -> Roster {
    let mut out = Roster {
        file: file.to_string(),
        ..Roster::default()
    };
    let Some(at) = rare(sdr, profile) else {
        return out;
    };
    let Some(store) = open_sidecar(&at) else {
        return out;
    };
    let Some(cache) = store.root(ANNOTATION_CACHE) else {
        return out;
    };
    out.trusted = true;
    out.annotations = annotations(cache);
    out
}

/// The file at `at`, parsed, with whatever went wrong printed and nothing
/// raised: one unreadable sidecar must never cost the walk.
fn open_sidecar(at: &Path) -> Option<Store> {
    let bytes = match std::fs::read(at) {
        Ok(bytes) => bytes,
        Err(err) => {
            eprintln!("!! sidecar: {} — {err}", at.display());
            return None;
        }
    };
    match Store::parse(&bytes) {
        Ok(store) => Some(store),
        Err(err) => {
            eprintln!("!! sidecar: {} — {err}", at.display());
            None
        }
    }
}

/// Every annotation an `annotation.cache.object` holds.
///
/// The record is `writeInt(nTypes)` and then a `(type code, tree)` pair per
/// kind, but the kind is on each record's own name, so the count and the codes
/// are stepped over and every [`ANNOTATION_TREE`] found is read. That also
/// reads the empty record, which carries no count at all.
fn annotations(cache: &Object) -> Vec<Annotation> {
    let mut out = Vec::new();
    for tree in cache.values.iter().filter_map(Value::as_object) {
        if tree.name != ANNOTATION_TREE {
            continue;
        }
        // `SavedAnnotationAVLTree.a` writes `writeInt(size)` and then the
        // records; the records are the objects.
        out.extend(
            tree.values
                .iter()
                .filter_map(Value::as_object)
                .filter_map(one),
        );
    }
    out
}

/// One `annotation.personal.*` record, read **positionally**. `None` where the
/// name is not one of the twelve, or a field is missing or of the wrong type.
fn one(record: &Object) -> Option<Annotation> {
    let kind = Kind::from_stored(record.name.strip_prefix(ANNOTATION)?)?;
    let text = |at: usize| match record.values.get(at) {
        Some(Value::Utf8(said)) => Some(said.clone().unwrap_or_default()),
        _ => None,
    };
    let sixth_field = text(5).unwrap_or_default();
    let (colour, body) = match sixth(kind) {
        Sixth::Colour => (sixth_field, String::new()),
        Sixth::Body => (String::new(), sixth_field),
        Sixth::Elsewhere | Sixth::None => (String::new(), String::new()),
    };
    Some(Annotation {
        kind,
        start: text(0)?,
        end: text(1)?,
        created: record.values.get(2)?.as_long()?,
        modified: record.values.get(3)?.as_long()?,
        flags: text(4).unwrap_or_default(),
        colour,
        body,
    })
}

/// The frequent sidecar inside `sdr`, matched on a [`FREQUENT`] suffix.
fn frequent(sdr: &Path) -> Option<PathBuf> {
    let mut found = files_in(sdr, &FREQUENT);
    found.sort();
    found.pop().map(|(_, path)| path)
}

/// The rare sidecar inside `sdr`: the one carrying `profile`'s infix, else the
/// one carrying any infix, else whatever is there.
///
/// A directory can hold two. `ReaderPermanentState.a`'s half-finished
/// migration leaves the bare-named file beside the profile-named one, and a
/// second reading profile writes its own. So **glob and choose**; never
/// construct the name.
fn rare(sdr: &Path, profile: Option<&str>) -> Option<PathBuf> {
    let mut found = files_in(sdr, &RARE);
    // Ranked worst first, so the last is the one to take.
    found.sort_by_key(|(name, path)| {
        let infix = infix_of(name);
        let rank = match (&infix, profile) {
            (Some(held), Some(want)) if held == want => 3,
            (Some(_), _) => 2,
            (None, _) => 1,
        };
        (rank, path.clone())
    });
    found.pop().map(|(_, path)| path)
}

/// Every file in `sdr` whose name ends in one of `suffixes`, as
/// `(name, path)`. A superseded or half-written one is never among them.
fn files_in(sdr: &Path, suffixes: &[&str]) -> Vec<(String, PathBuf)> {
    let Ok(entries) = std::fs::read_dir(sdr) else {
        return Vec::new();
    };
    entries
        .flatten()
        .filter_map(|e| {
            let path = e.path();
            let name = path.file_name()?.to_str()?.to_string();
            let live = !name.ends_with(SUPERSEDED) && !name.ends_with(PARTIAL);
            let wanted = suffixes.iter().any(|s| name.ends_with(s));
            (live && wanted).then_some((name, path))
        })
        .collect()
}

/// The 32 lowercase hex characters `BaseProfileImpl.cHj()` writes between a
/// sidecar's stem and its suffix, and `None` for a name carrying none.
fn infix_of(name: &str) -> Option<String> {
    let stem = name.rsplit_once('.').map(|(head, _)| head).unwrap_or(name);
    let at = stem.len().checked_sub(INFIX_LEN)?;
    let tail = stem.get(at..)?;
    tail.bytes()
        .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
        .then(|| tail.to_string())
}

/// The profile most of the shelf's rare sidecars were written by, and `None`
/// where none carries an infix. A tie takes the lowest, so two runs answer
/// alike.
fn profile_of(dirs: &[(PathBuf, String)]) -> Option<String> {
    let mut seen: Vec<String> = dirs
        .iter()
        .flat_map(|(sdr, _)| files_in(sdr, &RARE))
        .filter_map(|(name, _)| infix_of(&name))
        .collect();
    seen.sort();
    let mut best: Option<(usize, String)> = None;
    let mut at = 0;
    while at < seen.len() {
        let run = seen[at..].iter().take_while(|s| **s == seen[at]).count();
        if best.as_ref().is_none_or(|(most, _)| run > *most) {
            best = Some((run, seen[at].clone()));
        }
        at += run;
    }
    best.map(|(_, infix)| infix)
}

#[cfg(test)]
mod tests {
    use super::*;

    // Whole sidecars, as bytes, written by the firmware writers' own field
    // order.
    //
    // Keep them as bytes. An encoder written in this module against this
    // module's reader would agree with the reader whatever either does, and
    // prove nothing about the format.

    /// Every kind the 5.19 writers name, one tree each.
    const KINDS: &str = concat!(
        "00000000001ab1260200000000000000010100000001fe000017616e6e6f746174696f6e",
        "2e63616368652e6f626a656374010000000c0100000000fe00001773617665642e61766c",
        "2e696e74657276616c2e747265650100000001fe00001c616e6e6f746174696f6e2e7065",
        "72736f6e616c2e626f6f6b6d61726b030000104151454141414141414141413a31303003",
        "0000104151454141414141414141413a31303002000001a07aea3e1a02000001a07aea3e",
        "1a0300000530efbfbc30030000096461726b5f626c7565ffff0100000001fe0000177361",
        "7665642e61766c2e696e74657276616c2e747265650100000001fe00001d616e6e6f7461",
        "74696f6e2e706572736f6e616c2e686967686c6967687403000010415149414141414141",
        "4141413a323030030000104151494141414149414141413a32303802000001a07aea3e1a",
        "02000001a07aea3e1a0300000530efbfbc300300000679656c6c6f77ffff0100000002fe",
        "00001773617665642e61766c2e696e74657276616c2e747265650100000001fe00001861",
        "6e6e6f746174696f6e2e706572736f6e616c2e6e6f74650300001041514d414141414141",
        "4141413a3330300300001041514d4141414143414141413a33303202000001a07aea3e1a",
        "02000001a07aea3e1a0300000530efbfbc300300000661206e6f7465ffff0100000003fe",
        "00001773617665642e61766c2e696e74657276616c2e747265650100000001fe00002061",
        "6e6e6f746174696f6e2e706572736f6e616c2e636c69705f61727469636c650300001041",
        "51514141414141414141413a34303003000010415151414141414a414141413a34303902",
        "000001a07aea3e1a02000001a07aea3e1a0300000530efbfbc30ffff0100000005fe0000",
        "1773617665642e61766c2e696e74657276616c2e747265650100000001fe00001f616e6e",
        "6f746174696f6e2e706572736f6e616c2e737469636b795f6e6f74650300001041515541",
        "41414141414141413a353030030000104151554141414142414141413a35303102000001",
        "a07aea3e1a02000001a07aea3e1a0300000530efbfbc300300000430616131ffff010000",
        "0006fe00001773617665642e61766c2e696e74657276616c2e747265650100000001fe00",
        "0024616e6e6f746174696f6e2e706572736f6e616c2e68616e647772697474656e5f6e6f",
        "7465030000104151594141414141414141413a3630300300001041515941414141424141",
        "41413a36303102000001a07aea3e1a02000001a07aea3e1a0300000530efbfbc30030000",
        "0430616132ffff0100000007fe00001773617665642e61766c2e696e74657276616c2e74",
        "7265650100000001fe00002f616e6e6f746174696f6e2e706572736f6e616c2e68616e64",
        "7772697474656e5f6f6e5f636f6e74656e745f6e6f746503000010415163414141414141",
        "4141413a373030030000104151634141414142414141413a37303102000001a07aea3e1a",
        "02000001a07aea3e1a0300000530efbfbc300300000430616133ffff0100000008fe0000",
        "1773617665642e61766c2e696e74657276616c2e747265650100000001fe000027616e6e",
        "6f746174696f6e2e706572736f6e616c2e67726170686963616c5f686967686c69676874",
        "030000104151674141414141414141413a38303003000010415167414141414541414141",
        "3a38303402000001a07aea3e1a02000001a07aea3e1a0300000530efbfbc30ffff010000",
        "0009fe00001773617665642e61766c2e696e74657276616c2e747265650100000001fe00",
        "0017616e6e6f746174696f6e2e706572736f6e616c2e70696e0300001041516b41414141",
        "41414141413a3930300300001041516b4141414141414141413a39303002000001a07aea",
        "3e1a02000001a07aea3e1a0300000530efbfbc3003000003726564ffff010000000afe00",
        "001773617665642e61766c2e696e74657276616c2e747265650100000001fe00001c616e",
        "6e6f746174696f6e2e706572736f6e616c2e617374657269736b0300001141516f414141",
        "4141414141413a313030300300001141516f4141414141414141413a3130303002000001",
        "a07aea3e1a02000001a07aea3e1a0300000530efbfbc30ffff010000000cfe0000177361",
        "7665642e61766c2e696e74657276616c2e747265650100000001fe00001a616e6e6f7461",
        "74696f6e2e706572736f6e616c2e636972636c6503000011415173414141414141414141",
        "3a31313030030000114151734141414147414141413a3131303602000001a07aea3e1a02",
        "000001a07aea3e1a0300000530efbfbc3003000005677265656effff010000000dfe0000",
        "1773617665642e61766c2e696e74657276616c2e747265650100000001fe00001d616e6e",
        "6f746174696f6e2e706572736f6e616c2e756e6465726c696e6503000011415177414141",
        "4141414141413a31323030030000114151774141414147414141413a3132303602000001",
        "a07aea3e1a02000001a07aea3e1a0300000530efbfbc300300000461717561ffffff",
    );

    /// Two highlights and a note; one highlight carries a colour.
    const COLOURED: &str = concat!(
        "00000000001ab1260200000000000000010100000001fe000017616e6e6f746174696f6e",
        "2e63616368652e6f626a65637401000000020100000001fe00001773617665642e61766c",
        "2e696e74657276616c2e747265650100000002fe00001d616e6e6f746174696f6e2e7065",
        "72736f6e616c2e686967686c6967687403000010415a594441414141414141413a323136",
        "03000010415a594441414156414141413a323337020000019fe1cad892020000019fe1ca",
        "d8920300000530efbfbc30fffe00001d616e6e6f746174696f6e2e706572736f6e616c2e",
        "686967686c6967687403000010415a634441414150414141413a35323703000010415a63",
        "4441414166414141413a353433020000019fe1cad892020000019fe1cad8920300000530",
        "efbfbc30030000066f72616e6765ffff0100000002fe00001773617665642e61766c2e69",
        "6e74657276616c2e747265650100000001fe000018616e6e6f746174696f6e2e70657273",
        "6f6e616c2e6e6f746503000010415a634441414150414141413a35323703000010415a63",
        "4441414166414141413a353433020000019fe1cad892020000019fe1cad8920300000530",
        "efbfbc300300000d74657374206f6e207369646c65ffffff",
    );

    /// A note body above the BMP: `writeUTF` emits a six-byte surrogate pair.
    const EMOJI_NOTE: &str = concat!(
        "00000000001ab1260200000000000000010100000001fe000017616e6e6f746174696f6e",
        "2e63616368652e6f626a65637401000000010100000002fe00001773617665642e61766c",
        "2e696e74657276616c2e747265650100000001fe000018616e6e6f746174696f6e2e7065",
        "72736f6e616c2e6e6f7465030000104163674141414141414141413a3530300300001041",
        "6367414141414b414141413a35313002000001a07aea3e1a02000001a07aea3e1a030000",
        "0530efbfbc30030000116772656174206c696e6520eda0bdedb4a5ffffff",
    );

    /// A record carrying a `char`, which is two bytes on the wire.
    const HAS_CHAR: &str = concat!(
        "00000000001ab1260200000000000000010100000002fe00000a736f6d652e73746f7265",
        "0900410100000007fffe000017616e6e6f746174696f6e2e63616368652e6f626a656374",
        "01000000010100000001fe00001773617665642e61766c2e696e74657276616c2e747265",
        "650100000001fe00001d616e6e6f746174696f6e2e706572736f6e616c2e686967686c69",
        "6768740300000f4151454141414141414141413a31300300000f41514541414141464141",
        "41413a313502000001a07aea3e1a02000001a07aea3e1a0300000530efbfbc30ffffff",
    );

    /// An `annotation.cache.object` the writer left without a count.
    const EMPTY_CACHE: &str = concat!(
        "00000000001ab1260200000000000000010100000002fe00000a666f6e742e7072656673",
        "030000165f494e56414c49445f2c756e643a626f6f6b65726c7901000000000100000002",
        "01000000000100000000010000000001000000000100000000010000000001ffffffff03",
        "01010000000003010000030101000000000301fffe000017616e6e6f746174696f6e2e63",
        "616368652e6f626a656374ff",
    );

    /// A mobi8 sidecar: bare-integer anchors, and four roots before the cache.
    const INT_ANCHOR: &str = concat!(
        "00000000001ab1260200000000000000010100000005fe00000a666f6e742e7072656673",
        "030000165f494e56414c49445f2c756e643a626f6f6b65726c7901000000000100000002",
        "01000000000100000000010000000001000000000100000000010000000001ffffffff03",
        "01010000000003010000030101000000000301fffe00000c5374617274416374696f6e73",
        "010000000103000012626f6f6b6c61756e636865646265666f72650300000474727565ff",
        "fe00000a64696374696f6e6172790300004a2f6d6e742f75732f646f63756d656e74732f",
        "64696374696f6e61726965732f4f78666f72642044696374696f6e617279206f6620456e",
        "676c6973685f4230303533564d4e59572e617a77fffe00000873796e635f6c70720001ff",
        "fe000017616e6e6f746174696f6e2e63616368652e6f626a656374010000000101000000",
        "01fe00001773617665642e61766c2e696e74657276616c2e747265650100000001fe0000",
        "1d616e6e6f746174696f6e2e706572736f6e616c2e686967686c69676874030000053832",
        "31303703000005383235313402000001a07aea3e1a02000001a07aea3e1a0300000530ef",
        "bfbc30ffffff",
    );

    /// A PDF sidecar: `<page> <x> <y> <nBounds>` anchors, on no position axis.
    const PDF_ANCHOR: &str = concat!(
        "00000000001ab1260200000000000000010100000001fe000017616e6e6f746174696f6e",
        "2e63616368652e6f626a65637401000000010100000001fe00001773617665642e61766c",
        "2e696e74657276616c2e747265650100000001fe00001d616e6e6f746174696f6e2e7065",
        "72736f6e616c2e686967686c696768740300000b33203130302032303020300300000b33",
        "2034383020323630203002000001a07aea3e1a02000001a07aea3e1a0300000530efbfbc",
        "30ffffff",
    );

    /// 5.19's colour vocabulary, and an underline beside the highlights.
    const SCRIBE: &str = concat!(
        "00000000001ab1260200000000000000010100000001fe000017616e6e6f746174696f6e",
        "2e63616368652e6f626a65637401000000020100000001fe00001773617665642e61766c",
        "2e696e74657276616c2e747265650100000003fe00001d616e6e6f746174696f6e2e7065",
        "72736f6e616c2e686967686c69676874030000114157554141414141414141413a313030",
        "3003000011415755414141416f414141413a3130343002000001a07aea3e1a02000001a0",
        "7aea3e1a0300000530efbfbc3003000005677265656efffe00001d616e6e6f746174696f",
        "6e2e706572736f6e616c2e686967686c6967687403000011415759414141414141414141",
        "3a3230303003000011415759414141416f414141413a3230343002000001a07aea3e1a02",
        "000001a07aea3e1a0300000530efbfbc300300000764656661756c74fffe00001d616e6e",
        "6f746174696f6e2e706572736f6e616c2e686967686c6967687403000011415763414141",
        "4141414141413a3330303003000011415763414141416f414141413a3330343002000001",
        "a07aea3e1a02000001a07aea3e1a0300000530efbfbc30030000096461726b5f626c7565",
        "ffff010000000cfe00001773617665642e61766c2e696e74657276616c2e747265650100",
        "000001fe00001d616e6e6f746174696f6e2e706572736f6e616c2e756e6465726c696e65",
        "030000114157674141414141414141413a3430303003000011415767414141416f414141",
        "413a3430343002000001a07aea3e1a02000001a07aea3e1a0300000530efbfbc30ffffff",
    );

    /// A fixture's bytes.
    fn fixture(hexed: &str) -> Vec<u8> {
        (0..hexed.len())
            .step_by(2)
            .map(|at| u8::from_str_radix(&hexed[at..at + 2], 16).expect("two hex digits"))
            .collect()
    }

    /// The annotations one fixture holds.
    fn marks(hexed: &str) -> Vec<Annotation> {
        let store = Store::parse(&fixture(hexed)).expect("a sidecar");
        annotations(store.root(ANNOTATION_CACHE).expect("an annotation cache"))
    }

    /// A KRDS container holding `records`, each a name and a list of longs.
    fn sidecar(records: &[(&str, &[i64])]) -> Vec<u8> {
        let mut out = Vec::from(SIGNATURE);
        out.push(T_LONG);
        out.extend_from_slice(&1i64.to_be_bytes());
        out.push(T_INT);
        out.extend_from_slice(&(records.len() as i32).to_be_bytes());
        for (name, values) in records {
            out.push(T_OBJECT_BEGIN);
            out.push(0);
            out.extend_from_slice(&(name.len() as u16).to_be_bytes());
            out.extend_from_slice(name.as_bytes());
            for v in *values {
                out.push(T_LONG);
                out.extend_from_slice(&v.to_be_bytes());
            }
            out.push(T_OBJECT_END);
        }
        out
    }

    /// The 32-hex profile infix a real sidecar carries between the book's stem
    /// and the suffix.
    const INFIX: &str = "c0354bae47758639f54c3d3efca2d41a";

    /// A `.sdr` under `root` holding `bytes` under `suffix`.
    fn lay_out(root: &Path, file: &str, suffix: &str, bytes: &[u8]) {
        let sdr = root.join(format!("{file}.sdr"));
        std::fs::create_dir_all(&sdr).expect("a sidecar directory");
        std::fs::write(sdr.join(format!("{file}{INFIX}{suffix}")), bytes).expect("the sidecar");
    }

    fn scratch(name: &str) -> PathBuf {
        let at = std::env::temp_dir().join(format!("readinglog-sidecar-{name}"));
        let _ = std::fs::remove_dir_all(&at);
        std::fs::create_dir_all(&at).expect("a scratch directory");
        at
    }

    #[test]
    fn a_sidecar_states_the_counter_the_log_states_on_every_turn() {
        let bytes = sidecar(&[("timer.model", &[0, 3_198_932, 30_965])]);
        let store = Store::parse(&bytes).expect("a sidecar");
        assert_eq!(counters(&store), Some((3_198_932, 30_965)));
    }

    #[test]
    fn a_record_the_file_does_not_hold_is_not_an_error() {
        // A book opened and never read: `lpr` alone, no timer.
        let bytes = sidecar(&[("lpr", &[0])]);
        let store = Store::parse(&bytes).expect("a sidecar");
        assert_eq!(counters(&store), None);
    }

    #[test]
    fn a_file_that_is_not_a_sidecar_is_refused_rather_than_read() {
        assert_eq!(Store::parse(b"not a sidecar"), Err(KrdsError::NotASidecar));
        let mut short = Vec::from(SIGNATURE);
        short.push(T_LONG);
        assert!(matches!(
            Store::parse(&short),
            Err(KrdsError::Truncated { .. })
        ));
    }

    #[test]
    fn a_char_is_two_bytes_and_everything_after_one_stays_in_place() {
        // `ObjectWriter.writeChar` emits the type byte and then
        // `DataOutputStream.writeChar`, which is a UTF-16 code unit. Reading
        // one byte shifts the rest of the file by one and loses all of it.
        let store = Store::parse(&fixture(HAS_CHAR)).expect("a sidecar");
        let held = store.root("some.store").expect("the record with the char");
        assert_eq!(held.values[0], Value::Char(u16::from(b'A')));
        assert_eq!(held.values[1], Value::Int(7));
        // And the record after it still reads.
        assert_eq!(marks(HAS_CHAR).len(), 1);
    }

    #[test]
    fn a_note_body_above_the_bmp_costs_nothing() {
        // `writeUTF` is Java *modified* UTF-8: an astral character is a
        // six-byte surrogate pair, which `String::from_utf8` refuses. A note
        // body is the reader's own words, so one emoji reaches this.
        let held = marks(EMOJI_NOTE);
        assert_eq!(held.len(), 1);
        assert_eq!(held[0].kind, Kind::Note);
        assert_eq!(held[0].body, "great line \u{1F525}");
    }

    #[test]
    fn a_modified_utf8_string_reads_where_a_plain_one_would_not() {
        // `U+0000` is `C0 80`, never a bare zero byte.
        assert_eq!(modified_utf8(b"\xc0\x80a").as_deref(), Some("\0a"));
        assert_eq!(
            modified_utf8("a é 漢".as_bytes()).as_deref(),
            Some("a é 漢")
        );
        // A four-byte sequence is not something the writer emits, and a lone
        // continuation byte is a torn string.
        assert_eq!(modified_utf8(&[0xF0, 0x9F, 0x94, 0xA5]), None);
        assert_eq!(modified_utf8(&[0x80]), None);
        // A lone surrogate is a character Java holds and Rust cannot; it costs
        // itself and not the file.
        assert_eq!(
            modified_utf8(&[0xED, 0xA0, 0xBD]).as_deref(),
            Some("\u{FFFD}")
        );
    }

    #[test]
    fn every_record_the_writers_name_is_read_under_its_own_kind() {
        let held = marks(KINDS);
        let kinds: Vec<Kind> = held.iter().map(|a| a.kind).collect();
        assert_eq!(kinds.len(), Kind::ALL.len());
        for kind in Kind::ALL {
            assert!(kinds.contains(&kind), "{kind:?} was not read");
        }
        // The sixth field is the subclass's, and which subclass wrote it is
        // the record's own name: a colour on the five that ask the device
        // whether it has one, the reader's words on a note, and nothing at all
        // on the three that write no sixth field.
        let of = |kind: Kind| held.iter().find(|a| a.kind == kind).expect("a record");
        assert_eq!(of(Kind::Highlight).colour, "yellow");
        assert_eq!(of(Kind::Bookmark).colour, "dark_blue");
        assert_eq!(of(Kind::Circle).colour, "green");
        assert_eq!(of(Kind::Note).body, "a note");
        assert_eq!(of(Kind::Note).colour, "", "a note states no colour");
        assert_eq!(of(Kind::Asterisk).colour, "");
        assert_eq!(of(Kind::Asterisk).body, "");
        // A handwritten note's sixth field is the id its ink is filed under,
        // which is neither the words nor a colour.
        assert_eq!(of(Kind::Handwriting).body, "");
        assert_eq!(of(Kind::Handwriting).colour, "");
    }

    #[test]
    fn a_record_states_its_place_and_its_two_instants() {
        let held = marks(COLOURED);
        assert_eq!(held.len(), 3);
        let first = &held[0];
        assert_eq!(first.kind, Kind::Highlight);
        assert_eq!(first.start_position(), Some(216));
        assert_eq!(first.end_position(), Some(237));
        assert_eq!(first.created, 1_786_199_595_154);
        assert_eq!(first.modified, first.created);
        assert_eq!(first.second(), 1_786_199_595);
        assert_eq!(first.flags, "0\u{FFFC}0");
    }

    #[test]
    fn an_anchor_is_read_by_its_shape_and_never_by_a_guess_at_its_stack() {
        // A bare integer is what `HTMLPosition`, `MobiPosition` and
        // `TopazPosition` write.
        let held = marks(INT_ANCHOR);
        assert_eq!(held.len(), 1);
        assert_eq!(held[0].start_position(), Some(82_107));
        assert_eq!(held[0].end_position(), Some(82_514));
        // `PDFPosition` writes a page and a point, which is on no position
        // axis at all. It states none rather than a number off another axis.
        let held = marks(PDF_ANCHOR);
        assert_eq!(held.len(), 1);
        assert_eq!(held[0].start, "3 100 200 0");
        assert_eq!(held[0].start_position(), None);
    }

    #[test]
    fn the_colour_vocabulary_is_carried_verbatim() {
        // `AnnotationColor` names eleven on 5.19. Nothing here reads them, so
        // whatever the record states is what is stored.
        let held = marks(SCRIBE);
        let colours: Vec<&str> = held.iter().map(|a| a.colour.as_str()).collect();
        assert_eq!(colours, ["green", "default", "dark_blue", ""]);
        assert_eq!(held[3].kind, Kind::Underline);
    }

    #[test]
    fn an_empty_cache_is_the_writer_saying_the_book_carries_none() {
        // `AnnotationCacheObject.a` returns before writing its count when no
        // tree is left. The record is there, holding nothing — which is what
        // lets an absence be read as a deletion.
        let store = Store::parse(&fixture(EMPTY_CACHE)).expect("a sidecar");
        let cache = store.root(ANNOTATION_CACHE).expect("the record");
        assert!(cache.values.is_empty());
        assert!(annotations(cache).is_empty());
    }

    #[test]
    fn every_stack_is_read_and_the_book_need_not_be_there() {
        let root = scratch("stacks");
        let counters = sidecar(&[("timer.model", &[0, 1_000, 20])]);
        lay_out(&root, "a-kfx-book", ".yjf", &counters);
        lay_out(&root, "a-mobi8-book", ".azw3f", &counters);
        // One folder down, and with no book file beside it: a deleted book.
        std::fs::create_dir_all(root.join("Sidle")).expect("a folder");
        lay_out(&root.join("Sidle"), "a deleted book", ".yjf", &counters);
        let found = read(&root, true);
        let names: Vec<&str> = found.counters.iter().map(|c| c.file.as_str()).collect();
        assert_eq!(names, ["a deleted book", "a-kfx-book", "a-mobi8-book"]);
        assert!(
            found
                .counters
                .iter()
                .all(|c| c.total_ms == 1_000 && c.words == 20)
        );
        // Every `.sdr` answers, and one holding no rare file says nothing.
        assert_eq!(found.rosters.len(), 3);
        assert!(found.rosters.iter().all(|r| !r.trusted));
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn a_superseded_sidecar_is_never_taken_for_the_live_one() {
        let root = scratch("superseded");
        lay_out(
            &root,
            "book",
            ".yjf",
            &sidecar(&[("timer.model", &[0, 7, 8])]),
        );
        let sdr = root.join("book.sdr");
        std::fs::write(
            sdr.join("book-old.yjf.bad_file"),
            sidecar(&[("timer.model", &[0, 999, 999])]),
        )
        .expect("the superseded file");
        // A superseded rare file parses and holds an empty cache; taking one
        // for the live sidecar would state that book carries nothing.
        std::fs::write(sdr.join("book.yjr.bad_file"), fixture(EMPTY_CACHE))
            .expect("the superseded rare file");
        std::fs::write(sdr.join("book.yjr.tmp"), fixture(EMPTY_CACHE)).expect("a half-written one");
        let found = read(&root, true);
        assert_eq!(found.counters.len(), 1);
        assert_eq!(found.counters[0].total_ms, 7);
        assert_eq!(found.rosters.len(), 1);
        assert!(
            !found.rosters[0].trusted,
            "a superseded file states nothing about what exists"
        );
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn a_damaged_sidecar_costs_only_itself() {
        let root = scratch("damaged");
        lay_out(
            &root,
            "good",
            ".yjf",
            &sidecar(&[("timer.model", &[0, 5, 6])]),
        );
        lay_out(
            &root,
            "torn",
            ".yjf",
            b"\x00\x00\x00\x00\x00\x1a\xb1\x26\x02",
        );
        let found = read(&root, true);
        assert_eq!(found.counters.len(), 1, "{found:?}");
        assert_eq!(found.counters[0].file, "good");
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn a_directory_holding_two_rare_files_takes_the_profiles_own() {
        // `ReaderPermanentState.a`'s migration leaves the bare-named file
        // beside the profile-named one, and a second reading profile writes
        // its own. Never construct the name — glob, and choose.
        let root = scratch("two-rare");
        let sdr = root.join("Timequake.sdr");
        std::fs::create_dir_all(&sdr).expect("a sidecar directory");
        std::fs::write(sdr.join("Timequake.yjr"), fixture(EMPTY_CACHE)).expect("the bare one");
        std::fs::write(sdr.join(format!("Timequake{INFIX}.yjr")), fixture(COLOURED))
            .expect("the profile's own");
        // A second book, so the shelf's own profile is the one to prefer.
        lay_out(&root, "Another", ".yjr", &fixture(KINDS));
        let found = read(&root, true);
        let held = found
            .rosters
            .iter()
            .find(|r| r.file == "Timequake")
            .expect("the book");
        assert!(held.trusted);
        assert_eq!(held.annotations.len(), 3, "the profile-named file was read");
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn the_profile_is_whichever_infix_most_of_the_shelf_carries() {
        let root = scratch("profiles");
        for book in ["one", "two", "three"] {
            lay_out(&root, book, ".yjr", &fixture(EMPTY_CACHE));
        }
        let dirs = {
            let mut out = Vec::new();
            collect(&root, 0, &mut out);
            out
        };
        assert_eq!(profile_of(&dirs).as_deref(), Some(INFIX));
        assert_eq!(profile_of(&[]), None);
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn a_name_states_its_profile_only_where_it_carries_one() {
        assert_eq!(
            infix_of(&format!("A Book{INFIX}.yjr")).as_deref(),
            Some(INFIX)
        );
        assert_eq!(infix_of("A Book.yjr"), None);
        // 32 characters, but not hex.
        assert_eq!(infix_of("A Bookzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzz.yjr"), None);
    }

    #[test]
    fn a_documents_directory_that_is_not_there_is_no_sidecars() {
        let found = read(Path::new("/nonexistent/documents"), true);
        assert!(found.counters.is_empty());
        assert!(found.rosters.is_empty());
    }
}
