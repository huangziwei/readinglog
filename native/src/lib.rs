//! [`log`] reads the reading logs and [`library`] the library, [`record`] keeps what both state,
//! [`stats`] totals it, and [`view`] draws it.

// `private_intra_doc_links`: the docs link private helpers, read with `--document-private-items`.
#![allow(rustdoc::private_intra_doc_links)]

pub mod app;
pub mod applog;
pub mod backup;
pub mod clippings;
pub mod clock;
pub mod covers;
pub mod date;
pub mod eink;
pub mod font;
pub mod hanfold;
pub mod highlights;
pub mod identify;
pub mod keyboard;
pub mod lang;
pub mod library;
pub mod lipc;
pub mod log;
pub mod mark;
pub mod markdown;
pub mod metadata;
pub mod net;
pub mod open;
pub mod orientation;
pub mod record;
pub mod settings;
pub mod sidecar;
pub mod stamp;
pub mod stats;
pub mod ui;
pub mod update;
pub mod view;
pub mod vocab;
pub mod wrap;
pub mod written;
pub mod zone;
