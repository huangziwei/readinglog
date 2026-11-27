//! Paper books written here: `p` and `q` rows of the record of a Kindle this
//! workspace holds, which that Kindle takes at its next sync and draws as it
//! draws its own books, and their covers beside the Kindle's own. [`add_book`]
//! … [`delete`] write them, each refusal a `paper::Refusal`, [`NoKindle`] or
//! [`NotAPicture`] the window states in its own words.

use std::path::Path;

use anyhow::{Context, Result, anyhow};
use serde::Deserialize;

use readinglog_native::covers::COVERS_DIR;
use readinglog_native::date;
use readinglog_native::paper::{self, Refusal};

use crate::kindles::Source;
use crate::metadata::suggested_language;
use crate::workspace::{Workspace, keep_written, now, write_file};

/// `NewBook`: a paper book as the window adds it; a blank `language` takes the
/// one its characters say, where they say one, and `cover` names a picture on
/// disk to take as its cover.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct NewBook {
    pub title: String,
    #[serde(default)]
    pub author: String,
    #[serde(default)]
    pub language: String,
    pub pages: i64,
    #[serde(default)]
    pub cover: Option<String>,
}

/// `Read`: a sitting as the window logs it: its day (`YYYY-MM-DD`), the minute
/// it opened and the minute it closed (`HH:MM`), the next day's where that is
/// earlier, and the pages it opened and closed on.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct Read {
    pub day: String,
    pub from: String,
    pub to: String,
    pub from_page: i64,
    pub to_page: i64,
}

impl Read {
    /// `stamps`: when it opened and closed, as a record stamps them; `None`
    /// where `day`, `from` or `to` is no day or time.
    fn stamps(&self) -> Option<(String, String)> {
        let day = date::parse_day(self.day.trim()).filter(|_| self.day.trim().len() == 10)?;
        let minute = |at: &str| -> Option<i64> {
            let mut parts = at.trim().split(':');
            let mut next = || -> Option<i64> {
                let part = parts.next()?;
                let digits = part.len() == 2 && part.bytes().all(|b| b.is_ascii_digit());
                digits.then(|| part.parse().ok()).flatten()
            };
            let (h, m) = (next()?, next()?);
            (h < 24 && m < 60).then_some(h * 3600 + m * 60)
        };
        let (from, to) = (minute(&self.from)?, minute(&self.to)?);
        let closes = match to < from {
            true => day + 1,
            false => day,
        };
        Some((date::stamp(day, from), date::stamp(closes, to)))
    }
}

/// `NoKindle`: why a paper book has nowhere to go: the workspace holds no
/// Kindle's record for its rows to ride in.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct NoKindle;

impl std::fmt::Display for NoKindle {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("a paper book goes into a Kindle's record, and this folder holds none yet")
    }
}

impl std::error::Error for NoKindle {}

/// `NotAPicture`: why a cover was not taken: its file decodes as neither a JPEG
/// nor a PNG.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct NotAPicture;

impl std::fmt::Display for NotAPicture {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("a cover has to be a JPEG or a PNG")
    }
}

impl std::error::Error for NotAPicture {}

/// `COVER_BOX`: the box a cover is fitted within, never sampled up to it: past
/// the window's largest cover at twice its pixels, and the Kindle's.
const COVER_BOX: (u32, u32) = (400, 600);

/// `COVER_QUALITY`: the JPEG quality a cover is written at.
const COVER_QUALITY: u8 = 88;

/// `fitted`: the picture at `path` as a cover is kept: turned the way its EXIF
/// orientation says, fitted within [`COVER_BOX`], its transparency laid on
/// white, a baseline JPEG with no metadata; [`NotAPicture`] where it decodes as
/// neither a JPEG nor a PNG.
pub fn fitted(path: &Path) -> Result<Vec<u8>> {
    use image::ImageDecoder as _;
    let reader = image::ImageReader::open(path)
        .with_context(|| format!("reading {}", path.display()))?
        .with_guessed_format()
        .with_context(|| format!("reading {}", path.display()))?;
    let mut decoder = reader.into_decoder().map_err(|_| NotAPicture)?;
    let turned = decoder
        .orientation()
        .unwrap_or(image::metadata::Orientation::NoTransforms);
    let mut picture = image::DynamicImage::from_decoder(decoder).map_err(|_| NotAPicture)?;
    picture.apply_orientation(turned);
    let (w, h) = COVER_BOX;
    if picture.width() > w || picture.height() > h {
        picture = picture.resize(w, h, image::imageops::FilterType::Lanczos3);
    }
    let rgba = picture.to_rgba8();
    let on_white = image::RgbImage::from_fn(rgba.width(), rgba.height(), |x, y| {
        let [r, g, b, a] = rgba.get_pixel(x, y).0;
        let lay = |c: u8| ((u32::from(c) * u32::from(a) + 255 * (255 - u32::from(a))) / 255) as u8;
        image::Rgb([lay(r), lay(g), lay(b)])
    });
    let mut out = Vec::new();
    image::codecs::jpeg::JpegEncoder::new_with_quality(&mut out, COVER_QUALITY)
        .encode_image(&on_white)
        .context("writing a cover")?;
    Ok(out)
}

/// `cover_into` writes `bytes` as a new cover of the book added at `added`, into
/// the covers folder beside `source`'s record under the name its row takes.
fn cover_into(source: &mut Source, added: &str, bytes: &[u8]) -> Result<()> {
    let name = source.record.set_paper_cover(added, &now())?;
    write_file(&source.dir.join(COVERS_DIR).join(name), bytes)
}

/// `kindles`: the cut serials of the Kindles whose records `ws` holds, which a
/// paper book can go into.
pub fn kindles(ws: &Workspace) -> Vec<String> {
    ws.sources().into_iter().filter_map(|s| s.serial).collect()
}

/// `source`: the record of the Kindle `kindle` names, the one Kindle `ws`
/// holds where `kindle` is empty; [`NoKindle`] where it holds none.
fn source(ws: &Workspace, kindle: &str) -> Result<Source> {
    let mut held: Vec<Source> = ws
        .sources()
        .into_iter()
        .filter(|s| s.serial.is_some())
        .collect();
    if held.is_empty() {
        return Err(NoKindle.into());
    }
    let at = match (kindle.is_empty(), held.len()) {
        (true, 1) => 0,
        _ => held
            .iter()
            .position(|s| s.serial.as_deref() == Some(kindle))
            .ok_or_else(|| anyhow!("no record of {kindle} in {}", ws.dir.display()))?,
    };
    Ok(held.swap_remove(at))
}

/// `add_book` adds `book` to the record of the Kindle `kindle` names, at the
/// clock's moment, its cover with it where it names one, and answers the key
/// the book draws under. A cover that is no picture adds nothing.
pub fn add_book(ws: &Workspace, kindle: &str, book: &NewBook) -> Result<String> {
    let mut source = source(ws, kindle)?;
    let language = match book.language.trim() {
        "" => suggested_language(&book.title, &book.author).unwrap_or_default(),
        code => code,
    };
    let cover = match book.cover.as_deref().filter(|p| !p.is_empty()) {
        Some(path) => Some(fitted(Path::new(path))?),
        None => None,
    };
    let added =
        source
            .record
            .add_paper_book(&book.title, &book.author, language, book.pages, &now())?;
    if let Some(bytes) = cover {
        cover_into(&mut source, &added, &bytes)?;
    }
    keep_written(&source)?;
    Ok(paper::key(&added))
}

/// `set_cover` takes the picture at `picture` as the cover of the book added at
/// `added`, [`fitted`] and written beside the Kindle's covers here; the next
/// sync puts it on the Kindle.
pub fn set_cover(ws: &Workspace, kindle: &str, added: &str, picture: &Path) -> Result<()> {
    let mut source = source(ws, kindle)?;
    source.record.paper_book(added).ok_or(Refusal::NoBook)?;
    let bytes = fitted(picture)?;
    cover_into(&mut source, added, &bytes)?;
    keep_written(&source)
}

/// `remove_cover` takes the cover off the book added at `added`.
pub fn remove_cover(ws: &Workspace, kindle: &str, added: &str) -> Result<()> {
    let mut source = source(ws, kindle)?;
    source.record.take_paper_cover(added, &now())?;
    keep_written(&source)
}

/// `set_pages` sets the pages of the book added at `added`.
pub fn set_pages(ws: &Workspace, kindle: &str, added: &str, pages: i64) -> Result<()> {
    let mut source = source(ws, kindle)?;
    source.record.set_paper_pages(added, pages, &now())?;
    keep_written(&source)
}

/// `sittings_in`: how many sittings stand in the book added at `added`, the ones
/// [`delete_book`] takes with it.
pub fn sittings_in(ws: &Workspace, kindle: &str, added: &str) -> Result<usize> {
    Ok(source(ws, kindle)?.record.paper_sittings_of(added).len())
}

/// `delete_book` deletes the book added at `added`, its sittings with it.
pub fn delete_book(ws: &Workspace, kindle: &str, added: &str) -> Result<()> {
    let mut source = source(ws, kindle)?;
    source.record.delete_paper_book(added, &now())?;
    keep_written(&source)
}

/// `log` logs `read` in the book added at `book`, and answers the moment that
/// names the sitting.
pub fn log(ws: &Workspace, kindle: &str, book: &str, read: &Read) -> Result<String> {
    let mut source = source(ws, kindle)?;
    let (from, to) = read.stamps().ok_or(Refusal::NotAnInstant)?;
    let logged = source.record.log_paper_sitting(
        book,
        (&from, &to),
        (read.from_page, read.to_page),
        &now(),
    )?;
    keep_written(&source)?;
    Ok(logged)
}

/// `edit` sets the sitting logged at `logged` to `read`.
pub fn edit(ws: &Workspace, kindle: &str, logged: &str, read: &Read) -> Result<()> {
    let mut source = source(ws, kindle)?;
    let (from, to) = read.stamps().ok_or(Refusal::NotAnInstant)?;
    source.record.edit_paper_sitting(
        logged,
        (&from, &to),
        (read.from_page, read.to_page),
        &now(),
    )?;
    keep_written(&source)
}

/// `delete` deletes the sitting logged at `logged`.
pub fn delete(ws: &Workspace, kindle: &str, logged: &str) -> Result<()> {
    let mut source = source(ws, kindle)?;
    source.record.delete_paper_sitting(logged, &now())?;
    keep_written(&source)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tests::{book, record_of, scratch, sitting};
    use readinglog_native::record::Record;

    /// `with_kindle`: a workspace holding the record of the Kindle `G000K1`, one book read once.
    fn with_kindle(name: &str) -> Workspace {
        let ws = Workspace::create(&scratch(name)).unwrap();
        let record = record_of(
            vec![book(100, "B1", "One")],
            vec![sitting("2026-09-01T09:00:00", 100, 600)],
        );
        record
            .save(&ws.kindle_dir("G000K1").join("record"))
            .unwrap();
        ws
    }

    fn read(day: &str, from: &str, to: &str, pages: (i64, i64)) -> Read {
        Read {
            day: day.into(),
            from: from.into(),
            to: to.into(),
            from_page: pages.0,
            to_page: pages.1,
        }
    }

    #[test]
    fn a_sitting_is_a_day_and_two_minutes_the_close_running_into_the_next_day() {
        let stamps = |r: Read| r.stamps();
        assert_eq!(
            stamps(read("2026-09-02", "21:10", "21:55", (0, 1))),
            Some(("2026-09-02T21:10:00".into(), "2026-09-02T21:55:00".into()))
        );
        assert_eq!(
            stamps(read("2026-09-02", "23:20", "00:30", (0, 1))),
            Some(("2026-09-02T23:20:00".into(), "2026-09-03T00:30:00".into()))
        );
        assert_eq!(
            stamps(read("2026-09-02", "21:10:30", "21:55", (0, 1))),
            Some(("2026-09-02T21:10:00".into(), "2026-09-02T21:55:00".into())),
            "a time naming its seconds keeps its minute"
        );
        for (day, from, to) in [
            ("2026-09-02", "", "21:55"),
            ("2026-9-2", "21:10", "21:55"),
            ("2026-09-02", "24:00", "01:00"),
            ("2026-09-02", "9:5", "10:00"),
        ] {
            assert_eq!(
                stamps(read(day, from, to, (0, 1))),
                None,
                "{day} {from} {to}"
            );
        }
    }

    #[test]
    fn a_paper_book_and_its_sittings_land_in_the_kindles_record() {
        let ws = with_kindle("paper-lands");
        let key = add_book(
            &ws,
            "",
            &NewBook {
                title: "こころ".into(),
                author: "夏目漱石".into(),
                language: String::new(),
                pages: 300,
                cover: None,
            },
        )
        .unwrap();
        let held = Record::load(&ws.kindle_dir("G000K1").join("record"));
        assert_eq!(held.paper_books.len(), 1);
        assert_eq!(held.paper_books[0].language, "ja", "the kana say Japanese");
        assert_eq!(held.paper_books[0].key(), key);
        let added = held.paper_books[0].added.clone();
        let logged = log(
            &ws,
            "G000K1",
            &added,
            &read("2026-09-02", "21:10", "21:55", (0, 40)),
        )
        .unwrap();
        // `again` overlaps `logged`: refused in the record's own words.
        let again = log(
            &ws,
            "G000K1",
            &added,
            &read("2026-09-02", "21:30", "22:00", (40, 50)),
        )
        .unwrap_err();
        assert_eq!(again.downcast_ref::<Refusal>(), Some(&Refusal::Overlaps));
        edit(
            &ws,
            "G000K1",
            &logged,
            &read("2026-09-02", "21:10", "22:10", (0, 45)),
        )
        .unwrap();
        assert_eq!(
            set_pages(&ws, "G000K1", &added, 44)
                .unwrap_err()
                .downcast_ref::<Refusal>(),
            Some(&Refusal::UnderLogged)
        );
        set_pages(&ws, "G000K1", &added, 320).unwrap();
        let held = Record::load(&ws.kindle_dir("G000K1").join("record"));
        assert_eq!(held.paper_books[0].pages, 320);
        assert_eq!(held.paper_sittings.len(), 1);
        assert_eq!(held.paper_sittings[0].to, "2026-09-02T22:10:00");
        assert_eq!(held.sittings.len(), 1, "the Kindle's own rows stand");
        let drawn = held.drawn().record;
        assert_eq!(drawn.books.len(), 2);
        assert_eq!(drawn.sittings.len(), 2);
        // `on`: a note written on the paper book, laid in under it.
        let on = crate::notes::On::Book {
            key: key.clone(),
            extent: held.paper_books[0].extent(),
        };
        crate::notes::write(&ws, "G000K1", &on, "Read on the train").unwrap();
        let drawn = Record::load(&ws.kindle_dir("G000K1").join("record")).drawn();
        assert_eq!(drawn.written.len(), 1);
        let book = drawn
            .record
            .books
            .iter()
            .find(|b| b.cde_key == key)
            .unwrap();
        assert_eq!(drawn.record.highlights[0].extent, book.extent);
        delete(&ws, "G000K1", &logged).unwrap();
        delete_book(&ws, "G000K1", &added).unwrap();
        let held = Record::load(&ws.kindle_dir("G000K1").join("record"));
        assert!(held.paper_books[0].deleted && held.paper_sittings[0].deleted);
        assert_eq!(held.drawn().record.books.len(), 1);
    }

    #[test]
    fn a_folder_holding_no_kindle_has_nowhere_to_put_one_and_several_ask_which() {
        let ws = Workspace::create(&scratch("paper-nowhere")).unwrap();
        let new = NewBook {
            title: "A Book".into(),
            author: String::new(),
            language: "en".into(),
            pages: 100,
            cover: None,
        };
        let err = add_book(&ws, "", &new).unwrap_err();
        assert_eq!(err.downcast_ref::<NoKindle>(), Some(&NoKindle));
        let ws = with_kindle("paper-several");
        record_of(vec![book(200, "B2", "Two")], Vec::new())
            .save(&ws.kindle_dir("G000K2").join("record"))
            .unwrap();
        assert_eq!(kindles(&ws), vec!["G000K1", "G000K2"]);
        assert!(add_book(&ws, "", &new).is_err(), "two Kindles: which one");
        add_book(&ws, "G000K2", &new).unwrap();
        let held = Record::load(&ws.kindle_dir("G000K2").join("record"));
        assert_eq!(held.paper_books.len(), 1);
    }

    /// `saved`: a picture `w` by `h` at `path`, a JPEG or a PNG by its extension, every pixel
    /// `rgba`.
    fn saved(path: &std::path::Path, (w, h): (u32, u32), rgba: [u8; 4]) {
        let picture = image::RgbaImage::from_pixel(w, h, image::Rgba(rgba));
        match path.extension().and_then(|e| e.to_str()) {
            Some("png") => picture.save(path).unwrap(),
            _ => image::DynamicImage::ImageRgba8(picture)
                .to_rgb8()
                .save(path)
                .unwrap(),
        }
    }

    /// `size`: the width and height of the JPEG `bytes` hold.
    fn size(bytes: &[u8]) -> (u32, u32) {
        assert!(bytes.starts_with(b"\xff\xd8\xff"), "a JPEG");
        let picture = image::load_from_memory_with_format(bytes, image::ImageFormat::Jpeg).unwrap();
        (picture.width(), picture.height())
    }

    #[test]
    fn a_cover_is_fitted_into_its_box_turned_upright_and_laid_on_white() {
        let dir = scratch("paper-fitted");
        let at = |name: &str| dir.join(name);
        saved(&at("wide.jpg"), (1200, 800), [200, 40, 40, 255]);
        assert_eq!(size(&fitted(&at("wide.jpg")).unwrap()), (400, 267));
        saved(&at("tall.png"), (300, 2000), [30, 60, 140, 255]);
        assert_eq!(size(&fitted(&at("tall.png")).unwrap()), (90, 600));
        // `small`, transparent: never sampled up, and laid on white.
        saved(&at("small.png"), (100, 150), [0, 0, 0, 0]);
        let small = fitted(&at("small.png")).unwrap();
        assert_eq!(size(&small), (100, 150));
        let pixel = image::load_from_memory(&small)
            .unwrap()
            .to_rgb8()
            .get_pixel(50, 75)
            .0;
        assert!(pixel.iter().all(|c| *c > 245), "{pixel:?}");
        // `turned`: a phone's photo, 40 by 20 as stored, its EXIF saying it stands turned a
        // quarter: an APP1 segment, `Exif`, a little-endian TIFF header and one IFD entry,
        // Orientation (0x0112) at 6.
        saved(&at("flat.jpg"), (40, 20), [40, 120, 90, 255]);
        let flat = std::fs::read(at("flat.jpg")).unwrap();
        let mut exif = vec![0xFF, 0xE1, 0x00, 0x22];
        exif.extend_from_slice(b"Exif\0\0II\x2A\0\x08\0\0\0\x01\0");
        exif.extend_from_slice(&[0x12, 0x01, 0x03, 0x00, 0x01, 0x00, 0x00, 0x00]);
        exif.extend_from_slice(&[0x06, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00]);
        let turned = [&flat[..2], &exif, &flat[2..]].concat();
        std::fs::write(at("turned.jpg"), turned).unwrap();
        assert_eq!(size(&fitted(&at("turned.jpg")).unwrap()), (20, 40));
        // `notes.txt`: no picture at all.
        std::fs::write(at("notes.txt"), "not a picture").unwrap();
        let err = fitted(&at("notes.txt")).unwrap_err();
        assert_eq!(err.downcast_ref::<NotAPicture>(), Some(&NotAPicture));
    }

    #[test]
    fn a_cover_set_here_is_written_beside_the_kindles_and_named_on_its_row() {
        let ws = with_kindle("paper-cover");
        let dir = ws.kindle_dir("G000K1").join("record");
        let picture = scratch("paper-cover-picture").join("front.png");
        saved(&picture, (800, 1200), [180, 140, 40, 255]);
        let new = NewBook {
            title: "On Paper".into(),
            author: String::new(),
            language: "en".into(),
            pages: 200,
            cover: Some(picture.to_string_lossy().into_owned()),
        };
        add_book(&ws, "", &new).unwrap();
        let held = Record::load(&dir);
        let book = &held.paper_books[0];
        let name = std::path::Path::new(&book.cover).file_name().unwrap();
        assert!(paper::is_cover(&name.to_string_lossy()));
        assert_eq!(book.cover, paper::on_the_kindle(&name.to_string_lossy()));
        let written = std::fs::read(dir.join(COVERS_DIR).join(name)).unwrap();
        assert_eq!(size(&written), (400, 600));
        // `set_cover` again: a file of its own; `remove_cover` leaves both files standing.
        let added = book.added.clone();
        set_cover(&ws, "", &added, &picture).unwrap();
        let again = Record::load(&dir).paper_books[0].cover.clone();
        assert_ne!(again, book.cover);
        remove_cover(&ws, "", &added).unwrap();
        assert!(Record::load(&dir).paper_books[0].cover.is_empty());
        let files = std::fs::read_dir(dir.join(COVERS_DIR)).unwrap().count();
        assert_eq!(files, 2);
        // `notes`: no picture, and no book added for it.
        let notes = scratch("paper-cover-notes").join("notes.txt");
        std::fs::write(&notes, "not a picture").unwrap();
        let bad = NewBook {
            cover: Some(notes.to_string_lossy().into_owned()),
            ..new
        };
        let err = add_book(&ws, "", &bad).unwrap_err();
        assert_eq!(err.downcast_ref::<NotAPicture>(), Some(&NotAPicture));
        assert_eq!(Record::load(&dir).paper_books.len(), 1);
    }
}
