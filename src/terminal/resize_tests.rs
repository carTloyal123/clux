//! Resize and reflow tests.

use super::test_support::*;
use super::*;

#[test]
fn test_resize() {
    let mut term = Terminal::new(24, 80);
    term.put_char('A');
    term.cursor.row = 10;
    term.cursor.col = 40;

    term.resize(48, 120);
    assert_eq!(term.rows(), 48);
    assert_eq!(term.cols(), 120);
    // Cursor should be preserved
    assert_eq!(term.cursor.row, 10);
    assert_eq!(term.cursor.col, 40);
}

#[test]
fn test_scroll_offset_preserved_on_resize() {
    let mut term = Terminal::new(24, 80);

    // Fill terminal with content and force scrollback by going past the bottom
    // First fill the screen
    for row in 0..24 {
        term.cursor.row = row;
        term.cursor.col = 0;
        for c in format!("Line {}", row).chars() {
            term.put_char(c);
        }
    }

    // Now add more lines to push content into scrollback
    // Each linefeed at row 23 will scroll content up
    for i in 0..10 {
        term.cursor.row = 23;
        term.cursor.col = 0;
        term.linefeed(); // This pushes row 0 to scrollback
        for c in format!("New line {}", i).chars() {
            term.put_char(c);
        }
    }

    // Verify scrollback has content
    assert!(term.history_rows() >= 5, "Scrollback should have content");

    // Scroll back into history (positive = older)
    term.scroll_view(5);
    assert_eq!(
        term.buffer.scroll_offset(),
        5,
        "Should be scrolled up 5 lines"
    );

    // Resize (same size to test offset preservation)
    term.resize(24, 80);

    // Scroll offset should be preserved
    assert_eq!(
        term.buffer.scroll_offset(),
        5,
        "Scroll offset should be preserved after resize"
    );
}

/// Fill the scrollback by pushing `count` lines out of the top of the grid.#[test]
fn test_resize_shrink_preserves_content_via_scrollback() {
    let mut term = Terminal::new(24, 80);

    // Fill terminal with content - put cursor at row 20
    for row in 0..21 {
        term.cursor.row = row;
        term.cursor.col = 0;
        for c in format!("Line {}", row).chars() {
            term.put_char(c);
        }
    }
    term.cursor.row = 20;
    term.cursor.col = 7; // After "Line 20"

    // Verify initial state
    assert_eq!(term.history_rows(), 0, "No scrollback yet");
    assert_eq!(term.cursor.row, 20);

    // Now resize to only 10 rows - cursor at row 20 would be out of bounds
    term.resize(10, 80);

    // Cursor should now be within bounds
    assert!(
        term.cursor.row < 10,
        "Cursor row {} should be < 10 after resize",
        term.cursor.row
    );

    // Content should have been pushed to scrollback
    assert!(
        term.history_rows() > 0,
        "Scrollback should have content after shrinking with cursor below new height"
    );

    // The content from the top rows should now be in scrollback
    // We scrolled up (20 - 10 + 1 = 11) rows to bring cursor into view
    assert!(
        term.history_rows() >= 11,
        "Scrollback should have at least 11 lines, got {}",
        term.history_rows()
    );
}

#[test]
fn test_resize_shrink_cursor_in_bounds_no_scroll() {
    let mut term = Terminal::new(24, 80);

    // Put content at the top, cursor at row 5
    for row in 0..6 {
        term.cursor.row = row;
        term.cursor.col = 0;
        for c in format!("Line {}", row).chars() {
            term.put_char(c);
        }
    }
    term.cursor.row = 5;
    term.cursor.col = 7;

    // Verify initial state
    assert_eq!(term.history_rows(), 0, "No scrollback yet");

    // Resize to 10 rows - cursor at row 5 is still within bounds
    term.resize(10, 80);

    // Cursor should remain at row 5
    assert_eq!(term.cursor.row, 5, "Cursor should stay at row 5");

    // No scrollback needed since cursor was in bounds
    assert_eq!(
        term.history_rows(),
        0,
        "No scrollback needed when cursor stays in bounds"
    );

    // Content should still be there
    assert_eq!(
        term.buffer.cell(0, 0).map(|c| c.c),
        Some('L'),
        "Content at row 0 preserved"
    );
}
