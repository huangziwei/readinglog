//! `Settings`: the fields `view::config` sets, and the `key=value` file
//! `Settings::load` and `Settings::save` carry them in.

use std::path::{Path, PathBuf};

use crate::lang::Lang;

/// The paths `Settings::load` reads and `Settings::save` writes, in order.
const SETTINGS_PATHS: &[&str] = &[
    "/mnt/us/extensions/readinglog/settings",
    "/var/local/readinglog/settings",
];

/// The size `Theme::sized` builds a screen at.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum TextSize {
    Small,
    #[default]
    Medium,
    Large,
}

impl TextSize {
    pub const ALL: [TextSize; 3] = [TextSize::Small, TextSize::Medium, TextSize::Large];

    /// The factor `Theme::sized` multiplies `BODY_PX` by. `Theme::tab_px`
    /// takes `BODY_PX` unscaled.
    pub fn scale(self) -> f32 {
        match self {
            TextSize::Small => 0.85,
            TextSize::Medium => 1.0,
            TextSize::Large => 1.2,
        }
    }

    fn token(self) -> &'static str {
        match self {
            TextSize::Small => "small",
            TextSize::Medium => "medium",
            TextSize::Large => "large",
        }
    }

    fn of_token(token: &str) -> Option<Self> {
        match token {
            "small" => Some(TextSize::Small),
            "medium" => Some(TextSize::Medium),
            "large" => Some(TextSize::Large),
            _ => None,
        }
    }
}

/// The colours `ui::charts` draws in, one bar hue apiece: 鳶 23°, 若竹 97°,
/// 紺 222°. `ui::paint::Palette::for_panel` reads it under `eink::fb::has_cfa`.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum ColorScheme {
    /// 鳶 across the ramp, marked in 黄金.
    TobiKogane,
    /// 若竹 and 松葉 across the ramp, marked at 桜's hue.
    SakuraWakatake,
    /// 紺's hue across the ramp, marked at 紅's.
    #[default]
    KurenaiKon,
    /// `ui::paint::Palette::GREY`.
    Grey,
}

impl ColorScheme {
    pub const ALL: [ColorScheme; 4] = [
        ColorScheme::TobiKogane,
        ColorScheme::SakuraWakatake,
        ColorScheme::KurenaiKon,
        ColorScheme::Grey,
    ];

    fn token(self) -> &'static str {
        match self {
            ColorScheme::TobiKogane => "tobi",
            ColorScheme::SakuraWakatake => "wakatake",
            ColorScheme::KurenaiKon => "kon",
            ColorScheme::Grey => "grey",
        }
    }

    fn of_token(token: &str) -> Option<Self> {
        ColorScheme::ALL
            .into_iter()
            .find(|scheme| scheme.token() == token)
    }
}

/// The figures `view::book` states.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Figures {
    /// `timer.model`'s counters, `TotalWPM` and `NewTimeLeft`.
    #[default]
    Device,
    /// The sittings `BookStat` totals.
    App,
}

impl Figures {
    pub const ALL: [Figures; 2] = [Figures::Device, Figures::App];

    fn token(self) -> &'static str {
        match self {
            Figures::Device => "device",
            Figures::App => "app",
        }
    }

    fn of_token(token: &str) -> Option<Self> {
        Figures::ALL.into_iter().find(|f| f.token() == token)
    }
}

/// The shortest run `stats::Stats::build` counts as reading. A run under it
/// carries no `Stats::sittings` entry and no total; its book keeps a row in
/// `Stats::books`, at zero.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum SittingFloor {
    /// Every run, however short.
    All,
    #[default]
    OneMinute,
    FiveMinutes,
    FifteenMinutes,
}

impl SittingFloor {
    pub const ALL: [SittingFloor; 4] = [
        SittingFloor::All,
        SittingFloor::OneMinute,
        SittingFloor::FiveMinutes,
        SittingFloor::FifteenMinutes,
    ];

    /// The minutes a run must reach.
    pub fn minutes(self) -> i64 {
        match self {
            SittingFloor::All => 0,
            SittingFloor::OneMinute => 1,
            SittingFloor::FiveMinutes => 5,
            SittingFloor::FifteenMinutes => 15,
        }
    }

    /// [`Self::minutes`] in seconds.
    pub fn seconds(self) -> i64 {
        self.minutes() * 60
    }

    /// [`Self::minutes`] and `Strings::minutes`, spaced by
    /// `Strings::unit_space`.
    pub fn label(self, s: &crate::lang::Strings) -> String {
        let space = match s.unit_space {
            true => " ",
            false => "",
        };
        format!("{}{space}{}", self.minutes(), s.minutes)
    }

    fn token(self) -> &'static str {
        match self {
            SittingFloor::All => "all",
            SittingFloor::OneMinute => "1m",
            SittingFloor::FiveMinutes => "5m",
            SittingFloor::FifteenMinutes => "15m",
        }
    }

    fn of_token(token: &str) -> Option<Self> {
        SittingFloor::ALL.into_iter().find(|f| f.token() == token)
    }
}

/// The day `WeekStart::column_of` puts in column 0.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum WeekStart {
    #[default]
    Monday,
    Sunday,
}

impl WeekStart {
    pub const ALL: [WeekStart; 2] = [WeekStart::Monday, WeekStart::Sunday];

    /// The rotation `WeekStart::column_of` and `WeekStart::day_in` apply.
    /// `date::weekday` counts from Monday.
    pub fn shift(self) -> usize {
        match self {
            WeekStart::Monday => 0,
            WeekStart::Sunday => 1,
        }
    }

    /// The column `monday_first` sits in.
    pub fn column_of(self, monday_first: usize) -> usize {
        (monday_first + self.shift()) % 7
    }

    /// The Monday-first weekday drawn in `column`.
    pub fn day_in(self, column: usize) -> usize {
        (column + 7 - self.shift()) % 7
    }

    fn token(self) -> &'static str {
        match self {
            WeekStart::Monday => "monday",
            WeekStart::Sunday => "sunday",
        }
    }

    fn of_token(token: &str) -> Option<Self> {
        match token {
            "monday" => Some(WeekStart::Monday),
            "sunday" => Some(WeekStart::Sunday),
            _ => None,
        }
    }
}

/// Which list the search names, which its head row picks between.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Scope {
    /// Titles and authors, through `view::search::listed`.
    #[default]
    Books,
    /// Passages and the notes written on them, through
    /// `view::search::listed_marks`.
    Marks,
}

impl Scope {
    pub const ALL: [Scope; 2] = [Scope::Books, Scope::Marks];

    fn token(self) -> &'static str {
        match self {
            Scope::Books => "books",
            Scope::Marks => "marks",
        }
    }

    fn of_token(token: &str) -> Option<Self> {
        Scope::ALL.into_iter().find(|s| s.token() == token)
    }
}

/// The fields `view::config` sets.
#[derive(Clone, Debug, PartialEq)]
pub struct Settings {
    pub language: Lang,
    pub week_start: WeekStart,
    pub text_size: TextSize,
    /// The colours `ui::charts` draws in.
    pub color_scheme: ColorScheme,
    /// The figures `view::book` states.
    pub figures: Figures,
    /// The shortest run `Stats::build` counts as reading.
    pub sitting_floor: SittingFloor,
    /// Whether `Stats` totals hold sittings no `BookRecord` names.
    pub show_unnamed: bool,
    /// Whether `view::books::listed` keeps a book failing `BookStat::has_cover`.
    pub show_uncovered: bool,
    /// Which list the search opens on, which is the one it last showed.
    pub scope: Scope,
    /// The lines `Settings::parse` matched no key for, held for `to_text`.
    unknown: Vec<String>,
}

impl Settings {
    /// `detected` for `language`, `Default` for every other field.
    pub fn new(detected: Lang) -> Self {
        Self {
            language: detected,
            week_start: WeekStart::default(),
            text_size: TextSize::default(),
            color_scheme: ColorScheme::default(),
            figures: Figures::default(),
            sitting_floor: SittingFloor::default(),
            show_unnamed: true,
            show_uncovered: true,
            scope: Scope::default(),
            unknown: Vec::new(),
        }
    }

    /// The first `SETTINGS_PATHS` entry that is a file, over
    /// [`Settings::new`]. A path naming no file yields [`Settings::new`].
    pub fn load(detected: Lang) -> Self {
        match SETTINGS_PATHS.iter().map(Path::new).find(|p| p.is_file()) {
            Some(path) => Self::load_from(path, detected),
            None => Self::new(detected),
        }
    }

    /// [`Settings::load`] against a named file.
    pub fn load_from(path: &Path, detected: Lang) -> Self {
        let Ok(text) = std::fs::read_to_string(path) else {
            return Self::new(detected);
        };
        Self::parse(&text, detected)
    }

    /// `key=value` a line, `#` a comment. A value `of_token` reads as `None`
    /// keeps the [`Settings::new`] default for that one field.
    pub fn parse(text: &str, detected: Lang) -> Self {
        let mut out = Self::new(detected);
        for line in text.lines() {
            let line = line.trim();
            if line.is_empty() || line.starts_with('#') {
                continue;
            }
            let Some((key, value)) = line.split_once('=') else {
                out.unknown.push(line.to_string());
                continue;
            };
            let value = value.trim();
            match key.trim() {
                "language" => out.language = Lang::from_letter(value),
                "week_start" => {
                    if let Some(week) = WeekStart::of_token(value) {
                        out.week_start = week;
                    }
                }
                "text_size" => {
                    if let Some(size) = TextSize::of_token(value) {
                        out.text_size = size;
                    }
                }
                "color_scheme" => {
                    if let Some(scheme) = ColorScheme::of_token(value) {
                        out.color_scheme = scheme;
                    }
                }
                "figures" => {
                    if let Some(from) = Figures::of_token(value) {
                        out.figures = from;
                    }
                }
                "sitting_floor" => {
                    if let Some(floor) = SittingFloor::of_token(value) {
                        out.sitting_floor = floor;
                    }
                }
                "show_unnamed" => out.show_unnamed = value != "no",
                "show_uncovered" => out.show_uncovered = value != "no",
                "scope" => {
                    if let Some(scope) = Scope::of_token(value) {
                        out.scope = scope;
                    }
                }
                _ => out.unknown.push(line.to_string()),
            }
        }
        out
    }

    /// The file's whole text.
    pub fn to_text(&self) -> String {
        let mut out = String::from("# Reading Log settings. Written by the config page.\n");
        out.push_str(&format!("language={}\n", self.language.letter()));
        out.push_str(&format!("week_start={}\n", self.week_start.token()));
        out.push_str(&format!("text_size={}\n", self.text_size.token()));
        out.push_str(&format!("color_scheme={}\n", self.color_scheme.token()));
        out.push_str(&format!("figures={}\n", self.figures.token()));
        out.push_str(&format!("sitting_floor={}\n", self.sitting_floor.token()));
        let yes_no = |set: bool| match set {
            true => "yes",
            false => "no",
        };
        out.push_str(&format!("show_unnamed={}\n", yes_no(self.show_unnamed)));
        out.push_str(&format!("show_uncovered={}\n", yes_no(self.show_uncovered)));
        out.push_str(&format!("scope={}\n", self.scope.token()));
        for line in &self.unknown {
            out.push_str(line);
            out.push('\n');
        }
        out
    }

    /// Writes [`Settings::to_text`] to the first `SETTINGS_PATHS` entry whose
    /// parent is a directory. A failed write prints to stderr.
    pub fn save(&self) {
        for path in SETTINGS_PATHS.iter().map(PathBuf::from) {
            let Some(dir) = path.parent() else { continue };
            if !dir.is_dir() {
                continue;
            }
            match std::fs::write(&path, self.to_text()) {
                Ok(()) => return,
                Err(err) => eprintln!("!! settings: {} not written: {err}", path.display()),
            }
        }
        eprintln!("?? settings: nowhere to write; this run keeps them");
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_reader_who_never_opened_the_page_gets_the_device_s_language() {
        let s = Settings::new(Lang::Japanese);
        assert_eq!(s.language, Lang::Japanese);
        assert_eq!(s.week_start, WeekStart::Monday);
        // A path naming no file.
        let missing = Settings::load_from(Path::new("/nonexistent/settings"), Lang::German);
        assert_eq!(missing.language, Lang::German);
    }

    #[test]
    fn a_written_file_round_trips() {
        let mut s = Settings::new(Lang::English);
        s.language = Lang::TraditionalChinese;
        s.week_start = WeekStart::Sunday;
        s.text_size = TextSize::Large;
        s.color_scheme = ColorScheme::TobiKogane;
        s.figures = Figures::App;
        s.show_unnamed = false;
        s.show_uncovered = false;
        let back = Settings::parse(&s.to_text(), Lang::English);
        assert_eq!(back.language, Lang::TraditionalChinese);
        assert_eq!(back.week_start, WeekStart::Sunday);
        assert_eq!(back.text_size, TextSize::Large);
        assert_eq!(back.color_scheme, ColorScheme::TobiKogane);
        assert_eq!(back.figures, Figures::App);
        assert!(!back.show_unnamed);
        assert!(!back.show_uncovered);
    }

    #[test]
    fn every_scheme_survives_a_write_and_none_shares_a_token() {
        for scheme in ColorScheme::ALL {
            let mut s = Settings::new(Lang::English);
            s.color_scheme = scheme;
            let back = Settings::parse(&s.to_text(), Lang::English);
            assert_eq!(back.color_scheme, scheme, "{scheme:?} did not survive");
        }
        let mut tokens: Vec<&str> = ColorScheme::ALL.iter().map(|c| c.token()).collect();
        tokens.sort_unstable();
        let count = tokens.len();
        tokens.dedup();
        assert_eq!(tokens.len(), count, "two schemes share a token");
    }

    #[test]
    fn a_file_written_before_the_schemes_existed_opens_on_the_default() {
        let s = Settings::parse("language=e\ntext_size=large\n", Lang::English);
        assert_eq!(s.color_scheme, ColorScheme::KurenaiKon);
        assert_eq!(s.text_size, TextSize::Large, "the rest still reads");
        // Tokens `ColorScheme::of_token` answers `None` for.
        for token in ["notacolour", "azure", "asagi"] {
            let odd = Settings::parse(&format!("color_scheme={token}\n"), Lang::English);
            assert_eq!(odd.color_scheme, ColorScheme::KurenaiKon, "{token}");
        }
    }

    #[test]
    fn a_sitting_counts_from_a_minute_until_the_page_says_otherwise() {
        assert_eq!(
            Settings::new(Lang::English).sitting_floor,
            SittingFloor::OneMinute
        );
        // Text with no `sitting_floor` line.
        let old = Settings::parse("language=e\nfigures=app\n", Lang::English);
        assert_eq!(old.sitting_floor, SittingFloor::OneMinute);
        assert_eq!(old.figures, Figures::App, "the rest still reads");
        // A token `of_token` answers `None` for keeps the default.
        for token in ["", "1", "60", "none", "2m"] {
            let odd = Settings::parse(&format!("sitting_floor={token}\n"), Lang::English);
            assert_eq!(odd.sitting_floor, SittingFloor::OneMinute, "{token}");
        }
        // Every floor survives a write, on a token of its own.
        for floor in SittingFloor::ALL {
            let mut s = Settings::new(Lang::English);
            s.sitting_floor = floor;
            let back = Settings::parse(&s.to_text(), Lang::English);
            assert_eq!(back.sitting_floor, floor, "{floor:?} did not survive");
        }
        let mut tokens: Vec<&str> = SittingFloor::ALL.iter().map(|f| f.token()).collect();
        tokens.sort_unstable();
        let count = tokens.len();
        tokens.dedup();
        assert_eq!(tokens.len(), count, "two floors share a token");
    }

    #[test]
    fn the_floors_run_in_order_and_each_states_its_own_chip() {
        let secs: Vec<i64> = SittingFloor::ALL.iter().map(|f| f.seconds()).collect();
        assert_eq!(secs, [0, 60, 300, 900]);
        let mut sorted = secs.clone();
        sorted.sort_unstable();
        assert_eq!(sorted, secs, "the chips are not in order");
        // `label` is `minutes` and `Strings::minutes`.
        let s = Lang::English.strings();
        let chips: Vec<String> = SittingFloor::ALL.iter().map(|f| f.label(s)).collect();
        assert_eq!(chips, ["0m", "1m", "5m", "15m"]);
        for lang in Lang::ALL {
            let s = lang.strings();
            for floor in SittingFloor::ALL {
                assert!(
                    floor.label(s).contains(s.minutes),
                    "{lang:?} {floor:?} states no unit"
                );
            }
        }
    }

    #[test]
    fn the_unnamed_books_are_counted_until_the_page_says_otherwise() {
        assert!(Settings::new(Lang::English).show_unnamed);
        // Text with no `show_unnamed` line.
        assert!(Settings::parse("language=e\n", Lang::English).show_unnamed);
        assert!(!Settings::parse("show_unnamed=no\n", Lang::English).show_unnamed);
        assert!(Settings::parse("show_unnamed=yes\n", Lang::English).show_unnamed);
    }

    #[test]
    fn a_book_with_no_jacket_is_listed_until_the_page_says_otherwise() {
        assert!(Settings::new(Lang::English).show_uncovered);
        // Text with no `show_uncovered` line.
        assert!(Settings::parse("language=e\n", Lang::English).show_uncovered);
        assert!(!Settings::parse("show_uncovered=no\n", Lang::English).show_uncovered);
        // `show_uncovered` and `show_unnamed` are separate fields.
        let hidden = Settings::parse("show_uncovered=no\n", Lang::English);
        assert!(hidden.show_unnamed, "hiding one hid the other");
    }

    #[test]
    fn the_search_opens_on_the_books_until_it_has_been_flipped() {
        assert_eq!(Settings::new(Lang::English).scope, Scope::Books);
        // A file written before the scope existed.
        let old = Settings::parse("language=e\nshow_uncovered=no\n", Lang::English);
        assert_eq!(old.scope, Scope::Books);
        let marks = Settings::parse("scope=marks\n", Lang::English);
        assert_eq!(marks.scope, Scope::Marks);
        // A token `of_token` answers `None` for keeps the default.
        let odd = Settings::parse("scope=highlights\n", Lang::English);
        assert_eq!(odd.scope, Scope::Books);
        // And every scope survives a write.
        for scope in Scope::ALL {
            let mut s = Settings::new(Lang::English);
            s.scope = scope;
            assert_eq!(Settings::parse(&s.to_text(), Lang::English).scope, scope);
        }
    }

    #[test]
    fn one_bad_line_does_not_cost_the_other_settings() {
        let text = "language=t\nweek_start=notaday\n";
        let s = Settings::parse(text, Lang::English);
        assert_eq!(s.language, Lang::TraditionalChinese);
        assert_eq!(s.week_start, WeekStart::Monday, "the default stands");
    }

    #[test]
    fn a_later_build_s_setting_survives_this_one() {
        // `to_text` reprints `future_setting=7`.
        let text = "language=d\nfuture_setting=7\n";
        let s = Settings::parse(text, Lang::English);
        assert_eq!(s.language, Lang::German);
        assert!(s.to_text().contains("future_setting=7"));
    }

    #[test]
    fn the_sizes_run_in_order_and_medium_is_the_base() {
        assert_eq!(TextSize::Medium.scale(), 1.0);
        assert!(TextSize::Small.scale() < TextSize::Medium.scale());
        assert!(TextSize::Large.scale() > TextSize::Medium.scale());
    }

    #[test]
    fn a_sunday_week_shows_the_same_days_in_another_order() {
        // `date::weekday` answers the Monday-first index.
        let (mon, sun) = (WeekStart::Monday, WeekStart::Sunday);
        assert_eq!(mon.column_of(0), 0, "Monday leads a Monday week");
        assert_eq!(sun.column_of(6), 0, "Sunday leads a Sunday week");
        assert_eq!(sun.column_of(0), 1, "Monday is second");
        for start in WeekStart::ALL {
            let seen: Vec<usize> = (0..7).map(|c| start.day_in(c)).collect();
            let mut sorted = seen.clone();
            sorted.sort_unstable();
            assert_eq!(sorted, (0..7).collect::<Vec<_>>(), "{start:?} drops a day");
            for day in 0..7 {
                assert_eq!(start.day_in(start.column_of(day)), day, "{start:?} {day}");
            }
        }
    }
}
