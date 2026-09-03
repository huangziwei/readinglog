//! The device-independent half of Reading Log: the log parser, the device
//! catalog, the session store and the statistics over them.
//!
//! `main.rs` declares these again alongside the Linux-only display and input
//! modules, which cannot compile on a host. Anything needing a framebuffer or
//! an evdev node belongs there and not here.

pub mod catalog;
pub mod covers;
pub mod date;
pub mod lang;
pub mod log;
pub mod settings;
pub mod stats;
pub mod store;
