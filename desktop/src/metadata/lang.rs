//! Harmonize the language tags a reader sets into one canonical code, so the
//! books in one language are counted together. ISO 639-1 where one exists; `zh`
//! keeps script.

/// Normalize a raw language tag to its canonical code. Empty (or whitespace) in
/// → empty out, so a book with no language stays blank rather than gaining one.
pub fn normalize(raw: &str) -> String {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return String::new();
    }
    // BCP-47 subtags are '-'-separated; tolerate the '_' some sources use.
    let parts: Vec<String> = trimmed
        .split(['-', '_'])
        .filter(|s| !s.is_empty())
        .map(|s| s.to_ascii_lowercase())
        .collect();
    let Some(first) = parts.first() else {
        return String::new();
    };

    let primary = to_iso639_1(first);
    if primary == "zh" {
        return chinese_with_script(&parts);
    }
    primary
}

/// Fold a primary subtag to its ISO 639-1 two-letter form. Two-letter codes pass
fn to_iso639_1(code: &str) -> String {
    if code.len() == 2 {
        return code.to_string();
    }
    let mapped = match code {
        "eng" => "en",
        "jpn" => "ja",
        "zho" | "chi" | "cmn" => "zh",
        "kor" => "ko",
        "fra" | "fre" => "fr",
        "deu" | "ger" => "de",
        "spa" => "es",
        "ita" => "it",
        "por" => "pt",
        "rus" => "ru",
        "nld" | "dut" => "nl",
        "ara" => "ar",
        "heb" => "he",
        "hin" => "hi",
        "tha" => "th",
        "vie" => "vi",
        "ind" => "id",
        "msa" | "may" => "ms",
        "pol" => "pl",
        "swe" => "sv",
        "dan" => "da",
        "nor" => "no",
        "fin" => "fi",
        "ell" | "gre" => "el",
        "ces" | "cze" => "cs",
        "tur" => "tr",
        "ukr" => "uk",
        "ron" | "rum" => "ro",
        "hun" => "hu",
        other => other,
    };
    mapped.to_string()
}

/// Resolve a `zh` tag to `zh-Hans` / `zh-Hant` / `zh`. An explicit script subtag
/// wins; otherwise the region picks the script; with neither it stays `zh`.
fn chinese_with_script(parts: &[String]) -> String {
    for p in &parts[1..] {
        match p.as_str() {
            "hans" => return "zh-Hans".to_string(),
            "hant" => return "zh-Hant".to_string(),
            _ => {}
        }
    }
    for p in &parts[1..] {
        match p.as_str() {
            "cn" | "sg" | "my" => return "zh-Hans".to_string(),
            "tw" | "hk" | "mo" => return "zh-Hant".to_string(),
            _ => {}
        }
    }
    "zh".to_string()
}

#[cfg(test)]
mod tests {
    use super::normalize;

    #[test]
    fn english_variants_collapse() {
        for raw in [
            "en", "en-US", "en-us", "en_US", "EN", "en-GB", "eng", "ENG", "  en  ",
        ] {
            assert_eq!(normalize(raw), "en", "{raw:?}");
        }
    }

    #[test]
    fn japanese_and_korean() {
        for raw in ["ja", "ja-JP", "jpn", "JA"] {
            assert_eq!(normalize(raw), "ja", "{raw:?}");
        }
        assert_eq!(normalize("ko-KR"), "ko");
        assert_eq!(normalize("kor"), "ko");
    }

    #[test]
    fn chinese_script_preserved() {
        for raw in [
            "zh-Hans",
            "zh-hans",
            "zh-CN",
            "zh_cn",
            "zh-SG",
            "zh-Hans-CN",
        ] {
            assert_eq!(normalize(raw), "zh-Hans", "{raw:?}");
        }
        for raw in [
            "zh-Hant",
            "zh-hant",
            "zh-TW",
            "zh-HK",
            "zh-MO",
            "zh-Hant-TW",
        ] {
            assert_eq!(normalize(raw), "zh-Hant", "{raw:?}");
        }
        // Script outranks a conflicting region.
        assert_eq!(normalize("zh-Hant-CN"), "zh-Hant");
        // Bare / generic Chinese has no script to keep.
        assert_eq!(normalize("zh"), "zh");
        assert_eq!(normalize("zho"), "zh");
        assert_eq!(normalize("chi"), "zh");
    }

    #[test]
    fn empty_and_unknown() {
        assert_eq!(normalize(""), "");
        assert_eq!(normalize("   "), "");
        // Unmapped three-letter code passes through (Klingon).
        assert_eq!(normalize("tlh"), "tlh");
    }
}
