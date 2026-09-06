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

// _IOW('E', 0x90, int). Call sites cast with `as _`: `libc::ioctl`'s request
// arg is `c_int` on armv7 Linux and `c_ulong` on the host, and the value fits
// both.
const EVIOCGRAB: libc::c_int = 0x40044590;

/// Which bezel button fired. The KOA2 maps `KEY_PAGEUP` (top) → `Next` and
/// `KEY_PAGEDOWN` (bottom) → `Prev`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PageButton {
    Prev,
    Next,
}

pub struct Buttons {
    file: File,
    /// Whether `EVIOCGRAB` succeeded.
    grabbed: bool,
    /// Orientation. `Down` swaps `Prev` and `Next`.
    orientation: Orientation,
}

impl Buttons {
    /// Opens and grabs the page-button device. `Ok(None)` where no `gpio-keys`
    /// device exists, leaving touch as the whole of the input.
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
            file,
            grabbed,
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
        if type_ == EV_KEY && value == 1 {
            let btn = match code {
                // KOA2, hardware-confirmed: the top button emits KEY_PAGEUP
                // and the bottom KEY_PAGEDOWN. Top pages forward, inverting
                // the keycodes' literal names.
                KEY_PAGEUP => Some(PageButton::Next),
                KEY_PAGEDOWN => Some(PageButton::Prev),
                _ => None,
            };
            // On a 180° flip the physical buttons swap sides: prev/next swap
            // with them.
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
