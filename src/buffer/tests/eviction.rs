//! The history budget, and releasing pages.

use super::*;

#[test]
fn history_is_capped_at_the_budget() {
    let mut buffer = Buffer::new(3, 10, 5 * 10 * std::mem::size_of::<Cell>());
    print_lines(&mut buffer, 50);

    assert_eq!(
        buffer.history_rows(),
        5,
        "history must not exceed its budget"
    );
    // The active area is always intact.
    assert_eq!(viewport(&buffer).len(), 3);
}

#[test]
fn the_newest_history_survives_eviction() {
    let mut buffer = Buffer::new(2, 10, 3 * 10 * std::mem::size_of::<Cell>());
    print_lines(&mut buffer, 20);

    buffer.scroll_view(3);
    let oldest_kept = viewport_text(&buffer, 0);

    // With 20 lines printed and room for 3 rows of history, the oldest lines are
    // gone and what remains is the most recent.
    let number: usize = oldest_kept
        .strip_prefix("line ")
        .and_then(|n| n.parse().ok())
        .expect("a numbered line");
    assert!(number >= 15, "expected recent history, got {oldest_kept:?}");
}

#[test]
fn a_zero_history_buffer_keeps_only_the_screen() {
    // This is how the alternate screen works: no history at all.
    let mut buffer = Buffer::new(3, 10, 0);
    print_lines(&mut buffer, 20);

    assert_eq!(buffer.history_rows(), 0);
    assert!(!buffer.scroll_view(5), "there is nothing to scroll back to");
    assert!(!buffer.is_scrolled());
}

#[test]
fn pages_are_released_once_they_fall_out_of_the_budget() {
    let rows_per_page = Buffer::rows_per_page();
    let mut buffer = Buffer::new(4, 10, rows_per_page * 10 * std::mem::size_of::<Cell>());

    // Push far more than the budget through it.
    print_lines(&mut buffer, rows_per_page * 4);

    assert_eq!(buffer.history_rows(), rows_per_page);
    // Storage should be a handful of pages, not one per row.
    assert!(
        buffer.page_count() <= 3,
        "expected pages to be released, have {}",
        buffer.page_count()
    );
}

#[test]
fn a_pinned_view_clamps_forward_when_its_content_is_evicted() {
    let mut buffer = Buffer::new(2, 10, 4 * 10 * std::mem::size_of::<Cell>());
    print_lines(&mut buffer, 10);

    buffer.scroll_view(4);
    assert!(buffer.is_scrolled());

    // Enough new output to evict everything the view was looking at.
    print_lines(&mut buffer, 20);

    // Still a valid position, and still showing real rows.
    assert!(buffer.scroll_offset() <= buffer.history_rows());
    assert!(buffer.row_cells(0).is_some());
}
