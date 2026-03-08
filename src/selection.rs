//! Text selection for copy/paste.
//!
//! Handles mouse-based text selection across the visible grid
//! and scrollback buffer.

// This module will be fully used in Phase 2 (mouse selection)
#![allow(dead_code)]

use crate::cell::Cell;
use crate::grid::Grid;
use crate::scrollback::Scrollback;

/// A point in the terminal (can be in visible area or scrollback).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Point {
    /// Line number. Negative = scrollback, positive = visible grid.
    /// -1 is the most recent scrollback line, 0 is the first visible line.
    pub line: i32,
    /// Column number (0-indexed).
    pub col: usize,
}

impl Point {
    pub fn new(line: i32, col: usize) -> Self {
        Self { line, col }
    }

    /// Check if this point is in the scrollback area.
    pub fn is_scrollback(&self) -> bool {
        self.line < 0
    }
}

impl PartialOrd for Point {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for Point {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        match self.line.cmp(&other.line) {
            std::cmp::Ordering::Equal => self.col.cmp(&other.col),
            ord => ord,
        }
    }
}

/// Selection mode.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SelectionMode {
    /// Character-wise selection (click and drag).
    Normal,
    /// Word selection (double-click).
    Word,
    /// Line selection (triple-click).
    Line,
    /// Block/rectangular selection (Alt+drag).
    Block,
}

/// Text selection state.
#[derive(Clone, Debug)]
pub struct Selection {
    /// Starting point (anchor).
    pub start: Point,
    /// Ending point (moves with cursor).
    pub end: Point,
    /// Selection mode.
    pub mode: SelectionMode,
    /// Whether selection is active.
    pub active: bool,
}

impl Selection {
    /// Start a new selection at the given point.
    pub fn start(point: Point, mode: SelectionMode) -> Self {
        Self {
            start: point,
            end: point,
            mode,
            active: true,
        }
    }

    /// Extend the selection to a new point.
    pub fn extend(&mut self, point: Point) {
        self.end = point;
    }

    /// Get the normalized selection (start <= end).
    pub fn normalized(&self) -> (Point, Point) {
        if self.start <= self.end {
            (self.start, self.end)
        } else {
            (self.end, self.start)
        }
    }

    /// Check if a point is within the selection.
    pub fn contains(&self, point: Point) -> bool {
        let (start, end) = self.normalized();

        match self.mode {
            SelectionMode::Block => {
                // Rectangular selection
                let min_col = start.col.min(end.col);
                let max_col = start.col.max(end.col);
                point.line >= start.line
                    && point.line <= end.line
                    && point.col >= min_col
                    && point.col <= max_col
            }
            _ => {
                // Linear selection
                if point.line < start.line || point.line > end.line {
                    return false;
                }
                if point.line == start.line && point.line == end.line {
                    // Same line
                    point.col >= start.col && point.col <= end.col
                } else if point.line == start.line {
                    point.col >= start.col
                } else if point.line == end.line {
                    point.col <= end.col
                } else {
                    true // Middle lines are fully selected
                }
            }
        }
    }

    /// Extract selected text from grid and scrollback.
    pub fn extract_text(
        &self,
        grid: &Grid,
        scrollback: &Scrollback,
        scroll_offset: usize,
    ) -> String {
        let (start, end) = self.normalized();
        let mut result = String::new();
        let cols = grid.cols();

        for line in start.line..=end.line {
            if !result.is_empty() && self.mode != SelectionMode::Block {
                result.push('\n');
            }

            let (start_col, end_col) = match self.mode {
                SelectionMode::Block => (start.col.min(end.col), start.col.max(end.col)),
                SelectionMode::Line => (0, cols.saturating_sub(1)),
                _ => {
                    let sc = if line == start.line { start.col } else { 0 };
                    let ec = if line == end.line {
                        end.col
                    } else {
                        cols.saturating_sub(1)
                    };
                    (sc, ec)
                }
            };

            // Get cells for this line
            let line_text = if line < 0 {
                // Scrollback
                let sb_offset = (-line - 1) as usize + scroll_offset;
                if let Some(sb_line) = scrollback.get(sb_offset) {
                    extract_cells_text(sb_line.cells(), start_col, end_col)
                } else {
                    String::new()
                }
            } else {
                // Visible grid
                let row = line as usize;
                if let Some(grid_row) = grid.row(row) {
                    let cells: Vec<Cell> =
                        (0..cols).filter_map(|c| grid_row.get(c).copied()).collect();
                    extract_cells_text(&cells, start_col, end_col)
                } else {
                    String::new()
                }
            };

            result.push_str(&line_text);
        }

        // Trim trailing whitespace from each line for cleaner output
        result
            .lines()
            .map(|l| l.trim_end())
            .collect::<Vec<_>>()
            .join("\n")
    }

    /// Clear the selection.
    pub fn clear(&mut self) {
        self.active = false;
    }

    /// Check if selection is empty (start == end).
    pub fn is_empty(&self) -> bool {
        self.start == self.end
    }
}

/// Extract text from a slice of cells between columns.
fn extract_cells_text(cells: &[Cell], start_col: usize, end_col: usize) -> String {
    cells
        .iter()
        .skip(start_col)
        .take(end_col - start_col + 1)
        .map(|c| c.c)
        .collect()
}

/// Detect word boundaries for double-click selection.
pub fn find_word_bounds(cells: &[Cell], col: usize) -> (usize, usize) {
    if col >= cells.len() {
        return (col, col);
    }

    let is_word_char = |c: char| c.is_alphanumeric() || c == '_' || c == '-';
    let target_char = cells[col].c;

    // If clicking on whitespace, select just that
    if target_char.is_whitespace() {
        return (col, col);
    }

    let is_word = is_word_char(target_char);

    // Find start of word
    let mut start = col;
    while start > 0 {
        let c = cells[start - 1].c;
        if is_word {
            if !is_word_char(c) {
                break;
            }
        } else {
            // For punctuation, only select contiguous same chars
            if c != target_char {
                break;
            }
        }
        start -= 1;
    }

    // Find end of word
    let mut end = col;
    while end + 1 < cells.len() {
        let c = cells[end + 1].c;
        if is_word {
            if !is_word_char(c) {
                break;
            }
        } else {
            if c != target_char {
                break;
            }
        }
        end += 1;
    }

    (start, end)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_point_ordering() {
        let p1 = Point::new(0, 5);
        let p2 = Point::new(0, 10);
        let p3 = Point::new(1, 0);

        assert!(p1 < p2);
        assert!(p2 < p3);
        assert!(p1 < p3);
    }

    #[test]
    fn test_selection_contains() {
        let sel = Selection {
            start: Point::new(0, 5),
            end: Point::new(2, 10),
            mode: SelectionMode::Normal,
            active: true,
        };

        assert!(sel.contains(Point::new(0, 5)));
        assert!(sel.contains(Point::new(1, 0)));
        assert!(sel.contains(Point::new(2, 10)));
        assert!(!sel.contains(Point::new(0, 4)));
        assert!(!sel.contains(Point::new(2, 11)));
        assert!(!sel.contains(Point::new(3, 0)));
    }

    #[test]
    fn test_find_word_bounds() {
        fn make_cells(s: &str) -> Vec<Cell> {
            s.chars()
                .map(|c| Cell {
                    c,
                    ..Cell::default()
                })
                .collect()
        }

        let cells = make_cells("hello world test");

        // Click on 'e' in "hello"
        assert_eq!(find_word_bounds(&cells, 1), (0, 4));

        // Click on 'w' in "world"
        assert_eq!(find_word_bounds(&cells, 6), (6, 10));

        // Click on space
        assert_eq!(find_word_bounds(&cells, 5), (5, 5));
    }
}
