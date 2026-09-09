//! The device's own on-screen keyboard. It is a standalone X client stacked
//! above every application window: raising it costs one property set, it
//! covers the foot of the screen without moving or resizing anything of ours,
//! and it goes away on another.

use std::process::Command;

/// The keyboard's own lipc service, and the two properties that raise and
/// dismiss it.
const SERVICE: &str = "com.lab126.keyboard";
const OPEN: &str = "open";
const CLOSE: &str = "close";

/// The name the keyboard addresses its client by. It sets the properties
/// carrying an IME's commits on this service, and takes it off the `open`
/// string with no validation: what we pass here is what it answers to.
const CLIENT: &str = "com.readinglog.stats";

/// Which of the keymap's layouts is drawn. Only `pad` and `web` are matched
/// against; anything else takes the plain alphabetic one.
const LAYOUT: &str = "abc";

/// The flags the client opens with: bit 0 runs the predictor and shows the
/// candidate bar, which is what the languages with an IME need. Bit 1 asks
/// for surrounding text and bit 2 makes backspace eat a whole word; a field
/// this short wants neither.
const FLAGS: u32 = 0x1;

/// Where the live layout is written when the keyboard language changes.
const KEYMAP: &str = "/var/local/system/current.keymap";

/// The field naming how much of the screen the keyboard takes: the keys and
/// the candidate bar over them together.
const FIELD: &str = "\"portrait_height\"";

/// What the keymaps state for the panels the device ships, screen height to
/// keyboard height. Every language carries the same figure for a panel.
const PANELS: [(i32, i32); 3] = [(2480, 808), (1696, 578), (1680, 578)];

/// How much of a `screen` px tall screen the keyboard covers, anchored to its
/// foot and drawn the full width.
///
/// The live layout states it; a panel this build has never seen falls back to
/// the table, and one absent from that to a third of the screen.
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

/// Raise the keyboard, in whatever language it was last left in. Answers
/// whether `lipc-set-prop` ran and exited clean.
pub fn open() -> bool {
    set(OPEN, &format!("{CLIENT}:{LAYOUT}:{FLAGS}"))
}

/// Dismiss it. The value is the bare client name: anything else and the
/// keyboard finds no match for it and stays up.
pub fn close() -> bool {
    set(CLOSE, CLIENT)
}

/// One `lipc-set-prop` on the keyboard's service.
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

/// The `portrait_height` a keymap states, as a positive number of pixels.
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

    /// The head of the Scribe's own `current.keymap`.
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

    /// A keymap naming no such field, or naming it as nothing, states nothing.
    #[test]
    fn a_keymap_without_the_field_states_nothing() {
        assert_eq!(of_keymap("{}"), None);
        assert_eq!(of_keymap(r#"{"portrait_height" : }"#), None);
        assert_eq!(of_keymap(r#"{"portrait_height" : 0}"#), None);
    }

    /// Every panel the device ships is in the table, and anything else takes
    /// a third of its screen.
    #[test]
    fn a_panel_off_the_table_takes_a_third() {
        assert_eq!(of_panel(2480), 808);
        assert_eq!(of_panel(1696), 578);
        assert_eq!(of_panel(1680), 578);
        assert_eq!(of_panel(1448), 1448 / 3);
    }
}
