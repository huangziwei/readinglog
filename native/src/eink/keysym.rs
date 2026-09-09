//! [`of_keysym`] reads one X keysym as a [`Typed`].

/// Keysyms [`of_keysym`] names, all in the 0xFF00 block.
const BACKSPACE: u32 = 0xFF08;
const TAB: u32 = 0xFF09;
const RETURN: u32 = 0xFF0D;
const ESCAPE: u32 = 0xFF1B;
const DELETE: u32 = 0xFFFF;
const KEYPAD_ENTER: u32 = 0xFF8D;

/// `UNICODE + cp` is the standard form of a Unicode keysym; `UNICODE_TOP`
/// caps `cp`.
const UNICODE: u32 = 0x0100_0000;
const UNICODE_TOP: u32 = 0x0110_FFFF;

/// What [`of_keysym`] reads a keysym as.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Typed {
    /// The character the keysym names.
    Char(char),
    /// `BACKSPACE` or `DELETE`.
    Backspace,
    /// `RETURN` or `KEYPAD_ENTER`.
    Enter,
    /// `ESCAPE`.
    Escape,
}

/// What `keysym` stands for; `None` for a modifier, an arrow or a function
/// key.
pub fn of_keysym(keysym: u32) -> Option<Typed> {
    match keysym {
        BACKSPACE | DELETE => return Some(Typed::Backspace),
        RETURN | KEYPAD_ENTER => return Some(Typed::Enter),
        ESCAPE => return Some(Typed::Escape),
        // `TAB` names no character.
        TAB => return None,
        _ => {}
    }
    let scalar = match keysym {
        // Latin-1 keysyms are their own codepoint, less C0 and C1.
        0x20..=0x7E | 0xA0..=0xFF => keysym,
        // `keysym - UNICODE` is the codepoint.
        UNICODE..=UNICODE_TOP => keysym - UNICODE,
        // A bare scalar. 0xE000..=0xF8FF is private use and 0xFD00 up holds
        // modifiers, arrows and function keys.
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

    #[test]
    fn a_character_off_the_map_arrives_in_either_form() {
        assert_eq!(of_keysym(0x56F3), Some(Typed::Char('図')));
        assert_eq!(of_keysym(UNICODE + 0x56F3), Some(Typed::Char('図')));
        assert_eq!(of_keysym(0x3042), Some(Typed::Char('あ')));
        assert_eq!(of_keysym(0x0430), Some(Typed::Char('а')));
    }

    #[test]
    fn the_function_block_is_read_first() {
        assert_eq!(of_keysym(0xFF0D), Some(Typed::Enter));
        assert_eq!(of_keysym(0xFF08), Some(Typed::Backspace));
        assert_eq!(of_keysym(0xFF1B), Some(Typed::Escape));
        assert_eq!(of_keysym(0xFF09), None);
    }

    #[test]
    fn a_key_naming_no_character_names_nothing() {
        // Shift_L, Left, F1, Multi_key, NoSymbol, ESC.
        for keysym in [0xFFE1, 0xFF51, 0xFFBE, 0xFF20, 0xFD1E, 0, 0x1B] {
            assert_eq!(of_keysym(keysym), None, "{keysym:#x}");
        }
        assert_eq!(of_keysym(0xD800), None, "a surrogate is no character");
        assert_eq!(of_keysym(0xE001), None, "private use is no character");
    }
}
