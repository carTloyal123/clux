//! Text selection geometry for copy.
//!
//! Coordinates are client screen coordinates: the client owns selection, because
//! it owns the composited screen and the mouse events. Text extraction lives with
//! the screen buffer (`client::screen`), which knows pane rects and soft wraps.

// Word/Line modes and find_word_bounds land with double- and triple-click.

use crate::cell::Cell;

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

    /// Clear the selection.
    pub fn clear(&mut self) {
        self.active = false;
    }

    /// Check if selection is empty (start == end).
    pub fn is_empty(&self) -> bool {
        self.start == self.end
    }
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
mod tests;
