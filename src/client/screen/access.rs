//! Resizing, clearing, and reading back rows.

use super::ansi::{cells_to_ansi, cells_to_ansi_with_links};
use super::{CursorPosition, NO_LINK};
use crate::cell::Cell;
impl super::ScreenBuffer {
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
    /// Get the hyperlink URL at a screen position, if any.
    pub fn link_at(&self, row: usize, col: usize) -> Option<&str> {
        let id = *self.link_ids.get(row)?.get(col)?;
        self.urls.get(&id).map(|u| u.as_ref())
    }
}
