//! Font fallback over the faces the device ships. [`select`] picks one face
//! per string, dropping to per-character where none covers the run.
//! [`FontChain::load`] reads the first candidate and leaves the rest `Pending`.

use std::path::{Path, PathBuf};

use ab_glyph::{Font as _, FontVec};
use anyhow::{Result, anyhow};

/// The regional convention a run of text is set in.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Script {
    /// No preference. [`Script::Unknown`] promotes no face.
    #[default]
    Unknown,
    Japanese,
    SimplifiedChinese,
    TraditionalChinese,
}

impl Script {
    /// Read a language tag as a preference. Accepts BCP-47 shapes, `_` or `-`
    /// separated, in any case.
    pub fn of_language(tag: &str) -> Script {
        let mut subtags = tag.split(['-', '_']).map(str::trim);
        let primary = subtags.next().unwrap_or_default().to_ascii_lowercase();
        match primary.as_str() {
            // `jp` is a country code.
            "ja" | "jp" => Script::Japanese,
            // CLDR resolves `yue` to `yue-Hant-HK`.
            "yue" => Script::TraditionalChinese,
            "zh" => {
                for subtag in subtags {
                    match subtag.to_ascii_lowercase().as_str() {
                        "hant" | "tw" | "hk" | "mo" => return Script::TraditionalChinese,
                        "hans" | "cn" | "sg" => return Script::SimplifiedChinese,
                        _ => {}
                    }
                }
                // CLDR resolves bare `zh` to `zh-Hans-CN`.
                Script::SimplifiedChinese
            }
            _ => Script::Unknown,
        }
    }
}

/// One face the device might have, and the convention it sets.
pub struct Candidate {
    pub path: PathBuf,
    pub script: Script,
}

/// Directories the firmware keeps faces in.
///
/// Scanned by [`discover`], after `READINGLOG_FONTS` where that is set,
/// colon-separated. A missing directory costs one failed `read_dir`.
const FONT_DIRS: &[&str] = &[
    // The firmware set.
    "/usr/java/lib/fonts",
    "/usr/share/fonts",
    // User-installed faces, ranked below the firmware set.
    "/mnt/us/fonts",
];

// Unscanned: `/chroot/usr/java/lib/fonts` mirrors the system set and
// `/var/local/font/**` holds the CJK packs.

/// Face families in preference order, matched case-insensitively against the
/// filename. `ember` leads; `code2000`, a pan-Unicode catch-all, sorts last.
const PREFERRED: &[&str] = &[
    "ember",
    "bookerly",
    "baskerville",
    "caecilia",
    "helvetica",
    "code2000",
];

/// Distance from the plain upright, lower better. The tokens overlap on real
/// filenames: `Amazon-Ember-RegularItalic` rules out on italic first,
/// `AmazonEmberBold-Regular` on weight, `-Heavy` and `-Medium` below "regular".
fn weight_rank(lower: &str) -> usize {
    if lower.contains("italic") || lower.contains("oblique") {
        return 3;
    }
    if ["bold", "heavy", "black", "light", "thin", "cond", "medium"]
        .iter()
        .any(|w| lower.contains(w))
    {
        return 2;
    }
    if lower.contains("regular") {
        return 0;
    }
    // No weight token at all — `code2000`, say.
    1
}

/// Rank a filename: lower sorts earlier. Family first, then weight.
fn rank(file_name: &str) -> (usize, usize) {
    let lower = file_name.to_ascii_lowercase();
    let family = PREFERRED
        .iter()
        .position(|p| lower.contains(p))
        .unwrap_or(PREFERRED.len());
    (family, weight_rank(&lower))
}

/// Everything drawable on disk, best first, feeding [`FontChain::load`].
pub fn discover() -> Vec<Candidate> {
    let mut found: Vec<(usize, usize, PathBuf)> = Vec::new();
    let extra = std::env::var("READINGLOG_FONTS").unwrap_or_default();
    let dirs = extra
        .split(':')
        .filter(|dir| !dir.is_empty())
        .chain(FONT_DIRS.iter().copied());
    for dir in dirs {
        let Ok(entries) = std::fs::read_dir(dir) else {
            continue;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            let Some(name) = path.file_name().and_then(|n| n.to_str()) else {
                continue;
            };
            let ext = path
                .extension()
                .and_then(|e| e.to_str())
                .unwrap_or_default()
                .to_ascii_lowercase();
            if !matches!(ext.as_str(), "ttf" | "otf" | "ttc") {
                continue;
            }
            let (family, styled) = rank(name);
            found.push((family, styled, path));
        }
    }
    found.sort();
    found
        .into_iter()
        .map(|(_, _, path)| Candidate {
            path,
            // No candidate claims a script; a language hint promotes none.
            script: Script::Unknown,
        })
        .collect()
}

/// Which face draws a run of text, decided once per string.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Selection {
    /// This face has every character in the run — draw it all with that one.
    Whole(usize),
    /// No single face covers the run: per-character resolution, carrying the
    /// run's preference for the order.
    PerChar(Script),
}

/// The face setting the wanted convention first, then the chain as declared.
/// `promoted` indexes the chain.
pub fn visiting_order(faces: usize, promoted: Option<usize>) -> impl Iterator<Item = usize> {
    promoted
        .into_iter()
        .chain((0..faces).filter(move |face| Some(*face) != promoted))
}

/// First face in `order` carrying every visible character of `text`.
/// `has_glyph` is asked until a face misses.
pub fn covering_face<I, F>(text: &str, order: I, mut has_glyph: F) -> Option<usize>
where
    I: IntoIterator<Item = usize>,
    F: FnMut(usize, char) -> bool,
{
    order.into_iter().find(|&face| {
        text.chars()
            .filter(|c| !is_invisible(*c))
            .all(|c| has_glyph(face, c))
    })
}

/// First face in `order` that has `ch`, or `None` when none does.
pub fn face_with<I, F>(ch: char, order: I, mut has_glyph: F) -> Option<usize>
where
    I: IntoIterator<Item = usize>,
    F: FnMut(usize, char) -> bool,
{
    order.into_iter().find(|&face| has_glyph(face, ch))
}

/// Code points carrying no glyph: the C0/C1 controls, the BOM and zero-width
/// spaces, bidi marks, the word joiner, invisible operators, the soft hyphen.
/// None reaches the rasterizer, which answers a miss with a visible `.notdef`.
pub fn is_invisible(c: char) -> bool {
    c.is_control()
        || matches!(c,
            '\u{00AD}'                  // soft hyphen
            | '\u{200B}'..='\u{200F}'   // ZWSP, ZWNJ, ZWJ, LRM, RLM
            | '\u{2060}'..='\u{2064}'   // word joiner + invisible operators
            | '\u{FEFF}'                // BOM / zero-width no-break space
        )
}

/// An ordered set of faces: the first usable candidate, read up front, plus
/// the rest of the chain waiting on disk.
pub struct FontChain {
    primary: FontVec,
    primary_path: PathBuf,
    primary_script: Script,
    rest: Vec<Face>,
}

/// A fallback slot. [`FontChain::load`] drops a candidate it cannot read.
/// `path` outlives the read, for [`FontChain::paths`].
struct Face {
    path: PathBuf,
    script: Script,
    state: State,
}

enum State {
    /// On disk, unparsed.
    Pending,
    Loaded(FontVec),
    /// Unreadable or unparsable. Skipped from here on.
    Absent,
}

impl FontChain {
    /// The first of `candidates` that parses as the primary, the rest as
    /// fallbacks unread until a character misses.
    /// Fails on an empty chain alone.
    pub fn load(candidates: &[Candidate]) -> Result<Self> {
        // Existence is settled here, parsing is not: a stat per candidate is
        // free, and it keeps `paths` an honest account of this device.
        let mut present = candidates
            .iter()
            .filter(|candidate| candidate.path.is_file());
        let mut primary = None;
        for candidate in present.by_ref() {
            if let Some(font) = read_face(&candidate.path) {
                primary = Some((font, candidate));
                break;
            }
        }
        let Some((primary, first)) = primary else {
            let tried: Vec<String> = candidates
                .iter()
                .map(|c| c.path.display().to_string())
                .collect();
            return Err(anyhow!("no usable font among {tried:?}"));
        };
        let rest = present
            .map(|candidate| Face {
                path: candidate.path.clone(),
                script: candidate.script,
                state: State::Pending,
            })
            .collect();
        Ok(Self {
            primary,
            primary_path: first.path.clone(),
            primary_script: first.script,
            rest,
        })
    }

    /// Faces in the chain, read or not. Never zero: [`FontChain::load`] fails
    /// on an empty chain.
    pub fn faces(&self) -> usize {
        1 + self.rest.len()
    }

    /// The chain on this device, primary first. Logged at startup.
    pub fn paths(&self) -> impl Iterator<Item = &Path> {
        std::iter::once(self.primary_path.as_path())
            .chain(self.rest.iter().map(|face| face.path.as_path()))
    }

    /// The face line metrics come from, whichever face draws a row.
    pub fn primary(&self) -> &FontVec {
        &self.primary
    }

    /// Face for the whole of `text`, preferring the one that sets `script` —
    /// see [`covering_face`].
    pub fn select(&mut self, text: &str, script: Script) -> Selection {
        let order = visiting_order(self.faces(), self.promoted(script));
        match covering_face(text, order, |face, ch| {
            self.ensure(face).is_some_and(|font| has_glyph(font, ch))
        }) {
            Some(face) => Selection::Whole(face),
            None => Selection::PerChar(script),
        }
    }

    /// The face index and the face itself for `ch` under `selection`, or
    /// `None` when nothing in the chain has the character. The index is the
    /// glyph cache's key: two faces rasterize the same codepoint differently.
    pub fn glyph_source(&mut self, selection: Selection, ch: char) -> Option<(usize, &FontVec)> {
        let face = match selection {
            Selection::Whole(face) => face,
            Selection::PerChar(script) => {
                let order = visiting_order(self.faces(), self.promoted(script));
                face_with(ch, order, |face, c| {
                    self.ensure(face).is_some_and(|font| has_glyph(font, c))
                })?
            }
        };
        self.ensure(face).map(|font| (face, font))
    }

    /// Chain position of the face setting `script` on this device.
    /// [`Script::Unknown`] promotes nothing: it names no preference, and the
    /// pan-Unicode catch-all sets it too.
    fn promoted(&self, script: Script) -> Option<usize> {
        if script == Script::Unknown {
            return None;
        }
        std::iter::once(self.primary_script)
            .chain(self.rest.iter().map(|face| face.script))
            .position(|candidate| candidate == script)
    }

    /// Face `index`, reading it from disk on first use.
    fn ensure(&mut self, index: usize) -> Option<&FontVec> {
        if index == 0 {
            return Some(&self.primary);
        }
        let face = self.rest.get_mut(index - 1)?;
        if matches!(face.state, State::Pending) {
            face.state = match read_face(&face.path) {
                Some(font) => State::Loaded(font),
                None => State::Absent,
            };
        }
        match &face.state {
            State::Loaded(font) => Some(font),
            State::Pending | State::Absent => None,
        }
    }
}

/// Whether `font` can draw `ch` at all. Glyph 0 is `.notdef`, which is what a
/// face hands back for a character it doesn't have — asking for it is how the
/// tofu got drawn in the first place.
pub fn has_glyph(font: &FontVec, ch: char) -> bool {
    font.glyph_id(ch).0 != 0
}

/// Read and parse one candidate. `None` covers both a path this firmware
/// doesn't have and a file that won't parse: either way the answer is "skip
/// this face", not "fail" — the chain only has to keep one.
fn read_face(path: &Path) -> Option<FontVec> {
    let bytes = std::fs::read(path).ok()?;
    FontVec::try_from_vec(bytes).ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Coverage oracle over face repertoires spelled as strings, one entry
    /// of `faces` per chain position.
    fn repertoires<'a>(faces: &'a [&'a str]) -> impl FnMut(usize, char) -> bool + 'a {
        move |face, ch| faces[face].contains(ch)
    }

    /// The chain tried in declared order — what a caller that knows nothing
    /// about the text's language gets.
    fn unhinted(faces: &[&str]) -> impl Iterator<Item = usize> {
        visiting_order(faces.len(), None)
    }

    #[test]
    fn a_run_one_face_covers_is_drawn_entirely_by_it() {
        // Every character is in face 0.
        let faces = ["あいう漢字", "汉字"];
        let face = covering_face("漢字あい", unhinted(&faces), repertoires(&faces));
        assert_eq!(face, Some(0));
    }

    #[test]
    fn a_run_the_first_face_misses_moves_whole_to_the_next() {
        // 楼 is in both faces, 红 only in face 1. Selection is per string:
        // face 1 draws 楼 too.
        let faces = ["楼梦", "红楼梦魇"];
        assert_eq!(
            covering_face("红楼梦魇", unhinted(&faces), repertoires(&faces)),
            Some(1)
        );
    }

    #[test]
    fn a_run_no_single_face_covers_resolves_per_character() {
        let faces = ["あ", "汉"];
        assert_eq!(
            covering_face("あ汉", unhinted(&faces), repertoires(&faces)),
            None
        );
        assert_eq!(
            face_with('あ', unhinted(&faces), repertoires(&faces)),
            Some(0)
        );
        assert_eq!(
            face_with('汉', unhinted(&faces), repertoires(&faces)),
            Some(1)
        );
    }

    #[test]
    fn a_character_no_face_has_resolves_to_nothing() {
        let faces = ["あ", "汉"];
        assert_eq!(face_with('𐀀', unhinted(&faces), repertoires(&faces)), None);
    }

    #[test]
    fn invisible_characters_do_not_decide_the_face() {
        // A title carrying a stray BOM must not be pushed off the face that
        // covers its visible text — no face has a glyph for U+FEFF.
        let faces = ["あいう", "汉字"];
        assert_eq!(
            covering_face("あ\u{FEFF}い", unhinted(&faces), repertoires(&faces)),
            Some(0)
        );
        assert!(is_invisible('\u{FEFF}'));
        assert!(!is_invisible('あ'));
    }

    #[test]
    fn control_characters_never_reach_the_rasterizer() {
        // A banner message joins its clauses with `\n`, and no face carries
        // U+000A. The layout consumes it; the renderer skips what is left,
        // here and for any control riding in on metadata.
        for c in ['\n', '\r', '\t', '\u{0}', '\u{7F}', '\u{85}'] {
            assert!(is_invisible(c), "{c:?} would draw as a box");
        }
        // Nothing in the chain has one, which is the whole damage: the box got
        // drawn, and the miss also pushed the rest of the run off its face.
        let faces = ["Synced 3", "汉字"];
        assert_eq!(face_with('\n', unhinted(&faces), repertoires(&faces)), None);
        assert_eq!(
            covering_face("Synced 3\nSynced 3", unhinted(&faces), repertoires(&faces)),
            Some(0)
        );
    }

    #[test]
    fn an_empty_run_selects_the_first_face() {
        let faces = ["あ"];
        assert_eq!(
            covering_face("", unhinted(&faces), repertoires(&faces)),
            Some(0)
        );
    }

    #[test]
    fn a_hint_moves_its_face_to_the_front_and_keeps_the_rest_in_order() {
        assert_eq!(visiting_order(4, Some(2)).collect::<Vec<_>>(), [2, 0, 1, 3]);
        assert_eq!(visiting_order(4, None).collect::<Vec<_>>(), [0, 1, 2, 3]);
        assert_eq!(visiting_order(4, Some(0)).collect::<Vec<_>>(), [0, 1, 2, 3]);
    }

    #[test]
    fn a_hint_wins_over_a_face_that_would_also_have_covered_the_run() {
        // Face 0 (Japanese) covers every character of this Traditional title.
        // Face 2 is the Traditional one.
        let faces = ["粵語語法講義", "粤语语法讲义", "粵語語法講義"];
        assert_eq!(
            covering_face("粵語語法講義", visiting_order(3, None), repertoires(&faces)),
            Some(0)
        );
        assert_eq!(
            covering_face(
                "粵語語法講義",
                visiting_order(3, Some(2)),
                repertoires(&faces)
            ),
            Some(2)
        );
    }

    #[test]
    fn a_hinted_face_that_misses_still_loses_to_coverage() {
        // A wrong language tag costs regional shapes, never glyphs.
        let faces = ["紅樓夢魘", "红楼梦魇"];
        assert_eq!(
            covering_face("红楼梦魇", visiting_order(2, Some(0)), repertoires(&faces)),
            Some(1)
        );
    }

    #[test]
    fn language_tags_name_the_convention_they_are_set_in() {
        assert_eq!(Script::of_language("ja"), Script::Japanese);
        // A country code where a language belongs — real imported metadata.
        assert_eq!(Script::of_language("jp"), Script::Japanese);
        assert_eq!(Script::of_language("zh-Hant"), Script::TraditionalChinese);
        assert_eq!(Script::of_language("zh_TW"), Script::TraditionalChinese);
        assert_eq!(Script::of_language("ZH-HK"), Script::TraditionalChinese);
        assert_eq!(Script::of_language("yue"), Script::TraditionalChinese);
        assert_eq!(Script::of_language("zh-Hans"), Script::SimplifiedChinese);
        assert_eq!(Script::of_language("zh-CN"), Script::SimplifiedChinese);
        // CLDR resolves bare `zh` to Simplified.
        assert_eq!(Script::of_language("zh"), Script::SimplifiedChinese);
        // No chain position claims these tags.
        assert_eq!(Script::of_language("en"), Script::Unknown);
        assert_eq!(Script::of_language("ko"), Script::Unknown);
        assert_eq!(Script::of_language(""), Script::Unknown);
    }

    #[test]
    fn the_ui_font_wins_on_a_real_device() {
        // Every Latin face on a Colorsoft (firmware 5.18), verbatim. Three
        // carry no bold-ish token and rank below Regular.
        let mut faces = [
            "Amazon-Ember-Bold.ttf",
            "Amazon-Ember-BoldItalic.ttf",
            "Amazon-Ember-Heavy.ttf",
            "Amazon-Ember-HeavyItalic.ttf",
            "Amazon-Ember-Medium.ttf",
            "Amazon-Ember-MediumItalic.ttf",
            "Amazon-Ember-Regular.ttf",
            "Amazon-Ember-RegularItalic.ttf",
            "AmazonEmberBold-Bold.ttf",
            "AmazonEmberBold-Regular.ttf",
            "Baskerville-Regular.ttf",
            "Bookerly-Regular.ttf",
            "BookerlyDisplay-Regular.ttf",
            "Caecilia_LT_65_Medium.ttf",
            "Futura-Medium.ttf",
            "Helvetica_LT_65_Medium.ttf",
            "KindleBlackboxRegular.ttf",
            "Palatino-Regular.ttf",
            "code2000.ttf",
        ];
        faces.sort_by_key(|f| (rank(f), *f));

        assert_eq!(
            faces[0], "Amazon-Ember-Regular.ttf",
            "the device's UI typeface, upright and regular, must draw the UI"
        );
        // The traps, each of which beat Regular under an earlier ranking:
        // a bold family whose cut is named "Regular", and two weights with no
        // bold-ish token in the name at all.
        for trap in [
            "AmazonEmberBold-Regular.ttf",
            "Amazon-Ember-Heavy.ttf",
            "Amazon-Ember-Medium.ttf",
            "Amazon-Ember-RegularItalic.ttf",
        ] {
            assert!(
                rank("Amazon-Ember-Regular.ttf") < rank(trap),
                "{trap} should rank below the plain regular"
            );
        }
        // The pan-Unicode catch-all ranks below every [`PREFERRED`] family and
        // above the unnamed ones (Blackbox, Futura, the Noto script packs).
        for named in ["Bookerly-Regular.ttf", "Baskerville-Regular.ttf"] {
            assert!(rank(named) < rank("code2000.ttf"));
        }
        assert!(rank("code2000.ttf") < rank("KindleBlackboxRegular.ttf"));
    }

    /// The catalog's `p_languages_0` is what reaches this, in the shapes the
    /// framework writes it.
    #[test]
    fn a_catalog_language_tag_picks_the_convention_to_set_in() {
        assert_eq!(Script::of_language("en"), Script::Unknown);
        assert_eq!(Script::of_language(""), Script::Unknown);
        assert_eq!(Script::of_language("ja-JP"), Script::Japanese);
        assert_eq!(Script::of_language("zh"), Script::SimplifiedChinese);
        assert_eq!(
            Script::of_language("zh-Hant-TW"),
            Script::TraditionalChinese
        );
    }

    #[test]
    fn a_chain_with_no_readable_candidate_fails_to_load() {
        // An empty chain is the one fatal case; a missing candidate is
        // skipped.
        let nowhere = [Candidate {
            path: PathBuf::from("/nonexistent/font.ttf"),
            script: Script::Unknown,
        }];
        let Err(err) = FontChain::load(&nowhere) else {
            panic!("a chain over a path that isn't there has nothing to draw with");
        };
        assert!(err.to_string().contains("no usable font"));
        assert!(FontChain::load(&[]).is_err());
    }
}
