//! Each language a book can be set in, named in the five interface languages
//! and in its own: [`own`] and [`name`] by the code `lang::normalize` writes.

use readinglog_native::lang::Lang;

/// A language: its code, its own name, then its name in English, German,
/// Simplified Chinese, Traditional Chinese and Japanese.
type Named = (&'static str, &'static str, [&'static str; 5]);

/// Every language named here, in the order a list offers them.
pub const LANGUAGES: [Named; 31] = [
    (
        "en",
        "English",
        ["English", "Englisch", "英语", "英語", "英語"],
    ),
    (
        "ja",
        "日本語",
        ["Japanese", "Japanisch", "日语", "日語", "日本語"],
    ),
    (
        "zh-Hans",
        "简体中文",
        [
            "Chinese, Simplified",
            "Chinesisch (vereinfacht)",
            "简体中文",
            "簡體中文",
            "中国語（簡体字）",
        ],
    ),
    (
        "zh-Hant",
        "繁體中文",
        [
            "Chinese, Traditional",
            "Chinesisch (traditionell)",
            "繁体中文",
            "繁體中文",
            "中国語（繁体字）",
        ],
    ),
    (
        "zh",
        "中文",
        ["Chinese", "Chinesisch", "中文", "中文", "中国語"],
    ),
    (
        "ko",
        "한국어",
        ["Korean", "Koreanisch", "韩语", "韓語", "韓国語"],
    ),
    (
        "de",
        "Deutsch",
        ["German", "Deutsch", "德语", "德語", "ドイツ語"],
    ),
    (
        "fr",
        "Français",
        ["French", "Französisch", "法语", "法語", "フランス語"],
    ),
    (
        "es",
        "Español",
        ["Spanish", "Spanisch", "西班牙语", "西班牙語", "スペイン語"],
    ),
    (
        "it",
        "Italiano",
        [
            "Italian",
            "Italienisch",
            "意大利语",
            "義大利語",
            "イタリア語",
        ],
    ),
    (
        "pt",
        "Português",
        [
            "Portuguese",
            "Portugiesisch",
            "葡萄牙语",
            "葡萄牙語",
            "ポルトガル語",
        ],
    ),
    (
        "ru",
        "Русский",
        ["Russian", "Russisch", "俄语", "俄語", "ロシア語"],
    ),
    (
        "nl",
        "Nederlands",
        ["Dutch", "Niederländisch", "荷兰语", "荷蘭語", "オランダ語"],
    ),
    (
        "ar",
        "العربية",
        ["Arabic", "Arabisch", "阿拉伯语", "阿拉伯語", "アラビア語"],
    ),
    (
        "he",
        "עברית",
        ["Hebrew", "Hebräisch", "希伯来语", "希伯來語", "ヘブライ語"],
    ),
    (
        "hi",
        "हिन्दी",
        ["Hindi", "Hindi", "印地语", "印地語", "ヒンディー語"],
    ),
    (
        "th",
        "ไทย",
        ["Thai", "Thailändisch", "泰语", "泰語", "タイ語"],
    ),
    (
        "vi",
        "Tiếng Việt",
        [
            "Vietnamese",
            "Vietnamesisch",
            "越南语",
            "越南語",
            "ベトナム語",
        ],
    ),
    (
        "id",
        "Bahasa Indonesia",
        [
            "Indonesian",
            "Indonesisch",
            "印度尼西亚语",
            "印尼語",
            "インドネシア語",
        ],
    ),
    (
        "ms",
        "Bahasa Melayu",
        ["Malay", "Malaiisch", "马来语", "馬來語", "マレー語"],
    ),
    (
        "pl",
        "Polski",
        ["Polish", "Polnisch", "波兰语", "波蘭語", "ポーランド語"],
    ),
    (
        "sv",
        "Svenska",
        [
            "Swedish",
            "Schwedisch",
            "瑞典语",
            "瑞典語",
            "スウェーデン語",
        ],
    ),
    (
        "da",
        "Dansk",
        ["Danish", "Dänisch", "丹麦语", "丹麥語", "デンマーク語"],
    ),
    (
        "no",
        "Norsk",
        [
            "Norwegian",
            "Norwegisch",
            "挪威语",
            "挪威語",
            "ノルウェー語",
        ],
    ),
    (
        "fi",
        "Suomi",
        ["Finnish", "Finnisch", "芬兰语", "芬蘭語", "フィンランド語"],
    ),
    (
        "el",
        "Ελληνικά",
        ["Greek", "Griechisch", "希腊语", "希臘語", "ギリシャ語"],
    ),
    (
        "cs",
        "Čeština",
        ["Czech", "Tschechisch", "捷克语", "捷克語", "チェコ語"],
    ),
    (
        "tr",
        "Türkçe",
        ["Turkish", "Türkisch", "土耳其语", "土耳其語", "トルコ語"],
    ),
    (
        "uk",
        "Українська",
        [
            "Ukrainian",
            "Ukrainisch",
            "乌克兰语",
            "烏克蘭語",
            "ウクライナ語",
        ],
    ),
    (
        "ro",
        "Română",
        [
            "Romanian",
            "Rumänisch",
            "罗马尼亚语",
            "羅馬尼亞語",
            "ルーマニア語",
        ],
    ),
    (
        "hu",
        "Magyar",
        [
            "Hungarian",
            "Ungarisch",
            "匈牙利语",
            "匈牙利語",
            "ハンガリー語",
        ],
    ),
];

/// The place [`Named`]'s names give `lang`.
fn column(lang: Lang) -> usize {
    match lang {
        Lang::English => 0,
        Lang::German => 1,
        Lang::SimplifiedChinese => 2,
        Lang::TraditionalChinese => 3,
        Lang::Japanese => 4,
    }
}

/// The language `code` names, in its own words; `None` for a code not listed.
pub fn own(code: &str) -> Option<&'static str> {
    LANGUAGES
        .iter()
        .find(|(listed, ..)| *listed == code)
        .map(|(_, own, _)| *own)
}

/// The language `code` names, in the interface language `lang`; `None` for a
/// code not listed.
pub fn name(code: &str, lang: Lang) -> Option<&'static str> {
    LANGUAGES
        .iter()
        .find(|(listed, ..)| *listed == code)
        .map(|(_, _, names)| names[column(lang)])
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_code_is_one_a_reader_sets_and_every_name_is_given() {
        for (code, own, names) in LANGUAGES {
            assert_eq!(super::super::lang::normalize(code), code, "{code}");
            assert!(!own.is_empty(), "{code}");
            assert!(names.iter().all(|n| !n.is_empty()), "{code}");
        }
        let mut codes: Vec<&str> = LANGUAGES.iter().map(|(code, ..)| *code).collect();
        codes.sort_unstable();
        codes.dedup();
        assert_eq!(codes.len(), LANGUAGES.len(), "each code once");
    }

    #[test]
    fn a_language_is_named_in_the_interface_language_and_its_own() {
        assert_eq!(own("ja"), Some("日本語"));
        assert_eq!(name("ja", Lang::English), Some("Japanese"));
        assert_eq!(name("de", Lang::Japanese), Some("ドイツ語"));
        assert_eq!(name("zh-Hant", Lang::SimplifiedChinese), Some("繁体中文"));
        assert_eq!(name("tlh", Lang::English), None);
    }
}
