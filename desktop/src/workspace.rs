//! A workspace: every Kindle's imports and record under its cut serial, the
//! backups no Kindle was named for and the record they fold into, and
//! `site.toml`.

use std::path::{Path, PathBuf};
use std::sync::{Mutex, MutexGuard, PoisonError};

use anyhow::{Context, Result, anyhow};
use serde::{Deserialize, Serialize};

use readinglog_native::backup;
use readinglog_native::date;
use readinglog_native::lang::Lang;
use readinglog_native::record::Record;
use readinglog_native::settings::{ColorScheme, Settings, SittingFloor, StatsFrom, WeekStart};

use crate::edits::HIGHLIGHTS_FILE;
use crate::kindles::Source;
use crate::notes::NOTES_FILE;

/// `site.toml`: the window's language and the Kindle settings its figures are
/// stated under.
pub const CONFIG_FILE: &str = "site.toml";

/// `books.toml`, which a reset removes with every other entry of [`KEPT`].
const BOOKS_FILE: &str = "books.toml";

/// `sittings.toml`, which a reset removes with every other entry of [`KEPT`].
const SITTINGS_FILE: &str = "sittings.toml";

/// The folder of backups kept byte for byte, in the workspace and in each
/// Kindle's folder.
pub const BACKUPS: &str = "backups";

/// The folder of a record, `sessions.tsv` and its cover copies, beside `BACKUPS`.
pub const RECORD: &str = "record";

/// `posts/`: Markdown files, each read into a note once by
/// [`crate::notes::take_markdown`].
pub const MARKDOWN: &str = "posts";

/// A folder per Kindle, named for its cut serial, each holding that Kindle's
/// `BACKUPS` and `RECORD` as the workspace holds those no Kindle was named for.
pub const KINDLES: &str = "kindles";

/// Every entry a workspace keeps in its folder, by its name there.
const KEPT: [&str; 9] = [
    KINDLES,
    BACKUPS,
    RECORD,
    MARKDOWN,
    CONFIG_FILE,
    BOOKS_FILE,
    HIGHLIGHTS_FILE,
    NOTES_FILE,
    SITTINGS_FILE,
];

/// The file the macOS Finder writes into a folder it has shown.
const FINDER_FILE: &str = ".DS_Store";

/// Held across each read, merge and write of a record's file, one at a time.
static WRITING: Mutex<()> = Mutex::new(());

/// [`WRITING`], held until the guard drops.
pub(crate) fn writing() -> MutexGuard<'static, ()> {
    WRITING.lock().unwrap_or_else(PoisonError::into_inner)
}

/// `site.toml`: the window's Config.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct ConfigToml {
    /// The language of the window's own words, a `Lang::language_tag`.
    pub language: String,
    /// The Kindle settings the figures take, each as its settings file spells it.
    pub figures: String,
    pub sitting_floor: String,
    pub week_start: String,
    pub color_scheme: String,
    /// `yes` or `no`: whether a total counts reading on unidentified books.
    pub show_unnamed: String,
    /// `yes` or `no`: whether a book no cover can be drawn for is listed.
    pub show_uncovered: String,
    /// Every other key the file holds, written back as it was read.
    #[serde(flatten)]
    pub rest: toml::Table,
}

impl Default for ConfigToml {
    fn default() -> Self {
        // The Kindle's defaults, in its own words.
        let kindle = Settings::new(Lang::English).to_text();
        Self {
            language: Lang::English.language_tag().into(),
            figures: token(&kindle, "figures").into(),
            sitting_floor: token(&kindle, "sitting_floor").into(),
            week_start: token(&kindle, "week_start").into(),
            color_scheme: token(&kindle, "color_scheme").into(),
            show_unnamed: token(&kindle, "show_unnamed").into(),
            show_uncovered: token(&kindle, "show_uncovered").into(),
            rest: toml::Table::new(),
        }
    }
}

impl ConfigToml {
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
                    "{CONFIG_FILE}: {key} = \"{value}\" is not a value the Kindle writes; \
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
    pub config: ConfigToml,
}

impl Workspace {
    /// The workspace at `dir`, its folders made and a `site.toml` written
    /// where none stands. A `site.toml` standing is read, never rewritten.
    pub fn create(dir: &Path) -> Result<Self> {
        for sub in [BACKUPS, RECORD] {
            let at = dir.join(sub);
            std::fs::create_dir_all(&at).with_context(|| format!("making {}", at.display()))?;
        }
        let file = dir.join(CONFIG_FILE);
        if !file.exists() {
            write_toml(&file, &ConfigToml::default())?;
        }
        Self::open(dir)
    }

    /// The workspace at `dir`, `highlights.toml`, `notes.toml` and `posts/` read
    /// into its records where they stand. A missing `site.toml` reads as the
    /// default.
    pub fn open(dir: &Path) -> Result<Self> {
        let file = dir.join(CONFIG_FILE);
        let config = match std::fs::read_to_string(&file) {
            Ok(text) => {
                toml::from_str(&text).with_context(|| format!("reading {}", file.display()))?
            }
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => ConfigToml::default(),
            Err(err) => return Err(err).with_context(|| format!("reading {}", file.display())),
        };
        let ws = Self {
            dir: dir.to_path_buf(),
            config,
        };
        for taken in [
            crate::edits::take_file(&ws),
            crate::notes::take_file(&ws),
            crate::notes::take_markdown(&ws),
        ] {
            if let Err(err) = taken {
                eprintln!("!! {err:#}");
            }
        }
        Ok(ws)
    }

    /// The record of the Kindle whose cut serial is `kindle`, empty for the
    /// backups naming none.
    pub fn source(&self, kindle: &str) -> Result<Source> {
        self.sources()
            .into_iter()
            .find(|s| s.serial.as_deref().unwrap_or_default() == kindle)
            .ok_or_else(|| anyhow!("no record of {kindle} in {}", self.dir.display()))
    }

    /// Write [`Self::config`] back to `site.toml`.
    pub fn save_config(&self) -> Result<()> {
        write_toml(&self.dir.join(CONFIG_FILE), &self.config)
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

    /// Each record the workspace holds that states a sitting or a book: every
    /// Kindle's, then the one no Kindle was named for.
    pub fn sources(&self) -> Vec<Source> {
        self.kindles()
            .into_iter()
            .map(|kindle| Source {
                record: Record::load(&kindle.join(RECORD)),
                serial: kindle.file_name().map(|n| n.to_string_lossy().into_owned()),
                dir: kindle.join(RECORD),
            })
            .chain([Source {
                record: Record::load(&self.record_dir()),
                dir: self.record_dir(),
                serial: None,
            }])
            .filter(|s| !s.record.sittings.is_empty() || !s.record.books.is_empty())
            .collect()
    }

    /// Every record the workspace holds as one, through
    /// [`crate::kindles::together`]; empty where nothing has been imported.
    pub fn record(&self) -> Record {
        crate::kindles::together(&self.sources())
    }

    /// Every backup the workspace keeps, by the folder it sits under: each
    /// Kindle's, then those naming no Kindle; newest first within each.
    pub fn every_backup(&self) -> Vec<(PathBuf, backup::Backup)> {
        self.kindles()
            .into_iter()
            .chain([self.dir.clone()])
            .flat_map(|group| {
                backup::list(&group)
                    .into_iter()
                    .map(move |held| (group.clone(), held))
            })
            .collect()
    }

    /// Each of [`KEPT`] standing in the folder.
    pub fn kept(&self) -> Vec<PathBuf> {
        KEPT.iter()
            .map(|name| self.dir.join(name))
            .filter(|at| at.symlink_metadata().is_ok())
            .collect()
    }

    /// The bytes [`Self::kept`] takes on disk.
    pub fn kept_bytes(&self) -> u64 {
        self.kept().iter().map(|at| bytes_under(at)).sum()
    }

    /// Remove each of [`Self::kept`], then the folder itself where nothing but
    /// a `FINDER_FILE` is left in it. Every other entry stays where it is.
    pub fn reset(&self) -> Result<()> {
        let _held = writing();
        for at in self.kept() {
            match at.symlink_metadata().is_ok_and(|m| m.is_dir()) {
                true => std::fs::remove_dir_all(&at),
                false => std::fs::remove_file(&at),
            }
            .with_context(|| format!("removing {}", at.display()))?;
        }
        let Ok(entries) = std::fs::read_dir(&self.dir) else {
            return Ok(());
        };
        let left: Vec<PathBuf> = entries.flatten().map(|e| e.path()).collect();
        if !left.iter().all(|at| at.ends_with(FINDER_FILE)) {
            return Ok(());
        }
        for at in &left {
            std::fs::remove_file(at).with_context(|| format!("removing {}", at.display()))?;
        }
        std::fs::remove_dir(&self.dir).with_context(|| format!("removing {}", self.dir.display()))
    }
}

/// The bytes `at` takes: a file's length, a folder's files all the way down.
fn bytes_under(at: &Path) -> u64 {
    match std::fs::symlink_metadata(at) {
        Ok(meta) if meta.is_dir() => std::fs::read_dir(at)
            .map(|entries| entries.flatten().map(|e| bytes_under(&e.path())).sum())
            .unwrap_or(0),
        Ok(meta) => meta.len(),
        Err(_) => 0,
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

/// Write `source`'s notes and corrections into its record's file under
/// [`writing`], merged with the ones the file holds by `Record::take_written`;
/// every other line stands as the file holds it.
pub fn keep_written(source: &Source) -> Result<()> {
    let _held = writing();
    let file = Record::file(&source.dir);
    let held = std::fs::read_to_string(&file).unwrap_or_else(|_| source.record.text());
    let mut written = Record::from_text(&held);
    written.take_written(&source.record);
    let text = readinglog_native::written::splice(&held, &written);
    match text == held {
        true => Ok(()),
        false => write_file(&file, text.as_bytes()),
    }
}

/// The clock's moment, as `YYYY-MM-DDTHH:MM:SS`.
pub fn now() -> String {
    let (day, secs) = date::now();
    date::stamp(day, secs)
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
    fn a_new_config_states_its_figures_as_a_new_kindle_does() {
        let config = ConfigToml::default();
        let (settings, problems) = config.settings();
        let kindle = Settings::new(Lang::English);
        assert!(problems.is_empty(), "{problems:?}");
        assert_eq!(settings.stats_from, kindle.stats_from);
        assert_eq!(settings.sitting_floor, kindle.sitting_floor);
        assert_eq!(settings.week_start, kindle.week_start);
        assert_eq!(settings.color_scheme, kindle.color_scheme);
    }

    #[test]
    fn a_setting_in_the_kindles_words_is_taken_and_any_other_is_named() {
        let config = ConfigToml {
            figures: "device".into(),
            sitting_floor: "5m".into(),
            week_start: "Sundays".into(),
            ..ConfigToml::default()
        };
        let (settings, problems) = config.settings();
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
                let mut config = ConfigToml::default();
                let value = pick.token.clone();
                match setting.key {
                    "language" => {
                        config.language = value;
                        assert_eq!(config.lang().language_tag(), pick.token);
                        continue;
                    }
                    "figures" => config.figures = value,
                    "sitting_floor" => config.sitting_floor = value,
                    "week_start" => config.week_start = value,
                    "color_scheme" => config.color_scheme = value,
                    "show_unnamed" => config.show_unnamed = value,
                    "show_uncovered" => config.show_uncovered = value,
                    other => panic!("{other} is offered and read nowhere"),
                }
                assert!(
                    config.settings().1.is_empty(),
                    "{} = {}",
                    setting.key,
                    pick.token
                );
            }
        }
    }

    #[test]
    fn a_reset_takes_every_entry_the_workspace_keeps_and_nothing_else() {
        let dir = scratch("workspace-reset");
        let ws = Workspace::create(&dir).unwrap();
        let kindle = ws.kindle_dir("G003H2-0001");
        for at in [kindle.join(BACKUPS), kindle.join(RECORD)] {
            std::fs::create_dir_all(&at).unwrap();
            std::fs::write(at.join("held"), b"12345").unwrap();
        }
        std::fs::write(ws.backups().join("readinglog-260910-200851.zip"), b"zip").unwrap();
        std::fs::write(dir.join(BOOKS_FILE), b"").unwrap();
        assert_eq!(ws.kept().len(), 5, "{:?}", ws.kept());
        assert!(ws.kept_bytes() >= 13, "{}", ws.kept_bytes());
        let mine = dir.join("notes.txt");
        std::fs::write(&mine, b"mine").unwrap();
        ws.reset().unwrap();
        assert!(ws.kept().is_empty(), "{:?}", ws.kept());
        assert_eq!(
            std::fs::read(&mine).unwrap(),
            b"mine",
            "a file it did not make"
        );

        // A folder left holding only what the Finder wrote goes with the rest.
        let bare = scratch("workspace-reset-bare");
        let ws = Workspace::create(&bare).unwrap();
        std::fs::write(bare.join(FINDER_FILE), b"finder").unwrap();
        ws.reset().unwrap();
        assert!(!bare.exists());
    }

    #[test]
    fn a_config_round_trips_every_key_it_holds_and_a_missing_one_reads_as_the_default() {
        let dir = scratch("workspace");
        let made = Workspace::create(&dir).unwrap();
        assert_eq!(made.config, ConfigToml::default());
        assert!(made.backups().is_dir() && made.record_dir().is_dir());

        // A key no field reads is written back as it was read.
        let file = dir.join(CONFIG_FILE);
        let held = std::fs::read_to_string(&file).unwrap();
        std::fs::write(&file, format!("title = \"Kept\"\n{held}")).unwrap();
        let mut changed = Workspace::open(&dir).unwrap();
        changed.config.language = "ja".into();
        changed.save_config().unwrap();
        let read = Workspace::create(&dir).unwrap();
        assert_eq!(read.config.lang(), Lang::Japanese);
        assert_eq!(
            read.config.rest.get("title").and_then(|v| v.as_str()),
            Some("Kept")
        );

        let bare = scratch("workspace-bare");
        assert_eq!(
            Workspace::open(&bare).unwrap().config,
            ConfigToml::default()
        );
    }
}
