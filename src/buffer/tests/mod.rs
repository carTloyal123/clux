//! Buffer tests, by concern.

mod budget;
mod eviction;
mod reflow;
mod scrolling;
mod viewport;
mod writing;

use crate::buffer::Buffer;
use crate::cell::Cell;

/// Write text into an active row.
fn write_row(buffer: &mut Buffer, row: usize, text: &str) {
    for (col, c) in text.chars().enumerate() {
        buffer.set_cell(row, col, Cell::new(c));
    }
}

/// Text of a viewport row, trailing blanks trimmed.
fn viewport_text(buffer: &Buffer, row: usize) -> String {
    buffer
        .row_cells(row)
        .map(|(cells, _)| cells.iter().map(|c| c.c).collect::<String>())
        .unwrap_or_default()
        .trim_end()
        .to_string()
}

/// The whole viewport, top to bottom.
fn viewport(buffer: &Buffer) -> Vec<String> {
    (0..buffer.screen_rows())
        .map(|r| viewport_text(buffer, r))
        .collect()
}

/// Print `count` numbered lines, scrolling as a terminal would.
fn print_lines(buffer: &mut Buffer, count: usize) {
    for i in 0..count {
        let last = buffer.screen_rows() - 1;
        write_row(buffer, last, &format!("line {}", i));
        buffer.scroll_up();
    }
}
