//! Terminal content storage: one buffer, with the screen as a window into it.
//!
//! Rows live in a deque of fixed-width [`Page`]s. The last `screen_rows` rows are
//! the *active area* - what escape sequences address and write to. Everything
//! before them is history. The *viewport* is what a client sees: normally the
//! active area, or a pinned position in history when scrolled back.
//!
//! This replaces the previous split between a mutable fixed grid and an immutable
//! ring of history lines, which could not reflow history on resize, could not
//! carry a selection across the boundary, and needed a trait to be read
//! uniformly. See docs/PAGED_BUFFER.md.

mod addressing;
mod edit;
mod page;
mod pin;
mod reflow;
mod rewrap;
mod scroll;
#[cfg(test)]
mod tests;

pub use pin::Pin;

use std::collections::VecDeque;

use page::Page;
use pin::Viewport;

use crate::cell::Cell;

/// One row as the user sees it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ViewRow {
    pub cells: Vec<Cell>,
    /// The row continues onto the next one (soft wrap).
    pub wrapped: bool,
}

/// Terminal content: history plus the active screen.
#[derive(Debug)]
pub struct Buffer {
    /// Front is the oldest history, back holds the active area.
    pages: VecDeque<Page>,
    cols: usize,
    /// Height of the active area - the screen the application writes to.
    screen_rows: usize,
    /// Absolute row number of the oldest live row.
    first_abs: u64,
    /// Rows at the front of the first page that have been evicted.
    ///
    /// Dead rows are skipped rather than removed so every page except the last
    /// holds exactly [`ROWS_PER_PAGE`] rows - which is what lets a row be located
    /// by arithmetic instead of walking the pages. At most one page's worth of
    /// rows is held this way.
    front_skip: usize,
    /// Rows stored across all pages, including the active area.
    stored_rows: usize,
    /// Memory budget for history, in bytes. Zero for the alt screen, which keeps
    /// none. A byte ceiling rather than a row count because a row's cost depends
    /// on the width: 10,000 rows is 19 MB at 80 columns and 48 MB at 200.
    history_bytes: usize,
    viewport: Viewport,
}

impl Buffer {
    /// A buffer with a blank active area and `history_bytes` of history storage.
    pub fn new(screen_rows: usize, cols: usize, history_bytes: usize) -> Self {
        let screen_rows = screen_rows.max(1);
        let cols = cols.max(1);

        let mut buffer = Self {
            pages: VecDeque::new(),
            cols,
            screen_rows,
            first_abs: 0,
            front_skip: 0,
            stored_rows: 0,
            history_bytes,
            viewport: Viewport::Active,
        };

        // The active area is always materialised.
        for _ in 0..screen_rows {
            buffer.append_blank_row();
        }
        buffer
    }

    /// Height of the active area.
    pub fn screen_rows(&self) -> usize {
        self.screen_rows
    }

    /// Width of every row.
    pub fn cols(&self) -> usize {
        self.cols
    }

    /// The row at a viewport position, or a blank row past the end.
    pub fn view_row(&self, row: u16) -> ViewRow {
        match self.row_cells(row as usize) {
            Some((cells, wrapped)) => ViewRow {
                cells: cells.to_vec(),
                wrapped,
            },
            None => ViewRow {
                cells: Vec::new(),
                wrapped: false,
            },
        }
    }

    /// Rows per page, exposed for the budget tests.
    #[cfg(test)]
    pub(crate) fn rows_per_page() -> usize {
        page::ROWS_PER_PAGE
    }

    /// Rows the buffer may hold in total, derived from the byte budget.
    ///
    /// Deriving it means the memory ceiling holds when the window changes width,
    /// where a fixed row count would silently cost more.
    pub(super) fn max_rows(&self) -> usize {
        self.screen_rows + self.history_bytes / self.row_bytes()
    }

    /// Bytes one row of cells occupies.
    fn row_bytes(&self) -> usize {
        (self.cols * std::mem::size_of::<Cell>()).max(1)
    }

    /// Bytes of cell storage currently allocated.
    pub fn allocated_bytes(&self) -> usize {
        self.pages.iter().map(|page| page.cell_bytes()).sum()
    }

    /// Pages currently allocated. Storage granularity, for tests and budgeting.
    pub fn page_count(&self) -> usize {
        self.pages.len()
    }

    /// Rows of history above the active area.
    pub fn history_rows(&self) -> usize {
        self.stored_rows - self.screen_rows
    }

    /// Absolute row number of the top of the active area.
    pub(super) fn active_start_abs(&self) -> u64 {
        self.first_abs + (self.stored_rows - self.screen_rows) as u64
    }
}

impl Buffer {
    /// How many rows the viewport shows.
    pub fn row_count(&self) -> usize {
        self.screen_rows
    }

    /// Cells and wrap flag of a viewport row: history when scrolled back,
    /// otherwise the active screen.
    pub fn row_cells(&self, index: usize) -> Option<(&[Cell], bool)> {
        if index >= self.screen_rows {
            return None;
        }
        let top = match self.viewport {
            Viewport::Active => self.active_start_abs(),
            Viewport::Pinned(pin) => pin.abs(),
        };
        self.row_at(self.index_of(top + index as u64)?)
    }
}
