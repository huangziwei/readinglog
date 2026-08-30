//! Where the log lines are on the device, and how they are read off it.
//!
//! `LIVE_LOG` is appended to continuously and holds the sitting in progress.
//! `tinyrot` gzips it into [`LOG_DIR`] on a size cap and prunes the oldest.
//! `log_backup.sh` gzips a daily snapshot into [`DUMP_DIR`], where about a
//! month accumulates. `showlog` reads the first two:
//!
//! ```text
//! ALLFILES=`ls -1 $ARCHIVE_DIR/${LOG}_*.gz | xargs`
//! cat $ALLFILES | zcat >> "$OUTFILE"
//! cat /var/log/$LOG >> "$OUTFILE"
//! ```
//!
//! `/var/log` is a directory on the tmpfs; `/var/local` is a symlink to the
//! `/var/base-local` flash mount, and no file named `messages` sits in
//! [`LOG_DIR`] beside the rotated chunks.
//!
//! All three overlap. Every pass sorts and de-duplicates what it took.

use std::io::Read;
use std::path::{Path, PathBuf};

/// The live syslog, on the root filesystem's tmpfs.
pub const LIVE_LOG: &str = "/var/log/messages";

/// The directory `tinyrot` gzips [`LIVE_LOG`]'s rotated chunks into, on flash.
pub const LOG_DIR: &str = "/var/local/log";

/// What a rotated chunk's name begins with:
/// `messages_00000807_20260807101501.gz`.
const CHUNK_PREFIX: &str = "messages_";

/// Where the firmware keeps its daily snapshots.
pub const DUMP_DIR: &str = "/mnt/us/system/logbackup";

/// What a daily snapshot's name begins with: `log_backup_260807101501.gz`.
const DUMP_PREFIX: &str = "log_backup_";

/// What one pass took, and from where.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct Sources {
    pub live: usize,
    pub chunks: usize,
    pub dumps: usize,
    /// Files passed over on their name alone, having nothing past the
    /// watermark.
    pub skipped: usize,
    /// Files that decoded only partway, giving up their intact prefix.
    pub truncated: usize,
}

/// The event lines a pass took, ordered and de-duplicated, and where from.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct Collected {
    pub lines: Vec<String>,
    pub from: Sources,
}

/// Every marker line at or after `watermark`, across the three sources.
///
/// `watermark` is `YYMMDD:HHMMSS`, the shape a log line begins with and a
/// filename encodes; every comparison here is a plain string ordering. An empty
/// one reads every file.
///
/// `on` takes the count of files opened and the count to open.
pub fn collect_from(
    live: &Path,
    log_dir: &Path,
    dump_dir: &Path,
    watermark: &str,
    on: &mut dyn FnMut(usize, usize),
) -> Collected {
    let mut out = Collected::default();
    let dumps = dated(
        dump_dir,
        DUMP_PREFIX,
        dump_stamp,
        watermark,
        false,
        &mut out,
    );
    let chunks = dated(
        log_dir,
        CHUNK_PREFIX,
        chunk_stamp,
        watermark,
        true,
        &mut out,
    );
    let total = dumps.len() + chunks.len() + 1;
    let mut done = 0;
    on(done, total);
    for path in dumps {
        if let Some(dump) = read_maybe_gzip(&path) {
            if !dump.complete {
                out.from.truncated += 1;
            }
            out.from.dumps += take_events(&dump.text, watermark, &mut out.lines);
        }
        done += 1;
        on(done, total);
    }
    // `LIVE_LOG` ahead of `LOG_DIR`, without tinyrot's lock: a rotation between
    // the two reads then duplicates lines into a selection that de-duplicates.
    if let Some(live) = read_maybe_gzip(live) {
        out.from.live += take_events(&live.text, watermark, &mut out.lines);
    }
    done += 1;
    on(done, total);
    for path in chunks {
        // A chunk pruned between the listing and the read yields nothing, and
        // one caught mid-rotation yields its intact prefix.
        if let Some(chunk) = read_maybe_gzip(&path) {
            out.from.chunks += take_events(&chunk.text, watermark, &mut out.lines);
        }
        done += 1;
        on(done, total);
    }
    out.lines.sort();
    out.lines.dedup();
    out
}

/// The files in `dir` worth opening, oldest first.
///
/// `straddle` keeps the newest file at or before `watermark` alongside
/// everything after it: a chunk name states its rotation instant, which sits
/// past its own content. A dump name states when it was taken.
///
/// An unparseable name is read, at the cost of one gunzip.
fn dated(
    dir: &Path,
    prefix: &str,
    stamp_of: fn(&str) -> Option<String>,
    watermark: &str,
    straddle: bool,
    out: &mut Collected,
) -> Vec<PathBuf> {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return Vec::new();
    };
    let mut found: Vec<(String, PathBuf)> = Vec::new();
    for entry in entries.flatten() {
        let name = entry.file_name().to_string_lossy().into_owned();
        if !name.starts_with(prefix) {
            continue;
        }
        found.push((stamp_of(&name).unwrap_or_default(), entry.path()));
    }
    found.sort();
    if watermark.is_empty() {
        return found.into_iter().map(|(_, p)| p).collect();
    }
    let first = match straddle {
        true => found
            .iter()
            .rposition(|(stamp, _)| stamp.as_str() <= watermark)
            .unwrap_or(0),
        false => found
            .iter()
            .position(|(stamp, _)| stamp.as_str() > watermark)
            .unwrap_or(found.len()),
    };
    out.from.skipped += first;
    found.split_off(first).into_iter().map(|(_, p)| p).collect()
}

/// `log_backup_260807101501.gz` → `260807:101501`.
fn dump_stamp(name: &str) -> Option<String> {
    let digits: String = name
        .strip_prefix(DUMP_PREFIX)?
        .chars()
        .take_while(char::is_ascii_digit)
        .collect();
    (digits.len() == 12).then(|| format!("{}:{}", &digits[..6], &digits[6..]))
}

/// `messages_00000807_20260807101501.gz` → `260807:101501`.
fn chunk_stamp(name: &str) -> Option<String> {
    let digits: String = name
        .strip_prefix(CHUNK_PREFIX)?
        .rsplit_once('.')?
        .0
        .rsplit_once('_')?
        .1
        .chars()
        .take_while(char::is_ascii_digit)
        .collect();
    (digits.len() == 14).then(|| format!("{}:{}", &digits[2..8], &digits[8..]))
}

/// Append every marker line in `text` at or after `watermark`, and answer with
/// how many that was.
///
/// At or after, not past: `watermark` names the first line a pass re-reads.
fn take_events(text: &str, watermark: &str, out: &mut Vec<String>) -> usize {
    let before = out.len();
    for line in text.lines() {
        if !super::MARKERS.iter().any(|m| line.contains(m)) {
            continue;
        }
        // A line the parser cannot stamp is kept here and dropped there.
        match super::line::line_stamp(line) {
            Some(stamp) if !watermark.is_empty() && stamp < watermark => continue,
            _ => out.push(line.to_string()),
        }
    }
    out.len() - before
}

/// A decoded log file, and whether it decoded to the end.
struct Decoded {
    text: String,
    /// False on a truncated decode, leaving `text` a prefix.
    complete: bool,
}

/// Decode a log file, gunzipping a gzipped one, and keep a truncated decode's
/// intact prefix.
///
/// `log_backup.sh` gzips a dump while a pass reads it, and that same file
/// decodes to the end a minute later under the same name.
///
/// Lossy, never `read_to_string`: the syslog carries bytes that are not valid
/// UTF-8, on which a strict decode fails for the whole file.
fn read_maybe_gzip(path: &Path) -> Option<Decoded> {
    let bytes = std::fs::read(path).ok()?;
    // An empty file is a created-and-unwritten dump.
    if bytes.is_empty() {
        return None;
    }
    if bytes.starts_with(&[0x1f, 0x8b]) {
        let mut buf = Vec::new();
        let complete = flate2::read::GzDecoder::new(&bytes[..])
            .read_to_end(&mut buf)
            .is_ok();
        return (!buf.is_empty()).then(|| Decoded {
            text: String::from_utf8_lossy(&buf).into_owned(),
            complete,
        });
    }
    Some(Decoded {
        text: String::from_utf8_lossy(&bytes).into_owned(),
        complete: true,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    const PAGE: &str = "260807:101501 cvm[6144]: I ReadingTimerController:Information::NextPage,TotalTime:7390020,CurrentPos:YJPosition: A:1,EndPos:YJPosition: B:148207,PosLeft:1;";
    const LATER: &str = "260807:120000 cvm[6144]: I ReadingTimerController:Information::NextPage,TotalTime:7400020,CurrentPos:YJPosition: A:2,EndPos:YJPosition: B:148207,PosLeft:1;";
    const NOISE: &str = "260807:101502 kernel: I mmc0: something entirely unrelated";

    fn tmp(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("readinglog-source-{name}"));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("a scratch directory");
        dir
    }

    #[test]
    fn a_name_gives_up_the_instant_it_encodes() {
        assert_eq!(
            dump_stamp("log_backup_260807101501.gz").as_deref(),
            Some("260807:101501")
        );
        assert_eq!(
            chunk_stamp("messages_00000807_20260807101501.gz").as_deref(),
            Some("260807:101501")
        );
        assert_eq!(dump_stamp("log_backup_short.gz"), None);
        assert_eq!(chunk_stamp("messages"), None);
    }

    #[test]
    fn only_marker_lines_are_taken_and_only_from_the_watermark_on() {
        let text = [PAGE, NOISE, LATER].join("\n");
        let mut out = Vec::new();
        assert_eq!(take_events(&text, "", &mut out), 2);
        out.clear();
        // At the watermark, not past it: a sitting open when it was written has
        // to be measured from its own start again.
        assert_eq!(take_events(&text, "260807:101501", &mut out), 2);
        out.clear();
        assert_eq!(take_events(&text, "260807:110000", &mut out), 1);
        assert_eq!(out, vec![LATER.to_string()]);
    }

    #[test]
    fn a_pass_reports_each_file_it_opens_against_the_count_to_open() {
        let dir = tmp("progress");
        let (log_dir, dump_dir) = (dir.join("log"), dir.join("dumps"));
        std::fs::create_dir_all(&log_dir).unwrap();
        std::fs::create_dir_all(&dump_dir).unwrap();
        let live = dir.join("messages");
        std::fs::write(&live, format!("{PAGE}\n")).unwrap();
        std::fs::write(log_dir.join("messages_00000807_20260807101501.gz"), PAGE).unwrap();
        std::fs::write(dump_dir.join("log_backup_260807101501.gz"), PAGE).unwrap();
        std::fs::write(dump_dir.join("log_backup_260808101501.gz"), PAGE).unwrap();

        let mut seen: Vec<(usize, usize)> = Vec::new();
        collect_from(&live, &log_dir, &dump_dir, "", &mut |done, total| {
            seen.push((done, total))
        });
        // Two dumps, one chunk and the live log.
        assert_eq!(seen, vec![(0, 4), (1, 4), (2, 4), (3, 4), (4, 4)]);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn the_three_sources_are_read_and_de_duplicated() {
        let dir = tmp("collect");
        let (log_dir, dump_dir) = (dir.join("log"), dir.join("dumps"));
        std::fs::create_dir_all(&log_dir).unwrap();
        std::fs::create_dir_all(&dump_dir).unwrap();
        let live = dir.join("messages");
        // The same line in all three, plus one only the live log has.
        std::fs::write(&live, format!("{PAGE}\n{LATER}\n")).unwrap();
        std::fs::write(log_dir.join("messages_00000807_20260807101501.gz"), PAGE).unwrap();
        std::fs::write(dump_dir.join("log_backup_260807101501.gz"), PAGE).unwrap();

        let got = collect_from(&live, &log_dir, &dump_dir, "", &mut |_, _| {});
        assert_eq!(got.lines, vec![PAGE.to_string(), LATER.to_string()]);
        assert_eq!(got.from.live, 2);
        assert_eq!(got.from.chunks, 1);
        assert_eq!(got.from.dumps, 1);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_dump_stamped_at_or_before_the_watermark_is_never_opened() {
        let dir = tmp("skip");
        let (log_dir, dump_dir) = (dir.join("log"), dir.join("dumps"));
        std::fs::create_dir_all(&log_dir).unwrap();
        std::fs::create_dir_all(&dump_dir).unwrap();
        // A snapshot holds everything up to the instant its name encodes, so
        // the newer one carries the older one's lines as well as its own.
        let newer = PAGE.replace("260807:101501", "260809:101501");
        std::fs::write(dump_dir.join("log_backup_260807101501.gz"), PAGE).unwrap();
        std::fs::write(
            dump_dir.join("log_backup_260809101501.gz"),
            format!("{PAGE}\n{newer}\n"),
        )
        .unwrap();

        let got = collect_from(
            &dir.join("nothing"),
            &log_dir,
            &dump_dir,
            "260808:000000",
            &mut |_, _| {},
        );
        assert_eq!(got.from.skipped, 1);
        // The older dump was never opened, and the older line inside the newer
        // one was dropped by its own stamp.
        assert_eq!(got.lines, vec![newer]);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn the_chunk_straddling_the_watermark_is_still_opened() {
        let dir = tmp("straddle");
        let (log_dir, dump_dir) = (dir.join("log"), dir.join("dumps"));
        std::fs::create_dir_all(&log_dir).unwrap();
        std::fs::create_dir_all(&dump_dir).unwrap();
        // Rotated at 11:00: it holds the 10:15 line. The watermark is later.
        std::fs::write(log_dir.join("messages_00000807_20260807110000.gz"), PAGE).unwrap();
        std::fs::write(log_dir.join("messages_00000807_20260807130000.gz"), LATER).unwrap();

        let got = collect_from(
            &dir.join("nothing"),
            &log_dir,
            &dump_dir,
            "260807:120000",
            &mut |_, _| {},
        );
        assert_eq!(got.from.skipped, 0);
        assert_eq!(got.lines, vec![LATER.to_string()]);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_gzipped_file_and_a_plain_one_read_the_same() {
        use std::io::Write as _;
        let dir = tmp("gzip");
        let mut enc = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::fast());
        enc.write_all(PAGE.as_bytes()).unwrap();
        let gz = dir.join("gz");
        std::fs::write(&gz, enc.finish().unwrap()).unwrap();
        let plain = dir.join("plain");
        std::fs::write(&plain, PAGE).unwrap();

        let a = read_maybe_gzip(&gz).expect("a gzipped file");
        let b = read_maybe_gzip(&plain).expect("a plain file");
        assert_eq!(a.text, b.text);
        assert!(a.complete && b.complete);
        // An empty file is a created-and-unwritten dump, not a decode failure.
        let empty = dir.join("empty");
        std::fs::write(&empty, b"").unwrap();
        assert!(read_maybe_gzip(&empty).is_none());
        let _ = std::fs::remove_dir_all(&dir);
    }
}
