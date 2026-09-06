//! [`Lang`] names an interface language. [`Strings`] holds every word drawn
//! in one, as a `&'static str` per field.

use std::path::Path;

/// `template` with `{d}` set to `count`, and text in brackets kept only where
/// `count` is not one: `{d} DAY[S]` gives "1 DAY" and "30 DAYS".
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

/// `template` with `{v}` set to `version`.
pub fn at_version(template: &str, version: &str) -> String {
    template.replace("{v}", version)
}

/// The languages [`Strings`] is written in.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Lang {
    #[default]
    English,
    German,
    Japanese,
    SimplifiedChinese,
    TraditionalChinese,
}

/// The file [`Lang::detect`] reads.
const LOCALE_FILE: &str = "/var/local/system/locale";

impl Lang {
    /// Every [`Lang`], in display order.
    pub const ALL: [Lang; 5] = [
        Lang::English,
        Lang::German,
        Lang::SimplifiedChinese,
        Lang::TraditionalChinese,
        Lang::Japanese,
    ];

    /// One or two characters per [`Lang`], in that language's own script.
    pub fn label(self) -> &'static str {
        match self {
            Lang::English => "EN",
            Lang::German => "DE",
            Lang::SimplifiedChinese => "简",
            Lang::TraditionalChinese => "繁",
            Lang::Japanese => "日",
        }
    }

    /// The letter [`Lang::from_letter`] reads.
    pub fn letter(self) -> char {
        match self {
            Lang::English => 'e',
            Lang::German => 'd',
            Lang::SimplifiedChinese => 'c',
            Lang::TraditionalChinese => 't',
            Lang::Japanese => 'j',
        }
    }

    /// The [`Lang`] a [`Lang::letter`] names. Any other `s` gives
    /// `Lang::English`.
    pub fn from_letter(s: &str) -> Lang {
        match s.trim() {
            "d" => Lang::German,
            "c" => Lang::SimplifiedChinese,
            "t" => Lang::TraditionalChinese,
            "j" => Lang::Japanese,
            _ => Lang::English,
        }
    }

    /// The tag `font::Script::of_language` reads for this [`Lang`].
    pub fn language_tag(self) -> &'static str {
        match self {
            Lang::English => "en",
            Lang::German => "de",
            Lang::Japanese => "ja",
            Lang::SimplifiedChinese => "zh-Hans",
            Lang::TraditionalChinese => "zh-Hant",
        }
    }

    /// The [`Strings`] for this [`Lang`].
    pub fn strings(self) -> &'static Strings {
        match self {
            Lang::English => &ENGLISH,
            Lang::German => &GERMAN,
            Lang::Japanese => &JAPANESE,
            Lang::SimplifiedChinese => &SIMPLIFIED,
            Lang::TraditionalChinese => &TRADITIONAL,
        }
    }

    /// The [`Lang`] `LOCALE_FILE` names. `Lang::English` where that file is
    /// missing, unreadable, or names no [`Lang`].
    pub fn detect() -> Lang {
        Self::detect_in(Path::new(LOCALE_FILE))
    }

    /// [`Lang::detect`] against `path`.
    pub fn detect_in(path: &Path) -> Lang {
        std::fs::read_to_string(path)
            .ok()
            .as_deref()
            .and_then(of_locale_file)
            .unwrap_or(Lang::English)
    }
}

/// The [`Lang`] a `LANG=` line in `text` names, quotes trimmed. `None` where
/// `text` holds no such line.
fn of_locale_file(text: &str) -> Option<Lang> {
    let value = text.lines().find_map(|line| {
        line.trim()
            .strip_prefix("LANG=")
            .map(|v| v.trim_matches(['"', '\'']))
    })?;
    of_posix(value)
}

/// The [`Lang`] `value` names. Every subtag is cut at its first `.`:
/// `zh_CN.utf8` reads as `zh` and `CN`.
pub fn of_posix(value: &str) -> Option<Lang> {
    let mut subtags = value
        .split(['-', '_'])
        .map(|s| s.split('.').next().unwrap_or_default().trim());
    let primary = subtags.next()?.to_ascii_lowercase();
    match primary.as_str() {
        "de" => Some(Lang::German),
        "ja" => Some(Lang::Japanese),
        "zh" => {
            for subtag in subtags {
                match subtag.to_ascii_lowercase().as_str() {
                    "hant" | "tw" | "hk" | "mo" => return Some(Lang::TraditionalChinese),
                    _ => {}
                }
            }
            Some(Lang::SimplifiedChinese)
        }
        "en" => Some(Lang::English),
        _ => None,
    }
}

/// Every word drawn in one [`Lang`].
pub struct Strings {
    // Tab strip.
    pub exit: &'static str,
    pub config: &'static str,
    pub today: &'static str,
    pub rhythm: &'static str,
    pub books: &'static str,
    // Today.
    pub read_today: &'static str,
    pub pages_turned: &'static str,
    pub current_streak: &'static str,
    pub what_was_read: &'static str,

    // Rhythm.
    pub all_time: &'static str,
    pub week: &'static str,
    /// Before a week number's digits, [`Strings::week_no_after`] after them.
    pub week_no: &'static str,
    pub week_no_after: &'static str,
    pub month: &'static str,
    pub year: &'static str,

    // All Time.
    /// `{m}` is the opening month, `{d}` the days run to.
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

    // Books.
    pub shelf_every: &'static str,
    pub shelf_finished: &'static str,
    pub shelf_unfinished: &'static str,
    /// The line for a shelf with no books.
    pub nothing_on_the_shelf: &'static str,
    /// The three list orders.
    pub by_recent: &'static str,
    pub by_time: &'static str,
    pub by_progress: &'static str,
    /// Headings over the [`Strings::trends`] bands.
    pub an_average_day: &'static str,
    pub an_average_week: &'static str,
    pub by_month: &'static str,
    /// The heading over the sitting histogram, and the word closing its count.
    pub sitting_lengths: &'static str,
    pub in_all: &'static str,
    /// The word before the fullest column.
    pub most: &'static str,
    /// The second page of [`Strings::all_time`].
    pub trends: &'static str,
    /// The chip returning a span page to today.
    pub now: &'static str,
    /// The chip opening one day as its own page.
    pub open_day: &'static str,
    pub nothing_read: &'static str,

    /// The word in `{a}–{b} of {c}`.
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
    /// The long form, and the short one drawn beside `restart`.
    pub continue_reading: &'static str,
    pub continue_short: &'static str,
    /// The two marking answers, each with its question and note.
    pub mark_finished: &'static str,
    pub mark_ask: &'static str,
    pub mark_note: &'static str,
    pub mark_unfinished: &'static str,
    pub unmark_ask: &'static str,
    pub unmark_note: &'static str,
    /// The place a book stood at as a span ended. `{d}` is that figure.
    pub percent_reached: &'static str,
    /// The figure on a bar. `{d}` is that figure, rounded.
    pub percent_plain: &'static str,
    /// The restart control, its question, and what that question states.
    pub restart: &'static str,
    pub restart_ask: &'static str,
    pub restart_note: &'static str,
    /// The way out of a question, beside the word the question is asked in.
    pub cancel: &'static str,
    pub yes: &'static str,
    pub no: &'static str,
    /// A book's own reading, start to finish. `{d}` is filled by [`counted`].
    pub the_journey: &'static str,
    pub read: &'static str,
    pub left: &'static str,

    // Splash.
    pub first_run_1: &'static str,
    pub first_run_2: &'static str,
    pub catching_up: &'static str,
    /// The line under a banner counting files: `{d}` files behind it, of `{n}`
    /// to open. `step_logs` counts the device's logs, `step_files` the entries
    /// of an archive.
    pub step_logs: &'static str,
    pub step_files: &'static str,

    // Config.
    /// The section headings, and the name of each setting's row.
    pub interface: &'static str,
    pub the_calendar: &'static str,
    pub language_row: &'static str,
    pub week_starts_on: &'static str,
    /// The record section, and the row counting unnamed books.
    pub the_record: &'static str,
    pub unnamed_row: &'static str,
    pub unnamed_show: &'static str,
    pub unnamed_hide: &'static str,
    /// The row stating what the record holds, above the reset controls.
    pub recorded_row: &'static str,
    /// `{d} sitting[s]` and `{d} book[s]`, which that row and the dialogs
    /// below count with.
    pub n_sittings: &'static str,
    pub n_books: &'static str,
    /// The reset row, and its two chips.
    pub reset_row: &'static str,
    pub reset_keep: &'static str,
    pub reset_none: &'static str,
    /// The restore row, and the chip offering the device's own logs.
    pub restore_row: &'static str,
    pub restore_logs: &'static str,
    /// Emptying the record, keeping an archive first. `{what}` is what goes,
    /// `{file}` the archive's name, `{size}` how large it is.
    pub wipe_ask: &'static str,
    pub wipe_note: &'static str,
    pub wipe_do: &'static str,
    /// Emptying it keeping nothing. `{what}` and `{size}` as above.
    pub nowipe_ask: &'static str,
    pub nowipe_note: &'static str,
    pub nowipe_do: &'static str,
    /// Taking an archive back. `{what}` is what it holds, `{file}` its name.
    pub restore_ask: &'static str,
    pub restore_note: &'static str,
    pub restore_do: &'static str,
    /// Reading the device's whole log again.
    pub rebuild_ask: &'static str,
    pub rebuild_note: &'static str,
    pub rebuild_do: &'static str,
    /// The banner headline over the log pass. `reset_row` and `restore_do`
    /// head the other two.
    pub rebuild_head: &'static str,
    /// The line under that headline while each of the three runs.
    pub wipe_doing: &'static str,
    pub restore_doing: &'static str,
    pub rebuild_doing: &'static str,
    /// The book screen's own control, and the question it puts up. `{what}` is
    /// the reading that goes.
    pub clear: &'static str,
    pub clear_ask: &'static str,
    pub clear_note: &'static str,
    /// Added to that note where the longest streak moves: `{a}` to `{b}` days.
    pub streak_note: &'static str,
    /// Its two answers: the record stays, or it goes as well.
    pub clear_keep: &'static str,
    pub clear_forget: &'static str,
    /// What the Books list says with nothing to list: before any reading, and
    /// after a reset. `{t}` is a duration.
    pub no_reading_yet: &'static str,
    pub nothing_since_reset: &'static str,
    pub unnamed_only: &'static str,
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
    /// The About section: the version, and the button checking for a newer one.
    pub about: &'static str,
    pub version_row: &'static str,
    pub update_row: &'static str,
    pub update_check: &'static str,
    /// The banner, while an update runs.
    pub update_asking: &'static str,
    pub update_downloading: &'static str,
    pub update_checking: &'static str,
    pub update_placing: &'static str,
    pub update_tap_to_stop: &'static str,
    /// How an update ended. `{v}` is a version, filled by [`at_version`].
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

    /// Hours and minutes, appended across [`Strings::unit_space`].
    pub hours: &'static str,
    pub minutes: &'static str,
    pub unit_space: bool,

    /// Whether a date runs year first: `2026年9月3日`.
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
    by_time: "Time",
    by_progress: "Progress",
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
    step_logs: "log {d} of {n}",
    step_files: "file {d} of {n}",

    the_record: "THE RECORD",
    unnamed_row: "Unidentified books",
    unnamed_show: "Show",
    unnamed_hide: "Hide",
    recorded_row: "Recorded",
    n_sittings: "{d} sitting[s]",
    n_books: "{d} book[s]",
    reset_row: "Reset",
    reset_keep: "Back up first",
    reset_none: "No backup",
    restore_row: "Backups",
    restore_logs: "From the logs",
    wipe_ask: "Back up, then reset?",
    wipe_note: "{what}, with every cover held, are copied into {file}, {size}. \
                The record then starts empty from today. To bring it back, tap \
                it under Backups.",
    wipe_do: "Back up and reset",
    nowipe_ask: "Reset without a backup?",
    nowipe_note: "{what} and every cover held are deleted, and no copy is kept. \
                  It frees {size}. Your Kindle's own logs are not touched, and \
                  about a month of them can be read again. Reading older than \
                  that, and books no longer in your library, cannot come back.",
    nowipe_do: "Reset, no backup",
    restore_ask: "Bring this back?",
    restore_note: "{file} holds {what}. Whatever the record already has is \
                   left as it stands, so nothing is counted twice. The backup \
                   itself goes once all of it is back in the record.",
    restore_do: "Restore",
    rebuild_ask: "Read the Kindle's logs again?",
    rebuild_note: "Every log the device still holds is read from the start, \
                   which takes a few minutes. Nothing you have now is lost. \
                   Reading older than the logs, and books taken off the record \
                   one at a time, do not come back.",
    rebuild_do: "Read them",
    rebuild_head: "Restore from the logs",
    wipe_doing: "Resetting the record.",
    restore_doing: "Bringing the backup back.",
    rebuild_doing: "Reading every log the device still holds.",
    clear: "Clear",
    clear_ask: "Clear this book's reading?",
    clear_note: "{what} go, and the book goes back to 0%. Keeping it leaves it \
                 on your lists with nothing against it; forgetting it takes \
                 the title, the author and the cover as well. A copy is saved \
                 first, under Backups.",
    streak_note: "Your longest streak goes from {a} days to {b}.",
    clear_keep: "Keep the book",
    clear_forget: "Forget it too",
    no_reading_yet: "No reading yet. Open a book, read a few pages, then come \
                     back — the log starts from the day this first runs.",
    nothing_since_reset: "Nothing read since the record was reset. Open a book \
                          and it starts again from here.",
    unnamed_only: "{t} read, on books the catalog names none of. A book is \
                   listed once the device has said what it is.",
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
    shelf_unfinished: "Offen",
    nothing_on_the_shelf: "Keine Bücher hier.",
    by_recent: "Zuletzt",
    by_time: "Zeit",
    by_progress: "Fortschritt",
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
    mark_finished: "Als fertig markieren",
    mark_ask: "Dieses Buch als fertig markieren?",
    mark_note: "Es kommt in das Regal Fertig, und die Bibliothek markiert es als gelesen. \
                Fortschritt, gelesene Zeit, Sitzungen und Tage bleiben unverändert.",
    mark_unfinished: "Als offen markieren",
    unmark_ask: "Markierung Fertig entfernen?",
    unmark_note: "Es verlässt das Regal Fertig, und die Bibliothek markiert es als \
                  ungelesen. Fortschritt, gelesene Zeit, Sitzungen und Tage bleiben \
                  unverändert.",
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
    step_logs: "Protokoll {d} von {n}",
    step_files: "Datei {d} von {n}",

    the_record: "DIE AUFZEICHNUNG",
    recorded_row: "Aufgezeichnet",
    n_sittings: "{d} Sitzung[en]",
    n_books: "{d} Titel",
    reset_row: "Zurücksetzen",
    reset_keep: "Erst sichern",
    reset_none: "Ohne Sicherung",
    restore_row: "Sicherungen",
    restore_logs: "Aus den Protokollen",
    wipe_ask: "Erst sichern, dann zurücksetzen?",
    wipe_note: "{what} werden mit allen Titelbildern nach {file} kopiert, \
                {size}. Die Aufzeichnung beginnt dann leer ab heute. Zum \
                Zurückholen unter Sicherungen antippen.",
    wipe_do: "Zurücksetzen",
    nowipe_ask: "Ohne Sicherung zurücksetzen?",
    nowipe_note: "{what} und alle Titelbilder werden ohne Kopie gelöscht, das \
                  gibt {size} frei. Die Protokolle des Kindle bleiben \
                  unberührt; etwa ein Monat lässt sich erneut lesen. Ältere \
                  Lesezeit und Bücher außerhalb der Bibliothek nicht.",
    nowipe_do: "Ohne Sicherung",
    restore_ask: "Das zurückholen?",
    restore_note: "{file} enthält {what}. Was die Aufzeichnung schon hat, \
                   bleibt unverändert, also wird nichts doppelt gezählt. Die \
                   Sicherung selbst wird gelöscht, sobald alles wieder in der \
                   Aufzeichnung steht.",
    restore_do: "Zurückholen",
    rebuild_ask: "Die Protokolle erneut lesen?",
    rebuild_note: "Jedes Protokoll auf dem Gerät wird von vorn gelesen, was \
                   einige Minuten dauert. Nichts Vorhandenes geht verloren. \
                   Ältere Lesezeit und einzeln entfernte Bücher kommen nicht \
                   zurück.",
    rebuild_do: "Erneut lesen",
    rebuild_head: "Aus den Protokollen zurückholen",
    wipe_doing: "Setzt die Aufzeichnung zurück.",
    restore_doing: "Holt die Sicherung zurück.",
    rebuild_doing: "Liest jedes Protokoll auf dem Gerät.",
    clear: "Löschen",
    clear_ask: "Die Zeiten dieses Buches löschen?",
    clear_note: "{what} werden gelöscht, und das Buch steht wieder bei 0 %. \
                 Bleibt es, steht es ohne Werte auf den Listen; wird es \
                 vergessen, gehen auch Titel, Autor und Bild. Eine Kopie wird \
                 vorher gesichert.",
    streak_note: "Längste Serie: von {a} auf {b} Tage.",
    clear_keep: "Buch behalten",
    clear_forget: "Auch vergessen",
    no_reading_yet: "Noch nichts gelesen. Öffne ein Buch, lies ein paar Seiten \
                     und komm zurück — das Protokoll beginnt am Tag des ersten \
                     Starts.",
    nothing_since_reset: "Seit dem Zurücksetzen nichts gelesen. Öffne ein Buch, \
                          dann beginnt es hier von vorn.",
    unnamed_only: "{t} gelesen, auf Büchern, die der Katalog nicht benennt. Ein \
                   Buch wird gelistet, sobald das Gerät sagt, welches es ist.",
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
    by_time: "時間",
    by_progress: "進捗",
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
    step_logs: "ログ {d}/{n}",
    step_files: "ファイル {d}/{n}",

    the_record: "記録",
    recorded_row: "記録の中身",
    n_sittings: "{d}回",
    n_books: "{d}冊",
    reset_row: "リセット",
    reset_keep: "先に保存",
    reset_none: "保存しない",
    restore_row: "バックアップ",
    restore_logs: "ログから",
    wipe_ask: "保存してからリセットしますか？",
    wipe_note: "{what}と表紙をすべて {file} に保存します（{size}）。記録は今日から空で始まります。\
                戻すときはバックアップから選んでください。",
    wipe_do: "保存してリセット",
    nowipe_ask: "保存せずにリセットしますか？",
    nowipe_note: "{what}と表紙をすべて削除し、控えは残りません。{size}の空きができます。\
                  Kindle自身のログには触れないので、約1か月分は読み直せます。\
                  それより古い読書と、ライブラリにない本は戻りません。",
    nowipe_do: "保存せずリセット",
    restore_ask: "これを戻しますか？",
    restore_note: "{file} には{what}が入っています。すでにある記録はそのままなので、二重には数えません。\
                   すべてが記録に戻ると、このバックアップ自体は削除されます。",
    restore_do: "戻す",
    rebuild_ask: "Kindleのログを読み直しますか？",
    rebuild_note: "端末に残っているログを最初から読み直します。数分かかります。\
                   今ある記録が失われることはありません。ログより古い読書と、\
                   1冊ずつ記録から外した本は戻りません。",
    rebuild_do: "読み直す",
    rebuild_head: "ログから戻す",
    wipe_doing: "記録をリセットしています。",
    restore_doing: "バックアップを記録に戻しています。",
    rebuild_doing: "端末に残っているログを読み直しています。",
    clear: "消去",
    clear_ask: "この本の読書記録を消去しますか？",
    clear_note: "{what}を削除し、進捗は0%に戻ります。本を残せば数値のない状態で一覧に残り、\
                 一緒に消せば題名・著者・表紙も消えます。控えは先に保存されます。",
    streak_note: "最長連続日数は{a}日から{b}日になります。",
    clear_keep: "本は残す",
    clear_forget: "本も消す",
    no_reading_yet: "まだ読書がありません。本を開いて数ページ読んでから戻ってきてください。\
                     記録は初回起動の日から始まります。",
    nothing_since_reset: "リセット後の読書はまだありません。本を開けば、ここから始まります。",
    unnamed_only: "{t}の読書がありますが、カタログが本を特定していません。\
                   端末が本を認識すると一覧に並びます。",
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
    by_time: "时长",
    by_progress: "进度",
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
    step_logs: "日志 {d}/{n}",
    step_files: "文件 {d}/{n}",

    the_record: "记录",
    recorded_row: "已记录",
    n_sittings: "{d}次",
    n_books: "{d}本",
    reset_row: "重置",
    reset_keep: "先备份",
    reset_none: "不备份",
    restore_row: "备份",
    restore_logs: "从日志读取",
    wipe_ask: "先备份再重置？",
    wipe_note: "{what}和所有封面会存入 {file}（{size}）。记录随后从今天起为空。\
                要取回，请在备份中点选。",
    wipe_do: "备份并重置",
    nowipe_ask: "不备份就重置？",
    nowipe_note: "{what}和所有封面都会删除，不留副本，可腾出 {size}。\
                  Kindle 自己的日志不会被动到，其中约一个月可以重新读取。\
                  更早的阅读，以及已不在书库中的书，无法恢复。",
    nowipe_do: "不备份重置",
    restore_ask: "取回这一份？",
    restore_note: "{file} 中有{what}。记录里已有的内容保持不变，因此不会重复计入。\
                   全部回到记录后，这份备份本身会被删除。",
    restore_do: "取回",
    rebuild_ask: "重新读取 Kindle 日志？",
    rebuild_note: "设备上仍保留的日志会从头读一遍，需要几分钟。现有记录不会丢失。\
                   比日志更早的阅读，以及逐本从记录中移除的书，不会回来。",
    rebuild_do: "重新读取",
    rebuild_head: "从日志取回",
    wipe_doing: "正在重置记录。",
    restore_doing: "正在取回备份。",
    rebuild_doing: "正在重新读取设备上的日志。",
    clear: "清除",
    clear_ask: "清除这本书的阅读记录？",
    clear_note: "将删除{what}，进度回到 0%。保留书籍时，它仍在列表中，只是没有任何数值；\
                 一并移除时，书名、作者和封面也会消失。系统会先保存一份副本。",
    streak_note: "最长连续天数将从 {a} 天变为 {b} 天。",
    clear_keep: "保留书籍",
    clear_forget: "一并移除",
    no_reading_yet: "还没有阅读记录。打开一本书读几页再回来——日志从首次运行当天开始。",
    nothing_since_reset: "重置后还没有阅读。打开一本书，就从这里重新开始。",
    unnamed_only: "已读 {t}，但目录未能指明是哪些书。设备识别出书名后就会列出。",
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
    by_time: "時長",
    by_progress: "進度",
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
    step_logs: "日誌 {d}/{n}",
    step_files: "檔案 {d}/{n}",

    the_record: "記錄",
    recorded_row: "已記錄",
    n_sittings: "{d}次",
    n_books: "{d}本",
    reset_row: "重設",
    reset_keep: "先備份",
    reset_none: "不備份",
    restore_row: "備份",
    restore_logs: "從日誌讀取",
    wipe_ask: "先備份再重設？",
    wipe_note: "{what}和所有封面會存入 {file}（{size}）。記錄隨後從今天起為空。\
                要取回，請在備份中點選。",
    wipe_do: "備份並重設",
    nowipe_ask: "不備份就重設？",
    nowipe_note: "{what}和所有封面都會刪除，不留副本，可騰出 {size}。\
                  Kindle 自己的日誌不會被動到，其中約一個月可以重新讀取。\
                  更早的閱讀，以及已不在書庫中的書，無法復原。",
    nowipe_do: "不備份重設",
    restore_ask: "取回這一份？",
    restore_note: "{file} 中有{what}。記錄裡已有的內容保持不變，因此不會重複計入。\
                   全部回到記錄後，這份備份本身會被刪除。",
    restore_do: "取回",
    rebuild_ask: "重新讀取 Kindle 日誌？",
    rebuild_note: "裝置上仍保留的日誌會從頭讀一遍，需要幾分鐘。現有記錄不會遺失。\
                   比日誌更早的閱讀，以及逐本從記錄中移除的書，不會回來。",
    rebuild_do: "重新讀取",
    rebuild_head: "從日誌取回",
    wipe_doing: "正在重設記錄。",
    restore_doing: "正在取回備份。",
    rebuild_doing: "正在重新讀取裝置上的日誌。",
    clear: "清除",
    clear_ask: "清除這本書的閱讀記錄？",
    clear_note: "將刪除{what}，進度回到 0%。保留書籍時，它仍在列表中，只是沒有任何數值；\
                 一併移除時，書名、作者和封面也會消失。系統會先儲存一份副本。",
    streak_note: "最長連續天數將從 {a} 天變為 {b} 天。",
    clear_keep: "保留書籍",
    clear_forget: "一併移除",
    no_reading_yet: "還沒有閱讀記錄。打開一本書讀幾頁再回來——日誌從首次執行當天開始。",
    nothing_since_reset: "重設後還沒有閱讀。打開一本書，就從這裡重新開始。",
    unnamed_only: "已讀 {t}，但目錄未能指明是哪些書。裝置辨識出書名後就會列出。",
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
        // A `template` with no brackets.
        assert_eq!(counted("{d}\u{65e5}", 1), "1\u{65e5}");
        // `since_days` and `the_journey`, for one and for many.
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

    /// Every value that can reach [`LOCALE_FILE`].
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
        // `zh_CN.utf8` splits to ["zh", "CN.utf8"]; every subtag is cut at
        // its first `.`.
        assert_eq!(of_posix("zh_CN.utf8"), Some(Lang::SimplifiedChinese));
        assert_eq!(of_posix("zh_TW.utf8"), Some(Lang::TraditionalChinese));
        assert_eq!(of_posix("zh-Hant-TW"), Some(Lang::TraditionalChinese));
        assert_eq!(of_posix("zh-Hans-CN"), Some(Lang::SimplifiedChinese));
        // A bare `zh` gives `Lang::SimplifiedChinese`.
        assert_eq!(of_posix("zh"), Some(Lang::SimplifiedChinese));
    }

    #[test]
    fn the_locale_file_is_read_the_way_the_picker_writes_it() {
        // `LANG` first.
        let written = "LANG=en_US.UTF-8\nLC_ALL=en_US.UTF-8\n";
        assert_eq!(of_locale_file(written), Some(Lang::English));
        assert_eq!(
            of_locale_file("LANG=zh_CN.utf8\nLC_ALL=zh_CN.utf8\n"),
            Some(Lang::SimplifiedChinese)
        );
        // `LC_ALL` alone gives `None`.
        assert_eq!(of_locale_file("LC_ALL=de_DE.utf8\n"), None);
        // Shell quoting, and a file that names nothing.
        assert_eq!(of_locale_file("LANG=\"de_DE.utf8\"\n"), Some(Lang::German));
        assert_eq!(of_locale_file(""), None);
        assert_eq!(of_locale_file("# nothing here\n"), None);
    }

    #[test]
    fn a_device_with_no_locale_file_reads_as_english() {
        assert_eq!(
            Lang::detect_in(Path::new("/nonexistent/locale")),
            Lang::English
        );
    }

    #[test]
    fn the_labels_are_one_character_each() {
        let labels: Vec<&str> = Lang::ALL.iter().map(|l| l.label()).collect();
        assert_eq!(labels, ["EN", "DE", "简", "繁", "日"]);
        // `Lang::letter` round-trips through `Lang::from_letter`.
        for lang in Lang::ALL {
            assert_eq!(Lang::from_letter(&lang.letter().to_string()), lang);
        }
        assert_eq!(Lang::from_letter("nonsense"), Lang::English);
    }

    #[test]
    fn every_language_names_the_convention_it_is_set_in() {
        // `language_tag` gives the tag `font::Script::of_language` reads.
        assert_eq!(Lang::Japanese.language_tag(), "ja");
        assert_eq!(Lang::TraditionalChinese.language_tag(), "zh-Hant");
        assert_eq!(Lang::SimplifiedChinese.language_tag(), "zh-Hans");
        // `en` and `de` name no Han convention.
        assert_eq!(Lang::English.language_tag(), "en");
        assert_eq!(Lang::German.language_tag(), "de");
    }

    #[test]
    fn no_language_leaves_a_word_empty() {
        // A field filled in with nothing.
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

    /// A line's width, the way `the_nav_labels_fit_the_narrowest_panel`
    /// estimates one: Latin at 0.6 em a character, CJK at a full em.
    fn em_width(said: &str, px: f32) -> u32 {
        let wide = said.chars().filter(|c| *c as u32 > 0x2E80).count() as f32;
        let thin = said.chars().count() as f32 - wide;
        ((wide + thin * 0.6) * px) as u32
    }

    /// How many lines `said` wraps to at `px` inside `width`.
    fn lines_of(said: &str, width: u32, px: f32) -> usize {
        crate::wrap::wrap_to_width(said, width, |t| em_width(t, px)).len()
    }

    #[test]
    fn every_dialog_states_its_whole_case_in_every_language() {
        // The narrowest panel at the largest text, which is where a clamp
        // bites first. `ui::dialog` boxes the note in `area.w - gap * 6`, less
        // `gap * 3` of padding on each side.
        let theme = crate::ui::theme::Theme::sized(1264, 1680, crate::settings::TextSize::Large);
        let inner = crate::ui::chrome::content_box(&theme).w - theme.gap * 12;

        for lang in Lang::ALL {
            let s = lang.strings();
            // Every field at its widest, so the budget holds for any record.
            let filled = |note: &str| {
                note.replace(
                    "{what}",
                    &format!(
                        "{} · {}",
                        counted(s.n_sittings, 9_999),
                        counted(s.n_books, 999)
                    ),
                )
                .replace("{file}", "readinglog-991231-235959.zip")
                .replace("{size}", "999.9 MB")
                .replace("{a}", "999")
                .replace("{b}", "999")
            };
            for (name, heading, note) in [
                ("wipe", s.wipe_ask, s.wipe_note),
                ("nowipe", s.nowipe_ask, s.nowipe_note),
                ("restore", s.restore_ask, s.restore_note),
                ("rebuild", s.rebuild_ask, s.rebuild_note),
                ("restart", s.restart_ask, s.restart_note),
                ("mark", s.mark_ask, s.mark_note),
                ("unmark", s.unmark_ask, s.unmark_note),
                (
                    "clear",
                    s.clear_ask,
                    // The note and the streak sentence stand together.
                    &format!("{} {}", s.clear_note, s.streak_note),
                ),
            ] {
                let head = lines_of(heading, inner as u32, theme.head_px);
                assert!(head <= 2, "{lang:?} {name} headline takes {head} lines");
                let body = lines_of(&filled(note), inner as u32, theme.body_px);
                assert!(body <= 8, "{lang:?} {name} note takes {body} lines");

                // And the box those lines make stands inside the screen: a
                // panel taller than the page is clamped, and the answers along
                // its foot are what falls off.
                let line = |px: f32| (px * 1.4) as i32;
                let high = theme.gap * 6
                    + head as i32 * line(theme.head_px)
                    + theme.gap * 2
                    + body as i32 * line(theme.body_px)
                    + theme.gap * 3
                    + theme.row_h * 2 / 3;
                let room = crate::ui::chrome::content_box(&theme).h;
                assert!(high <= room, "{lang:?} {name} boxes {high} px of {room}");
            }
        }
    }

    #[test]
    fn every_dialog_answer_fits_beside_its_fellows() {
        // Three answers abreast on the narrow panel at Large, gaps included.
        let theme = crate::ui::theme::Theme::sized(1264, 1680, crate::settings::TextSize::Large);
        let inner = crate::ui::chrome::content_box(&theme).w - theme.gap * 12;
        for lang in Lang::ALL {
            let s = lang.strings();
            for answer in [
                s.cancel,
                s.wipe_do,
                s.nowipe_do,
                s.restore_do,
                s.rebuild_do,
                s.clear_keep,
                s.clear_forget,
                s.clear,
            ] {
                let width = em_width(answer, theme.body_px) as i32;
                assert!(
                    width <= inner / 2,
                    "{lang:?} {answer:?} sets {width} px of {inner}"
                );
            }
        }
    }

    #[test]
    fn the_nav_labels_fit_the_narrowest_panel() {
        // Five cells of 252 px on a 1264 px panel, 238 usable inside the
        // inverted block.
        for lang in Lang::ALL {
            let s = lang.strings();
            for label in [s.exit, s.config, s.today, s.rhythm, s.books] {
                // Latin at 0.6 em a character, CJK at one.
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
