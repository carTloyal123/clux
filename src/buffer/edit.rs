//! Reading and writing the active area, and dirty tracking.
//!
//! Everything here addresses the *active area* - screen rows 0..`screen_rows` -
//! because that is what escape sequences mean by a row. History is immutable once
//! it leaves the screen.

use super::Buffer;
use crate::cell::Cell;

impl Buffer {
    /// Cells and wrap flag of an active-area row.
    pub fn active_row(&self, row: usize) -> Option<(&[Cell], bool)> {
        self.row_at(self.active_index(row)?)
    }

    /// The cell at an active-area position.
    pub fn cell(&self, row: usize, col: usize) -> Option<&Cell> {
        self.active_row(row)?.0.get(col)
    }

    /// Write a cell, marking its row dirty if the content changed.
    pub fn set_cell(&mut self, row: usize, col: usize, cell: Cell) {
        let Some(index) = self.active_index(row) else {
            return;
        };
        let Some((page_idx, page_row)) = self.locate(index) else {
            return;
        };

        if let Some(page) = self.pages.get_mut(page_idx) {
            page.set_cell(page_row, col, cell);
        }
    }

    /// Whether an active row continues onto the next.
    pub fn row_wrapped(&self, row: usize) -> bool {
        self.active_row(row).map(|(_, w)| w).unwrap_or(false)
    }

    /// Flag an active row as continuing onto the next (or not).
    pub fn set_row_wrapped(&mut self, row: usize, wrapped: bool) {
        let Some(index) = self.active_index(row) else {
            return;
        };
        let Some((page_idx, page_row)) = self.locate(index) else {
            return;
        };
        if let Some(meta) = self
            .pages
            .get_mut(page_idx)
            .and_then(|p| p.meta_mut(page_row))
        {
            meta.wrapped = wrapped;
        }
    }

    /// Blank an active row.
    pub fn clear_active_row(&mut self, row: usize) {
        self.clear_active_row_range(row, 0, self.cols);
        self.set_row_wrapped(row, false);
    }

    /// Blank `[from, to)` of an active row.
    pub fn clear_active_row_range(&mut self, row: usize, from: usize, to: usize) {
        let cols = self.cols;
        let Some(cells) = self.active_cells_mut(row) else {
            return;
        };
        for cell in &mut cells[from.min(cols)..to.min(cols)] {
            *cell = Cell::default();
        }
        self.mark_row_dirty(row);
    }

    /// Blank from a position to the end of the screen (ED 0).
    pub fn clear_below(&mut self, row: usize, col: usize) {
        self.clear_active_row_range(row, col, self.cols);
        for r in row + 1..self.screen_rows {
            self.clear_active_row(r);
        }
    }

    /// Blank from the start of the screen through a position (ED 1).
    pub fn clear_above(&mut self, row: usize, col: usize) {
        for r in 0..row {
            self.clear_active_row(r);
        }
        self.clear_active_row_range(row, 0, col + 1);
    }

    /// Blank the whole screen (ED 2). History is untouched.
    pub fn clear_screen(&mut self) {
        for row in 0..self.screen_rows {
            self.clear_active_row(row);
        }
    }

    // ------------------------------------------------------------------------
    // Dirty tracking
    // ------------------------------------------------------------------------

    /// Mark an active row as needing a repaint.
    pub fn mark_row_dirty(&mut self, row: usize) {
        let Some(index) = self.active_index(row) else {
            return;
        };
        self.mark_index_dirty(index);
    }

    pub(super) fn mark_rows_dirty(&mut self, from: usize, to: usize) {
        for row in from..to.min(self.screen_rows) {
            self.mark_row_dirty(row);
        }
    }

    /// Mark every viewport and active row dirty, so the next update is a full
    /// repaint. Used when the view moves rather than the content.
    pub fn mark_all_dirty(&mut self) {
        let top = self.viewport_top_abs();
        for row in 0..self.screen_rows {
            if let Some(index) = self.index_of(top + row as u64) {
                self.mark_index_dirty(index);
            }
        }

        // While scrolled back, the active rows are not the viewport rows, and they
        // need repainting too once the view returns.
        if self.is_scrolled() {
            self.mark_rows_dirty(0, self.screen_rows);
        }
    }

    fn mark_index_dirty(&mut self, index: usize) {
        let Some((page_idx, page_row)) = self.locate(index) else {
            return;
        };
        if let Some(meta) = self
            .pages
            .get_mut(page_idx)
            .and_then(|p| p.meta_mut(page_row))
        {
            meta.dirty = true;
        }
    }

    /// Viewport rows that changed since the last call, clearing their flags.
    ///
    /// Rows outside the viewport keep their flags: an application writing to the
    /// active area while the user reads history still needs a repaint when the
    /// view returns.
    pub fn take_dirty_rows(&mut self) -> Vec<u16> {
        let top = self.viewport_top_abs();
        let mut dirty = Vec::new();

        for row in 0..self.screen_rows {
            let Some(index) = self.index_of(top + row as u64) else {
                continue;
            };
            let Some((page_idx, page_row)) = self.locate(index) else {
                continue;
            };
            let Some(meta) = self
                .pages
                .get_mut(page_idx)
                .and_then(|p| p.meta_mut(page_row))
            else {
                continue;
            };
            if meta.dirty {
                meta.dirty = false;
                dirty.push(row as u16);
            }
        }

        dirty
    }
}
