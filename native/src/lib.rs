//! Reading Log: [`log`] over the reading logs, [`catalog`], [`store`],
//! [`stats`] over those, and [`view`] drawing them.

// Docs name the private helpers beside them: this crate is read with
// `--document-private-items`.
#![allow(rustdoc::private_intra_doc_links)]

pub mod annotate;
pub mod app;
pub mod backup;
pub mod catalog;
pub mod clippings;
pub mod covers;
pub mod date;
pub mod eink;
pub mod font;
pub mod hanfold;
pub mod identify;
pub mod keyboard;
pub mod lang;
pub mod lipc;
pub mod log;
pub mod mark;
pub mod net;
pub mod open;
pub mod orientation;
pub mod settings;
pub mod sidecar;
pub mod stats;
pub mod store;
pub mod ui;
pub mod update;
pub mod view;
pub mod vocab;
pub mod wrap;
pub mod zone;
