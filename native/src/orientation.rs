//! Display and touch orientation. The KOA2 framework rotates the screen 180°
//! on which side the page-turn bezel sits. [`Orientation::detect`] reads that;
//! `crate::eink::touch` transforms raw evdev coords against it.

use std::process::Command;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Orientation {
    /// Native portrait, page-turn bezel on the right. Coords pass through.
    Up,
    /// Rotated 180°, page-turn bezel on the left. Both axes mirror.
    Down,
}

impl Orientation {
    /// `lipc-get-prop com.lab126.winmgr orientation`, which prints "U", "D",
    /// "L" or "R". An error or an unrecognized value reads as
    /// [`Orientation::Up`].
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
        match String::from_utf8_lossy(&out.stdout).trim() {
            "D" => Self::Down,
            _ => Self::Up,
        }
    }
}
