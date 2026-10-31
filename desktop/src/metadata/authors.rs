//! [`parse_input`], [`split_display`] and [`join_display`]: an author line as names, and back.

/// `NAME_SUFFIXES`: what follows the comma of a name never flipped, `Davis, Jr.`,
/// compared lowercased with any trailing `.` stripped.
const NAME_SUFFIXES: &[&str] = &[
    "jr", "sr", "ii", "iii", "iv", "v", "phd", "md", "do", "esq", "dds", "jd",
];

/// Whether `c` is CJK, kana or Hangul: a name holding one is never flipped.
fn is_cjk(c: char) -> bool {
    matches!(c as u32,
        0x3040..=0x30FF |   // Hiragana + Katakana
        0x3400..=0x4DBF |   // CJK Extension A
        0x4E00..=0x9FFF |   // CJK Unified Ideographs
        0xF900..=0xFAFF |   // CJK Compatibility Ideographs
        0xFF00..=0xFFEF |   // Halfwidth/Fullwidth forms
        0xAC00..=0xD7AF |   // Hangul syllables
        0x20000..=0x2FA1F   // CJK Extension B+ / supplement
    )
}

/// `name` as `Given Surname` where it reads `Surname, Given` (one ASCII comma,
/// no CJK, no suffix after it); else `name`, its spaces collapsed.
fn normalize_display(name: &str) -> String {
    let collapsed = name.split_whitespace().collect::<Vec<_>>().join(" ");
    if collapsed.matches(',').count() != 1 || collapsed.chars().any(is_cjk) {
        return collapsed;
    }
    let (surname, given) = collapsed.split_once(',').unwrap();
    let (surname, given) = (surname.trim(), given.trim());
    if surname.is_empty() || given.is_empty() {
        return collapsed;
    }
    let tail = given.trim_end_matches('.').to_ascii_lowercase();
    if NAME_SUFFIXES.contains(&tail.as_str()) {
        return collapsed;
    }
    format!("{given} {surname}")
}

/// Each of `parts` through [`normalize_display`], the empty ones dropped.
fn canonicalize<'a>(parts: impl Iterator<Item = &'a str>) -> Vec<String> {
    parts
        .map(normalize_display)
        .filter(|s| !s.is_empty())
        .collect()
}

/// `s`, as typed, as names: split at `&` and `、`, each through [`canonicalize`].
pub fn parse_input(s: &str) -> Vec<String> {
    canonicalize(s.split(['&', '、']))
}

/// `author`, a stored line, as names: split at `&` and `、`, each trimmed, none flipped.
pub fn split_display(author: &str) -> Vec<String> {
    author
        .split(['&', '、'])
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_string)
        .collect()
}

/// `authors` as one line: joined by `、` where several all hold CJK, else by ` & `.
pub fn join_display(authors: &[String]) -> String {
    if authors.len() > 1 && authors.iter().all(|a| a.chars().any(is_cjk)) {
        authors.join("、")
    } else {
        authors.join(" & ")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn flips_western_surname_first() {
        assert_eq!(normalize_display("Doe, John"), "John Doe");
        assert_eq!(normalize_display("Roe, Jane"), "Jane Roe");
        assert_eq!(normalize_display("Smith, Anna"), "Anna Smith");
        assert_eq!(normalize_display("Quill, A.B.C."), "A.B.C. Quill");
        assert_eq!(normalize_display("Van Dorn, Mary K."), "Mary K. Van Dorn");
    }

    #[test]
    fn leaves_natural_order_untouched() {
        assert_eq!(normalize_display("John Doe"), "John Doe");
        assert_eq!(normalize_display("A. B. C. Quill"), "A. B. C. Quill");
    }

    #[test]
    fn never_flips_cjk_or_suffixes() {
        assert_eq!(normalize_display("山田太郎"), "山田太郎");
        // `Yamada, Taro` holds no CJK: flipped.
        assert_eq!(normalize_display("Yamada, Taro"), "Taro Yamada");
        assert_eq!(normalize_display("Davis, Jr."), "Davis, Jr.");
        assert_eq!(normalize_display("King, III"), "King, III");
        // `Davis, Sam, Jr.` holds two commas: not flipped.
        assert_eq!(normalize_display("Davis, Sam, Jr."), "Davis, Sam, Jr.");
    }

    #[test]
    fn parse_input_uses_ampersand_not_comma() {
        assert_eq!(parse_input("Quill, A.B.C."), vec!["A.B.C. Quill"]);
        assert_eq!(
            parse_input("John Doe & Jane Roe"),
            vec!["John Doe", "Jane Roe"]
        );
        assert_eq!(
            parse_input("山田太郎、佐藤花子"),
            vec!["山田太郎", "佐藤花子"]
        );
        assert!(parse_input("   ").is_empty());
    }

    #[test]
    fn join_display_picks_separator_by_script() {
        assert_eq!(join_display(&["John Doe".into()]), "John Doe");
        assert_eq!(
            join_display(&["John Doe".into(), "Jane Roe".into()]),
            "John Doe & Jane Roe"
        );
        assert_eq!(
            join_display(&["山田太郎".into(), "佐藤花子".into()]),
            "山田太郎、佐藤花子"
        );
        let stored = join_display(&["John Doe".into(), "Jane Roe".into()]);
        assert_eq!(split_display(&stored), vec!["John Doe", "Jane Roe"]);
    }
}
