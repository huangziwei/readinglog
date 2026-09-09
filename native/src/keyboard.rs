//! The device's on-screen keyboard. [`open`] and [`close`] set one property
//! each on [`SERVICE`]; [`height`] states how much of the screen it takes.

use std::process::Command;

/// The lipc service [`open`] and [`close`] set a property on.
const SERVICE: &str = "com.lab126.keyboard";
const OPEN: &str = "open";
const CLOSE: &str = "close";

/// The lipc service name [`open`] hands over, and [`close`] matches against.
const CLIENT: &str = "com.readinglog.stats";

/// The layout [`open`] asks for. `pad` and `web` are the two matched against;
/// any other value draws the alphabetic layout.
const LAYOUT: &str = "abc";

/// Bit 0 runs the predictor and its candidate bar, bit 1 asks for surrounding
/// text, bit 2 makes backspace take a word.
const FLAGS: u32 = 0x1;

/// The live layout, rewritten on a keyboard language change.
const KEYMAP: &str = "/var/local/system/current.keymap";

/// The [`KEYMAP`] field naming the keys and the candidate bar together.
const FIELD: &str = "\"portrait_height\"";

/// Screen height to keyboard height, as the keymaps state it. One figure per
/// panel, the same for all 29 languages.
const PANELS: [(i32, i32); 3] = [(2480, 808), (1696, 578), (1680, 578)];

/// Raises the keyboard, in the language it holds. Answers whether
/// `lipc-set-prop` exited clean.
pub fn open() -> bool {
    set(OPEN, &format!("{CLIENT}:{LAYOUT}:{FLAGS}"))
}

/// Dismisses it, by the bare [`CLIENT`] name.
pub fn close() -> bool {
    set(CLOSE, CLIENT)
}

/// One `lipc-set-prop` on [`SERVICE`].
fn set(prop: &str, value: &str) -> bool {
    match Command::new("lipc-set-prop")
        .args([SERVICE, prop, value])
        .status()
    {
        Ok(status) if status.success() => true,
        Ok(status) => {
            eprintln!("keyboard: lipc-set-prop {prop} {status}");
            false
        }
        Err(err) => {
            eprintln!("keyboard: lipc-set-prop would not run: {err}");
            false
        }
    }
}

/// How much of a `screen` px tall screen the keyboard covers, anchored to its
/// foot and drawn the full width.
///
/// [`KEYMAP`] first, then [`PANELS`], then a third of `screen`.
pub fn height(screen: i32) -> i32 {
    if let Some(said) = std::fs::read_to_string(KEYMAP).ok().and_then(|said| {
        let head: String = said.chars().take(2048).collect();
        of_keymap(&head)
    }) {
        return said;
    }
    of_panel(screen)
}

/// What [`PANELS`] states for a screen this tall, or a third of it.
fn of_panel(screen: i32) -> i32 {
    PANELS
        .iter()
        .find(|(panel, _)| *panel == screen)
        .map(|(_, height)| *height)
        .unwrap_or(screen / 3)
}

/// The [`FIELD`] value in `said`, as a positive number of pixels.
fn of_keymap(said: &str) -> Option<i32> {
    let (_, rest) = said.split_once(FIELD)?;
    let (_, rest) = rest.split_once(':')?;
    let digits: String = rest
        .trim_start()
        .chars()
        .take_while(char::is_ascii_digit)
        .collect();
    digits.parse().ok().filter(|height| *height > 0)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The head of a [`KEYMAP`].
    const SCRIBE: &str = r#"{
    "keyboard_language" : "en-US",
    "candidate_height" : 100,
    "keyboard_height" : 708,
    "portrait_height" : 808,
    "landscape_height" : 808,
"#;

    #[test]
    fn the_live_layout_states_the_height() {
        assert_eq!(of_keymap(SCRIBE), Some(808));
    }

    #[test]
    fn a_keymap_without_the_field_states_nothing() {
        assert_eq!(of_keymap("{}"), None);
        assert_eq!(of_keymap(r#"{"portrait_height" : }"#), None);
        assert_eq!(of_keymap(r#"{"portrait_height" : 0}"#), None);
    }

    #[test]
    fn a_panel_off_the_table_takes_a_third() {
        assert_eq!(of_panel(2480), 808);
        assert_eq!(of_panel(1696), 578);
        assert_eq!(of_panel(1680), 578);
        assert_eq!(of_panel(1448), 1448 / 3);
    }
}
