//! Reading Log: the log parser, the device catalog, the session store, the
//! statistics over them, and the screens drawn from those. Every module the
//! device binary runs is here.

pub mod app;
pub mod backup;
pub mod catalog;
pub mod covers;
pub mod date;
pub mod eink;
pub mod font;
pub mod lang;
pub mod log;
pub mod mark;
pub mod net;
pub mod open;
pub mod orientation;
pub mod settings;
pub mod stats;
pub mod store;
pub mod ui;
pub mod update;
pub mod view;
pub mod wrap;
