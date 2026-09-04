//! An invented library: a shelf in three scripts and [`DAYS`] of reading over
//! it. Every number comes off [`Rng`], seeded once: the same picture every
//! run, and two rounds of a design held against each other.

use std::path::Path;

use readinglog_native::date;
use readinglog_native::log::session::{Measure, Session};
use readinglog_native::store::{BookRecord, Store};

/// Days of reading laid down behind the day being drawn.
pub const DAYS: i64 = 1150;

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
    }
}

/// Days back from the last day drawn that the binge falls on.
const BINGE_DAY: i64 = 40;

/// Books read on the binge day, one to an hour from [`BINGE_OPENS`].
const BINGE_BOOKS: usize = 10;
const BINGE_OPENS: i64 = 8;

/// One day of [`BINGE_BOOKS`] short sittings, in place of whatever the
/// generator left on it: a list of more books than a page holds. Every
/// sitting stays under half an hour.
fn binge(store: &mut Store, last: i64) {
    let day = last - BINGE_DAY;
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
            title: book.title.into(),
            author: book.author.into(),
            thumbnail: jacket(art, slot),
            language: book.language.into(),
            percent: book.percent,
            on_device: slot % 5 != 4,
            cover: String::new(),
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
        let weekend = matches!(date::weekday(day), 0 | 6);
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
    binge(&mut store, last);
    ghosts(&mut store, last);
    store
        .sessions
        .sort_by(|a, b| a.started_at.cmp(&b.started_at));
    store
}

/// [`library`] with `keep` of the last day's sittings left on it: how a screen
/// reads on a day that has barely started, and on one with nothing on it.
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

/// The day [`library`] lays the binge on, for a run ending on `last`.
pub fn binge_day(last: i64) -> i64 {
    last - BINGE_DAY
}

/// Books keyed by an extent no record carries: what the log holds where the
/// catalog never stated a book's own number.
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
