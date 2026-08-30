//! Pure text-wrap utility, at the crate root: `ui::text::TextRenderer` pulls
//! in `Framebuffer`, which `cargo test --lib` cannot build on the host.

/// `text` wrapped to `max_width` per line, measured by `measure`.
/// Whitespace splits a Latin line; a token wider than `max_width` — a CJK
/// title, a URL — breaks at char boundaries.
///
/// Every candidate is measured whole. A face chain picks its face for the
/// string it is given, and one that covers `token` alone need not cover the
/// line `token` lands in: summing token widths reads narrower than the line
/// draws.
pub fn wrap_to_width<F>(text: &str, max_width: u32, mut measure: F) -> Vec<String>
where
    F: FnMut(&str) -> u32,
{
    let mut lines: Vec<String> = Vec::new();
    let mut current = String::new();

    for token in text.split_whitespace() {
        let joined = match current.is_empty() {
            true => token.to_string(),
            false => format!("{current} {token}"),
        };
        if measure(&joined) <= max_width {
            current = joined;
            continue;
        }
        if !current.is_empty() {
            lines.push(std::mem::take(&mut current));
        }
        if measure(token) <= max_width {
            current = token.to_string();
            continue;
        }
        // A token past `max_width` on its own breaks at char boundaries.
        for ch in token.chars() {
            let mut wider = current.clone();
            wider.push(ch);
            if measure(&wider) > max_width && !current.is_empty() {
                lines.push(std::mem::take(&mut current));
                current.push(ch);
            } else {
                current = wider;
            }
        }
    }
    if !current.is_empty() {
        lines.push(current);
    }
    lines
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
            // Trim until `"<last>…"` fits `max_width`.
            let mut candidate = format!("{last}…");
            while !last.is_empty() && measure(&candidate) > max_width {
                last.pop();
                candidate = format!("{last}…");
            }
            *last = candidate;
        }
    }
    lines
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A fixed-width face: every char 10px.
    fn fixed(s: &str) -> u32 {
        s.chars().count() as u32 * 10
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
