//! On-screen drawing: text rasterization, the frame a screen sits in, and the
//! shapes a reading log is read in.
//!
//! Every type size and every rule here is a design pixel at
//! [`scale::DESIGN_DPI`]; [`theme::Theme`] maps it through [`scale::Scale`] for
//! the panel in front of it.

pub mod charts;
pub mod chrome;
pub mod cover;
pub mod dialog;
pub mod paint;
pub mod scale;
pub mod splash;
pub mod text;
pub mod theme;
