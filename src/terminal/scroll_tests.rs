//! Scrollback and resize tests.

use super::test_support::*;
use super::*;

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
