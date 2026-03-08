//! Terminal grid storage.
//!
//! The grid stores the visible terminal content as a 2D array of cells.
//! Optimized for row-major access patterns and efficient rendering.

// Many methods will be used in later phases (selection, panes)
#![allow(dead_code)]

use crate::cell::Cell;

/// A single row in the terminal grid.
#[derive(Clone, Debug)]
pub struct Row {
    /// Cells in this row.
    cells: Vec<Cell>,
    /// Whether this row has been modified since last render.
    dirty: bool,
    /// Whether this row wrapped from the previous line.
    wrapped: bool,
}

impl Row {
    /// Create a new row with the given width.
    pub fn new(cols: usize) -> Self {
        Self {
            cells: vec![Cell::default(); cols],
            dirty: true,
            wrapped: false,
        }
    }

    /// Get a cell at the given column.
    #[inline]
    pub fn get(&self, col: usize) -> Option<&Cell> {
        self.cells.get(col)
    }

    /// Get a mutable cell at the given column.
    #[inline]
    pub fn get_mut(&mut self, col: usize) -> Option<&mut Cell> {
        self.dirty = true;
        self.cells.get_mut(col)
    }

    /// Set a cell at the given column.
    #[inline]
    pub fn set(&mut self, col: usize, cell: Cell) {
        if let Some(c) = self.cells.get_mut(col) {
            if *c != cell {
                *c = cell;
                self.dirty = true;
            }
        }
    }

    /// Get the number of columns in this row.
    #[inline]
    pub fn cols(&self) -> usize {
        self.cells.len()
    }

    /// Check if this row is dirty (needs re-rendering).
    #[inline]
    pub fn is_dirty(&self) -> bool {
        self.dirty
    }

    /// Clear the dirty flag.
    #[inline]
    pub fn clear_dirty(&mut self) {
        self.dirty = false;
    }

    /// Mark this row as dirty.
    #[inline]
    pub fn mark_dirty(&mut self) {
        self.dirty = true;
    }

    /// Check if this row wrapped from the previous line.
    #[inline]
    pub fn is_wrapped(&self) -> bool {
        self.wrapped
    }

    /// Set the wrapped flag.
    #[inline]
    pub fn set_wrapped(&mut self, wrapped: bool) {
        self.wrapped = wrapped;
    }

    /// Clear all cells in this row to default.
    pub fn clear(&mut self) {
        for cell in &mut self.cells {
            cell.reset();
        }
        self.dirty = true;
    }

    /// Clear cells from the given column to the end.
    pub fn clear_from(&mut self, col: usize) {
        for cell in self.cells.iter_mut().skip(col) {
            cell.reset();
        }
        self.dirty = true;
    }

    /// Clear cells from the start to the given column (exclusive).
    pub fn clear_to(&mut self, col: usize) {
        for cell in self.cells.iter_mut().take(col) {
            cell.reset();
        }
        self.dirty = true;
    }

    /// Resize this row to a new column count.
    pub fn resize(&mut self, cols: usize) {
        self.cells.resize(cols, Cell::default());
        self.dirty = true;
    }

    /// Iterate over cells in this row.
    pub fn iter(&self) -> impl Iterator<Item = &Cell> {
        self.cells.iter()
    }

    /// Iterate over cells with their column index.
    pub fn iter_enumerated(&self) -> impl Iterator<Item = (usize, &Cell)> {
        self.cells.iter().enumerate()
    }
}

/// The terminal grid containing visible content.
#[derive(Clone, Debug)]
pub struct Grid {
    /// Rows in the grid (visible area).
    rows: Vec<Row>,
    /// Number of columns.
    cols: usize,
    /// Number of visible rows.
    num_rows: usize,
}

impl Grid {
    /// Create a new grid with the given dimensions.
    pub fn new(rows: usize, cols: usize) -> Self {
        Self {
            rows: (0..rows).map(|_| Row::new(cols)).collect(),
            cols,
            num_rows: rows,
        }
    }

    /// Get the number of columns.
    #[inline]
    pub fn cols(&self) -> usize {
        self.cols
    }

    /// Get the number of rows.
    #[inline]
    pub fn rows(&self) -> usize {
        self.num_rows
    }

    /// Get a cell at the given position.
    #[inline]
    pub fn get(&self, row: usize, col: usize) -> Option<&Cell> {
        self.rows.get(row)?.get(col)
    }

    /// Get a mutable cell at the given position.
    #[inline]
    pub fn get_mut(&mut self, row: usize, col: usize) -> Option<&mut Cell> {
        self.rows.get_mut(row)?.get_mut(col)
    }

    /// Set a cell at the given position.
    #[inline]
    pub fn set(&mut self, row: usize, col: usize, cell: Cell) {
        if let Some(r) = self.rows.get_mut(row) {
            r.set(col, cell);
        }
    }

    /// Get a row by index.
    #[inline]
    pub fn row(&self, row: usize) -> Option<&Row> {
        self.rows.get(row)
    }

    /// Get a mutable row by index.
    #[inline]
    pub fn row_mut(&mut self, row: usize) -> Option<&mut Row> {
        self.rows.get_mut(row)
    }

    /// Iterate over rows.
    pub fn iter_rows(&self) -> impl Iterator<Item = &Row> {
        self.rows.iter()
    }

    /// Iterate over rows with their index.
    pub fn iter_rows_enumerated(&self) -> impl Iterator<Item = (usize, &Row)> {
        self.rows.iter().enumerate()
    }

    /// Scroll the grid up by one line, returning the scrolled-out line.
    /// A new empty line is added at the bottom.
    pub fn scroll_up(&mut self) -> Row {
        let old_row = self.rows.remove(0);
        self.rows.push(Row::new(self.cols));
        // Mark all rows dirty since positions changed
        for row in &mut self.rows {
            row.mark_dirty();
        }
        old_row
    }

    /// Scroll the grid down by one line.
    /// A new empty line is added at the top.
    pub fn scroll_down(&mut self) {
        self.rows.pop();
        self.rows.insert(0, Row::new(self.cols));
        // Mark all rows dirty since positions changed
        for row in &mut self.rows {
            row.mark_dirty();
        }
    }

    /// Scroll a region of the grid up.
    pub fn scroll_region_up(&mut self, top: usize, bottom: usize) -> Option<Row> {
        if top >= bottom || bottom > self.num_rows {
            return None;
        }

        let old_row = self.rows.remove(top);
        self.rows.insert(bottom - 1, Row::new(self.cols));

        // Mark affected rows dirty
        for row in self.rows.iter_mut().skip(top).take(bottom - top) {
            row.mark_dirty();
        }

        Some(old_row)
    }

    /// Scroll a region of the grid down.
    pub fn scroll_region_down(&mut self, top: usize, bottom: usize) {
        if top >= bottom || bottom > self.num_rows {
            return;
        }

        self.rows.remove(bottom - 1);
        self.rows.insert(top, Row::new(self.cols));

        // Mark affected rows dirty
        for row in self.rows.iter_mut().skip(top).take(bottom - top) {
            row.mark_dirty();
        }
    }

    /// Clear the entire grid.
    pub fn clear(&mut self) {
        for row in &mut self.rows {
            row.clear();
        }
    }

    /// Clear from cursor position to end of screen.
    pub fn clear_below(&mut self, row: usize, col: usize) {
        // Clear rest of current row
        if let Some(r) = self.rows.get_mut(row) {
            r.clear_from(col);
        }
        // Clear all rows below
        for r in self.rows.iter_mut().skip(row + 1) {
            r.clear();
        }
    }

    /// Clear from start of screen to cursor position.
    pub fn clear_above(&mut self, row: usize, col: usize) {
        // Clear all rows above
        for r in self.rows.iter_mut().take(row) {
            r.clear();
        }
        // Clear start of current row
        if let Some(r) = self.rows.get_mut(row) {
            r.clear_to(col + 1);
        }
    }

    /// Resize the grid to new dimensions (simple truncation, no reflow).
    pub fn resize(&mut self, new_rows: usize, new_cols: usize) {
        // Resize existing rows
        for row in &mut self.rows {
            row.resize(new_cols);
        }

        // Add or remove rows
        if new_rows > self.num_rows {
            for _ in 0..(new_rows - self.num_rows) {
                self.rows.push(Row::new(new_cols));
            }
        } else if new_rows < self.num_rows {
            self.rows.truncate(new_rows);
        }

        self.cols = new_cols;
        self.num_rows = new_rows;
    }

    /// Resize the grid with line reflow for width changes.
    /// Returns the new cursor position after reflow.
    pub fn resize_with_reflow(
        &mut self,
        new_rows: usize,
        new_cols: usize,
        cursor_row: usize,
        cursor_col: usize,
    ) -> (usize, usize) {
        let old_cols = self.cols;

        // If width hasn't changed, just do simple resize
        if new_cols == old_cols {
            self.resize(new_rows, new_cols);
            let new_cursor_row = cursor_row.min(new_rows.saturating_sub(1));
            let new_cursor_col = cursor_col.min(new_cols.saturating_sub(1));
            return (new_cursor_row, new_cursor_col);
        }

        // Step 1: Unwrap all soft-wrapped lines into logical lines
        let mut logical_lines: Vec<Vec<Cell>> = Vec::new();
        let mut current_line: Vec<Cell> = Vec::new();

        // Track which logical line and offset the cursor is on
        let mut cursor_logical_line = 0;
        let mut cursor_offset_in_line = 0;
        let mut found_cursor = false;

        for (row_idx, row) in self.rows.iter().enumerate() {
            // Add this row's cells to current logical line
            let start_offset = current_line.len();
            for cell in row.iter() {
                current_line.push(*cell);
            }

            // Track cursor position in logical line
            if row_idx == cursor_row && !found_cursor {
                cursor_logical_line = logical_lines.len();
                cursor_offset_in_line = start_offset + cursor_col;
                found_cursor = true;
            }

            // If this row is NOT wrapped, end the logical line
            if !row.is_wrapped() {
                // Trim trailing spaces to avoid unnecessary wrapping, but we've already
                // recorded the cursor position above, so it will be preserved correctly.
                while current_line.last().map(|c| c.c == ' ').unwrap_or(false) {
                    current_line.pop();
                }
                logical_lines.push(current_line);
                current_line = Vec::new();
            }
        }

        // Don't forget any remaining content
        if !current_line.is_empty() {
            logical_lines.push(current_line);
        }

        // Step 2: Re-wrap logical lines to new width
        let mut new_rows_data: Vec<Row> = Vec::new();
        let mut new_cursor_row = 0;
        let mut new_cursor_col = 0;

        for (line_idx, logical_line) in logical_lines.iter().enumerate() {
            if logical_line.is_empty() {
                // Empty logical line becomes one empty row
                let row = Row::new(new_cols);
                if line_idx == cursor_logical_line {
                    new_cursor_row = new_rows_data.len();
                    // Preserve cursor column for empty lines, clamped to new width
                    new_cursor_col = cursor_offset_in_line.min(new_cols.saturating_sub(1));
                }
                new_rows_data.push(row);
                continue;
            }

            // Split logical line into chunks of new_cols
            let mut offset = 0;

            while offset < logical_line.len() {
                let chunk_end = (offset + new_cols).min(logical_line.len());
                let mut row = Row::new(new_cols);

                // Copy cells to this row
                for (i, cell) in logical_line[offset..chunk_end].iter().enumerate() {
                    row.set(i, *cell);
                }

                // Mark as wrapped if there's more content after this chunk
                if chunk_end < logical_line.len() {
                    row.set_wrapped(true);
                }

                // Track cursor position
                if line_idx == cursor_logical_line {
                    let is_last_chunk = chunk_end >= logical_line.len();

                    // Check if cursor falls within this chunk
                    if cursor_offset_in_line >= offset && cursor_offset_in_line < chunk_end {
                        // Cursor is within the content of this chunk
                        new_cursor_row = new_rows_data.len();
                        new_cursor_col = cursor_offset_in_line - offset;
                    } else if is_last_chunk && cursor_offset_in_line >= offset {
                        // Cursor is past the content but this is the last chunk
                        // Preserve cursor position (clamped to new width) - it might be
                        // in trailing space that was trimmed, which is fine
                        new_cursor_row = new_rows_data.len();
                        new_cursor_col =
                            (cursor_offset_in_line - offset).min(new_cols.saturating_sub(1));
                    }
                }

                new_rows_data.push(row);
                offset = chunk_end;
            }
        }

        // Step 3: Adjust to target row count
        if new_rows_data.len() < new_rows {
            // Add empty rows at bottom
            while new_rows_data.len() < new_rows {
                new_rows_data.push(Row::new(new_cols));
            }
        } else if new_rows_data.len() > new_rows {
            // We have more rows than fit - keep rows around cursor visible
            // Try to keep cursor in the lower portion of screen
            let target_cursor_from_bottom = new_rows / 3; // Keep cursor in lower third
            let ideal_start =
                new_cursor_row.saturating_sub(new_rows.saturating_sub(target_cursor_from_bottom));
            let max_start = new_rows_data.len().saturating_sub(new_rows);
            let start = ideal_start.min(max_start);

            new_rows_data = new_rows_data
                .into_iter()
                .skip(start)
                .take(new_rows)
                .collect();
            new_cursor_row = new_cursor_row.saturating_sub(start);
        }

        // Update grid state
        self.rows = new_rows_data;
        self.cols = new_cols;
        self.num_rows = new_rows;

        // Ensure cursor is in bounds
        new_cursor_row = new_cursor_row.min(new_rows.saturating_sub(1));
        new_cursor_col = new_cursor_col.min(new_cols.saturating_sub(1));

        (new_cursor_row, new_cursor_col)
    }

    /// Check if any row is dirty.
    pub fn has_dirty_rows(&self) -> bool {
        self.rows.iter().any(|r| r.is_dirty())
    }

    /// Clear all dirty flags.
    pub fn clear_all_dirty(&mut self) {
        for row in &mut self.rows {
            row.clear_dirty();
        }
    }

    /// Get indices of dirty rows.
    pub fn dirty_row_indices(&self) -> impl Iterator<Item = usize> + '_ {
        self.rows
            .iter()
            .enumerate()
            .filter_map(|(i, r)| if r.is_dirty() { Some(i) } else { None })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_grid_creation() {
        let grid = Grid::new(24, 80);
        assert_eq!(grid.rows(), 24);
        assert_eq!(grid.cols(), 80);
    }

    #[test]
    fn test_grid_set_get() {
        let mut grid = Grid::new(24, 80);
        let cell = Cell::new('A');
        grid.set(5, 10, cell);

        let retrieved = grid.get(5, 10).unwrap();
        assert_eq!(retrieved.c, 'A');
    }

    #[test]
    fn test_grid_scroll_up() {
        let mut grid = Grid::new(3, 10);
        grid.set(0, 0, Cell::new('A'));
        grid.set(1, 0, Cell::new('B'));
        grid.set(2, 0, Cell::new('C'));

        let scrolled = grid.scroll_up();
        assert_eq!(scrolled.get(0).unwrap().c, 'A');
        assert_eq!(grid.get(0, 0).unwrap().c, 'B');
        assert_eq!(grid.get(1, 0).unwrap().c, 'C');
        assert_eq!(grid.get(2, 0).unwrap().c, ' ');
    }

    #[test]
    fn test_grid_resize() {
        let mut grid = Grid::new(24, 80);
        grid.set(0, 0, Cell::new('X'));

        grid.resize(30, 100);
        assert_eq!(grid.rows(), 30);
        assert_eq!(grid.cols(), 100);
        assert_eq!(grid.get(0, 0).unwrap().c, 'X');
    }

    #[test]
    fn test_row_clear() {
        let mut row = Row::new(10);
        row.set(0, Cell::new('A'));
        row.set(5, Cell::new('B'));

        row.clear_from(3);
        assert_eq!(row.get(0).unwrap().c, 'A');
        assert_eq!(row.get(5).unwrap().c, ' ');
    }

    #[test]
    fn test_dirty_tracking() {
        let mut grid = Grid::new(10, 10);
        grid.clear_all_dirty();

        assert!(!grid.has_dirty_rows());

        grid.set(5, 5, Cell::new('X'));
        assert!(grid.has_dirty_rows());

        let dirty: Vec<_> = grid.dirty_row_indices().collect();
        assert_eq!(dirty, vec![5]);
    }

    #[test]
    fn test_resize_preserves_content_basic() {
        // Simulate a real terminal scenario:
        // - Row 0: prompt + command "$ ls"
        // - Row 1: output "file1  file2"
        // - Row 2: new prompt "$ "
        // - Rows 3+: empty
        let mut grid = Grid::new(24, 80);

        // Write prompt and command on row 0
        for (i, c) in "$ ls".chars().enumerate() {
            grid.set(0, i, Cell::new(c));
        }

        // Write output on row 1
        for (i, c) in "file1  file2".chars().enumerate() {
            grid.set(1, i, Cell::new(c));
        }

        // Write new prompt on row 2
        for (i, c) in "$ ".chars().enumerate() {
            grid.set(2, i, Cell::new(c));
        }

        // Cursor at end of prompt on row 2
        let cursor_row = 2;
        let cursor_col = 2;

        // Resize to smaller width (like a vertical split)
        let (new_cursor_row, new_cursor_col) =
            grid.resize_with_reflow(24, 40, cursor_row, cursor_col);

        // Content should still be there
        let row0_text: String = (0..10)
            .filter_map(|c| grid.get(0, c).map(|cell| cell.c))
            .collect::<String>()
            .trim_end()
            .to_string();
        assert!(
            row0_text.starts_with("$ ls"),
            "Row 0 should have '$ ls', got '{}'",
            row0_text
        );

        let row1_text: String = (0..20)
            .filter_map(|c| grid.get(1, c).map(|cell| cell.c))
            .collect::<String>()
            .trim_end()
            .to_string();
        assert!(
            row1_text.starts_with("file1"),
            "Row 1 should have 'file1...', got '{}'",
            row1_text
        );

        let row2_text: String = (0..10)
            .filter_map(|c| grid.get(2, c).map(|cell| cell.c))
            .collect::<String>()
            .trim_end()
            .to_string();
        assert!(
            row2_text.starts_with("$"),
            "Row 2 should have prompt '$', got '{}'",
            row2_text
        );

        // Cursor should still be on row 2
        assert_eq!(
            new_cursor_row, 2,
            "Cursor should stay on row 2, got {}",
            new_cursor_row
        );
        assert_eq!(
            new_cursor_col, 2,
            "Cursor should stay at col 2, got {}",
            new_cursor_col
        );
    }

    #[test]
    fn test_resize_with_reflow_width_decrease() {
        // Create a grid with content that spans 80 columns
        let mut grid = Grid::new(24, 80);

        // Write "Hello World" spanning first row
        for (i, c) in "Hello World - this is a long line that will need to wrap when we shrink"
            .chars()
            .enumerate()
        {
            if i < 80 {
                grid.set(0, i, Cell::new(c));
            }
        }

        // Cursor at end of content
        let cursor_row = 0;
        let cursor_col = 70;

        // Resize to 40 columns - line should wrap
        let (new_row, new_col) = grid.resize_with_reflow(24, 40, cursor_row, cursor_col);

        assert_eq!(grid.cols(), 40);

        // Content should now span 2 rows (70 chars / 40 cols = ~2 rows)
        // First row should have first 40 chars
        let first_row_text: String = (0..40)
            .filter_map(|c| grid.get(0, c).map(|cell| cell.c))
            .collect();
        assert!(
            first_row_text.starts_with("Hello World"),
            "First row should start with 'Hello World', got '{}'",
            first_row_text
        );

        // First row should be marked as wrapped
        assert!(
            grid.row(0).unwrap().is_wrapped(),
            "First row should be marked as wrapped"
        );

        // Cursor should have moved to second row since it was at col 70
        // 70 / 40 = 1 (row 1), 70 % 40 = 30 (col 30)
        assert_eq!(new_row, 1, "Cursor should be on row 1 after reflow");
        assert_eq!(new_col, 30, "Cursor should be at col 30 after reflow");
    }

    #[test]
    fn test_resize_with_reflow_width_increase() {
        // Create a grid with wrapped content
        let mut grid = Grid::new(24, 40);

        // Write content that spans 2 rows when width is 40
        let text = "Hello World - this is a long line of text";
        for (i, c) in text.chars().enumerate() {
            let row = i / 40;
            let col = i % 40;
            if row < 24 {
                grid.set(row, col, Cell::new(c));
            }
        }
        // Mark first row as wrapped (soft wrap)
        grid.row_mut(0).unwrap().set_wrapped(true);

        // Cursor at beginning of second row
        let cursor_row = 1;
        let cursor_col = 0;

        // Resize to 80 columns - lines should unwrap
        let (new_row, new_col) = grid.resize_with_reflow(24, 80, cursor_row, cursor_col);

        assert_eq!(grid.cols(), 80);

        // Content should now fit on one row
        let first_row_text: String = (0..text.len())
            .filter_map(|c| grid.get(0, c).map(|cell| cell.c))
            .collect();
        assert_eq!(
            first_row_text, text,
            "Content should be unwrapped onto single row"
        );

        // First row should NOT be wrapped anymore
        assert!(
            !grid.row(0).unwrap().is_wrapped(),
            "First row should not be wrapped after width increase"
        );

        // Cursor was at beginning of row 1 (offset 40 in logical line)
        // After unwrap, should be at row 0, col 40
        assert_eq!(new_row, 0, "Cursor should be on row 0 after unwrap");
        assert_eq!(new_col, 40, "Cursor should be at col 40 after unwrap");
    }
}
