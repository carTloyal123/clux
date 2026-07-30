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
use crate::protocol::{PaneRow, WindowLayout};
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

    /// Apply a pane update to the screen buffer.
    /// Translates pane-local coordinates to screen coordinates.
    pub fn apply_pane_update(&mut self, pane_id: u32, changed_rows: &[PaneRow]) {
        let Some(layout) = &self.layout else {
            return;
        };

        // Find the pane in the layout
        let Some(pane) = layout.panes.iter().find(|p| p.pane_id == pane_id) else {
            return;
        };

        let pane_x = pane.x as usize;
        let pane_width = pane.width as usize;

        // Apply each row update
        for pane_row in changed_rows {
            let screen_row = pane.y as usize + pane_row.row_idx as usize;

            // Bounds check
            if screen_row >= self.rows {
                continue;
            }

            // Links arrive as the complete set for the row, so drop the pane's
            // previous ones across its full width - not just the columns this
            // update rewrites, or a shrinking link leaves a stale tail behind.
            let pane_end = (pane_x + pane_width).min(self.cols);
            for screen_col in pane_x..pane_end {
                self.link_ids[screen_row][screen_col] = NO_LINK;
            }

            // The wrap flag belongs to the pane row, so record it across the
            // pane's columns: neighbouring panes on this screen row wrap
            // independently.
            for screen_col in pane_x..pane_end {
                self.row_continues[screen_row][screen_col] = pane_row.wrapped;
            }

            // Copy cells to the correct screen position
            for (col_offset, cell) in pane_row.cells.iter().enumerate() {
                let screen_col = pane_x + col_offset;

                // Bounds check - don't overflow pane width
                if col_offset >= pane_width {
                    break;
                }
                if screen_col >= self.cols {
                    break;
                }

                self.cells[screen_row][screen_col] = *cell;
            }

            for link in &pane_row.links {
                if link.url.chars().any(|c| c.is_control()) {
                    // Never let a URL smuggle escape bytes into the host terminal.
                    continue;
                }

                let start = pane_x + link.start_col as usize;
                let end = (pane_x + link.end_col as usize)
                    .min(pane_x + pane_width)
                    .min(self.cols);

                if link.id == NO_LINK || start >= end {
                    continue;
                }

                // Refresh the target if this id now points somewhere else, but
                // avoid reallocating on the common case of an unchanged repaint.
                match self.urls.get(&link.id) {
                    Some(known) if known.as_ref() == link.url.as_str() => {}
                    _ => {
                        self.urls.insert(link.id, Arc::from(link.url.as_str()));
                    }
                }

                for screen_col in start..end {
                    self.link_ids[screen_row][screen_col] = link.id;

                    // A URL clux found itself gets an underline so it reads as a
                    // link. An application's own OSC 8 link is left exactly as it
                    // styled it - it already decided how its links should look.
                    if link.detected {
                        self.cells[screen_row][screen_col]
                            .flags
                            .insert(CellFlags::UNDERLINE);
                    }
                }
            }
        }

        self.prune_urls();
    }

    /// Drop URLs whose link id is no longer on screen.
    ///
    /// Only worth the scan once the table has grown; screens rarely hold more
    /// than a handful of distinct links.
    fn prune_urls(&mut self) {
        const PRUNE_THRESHOLD: usize = 256;

        if self.urls.len() <= PRUNE_THRESHOLD {
            return;
        }

        let live: std::collections::HashSet<u32> = self
            .link_ids
            .iter()
            .flatten()
            .copied()
            .filter(|&id| id != NO_LINK)
            .collect();

        self.urls.retain(|id, _| live.contains(id));
    }

    /// Get the hyperlink URL at a screen position, if any.
    pub fn link_at(&self, row: usize, col: usize) -> Option<&str> {
        let id = *self.link_ids.get(row)?.get(col)?;
        self.urls.get(&id).map(|u| u.as_ref())
    }

    // ------------------------------------------------------------------------
    // Selection
    // ------------------------------------------------------------------------

    /// Resize the screen buffer.
    /// Clears all content and resets layout.
    pub fn resize(&mut self, cols: usize, rows: usize) {
        self.cols = cols;
        self.rows = rows;
        self.cells = vec![vec![Cell::default(); cols]; rows];
        self.link_ids = vec![vec![NO_LINK; cols]; rows];
        self.row_continues = vec![vec![false; cols]; rows];
        self.urls.clear();
        self.layout = None;
        self.cursor = CursorPosition::default();
        self.selection = None;
    }

    /// Set the cursor position (in screen coordinates).
    pub fn set_cursor(&mut self, row: u16, col: u16, visible: bool) {
        self.cursor = CursorPosition { row, col, visible };
    }

    /// Get the current cursor position.
    pub fn cursor(&self) -> CursorPosition {
        self.cursor
    }

    /// Clear the screen buffer to default cells.
    pub fn clear(&mut self) {
        for row in &mut self.cells {
            for cell in row {
                *cell = Cell::default();
            }
        }
        for row in &mut self.link_ids {
            for id in row {
                *id = NO_LINK;
            }
        }
        for row in &mut self.row_continues {
            for wrapped in row {
                *wrapped = false;
            }
        }
        self.urls.clear();
        self.selection = None;
    }

    /// Get a row of cells.
    pub fn get_row(&self, row_idx: usize) -> Option<&[Cell]> {
        self.cells.get(row_idx).map(|r| r.as_slice())
    }

    /// Render a row to an ANSI escape sequence string, including OSC 8
    /// hyperlinks and any selection highlight on that row.
    pub fn render_row_ansi(&self, row_idx: usize) -> String {
        let Some(row) = self.cells.get(row_idx) else {
            return String::new();
        };

        // Selection is transient, so it is applied here rather than baked into
        // the stored cells: clearing it needs no restore.
        let highlighted = self.highlight_selection(row_idx, row);
        let row = highlighted.as_deref().unwrap_or(row);

        match self.link_ids.get(row_idx) {
            Some(ids) => cells_to_ansi_with_links(row, ids, &self.urls),
            None => cells_to_ansi(row),
        }
    }

    /// Draw dividers between panes based on the current layout.
    fn draw_dividers(&mut self) {
        let Some(layout) = &self.layout else {
            return;
        };

        // For each pane, check if we need to draw dividers
        // We draw dividers to the LEFT and ABOVE each pane (except the first)
        for pane in &layout.panes {
            // Draw left vertical divider if pane doesn't start at column 0
            if pane.x > 0 {
                let divider_col = pane.x as usize - 1;
                for row in pane.y as usize..(pane.y as usize + pane.height as usize) {
                    if row < self.rows && divider_col < self.cols {
                        self.cells[row][divider_col] = divider_cell('│');
                    }
                }
            }

            // Draw top horizontal divider if pane doesn't start at row 0
            if pane.y > 0 {
                let divider_row = pane.y as usize - 1;
                if divider_row < self.rows {
                    for col in pane.x as usize..(pane.x as usize + pane.width as usize) {
                        if col < self.cols {
                            // Check for intersection with vertical divider
                            let existing = self.cells[divider_row][col].c;
                            let ch = if existing == '│' {
                                '┼' // Intersection
                            } else {
                                '─'
                            };
                            self.cells[divider_row][col] = divider_cell(ch);
                        }
                    }
                }
            }
        }
    }
}

/// Create a divider cell with default styling.
fn divider_cell(c: char) -> Cell {
    Cell::styled(
        c,
        Color::indexed(8),
        Color::default_color(),
        CellFlags::empty(),
    )
}

mod ansi;
mod selection;

pub use ansi::{cells_to_ansi, cells_to_ansi_with_links};

#[cfg(test)]
mod test_support;

#[cfg(test)]
mod ansi_tests;
#[cfg(test)]
mod link_tests;
#[cfg(test)]
mod selection_tests;
#[cfg(test)]
mod tests;
