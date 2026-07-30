//! Viewport movement.

use super::*;

#[test]
fn a_fresh_buffer_follows_output() {
    let mut buffer = Buffer::new(3, 10, 100 * 64);
    assert!(!buffer.is_scrolled());
    assert_eq!(buffer.scroll_offset(), 0);
    assert!(!buffer.reset_scroll(), "already live");
}

#[test]
fn scrolling_is_clamped_at_both_ends() {
    let mut buffer = Buffer::new(3, 10, 100 * 64);
    print_lines(&mut buffer, 10 * 64);

    buffer.scroll_view(10_000);
    assert_eq!(buffer.scroll_offset(), buffer.history_rows());
    assert!(!buffer.scroll_view(10_000), "already at the oldest row");

    buffer.scroll_view(-10_000);
    assert_eq!(buffer.scroll_offset(), 0);
    assert!(!buffer.scroll_view(-10_000), "already live");
    assert!(!buffer.is_scrolled());
}

#[test]
fn a_pinned_view_does_not_move_when_output_arrives() {
    let mut buffer = Buffer::new(3, 10, 100 * 64);
    print_lines(&mut buffer, 10 * 64);

    buffer.scroll_view(4);
    let pinned = viewport(&buffer);

    print_lines(&mut buffer, 5);
    assert_eq!(
        viewport(&buffer),
        pinned,
        "an absolute pin should keep showing the same rows"
    );
    // It is further back now, because the live view moved on.
    assert_eq!(buffer.scroll_offset(), 9);
}

#[test]
fn returning_to_live_shows_the_newest_output() {
    let mut buffer = Buffer::new(2, 10, 100 * 64);
    print_lines(&mut buffer, 10 * 64);

    let live = viewport(&buffer);
    buffer.scroll_view(5);
    assert_ne!(viewport(&buffer), live);

    assert!(buffer.reset_scroll());
    assert_eq!(viewport(&buffer), live);
}

#[test]
fn the_viewport_never_reads_past_the_active_area() {
    let mut buffer = Buffer::new(3, 10, 100 * 64);
    print_lines(&mut buffer, 10 * 64);

    assert!(buffer.row_cells(2).is_some());
    assert!(
        buffer.row_cells(3).is_none(),
        "the viewport is exactly the screen height"
    );
}
