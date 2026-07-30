//! A page: one chunk of rows, allocated together.
//!
//! Pages are the allocation unit of the buffer. A 10,000-row history costs ~80
//! allocations instead of 10,000, and dropping the oldest history is one
//! `pop_front` rather than a per-line free.
//!
//! Rows inside a page are fixed width, like Ghostty's. Uniform addressing keeps
//! reflow and iteration simple; variable-length rows would make writing a cell
//! past a row's current content impossible without moving it.

use crate::cell::Cell;

/// Rows per page. Small enough that eviction granularity stays fine, large enough
/// that allocation is rare.
pub(super) const ROWS_PER_PAGE: usize = 128;

/// Per-row bookkeeping.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(super) struct RowMeta {
    /// This row's content continues onto the next row (soft wrap).
    pub wrapped: bool,
    /// This row changed since the last time it was sent to a client.
    pub dirty: bool,
}

/// A fixed-width block of rows.
#[derive(Debug)]
pub(super) struct Page {
    cols: usize,
    meta: Vec<RowMeta>,
    cells: Vec<Cell>,
}

impl Page {
    /// An empty page that can hold [`ROWS_PER_PAGE`] rows of `cols` cells.
    pub fn new(cols: usize) -> Self {
        Self {
            cols,
            meta: Vec::with_capacity(ROWS_PER_PAGE),
            cells: Vec::with_capacity(ROWS_PER_PAGE * cols),
        }
    }

    /// Rows currently stored.
    pub fn len(&self) -> usize {
        self.meta.len()
    }

    /// Whether another row fits without growing the allocation.
    pub fn has_room(&self) -> bool {
        self.meta.len() < ROWS_PER_PAGE
    }

    /// Append a blank row. Caller must have checked [`Page::has_room`].
    pub fn push_blank_row(&mut self) {
        debug_assert!(self.has_room());
        self.meta.push(RowMeta {
            wrapped: false,
            dirty: true,
        });
        self.cells
            .resize(self.cells.len() + self.cols, Cell::default());
    }

    /// Append a row with the given cells, padded or truncated to the page width.
    pub fn push_row(&mut self, cells: &[Cell], wrapped: bool) {
        debug_assert!(self.has_room());
        self.meta.push(RowMeta {
            wrapped,
            dirty: true,
        });
        let start = self.cells.len();
        self.cells.resize(start + self.cols, Cell::default());
        let take = cells.len().min(self.cols);
        self.cells[start..start + take].copy_from_slice(&cells[..take]);
    }

    /// Drop the newest row.
    pub fn pop_row(&mut self) {
        if self.meta.pop().is_some() {
            self.cells.truncate(self.cells.len() - self.cols);
        }
    }

    /// Write a cell, marking the row dirty if it changed. Returns whether it did.
    ///
    /// On the hot path: one page lookup covers the write and the dirty flag.
    pub fn set_cell(&mut self, row: usize, col: usize, cell: Cell) -> bool {
        let start = match row.checked_mul(self.cols) {
            Some(start) => start,
            None => return false,
        };
        if col >= self.cols {
            return false;
        }
        let Some(slot) = self.cells.get_mut(start + col) else {
            return false;
        };
        if *slot == cell {
            return false;
        }
        *slot = cell;
        if let Some(meta) = self.meta.get_mut(row) {
            meta.dirty = true;
        }
        true
    }

    pub fn cells(&self, row: usize) -> Option<&[Cell]> {
        let start = row.checked_mul(self.cols)?;
        self.cells.get(start..start + self.cols)
    }

    pub fn cells_mut(&mut self, row: usize) -> Option<&mut [Cell]> {
        let start = row.checked_mul(self.cols)?;
        let end = start + self.cols;
        (end <= self.cells.len()).then(|| &mut self.cells[start..end])
    }

    pub fn meta(&self, row: usize) -> Option<&RowMeta> {
        self.meta.get(row)
    }

    pub fn meta_mut(&mut self, row: usize) -> Option<&mut RowMeta> {
        self.meta.get_mut(row)
    }

    /// Bytes of cell storage this page holds, for the memory budget.
    pub fn cell_bytes(&self) -> usize {
        self.cells.capacity() * std::mem::size_of::<Cell>()
    }
}
