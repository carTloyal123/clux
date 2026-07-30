//! One character with its styling.
//!
//! A cell is copied per character into every pane's history, so its size is the
//! dominant memory cost of the program - see the size guard in `tests.rs`.

use std::num::NonZeroU32;

use bitflags::bitflags;
use serde::{Deserialize, Serialize};

/// A single cell in the terminal grid.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Cell {
    /// The character displayed in this cell.
    pub c: char,
    /// Foreground color.
    pub fg: Color,
    /// Background color.
    pub bg: Color,
    /// Style flags (bold, italic, etc.).
    pub flags: CellFlags,
    /// Hyperlink ID (Phase 3 - for now always None).
    pub hyperlink: Option<HyperlinkId>,
}

impl Default for Cell {
    fn default() -> Self {
        Self {
            c: ' ',
            fg: Color::default(),
            bg: Color::default(),
            flags: CellFlags::empty(),
            hyperlink: None,
        }
    }
}

impl Cell {
    /// Create a new cell with a character and default styling.
    pub fn new(c: char) -> Self {
        Self {
            c,
            ..Default::default()
        }
    }

    /// Create a cell with full styling.
    pub fn styled(c: char, fg: Color, bg: Color, flags: CellFlags) -> Self {
        Self {
            c,
            fg,
            bg,
            flags,
            hyperlink: None,
        }
    }

    /// Check if this cell is a space with default colors (can skip rendering).
    #[inline]
    pub fn is_empty(&self) -> bool {
        self.c == ' ' && self.fg.is_default() && self.bg.is_default() && self.flags.is_empty()
    }

    /// Reset cell to default state.
    pub fn reset(&mut self) {
        *self = Self::default();
    }
}

bitflags! {
    /// Cell style attributes.
    #[derive(Clone, Copy, Debug, PartialEq, Eq, Default, Serialize, Deserialize)]
    #[serde(transparent)]
    pub struct CellFlags: u8 {
        const BOLD          = 0b0000_0001;
        const DIM           = 0b0000_0010;
        const ITALIC        = 0b0000_0100;
        const UNDERLINE     = 0b0000_1000;
        const BLINK         = 0b0001_0000;
        const INVERSE       = 0b0010_0000;
        const HIDDEN        = 0b0100_0000;
        const STRIKETHROUGH = 0b1000_0000;
    }
}

impl CellFlags {}

/// Reference to an interned URL, so a cell costs 4 bytes instead of a `String`.
///
/// Non-zero so `Option<HyperlinkId>` fits in 4 bytes rather than 8: the niche
/// takes 4 bytes off every cell in every pane, history included.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct HyperlinkId(pub NonZeroU32);

impl HyperlinkId {
    /// Build an id from a counter, which must not be zero.
    pub fn new(id: u32) -> Option<Self> {
        NonZeroU32::new(id).map(Self)
    }

    /// The raw id.
    pub fn get(self) -> u32 {
        self.0.get()
    }
}

mod color;
#[cfg(test)]
mod tests;

pub use color::{Color, ColorKind};
