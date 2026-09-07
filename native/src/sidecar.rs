//! `.sdr` sidecar directories: the KRDS container, and `timer.model`'s counters.

use std::fmt;
use std::path::{Path, PathBuf};

/// The directory holding book files and their `.sdr` directories.
pub const DOCUMENTS_DIR: &str = "/mnt/us/documents";

/// The suffix a sidecar directory carries.
const SDR: &str = ".sdr";

/// The frequently updated sidecar's suffix, one per reader stack: KFX, mobi8,
/// Mobipocket, Topaz, PDF. `timer.model` is in this half.
const FREQUENT: [&str; 5] = [".yjf", ".azw3f", ".mbs", ".tas", ".pdt"];

/// The suffix marking a superseded sidecar.
const SUPERSEDED: &str = ".bad_file";

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
    Char(u8),
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
        String::from_utf8(raw.to_vec())
            .map(Some)
            .map_err(|_| KrdsError::BadUtf8 { at })
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
            T_CHAR => Value::Char(self.u8()?),
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

/// Every sidecar under `documents`, book file present or not. Empty where
/// `documents` does not exist; a sidecar that will not parse is printed and
/// skipped.
pub fn read(documents: &Path) -> Vec<Counter> {
    let mut out = Vec::new();
    collect(documents, 0, &mut out);
    out.sort_by(|a, b| a.file.cmp(&b.file));
    out
}

fn collect(dir: &Path, depth: usize, out: &mut Vec<Counter>) {
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
                if let Some(counter) = read_one(&path, &name[..name.len() - SDR.len()]) {
                    out.push(counter);
                }
            }
            _ => collect(&path, depth + 1, out),
        }
    }
}

/// The counters in `sdr`'s frequent sidecar, or `None` where it holds none.
fn read_one(sdr: &Path, file: &str) -> Option<Counter> {
    let at = frequent(sdr)?;
    let bytes = match std::fs::read(&at) {
        Ok(bytes) => bytes,
        Err(err) => {
            eprintln!("sidecar: {} — {err}", at.display());
            return None;
        }
    };
    let store = match Store::parse(&bytes) {
        Ok(store) => store,
        Err(err) => {
            eprintln!("sidecar: {} — {err}", at.display());
            return None;
        }
    };
    let (total_ms, words) = counters(&store)?;
    Some(Counter {
        file: file.to_string(),
        total_ms,
        words,
    })
}

/// The frequent sidecar inside `sdr`, matched on a [`FREQUENT`] suffix.
fn frequent(sdr: &Path) -> Option<PathBuf> {
    let entries = std::fs::read_dir(sdr).ok()?;
    entries.flatten().map(|e| e.path()).find(|p| {
        let Some(name) = p.file_name().and_then(|n| n.to_str()) else {
            return false;
        };
        !name.ends_with(SUPERSEDED) && FREQUENT.iter().any(|s| name.ends_with(s))
    })
}

#[cfg(test)]
mod tests {
    use super::*;

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

    /// A `.sdr` under `root` holding `bytes` as its frequent sidecar.
    fn lay_out(root: &Path, file: &str, suffix: &str, bytes: &[u8]) {
        let sdr = root.join(format!("{file}.sdr"));
        std::fs::create_dir_all(&sdr).expect("a sidecar directory");
        // The 32-hex profile infix between the stem and the suffix.
        let infix = "c0354bae47758639f54c3d3efca2d41a";
        std::fs::write(sdr.join(format!("{file}{infix}{suffix}")), bytes).expect("the sidecar");
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
    fn every_stack_is_read_and_the_book_need_not_be_there() {
        let root = scratch("stacks");
        let counters = sidecar(&[("timer.model", &[0, 1_000, 20])]);
        lay_out(&root, "a-kfx-book", ".yjf", &counters);
        lay_out(&root, "a-mobi8-book", ".azw3f", &counters);
        // One folder down, and with no book file beside it: a deleted book.
        std::fs::create_dir_all(root.join("Sidle")).expect("a folder");
        lay_out(&root.join("Sidle"), "a deleted book", ".yjf", &counters);
        let found = read(&root);
        let names: Vec<&str> = found.iter().map(|c| c.file.as_str()).collect();
        assert_eq!(names, ["a deleted book", "a-kfx-book", "a-mobi8-book"]);
        assert!(found.iter().all(|c| c.total_ms == 1_000 && c.words == 20));
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
        let found = read(&root);
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].total_ms, 7);
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
        let found = read(&root);
        assert_eq!(found.len(), 1, "{found:?}");
        assert_eq!(found[0].file, "good");
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn a_documents_directory_that_is_not_there_is_no_sidecars() {
        assert!(read(Path::new("/nonexistent/documents")).is_empty());
    }
}
