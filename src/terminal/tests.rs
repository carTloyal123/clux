//! Terminal parsing tests.

use super::test_support::*;
use super::*;
use crate::cell::ColorKind;

#[test]
fn test_terminal_creation() {
    let term = Terminal::new(24, 80);
    assert_eq!(term.rows(), 24);
    assert_eq!(term.cols(), 80);
    assert_eq!(term.cursor.row, 0);
    assert_eq!(term.cursor.col, 0);
}

#[test]
fn test_put_char() {
    let mut term = Terminal::new(24, 80);
    term.put_char('A');
    assert_eq!(term.buffer.cell(0, 0).unwrap().c, 'A');
    assert_eq!(term.cursor.col, 1);
}

#[test]
fn test_linefeed() {
    let mut term = Terminal::new(24, 80);
    term.cursor.row = 5;
    term.linefeed();
    assert_eq!(term.cursor.row, 6);
}

#[test]
fn test_cursor_movement_via_parser() {
    let mut term = Terminal::new(24, 80);
    let mut parser = vte::Parser::new();

    term.cursor.row = 10;
    term.cursor.col = 10;

    // CSI A = cursor up: ESC [ A
    let seq = b"\x1b[A";
    parser.advance(&mut term, seq);
    assert_eq!(term.cursor.row, 9);
}

/// Feed bytes through a real VTE parser, as the server does.#[test]
fn test_osc8_hyperlink_marks_cells() {
    let mut term = Terminal::new(24, 80);
    feed(
        &mut term,
        b"\x1b]8;;https://example.com\x1b\\link\x1b]8;;\x1b\\ plain",
    );

    assert_eq!(link_at(&term, 0, 0), Some("https://example.com"));
    assert_eq!(link_at(&term, 0, 3), Some("https://example.com"));
    assert_eq!(link_at(&term, 0, 4), None, "closed link leaked");
}

#[test]
fn test_osc8_keeps_semicolons_in_url() {
    let mut term = Terminal::new(24, 80);
    // ';' is legal in a URI; the parser splits on it, so it must be rejoined.
    feed(
        &mut term,
        b"\x1b]8;;https://example.com/a;b=1;c=2\x1b\\x\x1b]8;;\x1b\\",
    );

    assert_eq!(link_at(&term, 0, 0), Some("https://example.com/a;b=1;c=2"));
}

#[test]
fn test_osc8_rejects_control_characters_in_url() {
    let mut term = Terminal::new(24, 80);
    // A URL is re-emitted inside OSC 8, so control bytes must never survive.
    feed(&mut term, b"\x1b]8;;https://a.io/\x07x\x1b\\y");

    let url = link_at(&term, 0, 0).expect("link");
    assert!(
        !url.chars().any(|c| c.is_control()),
        "control char in {url:?}"
    );
}

#[test]
fn test_sgr() {
    let mut term = Terminal::new(24, 80);

    // Set bold
    term.handle_sgr(&[1]);
    assert!(term.flags.contains(CellFlags::BOLD));

    // Reset
    term.handle_sgr(&[0]);
    assert!(!term.flags.contains(CellFlags::BOLD));

    // Set foreground color
    term.handle_sgr(&[31]);
    assert_eq!(term.fg.kind, ColorKind::Indexed);
}
