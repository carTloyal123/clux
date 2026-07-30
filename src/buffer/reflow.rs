//! Resize, including re-wrapping history.
//!
//! Re-wrapping the whole buffer is the payoff of storing history in the same place
//! as the screen: widening the window puts a wrapped line back together, instead of
//! leaving old output broken at a column that no longer exists.
//!
//! Rows are renumbered afterwards, so pins taken before a reflow resolve to
//! nothing rather than to the wrong row. Anything that must survive - the cursor,
//! the viewport - is remapped explicitly through its logical position.

use super::pin::{Pin, Viewport};
use super::Buffer;
use crate::cell::Cell;

impl Buffer {
    /// Resize the screen, re-wrapping content if the width changed.
    ///
    /// Takes the cursor's active-area position and returns where that character
    /// ended up.
    pub fn resize(
        &mut self,
        screen_rows: usize,
        cols: usize,
        cursor: (usize, usize),
    ) -> (usize, usize) {
        let screen_rows = screen_rows.max(1);
        let cols = cols.max(1);

        let cursor_screen_row = cursor.0.min(screen_rows - 1);

        if cols == self.cols {
            let cursor_index = self.active_index(cursor.0).unwrap_or(0);
            let cursor_row = self.resize_rows(screen_rows, cursor_index);
            return (cursor_row, cursor.1.min(cols - 1));
        }

        if self.history_bytes == 0 {
            // The alternate screen has no history to re-wrap, and applications
            // redraw it themselves. Resize without reflow, as terminals do.
            self.resize_without_reflow(screen_rows, cols);
            return (
                cursor.0.min(self.screen_rows - 1),
                cursor.1.min(self.cols - 1),
            );
        }

        // Where the cursor and the viewport sit, in terms that survive re-wrapping.
        let old_cols = self.cols;
        let cursor_index = self.active_index(cursor.0).unwrap_or(0);
        let viewport_index = self.index_of(self.viewport_top_abs());

        let lines = self.logical_lines();
        let anchors = self.rebuild(&lines, screen_rows, cols);

        let cursor_pos = anchors.remap(cursor_index, cursor.1, old_cols, cols);
        self.place_active_area(cursor_pos.0, cursor_screen_row);
        let cursor_row = cursor_pos
            .0
            .saturating_sub(self.stored_rows - self.screen_rows);

        if let Some(index) = viewport_index {
            let (row, _) = anchors.remap(index, 0, old_cols, cols);
            let abs = self.first_abs + row as u64;
            self.viewport = if abs >= self.active_start_abs() {
                Viewport::Active
            } else {
                Viewport::Pinned(Pin(abs))
            };
        }

        self.mark_all_dirty();
        (
            cursor_row.min(self.screen_rows - 1),
            cursor_pos.1.min(cols - 1),
        )
    }

    /// Change the screen height, moving the active/history boundary.
    ///
    /// The cursor keeps its *absolute* row, so growing the screen pulls history
    /// back into view rather than adding blank rows below - which is what a
    /// terminal does when you enlarge the window. Blank rows below the cursor are
    /// reclaimed first, so shrinking a mostly-empty screen does not shove live
    /// content into history.
    ///
    /// Returns the cursor's new screen row.
    fn resize_rows(&mut self, screen_rows: usize, cursor_index: usize) -> usize {
        self.screen_rows = screen_rows;

        while self.stored_rows > screen_rows
            && cursor_index + 1 < self.stored_rows
            && self.pop_blank_tail_row()
        {}
        while self.stored_rows < screen_rows {
            self.append_blank_row();
        }
        self.enforce_budget_now();
        self.mark_all_dirty();

        cursor_index
            .saturating_sub(self.stored_rows - self.screen_rows)
            .min(screen_rows - 1)
    }

    /// Position the active area so the cursor keeps its screen row.
    ///
    /// Re-wrapping changes how many rows content occupies, so without this the
    /// "active area is the last rows" rule would slide content into history.
    fn place_active_area(&mut self, cursor_row: usize, cursor_screen_row: usize) {
        // Never ask for more rows than the budget allows: appending past it evicts
        // from the front, so the loop below would never finish.
        let wanted = (cursor_row + self.screen_rows - cursor_screen_row)
            .max(self.screen_rows)
            .min(self.max_rows());

        while self.stored_rows < wanted {
            self.append_blank_row();
        }
        // Only padding may be dropped; real content stays.
        while self.stored_rows > wanted && self.pop_blank_tail_row() {}
        self.enforce_budget_now();
    }

    /// Resize without re-wrapping: rows keep their content, clipped or padded.
    fn resize_without_reflow(&mut self, screen_rows: usize, cols: usize) {
        let rows: Vec<(Vec<Cell>, bool)> = (0..self.stored_rows)
            .filter_map(|i| self.row_at(i).map(|(cells, w)| (cells.to_vec(), w)))
            .collect();

        self.reset_storage(cols, screen_rows);
        for (cells, wrapped) in rows {
            self.push_row(&cells, wrapped);
        }
        while self.stored_rows < screen_rows {
            self.append_blank_row();
        }
        self.enforce_budget_now();
        self.mark_all_dirty();
    }
}
