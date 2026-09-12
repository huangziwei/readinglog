//! On-screen drawing. Every type size and rule here is a design pixel written
//! for [`scale::REFERENCE`], which [`theme::Theme`] maps through
//! [`scale::Scale`] for the panel in front of it.

pub mod charts;
pub mod chrome;
pub mod cover;
pub mod dialog;
pub mod paint;
pub mod scale;
pub mod splash;
pub mod text;
pub mod theme;
