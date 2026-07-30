//! The memory budget for history.

use super::*;

/// Bytes one 80-column row costs.
fn row_bytes(cols: usize) -> usize {
    cols * std::mem::size_of::<Cell>()
}

#[test]
fn history_is_capped_by_bytes_not_rows() {
    let cols = 20;
    let budget = row_bytes(cols) * 10;
    let mut buffer = Buffer::new(3, cols, budget);

    print_lines(&mut buffer, 500);

    assert_eq!(
        buffer.history_rows(),
        10,
        "the budget should buy exactly ten rows of history"
    );
}

#[test]
fn a_wider_window_holds_fewer_rows_for_the_same_memory() {
    // The point of budgeting bytes: the ceiling holds when the window grows.
    let budget = row_bytes(20) * 10;
    let mut buffer = Buffer::new(3, 20, budget);
    print_lines(&mut buffer, 200);
    assert_eq!(buffer.history_rows(), 10);

    buffer.resize(3, 40, (2, 0));
    print_lines(&mut buffer, 200);

    assert_eq!(
        buffer.history_rows(),
        5,
        "twice the width, half the rows, same memory"
    );
}

#[test]
fn allocated_memory_stays_within_a_page_of_the_budget() {
    let cols = 80;
    let budget = row_bytes(cols) * 500;
    let mut buffer = Buffer::new(24, cols, budget);

    print_lines(&mut buffer, 5_000);

    // Pages are allocated whole, so the ceiling is the budget plus the active
    // area plus at most one partly-used page.
    let ceiling = budget + row_bytes(cols) * (24 + Buffer::rows_per_page());
    assert!(
        buffer.allocated_bytes() <= ceiling,
        "allocated {} bytes, ceiling {}",
        buffer.allocated_bytes(),
        ceiling
    );
}

#[test]
fn a_zero_budget_keeps_no_history() {
    let mut buffer = Buffer::new(4, 20, 0);
    print_lines(&mut buffer, 100);

    assert_eq!(buffer.history_rows(), 0);
    assert_eq!(buffer.page_count(), 1, "one page holds the screen");
}
