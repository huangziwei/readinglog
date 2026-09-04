//! What a candidate screen is drawn with. [`drafts`] registers the candidates
//! themselves, and is empty in the repository.

mod drafts;

use readinglog_native::ui::chrome::Tab;
use readinglog_native::ui::paint::Rect;
use readinglog_native::view::{Ctx, State};

/// What a sketch draws with: the same three arguments a screen takes.
pub type Draw = fn(&mut Ctx, Rect, &State);

/// A candidate screen: what the preview calls it, the tab it stands under, and
/// what it draws.
#[derive(Clone, Copy)]
pub struct Sketch {
    pub name: &'static str,
    pub tab: Tab,
    pub draw: Draw,
}

pub use drafts::DRAFTS as ALL;
