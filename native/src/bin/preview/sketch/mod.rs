//! Candidate screens, drawn beside the ones that ship.
//!
//! A sketch takes the same content box, the same [`Ctx`] and the same
//! [`State`] a screen does, and draws with the same primitives, so what the
//! preview shows is what the screen would be. Nothing here reaches the device
//! binary: the shipped screens are in `view`, and a sketch becomes one of them
//! by being written there.

mod rulers;

use readinglog_native::ui::chrome::Tab;
use readinglog_native::ui::paint::Rect;
use readinglog_native::view::{Ctx, State};

/// What a sketch draws with: the same three arguments a screen takes.
pub type Draw = fn(&mut Ctx, Rect, &State);

/// A candidate screen: what the preview calls it, the tab it stands under, and
/// what it draws.
pub struct Sketch {
    pub name: &'static str,
    pub tab: Tab,
    pub draw: Draw,
}

/// Every sketch, by the name the preview calls it.
pub const ALL: &[Sketch] = &[Sketch {
    name: "rulers",
    tab: Tab::Rhythm,
    draw: rulers::draw,
}];
