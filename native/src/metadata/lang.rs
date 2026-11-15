//! [`normalize`]: a language tag as one canonical code, ISO 639-1 where one exists, `zh` keeping
//! its script.

/// `normalize`: a raw language tag as its canonical code; empty (or white space) in, empty out.
pub fn normalize(raw: &str) -> String {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return String::new();
    }
    // `parts`: BCP-47 subtags are '-'-separated; tolerate the '_' some sources use.
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

/// `to_iso639_1` folds a primary subtag to its ISO 639-1 two-letter form; two-letter codes pass
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

/// `chinese_with_script` resolves a `zh` tag to `zh-Hans`, `zh-Hant` or `zh`: by its script
/// subtag where it states one, else by its region, else `zh`.
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
        // `zh-Hant-CN`: the script outranks a conflicting region.
        assert_eq!(normalize("zh-Hant-CN"), "zh-Hant");
        // `zh`, `zho`, `chi`: generic Chinese, no script to keep.
        assert_eq!(normalize("zh"), "zh");
        assert_eq!(normalize("zho"), "zh");
        assert_eq!(normalize("chi"), "zh");
    }

    #[test]
    fn empty_and_unknown() {
        assert_eq!(normalize(""), "");
        assert_eq!(normalize("   "), "");
        // `tlh`: an unmapped three-letter code (Klingon) passes through.
        assert_eq!(normalize("tlh"), "tlh");
    }
}
