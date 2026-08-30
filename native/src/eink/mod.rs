//! On-device display and input plumbing. [`fb`] draws into an X11 window over
//! `x11rb`; [`touch`] and [`buttons`] read raw evdev under `EVIOCGRAB`,
//! multiplexed by [`input`].

pub mod buttons;
pub mod fb;
pub mod input;
pub mod screenshot;
pub mod touch;
