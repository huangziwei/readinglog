//! [`library`]: an invented library, books in three scripts and [`DAYS`] of reading over them,
//! every number off [`Rng`], seeded once: the same picture on every run.

use std::path::Path;

use readinglog_native::clippings::Kind;
use readinglog_native::date;
use readinglog_native::highlights::{Highlight, State};
use readinglog_native::log::sitting::{Measure, Sitting, Stretch};
use readinglog_native::record::{self, FINISHED_PERCENT, IdentifiedBy, Record};
use readinglog_native::written::{HighlightId, Note, Passage};

/// `DAYS`: days of reading laid down behind the day being drawn.
pub const DAYS: i64 = 1150;

/// `HIGHLIGHTED`: the [`LIBRARY`] slot `Book::finished` is set on.
const HIGHLIGHTED: usize = 7;

/// `REREAD`, `READING_AGAIN`: the book read end to end twice, and the one read again part way.
const REREAD: usize = 0;
const READING_AGAIN: usize = 2;

/// `MANY`, `TIMES`: the collection gone back to again and again, filling the Graphs tab's list.
const MANY: usize = 11;
const TIMES: usize = 8;

/// `UNCOVERED`: slots with no cover, a Latin title, a Han one and one too long for its box.
const UNCOVERED: [usize; 3] = [3, 6, 7];

/// `Listed`: a book in the library, and the span of days it was read over.
struct Listed {
    title: &'static str,
    author: &'static str,
    language: &'static str,
    percent: f64,
    /// `opened`: days back from the last day drawn that the book was opened.
    opened: i64,
    /// `days`: how many days its reading runs for.
    days: i64,
}

/// `LIBRARY`: Titles from one word to a line and a half, in three scripts, at every stage from
/// opened yesterday to all but finished.
const LIBRARY: &[Listed] = &[
    Listed {
        title: "The Salt Road Companion",
        author: "Beatrix Oyelaran",
        language: "en",
        percent: 100.0,
        opened: 1140,
        days: 210,
    },
    Listed {
        title: "静かな海の測量",
        author: "三好あかり",
        language: "ja",
        percent: 100.0,
        opened: 1010,
        days: 190,
    },
    Listed {
        title: "第二座橋",
        author: "陳望之",
        language: "zh-Hant",
        percent: 93.0,
        opened: 870,
        days: 240,
    },
    Listed {
        title: "Notes Toward a Theory of Weather",
        author: "Aurelio Sandoval",
        language: "en",
        percent: 100.0,
        opened: 700,
        days: 200,
    },
    Listed {
        title: "长夜行车",
        author: "邹允",
        language: "zh-Hans",
        percent: 58.0,
        opened: 560,
        days: 180,
    },
    Listed {
        title: "The Cartographer's Apprentice",
        author: "Nell Hargreave",
        language: "en",
        percent: 100.0,
        opened: 470,
        days: 150,
    },
    Listed {
        title: "沒有名字的河：一段流域史與它的居民",
        author: "周牧",
        language: "zh-Hant",
        percent: 71.0,
        opened: 420,
        days: 260,
    },
    Listed {
        title: "A Complete History of Nothing in Particular, with Notes and an Index, Volume 1",
        author: "Margaret Ellery",
        language: "en",
        percent: 46.0,
        opened: 360,
        days: 330,
    },
    Listed {
        title: "ねむらない街の図鑑 ～第一巻～",
        author: "白鳥ゆかり",
        language: "ja",
        percent: 88.0,
        opened: 300,
        days: 120,
    },
    Listed {
        title: "Writing the Slow Chase",
        author: "Iris Vandermeer",
        language: "en",
        percent: 100.0,
        opened: 240,
        days: 70,
    },
    Listed {
        title: "灰的重量",
        author: "何允之",
        language: "zh-Hans",
        percent: 34.0,
        opened: 150,
        days: 130,
    },
    Listed {
        title: "The Ninth Winter and Other Stories",
        author: "Cordelia Nash",
        language: "en",
        percent: 62.0,
        opened: 96,
        days: 90,
    },
    Listed {
        title: "夢遊症候群",
        author: "林素",
        language: "zh-Hant",
        percent: 19.0,
        opened: 40,
        days: 40,
    },
    Listed {
        title: "Interval",
        author: "Tomas Reidy",
        language: "en",
        percent: 3.0,
        opened: 2,
        days: 3,
    },
];

/// `CLOCK`: the hours a sitting opens in, each written as many times as it is common: evenings
/// mostly, a morning habit, and the odd late night.
const CLOCK: &[i64] = &[
    0, 1, 6, 7, 7, 8, 8, 12, 13, 13, 16, 17, 19, 20, 20, 21, 21, 21, 22, 22, 22, 23,
];

/// `Rng`: a deterministic stream, the same on any machine.
struct Rng(u64);

impl Rng {
    fn next(&mut self) -> u64 {
        self.0 = self
            .0
            .wrapping_mul(6_364_136_223_846_793_005)
            .wrapping_add(1_442_695_040_888_963_407);
        self.0 >> 33
    }

    /// `upto`: a number below `bound`.
    fn upto(&mut self, bound: i64) -> i64 {
        (self.next() % bound.max(1) as u64) as i64
    }

    /// `between`: a number from `from` up to `to`.
    fn between(&mut self, from: i64, to: i64) -> i64 {
        from + self.upto(to - from + 1)
    }

    /// `one_in`: whether something one time in `n` happens.
    fn one_in(&mut self, n: i64) -> bool {
        self.upto(n) == 0
    }
}

/// `cover`: a stand-in cover for the book at `slot`, 217x330, two bands and a block.
fn cover(dir: &Path, slot: usize) -> String {
    let (w, h) = (217u32, 330u32);
    let hue = [
        [200u8, 40, 40],
        [30, 60, 140],
        [180, 140, 40],
        [40, 120, 90],
        [90, 40, 130],
        [220, 180, 40],
        [40, 40, 40],
        [150, 90, 60],
    ][slot % 8];
    let mut img = image::RgbImage::from_pixel(w, h, image::Rgb(hue));
    for y in 0..h {
        for x in 0..w {
            let band = (h / 3..h / 3 + h / 12).contains(&y);
            let block = (h * 2 / 3..h * 2 / 3 + h / 5).contains(&y) && (20..w - 20).contains(&x);
            if band || block {
                img.put_pixel(x, y, image::Rgb([250, 250, 250]));
            }
        }
    }
    let path = dir.join(format!("cover{slot}.png"));
    img.save(&path).expect("a written cover");
    path.display().to_string()
}

/// `sitting`: one sitting of `secs`, opening `at` seconds into `day`, its seconds booked to each
/// clock hour it crosses.
fn sitting(day: i64, at: i64, secs: i64, extent: i64, measure: Measure) -> Sitting {
    let (y, m, d) = date::civil_from_days(day);
    let stamp = |secs: i64| {
        let secs = secs.min(86_399);
        format!(
            "{y:04}-{m:02}-{d:02}T{:02}:{:02}:{:02}",
            secs / 3600,
            (secs / 60) % 60,
            secs % 60
        )
    };
    let mut hours = Vec::new();
    let (mut cursor, mut left) = (at, secs);
    while left > 0 && cursor < 86_400 {
        let hour = cursor / 3600;
        let till = ((hour + 1) * 3600).min(cursor + left);
        hours.push((hour as u8, till - cursor));
        left -= till - cursor;
        cursor = till;
    }
    let turns = secs / 42;
    Sitting {
        started_at: stamp(at),
        ended_at: stamp(at + secs),
        end_position: extent,
        seconds: secs,
        page_turns: turns,
        words: turns * 260,
        hours,
        measure,
        asin: None,
        progress: None,
        ..Sitting::default()
    }
}

/// `BINGE_DAY`: days back from the last day drawn that the binge falls on.
const BINGE_DAY: i64 = 40;

/// `BINGE_BOOKS`: books read on the binge day, one to an hour from [`BINGE_OPENS`].
const BINGE_BOOKS: usize = 10;
const BINGE_OPENS: i64 = 8;

/// `binge`: one day of [`BINGE_BOOKS`] short sittings, in place of whatever [`library`] left on it;
/// every sitting stays under half an hour.
fn binge(record: &mut Record, day: i64) {
    record
        .sittings
        .retain(|s| date::parse_day(date::day_of(&s.started_at)) != Some(day));
    for slot in 0..BINGE_BOOKS {
        let at = (BINGE_OPENS + slot as i64) * 3600 + 600;
        let secs = 60 * (18 + (slot as i64 * 7) % 9);
        let extent = record.books[slot].extent;
        record
            .sittings
            .push(sitting(day, at, secs, extent, Measure::Counted));
    }
}

/// [`LIBRARY`], and [`DAYS`] of reading over it ending on `last`.
pub fn library(last: i64, covers: &Path) -> Record {
    let mut record = Record::default();
    for (slot, book) in LIBRARY.iter().enumerate() {
        record.books.push(record::Book {
            extent: 100_000 + slot as i64,
            cde_key: format!("KEY{slot}"),
            cde_type: "EBOK".into(),
            title: book.title.into(),
            author: book.author.into(),
            thumbnail: match UNCOVERED.contains(&slot) {
                true => String::new(),
                false => cover(covers, slot),
            },
            language: book.language.into(),
            percent: book.percent,
            // `timer_covered`, `timer_words`: the Kindle's own counter, the whole of `percent` at a
            // rate putting a full read at eight hours.
            timer_covered: book.percent.max(0.0) / 100.0,
            timer_words: (120_000.0 * book.percent.max(0.0) / 100.0) as i64,
            timer_ms: (8.0 * 3600.0 * 1000.0 * book.percent.max(0.0) / 100.0) as i64,
            on_kindle: slot % 5 != 4,
            cover: String::new(),
            // `location`: a title with a space in it, which `open::uri` escapes.
            location: match slot % 5 != 4 {
                true => format!("/mnt/us/documents/{}.kfx", book.title),
                false => String::new(),
            },
            // `Record::remember` marks a book its progress states read through;
            // this one is `HIGHLIGHTED`, short of `FINISHED_PERCENT`.
            finished: slot == HIGHLIGHTED || book.percent >= FINISHED_PERCENT,
            restart: None,
            restarted_on: Vec::new(),
            read_state: -1,
            kept: false,
            identified_by: IdentifiedBy::Library,
        });
    }

    let mut rng = Rng(0x5EED_1D0C);
    // `shut`: a fortnight with the Kindle shut, somewhere in the spring.
    let shut = last - rng.between(150, 250);
    for back in (0..DAYS).rev() {
        let day = last - back;
        if (shut..shut + 14).contains(&day) {
            continue;
        }
        // `date::weekday` counts from Monday: the weekend is 5 and 6.
        let weekend = matches!(date::weekday(day), 5 | 6);
        // `rng.upto(10)`: a day off, three weekdays in ten and one weekend day in ten.
        if rng.upto(10) < if weekend { 1 } else { 3 } {
            continue;
        }
        let open: Vec<usize> = LIBRARY
            .iter()
            .enumerate()
            .filter(|(_, b)| (b.opened - b.days..b.opened).contains(&back))
            .map(|(slot, _)| slot)
            .collect();
        if open.is_empty() {
            continue;
        }
        let count = match weekend {
            true => rng.between(2, 3),
            false => rng.between(1, 2),
        };
        let mut booked: Vec<i64> = Vec::new();
        for _ in 0..count {
            let slot = open[rng.upto(open.len() as i64) as usize];
            let extent = record.books[slot].extent;
            let hour = CLOCK[rng.upto(CLOCK.len() as i64) as usize];
            let at = hour * 3600 + rng.upto(3600);
            // `booked`: one sitting to an hour, none running past midnight.
            if booked.contains(&hour) {
                continue;
            }
            booked.push(hour);
            let minutes = match weekend {
                true => rng.between(20, 95),
                false => rng.between(8, 55),
            };
            let secs = (minutes * 60).min(86_399 - at);
            let measure = match rng.one_in(11) {
                true => Measure::Paged,
                false => Measure::Counted,
            };
            record
                .sittings
                .push(sitting(day, at, secs, extent, measure));
        }
    }
    binge(&mut record, last - BINGE_DAY);
    ghosts(&mut record, last);
    record
        .sittings
        .sort_by(|a, b| a.started_at.cmp(&b.started_at));
    climb(&mut record);
    highlighted(&mut record, last);
    reread(&mut record);
    record
}

/// `reread`: the books read end to end before, laid down after the seeded loop, a shot drawing
/// none staying pixel-identical.
fn reread(record: &mut Record) {
    for slot in [REREAD, READING_AGAIN, MANY] {
        let Some(extent) = record.books.get(slot).map(|b| b.extent) else {
            continue;
        };
        let days: Vec<i64> = record
            .sittings
            .iter()
            .filter(|s| s.end_position == extent)
            .filter_map(|s| date::parse_day(date::day_of(&s.started_at)))
            .collect();
        // `over`: each climb after the first opens on the day the book was begun again, as
        // [`climb`] cuts them.
        let over = (days.len() as f64 / times(slot)).ceil().max(1.0) as usize;
        let Some(book) = record.books.get_mut(slot) else {
            continue;
        };
        book.restarted_on = (1..times(slot) as usize)
            .filter_map(|n| days.get(n * over).copied())
            .collect();
        if slot == READING_AGAIN {
            book.finished = false;
        }
    }
}

/// `highlighted`: the highlights `record` holds, laid down after the seeded loop: a shot drawing no
/// highlight stays pixel-identical; nothing here is [`State::Retired`]: no fresh record holds one.
fn highlighted(record: &mut Record, last: i64) {
    /// `(slot, kind, state, through the book, the day it was made, the words)`.
    const HIGHLIGHTS: &[(usize, Kind, State, f64, i64, &str)] = &[
        (
            0,
            Kind::Highlight,
            State::Live,
            0.080,
            86,
            "The road remembers every cart that ever crossed it, and forgives none of them.",
        ),
        // `0`: a note on the passage above, its own range inside that one, the only tie between
        // the two.
        (
            0,
            Kind::Note,
            State::Live,
            0.0801,
            86,
            "This is the line the whole first part turns on — come back to it.",
        ),
        (
            0,
            Kind::Highlight,
            State::Live,
            0.224,
            61,
            "Salt is the only cargo that pays for its own weight twice: once going out, once coming back, and the second time in stories.",
        ),
        (0, Kind::Bookmark, State::Live, 0.310, 60, ""),
        (
            0,
            Kind::Highlight,
            State::Live,
            0.447,
            44,
            "She counted the wells the way other people count birthdays.",
        ),
        (
            0,
            Kind::Underline,
            State::Live,
            0.688,
            21,
            "Nobody crosses the same desert twice, and nobody who says otherwise has crossed it once.",
        ),
        (
            0,
            Kind::Highlight,
            State::Unconfirmed,
            0.912,
            9,
            "There is a kind of arithmetic that only works at night, and she had all of it by heart, and none of it written down anywhere that a customs officer could ever be shown.",
        ),
        (
            1,
            Kind::Highlight,
            State::Live,
            0.142,
            140,
            "海はいつも同じ顔をしているようで、測るたびに違う数字を返してくる。",
        ),
        (
            1,
            Kind::Note,
            State::Live,
            0.1421,
            140,
            "ここの言い回しがすごく好き。あとで引用する。",
        ),
        (
            1,
            Kind::Highlight,
            State::Unconfirmed,
            0.530,
            96,
            "静けさというのは音がないことではなく、聞くべき音が決まっていることなのだと父は言った。",
        ),
        (
            2,
            Kind::Highlight,
            State::Live,
            0.317,
            210,
            "橋を渡る者は、渡らなかった者のことを一度も考えない。",
        ),
        (
            2,
            Kind::Highlight,
            State::Live,
            0.402,
            200,
            "第二座橋建成那年，河水改道了。",
        ),
        // `5`: the deep one, enough highlights, and enough of them noted, that the list pages on
        // every panel; a book of one or two and a book of a dozen are different pictures.
        (
            5,
            Kind::Highlight,
            State::Live,
            0.031,
            300,
            "A harbour is a promise a coastline makes and a tide keeps breaking.",
        ),
        (
            5,
            Kind::Note,
            State::Live,
            0.0311,
            300,
            "Opening line. The whole book is arguing with this.",
        ),
        (
            5,
            Kind::Highlight,
            State::Live,
            0.094,
            288,
            "They kept the ledgers in salt water so that a lie would dissolve before it could be read twice.",
        ),
        (
            5,
            Kind::Underline,
            State::Live,
            0.161,
            270,
            "Every port keeps two clocks: the one the ships run on and the one the town believes.",
        ),
        (
            5,
            Kind::Note,
            State::Live,
            0.1611,
            270,
            "cf. the chapter on the customs house — the same joke, told straight.",
        ),
        (
            5,
            Kind::Highlight,
            State::Live,
            0.237,
            251,
            "He had the particular patience of a man who has already lost the argument and is waiting to be proved right about it.",
        ),
        (
            5,
            Kind::Highlight,
            State::Live,
            0.321,
            233,
            "Weather is the only creditor that never sends a letter first.",
        ),
        (
            5,
            Kind::Note,
            State::Live,
            0.3211,
            233,
            "Use for the epigraph.",
        ),
        (
            5,
            Kind::Highlight,
            State::Live,
            0.402,
            210,
            "What the charts call a shoal, the people who live on it call a street.",
        ),
        (
            5,
            Kind::Highlight,
            State::Live,
            0.489,
            188,
            "She learned the language the way you learn a debt: one demand at a time, and always in arrears.",
        ),
        (
            5,
            Kind::Circle,
            State::Live,
            0.574,
            160,
            "The lighthouse keeper voted twice in every election and neither vote was counted, which he considered a fair exchange for the view.",
        ),
        (
            5,
            Kind::Highlight,
            State::Unconfirmed,
            0.661,
            141,
            "There is no word in the harbour dialect for a journey that ends where it began, and no shortage of them.",
        ),
        // `13`: read on the last day drawn, the day's own list stating what was highlighted on it.
        (
            13,
            Kind::Highlight,
            State::Live,
            0.017,
            0,
            "An interval is not a gap in the music; it is the part of the music that is only listening.",
        ),
        (
            13,
            Kind::Note,
            State::Live,
            0.0171,
            0,
            "Title comes from here.",
        ),
        (
            13,
            Kind::Highlight,
            State::Live,
            0.026,
            0,
            "Rests are counted, not waited out.",
        ),
        // `9`: short passages, a row as tall as its own words: this book's page holds far more rows
        // than slot 5's long ones.
        (
            9,
            Kind::Highlight,
            State::Live,
            0.052,
            330,
            "Begin in the wrong place.",
        ),
        (
            9,
            Kind::Highlight,
            State::Live,
            0.118,
            316,
            "A chase is a shape, not a speed.",
        ),
        (
            9,
            Kind::Highlight,
            State::Live,
            0.186,
            300,
            "Cut on the breath, not the beat.",
        ),
        (
            9,
            Kind::Highlight,
            State::Live,
            0.254,
            288,
            "Nobody runs in a straight line.",
        ),
        (
            9,
            Kind::Highlight,
            State::Live,
            0.331,
            270,
            "Fear is specific or it is nothing.",
        ),
        (
            9,
            Kind::Highlight,
            State::Live,
            0.408,
            255,
            "Give the pursuer an errand.",
        ),
        (
            9,
            Kind::Highlight,
            State::Live,
            0.486,
            240,
            "Let one street be described twice.",
        ),
        (
            9,
            Kind::Highlight,
            State::Live,
            0.563,
            221,
            "End before the running does.",
        ),
    ];
    for (slot, kind, state, through, back, body) in HIGHLIGHTS {
        let Some(book) = record.books.get(*slot) else {
            continue;
        };
        let start = (book.extent as f64 * through) as i64;
        // `placed`: a highlight no sidecar placed has no position at all — only the display
        // location the clipping named, which is on another axis.
        let placed = *state != State::Unconfirmed;
        record.highlights.push(Highlight {
            extent: book.extent,
            title: book.title.clone(),
            kind: *kind,
            at: date::stamp(last - back, 9 * 3600 + (start % 3_600)),
            state: *state,
            start: match placed {
                true => start,
                false => -1,
            },
            // `end`: a note covers the few words it was written on, a passage a run of them.
            end: match (placed, kind) {
                (true, Kind::Note) => start + 8,
                (true, _) => start + 400,
                (false, _) => -1,
            },
            location: start / 150,
            // `page`: all three positions on a book carrying publisher pages, the two its sources
            // named on one without.
            page: match slot % 2 {
                0 => (start / 3_000).to_string(),
                _ => String::new(),
            },
            colour: match (placed, kind) {
                (true, Kind::Highlight | Kind::Underline) => "orange".into(),
                _ => String::new(),
            },
            body: (*body).into(),
        });
    }
    record.highlights.sort_by(|a, b| a.at.cmp(&b.at));
}

/// `TWICE`: the slots [`climb`] reads more than once, [`RESTARTED`] never reaching the end.
const TWICE: [usize; 3] = [RESTARTED, REREAD, READING_AGAIN];
const RESTARTED: usize = 12;

/// `times`: how many times [`climb`] reads one slot through.
fn times(slot: usize) -> f64 {
    match slot {
        MANY => TIMES as f64,
        slot if TWICE.contains(&slot) => 2.0,
        _ => 1.0,
    }
}

/// `climb` gives each sitting the progress its book stood at as it ended: an even climb over that
/// book's own sittings, ending at the book's `percent`; a sitting [`ghosts`] left names no book and
/// keeps `None`; [`RESTARTED`] climbs twice.
fn climb(record: &mut Record) {
    for slot in 0..record.books.len() {
        let (extent, percent) = (record.books[slot].extent, record.books[slot].percent);
        if percent < 0.0 {
            continue;
        }
        let at: Vec<usize> = (0..record.sittings.len())
            .filter(|&i| record.sittings[i].end_position == extent)
            .collect();
        let count = at.len() as f64;
        let over = (count / times(slot)).ceil().max(1.0);
        let mut stood = 0.0;
        for (n, i) in at.iter().enumerate() {
            let step = (n as f64 % over) + 1.0;
            // `peak`: every reading but the last ends at the end of the book, except for the one
            // that was begun again part way through.
            let peak = match (n as f64) < count - over && slot != RESTARTED {
                true => 100.0,
                false => percent,
            };
            let progress = (peak / 100.0 * step / over).min(1.0);
            record.sittings[*i].progress = Some(progress);
            // `opened`: a sitting covers the ground between where the one before it left off and
            // where it ends — except every fifth, which dips into the front matter first and
            // carries on, the shape a jump leaves.
            let opened = match step == 1.0 {
                true => 0.0,
                false => stood,
            };
            record.sittings[*i].stretches = match n % 5 == 4 && opened < progress {
                true => vec![
                    Stretch {
                        from: 0.01,
                        to: 0.03,
                    },
                    Stretch {
                        from: opened,
                        to: progress,
                    },
                ],
                false => vec![Stretch {
                    from: opened,
                    to: progress,
                }],
            };
            stood = progress;
        }
    }
}

/// [`library`] with `keep` of the last day's sittings left on it.
pub fn thinned(last: i64, covers: &Path, keep: usize) -> Record {
    let mut record = library(last, covers);
    let mut seen = 0usize;
    record.sittings.retain(|s| {
        if date::parse_day(date::day_of(&s.started_at)) != Some(last) {
            return true;
        }
        seen += 1;
        seen <= keep
    });
    record
}

/// [`library`] with a second binge on the last day itself, which is the day
/// Today draws: a day of more books than one page of the list holds.
pub fn crowded(last: i64, covers: &Path) -> Record {
    let mut record = library(last, covers);
    binge(&mut record, last);
    // `climb`: the second binge lands past [`library`]'s own call, each sitting taking its
    // progress here.
    climb(&mut record);
    record
}

/// [`library`] with `w` rows, notes, on slot 9's book: one opening
/// on a `# ` heading and one with none on the book alone, two under one passage,
/// and one under another beside the Kindle's own note; in Japanese where `ja`.
pub fn written(last: i64, covers: &Path, ja: bool) -> Record {
    let mut record = library(last, covers);
    let Some(book) = record.books.get(9).cloned() else {
        return record;
    };
    let say = |en: &str, jp: &str| match ja {
        true => jp.to_string(),
        false => en.to_string(),
    };
    let passage = |record: &Record, words: &str| {
        record
            .highlights
            .iter()
            .find(|h| h.body == words && h.kind == Kind::Highlight)
            .cloned()
    };
    let street = passage(&record, "Let one street be described twice.");
    if let Some(street) = &street {
        record.highlights.push(Highlight {
            kind: Kind::Note,
            start: street.start + 1,
            end: street.start + 9,
            colour: String::new(),
            body: "The second time at night.".into(),
            ..street.clone()
        });
        record.sort_highlights();
    }
    let end = passage(&record, "End before the running does.");
    let notes = [
        (
            219,
            None,
            say(
                "# On chases that end early\n\nThe book keeps telling you to stop sooner than feels right. Every chapter I put it to got better, and none of them got shorter in the way I feared.",
                "# 早めに終わる追跡について\n\nこの本は、気持ちより早く止めろと言い続ける。試した章はどれも良くなり、恐れていたほど短くもならなかった。",
            ),
        ),
        (
            220,
            end.as_ref(),
            say("The rule I keep breaking.", "何度も破ってしまう規則。"),
        ),
        (
            216,
            end.as_ref(),
            say(
                "Tried it on chapter three: it works.",
                "三章で試した。うまくいく。",
            ),
        ),
        (
            234,
            None,
            say(
                "Halfway, and the chase has not started yet. That is the point.",
                "半分まで来て、追跡はまだ始まっていない。それが狙いだ。",
            ),
        ),
        (
            238,
            street.as_ref(),
            say(
                "Same street, other weather, in my draft.",
                "下書きでも同じ通りを、別の天気で。",
            ),
        ),
    ];
    for (back, on, text) in notes {
        let written = date::stamp(last - back, 20 * 3600);
        record.add_note(Note {
            written: written.clone(),
            edited: written,
            key: book.cde_key.clone(),
            extent: book.extent,
            on: on.map(|h| Passage {
                id: HighlightId::of(h),
                from: h.start,
                to: h.end,
                words: h.body.clone(),
            }),
            text,
            deleted: false,
        });
    }
    // `first`: slot 0's book, under a review longer than any page holds, every kind of block in it.
    if let Some(first) = record.books.first().cloned() {
        let written = date::stamp(last - 2, 21 * 3600);
        let paragraph = "The roads in this book are kept the way a ledger is kept, every crossing entered and none forgiven, and the people on them learn to travel light because the road remembers what they carried.";
        record.add_note(Note {
            written: written.clone(),
            edited: written,
            key: first.cde_key.clone(),
            extent: first.extent,
            on: None,
            text: format!(
                "# A long way round\n\n{paragraph}\n\n{paragraph}\n\n- salt, twice\n- wells, counted\n  - and the birthdays\n1. the first part\n2. the second\n\n> Nobody crosses the same desert twice.\n\n```\nkept as written\n  and indented\n```\n\n{paragraph}\n\n{paragraph}\n\n{paragraph}"
            ),
            deleted: false,
        });
    }
    // `second`: slot 1's book, under forty paragraphs of a line each, more air than any page holds.
    if let Some(second) = record.books.get(1).cloned() {
        let written = date::stamp(last - 3, 21 * 3600);
        record.add_note(Note {
            written: written.clone(),
            edited: written,
            key: second.cde_key.clone(),
            extent: second.extent,
            on: None,
            text: vec!["A line of its own."; 40].join("\n\n"),
            deleted: false,
        });
    }
    record
}

/// `binge_day`: the day [`library`] lays the binge on, for reading ending on `last`.
pub fn binge_day(last: i64) -> i64 {
    last - BINGE_DAY
}

/// `GHOSTS`: books keyed by an extent the record holds no book for.
const GHOSTS: i64 = 5;

/// `GHOST_EVERY`: days between one day of ghost sittings and the next.
const GHOST_EVERY: i64 = 3;

/// `GHOST_KEY`: the first ghost extent, past every extent [`library`] gives its books.
const GHOST_KEY: i64 = 640_000;

/// `ghosts`: one to three ghost sittings on every [`GHOST_EVERY`]th day, today included: a total no
/// list of books adds up to.
fn ghosts(record: &mut Record, last: i64) {
    for back in (0..DAYS).step_by(GHOST_EVERY as usize).chain([BINGE_DAY]) {
        let day = last - back;
        let round = back / GHOST_EVERY;
        for which in 0..=round % 3 {
            let key = GHOST_KEY + (round + which) % GHOSTS;
            let at = (17 + which) * 3600 + 600;
            let secs = 60 * (12 + (back * 7 + which * 13) % 48);
            record
                .sittings
                .push(sitting(day, at, secs, key, Measure::Counted));
        }
    }
}

/// `TAGGED`: each [`LIBRARY`] slot's tags: its form, its genre in the book's own words, and its
/// language.
const TAGGED: [(&str, &str, &str); 14] = [
    ("Nonfiction", "Travel", "English"),
    ("Fiction", "ミステリー", "Japanese"),
    ("Fiction", "Literary", "Chinese, Traditional"),
    ("Essays", "Science", "English"),
    ("Fiction", "推理", "Chinese, Simplified"),
    ("Fiction", "Fantasy", "English"),
    ("Nonfiction", "History", "Chinese, Traditional"),
    ("Nonfiction", "History", "English"),
    ("Comics", "Fantasy", "Japanese"),
    ("Fiction", "Mystery", "English"),
    ("Fiction", "Literary", "Chinese, Simplified"),
    ("Short stories", "Literary", "English"),
    ("Fiction", "Sci-Fi", "Chinese, Traditional"),
    ("Poetry", "Literary", "English"),
];

/// `RATED`: the slots `tagged` gives a Rating, a type whose every tag is a whole number.
const RATED: [(usize, &str); 8] = [
    (0, "5"),
    (2, "3"),
    (4, "4"),
    (5, "5"),
    (6, "2"),
    (7, "4"),
    (8, "3"),
    (9, "4"),
];

/// `record` with `d` and `l` rows for [`TAGGED`]: every book's
/// tags, a co-author on slot 3, two publishers over five books, slot 3 under Nonfiction
/// beside Essays, and Mystery, ミステリー and 推理 linked as one genre.
pub fn tagged(mut record: Record) -> Record {
    use readinglog_native::metadata::links::{Link, Linked};
    use readinglog_native::metadata::{Field, Metadata, Tag, tags_value};
    let row = |slot: usize, field: Field, value: String| Metadata {
        edited: "2026-11-14T10:00:00".into(),
        key: format!("KEY{slot}"),
        extent: 100_000 + slot as i64,
        field,
        value: Some(value),
        was: None,
    };
    let tag = |r#type: &str, tag: &str| Tag {
        r#type: r#type.into(),
        tag: tag.into(),
    };
    for (slot, (form, genre, language)) in TAGGED.iter().enumerate() {
        let mut tags = vec![
            tag("Form", form),
            tag("Genre", genre),
            tag("Language", language),
        ];
        if slot == 3 {
            tags.insert(1, tag("Form", "Nonfiction"));
        }
        if let Some((_, rating)) = RATED.iter().find(|(rated, _)| *rated == slot) {
            tags.push(tag("Rating", rating));
        }
        record
            .metadata
            .push(row(slot, Field::Tags, tags_value(&tags)));
    }
    record.metadata.push(row(
        3,
        Field::Authors,
        "Aurelio Sandoval & Mina Okafor".into(),
    ));
    for (slot, publisher) in [
        (0, "Harbour Books"),
        (5, "Harbour Books"),
        (9, "Harbour Books"),
        (1, "みなと書房"),
        (8, "みなと書房"),
    ] {
        record
            .metadata
            .push(row(slot, Field::Publisher, publisher.into()));
    }
    record.links.push(Link {
        edited: "2026-11-14T10:00:00".into(),
        linked: Linked::Tag,
        r#type: "Genre".into(),
        first: "Mystery".into(),
        names: Some(vec!["ミステリー".into(), "推理".into()]),
    });
    record
}

/// `PAPER_READ`: [`papered`]'s sittings: days back from the last day drawn, the minute it opened at,
/// minutes read, and the pages it opened and closed on. The fourth runs past midnight.
const PAPER_READ: [(i64, i64, i64, i64, i64); 8] = [
    (15, 21 * 60 + 10, 45, 0, 40),
    (13, 22 * 60, 55, 40, 95),
    (12, 7 * 60 + 20, 25, 95, 118),
    (10, 23 * 60 + 20, 70, 118, 190),
    (7, 20 * 60 + 40, 50, 190, 240),
    (5, 21 * 60 + 15, 35, 240, 275),
    (2, 13 * 60, 90, 275, 360),
    (0, 21 * 60 + 30, 45, 360, 405),
];

/// [`library`] with a book of 432 pages read on paper over the last fortnight, the `p` and `q` rows
/// the desktop writes, the newest sitting closing on `last`.
pub fn papered(last: i64, covers: &Path) -> Record {
    let mut record = library(last, covers);
    let now = date::stamp(last, 86_399);
    let Ok(added) = record.add_paper_book(
        "The Orchard at Hollin Bank",
        "Margit Sølvberg",
        "en",
        432,
        &date::stamp(last - 16, 19 * 3600),
    ) else {
        return record;
    };
    for (back, at, minutes, from, to) in PAPER_READ {
        let opens = (last - back) * 86_400 + at * 60;
        let closes = opens + minutes * 60;
        let stamp = |at: i64| date::stamp(at.div_euclid(86_400), at.rem_euclid(86_400));
        let logged =
            record.log_paper_sitting(&added, (&stamp(opens), &stamp(closes)), (from, to), &now);
        if let Err(why) = logged {
            eprintln!("!! fixture: a paper sitting {back} days back: {why}");
        }
    }
    record
}
