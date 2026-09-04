//! Font fallback over the faces the device ships. A character is drawn from
//! the [`Band`] its script belongs to, never from whichever face covers the
//! whole string. [`FontChain::load`] leaves every candidate but one `Pending`.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use ab_glyph::{Font as _, FontVec, PxScale};
use anyhow::{Result, anyhow};

/// The regional convention a run of text is set in.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum Script {
    /// No preference. [`Script::resolve`] reads one off the text instead.
    #[default]
    Unknown,
    Japanese,
    SimplifiedChinese,
    TraditionalChinese,
    Korean,
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
            "ko" | "kr" => Script::Korean,
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

    /// The convention `text` is set in: the catalog's tag, else the
    /// characters. Kana settles a run for Japanese and Hangul for Korean; Han
    /// alone cannot be told apart and takes a bare `zh`'s default.
    pub fn resolve(hint: Script, text: &str) -> Script {
        if hint != Script::Unknown {
            return hint;
        }
        let mut han = false;
        for ch in text.chars() {
            if is_kana(ch) {
                return Script::Japanese;
            }
            if is_hangul(ch) {
                return Script::Korean;
            }
            han |= is_han(ch);
        }
        if han {
            Script::SimplifiedChinese
        } else {
            Script::Unknown
        }
    }

    /// The Han convention this script is set in. Every script has one: a run
    /// with no Han preference still has to choose a face for an ideograph.
    fn han(self) -> Script {
        match self {
            Script::Japanese => Script::Japanese,
            Script::TraditionalChinese => Script::TraditionalChinese,
            _ => Script::SimplifiedChinese,
        }
    }
}

/// The faces a character is drawn from, and the order they are tried in. Han
/// is cut three ways because the conventions disagree on one codepoint — 者
/// carries a dot in Chinese and none in Japanese — so the band carries it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Band {
    Latin,
    Han(Script),
    Kana,
    Hangul,
}

/// How many bands [`FontChain`] holds an order for.
const BANDS: usize = 6;

impl Band {
    /// Where this band's face order sits in [`FontChain::orders`]. Also the
    /// band's share of a glyph cache key: one face draws two bands at two
    /// different drops.
    pub fn slot(self) -> usize {
        match self {
            Band::Latin => 0,
            Band::Han(Script::TraditionalChinese) => 1,
            Band::Han(Script::Japanese) => 2,
            // Every other script sets Han in the default convention.
            Band::Han(_) => 3,
            Band::Kana => 4,
            Band::Hangul => 5,
        }
    }

    /// Whether this band's ink fills an em box rather than standing on the
    /// baseline — see [`FontChain::centring`].
    pub fn is_cjk(self) -> bool {
        !matches!(self, Band::Latin)
    }

    /// The characters whose ink stands for the band, first that the face has.
    fn probes(self) -> &'static [char] {
        match self {
            Band::Latin => &[],
            Band::Han(_) => &['中'],
            // code2000 draws kana and has no Han at all.
            Band::Kana => &['中', 'あ'],
            Band::Hangul => &['한'],
        }
    }
}

/// The band `ch` is drawn from, under a run set in `run`.
pub fn band_of(ch: char, run: Script) -> Band {
    if is_hangul(ch) {
        return Band::Hangul;
    }
    if is_kana(ch) {
        return Band::Kana;
    }
    if is_han(ch) {
        return Band::Han(run.han());
    }
    if is_wide(ch) {
        // Punctuation between ideographs comes off the same face they do.
        return match run {
            Script::Japanese => Band::Kana,
            Script::Korean => Band::Hangul,
            _ => Band::Han(run.han()),
        };
    }
    Band::Latin
}

/// The Han blocks a reading device sees: the unified repertoire, Extension A,
/// the compatibility ideographs and the SIP.
fn is_han(c: char) -> bool {
    matches!(c, '\u{3400}'..='\u{4DBF}'
        | '\u{4E00}'..='\u{9FFF}'
        | '\u{F900}'..='\u{FAFF}'
        | '\u{20000}'..='\u{3FFFF}')
}

/// Hiragana, katakana, and the halfwidth katakana a filename can carry.
fn is_kana(c: char) -> bool {
    matches!(c, '\u{3040}'..='\u{30FF}'
        | '\u{31F0}'..='\u{31FF}'
        | '\u{FF66}'..='\u{FF9F}')
}

/// Hangul syllables, and the jamo they decompose to.
fn is_hangul(c: char) -> bool {
    matches!(c, '\u{1100}'..='\u{11FF}'
        | '\u{3130}'..='\u{318F}'
        | '\u{A960}'..='\u{A97F}'
        | '\u{AC00}'..='\u{D7FF}')
}

/// CJK punctuation and the fullwidth forms: an em wide, and drawn by whatever
/// face the run's ideographs come from. Halfwidth katakana is [`is_kana`]'s.
fn is_wide(c: char) -> bool {
    matches!(c, '\u{3000}'..='\u{303F}' | '\u{FF00}'..='\u{FF65}' | '\u{FFA0}'..='\u{FFEF}')
}

/// One face the device might have, and the convention it sets.
pub struct Candidate {
    pub path: PathBuf,
    pub script: Script,
}

/// Directories the firmware keeps faces in. Scanned by [`discover`], after
/// `READINGLOG_FONTS` where that is set, colon-separated.
const FONT_DIRS: &[&str] = &[
    // The firmware set.
    "/usr/java/lib/fonts",
    "/usr/share/fonts",
    // The regional packs, which carry the faces the base image leaves out.
    "/var/local/font/mnt/zh-Hant_font/fonts",
    "/var/local/font/mnt/ja_font/fonts",
    // User-installed faces, ranked below the firmware set.
    "/mnt/us/fonts",
];

// Unscanned: `/chroot/usr/java/lib/fonts` mirrors the system set.

/// Latin families in preference order, matched case-insensitively against the
/// filename. `ember` is the device's own UI face and leads.
const PREFERRED: &[&str] = &["ember", "bookerly", "baskerville", "caecilia", "helvetica"];

/// The CJK families, by the convention each sets, ranked in the order written.
/// Amazon's `code2000` is **not** among them: its copy carries kana and Hangul
/// but not one Han ideograph, so it is a [`CATCH_ALL`], never a Han face.
const CJK_FAMILIES: &[(Script, &[&str])] = &[
    (
        Script::TraditionalChinese,
        &["stheititc", "stsongtc", "stkaititc", "styuantc"],
    ),
    (
        Script::Japanese,
        &["tbgothic", "tbmincho", "tsukumin", "tsukugo"],
    ),
    (
        Script::SimplifiedChinese,
        &["stheitimedium", "stheitibold", "stsongmedium", "stsongbold"],
    ),
    (Script::Korean, &["notosanskr", "notoserifkr"]),
];

/// Pan-Unicode faces, tried after every named family and before the unnamed
/// rest. They keep a character drawable when no proper face has it.
const CATCH_ALL: &[&str] = &["code2000", "mtchinesesurrogates"];

/// Distance from the plain upright, lower better. The tokens overlap on real
/// filenames, so the order they are tested in is load-bearing: italic, then
/// weight, then `serif` to demote the `AmazonEmberSerif_*` cuts.
fn weight_rank(lower: &str) -> usize {
    if lower.contains("italic") || lower.contains("oblique") {
        return 3;
    }
    if [
        "bold", "heavy", "black", "light", "thin", "cond", "medium", "serif", "poster",
    ]
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

/// Which convention a filename sets, and where it sits in that family's list.
fn cjk_family(lower: &str) -> Option<(Script, usize)> {
    CJK_FAMILIES.iter().find_map(|(script, tokens)| {
        tokens
            .iter()
            .position(|t| lower.contains(t))
            .map(|at| (*script, at))
    })
}

/// Rank a filename: lower sorts earlier. Tier first — the Latin families, then
/// the CJK families, then the pan-Unicode faces, then everything else — and
/// within a tier the family, then the weight.
fn rank(file_name: &str) -> (usize, usize, usize) {
    let lower = file_name.to_ascii_lowercase();
    let weight = weight_rank(&lower);
    if let Some(at) = PREFERRED.iter().position(|p| lower.contains(p)) {
        return (0, at, weight);
    }
    if let Some((_, at)) = cjk_family(&lower) {
        return (1, at, weight);
    }
    if let Some(at) = CATCH_ALL.iter().position(|p| lower.contains(p)) {
        return (2, at, weight);
    }
    (3, 0, weight)
}

/// Everything drawable on disk, best first, feeding [`FontChain::load`].
pub fn discover() -> Vec<Candidate> {
    let mut found: Vec<((usize, usize, usize), PathBuf, Script)> = Vec::new();
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
            let script = cjk_family(&name.to_ascii_lowercase()).map_or(Script::Unknown, |(s, _)| s);
            found.push((rank(name), path, script));
        }
    }
    found.sort_by(|a, b| (&a.0, &a.1).cmp(&(&b.0, &b.1)));
    found
        .into_iter()
        .map(|(_, path, script)| Candidate { path, script })
        .collect()
}

/// Faces to try for a band, best first: those setting `wanted`, then the
/// `also` conventions in the order given, then the chain as ranked so a
/// character some face has is never drawn as a box.
fn order_for(scripts: &[Script], wanted: Script, also: &[Script]) -> Vec<usize> {
    let mut order: Vec<usize> = Vec::with_capacity(scripts.len());
    for script in std::iter::once(wanted).chain(also.iter().copied()) {
        order.extend(
            scripts
                .iter()
                .enumerate()
                .filter(|(_, s)| **s == script)
                .map(|(i, _)| i),
        );
    }
    let tail: Vec<usize> = (0..scripts.len()).filter(|i| !order.contains(i)).collect();
    order.extend(tail);
    order
}

/// An ordered set of faces: the first usable candidate, read up front, plus
/// the rest of the chain waiting on disk.
pub struct FontChain {
    primary: FontVec,
    primary_path: PathBuf,
    rest: Vec<Face>,
    /// One face order per [`Band`], indexed by [`Band::slot`]. Built once:
    /// resolving a character walks the order for its band.
    orders: [Vec<usize>; BANDS],
    /// How far a band's ink drops to sit on the Latin cap's centre, in ems,
    /// per face — see [`FontChain::centring`].
    centres: HashMap<(usize, usize), f32>,
}

/// A fallback slot. [`FontChain::load`] drops a candidate it cannot read.
/// `path` outlives the read, for [`FontChain::paths`].
struct Face {
    path: PathBuf,
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
        let mut scripts = vec![first.script];
        let rest = present
            .map(|candidate| {
                scripts.push(candidate.script);
                Face {
                    path: candidate.path.clone(),
                    state: State::Pending,
                }
            })
            .collect();
        Ok(Self {
            primary,
            primary_path: first.path.clone(),
            rest,
            orders: band_orders(&scripts),
            centres: HashMap::new(),
        })
    }

    /// The chain on this device, primary first. Logged at startup.
    pub fn paths(&self) -> impl Iterator<Item = &Path> {
        std::iter::once(self.primary_path.as_path())
            .chain(self.rest.iter().map(|face| face.path.as_path()))
    }

    /// The face Latin is set in, which line metrics come from.
    pub fn primary(&self) -> &FontVec {
        &self.primary
    }

    /// Where in the chain `ch` is drawn from for `band`, or `None` when
    /// nothing has it. The index is part of the glyph cache's key: two faces
    /// rasterize the same codepoint differently.
    pub fn face_for(&mut self, band: Band, ch: char) -> Option<usize> {
        // The order is owned back for the walk: `ensure` reads faces, and the
        // orders are fixed for the life of the chain.
        let order = std::mem::take(&mut self.orders[band.slot()]);
        let face = order
            .iter()
            .copied()
            .find(|&face| self.ensure(face).is_some_and(|font| has_glyph(font, ch)));
        self.orders[band.slot()] = order;
        face
    }

    /// Face `index`, which [`FontChain::face_for`] has already read. `None`
    /// for a face that never loaded.
    pub fn font(&self, index: usize) -> Option<&FontVec> {
        if index == 0 {
            return Some(&self.primary);
        }
        match &self.rest.get(index - 1)?.state {
            State::Loaded(font) => Some(font),
            State::Pending | State::Absent => None,
        }
    }

    /// How far `face` drops `band`'s ink to sit on the centre of a Latin cap,
    /// in ems, positive downward. Nothing about where CJK ink lands is in the
    /// metrics, so it is outlined once per face. Latin is exempt.
    pub fn centring(&mut self, face: usize, band: Band) -> f32 {
        if !band.is_cjk() {
            return 0.0;
        }
        let key = (face, band.slot());
        if let Some(hit) = self.centres.get(&key) {
            return *hit;
        }
        // `px_bounds` is whole pixels; measuring large makes that a rounding
        // error in an answer every size scales from.
        let drop = self
            .ensure(face)
            .and_then(|font| {
                let probe = band
                    .probes()
                    .iter()
                    .copied()
                    .find(|&c| has_glyph(font, c))?;
                let glyph = font.glyph_id(probe).with_scale(scale_of(font, CENTRING_PX));
                let ink = font.outline_glyph(glyph)?.px_bounds();
                Some(-CJK_CENTRE - (ink.min.y + ink.max.y) / 2.0 / CENTRING_PX)
            })
            .unwrap_or(0.0);
        self.centres.insert(key, drop);
        drop
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

/// The face order for every band, over a chain whose faces set `scripts`.
fn band_orders(scripts: &[Script]) -> [Vec<usize>; BANDS] {
    use Script::{Japanese as Ja, Korean as Ko, SimplifiedChinese as Sc, TraditionalChinese as Tc};
    // A convention's own faces first, then the other Han conventions: a
    // regional form is wrong where a missing glyph is unreadable.
    let mut orders = [const { Vec::new() }; BANDS];
    orders[Band::Latin.slot()] = (0..scripts.len()).collect();
    orders[Band::Han(Tc).slot()] = order_for(scripts, Tc, &[Sc, Ja]);
    orders[Band::Han(Ja).slot()] = order_for(scripts, Ja, &[Tc, Sc]);
    orders[Band::Han(Sc).slot()] = order_for(scripts, Sc, &[Tc, Ja]);
    orders[Band::Kana.slot()] = order_for(scripts, Ja, &[Sc, Tc]);
    orders[Band::Hangul.slot()] = order_for(scripts, Ko, &[]);
    orders
}

/// A Latin cap, in ems: Amazon Ember draws `H` to 0.695. What a boxed
/// label centres on, Latin and CJK alike.
pub const CAP: f32 = 0.695;

/// Where CJK ink is centred above the baseline, in ems. Half a [`CAP`].
const CJK_CENTRE: f32 = CAP / 2.0;

/// The em [`FontChain::centring`] measures at. Large, and never drawn.
const CENTRING_PX: f32 = 512.0;

/// The [`PxScale`] that draws `font` with an em `px` pixels tall. ab_glyph
/// scales `hhea.ascender - hhea.descender` to `PxScale`, and that span is not
/// the em — it runs 1.000 to 1.480 across these faces — so it is divided out.
pub fn scale_of(font: &FontVec, px: f32) -> PxScale {
    let height = font.height_unscaled();
    let upem = font.units_per_em().unwrap_or(height);
    PxScale::from(px * height / upem)
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

/// Whether `font` can draw `ch` at all. Glyph 0 is `.notdef`, which is what a
/// face hands back for a character it doesn't have, and what it draws as a
/// box: a miss has to be read off the id, not off the outline.
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

    /// Every face on a Kindle Scribe's `/usr/java/lib/fonts`, verbatim, cut to
    /// the ones a title can reach. The full directory holds 122.
    const DEVICE: &[&str] = &[
        "Amazon-Ember-Bold.ttf",
        "Amazon-Ember-Heavy.ttf",
        "Amazon-Ember-Medium.ttf",
        "Amazon-Ember-Regular.ttf",
        "Amazon-Ember-RegularItalic.ttf",
        "AmazonEmberBold-Bold.ttf",
        "AmazonEmberBold-Regular.ttf",
        "AmazonEmberSerif_Rg.ttf",
        "AmazonEmberSerif_Poster.ttf",
        "Baskerville-Regular.ttf",
        "Bookerly-Regular.ttf",
        "Caecilia_LT_65_Medium.ttf",
        "Futura-Medium.ttf",
        "Helvetica_LT_65_Medium.ttf",
        "KindleBlackboxRegular.ttf",
        "MTChineseSurrogates.ttf",
        "NotoSansKR-Regular.otf",
        "NotoSerifKR-Medium.otf",
        "Palatino-Regular.ttf",
        "STHeitiBold.ttf",
        "STHeitiMedium.ttf",
        "STHeitiTC.ttf",
        "STHeitiTCBold.ttf",
        "STSongMedium.ttf",
        "STSongTC.ttf",
        "TBGothicBold_213.ttf",
        "TBGothicMed_213.ttf",
        "TBMinchoMedium_213.ttf",
        "code2000.ttf",
    ];

    /// [`DEVICE`] in the order [`discover`] would hand it over, with the
    /// convention each face sets.
    fn device_chain() -> (Vec<&'static str>, Vec<Script>) {
        let mut names = DEVICE.to_vec();
        names.sort_by_key(|n| (rank(n), *n));
        let scripts = names
            .iter()
            .map(|n| cjk_family(&n.to_ascii_lowercase()).map_or(Script::Unknown, |(s, _)| s))
            .collect();
        (names, scripts)
    }

    #[test]
    fn the_ui_font_wins_on_a_real_device() {
        let (names, _) = device_chain();
        assert_eq!(
            names[0], "Amazon-Ember-Regular.ttf",
            "the device's UI typeface, upright and regular, must draw the UI"
        );
        // The traps: a bold family whose cut is named "Regular", two weights
        // with no bold-ish token, and the serif cuts naming no weight at all.
        for trap in [
            "AmazonEmberBold-Regular.ttf",
            "Amazon-Ember-Heavy.ttf",
            "Amazon-Ember-Medium.ttf",
            "Amazon-Ember-RegularItalic.ttf",
            "AmazonEmberSerif_Rg.ttf",
            "AmazonEmberSerif_Poster.ttf",
        ] {
            assert!(
                rank("Amazon-Ember-Regular.ttf") < rank(trap),
                "{trap} should rank below the plain regular"
            );
        }
    }

    #[test]
    fn a_han_face_outranks_the_pan_unicode_catch_all() {
        // This firmware's `code2000` holds 24,713 glyphs and none of them is
        // Han, so it must never stand between a title and a Han face.
        for han in [
            "STHeitiMedium.ttf",
            "STSongMedium.ttf",
            "STHeitiTC.ttf",
            "TBGothicMed_213.ttf",
            "NotoSansKR-Regular.otf",
        ] {
            assert!(
                rank(han) < rank("code2000.ttf"),
                "{han} must outrank code2000"
            );
        }
        // And the catch-all still outranks the unnamed rest, so a character
        // no named family has is drawn rather than boxed.
        for unnamed in ["KindleBlackboxRegular.ttf", "Futura-Medium.ttf"] {
            assert!(rank("code2000.ttf") < rank(unnamed));
        }
    }

    #[test]
    fn each_cjk_face_is_filed_under_the_convention_it_sets() {
        let cases = [
            ("STHeitiMedium.ttf", Script::SimplifiedChinese),
            ("STHeitiBold.ttf", Script::SimplifiedChinese),
            ("STSongMedium.ttf", Script::SimplifiedChinese),
            // The TC cuts share a prefix with the SC ones and must not be
            // read as them.
            ("STHeitiTC.ttf", Script::TraditionalChinese),
            ("STHeitiTCBold.ttf", Script::TraditionalChinese),
            ("STSongTC.ttf", Script::TraditionalChinese),
            ("STSongTCBold.ttf", Script::TraditionalChinese),
            ("STKaitiTC.ttf", Script::TraditionalChinese),
            ("TBGothicMed_213.ttf", Script::Japanese),
            ("TBMinchoMedium_213.ttf", Script::Japanese),
            ("TsukuMinPr5-Medium.ttf", Script::Japanese),
            ("NotoSansKR-Regular.otf", Script::Korean),
            ("NotoSerifKR-Medium.otf", Script::Korean),
        ];
        for (name, want) in cases {
            let got = cjk_family(&name.to_ascii_lowercase()).map(|(s, _)| s);
            assert_eq!(got, Some(want), "{name}");
        }
        // Not CJK faces, and none of them may claim a convention.
        for name in [
            "code2000.ttf",
            "Amazon-Ember-Regular.ttf",
            "Bookerly-Regular.ttf",
        ] {
            assert_eq!(cjk_family(&name.to_ascii_lowercase()), None, "{name}");
        }
    }

    #[test]
    fn a_chinese_title_is_drawn_by_a_chinese_face() {
        // TBGothic is Japanese and covers most of the same codepoints, so a
        // Simplified band that ranked by filename alone would reach it first.
        let (names, scripts) = device_chain();
        let orders = band_orders(&scripts);
        let first = |band: Band| names[orders[band.slot()][0]];
        assert_eq!(
            first(Band::Han(Script::SimplifiedChinese)),
            "STHeitiMedium.ttf"
        );
        assert_eq!(
            first(Band::Han(Script::TraditionalChinese)),
            "STHeitiTC.ttf"
        );
        assert_eq!(first(Band::Han(Script::Japanese)), "TBGothicMed_213.ttf");
        assert_eq!(first(Band::Kana), "TBGothicMed_213.ttf");
        assert_eq!(first(Band::Hangul), "NotoSansKR-Regular.otf");
        // Latin never leaves the UI face for a CJK one.
        assert_eq!(first(Band::Latin), "Amazon-Ember-Regular.ttf");
    }

    #[test]
    fn every_band_can_still_reach_every_face() {
        // A band prefers its own convention but never fences the rest off:
        // a character only one odd face has must still be drawable.
        let (names, scripts) = device_chain();
        let orders = band_orders(&scripts);
        for band in [
            Band::Latin,
            Band::Han(Script::SimplifiedChinese),
            Band::Han(Script::TraditionalChinese),
            Band::Han(Script::Japanese),
            Band::Kana,
            Band::Hangul,
        ] {
            let order = &orders[band.slot()];
            assert_eq!(order.len(), names.len(), "{band:?} drops faces");
            let mut seen = order.clone();
            seen.sort_unstable();
            seen.dedup();
            assert_eq!(seen.len(), names.len(), "{band:?} repeats a face");
        }
    }

    #[test]
    fn a_character_is_drawn_from_its_own_script_not_the_run_s() {
        // The Latin inside a CJK title comes off the UI face, and the
        // ideographs off the Han face — one string, two bands.
        let run = Script::TraditionalChinese;
        assert_eq!(band_of('周', run), Band::Han(Script::TraditionalChinese));
        assert_eq!(band_of('A', run), Band::Latin);
        assert_eq!(band_of(' ', run), Band::Latin);
        // Fullwidth punctuation is an em wide and belongs to the ideographs.
        assert_eq!(band_of('（', run), Band::Han(Script::TraditionalChinese));
        assert_eq!(band_of('：', run), Band::Han(Script::TraditionalChinese));
        // Kana and Hangul answer for themselves whatever the run is set in.
        assert_eq!(band_of('ゆ', run), Band::Kana);
        assert_eq!(band_of('ン', run), Band::Kana);
        assert_eq!(band_of('한', run), Band::Hangul);
    }

    #[test]
    fn the_run_decides_which_convention_han_is_set_in() {
        // One codepoint, three faces: 者 carries a dot in Chinese and none in
        // Japanese, and the tag is what tells them apart.
        assert_eq!(band_of('者', Script::Japanese), Band::Han(Script::Japanese));
        assert_eq!(
            band_of('者', Script::TraditionalChinese),
            Band::Han(Script::TraditionalChinese)
        );
        assert_eq!(
            band_of('者', Script::SimplifiedChinese),
            Band::Han(Script::SimplifiedChinese)
        );
        // Korean and untagged text still have to set an ideograph in some
        // convention, and take the same default a bare `zh` does.
        for run in [Script::Korean, Script::Unknown] {
            assert_eq!(band_of('者', run), Band::Han(Script::SimplifiedChinese));
        }
    }

    #[test]
    fn an_untagged_title_is_read_off_its_own_characters() {
        // A catalog that names no language still must not set Japanese in a
        // Chinese face: kana settles it, and Hangul settles Korean.
        assert_eq!(
            Script::resolve(Script::Unknown, "ねむらない街の図鑑"),
            Script::Japanese
        );
        assert_eq!(
            Script::resolve(Script::Unknown, "채식주의자"),
            Script::Korean
        );
        // Han alone cannot be told apart, and takes the bare-`zh` default.
        assert_eq!(
            Script::resolve(Script::Unknown, "紅樓夢"),
            Script::SimplifiedChinese
        );
        assert_eq!(
            Script::resolve(Script::Unknown, "Interval"),
            Script::Unknown
        );
        // A tag that names a convention always wins over the characters.
        assert_eq!(
            Script::resolve(Script::TraditionalChinese, "ねむらない"),
            Script::TraditionalChinese
        );
    }

    #[test]
    fn language_tags_name_the_convention_they_are_set_in() {
        assert_eq!(Script::of_language("ja"), Script::Japanese);
        // A country code where a language belongs — real imported metadata.
        assert_eq!(Script::of_language("jp"), Script::Japanese);
        assert_eq!(Script::of_language("ko"), Script::Korean);
        assert_eq!(Script::of_language("zh-Hant"), Script::TraditionalChinese);
        assert_eq!(Script::of_language("zh_TW"), Script::TraditionalChinese);
        assert_eq!(Script::of_language("ZH-HK"), Script::TraditionalChinese);
        assert_eq!(Script::of_language("yue"), Script::TraditionalChinese);
        assert_eq!(Script::of_language("zh-Hans"), Script::SimplifiedChinese);
        assert_eq!(Script::of_language("zh-CN"), Script::SimplifiedChinese);
        // CLDR resolves bare `zh` to Simplified.
        assert_eq!(Script::of_language("zh"), Script::SimplifiedChinese);
        // The catalog's `p_languages_0`, in the shapes the framework writes.
        assert_eq!(Script::of_language("ja-JP"), Script::Japanese);
        assert_eq!(
            Script::of_language("zh-Hant-TW"),
            Script::TraditionalChinese
        );
        assert_eq!(Script::of_language("en"), Script::Unknown);
        assert_eq!(Script::of_language(""), Script::Unknown);
    }

    #[test]
    fn invisible_characters_never_reach_the_rasterizer() {
        // A banner joins its clauses with `\n`, which no face carries, and a
        // title can arrive carrying a stray BOM.
        for c in [
            '\n', '\r', '\t', '\u{0}', '\u{7F}', '\u{85}', '\u{FEFF}', '\u{00AD}',
        ] {
            assert!(is_invisible(c), "{c:?} would draw as a box");
        }
        for c in ['あ', 'A', '中', '한', '　'] {
            assert!(!is_invisible(c), "{c:?} is ink");
        }
    }

    /// The real faces, where `READINGLOG_FONTS` points at a device's own font
    /// directories. `None` where it is unset, which skips the assertions below:
    /// they are about a device's files, not about the ranking.
    fn device_chain_on_disk() -> Option<FontChain> {
        let dirs = std::env::var("READINGLOG_FONTS").ok()?;
        let candidates: Vec<Candidate> = dirs
            .split(':')
            .filter(|d| !d.is_empty())
            .flat_map(|dir| {
                let mut names: Vec<PathBuf> = std::fs::read_dir(dir)
                    .into_iter()
                    .flatten()
                    .flatten()
                    .map(|e| e.path())
                    .filter(|p| {
                        let ext = p
                            .extension()
                            .and_then(|e| e.to_str())
                            .unwrap_or_default()
                            .to_ascii_lowercase();
                        matches!(ext.as_str(), "ttf" | "otf" | "ttc")
                    })
                    .collect();
                names.sort_by_key(|p| {
                    let name = p.file_name().and_then(|n| n.to_str()).unwrap_or_default();
                    (rank(name), name.to_string())
                });
                names
            })
            .map(|path| {
                let lower = path
                    .file_name()
                    .and_then(|n| n.to_str())
                    .unwrap_or_default()
                    .to_ascii_lowercase();
                let script = cjk_family(&lower).map_or(Script::Unknown, |(s, _)| s);
                Candidate { path, script }
            })
            .collect();
        FontChain::load(&candidates).ok()
    }

    #[test]
    fn a_mixed_title_takes_each_run_off_its_own_face() {
        let Some(mut chain) = device_chain_on_disk() else {
            return;
        };
        let name = |chain: &FontChain, face: usize| {
            chain
                .paths()
                .nth(face)
                .and_then(|p| p.file_name())
                .and_then(|n| n.to_str())
                .unwrap_or_default()
                .to_string()
        };
        // A Traditional Chinese author with a Latin gloss: one string, and
        // each half comes off the face for its own script.
        let run = Script::TraditionalChinese;
        for (ch, want) in [('周', "STHeitiTC.ttf"), ('牧', "STHeitiTC.ttf")] {
            let face = chain.face_for(band_of(ch, run), ch).expect("no face");
            assert_eq!(name(&chain, face), want, "{ch}");
        }
        for ch in ['A', 'n', 'e', ' ', 'C', 'h', 'o', 'u'] {
            let face = chain.face_for(band_of(ch, run), ch).expect("no face");
            assert_eq!(
                name(&chain, face),
                "Amazon-Ember-Regular.ttf",
                "{ch} must stay on the UI face inside a CJK title"
            );
        }
        // The fullwidth brackets belong to the ideographs, not to the Latin.
        for ch in ['（', '）'] {
            let face = chain.face_for(band_of(ch, run), ch).expect("no face");
            assert_eq!(name(&chain, face), "STHeitiTC.ttf", "{ch}");
        }
    }

    #[test]
    fn each_convention_reaches_its_own_face_on_a_real_device() {
        let Some(mut chain) = device_chain_on_disk() else {
            return;
        };
        let name = |chain: &FontChain, face: usize| {
            chain
                .paths()
                .nth(face)
                .and_then(|p| p.file_name())
                .and_then(|n| n.to_str())
                .unwrap_or_default()
                .to_string()
        };
        // 者 is a character the conventions disagree over, and TBGothic — a
        // Japanese face — covers it for all three.
        for (run, want) in [
            (Script::SimplifiedChinese, "STHeitiMedium.ttf"),
            (Script::TraditionalChinese, "STHeitiTC.ttf"),
            (Script::Japanese, "TBGothicMed_213.ttf"),
        ] {
            let face = chain.face_for(band_of('者', run), '者').expect("no face");
            assert_eq!(name(&chain, face), want, "{run:?}");
        }
        // Kana and Hangul have faces of their own, and neither is code2000 —
        // whose copy on this device carries both, and no Han at all.
        let kana = chain.face_for(Band::Kana, 'ゆ').expect("no face");
        assert_eq!(name(&chain, kana), "TBGothicMed_213.ttf");
        let hangul = chain.face_for(Band::Hangul, '한').expect("no face");
        assert_eq!(name(&chain, hangul), "NotoSansKR-Regular.otf");
    }

    #[test]
    fn every_face_is_scaled_to_the_same_em() {
        let Some(mut chain) = device_chain_on_disk() else {
            return;
        };
        // The faces disagree on `hhea.ascender - hhea.descender`, which is
        // what a bare `PxScale` sets. Every em measures `px` regardless.
        const PX: f32 = 28.0;
        for (band, ch) in [
            (Band::Latin, 'H'),
            (Band::Han(Script::SimplifiedChinese), '中'),
            (Band::Han(Script::Japanese), '中'),
            (Band::Kana, 'あ'),
            (Band::Hangul, '한'),
        ] {
            let face = chain.face_for(band, ch).expect("no face");
            let font = chain.font(face).expect("unloaded");
            let upem = font.units_per_em().unwrap_or(font.height_unscaled());
            let em = upem * scale_of(font, PX).y / font.height_unscaled();
            assert!(
                (em - PX).abs() < 0.01,
                "{band:?} draws an em of {em}, not {PX}"
            );
        }
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
