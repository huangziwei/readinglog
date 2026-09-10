//! evdev page-button reader: `gpio-keys`, a separate device from the
//! touchscreen, emitting `KEY_PAGEUP` (104) and `KEY_PAGEDOWN` (109). Matched
//! on exact `Name`: a grab on a `KEY_POWER` device takes power-off with it.

use std::fs::{File, OpenOptions};
use std::io::Read;
use std::os::fd::{AsRawFd, RawFd};
use std::path::PathBuf;

use anyhow::{Context, Result};

use crate::orientation::Orientation;

const EV_KEY: u16 = 0x01;
const KEY_PAGEUP: u16 = 104;
const KEY_PAGEDOWN: u16 = 109;
const EVENT_BYTES: usize = 16;

// _IOW('E', 0x90, int). Call sites cast with `as _` for `libc::ioctl`.
const EVIOCGRAB: libc::c_int = 0x40044590;

/// Which bezel button fired: `KEY_PAGEUP` (top) → `Next`, `KEY_PAGEDOWN`
/// (bottom) → `Prev`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PageButton {
    Prev,
    Next,
}

impl PageButton {
    /// `1` forward, `-1` back.
    pub fn step(self) -> i64 {
        match self {
            Self::Next => 1,
            Self::Prev => -1,
        }
    }
}

pub struct Buttons {
    /// The node this reader took, for the log's header block.
    node: String,
    file: File,
    /// Whether `EVIOCGRAB` is held.
    grabbed: bool,
    /// Whether [`Buttons::open`]'s `EVIOCGRAB` took.
    exclusive: bool,
    /// [`Buttons::set_covered`]'s state: no grab, no [`Buttons::read_one`].
    covered: bool,
    /// Orientation. `Down` swaps `Prev` and `Next`.
    orientation: Orientation,
}

impl Buttons {
    /// Opens and grabs the page-button device. `Ok(None)` where no `gpio-keys`
    /// device exists, leaving touch as the whole of the input.
    /// The bezel reader, or `None` where the model has no page buttons — which
    /// is most of them, and not a fault. [`describe`] states which for the log.
    pub fn open() -> Result<Option<Self>> {
        let Some(path) = find_button_device()? else {
            return Ok(None);
        };
        let file = OpenOptions::new()
            .read(true)
            .open(&path)
            .with_context(|| format!("open {}", path.display()))?;
        let grabbed = unsafe { libc::ioctl(file.as_raw_fd(), EVIOCGRAB as _, 1) } == 0;
        Ok(Some(Self {
            node: path.display().to_string(),
            file,
            grabbed,
            exclusive: grabbed,
            covered: false,
            orientation: Orientation::Up,
        }))
    }

    /// Raw fd for `poll(2)` multiplexing alongside the touchscreen — see
    /// [`crate::eink::input`].
    pub fn raw_fd(&self) -> RawFd {
        self.file.as_raw_fd()
    }

    /// Sets `orientation`. A 180° flip swaps prev/next.
    pub fn set_orientation(&mut self, orientation: Orientation) {
        self.orientation = orientation;
    }

    /// Drops `EVIOCGRAB` and sets `covered` while another window covers this
    /// app's; takes the grab back when that window goes.
    pub fn set_covered(&mut self, covered: bool) {
        if covered == self.covered {
            return;
        }
        self.covered = covered;
        let want = i32::from(!covered);
        let ok = unsafe { libc::ioctl(self.file.as_raw_fd(), EVIOCGRAB as _, want) } == 0;
        self.grabbed = ok && !covered;
        eprintln!("buttons: covered={covered} grabbed={}", self.grabbed);
    }

    /// Retakes `EVIOCGRAB` where `exclusive` holds and `grabbed` does not.
    pub fn retake(&mut self) {
        if self.grabbed || self.covered || !self.exclusive {
            return;
        }
        self.grabbed = unsafe { libc::ioctl(self.file.as_raw_fd(), EVIOCGRAB as _, 1) } == 0;
        if self.grabbed {
            eprintln!("buttons: EVIOCGRAB retaken");
        }
    }

    /// One event record, the caller having polled first. `Some` on a mapped
    /// page key's press (`value==1`) alone; a release, autorepeat, `SYN` or
    /// unmapped key answers `None`.
    pub fn read_one(&mut self) -> Result<Option<PageButton>> {
        let mut buf = [0u8; EVENT_BYTES];
        self.file
            .read_exact(&mut buf)
            .context("read /dev/input/eventN (buttons)")?;
        let type_ = u16::from_ne_bytes([buf[8], buf[9]]);
        let code = u16::from_ne_bytes([buf[10], buf[11]]);
        let value = i32::from_ne_bytes([buf[12], buf[13], buf[14], buf[15]]);
        // Covered: the record is read and dropped.
        if self.covered {
            return Ok(None);
        }
        if type_ == EV_KEY && value == 1 {
            let btn = match code {
                // KOA2: `KEY_PAGEUP` is the top button, `KEY_PAGEDOWN` the bottom.
                KEY_PAGEUP => Some(PageButton::Next),
                KEY_PAGEDOWN => Some(PageButton::Prev),
                _ => None,
            };
            // `Orientation::Down` swaps the sides `Prev` and `Next` sit on.
            return Ok(match (btn, self.orientation) {
                (Some(PageButton::Next), Orientation::Down) => Some(PageButton::Prev),
                (Some(PageButton::Prev), Orientation::Down) => Some(PageButton::Next),
                (other, _) => other,
            });
        }
        Ok(None)
    }
}

impl Drop for Buttons {
    fn drop(&mut self) {
        if self.grabbed {
            unsafe {
                libc::ioctl(self.file.as_raw_fd(), EVIOCGRAB as _, 0);
            }
        }
    }
}

/// The bezel page-button device at exact `Name="gpio-keys"` in
/// `/proc/bus/input/devices`, as an `/dev/input/eventN` path. `Ok(None)` where
/// it is absent. Strict on the name: a `KEY_POWER` device must never match.
fn find_button_device() -> Result<Option<PathBuf>> {
    let raw = std::fs::read_to_string("/proc/bus/input/devices")
        .context("read /proc/bus/input/devices")?;
    for block in raw.split("\n\n") {
        let is_buttons = block
            .lines()
            .any(|l| l.starts_with("N: Name=") && l.contains("\"gpio-keys\""));
        if !is_buttons {
            continue;
        }
        for line in block.lines() {
            let Some(rest) = line.strip_prefix("H: Handlers=") else {
                continue;
            };
            if let Some(ev) = rest.split_whitespace().find(|w| w.starts_with("event")) {
                return Ok(Some(PathBuf::from(format!("/dev/input/{ev}"))));
            }
        }
    }
    Ok(None)
}

/// What the bezel resolved to, for the log's header block. A model with no
/// page buttons is the common case and says so plainly: it is a fact about the
/// device, not something the reader can act on.
pub fn describe(held: Option<&Buttons>) -> String {
    match held {
        None => "buttons=none".to_string(),
        Some(buttons) => format!(
            "buttons={} grab={}",
            buttons.node,
            match buttons.grabbed {
                true => "ok",
                false => "refused",
            },
        ),
    }
}

#[cfg(test)]
mod tests {
    use super::{PageButton, describe};

    /// Most models have no page buttons. That is a fact about the device, not
    /// a fault the reader can act on, so it is a word in the header block and
    /// never a line in the body.
    #[test]
    fn a_model_with_no_page_buttons_says_so_as_a_fact() {
        assert_eq!(describe(None), "buttons=none");
    }

    /// `Next` steps forward and `Prev` back.
    #[test]
    fn a_bezel_press_steps_both_ways() {
        assert_eq!(PageButton::Next.step(), 1);
        assert_eq!(PageButton::Prev.step(), -1);
    }
}
