//! Shared terminal-test helpers.

use super::super::*;

pub(super) fn feed(term: &mut Terminal, bytes: &[u8]) {
    let mut parser = vte::Parser::new();
    parser.advance(term, bytes);
}

/// URL of the hyperlink on the cell at (row, col), if any.
pub(super) fn link_at(term: &Terminal, row: usize, col: usize) -> Option<&str> {
    let id = term.buffer.cell(row, col)?.hyperlink?;
    term.hyperlinks.get(id)
}

pub(super) fn fill_scrollback(term: &mut Terminal, count: usize) {
    for i in 0..count {
        term.cursor.row = term.rows() - 1;
        term.cursor.col = 0;
        term.linefeed();
        for c in format!("line {}", i).chars() {
            term.put_char(c);
        }
    }
}
