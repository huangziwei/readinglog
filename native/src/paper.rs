//! Paper books: a `p` row states a book read on paper and a `q` row one sitting in it, both
//! written on the desktop and carried beside the rows a Kindle derives as `w` rows are.
//! [`Record::lay_paper`] lays them in as the books and sittings every figure is built from.

use std::cmp::Reverse;
use std::fmt::Write as _;

use crate::date;
use crate::log::sitting::{Measure, Sitting, Stretch};
use crate::record::{Book, FINISHED_PERCENT, Record};
use crate::written::{after, escaped, later, unescaped};

/// `PAPER_FROM`: where paper books' extents start. A Kindle states 32-bit positions, so no Kindle
/// book or sitting reaches one.
pub const PAPER_FROM: i64 = 1 << 36;

/// `KEY_PREFIX`: what a paper book's key opens with.
pub const KEY_PREFIX: &str = "paper-";

/// `DAY_SECS`: a day, the longest a `q` row may state.
pub const DAY_SECS: i64 = 86_400;

/// `BOOK_ROW` and `SITTING_ROW`: the first field of a `p` row and of a `q` row.
const BOOK_ROW: &str = "p";
const SITTING_ROW: &str = "q";

/// `PaperBook`: a `p` row: one book read on paper, named in its record by the moment it was added.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct PaperBook {
    /// `added`: `YYYY-MM-DDTHH:MM:SS` on the clock that added it.
    pub added: String,
    /// `edited`: when it last changed or was deleted: of two copies, the later stands.
    pub edited: String,
    /// `deleted`: `true` keeps the row, drawn nowhere.
    pub deleted: bool,
    /// `pages`: the page its reading ends on.
    pub pages: i64,
    pub title: String,
    pub author: String,
    /// `language`: a language code, as a Kindle's library states one.
    pub language: String,
    /// `cover`: where its cover sits on the Kindle, under `covers::COVERS_DIR`, as a `b` row's
    /// `cover` names a Kindle book's; empty for none.
    pub cover: String,
}

impl PaperBook {
    /// `extent`: the extent this book draws at, [`extent`] of [`Self::added`], 0 where that is no
    /// instant.
    pub fn extent(&self) -> i64 {
        extent(&self.added).unwrap_or(0)
    }

    /// `key`: the key this book draws under, [`key`] of [`Self::added`].
    pub fn key(&self) -> String {
        key(&self.added)
    }

    /// `as_book`: the [`Book`] this draws as before any sitting: at 0, or unstated where it holds
    /// no page; kept, so it is listed while nothing is read.
    pub fn as_book(&self) -> Book {
        Book {
            extent: self.extent(),
            cde_key: self.key(),
            title: self.title.clone(),
            author: self.author.clone(),
            language: self.language.clone(),
            cover: self.cover.clone(),
            percent: match self.pages > 0 {
                true => 0.0,
                false => -1.0,
            },
            read_state: -1,
            kept: true,
            ..Book::default()
        }
    }
}

/// `PaperSitting`: a `q` row: one sitting in a paper book, named by the moment it was logged.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct PaperSitting {
    /// `logged`: `YYYY-MM-DDTHH:MM:SS` on the clock that logged it.
    pub logged: String,
    /// `edited`: when it last changed or was deleted: of two copies, the later stands.
    pub edited: String,
    /// `deleted`: `true` keeps the row, drawn nowhere.
    pub deleted: bool,
    /// `book`: the [`PaperBook::added`] of the book it was read in.
    pub book: String,
    /// `from` and `to`: when it opened and closed, `YYYY-MM-DDTHH:MM:SS` as every stamp stored.
    pub from: String,
    pub to: String,
    /// `from_page` and `to_page`: where the bookmark stood as it opened and as it closed.
    pub from_page: i64,
    pub to_page: i64,
}

impl PaperSitting {
    /// `span`: when it opens and closes, as [`instant`] states them; `None` where either is no
    /// instant or it closes no later than it opens.
    pub fn span(&self) -> Option<(i64, i64)> {
        let (from, to) = (instant(&self.from)?, instant(&self.to)?);
        (to > from).then_some((from, to))
    }
}

/// `Refusal`: why a paper book or sitting was not written.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Refusal {
    /// `Untitled`: a book given no title.
    Untitled,
    /// `NoPages`: a book given fewer than one page.
    NoPages,
    /// `UnderLogged`: a page count short of the furthest page a sitting reached.
    UnderLogged,
    /// `NotAnInstant`: a time that is no `YYYY-MM-DDTHH:MM:SS` instant.
    NotAnInstant,
    /// `Backwards`: a sitting closing no later than it opens.
    Backwards,
    /// `TooLong`: a sitting running past a day.
    TooLong,
    /// `Ahead`: a sitting closing after now.
    Ahead,
    /// `Pages`: pages outside 0 ≤ from ≤ to ≤ the book's pages.
    Pages,
    /// `Overlaps`: a sitting overlapping another paper sitting.
    Overlaps,
    /// `NoBook`: no paper book standing by the name given.
    NoBook,
    /// `NoSitting`: no paper sitting standing by the name given.
    NoSitting,
}

impl std::fmt::Display for Refusal {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            Self::Untitled => "a book needs a title",
            Self::NoPages => "a book needs at least one page",
            Self::UnderLogged => "a sitting already reaches past that page",
            Self::NotAnInstant => "a time needs a date, an hour and a minute",
            Self::Backwards => "a sitting has to close after it opens",
            Self::TooLong => "a sitting has to close within a day",
            Self::Ahead => "a sitting cannot close later than now",
            Self::Pages => "pages run from 0 to the book's last, the first no later than the last",
            Self::Overlaps => "another paper sitting covers that time",
            Self::NoBook => "no such paper book",
            Self::NoSitting => "no such paper sitting",
        })
    }
}

impl std::error::Error for Refusal {}

/// `instant`: `stamp`, a `YYYY-MM-DDTHH:MM:SS` instant, in seconds from the day [`date`] counts
/// from; `None` for any other text.
pub fn instant(stamp: &str) -> Option<i64> {
    let b = stamp.as_bytes();
    let digits = [0, 1, 2, 3, 5, 6, 8, 9, 11, 12, 14, 15, 17, 18];
    let shaped = b.len() == 19
        && (b[4], b[7], b[10], b[13], b[16]) == (b'-', b'-', b'T', b':', b':')
        && digits.iter().all(|&at| b[at].is_ascii_digit());
    if !shaped {
        return None;
    }
    let day = date::parse_day(&stamp[..10])?;
    let n = |at: usize| stamp[at..at + 2].parse::<i64>().unwrap_or(0);
    let (h, m, s) = (n(11), n(14), n(17));
    (h < 24 && m < 60 && s < 60).then_some(day * DAY_SECS + h * 3600 + m * 60 + s)
}

/// `stamp`: [`instant`]'s seconds as the instant they state.
fn stamp(at: i64) -> String {
    date::stamp(at.div_euclid(DAY_SECS), at.rem_euclid(DAY_SECS))
}

/// `extent`: the extent the book added at `added` draws at: [`PAPER_FROM`] and `added` in seconds;
/// `None` where `added` is no instant.
pub fn extent(added: &str) -> Option<i64> {
    instant(added).map(|at| PAPER_FROM + at)
}

/// `digits`: `stamp`, a `YYYY-MM-DDTHH:MM:SS` instant, without its punctuation:
/// `20261126T154512`.
fn digits(stamp: &str) -> String {
    stamp.chars().filter(char::is_ascii_alphanumeric).collect()
}

/// `stamped`: whether `text` is what [`digits`] makes of an instant: eight digits, `T`, six digits.
fn stamped(text: &[u8]) -> bool {
    text.len() == 15
        && text.iter().enumerate().all(|(at, c)| match at {
            8 => *c == b'T',
            _ => c.is_ascii_digit(),
        })
}

/// `key`: the key the book added at `added` draws under: [`KEY_PREFIX`] and `added`'s digits,
/// `paper-20261126T154512`.
pub fn key(added: &str) -> String {
    format!("{KEY_PREFIX}{}", digits(added))
}

/// `is_paper`: whether `key` is one [`key`] makes, alone or with the `@` and number a desktop
/// marks one Kindle's keys with among several. A Kindle book a sidecar alone names takes its
/// file's name as its key, so the prefix alone proves nothing.
pub fn is_paper(key: &str) -> bool {
    let Some(rest) = key.strip_prefix(KEY_PREFIX).map(str::as_bytes) else {
        return false;
    };
    let (stamp, mark) = rest.split_at(rest.len().min(15));
    let marked = match mark.split_first() {
        None => true,
        Some((b'@', n)) => !n.is_empty() && n.iter().all(u8::is_ascii_digit),
        Some(_) => false,
    };
    stamped(stamp) && marked
}

/// `COVER_PREFIX`: what a paper cover's file name opens with.
const COVER_PREFIX: &str = "paper_";

/// `cover_name`: the file a cover set at `at` on the book added at `added` takes under
/// `covers::COVERS_DIR`: `paper_20261126T154512_20261127T090000.jpg`, a name each time one is
/// set, so no cache holds the picture it replaced.
pub fn cover_name(added: &str, at: &str) -> String {
    format!("{COVER_PREFIX}{}_{}.jpg", digits(added), digits(at))
}

/// `is_cover`: whether `name` is a file name [`cover_name`] makes.
pub fn is_cover(name: &str) -> bool {
    let Some(rest) = name
        .strip_prefix(COVER_PREFIX)
        .and_then(|r| r.strip_suffix(".jpg"))
        .map(str::as_bytes)
    else {
        return false;
    };
    rest.len() == 31 && rest[15] == b'_' && stamped(&rest[..15]) && stamped(&rest[16..])
}

/// `on_the_kindle`: where the cover named `name` sits on the Kindle, what a `p` row's `cover`
/// states.
pub fn on_the_kindle(name: &str) -> String {
    std::path::Path::new(crate::record::RECORD_DIR)
        .join(crate::covers::COVERS_DIR)
        .join(name)
        .to_string_lossy()
        .into_owned()
}

/// `state`: the word a row states whether it stands in.
fn state(deleted: bool) -> &'static str {
    match deleted {
        true => "deleted",
        false => "live",
    }
}

/// `write_book`: a `p` row.
pub(crate) fn write_book(out: &mut String, b: &PaperBook) {
    let _ = writeln!(
        out,
        "{BOOK_ROW}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}",
        b.added,
        b.edited,
        state(b.deleted),
        b.pages,
        escaped(&b.title),
        escaped(&b.author),
        escaped(&b.language),
        escaped(&b.cover),
    );
}

/// `read_book`: a `p` row's fields past the first as a [`PaperBook`], `None` where it names no
/// instant.
pub(crate) fn read_book<'a>(f: &mut impl Iterator<Item = &'a str>) -> Option<PaperBook> {
    let mut next = || f.next().unwrap_or_default();
    let added = next().trim().to_string();
    instant(&added)?;
    let edited = next().trim().to_string();
    let deleted = next().trim() == "deleted";
    let pages = next().trim().parse().unwrap_or(0);
    let title = unescaped(next());
    let author = unescaped(next());
    let language = unescaped(next());
    let cover = unescaped(next());
    Some(PaperBook {
        edited: match edited.is_empty() {
            true => added.clone(),
            false => edited,
        },
        added,
        deleted,
        pages,
        title,
        author,
        language,
        cover,
    })
}

/// `write_sitting`: a `q` row.
pub(crate) fn write_sitting(out: &mut String, s: &PaperSitting) {
    let _ = writeln!(
        out,
        "{SITTING_ROW}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}",
        s.logged,
        s.edited,
        state(s.deleted),
        s.book,
        s.from,
        s.to,
        s.from_page,
        s.to_page,
    );
}

/// `read_sitting`: a `q` row's fields past the first as a [`PaperSitting`], `None` where it names
/// no instant it was logged at or no book.
pub(crate) fn read_sitting<'a>(f: &mut impl Iterator<Item = &'a str>) -> Option<PaperSitting> {
    let mut next = || f.next().unwrap_or_default();
    let logged = next().trim().to_string();
    instant(&logged)?;
    let edited = next().trim().to_string();
    let deleted = next().trim() == "deleted";
    let book = next().trim().to_string();
    instant(&book)?;
    let from = next().trim().to_string();
    let to = next().trim().to_string();
    let from_page = next().trim().parse().unwrap_or(0);
    let to_page = next().trim().parse().unwrap_or(0);
    Some(PaperSitting {
        edited: match edited.is_empty() {
            true => logged.clone(),
            false => edited,
        },
        logged,
        deleted,
        book,
        from,
        to,
        from_page,
        to_page,
    })
}

/// `owns`: whether `line` is a `p` or `q` row this build reads, which `written::splice` replaces;
/// any other it keeps as it stands.
pub(crate) fn owns(line: &str) -> bool {
    let mut f = line.split('\t');
    match f.next() {
        Some(BOOK_ROW) => read_book(&mut f).is_some(),
        Some(SITTING_ROW) => read_sitting(&mut f).is_some(),
        _ => false,
    }
}

/// `take_rows` takes `fresh` into `held`, each row by the moment `name` answers: one `held` lacks
/// is added, and of two copies the later `edited` stands. Answers whether anything moved.
fn take_rows<T: Clone>(
    held: &mut Vec<T>,
    fresh: &[T],
    name: impl Fn(&T) -> &str,
    edited: impl Fn(&T) -> &str,
) -> bool {
    let mut moved = false;
    for row in fresh {
        match held.iter_mut().find(|h| name(h) == name(row)) {
            Some(h) if edited(row) > edited(h) => {
                *h = row.clone();
                moved = true;
            }
            Some(_) => {}
            None => {
                held.push(row.clone());
                moved = true;
            }
        }
    }
    moved
}

/// `stored`: `share` to the six places a row holds a share to.
fn stored(share: f64) -> f64 {
    (share * 1e6).round() / 1e6
}

/// `left`: the seconds left as a sitting closes at `percent`, `spent` seconds having covered
/// `share` of the book, as `BookStat::time_left` projects them; `None` where nothing is covered,
/// or `percent` stands where that projects none.
fn left(spent: i64, share: f64, percent: f64) -> Option<i64> {
    if !(0.5..FINISHED_PERCENT).contains(&percent) || share <= 0.0 || spent <= 0 {
        return None;
    }
    let ahead = (100.0 - percent) / 100.0;
    Some((spent as f64 * ahead / share) as i64)
}

/// `drawn`: `book` as the [`Book`] it draws as, and `read`, its live sittings in the order read,
/// as the [`Sitting`]s they draw as: one a day each runs into, cut at midnight as the Kindle cuts
/// one, seconds by the clock either side and the pages in the same share. `None` where `book`
/// names no extent.
fn drawn(book: &PaperBook, read: &[&PaperSitting]) -> Option<(Book, Vec<Sitting>)> {
    let extent = extent(&book.added)?;
    let key = key(&book.added);
    let pages = book.pages;
    let mut sittings = Vec::new();
    let mut percent = book.as_book().percent;
    let mut finished = false;
    // `spent` and `share`: the seconds of every part so far that covered a page, and the share of
    // the book they covered, which the time left is projected off.
    let (mut spent, mut share) = (0_i64, 0.0_f64);
    for s in read {
        let Some((from, to)) = s.span() else {
            continue;
        };
        let turned = (s.to_page - s.from_page).max(0);
        let at = |t: i64| turned as f64 * (t - from) as f64 / (to - from) as f64;
        let mut start = from;
        while start < to {
            let midnight = (start.div_euclid(DAY_SECS) + 1) * DAY_SECS;
            let end = to.min(midnight);
            let (opened, closed) = (at(start), at(end));
            let seconds = end - start;
            let mut part = Sitting {
                started_at: stamp(start),
                ended_at: stamp(end.min(midnight - 1)),
                end_position: extent,
                seconds,
                page_turns: closed.round() as i64 - opened.round() as i64,
                measure: Measure::Counted,
                asin: Some(key.clone()),
                paged_seconds: seconds,
                ..Sitting::default()
            };
            if pages > 0 {
                let of = |page: f64| ((s.from_page as f64 + page) / pages as f64).clamp(0.0, 1.0);
                let covered = stored(of(closed) - of(opened));
                part.progress = Some(stored(of(closed)));
                part.stretches = vec![Stretch {
                    from: of(opened),
                    to: of(closed),
                }];
                part.covered = covered;
                percent = stored(of(closed) * 100.0);
                finished |= percent >= FINISHED_PERCENT;
                if covered > 0.0 {
                    spent += seconds;
                    share += covered;
                }
                part.time_left = left(spent, share, percent);
            }
            sittings.push(part);
            start = end;
        }
    }
    let book = Book {
        percent,
        finished,
        ..book.as_book()
    };
    Some((book, sittings))
}

impl Record {
    /// `take_paper` takes `other`'s `p` and `q` rows as [`Record::take_written`] takes notes,
    /// answering whether anything moved.
    pub(crate) fn take_paper(&mut self, other: &Record) -> bool {
        let books = take_rows(
            &mut self.paper_books,
            &other.paper_books,
            |b| b.added.as_str(),
            |b| b.edited.as_str(),
        );
        let sittings = take_rows(
            &mut self.paper_sittings,
            &other.paper_sittings,
            |s| s.logged.as_str(),
            |s| s.edited.as_str(),
        );
        books || sittings
    }

    /// `sort_paper` orders the `p` and `q` rows by the moment that names each, one row each, the
    /// later `edited` of two copies standing.
    pub(crate) fn sort_paper(&mut self) {
        self.paper_books
            .sort_by(|a, b| (&a.added, Reverse(&a.edited)).cmp(&(&b.added, Reverse(&b.edited))));
        self.paper_books.dedup_by(|a, b| a.added == b.added);
        self.paper_sittings
            .sort_by(|a, b| (&a.logged, Reverse(&a.edited)).cmp(&(&b.logged, Reverse(&b.edited))));
        self.paper_sittings.dedup_by(|a, b| a.logged == b.logged);
    }

    /// `lay_paper` takes the `p` and `q` rows out, and lays each paper book standing in as a
    /// [`Book`] and each sitting standing in one as its [`Sitting`]s, sorted as a load sorts.
    pub(crate) fn lay_paper(&mut self) {
        if self.paper_books.is_empty() && self.paper_sittings.is_empty() {
            return;
        }
        let laid: Vec<(Book, Vec<Sitting>)> = self
            .paper_books
            .iter()
            .filter(|b| !b.deleted)
            .filter_map(|b| drawn(b, &self.paper_sittings_of(&b.added)))
            .collect();
        self.paper_books.clear();
        self.paper_sittings.clear();
        for (book, sittings) in laid {
            self.books.push(book);
            self.sittings.extend(sittings);
        }
        self.sort();
    }

    /// `paper_book`: the paper book standing that was added at `added`.
    pub fn paper_book(&self, added: &str) -> Option<&PaperBook> {
        self.paper_books
            .iter()
            .find(|b| !b.deleted && b.added == added)
    }

    /// `paper_keyed`: the paper book standing that `key` names, [`key`] of its `added`.
    pub fn paper_keyed(&self, key: &str) -> Option<&PaperBook> {
        self.paper_books
            .iter()
            .find(|b| !b.deleted && b.key() == key)
    }

    /// `paper_sittings_of`: the sittings standing in the book added at `added`, in the order read.
    pub fn paper_sittings_of(&self, added: &str) -> Vec<&PaperSitting> {
        let mut out: Vec<&PaperSitting> = self
            .paper_sittings
            .iter()
            .filter(|s| !s.deleted && s.book == added)
            .collect();
        out.sort_by(|a, b| (&a.from, &a.to, &a.logged).cmp(&(&b.from, &b.to, &b.logged)));
        out
    }

    /// `bookmark`: where the book added at `added` was put down: the page its last sitting closed
    /// on, 0 before any.
    pub fn bookmark(&self, added: &str) -> i64 {
        self.paper_sittings_of(added)
            .last()
            .map_or(0, |s| s.to_page)
    }

    /// `add_paper_book` holds a new paper book, added at `at`, moved a second on while another
    /// book holds the same moment. Answers the moment that names it.
    pub fn add_paper_book(
        &mut self,
        title: &str,
        author: &str,
        language: &str,
        pages: i64,
        at: &str,
    ) -> Result<String, Refusal> {
        if title.trim().is_empty() {
            return Err(Refusal::Untitled);
        }
        if pages < 1 {
            return Err(Refusal::NoPages);
        }
        instant(at).ok_or(Refusal::NotAnInstant)?;
        let mut added = at.to_string();
        while self.paper_books.iter().any(|b| b.added == added) {
            added = later(&added);
        }
        self.paper_books.push(PaperBook {
            added: added.clone(),
            edited: added.clone(),
            deleted: false,
            pages,
            title: title.trim().to_string(),
            author: author.trim().to_string(),
            language: language.trim().to_string(),
            cover: String::new(),
        });
        self.sort_paper();
        Ok(added)
    }

    /// `set_paper_pages` sets the pages of the book added at `added`, as of `now`; no fewer than
    /// the furthest page a sitting reached.
    pub fn set_paper_pages(&mut self, added: &str, pages: i64, now: &str) -> Result<(), Refusal> {
        if pages < 1 {
            return Err(Refusal::NoPages);
        }
        self.paper_book(added).ok_or(Refusal::NoBook)?;
        let furthest = self
            .paper_sittings_of(added)
            .iter()
            .map(|s| s.to_page.max(s.from_page))
            .max()
            .unwrap_or(0);
        if pages < furthest {
            return Err(Refusal::UnderLogged);
        }
        let book = self.live_paper_book(added)?;
        book.pages = pages;
        book.edited = after(&book.edited, now);
        Ok(())
    }

    /// `set_paper_cover` names a new cover on the book added at `added`, as of `now`: its row
    /// states where it sits on the Kindle ([`on_the_kindle`]) under the name [`cover_name`] makes of
    /// the moment the row takes. Answers that name, the file the picture is written to.
    pub fn set_paper_cover(&mut self, added: &str, now: &str) -> Result<String, Refusal> {
        let book = self.live_paper_book(added)?;
        let at = after(&book.edited, now);
        let name = cover_name(added, &at);
        book.cover = on_the_kindle(&name);
        book.edited = at;
        Ok(name)
    }

    /// `take_paper_cover` takes the cover off the book added at `added`, as of `now`; its file
    /// stays where it is, drawn nowhere.
    pub fn take_paper_cover(&mut self, added: &str, now: &str) -> Result<(), Refusal> {
        let book = self.live_paper_book(added)?;
        book.cover = String::new();
        book.edited = after(&book.edited, now);
        Ok(())
    }

    /// `delete_paper_book` deletes the book added at `added` and every sitting in it, as of `now`:
    /// their rows stand, drawn nowhere.
    pub fn delete_paper_book(&mut self, added: &str, now: &str) -> Result<(), Refusal> {
        let book = self.live_paper_book(added)?;
        book.deleted = true;
        book.edited = after(&book.edited, now);
        for s in self
            .paper_sittings
            .iter_mut()
            .filter(|s| !s.deleted && s.book == added)
        {
            s.deleted = true;
            s.edited = after(&s.edited, now);
        }
        Ok(())
    }

    /// `log_paper_sitting` holds a sitting in the book added at `book`, from `from` to `to` and
    /// from page `from_page` to `to_page`, logged at `now`, moved a second on while another
    /// sitting holds the same moment. Answers the moment that names it.
    pub fn log_paper_sitting(
        &mut self,
        book: &str,
        (from, to): (&str, &str),
        (from_page, to_page): (i64, i64),
        now: &str,
    ) -> Result<String, Refusal> {
        // `logged` is free before `admits` sets aside the sitting it names.
        let mut logged = now.to_string();
        while self.paper_sittings.iter().any(|s| s.logged == logged) {
            logged = later(&logged);
        }
        let fresh = PaperSitting {
            logged: logged.clone(),
            edited: logged.clone(),
            deleted: false,
            book: book.to_string(),
            from: from.to_string(),
            to: to.to_string(),
            from_page,
            to_page,
        };
        self.admits(&fresh, now)?;
        self.paper_sittings.push(fresh);
        self.sort_paper();
        Ok(logged)
    }

    /// `edit_paper_sitting` sets the times and pages of the sitting logged at `logged`, as of
    /// `now`.
    pub fn edit_paper_sitting(
        &mut self,
        logged: &str,
        (from, to): (&str, &str),
        (from_page, to_page): (i64, i64),
        now: &str,
    ) -> Result<(), Refusal> {
        let held = self.live_paper_sitting(logged)?.clone();
        let fresh = PaperSitting {
            edited: after(&held.edited, now),
            from: from.to_string(),
            to: to.to_string(),
            from_page,
            to_page,
            ..held
        };
        self.admits(&fresh, now)?;
        *self.live_paper_sitting(logged)? = fresh;
        Ok(())
    }

    /// `delete_paper_sitting` deletes the sitting logged at `logged`, as of `now`: its row stands,
    /// drawn nowhere.
    pub fn delete_paper_sitting(&mut self, logged: &str, now: &str) -> Result<(), Refusal> {
        let row = self.live_paper_sitting(logged)?;
        row.deleted = true;
        row.edited = after(&row.edited, now);
        Ok(())
    }

    /// `admits` answers why `fresh` may not stand as of `now`, the sitting logged at its own
    /// `logged` aside: it has to close after it opens, within a day and no later than `now`, read
    /// pages inside its book's, and overlap no other paper sitting.
    fn admits(&self, fresh: &PaperSitting, now: &str) -> Result<(), Refusal> {
        let book = self.paper_book(&fresh.book).ok_or(Refusal::NoBook)?;
        let (from, to) = (
            instant(&fresh.from).ok_or(Refusal::NotAnInstant)?,
            instant(&fresh.to).ok_or(Refusal::NotAnInstant)?,
        );
        if to <= from {
            return Err(Refusal::Backwards);
        }
        if to - from > DAY_SECS {
            return Err(Refusal::TooLong);
        }
        if instant(now).is_some_and(|now| to > now) {
            return Err(Refusal::Ahead);
        }
        let pages = 0 <= fresh.from_page && fresh.from_page <= fresh.to_page;
        if !pages || fresh.to_page > book.pages {
            return Err(Refusal::Pages);
        }
        let standing = |b: &str| self.paper_book(b).is_some();
        let overlaps = self
            .paper_sittings
            .iter()
            .filter(|s| !s.deleted && s.logged != fresh.logged && standing(&s.book))
            .filter_map(PaperSitting::span)
            .any(|(opens, closes)| from < closes && opens < to);
        match overlaps {
            true => Err(Refusal::Overlaps),
            false => Ok(()),
        }
    }

    /// `live_paper_book`: the paper book standing that was added at `added`, to change.
    fn live_paper_book(&mut self, added: &str) -> Result<&mut PaperBook, Refusal> {
        self.paper_books
            .iter_mut()
            .find(|b| !b.deleted && b.added == added)
            .ok_or(Refusal::NoBook)
    }

    /// `live_paper_sitting`: the sitting standing that was logged at `logged`, in a paper book
    /// standing, to change.
    fn live_paper_sitting(&mut self, logged: &str) -> Result<&mut PaperSitting, Refusal> {
        let book = self
            .paper_sittings
            .iter()
            .find(|s| !s.deleted && s.logged == logged)
            .map(|s| s.book.clone())
            .ok_or(Refusal::NoSitting)?;
        self.paper_book(&book).ok_or(Refusal::NoBook)?;
        self.paper_sittings
            .iter_mut()
            .find(|s| !s.deleted && s.logged == logged)
            .ok_or(Refusal::NoSitting)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::settings::{SittingFloor, StatsFrom};
    use crate::stats::Stats;

    /// `ADDED`: when [`record`]'s book was added.
    const ADDED: &str = "2026-11-01T10:00:00";

    /// `NOW`: the clock every row in [`record`] was written by.
    const NOW: &str = "2026-11-05T08:00:00";

    /// `record`: one paper book of 200 pages, read 0 to 50 over an hour on the 2nd, then 50 to 80
    /// over the hour around midnight from the 3rd into the 4th.
    fn record() -> Record {
        let mut out = Record::default();
        let added = out
            .add_paper_book("A Paper\tBook", "An Author", "en", 200, ADDED)
            .unwrap();
        assert_eq!(added, ADDED);
        out.log_paper_sitting(
            ADDED,
            ("2026-11-02T20:00:00", "2026-11-02T21:00:00"),
            (0, 50),
            NOW,
        )
        .unwrap();
        out.log_paper_sitting(
            ADDED,
            ("2026-11-03T23:30:00", "2026-11-04T00:30:00"),
            (50, 80),
            NOW,
        )
        .unwrap();
        out
    }

    fn day(stamp: &str) -> i64 {
        date::parse_day(stamp).unwrap()
    }

    #[test]
    fn the_rows_round_trip_through_the_file_words_and_all() {
        let held = record();
        assert_eq!(held.paper_sittings.len(), 2);
        assert_eq!(
            held.paper_sittings[1].logged, "2026-11-05T08:00:01",
            "two logged in one second stay two"
        );
        let text = held.text();
        let read = Record::from_text(&text);
        assert_eq!(read.paper_books, held.paper_books);
        assert_eq!(read.paper_sittings, held.paper_sittings);
        assert_eq!(read.paper_books[0].title, "A Paper\tBook");
        assert_eq!(read.text(), text);
        assert!(
            text.lines()
                .any(|l| l.starts_with("p\t2026-11-01T10:00:00\t"))
        );
        assert!(
            text.lines().all(|l| !l.is_empty()),
            "no row spans two lines"
        );
        assert_eq!(held.paper_books[0].key(), "paper-20261101T100000");
        assert_eq!(
            held.paper_books[0].extent(),
            PAPER_FROM + day("2026-11-01") * DAY_SECS + 10 * 3600
        );
        assert!(is_paper(&held.paper_books[0].key()));
    }

    #[test]
    fn of_two_copies_the_later_edit_stands_and_a_deletion_outlasts_an_older_copy() {
        let mut desk = record();
        let mut kindle = Record::default();
        assert!(kindle.take_written(&desk));
        assert_eq!(kindle.paper_books, desk.paper_books);
        assert_eq!(kindle.paper_sittings, desk.paper_sittings);
        let older = kindle.clone();
        desk.set_paper_pages(ADDED, 240, "2026-11-06T09:00:00")
            .unwrap();
        let logged = desk.paper_sittings[0].logged.clone();
        desk.delete_paper_sitting(&logged, "2026-11-06T09:00:00")
            .unwrap();
        assert!(kindle.take_written(&desk));
        assert_eq!(kindle.paper_books[0].pages, 240);
        assert!(!kindle.take_written(&older), "an older copy moves nothing");
        let mut again = older.clone();
        assert!(again.take_written(&kindle));
        assert!(again.paper_sittings[0].deleted);
        assert_eq!(again.paper_sittings_of(ADDED).len(), 1);
        assert!(
            !again.take_written(&Record::default()),
            "a copy holding none takes none away"
        );
    }

    #[test]
    fn a_splice_owns_the_rows_it_reads_and_keeps_one_it_cannot() {
        let held = record();
        let stray = "p\tsoon\tlater\tlive\t10\tA title\tAn author\ten";
        let text = format!("{}{stray}\n", Record::default().text());
        let spliced = crate::written::splice(&text, &held);
        assert!(spliced.lines().any(|line| line == stray), "{spliced}");
        let read = Record::from_text(&spliced);
        assert_eq!(read.paper_books, held.paper_books);
        assert_eq!(read.paper_sittings, held.paper_sittings);
        assert!(crate::record::closed(&spliced));
        assert_eq!(crate::written::splice(&held.text(), &held), held.text());
    }

    #[test]
    fn a_pass_a_reset_and_a_restore_each_leave_the_rows_standing() {
        let mut held = record();
        held.watermark = "261104:010000".into();
        held.absorb(&[], "261101:000000");
        assert_eq!(held.paper_books.len(), 1);
        assert_eq!(held.paper_sittings.len(), 2);
        assert!(held.reset());
        assert_eq!(held.paper_books.len(), 1);
        assert_eq!(held.paper_sittings.len(), 2);
        let mut restored = Record::default();
        restored.fold_in(&held);
        assert_eq!(restored.paper_books, held.paper_books);
        assert_eq!(restored.paper_sittings, held.paper_sittings);
    }

    #[test]
    fn a_book_and_its_sittings_draw_as_the_figures_a_kindle_states() {
        let drawn = record().drawn().record;
        assert!(drawn.paper_books.is_empty() && drawn.paper_sittings.is_empty());
        assert_eq!(drawn.books.len(), 1);
        let book = &drawn.books[0];
        assert_eq!(book.cde_key, "paper-20261101T100000");
        assert_eq!(book.extent, extent(ADDED).unwrap());
        assert_eq!(
            (
                book.title.as_str(),
                book.author.as_str(),
                book.language.as_str()
            ),
            ("A Paper\tBook", "An Author", "en")
        );
        assert_eq!(book.percent, 40.0);
        assert!(!book.finished && book.kept && !book.on_kindle);
        assert!(book.is_paper() && book.is_book());

        let rows: Vec<(&str, &str, i64, i64)> = drawn
            .sittings
            .iter()
            .map(|s| {
                (
                    s.started_at.as_str(),
                    s.ended_at.as_str(),
                    s.seconds,
                    s.page_turns,
                )
            })
            .collect();
        assert_eq!(
            rows,
            vec![
                ("2026-11-02T20:00:00", "2026-11-02T21:00:00", 3600, 50),
                ("2026-11-03T23:30:00", "2026-11-03T23:59:59", 1800, 15),
                ("2026-11-04T00:00:00", "2026-11-04T00:30:00", 1800, 15),
            ]
        );
        let close = |a: f64, b: f64| (a - b).abs() < 1e-9;
        for (s, (progress, from, to)) in
            drawn
                .sittings
                .iter()
                .zip([(0.25, 0.0, 0.25), (0.325, 0.25, 0.325), (0.4, 0.325, 0.4)])
        {
            assert_eq!(s.end_position, book.extent);
            assert_eq!(s.asin.as_deref(), Some(book.cde_key.as_str()));
            assert_eq!(s.measure, Measure::Counted);
            assert_eq!(s.paged_seconds, s.seconds);
            assert!(close(s.progress.unwrap(), progress));
            assert_eq!(s.stretches.len(), 1);
            assert!(close(s.stretches[0].from, from) && close(s.stretches[0].to, to));
            assert!(close(s.covered, to - from));
            assert!(s.hours.is_empty() && s.words == 0 && s.stated_wpm.is_none());
        }
        // `time_left`: the hour that covered a quarter projects three quarters at three hours.
        assert_eq!(drawn.sittings[0].time_left, Some(10_800));
        let last = drawn.sittings[2].time_left.unwrap();
        assert!((10_799..=10_800).contains(&last), "{last}");
    }

    #[test]
    fn a_sitting_cut_at_midnight_shares_its_pages_and_rounds_them_once() {
        let mut held = Record::default();
        held.add_paper_book("Odd", "", "", 100, ADDED).unwrap();
        held.log_paper_sitting(
            ADDED,
            ("2026-11-03T23:30:00", "2026-11-04T00:30:00"),
            (50, 81),
            NOW,
        )
        .unwrap();
        let drawn = held.drawn().record;
        let turns: Vec<i64> = drawn.sittings.iter().map(|s| s.page_turns).collect();
        assert_eq!(turns, vec![16, 15], "15.5 pages round once, the rest after");
        assert_eq!(
            drawn.sittings[0].stretches[0].to, drawn.sittings[1].stretches[0].from,
            "the stretches meet"
        );
    }

    #[test]
    fn both_figure_settings_state_one_time_one_speed_and_one_projection() {
        let mut held = record();
        held.add_paper_book("Not yet begun", "", "", 120, "2026-11-01T10:05:00")
            .unwrap();
        let today = day("2026-11-05");
        let under = |from| Stats::drawn(&held, today, true, from, SittingFloor::OneMinute);
        let (app, kindle) = (under(StatsFrom::App), under(StatsFrom::Kindle));
        let read = |stats: &Stats, title: &str, from| {
            let b = stats.books.iter().find(|b| b.title == title).unwrap();
            (
                b.read_seconds(from),
                b.page_turns,
                b.sittings,
                b.percent_shown(),
                b.time_left(from),
                b.ppm(from).map(|p| (p * 10.0).round() as i64),
            )
        };
        let book = read(&app, "A Paper\tBook", StatsFrom::App);
        assert_eq!(book, read(&kindle, "A Paper\tBook", StatsFrom::Kindle));
        assert_eq!(book.0, 7200);
        assert_eq!(book.1, 80);
        assert_eq!(book.2, 3);
        assert_eq!(book.3, 40);
        assert_eq!(book.5, Some(7), "80 pages over two hours is 0.7 a minute");
        // `Not yet begun`: listed at zero, its progress at 0.
        let begun = app
            .books
            .iter()
            .find(|b| b.title == "Not yet begun")
            .unwrap();
        assert_eq!(
            (begun.sittings, begun.seconds, begun.percent_shown()),
            (0, 0, 0)
        );
        assert!(begun.is_paper() && !begun.can_mark() && !begun.can_open());
        assert_eq!(app.total_seconds, 7200);
        assert_eq!(
            app.days.len(),
            3,
            "the sitting past midnight counts on both days"
        );
    }

    #[test]
    fn a_book_read_to_its_last_page_stays_finished() {
        let mut held = record();
        held.log_paper_sitting(
            ADDED,
            ("2026-11-04T20:00:00", "2026-11-04T23:00:00"),
            (80, 200),
            NOW,
        )
        .unwrap();
        held.log_paper_sitting(
            ADDED,
            ("2026-11-05T07:00:00", "2026-11-05T07:30:00"),
            (10, 20),
            NOW,
        )
        .unwrap();
        let drawn = held.drawn().record;
        assert!(drawn.books[0].finished);
        assert_eq!(drawn.books[0].percent, 10.0, "the newest sitting stands");
        let stats = Stats::drawn(
            &held,
            day("2026-11-05"),
            true,
            StatsFrom::App,
            SittingFloor::All,
        );
        assert_eq!(stats.books[0].time_left(StatsFrom::App), Some(0));
        assert_eq!(stats.books[0].time_left(StatsFrom::Kindle), Some(0));
    }

    #[test]
    fn a_note_on_a_paper_book_is_laid_in_under_it() {
        let mut held = record();
        let book = held.paper_book(ADDED).unwrap().clone();
        held.add_note(crate::written::Note {
            written: "2026-11-04T09:00:00".into(),
            edited: "2026-11-04T09:00:00".into(),
            key: book.key(),
            extent: book.extent(),
            on: None,
            text: "Read on the train".into(),
            deleted: false,
        });
        let stats = Stats::drawn(
            &held,
            day("2026-11-05"),
            true,
            StatsFrom::App,
            SittingFloor::All,
        );
        assert_eq!(stats.highlights.len(), 1);
        assert_eq!(stats.books[0].highlights, vec![0]);
        assert_eq!(stats.written.len(), 1);
    }

    #[test]
    fn a_sitting_or_a_book_deleted_draws_nowhere() {
        let mut held = record();
        let logged = held.paper_sittings[1].logged.clone();
        held.delete_paper_sitting(&logged, NOW).unwrap();
        assert_eq!(held.drawn().record.sittings.len(), 1);
        assert_eq!(held.bookmark(ADDED), 50);
        held.delete_paper_book(ADDED, NOW).unwrap();
        assert!(held.paper_sittings.iter().all(|s| s.deleted));
        let drawn = held.drawn().record;
        assert!(drawn.books.is_empty() && drawn.sittings.is_empty());
        assert_eq!(held.paper_books.len(), 1, "the row stands");
    }

    #[test]
    fn a_sitting_has_to_close_after_it_opens_within_a_day_and_by_now() {
        let mut held = record();
        let mut log = |from: &str, to: &str, pages: (i64, i64)| {
            held.log_paper_sitting(ADDED, (from, to), pages, NOW)
        };
        let at = |h: &str| format!("2026-11-04T{h}:00");
        assert_eq!(
            log(&at("10:00"), &at("10:00"), (80, 90)),
            Err(Refusal::Backwards)
        );
        assert_eq!(
            log(&at("10:00"), "2026-11-05T10:01:00", (80, 90)),
            Err(Refusal::TooLong)
        );
        assert_eq!(
            log("2026-11-05T07:00:00", "2026-11-05T09:00:00", (80, 90)),
            Err(Refusal::Ahead)
        );
        assert_eq!(
            log(&at("10:00"), &at("11:00"), (90, 80)),
            Err(Refusal::Pages)
        );
        assert_eq!(
            log(&at("10:00"), &at("11:00"), (80, 201)),
            Err(Refusal::Pages)
        );
        assert_eq!(
            log(&at("10:00"), &at("11:00"), (-1, 10)),
            Err(Refusal::Pages)
        );
        assert_eq!(
            log("2026-11-04T00:10", &at("11:00"), (80, 90)),
            Err(Refusal::NotAnInstant)
        );
        // `00:00` to `00:40` on the 4th runs into the sitting that closed at `00:30`.
        assert_eq!(
            log(&at("00:00"), &at("00:40"), (80, 90)),
            Err(Refusal::Overlaps)
        );
        // `20:30` on the 2nd runs into the first sitting, logged at `NOW` as this one is.
        assert_eq!(
            log("2026-11-02T20:30:00", "2026-11-02T20:45:00", (10, 20)),
            Err(Refusal::Overlaps)
        );
        // `00:30` on, it opens where that one closed.
        assert!(log(&at("00:30"), &at("00:40"), (80, 90)).is_ok());
        // `ADDED` aside, no paper book by that name.
        assert_eq!(
            held.log_paper_sitting(
                "2026-11-01T10:00:01",
                (&at("12:00"), &at("13:00")),
                (0, 1),
                NOW
            ),
            Err(Refusal::NoBook)
        );
    }

    #[test]
    fn an_edit_moves_its_own_sitting_and_the_pages_never_fall_under_one_logged() {
        let mut held = record();
        let logged = held.paper_sittings[0].logged.clone();
        // `logged` moves onto its own hour, a minute later: no overlap with itself.
        held.edit_paper_sitting(
            &logged,
            ("2026-11-02T20:01:00", "2026-11-02T21:01:00"),
            (0, 60),
            "2026-11-06T08:00:00",
        )
        .unwrap();
        assert_eq!(held.paper_sittings[0].to_page, 60);
        assert_eq!(held.paper_sittings[0].edited, "2026-11-06T08:00:00");
        assert_eq!(
            held.set_paper_pages(ADDED, 79, NOW),
            Err(Refusal::UnderLogged)
        );
        assert_eq!(held.set_paper_pages(ADDED, 0, NOW), Err(Refusal::NoPages));
        held.set_paper_pages(ADDED, 80, NOW).unwrap();
        assert_eq!(held.paper_books[0].pages, 80);
        assert_eq!(
            held.add_paper_book(" ", "", "", 10, NOW),
            Err(Refusal::Untitled)
        );
        assert_eq!(
            held.add_paper_book("Thin", "", "", 0, NOW),
            Err(Refusal::NoPages)
        );
        let second = held
            .add_paper_book("Same second", "", "", 10, ADDED)
            .unwrap();
        assert_eq!(
            second, "2026-11-01T10:00:01",
            "two added in one second stay two"
        );
    }

    #[test]
    fn an_instant_is_a_whole_stamp_and_nothing_else() {
        assert_eq!(instant("1970-01-02T00:00:01"), Some(DAY_SECS + 1));
        for not in [
            "2026-11-04T10:00",
            "2026-11-04 10:00:00",
            "2026-02-30T10:00:00",
            "2026-11-04T24:00:00",
            "2026-11-04T+1:00:00",
            "",
        ] {
            assert_eq!(instant(not), None, "{not}");
        }
    }

    #[test]
    fn a_paper_key_is_its_whole_shape_and_a_sideload_named_like_one_is_not() {
        assert!(is_paper("paper-20261126T154512"));
        assert!(is_paper("paper-20261126T154512@1"), "one Kindle of several");
        for not in [
            "paper-towns",
            "paper-20261126T15451",
            "paper-20261126T154512x",
            "paper-20261126T154512@",
            "paper-20261126t154512",
            "Paper-20261126T154512",
            "paper-２0261126T154512",
        ] {
            assert!(!is_paper(not), "{not}");
        }
        // `towns`: a sideload its sidecar alone names keeps its Kindle's controls.
        let towns = crate::stats::BookStat {
            cde_key: "paper-towns".into(),
            ..crate::stats::BookStat::default()
        };
        assert!(!towns.is_paper() && towns.can_mark());
    }

    /// `picture`: a small PNG at `path`, the kind of file `covers::drawable` takes.
    fn picture(path: &std::path::Path) {
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        image::RgbImage::from_pixel(4, 6, image::Rgb([40, 90, 160]))
            .save(path)
            .unwrap();
    }

    fn scratch(name: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!("readinglog-paper-{name}"));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn a_cover_rides_the_row_and_draws_as_any_books_cover() {
        let mut held = record();
        // `set_paper_cover` names the file by the moment the row takes: past its last edit.
        let name = held.set_paper_cover(ADDED, "2026-11-06T08:00:00").unwrap();
        assert_eq!(name, "paper_20261101T100000_20261106T080000.jpg");
        assert!(is_cover(&name));
        assert!(!is_cover("paper_20261101T100000.jpg") && !is_cover("B00KEY.jpg"));
        assert_eq!(held.paper_books[0].edited, "2026-11-06T08:00:00");
        assert_eq!(
            held.paper_books[0].cover,
            format!("/mnt/us/extensions/readinglog/covers/{name}")
        );
        let again = held.set_paper_cover(ADDED, "2026-11-06T08:00:00").unwrap();
        assert_ne!(
            again, name,
            "a second cover in one second takes a name of its own"
        );
        let read = Record::from_text(&held.text());
        assert_eq!(
            read.paper_books[0].cover,
            on_the_kindle(&again),
            "the row carries it"
        );
        // `at`: a picture where a test can write one, standing for the Kindle's own path.
        let at = scratch("cover").join("covers").join(&name);
        picture(&at);
        let path = at.to_string_lossy().into_owned();
        held.paper_books[0].cover.clone_from(&path);
        let today = day("2026-11-06");
        let stats = Stats::drawn(&held, today, true, StatsFrom::App, SittingFloor::All);
        assert_eq!(stats.books[0].thumbnail, path);
        assert!(stats.books[0].has_cover());
        // `take_paper_cover`: no cover drawn, whatever file stands.
        held.take_paper_cover(ADDED, "2026-11-07T08:00:00").unwrap();
        assert!(held.paper_books[0].cover.is_empty() && at.exists());
        let stats = Stats::drawn(&held, today, true, StatsFrom::App, SittingFloor::All);
        assert!(!stats.books[0].has_cover());
        held.delete_paper_book(ADDED, NOW).unwrap();
        assert_eq!(held.set_paper_cover(ADDED, NOW), Err(Refusal::NoBook));
    }

    #[test]
    fn the_kindles_sweep_leaves_every_paper_cover_where_the_desktop_put_it() {
        let dir = scratch("sweep");
        let kept = cover_name(ADDED, NOW);
        for name in [
            kept.as_str(),
            "B00KEY.jpg",
            "B00GONE.jpg",
            "paper_towns.jpg",
        ] {
            picture(&dir.join(crate::covers::COVERS_DIR).join(name));
        }
        assert_eq!(crate::covers::sweep(&dir, &["B00KEY"]), 2);
        let left = |name: &str| dir.join(crate::covers::COVERS_DIR).join(name).exists();
        assert!(left(&kept) && left("B00KEY.jpg"));
        assert!(!left("B00GONE.jpg") && !left("paper_towns.jpg"));
    }
}
