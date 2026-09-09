//! What a key the on-screen keyboard sends stands for.
//!
//! The keyboard writes a character's own codepoint as the keysym, which is
//! how its keymaps are written: the Return key's value is U+FF0D because that
//! number *is* `XK_Return`. So a bare scalar above Latin-1 is the character
//! itself, and the function-key block has to be read before it.

/// The keysyms the on-screen keyboard sends that are not characters. Nothing
/// else in the U+FF00 block is a value in any of the 29 keymaps.
const BACKSPACE: u32 = 0xFF08;
const TAB: u32 = 0xFF09;
const RETURN: u32 = 0xFF0D;
const ESCAPE: u32 = 0xFF1B;
const DELETE: u32 = 0xFFFF;
const KEYPAD_ENTER: u32 = 0xFF8D;

/// Where the standard form of a Unicode keysym opens, and the highest
/// codepoint it can name.
const UNICODE: u32 = 0x0100_0000;
const UNICODE_TOP: u32 = 0x0110_FFFF;

/// What one keysym means to a field being typed into.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Typed {
    /// A character to take on.
    Char(char),
    /// Take the last character off.
    Backspace,
    /// The query is finished, and the keyboard can go.
    Enter,
    /// Leave the field as it stands.
    Escape,
}

/// What `keysym` stands for, or nothing where it names no character and no
/// key a field answers — a modifier, an arrow, a function key.
pub fn of_keysym(keysym: u32) -> Option<Typed> {
    match keysym {
        BACKSPACE | DELETE => return Some(Typed::Backspace),
        RETURN | KEYPAD_ENTER => return Some(Typed::Enter),
        ESCAPE => return Some(Typed::Escape),
        // A tab is not a character a one-line field takes.
        TAB => return None,
        _ => {}
    }
    let scalar = match keysym {
        // Latin-1, which is its own codepoint, less the C0 and C1 blocks.
        0x20..=0x7E | 0xA0..=0xFF => keysym,
        // The standard form of any other Unicode keysym.
        UNICODE..=UNICODE_TOP => keysym - UNICODE,
        // The bare scalar the keyboard writes for a character off the map.
        // 0xFD00 up is the block X11 keeps for modifiers, arrows and function
        // keys — a bonded hardware keyboard sends those — and 0xE000 up is
        // private use, where the Japanese layouts put keys the IME eats.
        // Neither is a character to type.
        0x0100..=0xDFFF | 0xF900..=0xFCFF => keysym,
        _ => return None,
    };
    char::from_u32(scalar).map(Typed::Char)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn latin_is_its_own_codepoint() {
        assert_eq!(of_keysym(0x61), Some(Typed::Char('a')));
        assert_eq!(of_keysym(0x41), Some(Typed::Char('A')));
        assert_eq!(of_keysym(0x20), Some(Typed::Char(' ')));
        assert_eq!(of_keysym(0xE9), Some(Typed::Char('é')));
    }

    /// The keyboard writes a bare scalar; the standard form is what a hardware
    /// keyboard sends. Both name the same character.
    #[test]
    fn a_character_off_the_map_arrives_in_either_form() {
        assert_eq!(of_keysym(0x56F3), Some(Typed::Char('図')));
        assert_eq!(of_keysym(UNICODE + 0x56F3), Some(Typed::Char('図')));
        assert_eq!(of_keysym(0x3042), Some(Typed::Char('あ')));
        assert_eq!(of_keysym(0x0430), Some(Typed::Char('а')));
    }

    /// The keymaps' Return key carries U+FF0D, which is `XK_Return` itself.
    /// The function block is read before any character.
    #[test]
    fn the_function_block_is_read_first() {
        assert_eq!(of_keysym(0xFF0D), Some(Typed::Enter));
        assert_eq!(of_keysym(0xFF08), Some(Typed::Backspace));
        assert_eq!(of_keysym(0xFF1B), Some(Typed::Escape));
        assert_eq!(of_keysym(0xFF09), None);
    }

    /// A modifier, an arrow, a function key and a surrogate name nothing to
    /// type, and neither does a key the Japanese layouts keep to themselves.
    #[test]
    fn a_key_naming_no_character_names_nothing() {
        // Shift_L, Left, F1, Multi_key, NUL, the C0 escape.
        for keysym in [0xFFE1, 0xFF51, 0xFFBE, 0xFF20, 0xFD1E, 0, 0x1B] {
            assert_eq!(of_keysym(keysym), None, "{keysym:#x}");
        }
        assert_eq!(of_keysym(0xD800), None, "a surrogate is no character");
        assert_eq!(of_keysym(0xE001), None, "private use is no character");
    }
}
