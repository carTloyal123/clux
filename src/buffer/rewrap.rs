//! Re-wrapping content at a new width.
//!
//! Content is split back into logical lines - what the text would be with no
//! wrapping at all - and re-emitted at the new width. Positions that must survive,
//! the cursor and the viewport, are remapped through [`Anchors`].

use super::pin::Viewport;
use super::Buffer;
use crate::cell::Cell;

/// A logical line: content as it would be with no wrapping at all.
pub(super) struct LogicalLine {
    cells: Vec<Cell>,
    /// Storage index this line started at, before the reflow.
    old_start: usize,
}

impl Buffer {
    /// Split stored rows into logical lines, dropping padding at line ends.
    pub(super) fn logical_lines(&self) -> Vec<LogicalLine> {
        let mut lines = Vec::new();
        let mut current = LogicalLine {
            cells: Vec::new(),
            old_start: 0,
        };
        let mut open = false;

        for index in 0..self.stored_rows {
            let Some((cells, wrapped)) = self.row_at(index) else {
                continue;
            };
            if !open {
                current.old_start = index;
                open = true;
            }
            current.cells.extend_from_slice(cells);

            if !wrapped {
                while current.cells.last().map(|c| c.is_empty()).unwrap_or(false) {
                    current.cells.pop();
                }
                lines.push(std::mem::replace(
                    &mut current,
                    LogicalLine {
                        cells: Vec::new(),
                        old_start: 0,
                    },
                ));
                open = false;
            }
        }

        if open {
            lines.push(current);
        }
        lines
    }

    /// Re-emit logical lines at a new width, returning where each line landed.
    pub(super) fn rebuild(
        &mut self,
        lines: &[LogicalLine],
        screen_rows: usize,
        cols: usize,
    ) -> Anchors {
        let mut anchors = Anchors {
            starts: Vec::with_capacity(lines.len()),
        };

        self.reset_storage(cols, screen_rows);

        for line in lines {
            anchors.starts.push((line.old_start, self.stored_rows));

            if line.cells.is_empty() {
                self.append_blank_row();
                continue;
            }

            let mut offset = 0;
            while offset < line.cells.len() {
                let end = (offset + cols).min(line.cells.len());
                let wrapped = end < line.cells.len();
                self.push_row(&line.cells[offset..end], wrapped);
                offset = end;
            }
        }

        while self.stored_rows < screen_rows {
            self.append_blank_row();
        }
        self.enforce_budget_now();
        anchors
    }

    /// Drop all storage and start numbering rows past everything written so far.
    pub(super) fn reset_storage(&mut self, cols: usize, screen_rows: usize) {
        self.first_abs += self.stored_rows as u64;
        self.pages.clear();
        self.front_skip = 0;
        self.stored_rows = 0;
        self.cols = cols;
        self.screen_rows = screen_rows;
        self.viewport = Viewport::Active;
    }
}

/// Where each logical line started before and after a reflow.
pub(super) struct Anchors {
    starts: Vec<(usize, usize)>,
}

impl Anchors {
    /// Map an old `(row, col)` to its new position.
    ///
    /// Both widths matter: the offset within the logical line is measured at the
    /// old width, and split again at the new one.
    pub(super) fn remap(
        &self,
        old_row: usize,
        old_col: usize,
        old_cols: usize,
        new_cols: usize,
    ) -> (usize, usize) {
        // The line containing this row is the last one that started at or before it.
        let Some(&(old_start, new_start)) = self
            .starts
            .iter()
            .rev()
            .find(|&&(old_start, _)| old_start <= old_row)
        else {
            return (old_row, old_col);
        };

        // Offset of the character within its logical line is preserved.
        let offset = (old_row - old_start) * old_cols + old_col;
        (new_start + offset / new_cols, offset % new_cols)
    }
}
