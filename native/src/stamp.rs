//! A small fixed hash, for the gate values the record carries.
//!
//! `DefaultHasher` is SipHash with a documented promise that its output may
//! change between Rust releases. A value written into `sessions.tsv` has to
//! mean the same thing to the build that reads it back, so the gates use
//! FNV-1a instead: eight lines, fixed forever, and quite strong enough for a
//! test that only ever asks *did this move*.
//!
//! A collision costs a pass that was going to be skipped, or one skipped that
//! would have found nothing new. Neither loses a row.

const OFFSET: u64 = 0xcbf2_9ce4_8422_2325;
const PRIME: u64 = 0x0000_0100_0000_01b3;

/// An FNV-1a hash under construction.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Stamp(u64);

impl Default for Stamp {
    fn default() -> Self {
        Self(OFFSET)
    }
}

impl Stamp {
    /// Take `bytes`, and a separator after them so that two fields cannot run
    /// together into a third that would hash the same.
    pub fn take(&mut self, bytes: &[u8]) {
        for byte in bytes {
            self.0 = (self.0 ^ u64::from(*byte)).wrapping_mul(PRIME);
        }
        self.0 = (self.0 ^ 0xff).wrapping_mul(PRIME);
    }

    /// Take a number, little-endian.
    pub fn num(&mut self, n: i64) {
        self.take(&n.to_le_bytes());
    }

    /// Take a string.
    pub fn text(&mut self, text: &str) {
        self.take(text.as_bytes());
    }

    /// What it came to.
    pub fn done(self) -> u64 {
        self.0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn of(fields: &[&str]) -> u64 {
        let mut stamp = Stamp::default();
        for field in fields {
            stamp.text(field);
        }
        stamp.done()
    }

    #[test]
    fn nothing_taken_is_the_offset_basis() {
        assert_eq!(Stamp::default().done(), OFFSET);
    }

    #[test]
    fn two_fields_never_run_together_into_one() {
        assert_ne!(of(&["ab", "c"]), of(&["a", "bc"]));
        assert_ne!(of(&["a", ""]), of(&["a"]));
    }

    #[test]
    fn the_same_fields_come_to_the_same_number_every_time() {
        assert_eq!(of(&["a", "b"]), of(&["a", "b"]));
        assert_ne!(of(&["a", "b"]), of(&["b", "a"]));
        let mut mixed = Stamp::default();
        mixed.num(1);
        mixed.text("x");
        let held = mixed.done();
        let mut again = Stamp::default();
        again.num(1);
        again.text("x");
        assert_eq!(again.done(), held);
    }
}
