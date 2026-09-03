//! The interface language: what the device is set to, and every word this app
//! draws in it.
//!
//! [`Strings`] is one struct of `&'static str`, so a language that forgets a
//! word does not compile. Nothing is loaded at runtime — this app draws about
//! a hundred labels, not a document.

use std::path::Path;

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

/// Where the framework writes the locale the reader picked.
///
/// `/etc/upstart/langpicker.conf` names it and writes two shell lines into it:
/// `LANG=en_US.UTF-8` and `LC_ALL=en_US.UTF-8`. The same fact travels over lipc
/// as `com.lab126.locale`, which this app has no use for: a locale cannot
/// change while a KUAL extension holds the screen.
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
    ///
    /// A chip names its option and nothing more — 简 says everything 简体中文
    /// does, in a fifth of the width, and five of these stand on one line
    /// where five spelled-out names do not.
    pub fn label(self) -> &'static str {
        match self {
            Lang::English => "EN",
            Lang::German => "DE",
            Lang::SimplifiedChinese => "简",
            Lang::TraditionalChinese => "繁",
            Lang::Japanese => "日",
        }
    }

    /// What the setting is stored as: one letter, so the file stays readable
    /// and a hand edit is hard to get wrong.
    pub fn letter(self) -> char {
        match self {
            Lang::English => 'e',
            Lang::German => 'd',
            Lang::SimplifiedChinese => 'c',
            Lang::TraditionalChinese => 't',
            Lang::Japanese => 'j',
        }
    }

    /// The language a stored letter names. Anything else is English: a
    /// settings file written by another build must not stop this one drawing.
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
    /// `font::Script::of_language` reads. A tag rather than a `Script` so this
    /// module compiles into the host library beside `date`, which needs its
    /// words and cannot see the drawing code.
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
    /// unreadable, or names a language this app is not written in — the ten
    /// the framework ships include six it has no words for.
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
///
/// The file is shell, so the value may be quoted and `LC_ALL` may sit beside
/// it. Only `LANG` is read: the two are written together and `LANG` is first.
fn of_locale_file(text: &str) -> Option<Lang> {
    let value = text.lines().find_map(|line| {
        line.trim()
            .strip_prefix("LANG=")
            .map(|v| v.trim_matches(['"', '\'']))
    })?;
    of_posix(value)
}

/// The language a POSIX locale names.
///
/// **The codeset rides on the region** — the framework writes `zh_CN.utf8`,
/// not `zh_CN` — so every subtag is cut at its first `.` before it is read.
/// The sixteen values that reach this are every `posix.id.*` in
/// `/opt/amazon/ebook/config/locales/*.properties`.
pub fn of_posix(value: &str) -> Option<Lang> {
    let mut subtags = value
        .split(['-', '_'])
        .map(|s| s.split('.').next().unwrap_or_default().trim());
    let primary = subtags.next()?.to_ascii_lowercase();
    match primary.as_str() {
        "de" => Some(Lang::German),
        "ja" => Some(Lang::Japanese),
        "zh" => {
            // The framework ships `zh-Hans-CN` alone, but a region is read
            // where one is given: nothing else can name Traditional.
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

/// Every word the interface draws.
///
/// Durations keep `h` and `m` in German: `5 Std 8 Min` sets three figures
/// across the Book screen at 1258 px into a row 1186 px wide, and `h`/`min`
/// are read in German anyway. Chinese and Japanese take their own units, which
/// fit.
pub struct Strings {
    // The strip along the bottom.
    pub exit: &'static str,
    pub config: &'static str,
    pub today: &'static str,
    pub calendar: &'static str,
    pub books: &'static str,
    pub clock: &'static str,

    // Today. The three figures, then the heading over the day's books — the
    // day's own date heads its timeline, so it needs no word here.
    pub read_today: &'static str,
    pub pages_turned: &'static str,
    pub current_streak: &'static str,
    pub what_was_read: &'static str,

    // Calendar.
    pub the_month: &'static str,
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
    pub measured_as: &'static str,
    pub on_the_device: &'static str,
    pub yes: &'static str,
    pub no_removed: &'static str,
    pub last_thirty_days: &'static str,
    pub read: &'static str,
    pub left: &'static str,

    /// The three measures, and the two mixed cases. These name what the app
    /// can and cannot know about a number; a loose translation makes it lie.
    pub kindle_timer: &'static str,
    pub timer_and_pages: &'static str,
    pub time_awake: &'static str,
    pub part_bounded: &'static str,
    pub page_by_page: &'static str,

    // Clock.
    pub hour_of_day: &'static str,
    pub weekday: &'static str,
    pub month: &'static str,
    pub shape_of_it: &'static str,
    pub busiest: &'static str,
    pub then: &'static str,
    pub in_the_busiest: &'static str,

    pub counted_over: &'static str,

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
    pub text_size: &'static str,
    pub size_small: &'static str,
    pub size_medium: &'static str,
    pub size_large: &'static str,

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
    calendar: "Calendar",
    books: "Books",
    clock: "Clock",

    read_today: "read today",
    pages_turned: "pages turned",
    current_streak: "current streak",
    what_was_read: "WHAT WAS READ",

    the_month: "THE MONTH",
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
    measured_as: "Measured as",
    on_the_device: "On the device",
    yes: "yes",
    no_removed: "no, removed",
    last_thirty_days: "THE LAST THIRTY DAYS",
    read: "read",
    left: "left",

    kindle_timer: "the Kindle's own timer",
    timer_and_pages: "timer and pages",
    time_awake: "time awake, a bound",
    part_bounded: "part bounded",
    page_by_page: "page by page",

    hour_of_day: "Hour of day",
    weekday: "Weekday",
    month: "Month",
    shape_of_it: "THE SHAPE OF IT",
    busiest: "Busiest",
    then: "Then",
    in_the_busiest: "In the busiest",
    counted_over: "Counted over",

    first_run_1: "First run: every log the device still holds",
    first_run_2: "is read once. This can take a few minutes.",
    catching_up: "Reading what the log has added.",

    interface: "INTERFACE",
    the_calendar: "THE CALENDAR",
    language_row: "Language",
    week_starts_on: "Week starts on",
    text_size: "Text size",
    size_small: "Small",
    size_medium: "Medium",
    size_large: "Large",

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
    calendar: "Kalender",
    books: "Bücher",
    clock: "Uhr",

    read_today: "heute gelesen",
    pages_turned: "Seiten",
    current_streak: "aktuelle Serie",
    what_was_read: "WAS GELESEN WURDE",

    the_month: "DER MONAT",
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
    measured_as: "Gemessen als",
    on_the_device: "Auf dem Gerät",
    yes: "ja",
    no_removed: "nein, entfernt",
    last_thirty_days: "DIE LETZTEN DREISSIG TAGE",
    read: "gelesen",
    left: "übrig",

    kindle_timer: "Kindles eigener Timer",
    timer_and_pages: "Timer und Seiten",
    time_awake: "Wachzeit, eine Schranke",
    part_bounded: "teils geschätzt",
    page_by_page: "Seite für Seite",

    hour_of_day: "Tageszeit",
    weekday: "Wochentag",
    month: "Monat",
    shape_of_it: "DIE FORM",
    busiest: "Am meisten",
    then: "Dann",
    in_the_busiest: "In der aktivsten",
    counted_over: "Gezählt über",

    first_run_1: "Erster Start: jedes Protokoll auf dem Gerät",
    first_run_2: "wird einmal gelesen. Das dauert einige Minuten.",
    catching_up: "Liest, was dazugekommen ist.",

    interface: "OBERFLÄCHE",
    the_calendar: "DER KALENDER",
    language_row: "Sprache",
    week_starts_on: "Woche beginnt am",
    text_size: "Schriftgröße",
    size_small: "Klein",
    size_medium: "Mittel",
    size_large: "Groß",

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
    calendar: "カレンダー",
    books: "本",
    clock: "時計",

    read_today: "今日の読書",
    pages_turned: "めくったページ",
    current_streak: "継続日数",
    what_was_read: "読んだ本",

    the_month: "この月",
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
    measured_as: "計測方法",
    on_the_device: "端末内",
    yes: "あり",
    no_removed: "なし、削除済み",
    last_thirty_days: "この三十日間",
    read: "読了",
    left: "残り",

    kindle_timer: "Kindle 自身のタイマー",
    timer_and_pages: "タイマーとページ",
    time_awake: "起動時間、上限値",
    part_bounded: "一部は上限値",
    page_by_page: "ページ単位",

    hour_of_day: "時刻",
    weekday: "曜日",
    month: "月",
    shape_of_it: "その形",
    busiest: "最多",
    then: "次に",
    in_the_busiest: "最多の時間帯",
    counted_over: "集計対象",

    first_run_1: "初回起動：端末に残るすべての記録を",
    first_run_2: "一度読み込みます。数分かかります。",
    catching_up: "追加された記録を読み込み中。",

    interface: "表示",
    the_calendar: "カレンダー",
    language_row: "言語",
    week_starts_on: "週の始まり",
    text_size: "文字の大きさ",
    size_small: "小",
    size_medium: "中",
    size_large: "大",

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
    calendar: "日历",
    books: "书",
    clock: "时钟",

    read_today: "今日阅读",
    pages_turned: "翻页",
    current_streak: "当前连续",
    what_was_read: "读了什么",

    the_month: "本月",
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
    measured_as: "计量方式",
    on_the_device: "在设备上",
    yes: "是",
    no_removed: "否，已删除",
    last_thirty_days: "最近三十天",
    read: "已读",
    left: "剩余",

    kindle_timer: "Kindle 自带计时",
    timer_and_pages: "计时与翻页",
    time_awake: "唤醒时长，上限",
    part_bounded: "部分为上限",
    page_by_page: "逐页计量",

    hour_of_day: "时段",
    weekday: "星期",
    month: "月份",
    shape_of_it: "分布",
    busiest: "最多",
    then: "其次",
    in_the_busiest: "最多时段占",
    counted_over: "统计范围",

    first_run_1: "首次运行：设备上保留的每份记录",
    first_run_2: "都会读取一次，需要几分钟。",
    catching_up: "正在读取新增的记录。",

    interface: "界面",
    the_calendar: "日历",
    language_row: "语言",
    week_starts_on: "每周开始于",
    text_size: "字号",
    size_small: "小",
    size_medium: "中",
    size_large: "大",

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
    calendar: "日曆",
    books: "書",
    clock: "時鐘",

    read_today: "今日閱讀",
    pages_turned: "翻頁",
    current_streak: "目前連續",
    what_was_read: "讀了什麼",

    the_month: "本月",
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
    measured_as: "計量方式",
    on_the_device: "在裝置上",
    yes: "是",
    no_removed: "否，已刪除",
    last_thirty_days: "最近三十天",
    read: "已讀",
    left: "剩餘",

    kindle_timer: "Kindle 內建計時",
    timer_and_pages: "計時與翻頁",
    time_awake: "喚醒時長，上限",
    part_bounded: "部分為上限",
    page_by_page: "逐頁計量",

    hour_of_day: "時段",
    weekday: "星期",
    month: "月份",
    shape_of_it: "分布",
    busiest: "最多",
    then: "其次",
    in_the_busiest: "最多時段佔",
    counted_over: "統計範圍",

    first_run_1: "首次執行：裝置上保留的每份記錄",
    first_run_2: "都會讀取一次，需要幾分鐘。",
    catching_up: "正在讀取新增的記錄。",

    interface: "介面",
    the_calendar: "日曆",
    language_row: "語言",
    week_starts_on: "每週開始於",
    text_size: "字級",
    size_small: "小",
    size_medium: "中",
    size_large: "大",

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
    use super::*;

    /// Every `posix.id.*` in `/opt/amazon/ebook/config/locales/*.properties`
    /// on a Kindle Scribe — the complete set of values the framework can write
    /// to [`LOCALE_FILE`], and nothing else reaches [`of_posix`].
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
        // whole would miss it. Every subtag is cut at its first `.`.
        assert_eq!(of_posix("zh_CN.utf8"), Some(Lang::SimplifiedChinese));
        assert_eq!(of_posix("zh_TW.utf8"), Some(Lang::TraditionalChinese));
        assert_eq!(of_posix("zh-Hant-TW"), Some(Lang::TraditionalChinese));
        assert_eq!(of_posix("zh-Hans-CN"), Some(Lang::SimplifiedChinese));
        // The framework ships no Traditional locale, so a bare `zh` is the
        // Simplified one it does ship.
        assert_eq!(of_posix("zh"), Some(Lang::SimplifiedChinese));
    }

    #[test]
    fn the_locale_file_is_read_the_way_the_picker_writes_it() {
        // Verbatim from `/etc/upstart/langpicker.conf`:
        //   echo -e "LANG=en_US.UTF-8\nLC_ALL=en_US.UTF-8" > $LOCALE_FILE
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
        // through, so a label and a book title choose faces the same way.
        assert_eq!(Lang::Japanese.language_tag(), "ja");
        assert_eq!(Lang::TraditionalChinese.language_tag(), "zh-Hant");
        assert_eq!(Lang::SimplifiedChinese.language_tag(), "zh-Hans");
        // Latin names no Han convention, and must not promote one.
        assert_eq!(Lang::English.language_tag(), "en");
        assert_eq!(Lang::German.language_tag(), "de");
    }

    #[test]
    fn no_language_leaves_a_word_empty() {
        // The struct makes a missing field a compile error; this catches a
        // field filled in with nothing.
        for lang in Lang::ALL {
            let s = lang.strings();
            let named: [(&str, &str); 11] = [
                ("exit", s.exit),
                ("config", s.config),
                ("today", s.today),
                ("calendar", s.calendar),
                ("books", s.books),
                ("clock", s.clock),
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
        // Six cells of 210 px on a 1264 px panel, 196 usable inside the
        // inverted block. Measured at BODY_PX against the real faces: the
        // worst are Japanese カレンダー at 190 px and German Optionen at 163.
        // `Einstellungen` sets 235 and is why German says Optionen.
        for lang in Lang::ALL {
            let s = lang.strings();
            for label in [s.exit, s.config, s.today, s.calendar, s.books, s.clock] {
                // A Latin cell at 0.6 em a character is the pessimistic
                // metric; CJK is one em and shorter in characters.
                let em: f32 = if label.chars().any(|c| c as u32 > 0x2E80) {
                    1.0
                } else {
                    0.6
                };
                let width = label.chars().count() as f32 * em * 38.0;
                assert!(width <= 196.0, "{lang:?} {label:?} sets {width} px");
            }
        }
    }
}
