//! Locating rows in the pages.
//!
//! Rows are addressed three ways, and keeping them straight matters: an *absolute*
//! row number (stable for the life of the pane), a *storage* index (0 = oldest live
//! row), and a *screen* row (relative to the active area or the viewport).

use super::page::ROWS_PER_PAGE;
use super::Buffer;
use crate::cell::Cell;

impl Buffer {
    /// Storage index of an absolute row, if it is still stored.
    pub(super) fn index_of(&self, abs: u64) -> Option<usize> {
        let index = abs.checked_sub(self.first_abs)? as usize;
        (index < self.stored_rows).then_some(index)
    }

    /// Locate a live row as `(page, row within page)`.
    ///
    /// O(1): pages are uniform, so this is division rather than a walk. Cell
    /// writes go through here on the hot path.
    pub(super) fn locate(&self, index: usize) -> Option<(usize, usize)> {
        if index >= self.stored_rows {
            return None;
        }
        let pos = index + self.front_skip;
        Some((pos / ROWS_PER_PAGE, pos % ROWS_PER_PAGE))
    }

    /// Cells and wrap flag of a stored row.
    pub(super) fn row_at(&self, index: usize) -> Option<(&[Cell], bool)> {
        let (page_idx, row) = self.locate(index)?;
        let page = self.pages.get(page_idx)?;
        Some((page.cells(row)?, page.meta(row)?.wrapped))
    }

    /// Cells of an active-area row, for writing.
    pub(super) fn active_cells_mut(&mut self, row: usize) -> Option<&mut [Cell]> {
        let index = self.active_index(row)?;
        let (page_idx, row) = self.locate(index)?;
        self.pages.get_mut(page_idx)?.cells_mut(row)
    }

    /// Storage index of an active-area row.
    pub(super) fn active_index(&self, row: usize) -> Option<usize> {
        (row < self.screen_rows).then(|| self.stored_rows - self.screen_rows + row)
    }
}
