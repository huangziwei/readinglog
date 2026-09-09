//! An invented library: a shelf in three scripts and [`DAYS`] of reading over
//! it. Every number comes off [`Rng`], seeded once: the same picture on
//! every run.

use std::path::Path;

use readinglog_native::annotate::{Mark, State};
use readinglog_native::clippings::Kind;
use readinglog_native::date;
use readinglog_native::log::session::{Measure, Session};
use readinglog_native::store::{BookRecord, FINISHED_PERCENT, Named, Store};

/// Days of reading laid down behind the day being drawn.
pub const DAYS: i64 = 1150;

/// The [`SHELF`] slot `BookRecord::finished` is set on.
const MARKED: usize = 7;

/// The [`SHELF`] slots the device holds no jacket for: a Latin title, a Han
/// one, and a title too long for the box it stands in, so what a cover box
/// says without a cover is drawn in every shape it takes.
const UNJACKETED: [usize; 3] = [3, 6, 7];

/// A book on the shelf, and the stretch of days it was read over.
struct Shelved {
    title: &'static str,
    author: &'static str,
    language: &'static str,
    percent: f64,
    /// Days back from the last day drawn that the book was opened.
    opened: i64,
    /// How many days its reading runs for.
    runs: i64,
}

/// Titles from one word to a line and a half, in three scripts, at every stage
/// from opened yesterday to all but finished.
const SHELF: &[Shelved] = &[
    Shelved {
        title: "The Salt Road Companion",
        author: "Beatrix Oyelaran",
        language: "en",
        percent: 100.0,
        opened: 1140,
        runs: 210,
    },
    Shelved {
        title: "静かな海の測量",
        author: "三好あかり",
        language: "ja",
        percent: 100.0,
        opened: 1010,
        runs: 190,
    },
    Shelved {
        title: "第二座橋",
        author: "陳望之",
        language: "zh-Hant",
        percent: 93.0,
        opened: 870,
        runs: 240,
    },
    Shelved {
        title: "Notes Toward a Theory of Weather",
        author: "Aurelio Sandoval",
        language: "en",
        percent: 100.0,
        opened: 700,
        runs: 200,
    },
    Shelved {
        title: "长夜行车",
        author: "邹允",
        language: "zh-Hans",
        percent: 58.0,
        opened: 560,
        runs: 180,
    },
    Shelved {
        title: "The Cartographer's Apprentice",
        author: "Nell Hargreave",
        language: "en",
        percent: 100.0,
        opened: 470,
        runs: 150,
    },
    Shelved {
        title: "沒有名字的河：一段流域史與它的居民",
        author: "周牧",
        language: "zh-Hant",
        percent: 71.0,
        opened: 420,
        runs: 260,
    },
    Shelved {
        title: "A Complete History of Nothing in Particular, with Notes and an Index, Volume 1",
        author: "Margaret Ellery",
        language: "en",
        percent: 46.0,
        opened: 360,
        runs: 330,
    },
    Shelved {
        title: "ねむらない街の図鑑 ～第一巻～",
        author: "白鳥ゆかり",
        language: "ja",
        percent: 88.0,
        opened: 300,
        runs: 120,
    },
    Shelved {
        title: "Writing the Slow Chase",
        author: "Iris Vandermeer",
        language: "en",
        percent: 100.0,
        opened: 240,
        runs: 70,
    },
    Shelved {
        title: "灰的重量",
        author: "何允之",
        language: "zh-Hans",
        percent: 34.0,
        opened: 150,
        runs: 130,
    },
    Shelved {
        title: "The Ninth Winter and Other Stories",
        author: "Cordelia Nash",
        language: "en",
        percent: 62.0,
        opened: 96,
        runs: 90,
    },
    Shelved {
        title: "夢遊症候群",
        author: "林素",
        language: "zh-Hant",
        percent: 19.0,
        opened: 40,
        runs: 40,
    },
    Shelved {
        title: "Interval",
        author: "Tomas Reidy",
        language: "en",
        percent: 3.0,
        opened: 2,
        runs: 3,
    },
];

/// The hours a sitting opens in, each written as many times as it is common:
/// evenings mostly, a morning habit, and the odd late night.
const CLOCK: &[i64] = &[
    0, 1, 6, 7, 7, 8, 8, 12, 13, 13, 16, 17, 19, 20, 20, 21, 21, 21, 22, 22, 22, 23,
];

/// A deterministic stream, the same on any machine.
struct Rng(u64);

impl Rng {
    fn next(&mut self) -> u64 {
        self.0 = self
            .0
            .wrapping_mul(6_364_136_223_846_793_005)
            .wrapping_add(1_442_695_040_888_963_407);
        self.0 >> 33
    }

    /// A number below `bound`.
    fn upto(&mut self, bound: i64) -> i64 {
        (self.next() % bound.max(1) as u64) as i64
    }

    /// A number from `from` up to `to`.
    fn between(&mut self, from: i64, to: i64) -> i64 {
        from + self.upto(to - from + 1)
    }

    /// Whether something one time in `n` happens.
    fn one_in(&mut self, n: i64) -> bool {
        self.upto(n) == 0
    }
}

/// A stand-in jacket for the book at `slot`, 217x330.
///
/// Two bands and a block.
fn jacket(dir: &Path, slot: usize) -> String {
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
    img.save(&path).expect("a written jacket");
    path.display().to_string()
}

/// One sitting of `secs`, opening `at` seconds into `day`, its seconds booked
/// to each clock hour it crosses.
fn sitting(day: i64, at: i64, secs: i64, extent: i64, measure: Measure) -> Session {
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
    Session {
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
        ..Session::default()
    }
}

/// Days back from the last day drawn that the binge falls on.
const BINGE_DAY: i64 = 40;

/// Books read on the binge day, one to an hour from [`BINGE_OPENS`].
const BINGE_BOOKS: usize = 10;
const BINGE_OPENS: i64 = 8;

/// One day of [`BINGE_BOOKS`] short sittings, in place of whatever
/// [`library`] left on it. Every sitting stays under half an hour.
fn binge(store: &mut Store, day: i64) {
    store
        .sessions
        .retain(|s| date::parse_day(date::day_of(&s.started_at)) != Some(day));
    for slot in 0..BINGE_BOOKS {
        let at = (BINGE_OPENS + slot as i64) * 3600 + 600;
        let secs = 60 * (18 + (slot as i64 * 7) % 9);
        let extent = store.books[slot].extent;
        store
            .sessions
            .push(sitting(day, at, secs, extent, Measure::Counted));
    }
}

/// The shelf, and [`DAYS`] of reading over it ending on `last`.
pub fn library(last: i64, art: &Path) -> Store {
    let mut store = Store::default();
    for (slot, book) in SHELF.iter().enumerate() {
        store.books.push(BookRecord {
            extent: 100_000 + slot as i64,
            cde_key: format!("KEY{slot}"),
            cde_type: "EBOK".into(),
            title: book.title.into(),
            author: book.author.into(),
            thumbnail: match UNJACKETED.contains(&slot) {
                true => String::new(),
                false => jacket(art, slot),
            },
            language: book.language.into(),
            percent: book.percent,
            on_device: slot % 5 != 4,
            cover: String::new(),
            // A title with a space in it, which `open::uri` escapes.
            location: match slot % 5 != 4 {
                true => format!("/mnt/us/documents/{}.kfx", book.title),
                false => String::new(),
            },
            // `Store::remember` marks a book its place states read through;
            // this one is `MARKED`, short of `FINISHED_PERCENT`.
            finished: slot == MARKED || book.percent >= FINISHED_PERCENT,
            restart: None,
            read_state: -1,
            kept: false,
            named_by: Named::Catalog,
        });
    }

    let mut rng = Rng(0x5EED_1D0C);
    // A fortnight with the device shut, somewhere in the spring.
    let shut = last - rng.between(150, 250);
    for back in (0..DAYS).rev() {
        let day = last - back;
        if (shut..shut + 14).contains(&day) {
            continue;
        }
        // `date::weekday` counts from Monday: the weekend is 5 and 6.
        let weekend = matches!(date::weekday(day), 5 | 6);
        // A day off, three weekdays in ten and one weekend day in ten.
        if rng.upto(10) < if weekend { 1 } else { 3 } {
            continue;
        }
        let open: Vec<usize> = SHELF
            .iter()
            .enumerate()
            .filter(|(_, b)| (b.opened - b.runs..b.opened).contains(&back))
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
            let extent = store.books[slot].extent;
            let hour = CLOCK[rng.upto(CLOCK.len() as i64) as usize];
            let at = hour * 3600 + rng.upto(3600);
            // One sitting to an hour, and none running past midnight.
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
                true => Measure::Dwell,
                false => Measure::Counted,
            };
            store.sessions.push(sitting(day, at, secs, extent, measure));
        }
    }
    binge(&mut store, last - BINGE_DAY);
    ghosts(&mut store, last);
    store
        .sessions
        .sort_by(|a, b| a.started_at.cmp(&b.started_at));
    climb(&mut store);
    marked(&mut store, last);
    store
}

/// What the reader marked, laid down after the seeded loop so every shot that
/// does not draw a mark stays pixel-identical.
///
/// One row per case the screen has to hold: a highlight with a colour and a
/// real position, a note in the reader's own words, one no sidecar could place
/// so that only the display location stands, a bookmark carrying no words at
/// all, and a book deep enough to page. Slot 1 and slot 2 are in Japanese and
/// Han so the list is read in the script the book is set in, slot 9's passages
/// are short and slot 5's long so both ends of the row packing stand, and most
/// of the shelf carries nothing — a book with no marks is the ordinary case
/// and its own picture.
///
/// Nothing here is [`State::Retired`]: `annotate::fold` stores no row for a
/// mark the reader deleted, so a store holding one is a store no device
/// writes.
fn marked(store: &mut Store, last: i64) {
    /// `(slot, kind, state, through the book, the day it was made, the words)`.
    const MARKS: &[(usize, Kind, State, f64, i64, &str)] = &[
        (
            0,
            Kind::Highlight,
            State::Live,
            0.080,
            86,
            "The road remembers every cart that ever crossed it, and forgives none of them.",
        ),
        // A note the reader wrote on the passage above: its own range sits
        // inside that one, which is the only thing tying the two together.
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
        // Slot 5 is the deep one: enough marks, and enough of them noted, that
        // the list pages on every panel. A book of one or two and a book of a
        // dozen are different pictures and both have to hold.
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
        // Slot 13 is read on the last day drawn, so the day's own list states
        // what was marked on it.
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
        // Slot 9's passages are all short on purpose. A row is as tall as its
        // own words, so this book's page holds far more rows than slot 5's
        // long ones: keep both, and keep these short.
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
    for (slot, kind, state, through, back, body) in MARKS {
        let Some(book) = store.books.get(*slot) else {
            continue;
        };
        let start = (book.extent as f64 * through) as i64;
        // A mark no sidecar placed has no position at all — only the display
        // location the clipping named, which is on another axis.
        let placed = *state != State::Unconfirmed;
        store.marks.push(Mark {
            extent: book.extent,
            title: book.title.clone(),
            kind: *kind,
            at: date::stamp(last - back, 9 * 3600 + (start % 3_600)),
            state: *state,
            start: match placed {
                true => start,
                false => -1,
            },
            // A note covers the few words it was written on; a passage covers
            // a run of them.
            end: match (placed, kind) {
                (true, Kind::Note) => start + 8,
                (true, _) => start + 400,
                (false, _) => -1,
            },
            location: start / 150,
            // A book carrying publisher pages states all three places; one
            // without states the two its sources named.
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
    store.marks.sort_by(|a, b| a.at.cmp(&b.at));
}

/// Give each sitting the place its book stood at as it ended: an even climb
/// over that book's own sittings, ending at the record's `percent`. A sitting
/// [`ghosts`] left names no book and keeps `None`.
fn climb(store: &mut Store) {
    for slot in 0..store.books.len() {
        let (extent, percent) = (store.books[slot].extent, store.books[slot].percent);
        if percent < 0.0 {
            continue;
        }
        let at: Vec<usize> = (0..store.sessions.len())
            .filter(|&i| store.sessions[i].end_position == extent)
            .collect();
        let count = at.len() as f64;
        for (n, i) in at.iter().enumerate() {
            store.sessions[*i].progress = Some(percent / 100.0 * (n as f64 + 1.0) / count);
        }
    }
}

/// [`library`] with `keep` of the last day's sittings left on it.
pub fn thinned(last: i64, art: &Path, keep: usize) -> Store {
    let mut store = library(last, art);
    let mut seen = 0usize;
    store.sessions.retain(|s| {
        if date::parse_day(date::day_of(&s.started_at)) != Some(last) {
            return true;
        }
        seen += 1;
        seen <= keep
    });
    store
}

/// [`library`] with a second binge on the last day itself, which is the day
/// Today draws: a day of more books than one page of the list holds.
pub fn crowded(last: i64, art: &Path) -> Store {
    let mut store = library(last, art);
    binge(&mut store, last);
    // The second binge lands past [`library`]'s own call, and each of its
    // sittings takes a place here.
    climb(&mut store);
    store
}

/// The day [`library`] lays the binge on, for a run ending on `last`.
pub fn binge_day(last: i64) -> i64 {
    last - BINGE_DAY
}

/// Books keyed by an extent no record carries.
const GHOSTS: i64 = 5;

/// Days between one day of ghost sittings and the next.
const GHOST_EVERY: i64 = 3;

/// The first ghost extent, past every extent [`library`] gives the shelf.
const GHOST_KEY: i64 = 640_000;

/// One to three ghost sittings on every [`GHOST_EVERY`]th day, today included:
/// a total no list of books adds up to.
fn ghosts(store: &mut Store, last: i64) {
    for back in (0..DAYS).step_by(GHOST_EVERY as usize).chain([BINGE_DAY]) {
        let day = last - back;
        let round = back / GHOST_EVERY;
        for which in 0..=round % 3 {
            let key = GHOST_KEY + (round + which) % GHOSTS;
            let at = (17 + which) * 3600 + 600;
            let secs = 60 * (12 + (back * 7 + which * 13) % 48);
            store
                .sessions
                .push(sitting(day, at, secs, key, Measure::Counted));
        }
    }
}
