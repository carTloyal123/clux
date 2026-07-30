//! Client-side screen buffer for hybrid rendering.
//!
//! The ScreenBuffer maintains a grid of styled cells and composites
//! pane content at the correct screen positions. This enables:
//! - Proper isolation between panes (no overwriting adjacent content)
//! - Client-side divider drawing
//! - Efficient partial updates

use std::collections::HashMap;
use std::sync::Arc;

use crate::cell::{Cell, CellFlags, Color};
use crate::protocol::WindowLayout;
use crate::selection::Selection;

/// Cursor position in screen coordinates.
#[derive(Debug, Clone, Copy, Default)]
pub struct CursorPosition {
    pub row: u16,
    pub col: u16,
    pub visible: bool,
}

/// Link id meaning "this cell is not part of a hyperlink".
pub(super) const NO_LINK: u32 = 0;

/// Begin a synchronized update (DECSET 2026).
///
/// The host terminal holds off presenting anything until the matching end, so a
/// repaint spanning several rows is never shown half-drawn. This is the form both
/// Ghostty and tmux advertise as the `Sync` terminfo capability; the older
/// iTerm2 `DCS = 1 s` form is not worth carrying.
pub const BEGIN_SYNC_UPDATE: &str = "\x1b[?2026h";

/// End a synchronized update (DECRST 2026), presenting the frame.
pub const END_SYNC_UPDATE: &str = "\x1b[?2026l";

/// An active selection, anchored in one pane.
///
/// Selections never span panes: the pane is fixed when the drag starts and every
/// later point is clamped into it, so dragging across a divider extends within
/// the original pane instead of splicing in the neighbour's text.
#[derive(Clone, Debug)]
pub(super) struct PaneSelection {
    pub(super) pane_id: u32,
    pub(super) selection: Selection,
}

/// Client-side screen buffer for compositing pane content.
pub struct ScreenBuffer {
    /// 2D grid of cells (row-major order).
    pub(super) cells: Vec<Vec<Cell>>,
    /// Link id per cell, mirroring `cells`. `NO_LINK` means no hyperlink.
    pub(super) link_ids: Vec<Vec<u32>>,
    /// Whether the pane row owning each cell continues onto the next row.
    ///
    /// Per cell rather than per screen row because side-by-side panes share a
    /// screen row and wrap independently.
    pub(super) row_continues: Vec<Vec<bool>>,
    /// URL for each link id currently on screen.
    pub(super) urls: HashMap<u32, Arc<str>>,
    /// Current window layout.
    pub(super) layout: Option<WindowLayout>,
    /// Screen width in columns.
    pub(super) cols: usize,
    /// Screen height in rows.
    pub(super) rows: usize,
    /// Current cursor position (screen coordinates, for focused pane).
    cursor: CursorPosition,
    /// Active text selection, if any.
    pub(super) selection: Option<PaneSelection>,
}

impl ScreenBuffer {
    /// Create a new screen buffer with the given dimensions.
    pub fn new(cols: usize, rows: usize) -> Self {
        let cells = vec![vec![Cell::default(); cols]; rows];
        Self {
            cells,
            link_ids: vec![vec![NO_LINK; cols]; rows],
            row_continues: vec![vec![false; cols]; rows],
            urls: HashMap::new(),
            layout: None,
            cols,
            rows,
            cursor: CursorPosition::default(),
            selection: None,
        }
    }

    /// Get the current dimensions.
    pub fn dimensions(&self) -> (usize, usize) {
        (self.cols, self.rows)
    }

    /// Set the window layout and draw dividers.
    pub fn set_layout(&mut self, layout: WindowLayout) {
        // Clear buffer before applying new layout
        self.clear();

        // Store layout
        self.layout = Some(layout);

        // Draw dividers between panes
        self.draw_dividers();
    }

    /// Get the current layout.
    pub fn layout(&self) -> Option<&WindowLayout> {
        self.layout.as_ref()
    }

    // ------------------------------------------------------------------------
    // Selection
    // ------------------------------------------------------------------------

    /// Set the cursor position (in screen coordinates).
    pub fn set_cursor(&mut self, row: u16, col: u16, visible: bool) {
        self.cursor = CursorPosition { row, col, visible };
    }

    /// Get the current cursor position.
    pub fn cursor(&self) -> CursorPosition {
        self.cursor
    }
}

/// Create a divider cell with default styling.
pub(super) fn divider_cell(c: char) -> Cell {
    Cell::styled(
        c,
        Color::indexed(8),
        Color::default_color(),
        CellFlags::empty(),
    )
}

mod access;
mod ansi;
mod color;
mod compose;
mod selection;

pub use ansi::{cells_to_ansi, cells_to_ansi_with_links};

#[cfg(test)]
mod test_support;

#[cfg(test)]
mod ansi_tests;
#[cfg(test)]
mod detect_tests;
#[cfg(test)]
mod link_tests;
#[cfg(test)]
mod selection_tests;
#[cfg(test)]
mod tests;
#[cfg(test)]
mod update_tests;
