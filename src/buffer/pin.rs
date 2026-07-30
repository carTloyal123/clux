//! Pins and the viewport.
//!
//! A [`Pin`] is an absolute row number: rows are numbered monotonically for the
//! life of a pane, so a pin keeps meaning as history is evicted. That is the one
//! place this design departs from Ghostty, which pins `(page, row)` pairs and has
//! to walk a tracked-pin set whenever pages change. Here eviction is one integer
//! update, and only reflow - which genuinely rewrites rows - has to remap pins.

/// An absolute row position in a buffer's history.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct Pin(pub(super) u64);

impl Pin {
    /// The raw absolute row number.
    pub fn abs(self) -> u64 {
        self.0
    }
}

/// What the viewport is showing.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Viewport {
    /// Follow new output: the viewport is the active area.
    Active,
    /// Hold a position in history. New output does not move it, which is what
    /// keeps a scrolled view from drifting.
    Pinned(Pin),
}

impl Default for Viewport {
    fn default() -> Self {
        Self::Active
    }
}
