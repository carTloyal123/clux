//! Shared helpers for the storage invariant tests.
//!
//! Public surface only - no grid or scrollback internals - so these stay
//! meaningful across the paged-buffer migration (docs/PAGED_BUFFER.md).

#![allow(dead_code)]

use clux::terminal::Terminal;

/// Feed bytes through a real VTE parser, as the server does.
pub fn feed(term: &mut Terminal, bytes: &[u8]) {
    let mut parser = vte::Parser::new();
    parser.advance(term, bytes);
}

/// Text of a viewport row, trailing blanks trimmed.
pub fn row_text(term: &Terminal, row: u16) -> String {
    term.view_row(row)
        .cells
        .iter()
        .map(|c| c.c)
        .collect::<String>()
        .trim_end()
        .to_string()
}

/// The whole viewport, top to bottom.
pub fn viewport(term: &Terminal) -> Vec<String> {
    (0..term.rows() as u16).map(|r| row_text(term, r)).collect()
}

/// Print `count` numbered lines through the parser.
pub fn print_lines(term: &mut Terminal, count: usize) {
    for i in 0..count {
        feed(term, format!("line {}\r\n", i).as_bytes());
    }
}

pub mod harness;
pub mod ssh;
pub mod ssh_env;
pub mod ssh_fixtures;
