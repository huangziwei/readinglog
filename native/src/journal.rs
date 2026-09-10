//! What this app writes about itself: one line a launch under a header block
//! restating what only changes with the device or the build. It goes to stderr,
//! which the shell appends to [`LOG_PATH`], and only that file's tail is read.
use std::fmt::Write as _;
use std::io::{Read, Seek, SeekFrom, Write};
use std::path::Path;

/// Where the shell points our stderr. `readinglog.sh` and `collect.sh` name
/// this same path; changing one means changing all three.
pub const LOG_PATH: &str = "/mnt/us/logs/readinglog.log";

/// Opens a header block, at column 0, so a reader and a trim can both find it.
const MARK: &str = "=== ";

/// Opens a line the app could not do what it was asked on. Every `eprintln!`
/// that reports a failure carries it, and no trim may drop such a line.
pub const FAILED: &str = "!!";

/// Opens a line the app went on from, less than whole: a bezel that would not
/// grab, a keyboard that would not open, settings with nowhere to go.
pub const WARNED: &str = "??";

/// How much of the log's tail is read to find the standing header. A block is
/// well under a kilobyte; past this the header is written again, which costs
/// one block and never a wrong answer.
const TAIL: u64 = 8192;

/// The facts a header block states. Each is what it is for the life of a
/// build on a device: a change in any of them is what makes a block worth
/// writing.
pub struct Header {
    pub version: &'static str,
    pub build: &'static str,
    pub arch: &'static str,
    pub panel: String,
    pub surface: String,
    pub input: String,
    pub fonts: String,
    pub store: String,
    pub zone: String,
}

impl Header {
    /// The block, `MARK` first and every other line indented under it.
    pub fn block(&self) -> String {
        let mut out = format!("{MARK}{} {} {}\n", self.version, self.build, self.arch);
        for (name, said) in [
            ("panel", &self.panel),
            ("surface", &self.surface),
            ("input", &self.input),
            ("fonts", &self.fonts),
            ("store", &self.store),
            ("zone", &self.zone),
        ] {
            let _ = writeln!(out, "    {name:<8}{said}");
        }
        out
    }

    /// Write the block where the log does not already end with it, answering
    /// whether it was written. A log we cannot read is treated as one that
    /// does not carry it.
    pub fn state(&self, log: &Path) -> bool {
        let block = self.block();
        if standing(log).is_some_and(|last| last == block) {
            return false;
        }
        eprint!("{block}");
        true
    }
}

/// The last header block in `log`, read out of its tail.
fn standing(log: &Path) -> Option<String> {
    let mut file = std::fs::File::open(log).ok()?;
    let end = file.seek(SeekFrom::End(0)).ok()?;
    file.seek(SeekFrom::Start(end.saturating_sub(TAIL))).ok()?;
    let mut bytes = Vec::with_capacity(TAIL as usize);
    file.read_to_end(&mut bytes).ok()?;
    // A tail cut mid-character is not this file's problem: drop what will not
    // decode and read the block out of the rest.
    let tail = String::from_utf8_lossy(&bytes);
    let at = tail
        .rfind(&format!("\n{MARK}"))
        .map(|i| i + 1)
        .or_else(|| {
            // The whole file is short enough that the tail holds its opening.
            (end <= TAIL && tail.starts_with(MARK)).then_some(0)
        })?;
    let block: String = tail[at..]
        .lines()
        .take_while(|line| line.starts_with(MARK) || line.starts_with("    "))
        .fold(String::new(), |mut out, line| {
            let _ = writeln!(out, "{line}");
            out
        });
    Some(block)
}

/// Now, in the `YYMMDD:HHMMSS` a syslog line and the store's mark both use, so
/// a log line sorts against the rows it is about.
pub fn stamp_now() -> String {
    let (days, secs) = crate::date::now();
    crate::log::line::log_stamp(&crate::date::stamp(days, secs)).unwrap_or_default()
}

/// The most the log may stand at before [`trim`] cuts it. Only a backstop:
/// the version boundary is what does the useful work, and a log that changes
/// version at a normal rate never reaches this.
pub const CEILING: u64 = 262_144;

/// The most one line may take before it is cut. A log line longer than this is
/// broken, and reading it whole is the thing a trim exists to prevent.
const LINE_CAP: usize = 65_536;

/// What one [`trim`] came to.
pub struct Trimmed {
    pub before: u64,
    pub after: u64,
    /// Header blocks dropped from the front.
    pub blocks: usize,
}

/// Cut `log` back to `ceiling`, keeping every [`FAILED`] and [`WARNED`] line
/// wherever it sits, then as far back as the previous [`MARK`] block will fit.
/// Streamed a line at a time: a log outgrows the RAM that would hold it.
pub fn trim(log: &Path, ceiling: u64) -> Option<Trimmed> {
    let before = std::fs::metadata(log).ok()?.len();
    if before <= ceiling {
        return None;
    }
    let (marks, by_bytes) = scan(log, before.saturating_sub(ceiling))?;
    // Where to cut from: the previous block, or the last one where keeping two
    // would leave the file over the ceiling anyway.
    let from = match marks.len() {
        // No header block to cut on: a log written by a build before they
        // existed, which is every log standing at the moment one arrives. The
        // newest `ceiling` bytes are kept, from a line boundary.
        0 => by_bytes,
        1 => marks[0],
        _ => {
            let two = marks[marks.len() - 2];
            match before - two > ceiling {
                true => marks[marks.len() - 1],
                false => two,
            }
        }
    };
    let dropped = marks.iter().filter(|&&at| at < from).count();
    let partial = log.with_extension("partial");
    let kept = copy_kept(log, &partial, from).ok()?;
    // **Write back through the same inode, never rename into place.** The
    // shell holds one `2>>` descriptor on this file for the whole launch, and
    // an unlinked inode takes every line printed afterwards nowhere.
    if put_back(&partial, log).is_err() {
        return None;
    }
    let _ = std::fs::remove_file(&partial);
    Some(Trimmed {
        before,
        after: kept,
        blocks: dropped,
    })
}

/// Copy `from` over `to`'s contents, keeping `to`'s inode.
fn put_back(from: &Path, to: &Path) -> std::io::Result<()> {
    let mut held = std::fs::File::open(from)?;
    let mut out = std::fs::File::options()
        .write(true)
        .truncate(true)
        .open(to)?;
    std::io::copy(&mut held, &mut out)?;
    out.flush()
}

/// One pass over `log`: the byte offset of every [`MARK`] line, and the offset
/// of the first line opening at or after `want`.
fn scan(log: &Path, want: u64) -> Option<(Vec<u64>, u64)> {
    let file = std::fs::File::open(log).ok()?;
    let mut reader = std::io::BufReader::new(file);
    let mut at = 0u64;
    let mut out = Vec::new();
    let mut from = None;
    let mut line = Vec::new();
    loop {
        line.clear();
        let read = read_capped(&mut reader, &mut line).ok()?;
        if read == 0 {
            break;
        }
        if line.starts_with(MARK.as_bytes()) {
            out.push(at);
        }
        if from.is_none() && at >= want {
            from = Some(at);
        }
        at += read as u64;
    }
    Some((out, from.unwrap_or(at)))
}

/// Copy `log` to `to`, keeping every line at or past `from` and every line
/// marked [`FAILED`] or [`WARNED`] before it. Answers the bytes written.
fn copy_kept(log: &Path, to: &Path, from: u64) -> std::io::Result<u64> {
    let mut reader = std::io::BufReader::new(std::fs::File::open(log)?);
    let mut writer = std::io::BufWriter::new(std::fs::File::create(to)?);
    let mut at = 0u64;
    let mut written = 0u64;
    let mut line = Vec::new();
    loop {
        line.clear();
        let read = read_capped(&mut reader, &mut line)?;
        if read == 0 {
            break;
        }
        let keep = at >= from
            || line.starts_with(FAILED.as_bytes())
            || line.starts_with(WARNED.as_bytes());
        if keep {
            writer.write_all(&line)?;
            written += line.len() as u64;
        }
        at += read as u64;
    }
    writer.flush()?;
    Ok(written)
}

/// One line into `out`, at most [`LINE_CAP`] bytes of it. Answers the bytes the
/// line took in the file, which is what a caller stepping offsets needs — the
/// rest of an over-long line is stepped over, not held.
fn read_capped(reader: &mut impl std::io::BufRead, out: &mut Vec<u8>) -> std::io::Result<usize> {
    let mut took = 0usize;
    loop {
        let available = match reader.fill_buf() {
            Ok(buf) => buf,
            Err(err) if err.kind() == std::io::ErrorKind::Interrupted => continue,
            Err(err) => return Err(err),
        };
        if available.is_empty() {
            return Ok(took);
        }
        let (used, done) = match available.iter().position(|&b| b == b'\n') {
            Some(i) => (i + 1, true),
            None => (available.len(), false),
        };
        if out.len() < LINE_CAP {
            let room = LINE_CAP - out.len();
            out.extend_from_slice(&available[..used.min(room)]);
        }
        reader.consume(used);
        took += used;
        if done {
            return Ok(took);
        }
    }
}

/// Which launch this was. A cron collect and a person opening the app do
/// different work, and separating them is what makes a run of launches read.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Mode {
    /// The app was opened and drew.
    Run,
    /// `--collect` folded the log and drew nothing.
    Collect,
}

impl Mode {
    fn tag(self) -> &'static str {
        match self {
            Self::Run => "run",
            Self::Collect => "col",
        }
    }
}

/// What one launch did, as the fields of one line. A field left `None` is one
/// this launch never reached, and it is left out rather than written as zero:
/// a pass that did not run and a pass that found nothing are different facts.
#[derive(Default)]
pub struct Launch {
    /// Log lines read, and where they came from.
    pub log: Option<String>,
    /// Sittings added, extended, and held after the pass.
    pub sittings: Option<String>,
    /// Catalog rows, and the records they refreshed.
    pub catalog: Option<String>,
    /// Book records the store holds. What names no book is `identify`'s.
    pub records: Option<String>,
    /// Jackets held, then the books whose artwork the device has lost and
    /// the books the store never had artwork for.
    pub covers: Option<String>,
    /// What the sources past the catalog named.
    pub identify: Option<String>,
    /// What the two annotation sources came to.
    pub annotate: Option<String>,
    /// What the screens were given to draw. Only a launch that draws has one.
    pub drawn: Option<String>,
}

impl Launch {
    /// The line, stamped and tagged. `stamp` is the shape the store's own
    /// mark uses, so a log line and a store row sort against each other.
    pub fn say(&self, stamp: &str, mode: Mode) -> String {
        let mut out = format!("{stamp} {}", mode.tag());
        for said in [
            &self.log,
            &self.sittings,
            &self.catalog,
            &self.records,
            &self.covers,
            &self.identify,
            &self.annotate,
            &self.drawn,
        ]
        .into_iter()
        .flatten()
        {
            let _ = write!(out, " {said}");
        }
        out
    }

    /// Write the line.
    pub fn state(&self, stamp: &str, mode: Mode) {
        eprintln!("{}", self.say(stamp, mode));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn header() -> Header {
        Header {
            version: "0.3.1",
            build: "260909:230850",
            arch: "armv7l",
            panel: "1264x1680 300ppi body=26px".into(),
            surface: "root 1264x1680 depth=24 stride=5056".into(),
            input: "touch=/dev/input/event2 score=7 grab=ok".into(),
            fonts: "86 faces, primary=Amazon-Ember-Regular.ttf".into(),
            store: "581 s, 491 b, 111 a".into(),
            zone: "Europe/Berlin +0200".into(),
        }
    }

    fn scratch(tag: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!("readinglog-journal-{tag}"));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("a scratch directory");
        dir
    }

    #[test]
    fn a_block_opens_at_column_zero_and_indents_the_rest() {
        let block = header().block();
        let mut lines = block.lines();
        let first = lines.next().expect("an opening line");
        assert!(first.starts_with(MARK), "{first}");
        assert!(
            first.contains("0.3.1") && first.contains("armv7l"),
            "{first}"
        );
        for line in lines {
            assert!(line.starts_with("    "), "indented under the mark: {line}");
        }
    }

    #[test]
    fn a_block_is_written_once_and_not_again_while_it_stands() {
        let dir = scratch("standing");
        let log = dir.join("readinglog.log");
        let block = header().block();

        // Nothing to compare against: the block is written.
        assert!(header().state(&log), "no log at all");
        std::fs::write(&log, &block).unwrap();
        assert!(!header().state(&log), "the same block already stands");

        // Launch lines after it do not hide it.
        std::fs::write(&log, format!("{block}260910:132307 run s=+2~1\n")).unwrap();
        assert!(!header().state(&log), "lines under it are not a new block");

        // A changed fact is a new block.
        let mut moved = header();
        moved.version = "0.3.2";
        assert!(moved.state(&log), "a new version states itself");
    }

    /// A log carrying no header block is the one every install stands on the
    /// moment a build that writes them arrives — and it is the large one. It
    /// must still be cut back, from a line boundary, keeping every failure.
    #[test]
    fn a_log_with_no_header_block_is_still_cut_back() {
        let dir = scratch("noblocks");
        let log = dir.join("readinglog.log");
        let mut text = String::from("!! open: a failure from before the blocks\n");
        text.push_str(&"fonts: /usr/java/lib/fonts/one.ttf -> two.ttf\n".repeat(4_000));
        std::fs::write(&log, &text).unwrap();
        let before = text.len() as u64;

        let cut = trim(&log, 20_000).expect("over the ceiling, and no block in it");
        let left = std::fs::read_to_string(&log).unwrap();
        assert_eq!(cut.before, before);
        assert_eq!(cut.blocks, 0, "there were none to drop");
        assert!(left.len() < text.len(), "cut back");
        assert!(
            left.contains("!! open: a failure from before the blocks"),
            "a failure is kept whatever the log's shape",
        );
        // Cut at a line boundary, never mid-line.
        assert!(
            left.lines()
                .all(|l| l.starts_with("!!") || l.starts_with("fonts: /")),
            "no half line at the head",
        );
    }

    /// The shell runs the binary as `2>> $LOG` and holds one descriptor for
    /// the whole launch. A trim that replaced the file would leave that
    /// descriptor on an unlinked inode and lose everything printed after it.
    #[test]
    fn a_trim_keeps_the_file_it_was_given() {
        let dir = scratch("inode");
        let log = dir.join("readinglog.log");
        let mut text = String::new();
        for v in ["0.3.0", "0.3.1", "0.3.2"] {
            let mut h = header();
            h.version = v;
            text.push_str(&h.block());
            text.push_str(&"260910:132307 run s=+0~0\n".repeat(400));
        }
        std::fs::write(&log, &text).unwrap();

        // A descriptor held open across the trim, as the shell's is.
        let mut held = std::fs::File::options().append(true).open(&log).unwrap();
        trim(&log, 20_000).expect("over the ceiling");
        writeln!(held, "260910:143000 run after the trim").unwrap();
        held.flush().unwrap();

        let left = std::fs::read_to_string(&log).unwrap();
        assert!(
            left.contains("after the trim"),
            "a line written through the descriptor still reaches the file",
        );
        assert!(!left.contains("=== 0.3.0"), "and the trim still trimmed");
        assert!(
            !dir.join("readinglog.partial").exists(),
            "the working file is cleared away",
        );
    }

    #[test]
    fn a_log_longer_than_the_tail_still_finds_its_last_block() {
        let dir = scratch("tail");
        let log = dir.join("readinglog.log");
        let filler = "260910:132307 run s=+0~0\n".repeat(2_000);
        assert!(filler.len() as u64 > TAIL, "the filler outruns the tail");
        std::fs::write(&log, format!("{filler}{}", header().block())).unwrap();
        assert!(!header().state(&log), "the standing block is in the tail");
    }

    #[test]
    fn a_block_pushed_out_of_the_tail_is_written_again() {
        let dir = scratch("pushed");
        let log = dir.join("readinglog.log");
        let filler = "260910:132307 run s=+0~0\n".repeat(2_000);
        std::fs::write(&log, format!("{}{filler}", header().block())).unwrap();
        assert!(
            header().state(&log),
            "past the tail it costs one block, never a wrong answer"
        );
    }

    /// A log under the ceiling is left exactly as it stands.
    #[test]
    fn a_log_under_the_ceiling_is_not_touched() {
        let dir = scratch("under");
        let log = dir.join("readinglog.log");
        let text = format!("{}260910:132307 run s=+0~0\n", header().block());
        std::fs::write(&log, &text).unwrap();
        assert!(trim(&log, CEILING).is_none());
        assert_eq!(std::fs::read_to_string(&log).unwrap(), text);
    }

    /// The trim keeps the previous block and everything after it, and drops
    /// the blocks before that.
    #[test]
    fn a_trim_keeps_the_previous_block_onward() {
        let dir = scratch("blocks");
        let log = dir.join("readinglog.log");
        let mut text = String::new();
        for v in ["0.2.9", "0.3.0", "0.3.1"] {
            let mut h = header();
            h.version = v;
            text.push_str(&h.block());
            text.push_str(&"260910:132307 run s=+0~0 draw=491b\n".repeat(400));
        }
        std::fs::write(&log, &text).unwrap();

        let cut = trim(&log, 20_000).expect("over the ceiling");
        let left = std::fs::read_to_string(&log).unwrap();
        assert_eq!(cut.blocks, 2, "0.2.9 and 0.3.0 went");
        assert!(!left.contains("0.2.9"), "the oldest block is gone");
        assert!(left.contains("=== 0.3.1"), "the newest block stands");
        assert!(left.len() < text.len());
        // Never a half block: what is left opens on a mark.
        assert!(
            left.starts_with(MARK),
            "cut at a boundary: {:?}",
            &left[..40]
        );
    }

    /// No trim, at any level, drops a line that reports a failure.
    #[test]
    fn a_trim_never_drops_a_failure() {
        let dir = scratch("failures");
        let log = dir.join("readinglog.log");
        let mut text = String::new();
        for (v, err) in [("0.2.9", "!! open: the oldest failure\n"), ("0.3.1", "")] {
            let mut h = header();
            h.version = v;
            text.push_str(&h.block());
            text.push_str(err);
            text.push_str("?? buttons: no gpio-keys device\n");
            text.push_str(&"260910:132307 run s=+0~0 draw=491b\n".repeat(400));
        }
        std::fs::write(&log, &text).unwrap();

        trim(&log, 10_000).expect("over the ceiling");
        let left = std::fs::read_to_string(&log).unwrap();
        assert!(
            left.contains("!! open: the oldest failure"),
            "a failure under a dropped block is still kept",
        );
        assert_eq!(left.matches("?? buttons").count(), 2, "both warnings stand");
        assert!(!left.contains("=== 0.2.9"), "its block went all the same");
    }

    /// A single over-long line is stepped over, not held: the offsets stay
    /// right and the trim still cuts where it should.
    #[test]
    fn one_enormous_line_does_not_derail_the_offsets() {
        let dir = scratch("longline");
        let log = dir.join("readinglog.log");
        let mut text = header().block();
        text.push_str(&"x".repeat(LINE_CAP * 2));
        text.push('\n');
        let mut newer = header();
        newer.version = "0.3.2";
        text.push_str(&newer.block());
        text.push_str(&"260910:132307 run s=+0~0\n".repeat(50));
        std::fs::write(&log, &text).unwrap();

        trim(&log, 1_000).expect("over the ceiling");
        let left = std::fs::read_to_string(&log).unwrap();
        assert!(left.starts_with("=== 0.3.2"), "cut at the newer mark");
        assert!(
            !left.contains("xxxx"),
            "the enormous line went with its block"
        );
    }

    /// A device fact is the same on every launch of one build, so it belongs
    /// in the header block and never in the body. Only an event or a marked
    /// failure may print from the open path, and this list is not an accident.
    #[test]
    fn the_device_layers_state_facts_in_the_header_and_events_in_the_body() {
        const SOURCES: [(&str, &str); 3] = [
            ("fb.rs", include_str!("eink/fb.rs")),
            ("touch.rs", include_str!("eink/touch.rs")),
            ("buttons.rs", include_str!("eink/buttons.rs")),
        ];
        // Events: a state the app moved through while running.
        const EVENTS: [&str; 6] = [
            "fb: laid out",
            "touch: covered=",
            "touch: EVIOCGRAB retaken",
            "buttons: covered=",
            "buttons: EVIOCGRAB retaken",
            "orientation:",
        ];

        let mut loose = Vec::new();
        for (name, text) in SOURCES {
            let mut in_tests = false;
            for line in text.lines() {
                in_tests |= line.trim_start().starts_with("mod tests");
                if in_tests || !line.contains("eprintln!(\"") {
                    continue;
                }
                let said = line
                    .split_once("eprintln!(\"")
                    .map(|(_, rest)| rest)
                    .unwrap_or_default();
                let marked = said.starts_with(FAILED) || said.starts_with(WARNED);
                let evented = EVENTS.iter().any(|e| said.starts_with(e));
                if !marked && !evented {
                    loose.push(format!("{name}: {}", said.trim_end_matches('"')));
                }
            }
        }
        assert!(
            loose.is_empty(),
            "a device fact must reach the header block, not a line a launch: {loose:#?}",
        );
    }

    #[test]
    fn a_launch_line_leaves_out_what_it_never_reached() {
        let bare = Launch::default();
        assert_eq!(bare.say("260910:132307", Mode::Run), "260910:132307 run");

        let full = Launch {
            log: Some("log=25524".into()),
            sittings: Some("s=+2~1/581".into()),
            covers: Some("cov=58-20".into()),
            ..Launch::default()
        };
        assert_eq!(
            full.say("260910:132307", Mode::Collect),
            "260910:132307 col log=25524 s=+2~1/581 cov=58-20",
        );
    }
}
