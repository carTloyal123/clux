//! Terminal tests.

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

/// Feed bytes through a real VTE parser, as the server does.
fn feed(term: &mut Terminal, bytes: &[u8]) {
    let mut parser = vte::Parser::new();
    parser.advance(term, bytes);
}

/// URL of the hyperlink on the cell at (row, col), if any.
fn link_at(term: &Terminal, row: usize, col: usize) -> Option<&str> {
    let id = term.buffer.cell(row, col)?.hyperlink?;
    term.hyperlinks.get(id)
}

#[test]
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

/// Fill the scrollback by pushing `count` lines out of the top of the grid.
fn fill_scrollback(term: &mut Terminal, count: usize) {
    for i in 0..count {
        term.cursor.row = term.rows() - 1;
        term.cursor.col = 0;
        term.linefeed();
        for c in format!("line {}", i).chars() {
            term.put_char(c);
        }
    }
}

#[test]
fn test_scroll_view_clamps_to_recorded_history() {
    let mut term = Terminal::new(24, 80);
    fill_scrollback(&mut term, 10);

    // Cannot scroll past the oldest recorded line...
    term.scroll_view(1000);
    assert_eq!(term.buffer.scroll_offset(), term.history_rows());

    // ...nor forward past the live view.
    term.scroll_view(-1000);
    assert_eq!(term.buffer.scroll_offset(), 0);
    assert!(!term.is_scrolled());
}

#[test]
fn test_scroll_view_reports_whether_it_moved() {
    let mut term = Terminal::new(24, 80);
    fill_scrollback(&mut term, 4);

    assert!(term.scroll_view(2), "moving back should report a change");
    assert!(!term.scroll_view(0), "a zero move changes nothing");
    assert!(term.reset_scroll(), "returning to live is a change");
    assert!(!term.reset_scroll(), "already live, nothing to do");
}

#[test]
fn test_scrolled_view_stays_pinned_as_output_arrives() {
    let mut term = Terminal::new(24, 80);
    fill_scrollback(&mut term, 10);

    term.scroll_view(5);
    let pinned = term.view_row(0);

    // More output pushes another line into the scrollback; the view must show
    // the same content rather than drifting a line at a time.
    fill_scrollback(&mut term, 1);
    assert_eq!(term.buffer.scroll_offset(), 6);
    assert_eq!(term.view_row(0), pinned);
}

#[test]
fn test_view_row_reads_history_when_scrolled() {
    let mut term = Terminal::new(24, 80);
    fill_scrollback(&mut term, 30);

    let live_top: String = term.view_row(0).cells.iter().map(|c| c.c).collect();
    term.scroll_view(3);
    let scrolled_top: String = term.view_row(0).cells.iter().map(|c| c.c).collect();

    assert_ne!(
        live_top.trim(),
        scrolled_top.trim(),
        "scrolling should show different content"
    );
}

#[test]
fn test_links_resolve_in_scrolled_back_history() {
    let mut term = Terminal::new(4, 40);

    // Put a URL on screen, then push it up into the scrollback.
    feed(&mut term, b"see https://example.com/history\n");
    fill_scrollback(&mut term, 8);

    let live = term.resolve_links(1, true, &[0, 1, 2, 3]);
    assert!(
        live.is_empty(),
        "the URL should have scrolled off the live view: {live:?}"
    );

    // Scroll back far enough to bring it into view.
    let mut found = None;
    for offset in 1..=term.history_rows() as i32 {
        term.reset_scroll();
        term.scroll_view(offset);
        let rows: Vec<u16> = (0..term.rows() as u16).collect();
        let links = term.resolve_links(1, true, &rows);
        if let Some(run) = links.values().flatten().next() {
            found = Some(run.url.clone());
            break;
        }
    }

    assert_eq!(
        found.as_deref(),
        Some("https://example.com/history"),
        "a URL in the scrollback should still resolve to a link"
    );
}

#[test]
fn test_explicit_osc8_links_resolve_in_history() {
    let mut term = Terminal::new(4, 40);
    feed(
        &mut term,
        b"\x1b]8;;https://example.com/app\x1b\\CLICKME\x1b]8;;\x1b\\\n",
    );
    fill_scrollback(&mut term, 8);

    let mut found = None;
    for offset in 1..=term.history_rows() as i32 {
        term.reset_scroll();
        term.scroll_view(offset);
        let rows: Vec<u16> = (0..term.rows() as u16).collect();
        let links = term.resolve_links(1, true, &rows);
        if let Some(run) = links.values().flatten().find(|r| !r.detected) {
            found = Some(run.url.clone());
            break;
        }
    }

    assert_eq!(
        found.as_deref(),
        Some("https://example.com/app"),
        "an application's OSC 8 link should survive into the scrollback"
    );
}

#[test]
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
