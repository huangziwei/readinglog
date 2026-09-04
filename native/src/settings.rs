//! Every setting the config page holds, and the file they survive a restart
//! in. Each has a default the device supplies or the app picks, and a line
//! this build does not know is kept on write.

use std::path::{Path, PathBuf};

use crate::lang::Lang;

/// Where the choices live, beside the extension and outside the store: a
/// setting is not a sitting, and `Store`'s `HEADER` must stay free to change
/// with what a sitting means.
const SETTINGS_PATHS: &[&str] = &[
    "/mnt/us/extensions/readinglog/settings",
    "/var/local/readinglog/settings",
];

/// How large the text on a screen is set.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum TextSize {
    Small,
    #[default]
    Medium,
    Large,
}

impl TextSize {
    pub const ALL: [TextSize; 3] = [TextSize::Small, TextSize::Medium, TextSize::Large];

    /// What it multiplies the body size by. `chrome::tabs` does not take it:
    /// its five cells are measured at the base size, and the chrome stays put
    /// while the content scales.
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

/// The colours a chart is drawn in. `ui::paint::Palette::for_panel` reads it
/// only where `eink::fb::has_cfa` holds.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum ColorScheme {
    /// One azure hue across the ramp, marked in warm red.
    #[default]
    Azure,
    /// 浅葱 across the ramp, marked in 朱.
    AsagiShu,
    /// 鳶 across the ramp, marked in 黄金.
    TobiKogane,
    /// 若竹 and 松葉 across the ramp, marked at 桜's hue.
    SakuraWakatake,
    /// 紺's hue across the ramp, marked at 紅's.
    KurenaiKon,
    /// The greys a panel without a colour filter draws, on one that has it.
    Grey,
}

impl ColorScheme {
    pub const ALL: [ColorScheme; 6] = [
        ColorScheme::Azure,
        ColorScheme::AsagiShu,
        ColorScheme::TobiKogane,
        ColorScheme::SakuraWakatake,
        ColorScheme::KurenaiKon,
        ColorScheme::Grey,
    ];

    fn token(self) -> &'static str {
        match self {
            ColorScheme::Azure => "azure",
            ColorScheme::AsagiShu => "asagi",
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

/// Which day a week is drawn from.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum WeekStart {
    #[default]
    Monday,
    Sunday,
}

impl WeekStart {
    pub const ALL: [WeekStart; 2] = [WeekStart::Monday, WeekStart::Sunday];

    /// How far to rotate a Monday-first weekday index for this start.
    /// `date::weekday` counts from Monday, which the ISO week and the grid
    /// both do; a Sunday-first week holds the same days in another order.
    pub fn shift(self) -> usize {
        match self {
            WeekStart::Monday => 0,
            WeekStart::Sunday => 1,
        }
    }

    /// The weekday `index` sits at, counting from this start.
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

/// Everything the config page sets.
#[derive(Clone, Debug, PartialEq)]
pub struct Settings {
    pub language: Lang,
    pub week_start: WeekStart,
    pub text_size: TextSize,
    /// The colours a chart is drawn in.
    pub color_scheme: ColorScheme,
    /// Whether a total counts reading on books the catalog names none of.
    pub show_unnamed: bool,
    /// Lines this build does not know, kept verbatim: a downgrade drops none
    /// of what a later build wrote.
    unknown: Vec<String>,
}

impl Settings {
    /// The defaults: the device's own language, and the week the way the ISO
    /// calendar and the existing grid draw it.
    pub fn new(detected: Lang) -> Self {
        Self {
            language: detected,
            week_start: WeekStart::default(),
            text_size: TextSize::default(),
            color_scheme: ColorScheme::default(),
            show_unnamed: true,
            unknown: Vec::new(),
        }
    }

    /// What is on disk, over the defaults. A missing or unreadable file is a
    /// config page never opened, and not an error.
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

    /// `key=value` a line, `#` a comment. A value that will not read keeps the
    /// default: one bad line must not cost every other setting.
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
                "show_unnamed" => out.show_unnamed = value != "no",
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
        let unnamed = match self.show_unnamed {
            true => "yes",
            false => "no",
        };
        out.push_str(&format!("show_unnamed={unnamed}\n"));
        for line in &self.unknown {
            out.push_str(line);
            out.push('\n');
        }
        out
    }

    /// Write to the first path whose directory exists. A device that will not
    /// take the file keeps the setting for this run and logs the refusal:
    /// losing a preference is not worth refusing to draw over.
    pub fn save(&self) {
        for path in SETTINGS_PATHS.iter().map(PathBuf::from) {
            let Some(dir) = path.parent() else { continue };
            if !dir.is_dir() {
                continue;
            }
            match std::fs::write(&path, self.to_text()) {
                Ok(()) => return,
                Err(err) => eprintln!("settings: {} not written: {err}", path.display()),
            }
        }
        eprintln!("settings: nowhere to write; this run keeps them");
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
        // And a file that is not there is the same thing, not an error.
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
        s.show_unnamed = false;
        let back = Settings::parse(&s.to_text(), Lang::English);
        assert_eq!(back.language, Lang::TraditionalChinese);
        assert_eq!(back.week_start, WeekStart::Sunday);
        assert_eq!(back.text_size, TextSize::Large);
        assert_eq!(back.color_scheme, ColorScheme::TobiKogane);
        assert!(!back.show_unnamed);
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
        assert_eq!(s.color_scheme, ColorScheme::Azure);
        assert_eq!(s.text_size, TextSize::Large, "the rest still reads");
        // A scheme no `of_token` arm names.
        let odd = Settings::parse("color_scheme=notacolour\n", Lang::English);
        assert_eq!(odd.color_scheme, ColorScheme::Azure);
    }

    #[test]
    fn the_unnamed_books_are_counted_until_the_page_says_otherwise() {
        assert!(Settings::new(Lang::English).show_unnamed);
        // A file written before this build carries no line for it.
        assert!(Settings::parse("language=e\n", Lang::English).show_unnamed);
        assert!(!Settings::parse("show_unnamed=no\n", Lang::English).show_unnamed);
        assert!(Settings::parse("show_unnamed=yes\n", Lang::English).show_unnamed);
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
        // A downgrade must not silently drop what a newer build wrote.
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
        // Monday-first indices are what `date::weekday` answers; the setting
        // only moves which column each lands in.
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
