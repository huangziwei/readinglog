//! Reading Log: the log parser, the device catalog, the session store, the
//! statistics over them, and the screens drawn from those.
//!
//! Every module the device binary runs is here, so a host binary draws the
//! same screens from the same code. [`eink::fb::Framebuffer::offscreen`] gives
//! it a surface with no display behind it.

pub mod app;
pub mod catalog;
pub mod covers;
pub mod date;
pub mod eink;
pub mod font;
pub mod lang;
pub mod log;
pub mod orientation;
pub mod settings;
pub mod stats;
pub mod store;
pub mod ui;
pub mod view;
pub mod wrap;
