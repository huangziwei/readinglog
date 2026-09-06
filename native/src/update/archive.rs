//! The zip reader [`unpack`] lands a release archive with: stored and
//! deflated entries only, no zip64. Sizes come from the central directory,
//! and a streaming zipper's empty local headers read the same as any other.

use std::fs::{self, File};
use std::io::{self, Read, Seek, SeekFrom, Write as _};
use std::path::{Component, Path, PathBuf};

/// End of central directory, central directory entry, local file header.
const EOCD_SIG: u32 = 0x0605_4b50;
const CD_SIG: u32 = 0x0201_4b50;
const LOCAL_SIG: u32 = 0x0403_4b50;

/// Fixed-size part of each of the three records.
const EOCD_LEN: usize = 22;
const CD_LEN: usize = 46;
const LOCAL_LEN: usize = 30;

/// How far back from the end to look for [`EOCD_SIG`].
const EOCD_SEARCH: usize = 64 * 1024 + EOCD_LEN;

/// The `0xFFFFFFFF` a field takes under zip64. Refused.
const ZIP64_MARK: u32 = 0xFFFF_FFFF;

/// Compression methods this reads.
const STORED: u16 = 0;
const DEFLATED: u16 = 8;

/// What [`write`] states it needs and was made by: zip 2.0, which is stored
/// and deflated entries with no zip64.
const VERSION: u16 = 20;

/// The modification time [`write`] stamps every entry with: midnight on
/// 1980-01-01, the zero of the DOS clock. The archive's own name carries the
/// date that means anything.
const DOS_TIME: u16 = 0;
const DOS_DATE: u16 = 0b0000_0000_0010_0001;

/// Ceiling on one entry's uncompressed size. Past this the unpack stops.
const MAX_ENTRY: u64 = 64 * 1024 * 1024;

#[derive(Debug)]
pub enum Error {
    Io(io::Error),
    /// Not a zip, or a zip this reader will not read.
    Malformed(String),
    /// Nothing inside ends in the marker the caller named.
    NoMarker(String),
    /// An entry's bytes do not match what the archive recorded for them.
    Corrupt(String),
}

impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Error::Io(e) => write!(f, "{e}"),
            Error::Malformed(s) => write!(f, "not a readable archive: {s}"),
            Error::NoMarker(m) => write!(f, "no {m} inside"),
            Error::Corrupt(s) => write!(f, "damaged download: {s}"),
        }
    }
}

impl std::error::Error for Error {}

impl From<io::Error> for Error {
    fn from(e: io::Error) -> Self {
        Error::Io(e)
    }
}

pub type Result<T> = std::result::Result<T, Error>;

/// One central-directory record.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Entry {
    /// The path as the archive spells it, `/`-separated.
    pub path: String,
    method: u16,
    crc: u32,
    compressed: u64,
    size: u64,
    /// Where this entry's local header starts.
    offset: u64,
}

impl Entry {
    /// A directory entry, which the archive marks with a trailing slash.
    pub fn is_dir(&self) -> bool {
        self.path.ends_with('/')
    }
}

/// An open archive: the file, and every entry its central directory lists.
pub struct Archive {
    file: File,
    entries: Vec<Entry>,
}

impl Archive {
    pub fn open(path: &Path) -> Result<Archive> {
        let mut file = File::open(path)?;
        let entries = read_central_directory(&mut file)?;
        Ok(Archive { file, entries })
    }

    pub fn entries(&self) -> &[Entry] {
        &self.entries
    }

    /// One entry's bytes, checked against its recorded size and CRC.
    pub fn read(&mut self, entry: &Entry) -> Result<Vec<u8>> {
        if entry.size > MAX_ENTRY {
            return Err(Error::Malformed(format!(
                "{} claims {} bytes",
                entry.path, entry.size
            )));
        }

        let mut header = [0u8; LOCAL_LEN];
        self.file.seek(SeekFrom::Start(entry.offset))?;
        self.file.read_exact(&mut header)?;
        if u32le(&header, 0) != LOCAL_SIG {
            return Err(Error::Malformed(format!("{}: no local header", entry.path)));
        }
        // The local header's own sizes are not read: a streaming zipper
        // leaves them zero. Its two length fields locate the data.
        let names = u16le(&header, 26) as u64 + u16le(&header, 28) as u64;
        self.file
            .seek(SeekFrom::Start(entry.offset + LOCAL_LEN as u64 + names))?;

        let mut raw = vec![0u8; entry.compressed as usize];
        self.file.read_exact(&mut raw)?;

        let bytes = match entry.method {
            STORED => raw,
            DEFLATED => {
                let mut out = Vec::with_capacity(entry.size as usize);
                flate2::read::DeflateDecoder::new(&raw[..])
                    .take(MAX_ENTRY)
                    .read_to_end(&mut out)?;
                out
            }
            other => {
                return Err(Error::Malformed(format!(
                    "{}: compression method {other}",
                    entry.path
                )));
            }
        };

        if bytes.len() as u64 != entry.size {
            return Err(Error::Corrupt(format!(
                "{}: {} bytes, expected {}",
                entry.path,
                bytes.len(),
                entry.size
            )));
        }
        let mut hasher = crc32fast::Hasher::new();
        hasher.update(&bytes);
        if hasher.finalize() != entry.crc {
            return Err(Error::Corrupt(format!("{}: checksum", entry.path)));
        }
        Ok(bytes)
    }
}

/// Everything in `paths` sits under one folder; this is that folder, named by
/// the entry ending in `marker`, and `""` for an archive rooted on it. The
/// marker ends a path *component*: `sbin/readinglog` is not `bin/readinglog`.
pub fn prefix_for<'a, I: IntoIterator<Item = &'a str>>(paths: I, marker: &str) -> Option<String> {
    for path in paths {
        if path == marker {
            return Some(String::new());
        }
        if let Some(prefix) = path.strip_suffix(marker)
            && prefix.ends_with('/')
        {
            return Some(prefix.to_string());
        }
    }
    None
}

/// Every file under the root `marker` names — see [`prefix_for`] — into the
/// staging directory `dest`, returning how many. Unix modes are not carried
/// across: the caller marks what has to be executable.
pub fn unpack(zip: &Path, marker: &str, dest: &Path) -> Result<usize> {
    let mut archive = Archive::open(zip)?;
    let entries = archive.entries().to_vec();

    let prefix = prefix_for(entries.iter().map(|e| e.path.as_str()), marker)
        .ok_or_else(|| Error::NoMarker(marker.to_string()))?;

    let mut written = 0usize;
    for entry in entries.iter().filter(|e| !e.is_dir()) {
        let Some(rest) = entry.path.strip_prefix(&prefix) else {
            continue;
        };
        if rest.is_empty() {
            continue;
        }
        let out = safe_join(dest, rest).ok_or_else(|| {
            Error::Malformed(format!("{} names a path outside the folder", entry.path))
        })?;
        let bytes = archive.read(entry)?;
        if let Some(parent) = out.parent() {
            fs::create_dir_all(parent)?;
        }
        fs::write(&out, &bytes)?;
        written += 1;
    }

    if written == 0 {
        return Err(Error::Malformed("nothing to unpack".into()));
    }
    Ok(written)
}

/// What goes into an archive under one name.
pub enum Source<'a> {
    /// Bytes already in hand.
    Bytes(&'a [u8]),
    /// A file, read one entry at a time so a cache of jackets is never held
    /// whole in memory.
    File(&'a Path),
}

/// Write `entries` as an archive at `at`, stored and never deflated, through a
/// `.partial` sibling and a rename. Names are `/`-separated and taken as
/// given.
///
/// The reader above is this writer's other half: what goes in comes back out
/// of [`Archive::read`] with its checksum standing.
pub fn write(at: &Path, entries: &[(String, Source<'_>)]) -> Result<()> {
    if let Some(dir) = at.parent() {
        fs::create_dir_all(dir)?;
    }
    let partial = at.with_extension("partial");
    let result = fill(&partial, entries);
    if result.is_err() {
        let _ = fs::remove_file(&partial);
        return result;
    }
    fs::rename(&partial, at)?;
    Ok(())
}

/// [`write`]'s body, before the rename that makes the archive the real one.
fn fill(partial: &Path, entries: &[(String, Source<'_>)]) -> Result<()> {
    let mut out = io::BufWriter::new(File::create(partial)?);
    // Name, checksum, size and where its local header starts.
    let mut listed: Vec<(&str, u32, u32, u32)> = Vec::with_capacity(entries.len());
    let mut at = 0u64;

    for (name, source) in entries {
        let held;
        let bytes: &[u8] = match source {
            Source::Bytes(b) => b,
            Source::File(path) => {
                held = fs::read(path)?;
                &held
            }
        };
        let size = fits(bytes.len() as u64, name)?;
        let offset = fits(at, name)?;
        let mut hasher = crc32fast::Hasher::new();
        hasher.update(bytes);
        let crc = hasher.finalize();

        let mut header = Vec::with_capacity(LOCAL_LEN + name.len());
        put32(&mut header, LOCAL_SIG);
        put16(&mut header, VERSION);
        put16(&mut header, 0); // no flags: no encryption, sizes stated here
        put16(&mut header, STORED);
        put16(&mut header, DOS_TIME);
        put16(&mut header, DOS_DATE);
        put32(&mut header, crc);
        put32(&mut header, size);
        put32(&mut header, size);
        put16(&mut header, name.len() as u16);
        put16(&mut header, 0); // no extra field
        header.extend_from_slice(name.as_bytes());
        out.write_all(&header)?;
        out.write_all(bytes)?;

        at += header.len() as u64 + bytes.len() as u64;
        listed.push((name, crc, size, offset));
    }

    let directory = fits(at, "the central directory")?;
    let mut size = 0u64;
    for (name, crc, bytes, offset) in &listed {
        let mut record = Vec::with_capacity(CD_LEN + name.len());
        put32(&mut record, CD_SIG);
        put16(&mut record, VERSION);
        put16(&mut record, VERSION);
        put16(&mut record, 0);
        put16(&mut record, STORED);
        put16(&mut record, DOS_TIME);
        put16(&mut record, DOS_DATE);
        put32(&mut record, *crc);
        put32(&mut record, *bytes);
        put32(&mut record, *bytes);
        put16(&mut record, name.len() as u16);
        put16(&mut record, 0); // extra
        put16(&mut record, 0); // comment
        put16(&mut record, 0); // the one disk
        put16(&mut record, 0); // internal attributes
        put32(&mut record, 0); // external attributes
        put32(&mut record, *offset);
        record.extend_from_slice(name.as_bytes());
        out.write_all(&record)?;
        size += record.len() as u64;
    }

    let mut eocd = Vec::with_capacity(EOCD_LEN);
    put32(&mut eocd, EOCD_SIG);
    put16(&mut eocd, 0); // this disk
    put16(&mut eocd, 0); // the disk the directory starts on
    put16(&mut eocd, u16::try_from(listed.len()).unwrap_or(u16::MAX));
    put16(&mut eocd, u16::try_from(listed.len()).unwrap_or(u16::MAX));
    put32(&mut eocd, fits(size, "the central directory")?);
    put32(&mut eocd, directory);
    put16(&mut eocd, 0); // no comment
    out.write_all(&eocd)?;

    out.into_inner()
        .map_err(|e| Error::Io(e.into_error()))?
        .sync_all()?;
    Ok(())
}

/// `value` as the 32 bits a zip field holds, or [`Error::Malformed`] where it
/// will not fit: this writer states no zip64.
fn fits(value: u64, what: &str) -> Result<u32> {
    u32::try_from(value)
        .ok()
        .filter(|v| *v != ZIP64_MARK)
        .ok_or_else(|| Error::Malformed(format!("{what} is too large for a zip")))
}

fn put16(out: &mut Vec<u8>, value: u16) {
    out.extend_from_slice(&value.to_le_bytes());
}

fn put32(out: &mut Vec<u8>, value: u32) {
    out.extend_from_slice(&value.to_le_bytes());
}

/// `rest` under `dest`, or `None` for `/etc/x`, `../x`, and anything else
/// landing outside it.
fn safe_join(dest: &Path, rest: &str) -> Option<PathBuf> {
    let relative = Path::new(rest);
    for part in relative.components() {
        match part {
            Component::Normal(_) => {}
            _ => return None,
        }
    }
    Some(dest.join(relative))
}

/// Every entry the central directory lists, in the order it lists them.
fn read_central_directory(file: &mut File) -> Result<Vec<Entry>> {
    let len = file.seek(SeekFrom::End(0))?;
    if len < EOCD_LEN as u64 {
        return Err(Error::Malformed("too short to be a zip".into()));
    }
    let window = EOCD_SEARCH.min(len as usize);
    let mut tail = vec![0u8; window];
    file.seek(SeekFrom::Start(len - window as u64))?;
    file.read_exact(&mut tail)?;

    // Last match wins: a zip comment can hold the signature.
    let eocd = (0..=tail.len().saturating_sub(EOCD_LEN))
        .rev()
        .find(|&i| u32le(&tail, i) == EOCD_SIG)
        .ok_or_else(|| Error::Malformed("no end-of-central-directory record".into()))?;

    let count = u16le(&tail, eocd + 10) as usize;
    let cd_size = u32le(&tail, eocd + 12);
    let cd_offset = u32le(&tail, eocd + 16);
    if cd_size == ZIP64_MARK || cd_offset == ZIP64_MARK || count == 0xFFFF {
        return Err(Error::Malformed("zip64".into()));
    }

    let mut cd = vec![0u8; cd_size as usize];
    file.seek(SeekFrom::Start(cd_offset as u64))?;
    file.read_exact(&mut cd)?;

    let mut entries = Vec::with_capacity(count);
    let mut at = 0usize;
    while at + CD_LEN <= cd.len() {
        if u32le(&cd, at) != CD_SIG {
            return Err(Error::Malformed(format!("central directory at {at}")));
        }
        let method = u16le(&cd, at + 10);
        let crc = u32le(&cd, at + 16);
        let compressed = u32le(&cd, at + 20);
        let size = u32le(&cd, at + 24);
        let name_len = u16le(&cd, at + 28) as usize;
        let extra_len = u16le(&cd, at + 30) as usize;
        let comment_len = u16le(&cd, at + 32) as usize;
        let offset = u32le(&cd, at + 42);
        if compressed == ZIP64_MARK || size == ZIP64_MARK || offset == ZIP64_MARK {
            return Err(Error::Malformed("zip64".into()));
        }

        let from = at + CD_LEN;
        let name = cd
            .get(from..from + name_len)
            .ok_or_else(|| Error::Malformed("truncated central directory".into()))?;
        // Zip names are bytes; the archive is ASCII and anything else is a
        // path this cannot reproduce faithfully.
        let path = std::str::from_utf8(name)
            .map_err(|_| Error::Malformed("a name that is not text".into()))?
            .to_string();

        entries.push(Entry {
            path,
            method,
            crc,
            compressed: compressed as u64,
            size: size as u64,
            offset: offset as u64,
        });
        at = from + name_len + extra_len + comment_len;
    }

    if entries.len() != count {
        return Err(Error::Malformed(format!(
            "{} entries listed, {} found",
            count,
            entries.len()
        )));
    }
    Ok(entries)
}

fn u16le(buf: &[u8], at: usize) -> u16 {
    u16::from_le_bytes([buf[at], buf[at + 1]])
}

fn u32le(buf: &[u8], at: usize) -> u32 {
    u32::from_le_bytes([buf[at], buf[at + 1], buf[at + 2], buf[at + 3]])
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    /// A zip built the way the release archives are: a couple of folder
    /// entries, a deflated file and a stored one, all under one root.
    struct Zipper {
        body: Vec<u8>,
        directory: Vec<u8>,
        count: u16,
    }

    impl Zipper {
        fn new() -> Zipper {
            Zipper {
                body: Vec::new(),
                directory: Vec::new(),
                count: 0,
            }
        }

        fn add(&mut self, name: &str, data: &[u8], deflate: bool) {
            let mut hasher = crc32fast::Hasher::new();
            hasher.update(data);
            let crc = hasher.finalize();

            let (method, payload) = if deflate {
                let mut enc =
                    flate2::write::DeflateEncoder::new(Vec::new(), flate2::Compression::fast());
                enc.write_all(data).unwrap();
                (DEFLATED, enc.finish().unwrap())
            } else {
                (STORED, data.to_vec())
            };

            let offset = self.body.len() as u32;
            let head = |sig: u32, extra: &[u8]| {
                let mut h = Vec::new();
                h.extend_from_slice(&sig.to_le_bytes());
                h.extend_from_slice(extra);
                h
            };

            // Local header. Its sizes are left at zero, the way a streaming
            // zipper writes them; the real ones are in the central directory.
            let mut local = head(LOCAL_SIG, &[20, 0, 0x08, 0]);
            local.extend_from_slice(&method.to_le_bytes());
            local.extend_from_slice(&[0u8; 4]); // time, date
            local.extend_from_slice(&0u32.to_le_bytes()); // crc
            local.extend_from_slice(&0u32.to_le_bytes()); // compressed
            local.extend_from_slice(&0u32.to_le_bytes()); // uncompressed
            local.extend_from_slice(&(name.len() as u16).to_le_bytes());
            local.extend_from_slice(&0u16.to_le_bytes()); // extra
            assert_eq!(local.len(), LOCAL_LEN);
            self.body.extend_from_slice(&local);
            self.body.extend_from_slice(name.as_bytes());
            self.body.extend_from_slice(&payload);

            let mut central = head(CD_SIG, &[20, 0, 20, 0, 0x08, 0]);
            central.extend_from_slice(&method.to_le_bytes());
            central.extend_from_slice(&[0u8; 4]); // time, date
            central.extend_from_slice(&crc.to_le_bytes());
            central.extend_from_slice(&(payload.len() as u32).to_le_bytes());
            central.extend_from_slice(&(data.len() as u32).to_le_bytes());
            central.extend_from_slice(&(name.len() as u16).to_le_bytes());
            central.extend_from_slice(&0u16.to_le_bytes()); // extra
            central.extend_from_slice(&0u16.to_le_bytes()); // comment
            central.extend_from_slice(&0u16.to_le_bytes()); // disk
            central.extend_from_slice(&0u16.to_le_bytes()); // internal attrs
            central.extend_from_slice(&0u32.to_le_bytes()); // external attrs
            central.extend_from_slice(&offset.to_le_bytes());
            assert_eq!(central.len(), CD_LEN);
            self.directory.extend_from_slice(&central);
            self.directory.extend_from_slice(name.as_bytes());
            self.count += 1;
        }

        fn finish(self) -> Vec<u8> {
            let mut out = self.body;
            let cd_offset = out.len() as u32;
            out.extend_from_slice(&self.directory);
            out.extend_from_slice(&EOCD_SIG.to_le_bytes());
            out.extend_from_slice(&[0u8; 4]); // this disk, disk with the cd
            out.extend_from_slice(&self.count.to_le_bytes());
            out.extend_from_slice(&self.count.to_le_bytes());
            out.extend_from_slice(&(self.directory.len() as u32).to_le_bytes());
            out.extend_from_slice(&cd_offset.to_le_bytes());
            out.extend_from_slice(&0u16.to_le_bytes()); // comment
            out
        }
    }

    fn tmpdir(name: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("readinglog-archive-{name}"));
        let _ = fs::remove_dir_all(&d);
        fs::create_dir_all(&d).unwrap();
        d
    }

    /// The layout the release archive has: `extensions/` and `documents/`
    /// open at the root, with the LICENSE beside them.
    fn sample() -> Vec<u8> {
        let mut z = Zipper::new();
        z.add("extensions/", b"", false);
        z.add("extensions/readinglog/", b"", false);
        z.add("extensions/readinglog/bin/", b"", false);
        z.add(
            "extensions/readinglog/bin/readinglog",
            &vec![b'E'; 4096],
            true,
        );
        z.add(
            "extensions/readinglog/bin/readinglog.sh",
            b"#!/bin/sh\n",
            false,
        );
        z.add("extensions/readinglog/config.xml", b"<extension/>", false);
        z.add("documents/", b"", false);
        z.add("documents/ReadingLog.sh", b"#!/bin/sh\n", false);
        z.add("LICENSE", b"GPL", false);
        z.finish()
    }

    #[test]
    fn an_archive_unpacks_out_of_its_own_folder() {
        let dir = tmpdir("unpack");
        let zip = dir.join("a.zip");
        fs::write(&zip, sample()).unwrap();
        let dest = dir.join("readinglog.new");

        // The three files under `extensions/readinglog/`, and nothing else:
        // the tile and the licence sit outside the marker's own root.
        assert_eq!(unpack(&zip, "bin/readinglog", &dest).unwrap(), 3);
        assert_eq!(
            fs::read(dest.join("bin/readinglog")).unwrap(),
            vec![b'E'; 4096]
        );
        assert_eq!(fs::read(dest.join("config.xml")).unwrap(), b"<extension/>");
        assert!(!dest.join("documents").exists(), "the tile came along");
        assert!(!dest.join("LICENSE").exists());
        // The archive's own folder names are not repeated inside the target.
        assert!(!dest.join("extensions").exists());
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn an_archive_without_the_marker_writes_nothing() {
        let dir = tmpdir("marker");
        let zip = dir.join("a.zip");
        fs::write(&zip, sample()).unwrap();
        let dest = dir.join("out");

        let err = unpack(&zip, "bin/nothing", &dest).unwrap_err();
        assert!(matches!(err, Error::NoMarker(_)), "{err}");
        assert!(err.to_string().contains("bin/nothing"));
        assert!(
            !dest.exists(),
            "nothing may land before the marker is found"
        );
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_damaged_entry_is_caught_rather_than_written() {
        let dir = tmpdir("crc");
        let zip = dir.join("a.zip");
        let mut bytes = sample();
        // Flip a byte inside the deflate stream. The CRC in the central
        // directory describes what the entry should have been.
        let at = bytes.len() / 3;
        bytes[at] ^= 0xFF;
        fs::write(&zip, bytes).unwrap();

        let err = unpack(&zip, "bin/readinglog", &dir.join("out")).unwrap_err();
        assert!(
            matches!(err, Error::Corrupt(_) | Error::Io(_) | Error::Malformed(_)),
            "{err}"
        );
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_name_that_walks_out_of_the_folder_is_refused() {
        let dest = Path::new("/staging");
        assert_eq!(safe_join(dest, "bin/tool"), Some(dest.join("bin/tool")));
        assert_eq!(safe_join(dest, "../../etc/passwd"), None);
        assert_eq!(safe_join(dest, "/etc/passwd"), None);
        assert_eq!(safe_join(dest, "bin/../../out"), None);
    }

    #[test]
    fn the_root_is_whatever_holds_the_marker() {
        let release = [
            "extensions/",
            "extensions/readinglog/",
            "extensions/readinglog/bin/",
            "extensions/readinglog/bin/readinglog",
            "extensions/readinglog/config.xml",
            "documents/ReadingLog.sh",
            "LICENSE",
        ];
        assert_eq!(
            prefix_for(release, "bin/readinglog"),
            Some("extensions/readinglog/".into())
        );
        // An archive rooted on the marker has no prefix at all.
        assert_eq!(
            prefix_for(["bin/readinglog", "config.xml"], "bin/readinglog"),
            Some(String::new())
        );
        assert_eq!(prefix_for(["README.md"], "bin/readinglog"), None);
        // The marker has to end a component, not just the name.
        assert_eq!(prefix_for(["x/not-bin/readinglog"], "bin/readinglog"), None);
        // And the whole name: the launcher beside it is not the binary.
        assert_eq!(
            prefix_for(
                ["extensions/readinglog/bin/readinglog.sh"],
                "bin/readinglog"
            ),
            None
        );
    }

    #[test]
    fn something_that_is_not_a_zip_is_not_read() {
        let dir = tmpdir("nonsense");
        let zip = dir.join("a.zip");
        fs::write(&zip, b"<!DOCTYPE html>").unwrap();
        assert!(unpack(&zip, "bin/readinglog", &dir.join("out")).is_err());
        assert!(unpack(&dir.join("absent.zip"), "bin/readinglog", &dir.join("out")).is_err());
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn what_the_writer_puts_in_the_reader_takes_out() {
        let dir = tmpdir("written");
        let jacket = dir.join("cover.jpg");
        fs::write(&jacket, vec![0xABu8; 5_000]).unwrap();
        let record = b"#readinglog\t2\nm\t260906:010231\n".to_vec();

        let zip = dir.join("backup.zip");
        write(
            &zip,
            &[
                ("sessions.tsv".to_string(), Source::Bytes(&record)),
                ("covers/B00OKPCRLG.jpg".to_string(), Source::File(&jacket)),
            ],
        )
        .expect("an archive");

        let mut read = Archive::open(&zip).expect("a readable archive");
        let entries = read.entries().to_vec();
        assert_eq!(
            entries.iter().map(|e| e.path.as_str()).collect::<Vec<_>>(),
            ["sessions.tsv", "covers/B00OKPCRLG.jpg"]
        );
        assert_eq!(read.read(&entries[0]).unwrap(), record);
        assert_eq!(read.read(&entries[1]).unwrap(), vec![0xABu8; 5_000]);
        assert!(!dir.join("backup.partial").exists(), "the partial was left");
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn an_archive_of_one_entry_reads_back() {
        let dir = tmpdir("one-entry");
        let zip = dir.join("one.zip");
        write(&zip, &[("sessions.tsv".to_string(), Source::Bytes(b"x"))]).expect("an archive");

        let mut read = Archive::open(&zip).expect("a readable archive");
        let entries = read.entries().to_vec();
        assert_eq!(entries.len(), 1);
        assert_eq!(read.read(&entries[0]).unwrap(), b"x");
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn an_archive_of_nothing_still_reads_as_an_archive() {
        let dir = tmpdir("empty");
        let zip = dir.join("empty.zip");
        write(&zip, &[]).expect("an archive");
        assert!(
            Archive::open(&zip)
                .expect("a readable archive")
                .entries()
                .is_empty()
        );
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn an_entry_naming_a_file_that_is_not_there_writes_no_archive() {
        let dir = tmpdir("absent-source");
        let zip = dir.join("nope.zip");
        let missing = dir.join("gone.jpg");
        assert!(write(&zip, &[("a.jpg".to_string(), Source::File(&missing))]).is_err());
        assert!(!zip.exists(), "a failed write left an archive standing");
        assert!(!dir.join("nope.partial").exists(), "the partial was left");
        let _ = fs::remove_dir_all(&dir);
    }
}
