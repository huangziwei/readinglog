//! A workspace: every Kindle's imports and record under its serial, the
//! backups no Kindle was named for and the record they fold into, the edits,
//! the posts and `site.toml`. The site is built from it into a folder of its own.

use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};

use readinglog_native::covers;
use readinglog_native::lang::Lang;
use readinglog_native::record::Record;
use readinglog_native::settings::{ColorScheme, Settings, SittingFloor, StatsFrom, WeekStart};

/// The file naming the site and the settings its figures are stated under.
pub const SITE_FILE: &str = "site.toml";

/// Every backup imported, byte for byte.
pub const BACKUPS: &str = "backups";

/// The record those backups fold into: `sessions.tsv` and its cover copies.
pub const RECORD: &str = "record";

/// One Markdown file per post.
pub const POSTS: &str = "posts";

/// A folder per Kindle, named for its serial, each holding that Kindle's
/// `BACKUPS` and `RECORD` as the workspace holds those no Kindle was named for.
pub const KINDLES: &str = "kindles";

/// Whether a book `books.toml` makes no choice about is on the site.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum NewBooks {
    #[default]
    Shown,
    Hidden,
}

/// `site.toml`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct SiteToml {
    /// The site's name, at the head of every page.
    pub title: String,
    /// Whose reading it is.
    pub author: String,
    /// The address the feed's links are made whole against; empty where unset.
    pub url: String,
    /// The language of the site's own words, a `Lang::language_tag`.
    pub language: String,
    /// Whether a book `books.toml` makes no choice about is shown.
    pub new_books: NewBooks,
    /// The folder the site is written to: under the workspace, or absolute.
    pub out: String,
    /// The Kindle settings the figures take, each as its settings file spells it.
    pub figures: String,
    pub sitting_floor: String,
    pub week_start: String,
    pub color_scheme: String,
    /// `yes` or `no`: whether a total counts reading on unidentified books.
    pub show_unnamed: String,
    /// `yes` or `no`: whether a book no cover can be drawn for is listed.
    pub show_uncovered: String,
}

impl Default for SiteToml {
    fn default() -> Self {
        // The Kindle's defaults, in its own words.
        let kindle = Settings::new(Lang::English).to_text();
        Self {
            title: "Reading".into(),
            author: String::new(),
            url: String::new(),
            language: Lang::English.language_tag().into(),
            new_books: NewBooks::default(),
            out: "site".into(),
            figures: token(&kindle, "figures").into(),
            sitting_floor: token(&kindle, "sitting_floor").into(),
            week_start: token(&kindle, "week_start").into(),
            color_scheme: token(&kindle, "color_scheme").into(),
            show_unnamed: token(&kindle, "show_unnamed").into(),
            show_uncovered: token(&kindle, "show_uncovered").into(),
        }
    }
}

impl SiteToml {
    /// The Kindle settings this file states, through `Settings::parse`, and a
    /// line for each value `Settings::parse` takes the default in place of.
    pub fn settings(&self) -> (Settings, Vec<String>) {
        let asked = [
            ("figures", &self.figures),
            ("sitting_floor", &self.sitting_floor),
            ("week_start", &self.week_start),
            ("color_scheme", &self.color_scheme),
            ("show_unnamed", &self.show_unnamed),
            ("show_uncovered", &self.show_uncovered),
        ];
        let text: String = asked
            .iter()
            .map(|(key, value)| format!("{key}={}\n", value.trim()))
            .collect();
        let settings = Settings::parse(&text, self.lang());
        let stated = settings.to_text();
        let problems = asked
            .iter()
            .filter(|(key, value)| token(&stated, key) != value.trim())
            .map(|(key, value)| {
                format!(
                    "{SITE_FILE}: {key} = \"{value}\" is not a value the Kindle writes; \
                     \"{}\" stands",
                    token(&stated, key)
                )
            })
            .collect();
        (settings, problems)
    }

    /// The language `language` names, English where it names none of the five.
    pub fn lang(&self) -> Lang {
        Lang::ALL
            .into_iter()
            .find(|l| l.language_tag().eq_ignore_ascii_case(self.language.trim()))
            .unwrap_or(Lang::English)
    }
}

/// What `key` is set to in `text`, a settings file's `key=value` lines.
fn token<'a>(text: &'a str, key: &str) -> &'a str {
    text.lines()
        .find_map(|line| line.strip_prefix(key)?.strip_prefix('='))
        .unwrap_or_default()
}

/// One value a setting takes: the word `site.toml` writes, and its name.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Pick {
    pub token: String,
    pub label: String,
}

/// One of `site.toml`'s settings, named as the Kindle's config page names it,
/// with every value it takes.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Setting {
    /// Its key in `site.toml`.
    pub key: &'static str,
    pub label: String,
    pub picks: Vec<Pick>,
}

/// `site.toml`'s settings in the Kindle's own words, each value off the
/// Kindle's settings file and named in `lang` as its config page names it.
pub fn offered(lang: Lang) -> Vec<Setting> {
    let s = lang.strings();
    let word = |key: &str, set: &dyn Fn(&mut Settings)| {
        let mut settings = Settings::new(Lang::English);
        set(&mut settings);
        token(&settings.to_text(), key).to_string()
    };
    vec![
        Setting {
            key: "language",
            label: s.language_row.into(),
            picks: Lang::ALL
                .iter()
                .map(|l| Pick {
                    token: l.language_tag().into(),
                    label: l.label().into(),
                })
                .collect(),
        },
        Setting {
            key: "week_start",
            label: s.week_starts_on.into(),
            picks: WeekStart::ALL
                .iter()
                .map(|w| Pick {
                    token: word("week_start", &|x| x.week_start = *w),
                    label: s.weekdays_short[w.day_in(0)].into(),
                })
                .collect(),
        },
        Setting {
            key: "color_scheme",
            label: s.color_scheme.into(),
            picks: ColorScheme::ALL
                .iter()
                .zip(s.color_schemes)
                .map(|(c, name)| Pick {
                    token: word("color_scheme", &|x| x.color_scheme = *c),
                    label: name.into(),
                })
                .collect(),
        },
        Setting {
            key: "figures",
            label: s.stats_from_row.into(),
            picks: StatsFrom::ALL
                .iter()
                .map(|f| Pick {
                    token: word("figures", &|x| x.stats_from = *f),
                    label: match f {
                        StatsFrom::Kindle => s.stats_from_kindle,
                        StatsFrom::App => s.stats_from_app,
                    }
                    .into(),
                })
                .collect(),
        },
        Setting {
            key: "sitting_floor",
            label: s.sitting_floor_row.into(),
            picks: SittingFloor::ALL
                .iter()
                .map(|f| Pick {
                    token: word("sitting_floor", &|x| x.sitting_floor = *f),
                    label: f.label(s),
                })
                .collect(),
        },
        Setting {
            key: "show_unnamed",
            label: s.unidentified_row.into(),
            picks: [true, false]
                .iter()
                .map(|on| Pick {
                    token: word("show_unnamed", &|x| x.show_unidentified = *on),
                    label: shown_or_not(*on, s),
                })
                .collect(),
        },
        Setting {
            key: "show_uncovered",
            label: s.uncovered_row.into(),
            picks: [true, false]
                .iter()
                .map(|on| Pick {
                    token: word("show_uncovered", &|x| x.show_uncovered = *on),
                    label: shown_or_not(*on, s),
                })
                .collect(),
        },
    ]
}

/// `s.chip_show` or `s.chip_hide`, as the config page names the two picks.
fn shown_or_not(on: bool, s: &readinglog_native::lang::Strings) -> String {
    match on {
        true => s.chip_show.into(),
        false => s.chip_hide.into(),
    }
}

/// A workspace folder, and its `site.toml`.
#[derive(Debug, Clone)]
pub struct Workspace {
    pub dir: PathBuf,
    pub site: SiteToml,
}

impl Workspace {
    /// The workspace at `dir`, its folders made and a `site.toml` written
    /// where none stands. A `site.toml` standing is read, never rewritten.
    pub fn create(dir: &Path) -> Result<Self> {
        for sub in [BACKUPS, RECORD, POSTS] {
            let at = dir.join(sub);
            std::fs::create_dir_all(&at).with_context(|| format!("making {}", at.display()))?;
        }
        let file = dir.join(SITE_FILE);
        if !file.exists() {
            write_toml(&file, &SiteToml::default())?;
        }
        Self::open(dir)
    }

    /// The workspace at `dir`. A missing `site.toml` reads as the default.
    pub fn open(dir: &Path) -> Result<Self> {
        let file = dir.join(SITE_FILE);
        let site = match std::fs::read_to_string(&file) {
            Ok(text) => {
                toml::from_str(&text).with_context(|| format!("reading {}", file.display()))?
            }
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => SiteToml::default(),
            Err(err) => return Err(err).with_context(|| format!("reading {}", file.display())),
        };
        Ok(Self {
            dir: dir.to_path_buf(),
            site,
        })
    }

    /// Write [`Self::site`] back to `site.toml`.
    pub fn save_site(&self) -> Result<()> {
        write_toml(&self.dir.join(SITE_FILE), &self.site)
    }

    /// The backups no Kindle was named for.
    pub fn backups(&self) -> PathBuf {
        self.dir.join(BACKUPS)
    }

    /// The record those backups fold into, the folder `Record::load` and
    /// `Record::save` take.
    pub fn record_dir(&self) -> PathBuf {
        self.dir.join(RECORD)
    }

    /// The folder the Kindle `serial` names keeps its imports and its record in.
    pub fn kindle_dir(&self, serial: &str) -> PathBuf {
        self.dir.join(KINDLES).join(folder_name(serial))
    }

    /// Each Kindle's folder, in the order their names sort.
    pub fn kindles(&self) -> Vec<PathBuf> {
        let mut out: Vec<PathBuf> = std::fs::read_dir(self.dir.join(KINDLES))
            .map(|entries| {
                entries
                    .flatten()
                    .map(|e| e.path())
                    .filter(|p| p.is_dir())
                    .collect()
            })
            .unwrap_or_default();
        out.sort();
        out
    }

    /// Every record the workspace holds, by its folder: each Kindle's, then the
    /// one no Kindle was named for.
    pub fn record_dirs(&self) -> Vec<PathBuf> {
        let mut out: Vec<PathBuf> = self.kindles().iter().map(|k| k.join(RECORD)).collect();
        out.push(self.record_dir());
        out
    }

    /// The copy of the cover `key` names, off the first record holding one.
    pub fn cover(&self, key: &str) -> Option<PathBuf> {
        self.record_dirs()
            .into_iter()
            .find(|dir| covers::held(dir, key))
            .map(|dir| covers::path(&dir, key))
    }

    pub fn posts(&self) -> PathBuf {
        self.dir.join(POSTS)
    }

    /// The folder the site is written to.
    pub fn out(&self) -> PathBuf {
        let out = Path::new(self.site.out.trim());
        match out.is_absolute() {
            true => out.to_path_buf(),
            false => self.dir.join(out),
        }
    }

    /// Every record the workspace holds as one, through
    /// [`crate::kindles::together`]; empty where nothing has been imported.
    pub fn record(&self) -> Record {
        let kindles: Vec<Record> = self
            .kindles()
            .iter()
            .map(|k| Record::load(&k.join(RECORD)))
            .collect();
        crate::kindles::together(&kindles, &Record::load(&self.record_dir()))
    }
}

/// `serial` as one folder name: its ASCII letters, digits, `-` and `_`, any
/// other character as `_`.
fn folder_name(serial: &str) -> String {
    let name: String = serial
        .trim()
        .chars()
        .map(
            |c| match c.is_ascii_alphanumeric() || c == '-' || c == '_' {
                true => c,
                false => '_',
            },
        )
        .collect();
    match name.is_empty() {
        true => "kindle".into(),
        false => name,
    }
}

/// `value` as TOML at `path`, through [`write_file`].
pub(crate) fn write_toml<T: Serialize>(path: &Path, value: &T) -> Result<()> {
    let text =
        toml::to_string_pretty(value).with_context(|| format!("writing {}", path.display()))?;
    write_file(path, text.as_bytes())
}

/// `bytes` at `path` through a `.partial` sibling and a rename: the file is
/// the old one or the new one, never a part of either.
pub(crate) fn write_file(path: &Path, bytes: &[u8]) -> Result<()> {
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir).with_context(|| format!("making {}", dir.display()))?;
    }
    let name = path
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default();
    let partial = path.with_file_name(format!("{name}.partial"));
    std::fs::write(&partial, bytes).with_context(|| format!("writing {}", partial.display()))?;
    std::fs::rename(&partial, path).with_context(|| format!("writing {}", path.display()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tests::scratch;
    use readinglog_native::settings::{SittingFloor, StatsFrom, WeekStart};

    #[test]
    fn a_new_site_states_its_figures_as_a_new_kindle_does() {
        let site = SiteToml::default();
        let (settings, problems) = site.settings();
        let kindle = Settings::new(Lang::English);
        assert!(problems.is_empty(), "{problems:?}");
        assert_eq!(settings.stats_from, kindle.stats_from);
        assert_eq!(settings.sitting_floor, kindle.sitting_floor);
        assert_eq!(settings.week_start, kindle.week_start);
        assert_eq!(settings.color_scheme, kindle.color_scheme);
    }

    #[test]
    fn a_setting_in_the_kindles_words_is_taken_and_any_other_is_named() {
        let site = SiteToml {
            figures: "device".into(),
            sitting_floor: "5m".into(),
            week_start: "Sundays".into(),
            ..SiteToml::default()
        };
        let (settings, problems) = site.settings();
        assert_eq!(settings.stats_from, StatsFrom::Kindle);
        assert_eq!(settings.sitting_floor, SittingFloor::FiveMinutes);
        assert_eq!(settings.week_start, WeekStart::Monday);
        assert_eq!(problems.len(), 1);
        assert!(problems[0].contains("week_start"), "{problems:?}");
    }

    #[test]
    fn every_value_offered_is_one_the_kindle_writes() {
        for setting in offered(Lang::German) {
            assert!(setting.picks.len() >= 2, "{}", setting.key);
            for pick in &setting.picks {
                let mut site = SiteToml::default();
                let value = pick.token.clone();
                match setting.key {
                    "language" => {
                        site.language = value;
                        assert_eq!(site.lang().language_tag(), pick.token);
                        continue;
                    }
                    "figures" => site.figures = value,
                    "sitting_floor" => site.sitting_floor = value,
                    "week_start" => site.week_start = value,
                    "color_scheme" => site.color_scheme = value,
                    "show_unnamed" => site.show_unnamed = value,
                    "show_uncovered" => site.show_uncovered = value,
                    other => panic!("{other} is offered and read nowhere"),
                }
                assert!(
                    site.settings().1.is_empty(),
                    "{} = {}",
                    setting.key,
                    pick.token
                );
            }
        }
    }

    #[test]
    fn a_site_file_round_trips_and_a_missing_one_reads_as_the_default() {
        let dir = scratch("workspace");
        let made = Workspace::create(&dir).unwrap();
        assert_eq!(made.site, SiteToml::default());
        assert!(made.backups().is_dir() && made.record_dir().is_dir() && made.posts().is_dir());

        let mut changed = made.clone();
        changed.site.title = "Kept".into();
        changed.site.language = "ja".into();
        changed.save_site().unwrap();
        let read = Workspace::create(&dir).unwrap();
        assert_eq!(read.site.title, "Kept");
        assert_eq!(read.site.lang(), Lang::Japanese);

        let bare = scratch("workspace-bare");
        assert_eq!(Workspace::open(&bare).unwrap().site, SiteToml::default());
    }
}
