//! `poll(2)` over the touchscreen and the bezel page-button device at once,
//! surfacing one [`InputEvent`]. The ready fd drains without blocking on the
//! other; `Touch::next_event` answers `None` short of a boundary and re-polls.

use std::os::fd::RawFd;
use std::time::Instant;

use anyhow::{Context, Result};

use super::buttons::{Buttons, PageButton};
use super::touch::{Touch, TouchEvent};
use crate::orientation::Orientation;

/// How long [`Input::next`] blocks before surfacing a `Tick`, bounding how
/// quickly a device rotation reaches the main loop. Fires on an idle poll:
/// real input returns first.
const TICK_MS: libc::c_int = 500;

/// A unified input event from either device.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InputEvent {
    Touch(TouchEvent),
    Page(PageButton),
    /// `poll` timed out with no input. The main loop re-checks orientation
    /// here: the X server rotates the display, raw evdev coords stay fixed.
    Tick,
}

pub struct Input {
    touch: Touch,
    /// `None` when no page-button device was found/openable — the picker runs
    /// touch-only and `poll` watches just the touchscreen.
    buttons: Option<Buttons>,
}

impl Input {
    pub fn new(touch: Touch, buttons: Option<Buttons>) -> Self {
        Self { touch, buttons }
    }

    /// Re-orient both devices after a detected rotation (the display is rotated
    /// by the X server; raw evdev coords/buttons are panel-fixed and need this).
    pub fn set_orientation(&mut self, orientation: Orientation) {
        self.touch.set_orientation(orientation);
        if let Some(buttons) = self.buttons.as_mut() {
            buttons.set_orientation(orientation);
        }
    }

    /// Block until the next event from either device (see
    /// [`Self::next_deadline`]); the everyday call, with only the idle
    /// [`TICK_MS`] wake and no arm deadline.
    pub fn event(&mut self) -> Result<InputEvent> {
        self.next_deadline(None)
    }

    /// [`Self::event`] with an [`InputEvent::Tick`] at `deadline`, past a busy
    /// touch fd: the timeout is the remaining time to the absolute `deadline`,
    /// recomputed each iteration. `None` gives a plain [`TICK_MS`] idle tick.
    pub fn next_deadline(&mut self, deadline: Option<Instant>) -> Result<InputEvent> {
        let touch_fd: RawFd = self.touch.raw_fd();
        loop {
            // At or past `deadline`, through move-jitter that kept `poll` busy.
            if let Some(d) = deadline
                && Instant::now() >= d
            {
                return Ok(InputEvent::Tick);
            }
            let button_fd: RawFd = self.buttons.as_ref().map(|b| b.raw_fd()).unwrap_or(-1);
            let mut fds = [
                libc::pollfd {
                    fd: touch_fd,
                    events: libc::POLLIN,
                    revents: 0,
                },
                libc::pollfd {
                    fd: button_fd,
                    events: libc::POLLIN,
                    revents: 0,
                },
            ];
            let nfds: libc::nfds_t = if self.buttons.is_some() { 2 } else { 1 };

            // Remaining time to `deadline`, floored at 1ms against a sub-ms
            // spin, else [`TICK_MS`]. `poll` wakes early on fd readiness.
            let timeout = match deadline {
                Some(d) => (d
                    .saturating_duration_since(Instant::now())
                    .as_millis()
                    .min(i32::MAX as u128) as libc::c_int)
                    .max(1),
                None => TICK_MS,
            };
            let rc = unsafe { libc::poll(fds.as_mut_ptr(), nfds, timeout) };
            if rc < 0 {
                let err = std::io::Error::last_os_error();
                if err.kind() == std::io::ErrorKind::Interrupted {
                    continue; // EINTR — re-arm the poll.
                }
                return Err(err).context("poll(touch, buttons)");
            }
            if rc == 0 {
                return Ok(InputEvent::Tick); // deadline reached, or idle timeout.
            }
            // `deadline` passed while `poll` blocked, with an event in the same
            // wake: the arm wins and the event surfaces next call. A finger
            // lifting on the threshold returns `Tick`, never `Up`.
            if let Some(d) = deadline
                && Instant::now() >= d
            {
                return Ok(InputEvent::Tick);
            }

            // Buttons first. `read_one` answers `None` on a release, autorepeat,
            // SYN or unmapped key, which re-polls.
            if let Some(buttons) = self.buttons.as_mut()
                && fds[1].revents & libc::POLLIN != 0
            {
                if let Some(page) = buttons.read_one()? {
                    return Ok(InputEvent::Page(page));
                }
                continue;
            }

            if fds[0].revents & libc::POLLIN != 0 {
                // `next_event` answers `None` short of a Down/Up boundary — a
                // move-only or partial packet — and re-polls. Touch is opened
                // `O_NONBLOCK` for this.
                if let Some(ev) = self.touch.next_event()? {
                    return Ok(InputEvent::Touch(ev));
                }
                continue;
            }

            // Spurious wake with no POLLIN — poll again.
        }
    }
}
