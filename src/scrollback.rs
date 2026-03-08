//! Scrollback buffer for terminal history.
//!
//! Implements a circular buffer that stores lines scrolled off the top
//! of the visible terminal area. Optimized for memory efficiency and
//! fast access patterns.

// Many methods will be used in later phases (selection, search)
#![allow(dead_code)]

use std::collections::VecDeque;

use crate::cell::Cell;

/// A line stored in the scrollback buffer.
#[derive(Clone, Debug)]
pub struct ScrollbackLine {
    /// Cells in this line.
    cells: Vec<Cell>,
    /// Whether this line wrapped from the previous line.
    wrapped: bool,
}

impl ScrollbackLine {
    /// Create a new scrollback line from cells.
    pub fn new(cells: Vec<Cell>, wrapped: bool) -> Self {
        Self { cells, wrapped }
    }

    /// Get the cells in this line.
    pub fn cells(&self) -> &[Cell] {
        &self.cells
    }

    /// Get a cell at the given column.
    pub fn get(&self, col: usize) -> Option<&Cell> {
        self.cells.get(col)
    }

    /// Get the number of cells in this line.
    pub fn len(&self) -> usize {
        self.cells.len()
    }

    /// Check if this line is empty.
    pub fn is_empty(&self) -> bool {
        self.cells.is_empty()
    }

    /// Check if this line wrapped from the previous line.
    pub fn is_wrapped(&self) -> bool {
        self.wrapped
    }

    /// Extract text content from this line.
    pub fn to_string(&self) -> String {
        self.cells
            .iter()
            .map(|c| c.c)
            .collect::<String>()
            .trim_end()
            .to_string()
    }
}

/// Scrollback buffer storing terminal history.
///
/// Uses a VecDeque as a circular buffer for efficient push/pop operations.
/// Index 0 is the most recent line (closest to visible area).
#[derive(Debug)]
pub struct Scrollback {
    /// Lines in the buffer (index 0 = most recent).
    lines: VecDeque<ScrollbackLine>,
    /// Maximum number of lines to store.
    max_lines: usize,
}

impl Scrollback {
    /// Create a new scrollback buffer with the given capacity.
    pub fn new(max_lines: usize) -> Self {
        Self {
            lines: VecDeque::with_capacity(max_lines.min(1000)), // Pre-allocate reasonably
            max_lines,
        }
    }

    /// Push a line into the scrollback buffer.
    /// If the buffer is full, the oldest line is discarded.
    pub fn push(&mut self, cells: Vec<Cell>, wrapped: bool) {
        if self.lines.len() >= self.max_lines {
            self.lines.pop_back(); // Remove oldest
        }
        self.lines.push_front(ScrollbackLine::new(cells, wrapped));
    }

    /// Get a line by offset from the visible area.
    /// Offset 0 = most recent line (just scrolled off).
    pub fn get(&self, offset: usize) -> Option<&ScrollbackLine> {
        self.lines.get(offset)
    }

    /// Get the total number of lines in the buffer.
    pub fn len(&self) -> usize {
        self.lines.len()
    }

    /// Check if the buffer is empty.
    pub fn is_empty(&self) -> bool {
        self.lines.is_empty()
    }

    /// Get the maximum capacity.
    pub fn capacity(&self) -> usize {
        self.max_lines
    }

    /// Clear the scrollback buffer.
    pub fn clear(&mut self) {
        self.lines.clear();
    }

    /// Iterate over lines from most recent to oldest.
    pub fn iter(&self) -> impl Iterator<Item = &ScrollbackLine> {
        self.lines.iter()
    }

    /// Extract text from a range of lines.
    /// Returns lines joined with newlines, respecting line wrapping.
    pub fn extract_text(&self, start_offset: usize, end_offset: usize) -> String {
        let mut result = String::new();
        let end = end_offset.min(self.lines.len());
        let mut prev_was_wrapped = false;

        for i in start_offset..end {
            if let Some(line) = self.lines.get(i) {
                // Add newline before this line ONLY if the previous line (lower index)
                // was NOT wrapped (i.e., it didn't continue from this line)
                if !result.is_empty() && !prev_was_wrapped {
                    result.push('\n');
                }
                result.push_str(&line.to_string());
                prev_was_wrapped = line.is_wrapped();
            }
        }

        result
    }

    /// Search for a pattern in the scrollback buffer.
    /// Returns offsets of lines containing the pattern.
    pub fn search(&self, pattern: &str) -> Vec<usize> {
        self.lines
            .iter()
            .enumerate()
            .filter(|(_, line)| {
                let text = line.to_string();
                text.contains(pattern)
            })
            .map(|(i, _)| i)
            .collect()
    }
}

impl Default for Scrollback {
    fn default() -> Self {
        Self::new(10_000) // Default 10k lines
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_cells(s: &str) -> Vec<Cell> {
        s.chars()
            .map(|c| Cell {
                c,
                ..Cell::default()
            })
            .collect()
    }

    #[test]
    fn test_scrollback_push_get() {
        let mut sb = Scrollback::new(100);
        sb.push(make_cells("line 1"), false);
        sb.push(make_cells("line 2"), false);
        sb.push(make_cells("line 3"), false);

        assert_eq!(sb.len(), 3);
        assert_eq!(sb.get(0).unwrap().to_string(), "line 3");
        assert_eq!(sb.get(1).unwrap().to_string(), "line 2");
        assert_eq!(sb.get(2).unwrap().to_string(), "line 1");
    }

    #[test]
    fn test_scrollback_capacity() {
        let mut sb = Scrollback::new(3);
        sb.push(make_cells("line 1"), false);
        sb.push(make_cells("line 2"), false);
        sb.push(make_cells("line 3"), false);
        sb.push(make_cells("line 4"), false);

        assert_eq!(sb.len(), 3);
        // Oldest line (line 1) should be gone
        assert_eq!(sb.get(0).unwrap().to_string(), "line 4");
        assert_eq!(sb.get(2).unwrap().to_string(), "line 2");
    }

    #[test]
    fn test_extract_text() {
        let mut sb = Scrollback::new(100);
        sb.push(make_cells("line 1"), false);
        sb.push(make_cells("line 2"), false);
        sb.push(make_cells("line 3"), false);

        let text = sb.extract_text(0, 3);
        assert_eq!(text, "line 3\nline 2\nline 1");
    }

    #[test]
    fn test_wrapped_lines() {
        let mut sb = Scrollback::new(100);
        sb.push(make_cells("start of long line"), false);
        sb.push(make_cells("continuation"), true); // wrapped

        let text = sb.extract_text(0, 2);
        // Wrapped lines should not have newline between them
        assert_eq!(text, "continuationstart of long line");
    }

    #[test]
    fn test_search() {
        let mut sb = Scrollback::new(100);
        sb.push(make_cells("hello world"), false);
        sb.push(make_cells("foo bar"), false);
        sb.push(make_cells("hello again"), false);

        let results = sb.search("hello");
        assert_eq!(results, vec![0, 2]); // "hello again" at 0, "hello world" at 2
    }
}
