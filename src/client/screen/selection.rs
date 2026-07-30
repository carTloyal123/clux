//! Mouse selection over the composited screen.

use super::{PaneSelection, ScreenBuffer};

use crate::cell::{Cell, CellFlags};
use crate::protocol::PaneLayout;
use crate::selection::{Point, Selection, SelectionMode};

impl ScreenBuffer {
    /// Start a selection at a screen position. Does nothing outside a pane (on a
    /// divider, or past the layout), which is what makes stray clicks harmless.
    ///
    /// Returns whether a selection was started.
    pub fn begin_selection(&mut self, row: usize, col: usize, mode: SelectionMode) -> bool {
        let Some(pane) = self.pane_at(row, col).cloned() else {
            self.selection = None;
            return false;
        };

        self.selection = Some(PaneSelection {
            pane_id: pane.pane_id,
            selection: Selection::start(Point::new(row as i32, col), mode),
        });
        true
    }
    /// Extend the active selection to a screen position, clamped into the pane
    /// the selection started in.
    pub fn extend_selection(&mut self, row: usize, col: usize) -> bool {
        let Some(active) = &self.selection else {
            return false;
        };
        let Some(pane) = self.pane_by_id(active.pane_id).cloned() else {
            return false;
        };

        let point = Self::clamp_to_pane(&pane, row, col);
        if let Some(active) = &mut self.selection {
            active.selection.extend(point);
        }
        true
    }
    /// Drop any active selection.
    pub fn clear_selection(&mut self) {
        self.selection = None;
    }
    /// Whether a selection is active and covers at least one cell.
    pub fn has_selection(&self) -> bool {
        self.selection
            .as_ref()
            .map(|active| active.selection.active)
            .unwrap_or(false)
    }
    /// The selected text, or `None` when nothing is selected.
    ///
    /// Rows that soft-wrap are joined without a newline, so copying a long path
    /// or URL out of a pane gives back one unbroken string. Hard line ends have
    /// their trailing blank cells trimmed.
    pub fn selected_text(&self) -> Option<String> {
        let active = self.selection.as_ref()?;
        if !active.selection.active {
            return None;
        }

        let pane = self.pane_by_id(active.pane_id)?;
        let selection = &active.selection;
        let (start, end) = selection.normalized();

        let pane_first_row = pane.y as usize;
        let pane_last_row = (pane.y as usize + pane.height as usize).min(self.rows);
        let first_row = (start.line.max(0) as usize).max(pane_first_row);
        let last_row = (end.line.max(0) as usize + 1).min(pane_last_row);

        let mut text = String::new();
        let mut pending_newline = false;

        for row in first_row..last_row {
            // Selected cells on a row are contiguous for every mode we support,
            // and asking the selection itself keeps the copied text identical to
            // what is highlighted.
            let mut segment = String::new();
            let mut last_col = None;

            for col in self.pane_columns(pane) {
                if selection.contains(Point::new(row as i32, col)) {
                    segment.push(self.cells[row][col].c);
                    last_col = Some(col);
                }
            }

            if segment.is_empty() {
                continue;
            }

            if pending_newline {
                text.push('\n');
            }

            // Only a hard line end gets trimmed; trailing blanks in the middle of
            // a wrapped line are real content as far as the join is concerned.
            let continues = last_col
                .map(|col| self.row_continues[row][col])
                .unwrap_or(false)
                && selection.mode != SelectionMode::Block;

            if continues {
                text.push_str(&segment);
                pending_newline = false;
            } else {
                text.push_str(segment.trim_end());
                pending_newline = true;
            }
        }

        Some(text)
    }
    /// The pane covering a screen position, if any.
    fn pane_at(&self, row: usize, col: usize) -> Option<&PaneLayout> {
        self.layout.as_ref()?.panes.iter().find(|pane| {
            row >= pane.y as usize
                && row < pane.y as usize + pane.height as usize
                && col >= pane.x as usize
                && col < pane.x as usize + pane.width as usize
        })
    }
    fn pane_by_id(&self, pane_id: u32) -> Option<&PaneLayout> {
        self.layout
            .as_ref()?
            .panes
            .iter()
            .find(|pane| pane.pane_id == pane_id)
    }
    /// Screen columns belonging to a pane, clipped to the buffer.
    fn pane_columns(&self, pane: &PaneLayout) -> std::ops::Range<usize> {
        let start = pane.x as usize;
        let end = (start + pane.width as usize).min(self.cols);
        start..end.max(start)
    }
    /// Clamp a screen position into a pane's rectangle.
    fn clamp_to_pane(pane: &PaneLayout, row: usize, col: usize) -> Point {
        let last_row = (pane.y as usize + pane.height as usize).saturating_sub(1);
        let last_col = (pane.x as usize + pane.width as usize).saturating_sub(1);

        Point::new(
            row.clamp(pane.y as usize, last_row) as i32,
            col.clamp(pane.x as usize, last_col),
        )
    }
    /// A copy of `row` with selected cells inverted, or `None` if this row has no
    /// selected cells.
    pub(super) fn highlight_selection(&self, row_idx: usize, row: &[Cell]) -> Option<Vec<Cell>> {
        let active = self.selection.as_ref()?;
        if !active.selection.active {
            return None;
        }

        let pane = self.pane_by_id(active.pane_id)?;
        let mut highlighted: Option<Vec<Cell>> = None;

        for col in self.pane_columns(pane) {
            if !active.selection.contains(Point::new(row_idx as i32, col)) {
                continue;
            }
            let cells = highlighted.get_or_insert_with(|| row.to_vec());
            if let Some(cell) = cells.get_mut(col) {
                cell.flags.insert(CellFlags::INVERSE);
            }
        }

        highlighted
    }
}
