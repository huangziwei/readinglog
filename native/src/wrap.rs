//! Pure text-wrap utility, at the crate root: `ui::text::TextRenderer` pulls
//! in `Framebuffer`, which `cargo test --lib` cannot build on the host.
//! [`can_break_between`] holds the punctuation rules a CJK line is set by.

/// What a character does to the break opportunities around it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Class {
    /// Breaks after, never before.
    Space,
    /// Opening punctuation. A line may not end on one.
    Open,
    /// Closing punctuation. A line may not begin with one.
    Close,
    /// Ideographs, kana and fullwidth forms — breakable on both sides.
    Ideograph,
    /// Latin letters, digits and everything else: breakable only at spaces.
    Other,
}

/// The class `c` breaks by.
pub fn classify(c: char) -> Class {
    match c {
        ' ' | '\t' | '\u{3000}' => Class::Space,

        // Opening brackets and quotes, CJK and ASCII.
        '「' | '『' | '（' | '【' | '〈' | '《' | '〔' | '［' | '｛' | '‘' | '“' | '(' | '['
        | '{' => Class::Open,

        // Closing punctuation: brackets, quotes, and the stops that must not
        // start a line.
        '」' | '』' | '）' | '】' | '〉' | '》' | '〕' | '］' | '｝' | '’' | '”' | ')' | ']'
        | '}' | '。' | '、' | '，' | '．' | '；' | '：' | '？' | '！' | '·' | '…' | '—' | ','
        | '.' | ';' | ':' | '?' | '!' => Class::Close,

        _ if is_ideographic(c) => Class::Ideograph,
        _ => Class::Other,
    }
}

/// Whether a line may begin with `c`: the two refusals
/// [`can_break_between`] makes, as a property of one character.
fn may_open_a_line(c: char) -> bool {
    !matches!(classify(c), Class::Space | Class::Close)
}

/// Scripts that break between characters, not between words.
fn is_ideographic(c: char) -> bool {
    matches!(c as u32,
        0x2E80..=0x2FDF   // radicals and Kangxi
        | 0x3040..=0x30FF // hiragana, katakana
        | 0x3400..=0x4DBF // CJK extension A
        | 0x4E00..=0x9FFF // CJK unified ideographs
        | 0xF900..=0xFAFF // compatibility ideographs
        | 0xFF00..=0xFF60 // fullwidth forms
        | 0x20000..=0x3FFFF // extensions B and beyond
    )
}

/// Whether a line may break between `a` and the `b` that follows it.
pub fn can_break_between(a: char, b: char) -> bool {
    use Class::*;
    match (classify(a), classify(b)) {
        // A space never starts a line; it hangs off the end of the previous one.
        (_, Space) => false,
        (Space, _) => true,
        // 「 must not end a line even though what follows it is breakable, and
        // 。 must not start one.
        (Open, _) => false,
        (_, Close) => false,
        (Ideograph, _) | (_, Ideograph) => true,
        _ => false,
    }
}

/// Whether a quarter em — 四分アキ — belongs between `a` and the `b` that
/// follows it: a Han or kana character against Latin letters or digits, in
/// either order. `ui::text::TextRenderer` gives it a width.
pub fn aki(a: char, b: char) -> bool {
    let han = |c| classify(c) == Class::Ideograph;
    let latin =
        |c: char| classify(c) == Class::Other && c.is_alphanumeric() && !crate::font::is_hangul(c);
    han(a) && latin(b) || latin(a) && han(b)
}

/// `text` wrapped to `max_width` per line, measured by `measure`. A line ends
/// at the last break [`can_break_between`] allows it, `\n` breaks one whatever
/// its width, and every candidate is measured whole.
pub fn wrap_to_width<F>(text: &str, max_width: u32, mut measure: F) -> Vec<String>
where
    F: FnMut(&str) -> u32,
{
    let chars: Vec<char> = text.chars().collect();
    let mut lines: Vec<String> = Vec::new();
    let mut start = 0usize;
    // The last index within this line a break is allowed at.
    let mut opportunity: Option<usize> = None;

    let mut i = 0usize;
    while i < chars.len() {
        let c = chars[i];
        if c == '\n' {
            lines.push(line_of(&chars[start..i]));
            (start, opportunity) = (i + 1, None);
            i += 1;
            continue;
        }
        if i > start && measure(&line_of(&chars[start..=i])) > max_width {
            match opportunity.filter(|&b| b > start) {
                // Push-out: the tail from the break moves down with `c`.
                Some(brk) => {
                    lines.push(line_of(&chars[start..brk]));
                    (start, opportunity) = (brk, None);
                    continue;
                }
                // A run this long that no break divides splits before `c`.
                None if may_open_a_line(c) => {
                    lines.push(line_of(&chars[start..i]));
                    (start, opportunity) = (i, None);
                    continue;
                }
                // A mark with no break behind it hangs past `max_width`.
                None => {}
            }
        }
        if i + 1 < chars.len() && can_break_between(c, chars[i + 1]) {
            opportunity = Some(i + 1);
        }
        i += 1;
    }

    let last = line_of(&chars[start..]);
    if !last.is_empty() {
        lines.push(last);
    }
    lines
}

/// `chars` as one line, less the space a break after it leaves at the end.
fn line_of(chars: &[char]) -> String {
    let line: String = chars.iter().collect();
    match line.trim_end() {
        trimmed if trimmed.len() == line.len() => line,
        trimmed => trimmed.to_string(),
    }
}

/// [`wrap_to_width`] clamped to `max_lines`, with `…` on the last kept line
/// where content was dropped. Trailing chars trim until `"<line>…"` measures
/// within `max_width`, down to a bare `…`.
pub fn wrap_and_clamp<F>(
    text: &str,
    max_width: u32,
    max_lines: usize,
    mut measure: F,
) -> Vec<String>
where
    F: FnMut(&str) -> u32,
{
    let mut lines = wrap_to_width(text, max_width, &mut measure);
    if lines.len() > max_lines {
        lines.truncate(max_lines);
        if let Some(last) = lines.last_mut() {
            mark_more(last, max_width, measure);
        }
    }
    lines
}

/// The mark a line carries where the text runs on past it.
pub const MORE: &str = "…";

/// Marks `last` as a line the text runs on past, trimming its own trailing
/// characters until `"<last>…"` measures within `max_width`, down to a bare
/// [`MORE`].
pub fn mark_more<F>(last: &mut String, max_width: u32, mut measure: F)
where
    F: FnMut(&str) -> u32,
{
    let mut candidate = format!("{last}{MORE}");
    while !last.is_empty() && measure(&candidate) > max_width {
        last.pop();
        candidate = format!("{last}{MORE}");
    }
    *last = candidate;
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A fixed-width face: every char 10px.
    fn fixed(s: &str) -> u32 {
        s.chars().count() as u32 * 10
    }

    /// Latin one unit a character, ideographs and CJK marks two — the
    /// proportion a real CJK face sets.
    fn cjk(s: &str) -> u32 {
        s.chars()
            .map(|c| {
                match is_ideographic(c)
                    || matches!(classify(c), Class::Open | Class::Close) && c as u32 > 0x2000
                {
                    true => 2,
                    false => 1,
                }
            })
            .sum()
    }

    /// Every line but the first opens with something allowed there.
    fn opens_legally(lines: &[String]) -> bool {
        lines
            .iter()
            .skip(1)
            .all(|l| l.chars().next().is_none_or(may_open_a_line))
    }

    #[test]
    fn a_line_never_begins_with_closing_punctuation() {
        // A break before 。 puts it at the head of line two.
        let lines = wrap_to_width("你好世界。再见", 4, cjk);
        assert_eq!(lines, ["你好", "世", "界。", "再见"]);
        assert!(opens_legally(&lines), "{lines:?}");
    }

    #[test]
    fn a_line_never_ends_with_opening_punctuation() {
        let lines = wrap_to_width("他说「你好世界」", 4, cjk);
        assert!(lines.iter().all(|l| !l.ends_with('「')), "{lines:?}");
        assert!(opens_legally(&lines), "{lines:?}");
    }

    #[test]
    fn a_stranded_mark_hangs_rather_than_opening_a_line() {
        // 「 refuses a break after it and 。 one before: between them the line
        // offers none, and 。 sits past the measure.
        let lines = wrap_to_width("你好「あ。再见", 4, cjk);
        assert_eq!(lines, ["你好", "「あ。", "再见"]);
        assert!(opens_legally(&lines), "{lines:?}");

        // A run of marks wider than a line of its own.
        let lines = wrap_to_width("你好世界。」再见", 4, cjk);
        assert_eq!(lines, ["你好", "世", "界。」", "再见"]);
        assert!(opens_legally(&lines), "{lines:?}");

        // Latin: a word chopped mid-letter offers no legal break either, and
        // the stop after it takes a line of its own.
        let lines = wrap_to_width("supercalifragilistic.", 5, cjk);
        assert_eq!(lines, ["super", "calif", "ragil", "istic."]);
        assert!(opens_legally(&lines), "{lines:?}");
    }

    #[test]
    fn a_line_never_begins_with_a_space_either() {
        assert_eq!(wrap_to_width("abc def", 3, cjk), ["abc", "def"]);
        assert_eq!(
            wrap_to_width("supercalifragilistic word", 5, cjk),
            ["super", "calif", "ragil", "istic", "word"]
        );
    }

    #[test]
    fn newlines_are_hard_breaks() {
        // Room for both on one line, and the newline breaks it anyway.
        assert_eq!(wrap_to_width("ab\ncd", 100, fixed), ["ab", "cd"]);
        assert_eq!(
            wrap_to_width("日記は記録だ。\n続きは過去の日記。", 200, cjk),
            ["日記は記録だ。", "続きは過去の日記。"]
        );
    }

    #[test]
    fn blank_lines_survive() {
        assert_eq!(wrap_to_width("a\n\nb", 100, fixed), ["a", "", "b"]);
    }

    #[test]
    fn mixed_scripts_break_at_the_script_boundary() {
        assert_eq!(wrap_to_width("hello 世界", 6, cjk), ["hello", "世界"]);
    }

    #[test]
    fn a_trailing_space_does_not_force_an_early_wrap() {
        // The space hangs past the right edge; "b" stays where it fits.
        assert_eq!(wrap_to_width("ab cd", 3, cjk), ["ab", "cd"]);
    }

    #[test]
    fn every_character_survives_the_wrap() {
        let text = "他说「你好，世界」then hello world 再见。";
        let lines = wrap_to_width(text, 7, cjk);
        let want: String = text.chars().filter(|c| !c.is_whitespace()).collect();
        let got: String = lines
            .concat()
            .chars()
            .filter(|c| !c.is_whitespace())
            .collect();
        assert_eq!(got, want, "{lines:?}");
        assert!(opens_legally(&lines), "{lines:?}");
    }

    #[test]
    fn classification_of_the_characters_the_rules_turn_on() {
        assert_eq!(classify('。'), Class::Close);
        assert_eq!(classify('、'), Class::Close);
        assert_eq!(classify('「'), Class::Open);
        assert_eq!(classify('世'), Class::Ideograph);
        assert_eq!(classify('あ'), Class::Ideograph);
        assert_eq!(classify('a'), Class::Other);
        assert_eq!(classify(' '), Class::Space);
        assert!(!can_break_between('世', '。'));
        assert!(!can_break_between('「', '你'));
        assert!(can_break_between('你', '好'));
        assert!(!can_break_between('h', 'i'));
    }

    #[test]
    fn a_quarter_em_goes_between_han_and_latin_and_nowhere_else() {
        assert!(aki('は', 'R'));
        assert!(aki('第', '3'));
        assert!(aki('t', '世'));
        assert!(aki('7', '月'));
        assert!(!aki('世', '界'), "two Han characters");
        assert!(!aki('h', 'i'), "two Latin letters");
        assert!(!aki('*', '世'), "a marker is not a word");
        assert!(!aki('世', '*'));
        assert!(!aki('世', '。'), "a stop carries its own sidebearing");
        assert!(!aki('」', 'a'));
        assert!(!aki('世', ' '), "a space is already a space");
        assert!(!aki('漢', '한'), "Hangul is CJK, not 欧文");
        assert!(!aki('글', '字'));
    }

    #[test]
    fn wraps_latin_at_word_boundaries() {
        // 10 chars per line. "hello world" measures 110px.
        let lines = wrap_to_width("hello world", 100, fixed);
        assert_eq!(lines, vec!["hello".to_string(), "world".to_string()]);
    }

    #[test]
    fn fits_single_line_when_under_max() {
        let lines = wrap_to_width("short", 100, fixed);
        assert_eq!(lines, vec!["short".to_string()]);
    }

    #[test]
    fn wraps_cjk_at_char_boundaries() {
        // 5 chars per line, one whitespace-free token: the char-level path.
        let lines = wrap_to_width("あいうえおかきくけこ", 50, fixed);
        assert_eq!(
            lines,
            vec!["あいうえお".to_string(), "かきくけこ".to_string()],
        );
    }

    #[test]
    fn empty_text_returns_no_lines() {
        let lines = wrap_to_width("", 100, fixed);
        assert!(lines.is_empty());
    }

    #[test]
    fn long_word_breaks_at_char_when_too_wide() {
        // 3 chars per line over 20 chars: 6 full chunks and a partial.
        let lines = wrap_to_width("supercalifragilistic", 30, fixed);
        assert_eq!(lines.len(), 7);
        assert_eq!(lines[0], "sup");
        assert_eq!(lines.last().unwrap(), "ic");
    }

    #[test]
    fn no_line_exceeds_max_in_mixed_text() {
        // Every line within `max` across a mixed-script input.
        let lines = wrap_to_width("a bb cccc ddddd", 30, fixed);
        assert!(lines.iter().all(|l| l.chars().count() <= 3));
    }

    #[test]
    fn clamp_keeps_all_lines_when_within_max() {
        // 2 wrapped lines under a `max_lines` of 3: no ellipsis.
        let lines = wrap_and_clamp("hello world", 100, 3, fixed);
        assert_eq!(lines, vec!["hello".to_string(), "world".to_string()]);
    }

    #[test]
    fn clamp_truncates_and_ellipsizes_last_line() {
        // 4 wrapped lines clamped to 2. `"bb…"` measures 30px.
        let lines = wrap_and_clamp("aaa bbb ccc ddd", 30, 2, fixed);
        assert_eq!(lines, vec!["aaa".to_string(), "bb…".to_string()]);
    }

    #[test]
    fn clamp_to_one_line_ellipsizes() {
        let lines = wrap_and_clamp("aaa bbb ccc", 30, 1, fixed);
        assert_eq!(lines, vec!["aa…".to_string()]);
    }

    /// 10px a char, and 16px a char on any string holding a wide char.
    fn widens(s: &str) -> u32 {
        let per = match s.chars().any(|c| c == '至') {
            true => 16,
            false => 10,
        };
        s.chars().count() as u32 * per
    }

    #[test]
    fn a_line_is_measured_whole_and_not_as_a_sum_of_its_tokens() {
        // "ab" and "至" each fit 40px alone (20 and 16). Together the face
        // widens both: "ab 至" measures 4 × 16 = 64.
        assert_eq!(widens("ab"), 20);
        assert_eq!(widens("至"), 16);
        assert_eq!(widens("ab 至"), 64);

        let lines = wrap_to_width("ab 至", 40, widens);
        assert_eq!(lines, vec!["ab".to_string(), "至".to_string()]);
        assert!(lines.iter().all(|l| widens(l) <= 40));
    }
}
