//! Scrollback and viewport invariants
//!
//! Behaviour that must survive the paged-buffer migration.

mod common;

use clux::terminal::Terminal;
use common::{print_lines, row_text, viewport};

#[test]
fn output_longer_than_the_screen_is_recorded_in_order() {
    // The core promise of scrollback: everything printed is still readable, in the
    // order it was printed, by scrolling back.
    let mut term = Terminal::new(6, 20);
    print_lines(&mut term, 40);

    // Reading the top row at each scroll offset walks the recorded lines one at a
    // time - consecutive offsets overlap, so comparing whole viewports would just
    // count every line once per screen height.
    let deepest = term.history_rows() as i32;
    let mut tops: Vec<String> = Vec::new();
    for offset in (0..=deepest).rev() {
        term.reset_scroll();
        term.scroll_view(offset);
        tops.push(row_text(&term, 0));
    }

    let numbers: Vec<usize> = tops
        .iter()
        .filter_map(|row| row.strip_prefix("line ")?.parse().ok())
        .collect();

    assert!(
        numbers.len() > 30,
        "most of the printed lines should still be recorded, got {:?}",
        tops
    );
    assert_eq!(numbers[0], 0, "the oldest line should still be reachable");
    for (i, n) in numbers.iter().enumerate() {
        assert_eq!(
            *n, i,
            "recorded lines must be in printing order with no gaps or repeats: {numbers:?}"
        );
    }
}

#[test]
fn the_live_view_shows_the_most_recent_output() {
    let mut term = Terminal::new(4, 20);
    print_lines(&mut term, 20);

    let live = viewport(&term);
    assert!(
        live.contains(&"line 19".to_string()),
        "live view should end with the newest line: {live:?}"
    );
    assert!(
        !live.contains(&"line 0".to_string()),
        "the oldest line should have scrolled off: {live:?}"
    );
}

#[test]
fn scrolling_back_and_forward_returns_to_the_same_view() {
    let mut term = Terminal::new(5, 20);
    print_lines(&mut term, 30);

    let before = viewport(&term);
    term.scroll_view(7);
    assert_ne!(viewport(&term), before, "scrolling should change the view");
    term.scroll_view(-7);
    assert_eq!(viewport(&term), before, "scrolling back should restore it");

    term.scroll_view(9);
    term.reset_scroll();
    assert_eq!(
        viewport(&term),
        before,
        "reset should restore the live view"
    );
}

#[test]
fn a_scrolled_view_stays_on_its_content_as_output_arrives() {
    let mut term = Terminal::new(5, 20);
    print_lines(&mut term, 30);

    term.scroll_view(6);
    let pinned = viewport(&term);

    print_lines(&mut term, 5);
    assert_eq!(
        viewport(&term),
        pinned,
        "new output must not drag a scrolled view along with it"
    );
}

#[test]
fn scrolling_is_clamped_at_both_ends() {
    let mut term = Terminal::new(4, 20);
    print_lines(&mut term, 12);

    term.scroll_view(10_000);
    let oldest = viewport(&term);
    term.scroll_view(10_000);
    assert_eq!(
        viewport(&term),
        oldest,
        "cannot scroll past the oldest line"
    );

    term.scroll_view(-10_000);
    let live = viewport(&term);
    term.scroll_view(-10_000);
    assert_eq!(viewport(&term), live, "cannot scroll past the live view");
    assert!(!term.is_scrolled());
}
