//! Display and touch orientation, 180° apart. [`Orientation::detect`] reads
//! it; `crate::eink::touch` transforms raw evdev coords against it.

use std::process::Command;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Orientation {
    /// Native portrait, page-turn bezel on the right. Coords pass through.
    Up,
    /// Rotated 180°, page-turn bezel on the left. Both axes mirror.
    Down,
}

impl Orientation {
    /// `lipc-get-prop com.lab126.winmgr orientation`, through
    /// [`Orientation::of_prop`]. An error reads as [`Orientation::Up`].
    pub fn detect() -> Self {
        let Ok(out) = Command::new("lipc-get-prop")
            .args(["com.lab126.winmgr", "orientation"])
            .output()
        else {
            return Self::Up;
        };
        if !out.status.success() {
            return Self::Up;
        }
        Self::of_prop(&String::from_utf8_lossy(&out.stdout))
    }

    /// The `com.lab126.winmgr orientation` value: "U", "D", "L" or "R". "D" is
    /// [`Orientation::Down`] and every other value [`Orientation::Up`].
    pub fn of_prop(said: &str) -> Self {
        match said.trim() {
            "D" => Self::Down,
            _ => Self::Up,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::Orientation;

    /// "D" alone is [`Orientation::Down`], through the trailing newline
    /// `lipc-get-prop` prints.
    #[test]
    fn only_d_is_down() {
        assert_eq!(Orientation::of_prop("D\n"), Orientation::Down);
        for said in ["U\n", "U", "", "  ", "d", "Down"] {
            assert_eq!(Orientation::of_prop(said), Orientation::Up, "{said:?}");
        }
    }

    /// "L" and "R" read as [`Orientation::Up`].
    #[test]
    fn landscape_reads_as_up() {
        assert_eq!(Orientation::of_prop("L"), Orientation::Up);
        assert_eq!(Orientation::of_prop("R"), Orientation::Up);
    }
}
