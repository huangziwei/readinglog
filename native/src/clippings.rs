//! `My Clippings.txt`, the one place the reader writes an annotation's words
//! down outside the book: plain UTF-8, appended, never rewritten.
//!
//! A record names its book by title and author alone. What turns that into a
//! book is [`crate::identify`], which brackets the record's stamp with a
//! sitting; nothing here reaches the store.

use std::path::Path;

/// The file, one for the whole device. `Clipping.<init>` builds it from
/// `Const.add()` — the `BOOK_DIR_INTERNAL` default — and the literal name.
pub const CLIPPINGS_FILE: &str = "/mnt/us/documents/My Clippings.txt";

/// The line ending one record and opening the next. Byte-identical in every
/// bundle the reader ships, and the format's one invariant.
const SEPARATOR: &str = "==========";

/// What a record marks. `AnnotationTypes` numbers these; `ClippingsManager.C`
/// refuses handwriting, so no firmware writes ink here.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    Bookmark,
    #[default]
    Highlight,
    Note,
    /// `clip_article`, a periodical's article saved whole.
    Article,
    /// Underline, circle and asterisk are 5.19 and later.
    Underline,
    Circle,
    Asterisk,
}

impl Kind {
    /// The number `AnnotationTypes` gives this kind.
    pub fn code(self) -> u8 {
        match self {
            Self::Bookmark => 0,
            Self::Highlight => 1,
            Self::Note => 2,
            Self::Article => 3,
            Self::Circle => 12,
            Self::Underline => 13,
            Self::Asterisk => 14,
        }
    }
}

/// One record: the five lines between two [`SEPARATOR`]s.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct Clipping {
    /// `BookMetadata.getTitle()`, as the reader states it.
    pub title: String,
    /// `BookMetadata.kv()`, a joined list. Empty where the book names none.
    pub author: String,
    pub kind: Kind,
    /// The publisher's page label, where the book carried page numbers. A
    /// string, not a number: front matter gives `xii`.
    pub page: String,
    /// `Position.getUIString()`, the displayed location. Negative where the
    /// label carried none.
    pub start: i64,
    /// The end of a range, equal to [`Self::start`] where one number was
    /// stated.
    pub end: i64,
    /// `YYYY-MM-DDTHH:MM:SS`, device-local, as a sitting stores its own. This
    /// is when the record was appended, not when the reading happened. Empty
    /// where the stamp would not parse.
    pub at: String,
    /// The words: the book's own for a highlight, the user's for a note, empty
    /// for a bookmark.
    pub body: String,
}

/// Every record in the file at `path`, in write order. Empty where there is no
/// file to read.
pub fn read(path: &Path) -> Vec<Clipping> {
    match std::fs::read(path) {
        Ok(bytes) => parse(&String::from_utf8_lossy(&bytes)),
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => Vec::new(),
        Err(err) => {
            eprintln!("clippings: {} — {err}", path.display());
            Vec::new()
        }
    }
}

/// Every record `text` frames. A record that will not parse is dropped and the
/// rest are kept: one torn write must never cost the file.
pub fn parse(text: &str) -> Vec<Clipping> {
    // `Clipping.gQ` raises its BOM flag when the file is absent and never
    // lowers it, so every entry the session that created the file appended
    // carries one. Strip them anywhere, not only at byte 0.
    let text = text.replace('\u{FEFF}', "");
    let mut out = Vec::new();
    let mut record: Vec<&str> = Vec::new();
    for line in text.lines() {
        let line = line.strip_suffix('\r').unwrap_or(line);
        if line == SEPARATOR {
            out.extend(one(&record));
            record.clear();
            continue;
        }
        record.push(line);
    }
    // Whatever `record` still holds ran past the last separator: a torn write.
    out
}

/// One record's lines as a [`Clipping`]. `None` where there are fewer than the
/// three the pattern always writes.
fn one(lines: &[&str]) -> Option<Clipping> {
    // A record opens at the line after the previous separator, which is empty
    // when the writer's own trailing CRLF sits there.
    let lines = match lines.first() {
        Some(&"") => &lines[1..],
        _ => lines,
    };
    if lines.len() < 3 {
        return None;
    }
    let (title, author) = head(lines[0]);
    let (label, tail) = split_label(lines[1]);
    let (page, start, end) = locate(label);
    let body = lines[3..].join("\n");
    Some(Clipping {
        title,
        author,
        kind: kind(label, &body, start, end),
        page,
        start,
        end,
        at: stamp(tail).unwrap_or_default(),
        body,
    })
}

/// The title and author of a head line.
///
/// `{0} {1,choice,0# |1#({2})}`: a book with no author writes the title, the
/// pattern's own space and the `0#` arm's space, so **a line ending in two
/// spaces has no author** however many brackets the title carries.
fn head(line: &str) -> (String, String) {
    if line.ends_with("  ") {
        return (line.trim_end().to_string(), String::new());
    }
    // The last ` (` whose `)` closes the line opens the author.
    if let Some(rest) = line.strip_suffix(')') {
        let mut depth = 1usize;
        for (at, c) in rest.char_indices().rev() {
            match c {
                ')' => depth += 1,
                '(' => {
                    depth -= 1;
                    if depth == 0 {
                        return match rest[..at].strip_suffix(' ') {
                            Some(title) => (title.to_string(), rest[at + 1..].to_string()),
                            None => break,
                        };
                    }
                }
                _ => {}
            }
        }
    }
    (line.trim_end().to_string(), String::new())
}

/// The label and the stamp tail of a label line.
///
/// Split at the **last** space-bar: the page arm puts a ` | ` inside the label
/// too, and Japanese writes a bare `|` there and none after the separator. A
/// leading `-` opens the line in every bundle but Russian, which uses an EN
/// DASH.
fn split_label(line: &str) -> (&str, &str) {
    let line = line
        .trim_start()
        .trim_start_matches(['-', '\u{2013}'])
        .trim_start();
    match line.rfind(" |") {
        Some(at) => (line[..at].trim(), line[at + 2..].trim()),
        None => (line.trim(), ""),
    }
}

/// A fragment of `<kind>.clipping.label.pattern` that names its kind, over the
/// eleven bundles the reader ships.
///
/// **No fragment occurs in another kind's pattern in any bundle**, which is
/// what lets one flat table stand for every language. A fragment added here
/// has to keep that true.
const FRAGMENTS: [(&str, Kind); 37] = [
    // 5.19 English only: the reader ships no localised bundle at that build.
    ("underline", Kind::Underline),
    ("circle", Kind::Circle),
    ("asterisk", Kind::Asterisk),
    ("clip this article", Kind::Article),
    ("artikel", Kind::Article),
    ("artículo", Kind::Article),
    ("articolo", Kind::Article),
    ("artigo", Kind::Article),
    ("extrait", Kind::Article),
    ("вырезка", Kind::Article),
    ("記事クリップ", Kind::Article),
    ("文章剪切", Kind::Article),
    ("bookmark", Kind::Bookmark),
    ("lesezeichen", Kind::Bookmark),
    ("marcador", Kind::Bookmark),
    ("signet", Kind::Bookmark),
    ("segnalibro", Kind::Bookmark),
    ("bladwijzer", Kind::Bookmark),
    ("закладка", Kind::Bookmark),
    ("ブックマーク", Kind::Bookmark),
    ("书签", Kind::Bookmark),
    ("highlight", Kind::Highlight),
    ("markierung", Kind::Highlight),
    ("subrayado", Kind::Highlight),
    ("surlignement", Kind::Highlight),
    ("evidenziazione", Kind::Highlight),
    ("destaque", Kind::Highlight),
    ("выделенный", Kind::Highlight),
    ("ハイライト", Kind::Highlight),
    ("标注", Kind::Highlight),
    ("note", Kind::Note),
    ("notiz", Kind::Note),
    ("nota", Kind::Note),
    ("notitie", Kind::Note),
    ("заметка", Kind::Note),
    ("メモ", Kind::Note),
    ("笔记", Kind::Note),
];

/// What `label` marks, and where it names nothing, what the record's shape
/// says: an empty body is a bookmark, a range is a highlight, and a single
/// location with words is a note.
fn kind(label: &str, body: &str, start: i64, end: i64) -> Kind {
    let folded = label.to_lowercase();
    if let Some((_, kind)) = FRAGMENTS.iter().find(|(word, _)| folded.contains(word)) {
        return *kind;
    }
    match (body.is_empty(), end > start) {
        (true, _) => Kind::Bookmark,
        (false, true) => Kind::Highlight,
        (false, false) => Kind::Note,
    }
}

/// The page label and the location range a label carries.
///
/// The location is plain digits — `user.location.number` is `{0,number,###0}`,
/// with no grouping — so the digit runs are the numbers. Where the label
/// carries a page it also carries a bar, except in Chinese, which brackets the
/// location instead; the run before the location stands for the page there.
fn locate(label: &str) -> (String, i64, i64) {
    let (page_part, loc_part) = match label.rfind('|') {
        Some(at) => (&label[..at], &label[at + 1..]),
        None => ("", label),
    };
    let runs = digit_runs(loc_part);
    // A range only where the writer's own separator sits between the last
    // two runs: `Location 102-102` is a range, and a bracketed Chinese
    // location preceded by its page number is not.
    let ranged = runs.len() > 1 && {
        let (at, _) = runs[runs.len() - 1];
        let (before, _) = runs[runs.len() - 2];
        is_range(&loc_part[before + digits(before, loc_part)..at])
    };
    let (start, end) = match (runs.last(), ranged) {
        (None, _) => (-1, -1),
        (Some(&(_, last)), false) => (last, last),
        (Some(&(_, last)), true) => (runs[runs.len() - 2].1, last),
    };
    // The location took the last run, or the last two where it is a range.
    let took = 1 + usize::from(ranged);
    let page = match token(page_part) {
        Some(page) => page,
        // Chinese brackets the location instead of barring it, so whatever run
        // stands before the location is the page.
        None if runs.len() > took => runs[runs.len() - took - 1].1.to_string(),
        None => String::new(),
    };
    (page, start, end)
}

/// Whether `between` is what `reader.clipping.position.range.pattern` puts
/// between two locations: `-` everywhere but Russian, which uses an EN DASH,
/// and Dutch, which writes ` t/m `.
fn is_range(between: &str) -> bool {
    matches!(between.trim(), "-" | "\u{2013}" | "t/m")
}

/// Every run of ASCII digits in `text`, as `(byte offset, value)`.
fn digit_runs(text: &str) -> Vec<(usize, i64)> {
    let mut out = Vec::new();
    let bytes = text.as_bytes();
    let mut at = 0;
    while at < bytes.len() {
        if !bytes[at].is_ascii_digit() {
            at += 1;
            continue;
        }
        let from = at;
        while at < bytes.len() && bytes[at].is_ascii_digit() {
            at += 1;
        }
        // A run past `i64` is not a location; it is a torn line.
        if let Ok(value) = text[from..at].parse::<i64>() {
            out.push((from, value));
        }
    }
    out
}

/// The length of the digit run at `at`.
fn digits(at: usize, text: &str) -> usize {
    text[at..]
        .bytes()
        .take_while(u8::is_ascii_digit)
        .count()
        .max(1)
}

/// The last run of ASCII letters and digits in `text`, which is the page label
/// the reader interpolated. `None` where there is none: Japanese writes
/// `211ページ`, so the run stops at the first non-ASCII character.
fn token(text: &str) -> Option<String> {
    let mut out: Option<String> = None;
    let mut run = String::new();
    for c in text.chars() {
        if c.is_ascii_alphanumeric() {
            run.push(c);
            continue;
        }
        if !run.is_empty() {
            out = Some(std::mem::take(&mut run));
        }
    }
    if !run.is_empty() {
        out = Some(run);
    }
    // The label's own words are ASCII in six of the bundles; only a run
    // carrying a digit, or a roman numeral, is a page.
    out.filter(|t| t.bytes().any(|b| b.is_ascii_digit()) || roman(t))
}

/// Whether `text` reads as a roman numeral, which is what front matter pages
/// carry.
fn roman(text: &str) -> bool {
    !text.is_empty()
        && text
            .bytes()
            .all(|b| b"ivxlcdmIVXLCDM".contains(&b.to_ascii_lowercase()))
}

/// The twelve month names `FormatData` holds. Every firmware ships English
/// date data and nothing else, so a German device still writes these.
const MONTHS: [&str; 12] = [
    "January",
    "February",
    "March",
    "April",
    "May",
    "June",
    "July",
    "August",
    "September",
    "October",
    "November",
    "December",
];

/// The instant a stamp tail names, as `YYYY-MM-DDTHH:MM:SS`.
///
/// `{4,date,full} {4,time,medium}` in the JVM's default locale, and no
/// firmware carries non-English `FormatData` — 5.16 and 5.18 ship
/// `FormatData_en*` alone in `rt.jar` and 5.19's runtime image has no
/// `jdk.localedata` module. So three shapes, all English: `Wednesday, June 24,
/// 2026`, en_GB's `Wednesday, 24 June 2026` and en_IN's `Wednesday, 24 June,
/// 2026`. The weekday carries nothing and is skipped, and so is the bundle's
/// own word for *Added on*.
fn stamp(tail: &str) -> Option<String> {
    let (at, name) = MONTHS
        .iter()
        .enumerate()
        .find_map(|(i, m)| tail.find(m).map(|at| (at, (i as i64 + 1, m.len()))))?;
    let (month, len) = name;
    let before = digit_runs(&tail[..at]);
    let after = digit_runs(&tail[at + len..]);
    // `June 24, 2026` puts the day after the month; `24 June 2026` before it.
    let (day, year, rest) = match before.last() {
        Some(&(_, day)) if day <= 31 => (day, after.first()?.1, &after[1..]),
        _ => (after.first()?.1, after.get(1)?.1, &after[2..]),
    };
    if !crate::date::is_valid(year, month, day) {
        return None;
    }
    let [(_, hour), (_, minute), (_, second), ..] = rest else {
        return None;
    };
    // `h:mm:ss a` on a 12-hour locale, `HH:mm:ss` on en_GB. The space before
    // the meridiem is U+202F on 5.19, so match the letters and not the gap.
    let meridiem = tail[at + len..].rfind(['A', 'P']).and_then(|i| {
        tail[at + len + i..]
            .starts_with("AM")
            .then_some(0)
            .or_else(|| tail[at + len + i..].starts_with("PM").then_some(12))
    });
    let hour = match meridiem {
        Some(add) => hour % 12 + add,
        None => *hour,
    };
    if hour > 23 || *minute > 59 || *second > 59 {
        return None;
    }
    Some(crate::date::stamp(
        crate::date::days_from_civil(year, month, day),
        hour * 3600 + minute * 60 + second,
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Files the reader would have written, one per firmware and language
    /// mix. Every body is invented.
    ///
    /// A no-author title line **ends in two spaces**, which is what says it
    /// has no author: an editor that trims them breaks these tests.
    mod fixture {
        /// 5.19 English: every kind the file can carry, the 5.19-only
        /// underline, asterisk and circle among them, and a book with
        /// publisher page numbers both numeric and roman.
        pub const SCRIBE_EN: &str = "\
            \u{feff}Nineteen Eighty-Four (George Orwell)\r\n\
            - Your Highlight on Location 4210-4256 | Added on Tuesday, September 1, 2026 9:04:11\u{202f}PM\r\n\
            \r\n\
            The clocks were striking an hour that no clock is built to strike.\r\n\
            ==========\r\n\
            \u{feff}Nineteen Eighty-Four (George Orwell)\r\n\
            - Your Note on Location 4210 | Added on Tuesday, September 1, 2026 9:04:40\u{202f}PM\r\n\
            \r\n\
            the famous opening\r\n\
            ==========\r\n\
            \u{feff}Nineteen Eighty-Four (George Orwell)\r\n\
            - Your Bookmark on Location 4300 | Added on Tuesday, September 1, 2026 9:06:00\u{202f}PM\r\n\
            \r\n\
            \r\n\
            ==========\r\n\
            Nineteen Eighty-Four (George Orwell)\r\n\
            - Your Underline on Location 5000-5031 | Added on Wednesday, September 2, 2026 8:12:30\u{202f}AM\r\n\
            \r\n\
            Two words that cancel each other.\r\n\
            ==========\r\n\
            Nineteen Eighty-Four (George Orwell)\r\n\
            - Your Asterisk on Location 5100-5104 | Added on Wednesday, September 2, 2026 8:13:00\u{202f}AM\r\n\
            \r\n\
            doublethink\r\n\
            ==========\r\n\
            Nineteen Eighty-Four (George Orwell)\r\n\
            - Your Circle on Location 5200-5209 | Added on Wednesday, September 2, 2026 8:14:00\u{202f}AM\r\n\
            \r\n\
            Newspeak\r\n\
            ==========\r\n\
            The Making of the Atomic Bomb (Richard Rhodes)\r\n\
            - Your Highlight on page 211 | Location 12044-12099 | Added on Thursday, September 3, 2026 7:30:15\u{202f}PM\r\n\
            \r\n\
            In the square where the row passes, on a morning of no importance…\r\n\
            ==========\r\n\
            The Making of the Atomic Bomb (Richard Rhodes)\r\n\
            - Your Highlight on page xii | Location 300-340 | Added on Thursday, September 3, 2026 7:31:00\u{202f}PM\r\n\
            \r\n\
            A note from the front matter.\r\n\
            ==========\r\n\
            The Making of the Atomic Bomb (Richard Rhodes)\r\n\
            - Your Highlight on Location 102-102 | Added on Thursday, September 3, 2026 7:32:00\u{202f}PM\r\n\
            \r\n\
            A phrase short enough to sit inside a single location.\r\n\
            ==========\r\n\
        ";

        /// The same reading on 5.16/5.18: the Java 8 stamp, an ASCII space
        /// before the meridiem, and none of the 5.19-only kinds.
        pub const SEABREEZE_EN: &str = "\
            \u{feff}Nineteen Eighty-Four (George Orwell)\r\n\
            - Your Highlight on Location 4210-4256 | Added on Tuesday, September 1, 2026 9:04:11 PM\r\n\
            \r\n\
            A bright cold day, and every clock disagreeing.\r\n\
            ==========\r\n\
            Nineteen Eighty-Four (George Orwell)\r\n\
            - Your Note on Location 4210 | Added on Tuesday, September 1, 2026 9:04:40 PM\r\n\
            \r\n\
            the famous opening\r\n\
            ==========\r\n\
        ";

        /// A German 5.18 device: localised labels around an English stamp.
        pub const SEABREEZE_DE: &str = "\
            \u{feff}Der Prozess (Franz Kafka)\r\n\
            - Deine Markierung bei Position 120-180 | Hinzugefügt am Sunday, August 30, 2026 10:00:00 AM\r\n\
            \r\n\
            Jemand mußte etwas gesagt haben, denn die Tür stand offen.\r\n\
            ==========\r\n\
            Der Prozess (Franz Kafka)\r\n\
            - Dein Lesezeichen bei Position 400 | Hinzugefügt am Sunday, August 30, 2026 10:05:00 AM\r\n\
            \r\n\
            \r\n\
            ==========\r\n\
        ";

        /// Every bundle whose framing a naive parser gets wrong: Russian
        /// opens the line with an EN DASH and writes its range with one;
        /// Dutch writes ` t/m `; Japanese leaves no space after the bar;
        /// Chinese brackets the location instead of barring it, so its page
        /// arm has no separator at all; Italian opens its note location arm
        /// with a space of its own.
        pub const SEABREEZE_MIXED: &str = "\
            \u{feff}Война и мир (Лев Толстой)\r\n\
            – Ваш выделенный отрывок в месте 900–930 | Добавлено: Wednesday, August 12, 2026 в 7:45:00 AM\r\n\
            \r\n\
            Ну, князь, вот и всё, что осталось от того вечера.\r\n\
            ==========\r\n\
            Max Havelaar (Multatuli)\r\n\
            - Je highlight op locatie 55 t/m 88 | Toegevoegd op Thursday, August 13, 2026 8:01:02 PM\r\n\
            \r\n\
            Ik ben makelaar in niets bijzonders.\r\n\
            ==========\r\n\
            こころ (夏目漱石)\r\n\
            - 位置No. 77のメモ |作成日: Friday, August 14, 2026 6:30:00 AM\r\n\
            \r\n\
            先生のこと\r\n\
            ==========\r\n\
            围城 (钱锺书)\r\n\
            - 您在位置 #1500-1540的标注 | 添加于 Saturday, August 15, 2026 10:10:00 PM\r\n\
            \r\n\
            城里城外，谁也说不清是谁在逃\r\n\
            ==========\r\n\
            围城 (钱锺书)\r\n\
            - 您在第 88 页（位置 #1600-1640）的标注 | 添加于 Saturday, August 15, 2026 10:12:00 PM\r\n\
            \r\n\
            第八十八页\r\n\
            ==========\r\n\
            Il nome della rosa (Umberto Eco)\r\n\
            -  La tua nota alla posizione 412 | Aggiunto in data Sunday, August 16, 2026 11:05:00 AM\r\n\
            \r\n\
            il manoscritto\r\n\
            ==========\r\n\
            Le Petit Prince (Antoine de Saint-Exupéry)\r\n\
            - Votre surlignement à lʼemplacement 20-26 | Ajouté le Sunday, August 16, 2026 11:08:00 AM\r\n\
            \r\n\
            On ne voit bien quʼavec le cœur.\r\n\
            ==========\r\n\
            Cien años de soledad (Gabriel García Márquez)\r\n\
            - El marcador en la posición 3 | Añadido el Sunday, August 16, 2026 11:09:00 AM\r\n\
            \r\n\
            \r\n\
            ==========\r\n\
            Memórias Póstumas (Machado de Assis)\r\n\
            - Seu destaque na página xiv | posição 7-9 | Adicionado: Sunday, August 16, 2026 11:10:00 AM\r\n\
            \r\n\
            Ao verme que primeiro roeu as frias carnes.\r\n\
            ==========\r\n\
        ";

        /// Everything that breaks a line-counting parser: no author, an empty
        /// bookmark body, a body carrying a paragraph break, a body carrying
        /// the separator's own shape, a title with its own brackets both with
        /// an author and without, both clipping-limit forms, an emoji, a tab,
        /// and one note appended twice.
        pub const HAZARDS: &str = "\
            \u{feff}A Sideloaded Manuscript  \r\n\
            - Your Highlight on Location 10-42 | Added on Wednesday, July 1, 2026 9:00:00\u{202f}AM\r\n\
            \r\n\
            One line.\r\n\
            ==========\r\n\
            A Sideloaded Manuscript  \r\n\
            - Your Bookmark on Location 60 | Added on Wednesday, July 1, 2026 9:01:00\u{202f}AM\r\n\
            \r\n\
            \r\n\
            ==========\r\n\
            A Sideloaded Manuscript  \r\n\
            - Your Highlight on Location 100-260 | Added on Wednesday, July 1, 2026 9:02:00\u{202f}AM\r\n\
            \r\n\
            The first paragraph ends here.\r\n\
            \r\n\
            And the second begins.\r\n\
            ==========\r\n\
            A Sideloaded Manuscript  \r\n\
            - Your Highlight on Location 300-380 | Added on Wednesday, July 1, 2026 9:03:00\u{202f}AM\r\n\
            \r\n\
            Chapter rule\r\n\
            ==========\r\n\
            Something After (Not An Author)\r\n\
            ==========\r\n\
            Gödel, Escher, Bach (An Eternal Golden Braid) (Douglas Hofstadter)\r\n\
            - Your Highlight on Location 700-760 | Added on Thursday, July 2, 2026 12:00:00\u{202f}PM\r\n\
            \r\n\
            A strange loop.\r\n\
            ==========\r\n\
            Gödel, Escher, Bach (An Eternal Golden Braid)  \r\n\
            - Your Highlight on Location 800-830 | Added on Thursday, July 2, 2026 12:01:00\u{202f}PM\r\n\
            \r\n\
            The same book, sideloaded with no author.\r\n\
            ==========\r\n\
            A Limited Edition (Some Publisher)\r\n\
            - Your Highlight on Location 1000-1400 | Added on Friday, July 3, 2026 8:00:00\u{202f}AM\r\n\
            \r\n\
            The first eleven characters <You have reached the clipping limit set by the publisher, Some Publisher>\r\n\
            ==========\r\n\
            A Limited Edition (Some Publisher)\r\n\
            - Your Highlight on Location 1500-1520 | Added on Friday, July 3, 2026 8:01:00\u{202f}AM\r\n\
            \r\n\
            \u{20}<You have reached the clipping limit for this item>\r\n\
            ==========\r\n\
            A Sideloaded Manuscript  \r\n\
            - Your Note on Location 2000 | Added on Saturday, July 4, 2026 11:59:59\u{202f}PM\r\n\
            \r\n\
            🙂 note\r\n\
            ==========\r\n\
            A Sideloaded Manuscript  \r\n\
            - Your Note on Location 2100 | Added on Saturday, July 4, 2026 11:59:59\u{202f}PM\r\n\
            \r\n\
            \tindented\r\n\
            ==========\r\n\
            A Sideloaded Manuscript  \r\n\
            - Your Note on Location 2000 | Added on Sunday, July 5, 2026 8:00:00\u{202f}AM\r\n\
            \r\n\
            🙂 note, revised\r\n\
            ==========\r\n\
            A British Edition (A. Writer)\r\n\
            - Your Highlight at location 40-44 | Added on Monday, 6 July 2026 23:03:00\r\n\
            \r\n\
            Twenty-four hours and no meridiem.\r\n\
            ==========\r\n\
            An Indian Edition (A. Writer)\r\n\
            - Your Highlight on Location 50-55 | Added on Tuesday, 7 July, 2026 12:32:25 AM\r\n\
            \r\n\
            The day before the month, and a comma after it.\r\n\
            ==========\r\n\
        ";
    }

    fn at(text: &str) -> Vec<Clipping> {
        parse(text)
    }

    #[test]
    fn every_kind_the_file_can_carry_is_read_off_its_label() {
        let got = at(fixture::SCRIBE_EN);
        assert_eq!(got.len(), 9);
        let kinds: Vec<Kind> = got[..6].iter().map(|c| c.kind).collect();
        assert_eq!(
            kinds,
            [
                Kind::Highlight,
                Kind::Note,
                Kind::Bookmark,
                Kind::Underline,
                Kind::Asterisk,
                Kind::Circle,
            ],
            "the three 5.19 kinds are the ones a 5.18 parser would miss",
        );
        assert_eq!(got[0].title, "Nineteen Eighty-Four");
        assert_eq!(got[0].author, "George Orwell");
        assert_eq!((got[0].start, got[0].end), (4_210, 4_256));
        // The 5.19 stamp puts U+202F before the meridiem, which does not
        // split on an ASCII space.
        assert_eq!(got[0].at, "2026-09-01T21:04:11");
        assert_eq!(got[1].end, got[1].start, "a note carries no end position");
        assert_eq!(got[2].body, "", "a bookmark writes no words");
    }

    #[test]
    fn a_page_label_is_a_string_and_the_location_sits_past_the_bar() {
        let got = at(fixture::SCRIBE_EN);
        assert_eq!(got[6].page, "211");
        assert_eq!((got[6].start, got[6].end), (12_044, 12_099));
        assert_eq!(got[7].page, "xii", "front matter is a roman numeral");
        assert_eq!((got[7].start, got[7].end), (300, 340));
        // A range whose two locations render the same number is still a range:
        // the writer collapses on `Position.equals`, not on the digits.
        assert_eq!(got[8].page, "");
        assert_eq!((got[8].start, got[8].end), (102, 102));
    }

    #[test]
    fn the_java_8_stamp_reads_the_same_as_the_cldr_one() {
        let cldr = at(fixture::SCRIBE_EN);
        let jre = at(fixture::SEABREEZE_EN);
        assert_eq!(jre.len(), 2);
        assert_eq!(jre[0].at, cldr[0].at, "one instant, two space characters");
        assert_eq!(jre[1].at, cldr[1].at);
    }

    #[test]
    fn a_localised_label_names_its_kind_around_an_english_stamp() {
        let got = at(fixture::SEABREEZE_DE);
        assert_eq!(got[0].kind, Kind::Highlight);
        assert_eq!(got[0].title, "Der Prozess");
        assert_eq!(got[0].at, "2026-08-30T10:00:00");
        assert_eq!((got[0].start, got[0].end), (120, 180));
        assert_eq!(got[1].kind, Kind::Bookmark);
        assert_eq!(got[1].body, "");
    }

    #[test]
    fn the_bundles_that_do_not_frame_a_record_like_english_still_read() {
        let got = at(fixture::SEABREEZE_MIXED);
        assert_eq!(got.len(), 9);
        // Russian: an EN DASH opens the line and separates the range, and
        // ` в ` splits the stamp.
        assert_eq!(got[0].kind, Kind::Highlight);
        assert_eq!(got[0].title, "Война и мир");
        assert_eq!((got[0].start, got[0].end), (900, 930));
        assert_eq!(got[0].at, "2026-08-12T07:45:00");
        // Dutch writes ` t/m `.
        assert_eq!((got[1].start, got[1].end), (55, 88));
        // Japanese bars inside the label and not after it.
        assert_eq!(got[2].kind, Kind::Note);
        assert_eq!((got[2].start, got[2].end), (77, 77));
        assert_eq!(got[2].at, "2026-08-14T06:30:00");
        // Chinese brackets the location and never bars at all, so the page is
        // whatever run stands before it — with a range or without.
        assert_eq!(got[3].kind, Kind::Highlight);
        assert_eq!(got[3].page, "");
        assert_eq!((got[3].start, got[3].end), (1_500, 1_540));
        assert_eq!(got[4].page, "88");
        assert_eq!((got[4].start, got[4].end), (1_600, 1_640));
        // Italian opens its note location arm with a space of its own.
        assert_eq!(got[5].kind, Kind::Note);
        assert_eq!(got[5].title, "Il nome della rosa");
        // French, Spanish and Portuguese, the last on its page arm.
        assert_eq!(got[6].kind, Kind::Highlight);
        assert_eq!(got[7].kind, Kind::Bookmark);
        assert_eq!(got[8].kind, Kind::Highlight);
        assert_eq!(got[8].page, "xiv");
        assert_eq!((got[8].start, got[8].end), (7, 9));
    }

    #[test]
    fn a_title_with_its_own_brackets_keeps_them_when_there_is_no_author() {
        let got = at(fixture::HAZARDS);
        assert_eq!(
            got[4].title,
            "Gödel, Escher, Bach (An Eternal Golden Braid)"
        );
        assert_eq!(got[4].author, "Douglas Hofstadter");
        assert_eq!(
            got[5].title, "Gödel, Escher, Bach (An Eternal Golden Braid)",
            "the same line ending in two spaces is all title",
        );
        assert_eq!(got[5].author, "", "two trailing spaces mean no author");
        assert_eq!(got[0].title, "A Sideloaded Manuscript");
        assert_eq!(got[0].author, "");
    }

    #[test]
    fn a_body_is_taken_whole_and_a_record_it_frames_through_is_the_only_loss() {
        let got = at(fixture::HAZARDS);
        assert_eq!(
            got[2].body, "The first paragraph ends here.\n\nAnd the second begins.",
            "a paragraph break was folded away",
        );
        // A body carrying the separator's own shape frames through: the
        // record ends at it, and what follows has too few lines to stand.
        // There is nothing else the frame can be.
        assert_eq!(got[3].body, "Chapter rule");
        assert!(got.iter().all(|c| c.title != "Something After"));
        // The clipping limit, both forms, the second opening with a space.
        assert!(
            got[6]
                .body
                .ends_with("set by the publisher, Some Publisher>")
        );
        assert!(got[7].body.starts_with(' '));
        assert_eq!(got[9].body, "\tindented", "a tab survived the parse");
        // An edited note is appended a second time, and both records stand.
        assert_eq!(got[8].start, got[10].start);
        assert_eq!(got[8].body, "🙂 note");
        assert_eq!(got[10].body, "🙂 note, revised");
    }

    #[test]
    fn all_three_english_stamp_shapes_read() {
        let got = at(fixture::HAZARDS);
        // en_GB: the day before the month, 24 hours, no meridiem.
        assert_eq!(got[11].at, "2026-07-06T23:03:00");
        assert_eq!((got[11].start, got[11].end), (40, 44));
        // en_IN: the day before the month, a comma after it, and midnight as
        // `12:32:25 AM`.
        assert_eq!(got[12].at, "2026-07-07T00:32:25");
    }

    #[test]
    fn a_record_the_writer_never_wrote_is_dropped_and_the_file_stands() {
        let torn = concat!(
            "A Good Book (An Author)\r\n",
            "- Your Highlight on Location 1-2 | Added on Sunday, ",
            "June 7, 2026 11:00:00 AM\r\n\r\n",
            "Kept.\r\n",
            "==========\r\n",
            "Two lines and no more\r\n",
            "==========\r\n",
            "Another Book (An Author)\r\n",
            "- Your Highlight on Location 3-4 | Added on Sunday, ",
            "June 7, 2026 11:05:00 AM\r\n\r\n",
            "Also kept.\r\n",
            "==========\r\n",
            "A torn write with no separator after it\r\n",
        );
        let got = at(torn);
        assert_eq!(got.len(), 2);
        assert_eq!(got[0].body, "Kept.");
        assert_eq!(got[1].body, "Also kept.");
    }

    #[test]
    fn a_stamp_naming_no_day_leaves_the_record_unplaced() {
        let bad = concat!(
            "A Book (An Author)\r\n",
            "- Your Highlight on Location 1-2 | Added on Sunday, ",
            "June 31, 2026 11:00:00 AM\r\n\r\n",
            "A line.\r\n",
            "==========\r\n",
        );
        let got = at(bad);
        assert_eq!(got.len(), 1);
        assert_eq!(got[0].at, "", "June has thirty days");
        assert_eq!(got[0].title, "A Book", "the record went with its stamp");
    }

    #[test]
    fn every_bom_the_first_session_wrote_is_stripped() {
        // `Clipping.gQ` raises the flag when the file is absent and never
        // lowers it, so the fixture carries one on each of the first entries.
        assert!(fixture::SCRIBE_EN.matches('\u{FEFF}').count() > 1);
        let got = at(fixture::SCRIBE_EN);
        assert!(got.iter().all(|c| !c.title.contains('\u{FEFF}')));
        assert_eq!(got[0].title, "Nineteen Eighty-Four");
    }

    #[test]
    fn a_label_with_no_words_this_knows_falls_back_to_the_records_shape() {
        let odd = concat!(
            "A Book (An Author)\r\n",
            "- Klipp pa position 10-12 | Added on Sunday, ",
            "June 7, 2026 11:00:00 AM\r\n\r\n",
            "A range with words.\r\n",
            "==========\r\n",
            "A Book (An Author)\r\n",
            "- Klipp pa position 20 | Added on Sunday, ",
            "June 7, 2026 11:01:00 AM\r\n\r\n",
            "One location with words.\r\n",
            "==========\r\n",
            "A Book (An Author)\r\n",
            "- Klipp pa position 30 | Added on Sunday, ",
            "June 7, 2026 11:02:00 AM\r\n\r\n\r\n",
            "==========\r\n",
        );
        let got = at(odd);
        assert_eq!(got[0].kind, Kind::Highlight, "a range is a highlight");
        assert_eq!(got[1].kind, Kind::Note, "one location with words");
        assert_eq!(got[2].kind, Kind::Bookmark, "no words at all");
    }

    #[test]
    fn the_kind_codes_are_the_ones_annotation_types_names() {
        assert_eq!(
            [
                Kind::Bookmark.code(),
                Kind::Highlight.code(),
                Kind::Note.code(),
                Kind::Article.code(),
                Kind::Circle.code(),
                Kind::Underline.code(),
                Kind::Asterisk.code(),
            ],
            [0, 1, 2, 3, 12, 13, 14],
        );
    }
}
