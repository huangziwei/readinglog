//! Where the log lines are on the device. [`LIVE_LOG`] holds the sitting in
//! progress, [`LOG_DIR`] its rotated chunks, [`DUMP_DIR`] the daily snapshots.
//! [`collect_from`] reads all three and de-duplicates.

use std::collections::BTreeSet;
use std::fs::File;
use std::io::{BufRead, BufReader};
use std::path::{Path, PathBuf};

/// The live syslog, on the root filesystem's tmpfs.
pub const LIVE_LOG: &str = "/var/log/messages";

/// The directory holding [`LIVE_LOG`]'s rotated chunks, on flash.
pub const LOG_DIR: &str = "/var/local/log";

/// What a rotated chunk's name begins with.
const CHUNK_PREFIX: &str = "messages_";

/// The directory holding the daily snapshots.
pub const DUMP_DIR: &str = "/mnt/us/system/logbackup";

/// What a daily snapshot's name begins with: `log_backup_260807101501.gz`.
const DUMP_PREFIX: &str = "log_backup_";

/// What one read takes off flash at a time.
const READ_BUF: usize = 64 * 1024;

/// The most one line may take.
const LINE_CAP: usize = 64 * 1024;

/// What one pass took, and from where.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct Sources {
    pub live: usize,
    pub chunks: usize,
    pub dumps: usize,
    /// Files passed over on their name alone.
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
/// `watermark` is `YYMMDD:HHMMSS`, the shape a line begins with and a
/// filename encodes. `on` takes opened, to open.
pub fn collect_from(
    live: &Path,
    log_dir: &Path,
    dump_dir: &Path,
    watermark: &str,
    on: &mut dyn FnMut(usize, usize),
) -> Collected {
    let mut from = Sources::default();
    let dumps = dated(
        dump_dir,
        DUMP_PREFIX,
        dump_stamp,
        watermark,
        false,
        &mut from,
    );
    let chunks = dated(
        log_dir,
        CHUNK_PREFIX,
        chunk_stamp,
        watermark,
        true,
        &mut from,
    );
    let total = dumps.len() + chunks.len() + 1;
    let mut done = 0;
    on(done, total);
    let mut lines = BTreeSet::new();
    for path in dumps {
        if let Some(took) = scan(&path, watermark, &mut lines) {
            if !took.complete {
                from.truncated += 1;
            }
            from.dumps += took.kept;
        }
        done += 1;
        on(done, total);
    }
    // `live` ahead of `chunks`; `lines` de-duplicates what both hold.
    if let Some(took) = scan(live, watermark, &mut lines) {
        from.live += took.kept;
    }
    done += 1;
    on(done, total);
    for path in chunks {
        if let Some(took) = scan(&path, watermark, &mut lines) {
            from.chunks += took.kept;
        }
        done += 1;
        on(done, total);
    }
    Collected {
        lines: lines.into_iter().collect(),
        from,
    }
}

/// The files in `dir` worth opening, oldest first. `straddle` keeps the newest
/// file at or before `watermark` too. A name `stamp_of` cannot read is kept.
fn dated(
    dir: &Path,
    prefix: &str,
    stamp_of: fn(&str) -> Option<String>,
    watermark: &str,
    straddle: bool,
    from: &mut Sources,
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
    from.skipped += first;
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

/// What one file gave up.
struct Took {
    /// Marker lines held, duplicates of another file's included.
    kept: usize,
    /// False on a truncated decode, leaving the lines before the cut.
    complete: bool,
}

/// Insert every marker line in `path` at or after `watermark` into `lines`,
/// gunzipping a gzipped file on the way. `path` is read a line at a time,
/// never held whole. `None` where `path` gave up no bytes.
fn scan(path: &Path, watermark: &str, lines: &mut BTreeSet<String>) -> Option<Took> {
    let mut file = BufReader::with_capacity(READ_BUF, File::open(path).ok()?);
    let gzipped = file
        .fill_buf()
        .is_ok_and(|head| head.starts_with(&[0x1f, 0x8b]));
    match gzipped {
        true => take_events(
            BufReader::with_capacity(READ_BUF, flate2::read::GzDecoder::new(file)),
            watermark,
            lines,
        ),
        false => take_events(file, watermark, lines),
    }
}

/// Insert every marker line `src` holds at or after `watermark`, and answer
/// with how many. At or after, not past: `watermark` is the first line
/// re-read. `None` where `src` gave up no bytes at all.
fn take_events(
    mut src: impl BufRead,
    watermark: &str,
    lines: &mut BTreeSet<String>,
) -> Option<Took> {
    let mut took = Took {
        kept: 0,
        complete: true,
    };
    let mut raw = Vec::new();
    let mut any = false;
    loop {
        match read_line(&mut src, &mut raw) {
            Ok(true) => any = true,
            Ok(false) => break,
            Err(_) => {
                took.complete = false;
                break;
            }
        }
        // `raw` carries bytes that are not UTF-8.
        let line = String::from_utf8_lossy(&raw);
        if !super::MARKERS.iter().any(|m| line.contains(m)) {
            continue;
        }
        // A line `line_stamp` cannot read is kept.
        if let Some(stamp) = super::line::line_stamp(&line)
            && !watermark.is_empty()
            && stamp < watermark
        {
            continue;
        }
        took.kept += 1;
        lines.insert(line.into_owned());
    }
    any.then_some(took)
}

/// One line of `src` into `raw`, without its ending, answering whether there
/// was one. A line running past [`LINE_CAP`] is cut there, and the rest of it
/// dropped.
fn read_line(src: &mut impl BufRead, raw: &mut Vec<u8>) -> std::io::Result<bool> {
    raw.clear();
    let mut any = false;
    let mut ended = false;
    while !ended {
        let buf = src.fill_buf()?;
        if buf.is_empty() {
            break;
        }
        any = true;
        let upto = match buf.iter().position(|b| *b == b'\n') {
            Some(at) => {
                ended = true;
                at
            }
            None => buf.len(),
        };
        let room = LINE_CAP.saturating_sub(raw.len());
        raw.extend_from_slice(&buf[..upto.min(room)]);
        src.consume(upto + usize::from(ended));
    }
    // What `str::lines` drops: the carriage return of a CRLF ending.
    if ended && raw.last() == Some(&b'\r') {
        raw.pop();
    }
    Ok(any)
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

    /// [`take_events`] over `text`, answering the lines it held.
    fn took(text: &str, watermark: &str, out: &mut BTreeSet<String>) -> usize {
        take_events(text.as_bytes(), watermark, out).map_or(0, |t| t.kept)
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
        let mut out = BTreeSet::new();
        assert_eq!(took(&text, "", &mut out), 2);
        out.clear();
        // `watermark` is the first line taken.
        assert_eq!(took(&text, "260807:101501", &mut out), 2);
        out.clear();
        assert_eq!(took(&text, "260807:110000", &mut out), 1);
        assert_eq!(Vec::from_iter(out), vec![LATER.to_string()]);
    }

    #[test]
    fn a_line_running_past_the_cap_is_cut_at_it() {
        let run = format!("{PAGE}{}", "x".repeat(LINE_CAP));
        let text = [run.as_str(), LATER].join("\n");
        let mut out = BTreeSet::new();
        // `run` is held to `LINE_CAP`, and `LATER` behind it stays whole.
        assert_eq!(took(&text, "", &mut out), 2);
        assert!(out.iter().any(|line| line.len() == LINE_CAP));
        assert!(out.contains(LATER));
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
        // `PAGE` in all three, and `LATER` only in the live log.
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
        // The newer dump carries `PAGE` as well as `newer`.
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
        // The older dump is unopened, and `PAGE` in the newer one dropped.
        assert_eq!(got.lines, vec![newer]);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn the_chunk_straddling_the_watermark_is_still_opened() {
        let dir = tmp("straddle");
        let (log_dir, dump_dir) = (dir.join("log"), dir.join("dumps"));
        std::fs::create_dir_all(&log_dir).unwrap();
        std::fs::create_dir_all(&dump_dir).unwrap();
        // The 11:00 chunk holds `PAGE`, stamped 10:15, and `watermark` is later.
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

        let (mut a, mut b) = (BTreeSet::new(), BTreeSet::new());
        let took_gz = scan(&gz, "", &mut a).expect("a gzipped file");
        let took_plain = scan(&plain, "", &mut b).expect("a plain file");
        assert_eq!(a, b);
        assert_eq!(a, BTreeSet::from([PAGE.to_string()]));
        assert!(took_gz.complete && took_plain.complete);
        // `scan` answers `None` for an empty file.
        let empty = dir.join("empty");
        std::fs::write(&empty, b"").unwrap();
        assert!(scan(&empty, "", &mut BTreeSet::new()).is_none());
        let _ = std::fs::remove_dir_all(&dir);
    }
}
