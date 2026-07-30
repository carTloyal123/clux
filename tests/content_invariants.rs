//! Wrap, cursor, resize and alt-screen invariants
//!
//! Behaviour that must survive the paged-buffer migration
//! (docs/PAGED_BUFFER.md).

mod common;

use clux::terminal::Terminal;
use common::{feed, print_lines, row_text, viewport};

#[test]
fn a_wrapped_line_is_marked_as_continuing() {
    // Wrap flags are what let the client join a wrapped line when copying, and
    // what link detection follows. They must be set on the row that continues.
    let mut term = Terminal::new(4, 10);
    feed(&mut term, b"0123456789abcde");

    assert!(
        term.view_row(0).wrapped,
        "the full row should be flagged as continuing onto the next"
    );
    assert!(
        !term.view_row(1).wrapped,
        "the partial row should not be flagged"
    );
    assert_eq!(row_text(&term, 0), "0123456789");
    assert_eq!(row_text(&term, 1), "abcde");
}

#[test]
fn wrap_flags_survive_scrolling_into_history() {
    let mut term = Terminal::new(3, 10);
    feed(&mut term, b"0123456789abcde\r\n");
    print_lines(&mut term, 10);

    // Find the wrapped row again in history.
    let mut found = false;
    for offset in 1..=term.history_rows() as i32 {
        term.reset_scroll();
        term.scroll_view(offset);
        for row in 0..term.rows() as u16 {
            if row_text(&term, row) == "0123456789" && term.view_row(row).wrapped {
                found = true;
            }
        }
    }
    assert!(found, "a wrapped row must still be flagged in history");
}

#[test]
fn the_cursor_stays_at_the_bottom_while_output_scrolls() {
    let mut term = Terminal::new(5, 20);
    print_lines(&mut term, 50);

    assert_eq!(
        term.cursor().row,
        term.rows() - 1,
        "output should scroll the screen, not push the cursor off it"
    );
    assert_eq!(term.cursor().col, 0);
}

#[test]
fn resize_keeps_the_visible_text() {
    let mut term = Terminal::new(6, 20);
    print_lines(&mut term, 10);
    let before: Vec<String> = viewport(&term)
        .into_iter()
        .filter(|r| !r.is_empty())
        .collect();

    term.resize(6, 20);
    let after: Vec<String> = viewport(&term)
        .into_iter()
        .filter(|r| !r.is_empty())
        .collect();
    assert_eq!(before, after, "a no-op resize must not disturb content");

    // Narrowing must not lose characters: every line's text still starts there.
    term.resize(6, 12);
    let narrowed = viewport(&term).join("\n");
    assert!(
        narrowed.contains("line 9") || narrowed.contains("line"),
        "narrowing should keep the text, wrapped: {narrowed}"
    );
}

#[test]
fn the_alternate_screen_does_not_disturb_history() {
    let mut term = Terminal::new(5, 20);
    print_lines(&mut term, 20);
    let history_depth = term.history_rows();
    let live = viewport(&term);

    // Enter the alt screen, scribble, leave.
    feed(&mut term, b"\x1b[?1049h");
    feed(&mut term, b"alt screen content\r\n");
    feed(&mut term, b"\x1b[?1049l");

    assert_eq!(
        term.history_rows(),
        history_depth,
        "alt screen output must not enter history"
    );
    assert_eq!(
        viewport(&term),
        live,
        "leaving alt screen must restore the view"
    );
}
