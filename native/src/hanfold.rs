//! [`fold`] takes every Traditional Han character in a line to its Simplified
//! form; [`folded`] takes one character, off the pairs in [`table::FOLD`].

mod table;

use std::borrow::Cow;

use table::FOLD;

/// The first and last characters [`FOLD`] holds.
const FIRST: char = FOLD[0].0;
const LAST: char = FOLD[FOLD.len() - 1].0;

/// The Simplified form [`FOLD`] pairs with `ch`, and `ch` where it holds none.
pub fn folded(ch: char) -> char {
    if ch < FIRST || ch > LAST {
        return ch;
    }
    match FOLD.binary_search_by_key(&ch, |(from, _)| *from) {
        Ok(at) => FOLD[at].1,
        Err(_) => ch,
    }
}

/// `text` with every character [`folded`], borrowed where none of them moves.
pub fn fold(text: &str) -> Cow<'_, str> {
    match text.chars().any(|ch| folded(ch) != ch) {
        false => Cow::Borrowed(text),
        true => Cow::Owned(text.chars().map(folded).collect()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_traditional_line_folds_and_nothing_else_is_touched() {
        assert_eq!(fold("國"), "国");
        // 時, 間, 記 and 録 are keys of [`FOLD`]; の is not.
        assert_eq!(fold("時間の記録"), "时间の记录");
        // Simplified, Latin and kana: borrowed, never copied.
        for said in ["", "the ninth winter", "时间", "ひらがな", "国"] {
            assert!(
                matches!(fold(said), Cow::Borrowed(_)),
                "{said:?} was copied"
            );
        }
    }

    #[test]
    fn the_table_is_sorted_and_folds_nothing_onto_itself() {
        let mut last = None;
        for (from, to) in FOLD {
            assert_ne!(from, to, "{from} folds onto itself");
            if let Some(last) = last {
                assert!(last < from, "{last} stands before {from}");
            }
            last = Some(from);
        }
        assert_eq!(folded('A'), 'A');
        assert_eq!(folded('楽'), '楽', "the shinjitai is left alone");
        assert_eq!(folded('樂'), '乐');
    }
}
