//! The interface language: what the device is set to, and every word this app
//! draws in it. [`Strings`] is one struct of `&'static str`, and a language
//! that forgets a word does not compile. Nothing is loaded at runtime.

use std::path::Path;

/// `template` with `{d}` set to `count`, and an ending in brackets kept only
/// where `count` is not one: `{d} DAY[S]` reads "1 DAY" and "30 DAYS". A
/// language with one form writes no brackets.
pub fn counted(template: &str, count: i64) -> String {
    let mut out = String::with_capacity(template.len());
    let mut dropping = false;
    for ch in template.chars() {
        match ch {
            '[' => dropping = count == 1,
            ']' => dropping = false,
            _ if dropping => {}
            _ => out.push(ch),
        }
    }
    out.replace("{d}", &count.to_string())
}

/// `template` with `{v}` set to `version`. Beside [`counted`]: a language
/// whose word order puts the number elsewhere moves the brace.
pub fn at_version(template: &str, version: &str) -> String {
    template.replace("{v}", version)
}

/// The languages the interface is written in.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Lang {
    #[default]
    English,
    German,
    Japanese,
    SimplifiedChinese,
    TraditionalChinese,
}

/// Where the device writes the locale the config picker set.
const LOCALE_FILE: &str = "/var/local/system/locale";

impl Lang {
    /// Every language, in the order the config page lists them.
    pub const ALL: [Lang; 5] = [
        Lang::English,
        Lang::German,
        Lang::SimplifiedChinese,
        Lang::TraditionalChinese,
        Lang::Japanese,
    ];

    /// What the button says: one character each, in the language's own script.
    /// 简 says everything 简体中文 does in a fifth of the width, and five of
    /// these stand on one line where five spelled-out names do not.
    pub fn label(self) -> &'static str {
        match self {
            Lang::English => "EN",
            Lang::German => "DE",
            Lang::SimplifiedChinese => "简",
            Lang::TraditionalChinese => "繁",
            Lang::Japanese => "日",
        }
    }

    /// What the setting is stored as: one letter.
    pub fn letter(self) -> char {
        match self {
            Lang::English => 'e',
            Lang::German => 'd',
            Lang::SimplifiedChinese => 'c',
            Lang::TraditionalChinese => 't',
            Lang::Japanese => 'j',
        }
    }

    /// The language a stored letter names. Anything else is English.
    pub fn from_letter(s: &str) -> Lang {
        match s.trim() {
            "d" => Lang::German,
            "c" => Lang::SimplifiedChinese,
            "t" => Lang::TraditionalChinese,
            "j" => Lang::Japanese,
            _ => Lang::English,
        }
    }

    /// The convention this language's own labels are set in, as a tag
    /// `font::Script::of_language` reads. A tag, never a `Script`: this module
    /// compiles into the host library beside `date`, which cannot see `ui`.
    pub fn language_tag(self) -> &'static str {
        match self {
            Lang::English => "en",
            Lang::German => "de",
            Lang::Japanese => "ja",
            Lang::SimplifiedChinese => "zh-Hans",
            Lang::TraditionalChinese => "zh-Hant",
        }
    }

    /// The words themselves.
    pub fn strings(self) -> &'static Strings {
        match self {
            Lang::English => &ENGLISH,
            Lang::German => &GERMAN,
            Lang::Japanese => &JAPANESE,
            Lang::SimplifiedChinese => &SIMPLIFIED,
            Lang::TraditionalChinese => &TRADITIONAL,
        }
    }

    /// What the device is set to. English wherever the file is missing,
    /// unreadable, or names a language [`Lang`] does not carry.
    pub fn detect() -> Lang {
        Self::detect_in(Path::new(LOCALE_FILE))
    }

    /// [`Lang::detect`] against a named file.
    pub fn detect_in(path: &Path) -> Lang {
        std::fs::read_to_string(path)
            .ok()
            .as_deref()
            .and_then(of_locale_file)
            .unwrap_or(Lang::English)
    }
}

/// The language a `LANG=` line names, or `None` where there is no such line.
/// The file is shell: the value may be quoted and `LC_ALL` may sit beside it.
/// Only `LANG` is read — the two are written together and `LANG` is first.
fn of_locale_file(text: &str) -> Option<Lang> {
    let value = text.lines().find_map(|line| {
        line.trim()
            .strip_prefix("LANG=")
            .map(|v| v.trim_matches(['"', '\'']))
    })?;
    of_posix(value)
}

/// The language a POSIX locale names. **The codeset rides on the region** —
/// the device writes `zh_CN.utf8`, not `zh_CN` — and every subtag is cut at
/// its first `.` before it is read.
pub fn of_posix(value: &str) -> Option<Lang> {
    let mut subtags = value
        .split(['-', '_'])
        .map(|s| s.split('.').next().unwrap_or_default().trim());
    let primary = subtags.next()?.to_ascii_lowercase();
    match primary.as_str() {
        "de" => Some(Lang::German),
        "ja" => Some(Lang::Japanese),
        "zh" => {
            // A region is read where one is given: nothing else names
            // Traditional.
            for subtag in subtags {
                match subtag.to_ascii_lowercase().as_str() {
                    "hant" | "tw" | "hk" | "mo" => return Some(Lang::TraditionalChinese),
                    _ => {}
                }
            }
            Some(Lang::SimplifiedChinese)
        }
        "en" => Some(Lang::English),
        // es, fr, it, nl, pt, ru — locales this app has no words for.
        _ => None,
    }
}

/// Every word the interface draws. Durations keep `h` and `m` in German:
/// `5 Std 8 Min` sets three figures across the Book screen at 1258 px into a
/// row 1186 px wide. Chinese and Japanese take their own units, which fit.
pub struct Strings {
    // The strip along the bottom.
    pub exit: &'static str,
    pub config: &'static str,
    pub today: &'static str,
    pub rhythm: &'static str,
    pub books: &'static str,
    // Today. The three figures, then the heading over the day's books — the
    // day's own date heads its timeline and needs no word here.
    pub read_today: &'static str,
    pub pages_turned: &'static str,
    pub current_streak: &'static str,
    pub what_was_read: &'static str,

    // Rhythm. The four spans the picker offers, the heading over the hour
    // columns, and the word before the busiest hour of them.
    pub all_time: &'static str,
    pub week: &'static str,
    /// The week's number in its year, written around the digits: `W38`,
    /// `KW 38`, `第38週`. A language that puts nothing after them leaves
    /// [`Strings::week_no_after`] empty.
    pub week_no: &'static str,
    pub week_no_after: &'static str,
    pub month: &'static str,
    pub year: &'static str,

    // All Time. The line stating what the record covers, `{m}` its opening
    // month and `{d}` the days it runs to, then the twelve figures under it.
    pub since_days: &'static str,
    pub total_read: &'static str,
    pub days_read: &'static str,
    pub a_day: &'static str,
    pub book_count: &'static str,
    pub finished: &'static str,
    pub a_book: &'static str,
    pub longest_streak: &'static str,
    pub weeks_running: &'static str,
    pub best_day: &'static str,
    pub longest_sitting: &'static str,
    pub a_sitting: &'static str,

    // The Books screen, whose two chips narrow the list to one shelf.
    pub shelf_every: &'static str,
    pub shelf_finished: &'static str,
    pub shelf_unfinished: &'static str,
    /// What a shelf holding nothing states.
    pub nothing_on_the_shelf: &'static str,
    /// The orders the Books screen lists in.
    pub by_recent: &'static str,
    pub by_longest: &'static str,
    pub by_furthest: &'static str,
    /// The heading over each band of the Trends page: an average day and an
    /// average week, then what the record holds in each month of the year.
    pub an_average_day: &'static str,
    pub an_average_week: &'static str,
    pub by_month: &'static str,
    /// The heading over the sitting histogram, and the word closing its count.
    pub sitting_lengths: &'static str,
    pub in_all: &'static str,
    /// The word before the fullest column of a fold.
    pub most: &'static str,
    /// The second page of All Time, named at the right of its own top line.
    pub trends: &'static str,
    /// The chip returning a span page to the one holding today.
    pub now: &'static str,
    /// The chip opening the day picked off a grid as its own page.
    pub open_day: &'static str,
    pub nothing_read: &'static str,

    // Books. `{a}–{b} of {c}`, where `of` is this word.
    pub of: &'static str,

    // Book.
    pub the_reading: &'static str,
    pub sittings: &'static str,
    pub days: &'static str,
    pub average_a_day: &'static str,
    pub average_a_sitting: &'static str,
    pub words: &'static str,
    pub reading_speed: &'static str,
    pub wpm: &'static str,
    pub started: &'static str,
    pub last_read: &'static str,
    /// The day a book read through was last put down.
    pub finished_on: &'static str,
    pub on_the_device: &'static str,
    /// The control handing a book back to the Kindle's reader: the long form
    /// where it stands alone, the short one where `restart` stands beside it.
    pub continue_reading: &'static str,
    pub continue_short: &'static str,
    /// The answer that marks a book read through, and the answer that takes the
    /// mark off, each under the question it answers.
    pub mark_finished: &'static str,
    pub mark_ask: &'static str,
    pub mark_note: &'static str,
    pub mark_unfinished: &'static str,
    pub unmark_ask: &'static str,
    pub unmark_note: &'static str,
    /// The figure over the place a bar's reading stands at. `{d}` is `percent`,
    /// rounded.
    pub percent_at: &'static str,
    /// The place a book stood at as a day or a span ended. `{d}` is that
    /// figure, rounded.
    pub percent_reached: &'static str,
    /// The figure on a bar whose own fill states the same place. `{d}` is that
    /// figure, rounded.
    pub percent_plain: &'static str,
    /// The control that hands a book back to be read from its beginning, the
    /// question it puts, and what that question states.
    pub restart: &'static str,
    pub restart_ask: &'static str,
    pub restart_note: &'static str,
    /// The way out of a question, beside the word the question is asked in.
    pub cancel: &'static str,
    pub yes: &'static str,
    pub no: &'static str,
    /// The heading over a book's own reading, start to finish. `{d}` is the
    /// days it spans, filled by [`counted`].
    pub the_journey: &'static str,
    pub read: &'static str,
    pub left: &'static str,

    // Splash.
    pub first_run_1: &'static str,
    pub first_run_2: &'static str,
    pub catching_up: &'static str,

    // Config.
    /// The section headings, and the name of each setting's row.
    pub interface: &'static str,
    pub the_calendar: &'static str,
    pub language_row: &'static str,
    pub week_starts_on: &'static str,
    /// The reading section of the config page, and the row setting whether a
    /// total counts books the catalog names none of.
    pub the_record: &'static str,
    pub unnamed_row: &'static str,
    pub unnamed_show: &'static str,
    pub unnamed_hide: &'static str,
    /// `{n} unidentified · {duration}`, closing a list of books.
    pub unidentified: &'static str,
    /// `{n} books in the record`, under the last page of the Books list.
    pub in_the_record: &'static str,
    pub text_size: &'static str,
    /// The colours the charts draw in.
    pub color_scheme: &'static str,
    /// One name per scheme, in `ColorScheme::ALL` order.
    pub color_schemes: [&'static str; 6],
    pub size_small: &'static str,
    pub size_medium: &'static str,
    pub size_large: &'static str,

    // Updating.
    /// The About section of the config page: the version this build is, and
    /// the button that goes looking for a newer one.
    pub about: &'static str,
    pub version_row: &'static str,
    pub update_row: &'static str,
    pub update_check: &'static str,
    /// The banner, while an update runs. `update_tap_to_stop` is the only way
    /// out of it.
    pub update_asking: &'static str,
    pub update_downloading: &'static str,
    pub update_checking: &'static str,
    pub update_placing: &'static str,
    pub update_tap_to_stop: &'static str,
    /// How it ended. `{v}` is a version — see [`at_version`].
    pub update_up_to_date: &'static str,
    pub update_this_version: &'static str,
    pub update_installed: &'static str,
    pub update_reopen: &'static str,
    pub update_stopped: &'static str,
    pub update_failed: &'static str,
    pub update_by_hand: &'static str,
    pub update_offline: &'static str,
    pub update_no_answer: &'static str,
    pub update_no_release: &'static str,
    pub update_bad_download: &'static str,
    pub update_wrong_build: &'static str,
    pub update_not_placed: &'static str,

    /// Hours and minutes, appended to a number with no space in English and
    /// CJK, with one in German.
    pub hours: &'static str,
    pub minutes: &'static str,
    pub unit_space: bool,

    /// Whether a date runs year first: `2026年9月3日` against `3 September
    /// 2026`. Set for Chinese and Japanese.
    pub date_ymd: bool,

    pub months: [&'static str; 12],
    pub months_short: [&'static str; 12],
    pub weekdays_short: [&'static str; 7],
}

const ENGLISH: Strings = Strings {
    exit: "Exit",
    config: "Config",
    today: "Today",
    rhythm: "Rhythm",
    books: "Books",

    read_today: "read today",
    pages_turned: "pages turned",
    current_streak: "current streak",
    what_was_read: "WHAT WAS READ",

    all_time: "All Time",
    week: "Week",
    week_no: "W",
    week_no_after: "",
    month: "Month",
    year: "Year",

    since_days: "SINCE {m} · {d} DAY[S]",
    total_read: "read",
    days_read: "days read",
    a_day: "a day",
    book_count: "books",
    finished: "finished",
    a_book: "a book",
    longest_streak: "longest streak",
    weeks_running: "weeks running",
    best_day: "best day",
    longest_sitting: "longest sitting",
    a_sitting: "a sitting",

    shelf_every: "All",
    shelf_finished: "Finished",
    shelf_unfinished: "Unfinished",
    nothing_on_the_shelf: "No books here.",
    by_recent: "Recent",
    by_longest: "Longest",
    by_furthest: "Furthest",
    an_average_day: "AN AVERAGE DAY",
    an_average_week: "AN AVERAGE WEEK",
    by_month: "BY MONTH",
    sitting_lengths: "HOW MANY SITTINGS RAN THAT LONG",
    in_all: "in all",
    most: "MOST",
    trends: "TRENDS",
    now: "Now",
    open_day: "Open",
    nothing_read: "Nothing read.",

    of: "of",

    the_reading: "THE READING",
    sittings: "Sittings",
    days: "Days",
    average_a_day: "Average a day",
    average_a_sitting: "Average a sitting",
    words: "Words",
    reading_speed: "Reading speed",
    wpm: "wpm",
    started: "Started",
    last_read: "Last read",
    finished_on: "Finished on",
    on_the_device: "On the device",
    continue_reading: "Continue reading",
    continue_short: "Continue",
    mark_finished: "Mark Finished",
    mark_ask: "Mark this book finished?",
    mark_note: "It goes onto the Finished shelf, and the library marks it read. The \
                progress, the time, the sittings and the days read do not change.",
    mark_unfinished: "Mark Unfinished",
    unmark_ask: "Remove the Finished mark?",
    unmark_note: "It leaves the Finished shelf, and the library marks it unread. The \
                  progress, the time, the sittings and the days read do not change.",
    percent_at: "{d}% now",
    percent_reached: "at {d}%",
    percent_plain: "{d}%",
    restart: "Restart",
    restart_ask: "Restart this book?",
    restart_note: "The Finished mark comes off, the progress goes back to 0%, and the \
                   book opens at its beginning. Highlights and notes are kept, and so are \
                   the time, the sittings and the days already read.",
    cancel: "Cancel",
    yes: "yes",
    no: "no",
    the_journey: "THE JOURNEY · {d} DAY[S]",
    read: "read",
    left: "left",

    first_run_1: "First run: every log the device still holds",
    first_run_2: "is read once. This can take a few minutes.",
    catching_up: "Reading what the log has added.",

    the_record: "THE RECORD",
    unnamed_row: "Unidentified books",
    unnamed_show: "Show",
    unnamed_hide: "Hide",
    unidentified: "unidentified",
    in_the_record: "books in the record",
    interface: "INTERFACE",
    the_calendar: "THE CALENDAR",
    language_row: "Language",
    week_starts_on: "Week starts on",
    text_size: "Text size",
    color_scheme: "Colour scheme",
    color_schemes: ["Azure", "Teal", "Brown", "Green", "Indigo", "Grey"],
    size_small: "Small",
    size_medium: "Medium",
    size_large: "Large",

    about: "ABOUT",
    version_row: "Version",
    update_row: "Update",
    update_check: "Check now",
    update_asking: "Asking GitHub…",
    update_downloading: "Downloading…",
    update_checking: "Checking it runs here…",
    update_placing: "Putting it in place…",
    update_tap_to_stop: "Tap to stop",
    update_up_to_date: "Up to date",
    update_this_version: "Reading Log {v}",
    update_installed: "Updated to {v}",
    update_reopen: "Close Reading Log and open it again.",
    update_stopped: "Stopped",
    update_failed: "The update did not go through",
    update_by_hand: "Get it on a computer instead:",
    update_offline: "No route off this Kindle. Turn Wi-Fi on.",
    update_no_answer: "GitHub could not be reached.",
    update_no_release: "No release carries an archive.",
    update_bad_download: "The download did not arrive whole.",
    update_wrong_build: "That build does not run on this Kindle.",
    update_not_placed: "The new copy would not go into place.",

    hours: "h",
    minutes: "m",
    unit_space: false,

    date_ymd: false,

    months: [
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
    ],
    months_short: [
        "Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec",
    ],
    weekdays_short: ["Mon", "Tue", "Wed", "Thu", "Fri", "Sat", "Sun"],
};

const GERMAN: Strings = Strings {
    exit: "Beenden",
    // `Einstellungen` sets 235 px, and a cell on the 1264 px panel holds 196.
    config: "Optionen",
    today: "Heute",
    rhythm: "Rhythmus",
    books: "Bücher",

    read_today: "heute gelesen",
    pages_turned: "Seiten",
    current_streak: "aktuelle Serie",
    what_was_read: "WAS GELESEN WURDE",

    all_time: "Gesamt",
    week: "Woche",
    week_no: "KW ",
    week_no_after: "",
    month: "Monat",
    year: "Jahr",

    since_days: "SEIT {m} · {d} TAG[E]",
    total_read: "gelesen",
    days_read: "Tage gelesen",
    a_day: "pro Tag",
    book_count: "Bücher",
    finished: "fertig",
    a_book: "pro Buch",
    longest_streak: "längste Serie",
    weeks_running: "Wochen in Folge",
    best_day: "bester Tag",
    longest_sitting: "längste Sitzung",
    a_sitting: "pro Sitzung",

    shelf_every: "Alle",
    shelf_finished: "Fertig",
    shelf_unfinished: "Unfertig",
    nothing_on_the_shelf: "Keine Bücher hier.",
    by_recent: "Zuletzt",
    by_longest: "Längste",
    by_furthest: "Weiteste",
    an_average_day: "EIN DURCHSCHNITTSTAG",
    an_average_week: "EINE DURCHSCHNITTSWOCHE",
    by_month: "NACH MONAT",
    sitting_lengths: "WIE LANGE EINE SITZUNG DAUERTE",
    in_all: "insgesamt",
    most: "AM MEISTEN",
    trends: "VERLAUF",
    now: "Jetzt",
    open_day: "Öffnen",
    nothing_read: "Nichts gelesen.",

    of: "von",

    the_reading: "DAS LESEN",
    sittings: "Sitzungen",
    days: "Tage",
    average_a_day: "Schnitt pro Tag",
    average_a_sitting: "Schnitt pro Sitzung",
    words: "Wörter",
    reading_speed: "Lesetempo",
    wpm: "W/Min",
    started: "Begonnen",
    last_read: "Zuletzt gelesen",
    finished_on: "Beendet am",
    on_the_device: "Auf dem Gerät",
    continue_reading: "Weiterlesen",
    continue_short: "Weiter",
    mark_finished: "Fertig markieren",
    mark_ask: "Dieses Buch als fertig markieren?",
    mark_note: "Es kommt in das Regal Fertig, und die Bibliothek markiert es als gelesen. \
                Fortschritt, gelesene Zeit, Sitzungen und Tage bleiben unverändert.",
    mark_unfinished: "Unfertig markieren",
    unmark_ask: "Markierung Fertig entfernen?",
    unmark_note: "Es verlässt das Regal Fertig, und die Bibliothek markiert es als \
                  ungelesen. Fortschritt, gelesene Zeit, Sitzungen und Tage bleiben \
                  unverändert.",
    percent_at: "{d} % jetzt",
    percent_reached: "bei {d} %",
    percent_plain: "{d} %",
    restart: "Neu beginnen",
    restart_ask: "Dieses Buch neu beginnen?",
    restart_note: "Die Markierung Fertig wird entfernt, der Fortschritt geht auf 0 % \
                   zurück, und das Buch öffnet sich am Anfang. Markierungen und Notizen \
                   bleiben erhalten, ebenso gelesene Zeit, Sitzungen und Tage.",
    cancel: "Abbrechen",
    yes: "ja",
    no: "nein",
    the_journey: "DER VERLAUF · {d} TAG[E]",
    read: "gelesen",
    left: "übrig",

    first_run_1: "Erster Start: jedes Protokoll auf dem Gerät",
    first_run_2: "wird einmal gelesen. Das dauert einige Minuten.",
    catching_up: "Liest, was dazugekommen ist.",

    the_record: "DIE AUFZEICHNUNG",
    unnamed_row: "Unbekannte Bücher",
    unnamed_show: "Zeigen",
    unnamed_hide: "Verbergen",
    unidentified: "unbekannt",
    in_the_record: "Bücher aufgezeichnet",
    interface: "OBERFLÄCHE",
    the_calendar: "DER KALENDER",
    language_row: "Sprache",
    week_starts_on: "Woche beginnt am",
    text_size: "Schriftgröße",
    color_scheme: "Farbschema",
    color_schemes: ["Azur", "Petrol", "Braun", "Grün", "Indigo", "Grau"],
    size_small: "Klein",
    size_medium: "Mittel",
    size_large: "Groß",

    about: "ÜBER",
    version_row: "Version",
    update_row: "Aktualisierung",
    update_check: "Jetzt suchen",
    update_asking: "GitHub wird gefragt …",
    update_downloading: "Wird heruntergeladen …",
    update_checking: "Läuft es hier? Wird geprüft …",
    update_placing: "Wird eingesetzt …",
    update_tap_to_stop: "Zum Abbrechen tippen",
    update_up_to_date: "Aktuell",
    update_this_version: "Reading Log {v}",
    update_installed: "Aktualisiert auf {v}",
    update_reopen: "Reading Log schließen und neu öffnen.",
    update_stopped: "Abgebrochen",
    update_failed: "Die Aktualisierung ist fehlgeschlagen",
    update_by_hand: "Stattdessen am Computer holen:",
    update_offline: "Kein Weg aus diesem Kindle heraus. WLAN einschalten.",
    update_no_answer: "GitHub war nicht erreichbar.",
    update_no_release: "Keine Veröffentlichung mit einem Archiv.",
    update_bad_download: "Der Download kam nicht vollständig an.",
    update_wrong_build: "Diese Fassung läuft nicht auf diesem Kindle.",
    update_not_placed: "Die neue Fassung ließ sich nicht einsetzen.",

    // `5 Std 8 Min` sets the Book screen's three figures 1258 px across a row
    // 1186 px wide. `h`/`m` are read in German and are what fits.
    hours: "h",
    minutes: "m",
    unit_space: false,

    date_ymd: false,

    months: [
        "Januar",
        "Februar",
        "März",
        "April",
        "Mai",
        "Juni",
        "Juli",
        "August",
        "September",
        "Oktober",
        "November",
        "Dezember",
    ],
    months_short: [
        "Jan", "Feb", "Mär", "Apr", "Mai", "Jun", "Jul", "Aug", "Sep", "Okt", "Nov", "Dez",
    ],
    weekdays_short: ["Mo", "Di", "Mi", "Do", "Fr", "Sa", "So"],
};

const JAPANESE: Strings = Strings {
    exit: "終了",
    config: "設定",
    today: "今日",
    rhythm: "リズム",
    books: "本",

    read_today: "今日の読書",
    pages_turned: "めくったページ",
    current_streak: "継続日数",
    what_was_read: "読んだ本",

    all_time: "全期間",
    week: "週",
    week_no: "第",
    week_no_after: "週",
    month: "月",
    year: "年",

    since_days: "{m}から · {d}日",
    total_read: "読書時間",
    days_read: "読書日数",
    a_day: "一日あたり",
    book_count: "冊数",
    finished: "読了",
    a_book: "一冊あたり",
    longest_streak: "最長継続",
    weeks_running: "連続週数",
    best_day: "最高の一日",
    longest_sitting: "最長の一回",
    a_sitting: "一回あたり",

    shelf_every: "すべて",
    shelf_finished: "読了",
    shelf_unfinished: "未読了",
    nothing_on_the_shelf: "該当する本はありません。",
    by_recent: "最近",
    by_longest: "時間順",
    by_furthest: "進捗",
    an_average_day: "平均的な一日",
    an_average_week: "平均的な一週間",
    by_month: "月ごと",
    sitting_lengths: "一回の読書の長さ",
    in_all: "回",
    most: "最も多い",
    trends: "傾向",
    now: "現在",
    open_day: "開く",
    nothing_read: "読書なし。",

    of: "/",

    the_reading: "読書の記録",
    sittings: "回数",
    days: "日数",
    average_a_day: "一日あたり",
    average_a_sitting: "一回あたり",
    words: "語数",
    reading_speed: "読書速度",
    wpm: "語/分",
    started: "開始",
    last_read: "最終読書",
    finished_on: "読了日",
    on_the_device: "端末内",
    continue_reading: "続きを読む",
    continue_short: "続き",
    mark_finished: "読了にする",
    mark_ask: "この本を読了にしますか？",
    mark_note: "読了の棚に入り、ライブラリでも既読になります。進捗、読んだ時間、回数、日数は変わりません。",
    mark_unfinished: "未読了にする",
    unmark_ask: "読了の印を外しますか？",
    unmark_note: "読了の棚から外れ、ライブラリでも未読になります。進捗、読んだ時間、回数、日数は変わりません。",
    percent_at: "現在{d}%",
    percent_reached: "{d}%まで",
    percent_plain: "{d}%",
    restart: "最初から",
    restart_ask: "この本を最初から読みますか？",
    restart_note: "読了の印が外れ、進捗は0%に戻り、本は最初から開きます。ハイライトとメモ、\
                  これまでの時間・回数・日数はそのまま残ります。",
    cancel: "キャンセル",
    yes: "あり",
    no: "なし",
    the_journey: "読書の歩み · {d}日",
    read: "読了",
    left: "残り",

    first_run_1: "初回起動：端末に残るすべての記録を",
    first_run_2: "一度読み込みます。数分かかります。",
    catching_up: "追加された記録を読み込み中。",

    the_record: "記録",
    unnamed_row: "不明な本",
    unnamed_show: "表示",
    unnamed_hide: "非表示",
    unidentified: "冊が不明",
    in_the_record: "冊を記録",
    interface: "表示",
    the_calendar: "カレンダー",
    language_row: "言語",
    week_starts_on: "週の始まり",
    text_size: "文字の大きさ",
    color_scheme: "配色",
    color_schemes: ["空色", "浅葱", "鳶", "若竹", "紺", "墨"],
    size_small: "小",
    size_medium: "中",
    size_large: "大",

    about: "このアプリ",
    version_row: "バージョン",
    update_row: "更新",
    update_check: "今すぐ確認",
    update_asking: "GitHub に問い合わせ中…",
    update_downloading: "ダウンロード中…",
    update_checking: "この端末で動くか確認中…",
    update_placing: "入れ替え中…",
    update_tap_to_stop: "タップで中止",
    update_up_to_date: "最新です",
    update_this_version: "Reading Log {v}",
    update_installed: "{v} に更新しました",
    update_reopen: "Reading Log を閉じて開き直してください。",
    update_stopped: "中止しました",
    update_failed: "更新できませんでした",
    update_by_hand: "パソコンから入手してください：",
    update_offline: "ネットワークに接続していません。Wi-Fi を入れてください。",
    update_no_answer: "GitHub に接続できませんでした。",
    update_no_release: "配布物のあるリリースがありません。",
    update_bad_download: "ダウンロードが完全ではありませんでした。",
    update_wrong_build: "この Kindle では動かないビルドです。",
    update_not_placed: "新しいファイルを入れ替えられませんでした。",

    hours: "時間",
    minutes: "分",
    unit_space: false,

    date_ymd: true,

    months: [
        "1月", "2月", "3月", "4月", "5月", "6月", "7月", "8月", "9月", "10月", "11月", "12月",
    ],
    months_short: [
        "1月", "2月", "3月", "4月", "5月", "6月", "7月", "8月", "9月", "10月", "11月", "12月",
    ],
    weekdays_short: ["月", "火", "水", "木", "金", "土", "日"],
};

const SIMPLIFIED: Strings = Strings {
    exit: "退出",
    config: "设置",
    today: "今天",
    rhythm: "节奏",
    books: "书",

    read_today: "今日阅读",
    pages_turned: "翻页",
    current_streak: "当前连续",
    what_was_read: "读了什么",

    all_time: "全部时间",
    week: "周",
    week_no: "第",
    week_no_after: "周",
    month: "月",
    year: "年",

    since_days: "自{m} · {d}天",
    total_read: "阅读时长",
    days_read: "阅读天数",
    a_day: "每天",
    book_count: "书籍",
    finished: "已读完",
    a_book: "每本",
    longest_streak: "最长连续",
    weeks_running: "连续周数",
    best_day: "最佳单日",
    longest_sitting: "最长单次",
    a_sitting: "每次",

    shelf_every: "全部",
    shelf_finished: "已读完",
    shelf_unfinished: "未读完",
    nothing_on_the_shelf: "没有符合的书。",
    by_recent: "最近",
    by_longest: "时长",
    by_furthest: "进度",
    an_average_day: "平均的一天",
    an_average_week: "平均的一周",
    by_month: "按月",
    sitting_lengths: "每次阅读持续多久",
    in_all: "次",
    most: "最多",
    trends: "趋势",
    now: "现在",
    open_day: "打开",
    nothing_read: "没有阅读。",

    of: "/",

    the_reading: "阅读记录",
    sittings: "次数",
    days: "天数",
    average_a_day: "每天平均",
    average_a_sitting: "每次平均",
    words: "字数",
    reading_speed: "阅读速度",
    wpm: "字/分",
    started: "开始",
    last_read: "最近阅读",
    finished_on: "读完于",
    on_the_device: "在设备上",
    continue_reading: "继续阅读",
    continue_short: "继续",
    mark_finished: "标记读完",
    mark_ask: "将这本书标记为读完？",
    mark_note: "本书将进入已读完书架，图书馆中也标记为已读。进度、已读的时间、次数与天数不变。",
    mark_unfinished: "标记未读完",
    unmark_ask: "取消读完标记？",
    unmark_note: "本书将离开已读完书架，图书馆中也标记为未读。进度、已读的时间、次数与天数不变。",
    percent_at: "当前{d}%",
    percent_reached: "至{d}%",
    percent_plain: "{d}%",
    restart: "重新开始",
    restart_ask: "从头重读这本书？",
    restart_note: "读完标记将被取消，进度归零，本书将从头打开。标注与笔记，\
                  以及已记录的时间、次数和天数，都会保留。",
    cancel: "取消",
    yes: "是",
    no: "否",
    the_journey: "阅读历程 · {d}天",
    read: "已读",
    left: "剩余",

    first_run_1: "首次运行：设备上保留的每份记录",
    first_run_2: "都会读取一次，需要几分钟。",
    catching_up: "正在读取新增的记录。",

    the_record: "记录",
    unnamed_row: "未识别的书",
    unnamed_show: "显示",
    unnamed_hide: "隐藏",
    unidentified: "本未识别",
    in_the_record: "本已记录",
    interface: "界面",
    the_calendar: "日历",
    language_row: "语言",
    week_starts_on: "每周开始于",
    text_size: "字号",
    color_scheme: "配色",
    color_schemes: ["天蓝", "浅葱", "鸢", "若竹", "绀", "墨"],
    size_small: "小",
    size_medium: "中",
    size_large: "大",

    about: "关于",
    version_row: "版本",
    update_row: "更新",
    update_check: "立即检查",
    update_asking: "正在询问 GitHub…",
    update_downloading: "正在下载…",
    update_checking: "正在检查能否在此运行…",
    update_placing: "正在替换…",
    update_tap_to_stop: "点击停止",
    update_up_to_date: "已是最新",
    update_this_version: "Reading Log {v}",
    update_installed: "已更新到 {v}",
    update_reopen: "请关闭 Reading Log 后重新打开。",
    update_stopped: "已停止",
    update_failed: "更新未能完成",
    update_by_hand: "请在电脑上获取：",
    update_offline: "此 Kindle 没有网络。请打开 Wi-Fi。",
    update_no_answer: "无法连接 GitHub。",
    update_no_release: "没有带安装包的发布。",
    update_bad_download: "下载不完整。",
    update_wrong_build: "该版本无法在此 Kindle 上运行。",
    update_not_placed: "新文件无法就位。",

    hours: "小时",
    minutes: "分",
    unit_space: false,

    date_ymd: true,

    months: [
        "一月",
        "二月",
        "三月",
        "四月",
        "五月",
        "六月",
        "七月",
        "八月",
        "九月",
        "十月",
        "十一月",
        "十二月",
    ],
    months_short: [
        "1月", "2月", "3月", "4月", "5月", "6月", "7月", "8月", "9月", "10月", "11月", "12月",
    ],
    weekdays_short: ["周一", "周二", "周三", "周四", "周五", "周六", "周日"],
};

const TRADITIONAL: Strings = Strings {
    exit: "結束",
    config: "設定",
    today: "今天",
    rhythm: "節奏",
    books: "書",

    read_today: "今日閱讀",
    pages_turned: "翻頁",
    current_streak: "目前連續",
    what_was_read: "讀了什麼",

    all_time: "全部時間",
    week: "週",
    week_no: "第",
    week_no_after: "週",
    month: "月",
    year: "年",

    since_days: "自{m} · {d}天",
    total_read: "閱讀時長",
    days_read: "閱讀天數",
    a_day: "每天",
    book_count: "書籍",
    finished: "已讀完",
    a_book: "每本",
    longest_streak: "最長連續",
    weeks_running: "連續週數",
    best_day: "最佳單日",
    longest_sitting: "最長單次",
    a_sitting: "每次",

    shelf_every: "全部",
    shelf_finished: "已讀完",
    shelf_unfinished: "未讀完",
    nothing_on_the_shelf: "沒有符合的書。",
    by_recent: "最近",
    by_longest: "時長",
    by_furthest: "進度",
    an_average_day: "平均的一天",
    an_average_week: "平均的一週",
    by_month: "按月",
    sitting_lengths: "每次閱讀持續多久",
    in_all: "次",
    most: "最多",
    trends: "趨勢",
    now: "現在",
    open_day: "開啟",
    nothing_read: "沒有閱讀。",

    of: "/",

    the_reading: "閱讀記錄",
    sittings: "次數",
    days: "天數",
    average_a_day: "每天平均",
    average_a_sitting: "每次平均",
    words: "字數",
    reading_speed: "閱讀速度",
    wpm: "字/分",
    started: "開始",
    last_read: "最近閱讀",
    finished_on: "讀完於",
    on_the_device: "在裝置上",
    continue_reading: "繼續閱讀",
    continue_short: "繼續",
    mark_finished: "標記讀完",
    mark_ask: "將這本書標記為讀完？",
    mark_note: "本書將進入已讀完書架，圖書館中也標記為已讀。進度、已讀的時間、次數與天數不變。",
    mark_unfinished: "標記未讀完",
    unmark_ask: "取消讀完標記？",
    unmark_note: "本書將離開已讀完書架，圖書館中也標記為未讀。進度、已讀的時間、次數與天數不變。",
    percent_at: "目前{d}%",
    percent_reached: "至{d}%",
    percent_plain: "{d}%",
    restart: "重新開始",
    restart_ask: "從頭重讀這本書？",
    restart_note: "讀完標記將被取消，進度歸零，本書將從頭開啟。標註與筆記，\
                  以及已記錄的時間、次數和天數，都會保留。",
    cancel: "取消",
    yes: "是",
    no: "否",
    the_journey: "閱讀歷程 · {d}天",
    read: "已讀",
    left: "剩餘",

    first_run_1: "首次執行：裝置上保留的每份記錄",
    first_run_2: "都會讀取一次，需要幾分鐘。",
    catching_up: "正在讀取新增的記錄。",

    the_record: "記錄",
    unnamed_row: "未識別的書",
    unnamed_show: "顯示",
    unnamed_hide: "隱藏",
    unidentified: "本未識別",
    in_the_record: "本已記錄",
    interface: "介面",
    the_calendar: "日曆",
    language_row: "語言",
    week_starts_on: "每週開始於",
    text_size: "字級",
    color_scheme: "配色",
    color_schemes: ["天藍", "淺蔥", "鳶", "若竹", "紺", "墨"],
    size_small: "小",
    size_medium: "中",
    size_large: "大",

    about: "關於",
    version_row: "版本",
    update_row: "更新",
    update_check: "立即檢查",
    update_asking: "正在詢問 GitHub…",
    update_downloading: "正在下載…",
    update_checking: "正在檢查能否在此執行…",
    update_placing: "正在替換…",
    update_tap_to_stop: "點擊停止",
    update_up_to_date: "已是最新",
    update_this_version: "Reading Log {v}",
    update_installed: "已更新至 {v}",
    update_reopen: "請關閉 Reading Log 後重新開啟。",
    update_stopped: "已停止",
    update_failed: "更新未能完成",
    update_by_hand: "請在電腦上取得：",
    update_offline: "此 Kindle 沒有網路。請開啟 Wi-Fi。",
    update_no_answer: "無法連線 GitHub。",
    update_no_release: "沒有附安裝檔的版本。",
    update_bad_download: "下載不完整。",
    update_wrong_build: "此版本無法在這台 Kindle 上執行。",
    update_not_placed: "新檔案無法就位。",

    hours: "小時",
    minutes: "分",
    unit_space: false,

    date_ymd: true,

    months: [
        "一月",
        "二月",
        "三月",
        "四月",
        "五月",
        "六月",
        "七月",
        "八月",
        "九月",
        "十月",
        "十一月",
        "十二月",
    ],
    months_short: [
        "1月", "2月", "3月", "4月", "5月", "6月", "7月", "8月", "9月", "10月", "11月", "12月",
    ],
    weekdays_short: ["週一", "週二", "週三", "週四", "週五", "週六", "週日"],
};

#[cfg(test)]
mod tests {
    #[test]
    fn a_counted_template_takes_the_ending_that_goes_with_its_number() {
        assert_eq!(counted("{d} DAY[S]", 1), "1 DAY");
        assert_eq!(counted("{d} DAY[S]", 30), "30 DAYS");
        assert_eq!(counted("SEIT {m} · {d} TAG[E]", 1), "SEIT {m} · 1 TAG");
        // A language whose noun does not change writes no brackets.
        assert_eq!(counted("{d}\u{65e5}", 1), "1\u{65e5}");
        // Every language's own line reads for one day and for many.
        for lang in Lang::ALL {
            let s = lang.strings();
            for said in [s.since_days, s.the_journey] {
                for n in [1, 30] {
                    let out = counted(said, n);
                    assert!(!out.contains(['[', ']']), "{lang:?}: {out}");
                    assert!(out.contains(&n.to_string()), "{lang:?}: {out}");
                }
            }
        }
    }

    use super::*;

    /// Every value that can reach [`LOCALE_FILE`]. Nothing else reaches
    /// [`of_posix`].
    const DEVICE_LOCALES: &[(&str, Option<Lang>)] = &[
        ("de_DE.utf8", Some(Lang::German)),
        ("ja_JP.utf8", Some(Lang::Japanese)),
        ("zh_CN.utf8", Some(Lang::SimplifiedChinese)),
        ("en_GB.utf8", Some(Lang::English)),
        ("en_US.utf8", Some(Lang::English)),
        ("es_AR.utf8", None),
        ("es_CL.utf8", None),
        ("es_CO.utf8", None),
        ("es_ES.utf8", None),
        ("es_MX.utf8", None),
        ("fr_CA.utf8", None),
        ("fr_FR.utf8", None),
        ("it_IT.utf8", None),
        ("nl_NL.utf8", None),
        ("pt_BR.utf8", None),
        ("ru_RU.utf8", None),
    ];

    #[test]
    fn every_locale_the_device_can_be_set_to_is_read() {
        for (value, want) in DEVICE_LOCALES {
            assert_eq!(of_posix(value), *want, "{value}");
        }
    }

    #[test]
    fn the_codeset_does_not_hide_the_region() {
        // `zh_CN.utf8` splits to ["zh", "CN.utf8"], and a region matched
        // whole misses it. Every subtag is cut at its first `.`.
        assert_eq!(of_posix("zh_CN.utf8"), Some(Lang::SimplifiedChinese));
        assert_eq!(of_posix("zh_TW.utf8"), Some(Lang::TraditionalChinese));
        assert_eq!(of_posix("zh-Hant-TW"), Some(Lang::TraditionalChinese));
        assert_eq!(of_posix("zh-Hans-CN"), Some(Lang::SimplifiedChinese));
        // The device ships no Traditional locale, and a bare `zh` is the
        // Simplified one it does ship.
        assert_eq!(of_posix("zh"), Some(Lang::SimplifiedChinese));
    }

    #[test]
    fn the_locale_file_is_read_the_way_the_picker_writes_it() {
        // The pair the picker writes, `LANG` first.
        let written = "LANG=en_US.UTF-8\nLC_ALL=en_US.UTF-8\n";
        assert_eq!(of_locale_file(written), Some(Lang::English));
        assert_eq!(
            of_locale_file("LANG=zh_CN.utf8\nLC_ALL=zh_CN.utf8\n"),
            Some(Lang::SimplifiedChinese)
        );
        // `LC_ALL` alone is not read: the two are written together.
        assert_eq!(of_locale_file("LC_ALL=de_DE.utf8\n"), None);
        // Shell quoting, and a file that names nothing.
        assert_eq!(of_locale_file("LANG=\"de_DE.utf8\"\n"), Some(Lang::German));
        assert_eq!(of_locale_file(""), None);
        assert_eq!(of_locale_file("# nothing here\n"), None);
    }

    #[test]
    fn a_device_with_no_locale_file_reads_as_english() {
        // A missing file is the ordinary case off the device, and on one that
        // never ran the picker.
        assert_eq!(
            Lang::detect_in(Path::new("/nonexistent/locale")),
            Lang::English
        );
    }

    #[test]
    fn the_labels_are_one_character_each() {
        // Five chips on one line is what the abbreviation buys; a spelled-out
        // name wraps the row and drops the last language off it.
        let labels: Vec<&str> = Lang::ALL.iter().map(|l| l.label()).collect();
        assert_eq!(labels, ["EN", "DE", "简", "繁", "日"]);
        // And the letters they are stored as round-trip.
        for lang in Lang::ALL {
            assert_eq!(Lang::from_letter(&lang.letter().to_string()), lang);
        }
        assert_eq!(Lang::from_letter("nonsense"), Lang::English);
    }

    #[test]
    fn every_language_names_the_convention_it_is_set_in() {
        // The tags round-trip through the parser the catalog's own tags go
        // through: a label and a book title choose faces alike.
        assert_eq!(Lang::Japanese.language_tag(), "ja");
        assert_eq!(Lang::TraditionalChinese.language_tag(), "zh-Hant");
        assert_eq!(Lang::SimplifiedChinese.language_tag(), "zh-Hans");
        // Latin names no Han convention.
        assert_eq!(Lang::English.language_tag(), "en");
        assert_eq!(Lang::German.language_tag(), "de");
    }

    #[test]
    fn no_language_leaves_a_word_empty() {
        // The struct makes a missing field a compile error; this catches a
        // field filled in with nothing.
        for lang in Lang::ALL {
            let s = lang.strings();
            let named: [(&str, &str); 10] = [
                ("exit", s.exit),
                ("config", s.config),
                ("today", s.today),
                ("rhythm", s.rhythm),
                ("books", s.books),
                ("language_row", s.language_row),
                ("week_starts_on", s.week_starts_on),
                ("hours", s.hours),
                ("minutes", s.minutes),
                ("read", s.read),
            ];
            for (field, value) in named {
                assert!(!value.is_empty(), "{lang:?} leaves {field} empty");
            }
            assert!(!lang.label().is_empty(), "{lang:?} has no name");
            for (i, m) in s.months.iter().enumerate() {
                assert!(!m.is_empty(), "{lang:?} month {i}");
            }
            for (i, d) in s.weekdays_short.iter().enumerate() {
                assert!(!d.is_empty(), "{lang:?} weekday {i}");
            }
        }
    }

    #[test]
    fn the_nav_labels_fit_the_narrowest_panel() {
        // Five cells of 252 px on a 1264 px panel, 238 usable inside the
        // inverted block. Measured at BODY_PX against the real faces: German
        // `Rhythmus` sets 182 px, `Optionen` 163, `Einstellungen` 235.
        for lang in Lang::ALL {
            let s = lang.strings();
            for label in [s.exit, s.config, s.today, s.rhythm, s.books] {
                // A Latin cell at 0.6 em a character is the pessimistic
                // metric; CJK is one em and shorter in characters.
                let em: f32 = if label.chars().any(|c| c as u32 > 0x2E80) {
                    1.0
                } else {
                    0.6
                };
                let width = label.chars().count() as f32 * em * 38.0;
                assert!(width <= 238.0, "{lang:?} {label:?} sets {width} px");
            }
        }
    }
}
