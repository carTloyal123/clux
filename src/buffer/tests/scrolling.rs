//! Content scrolling: the active area sliding over history.

use super::*;

#[test]
fn scrolling_moves_the_top_row_into_history() {
    let mut buffer = Buffer::new(3, 10, 100 * 64);
    write_row(&mut buffer, 0, "first");
    write_row(&mut buffer, 1, "second");
    write_row(&mut buffer, 2, "third");

    buffer.scroll_up();

    // The screen slid down: "first" left the active area, a blank row arrived.
    assert_eq!(viewport(&buffer), ["second", "third", ""]);
    assert_eq!(buffer.history_rows(), 1);

    // ...and it is still there, one row back.
    buffer.scroll_view(1);
    assert_eq!(viewport(&buffer), ["first", "second", "third"]);
}

#[test]
fn history_accumulates_in_order() {
    let mut buffer = Buffer::new(2, 10, 100 * 64);
    print_lines(&mut buffer, 6);

    let mut tops = Vec::new();
    for offset in (0..=buffer.history_rows() as i32).rev() {
        buffer.reset_scroll();
        buffer.scroll_view(offset);
        tops.push(viewport_text(&buffer, 0));
    }

    // The blank rows the buffer started with are history too, so match on the
    // numbered lines rather than on position.
    let numbers: Vec<usize> = tops
        .iter()
        .filter_map(|row| row.strip_prefix("line ")?.parse().ok())
        .collect();

    assert_eq!(
        numbers,
        (0..6).collect::<Vec<_>>(),
        "history must read back in printing order: {tops:?}"
    );
}

#[test]
fn a_scroll_region_rotates_in_place_and_adds_no_history() {
    let mut buffer = Buffer::new(4, 10, 100 * 64);
    for (row, text) in ["a", "b", "c", "d"].iter().enumerate() {
        write_row(&mut buffer, row, text);
    }
    let history_before = buffer.history_rows();

    // Region [1, 3) scrolls up: "b" is overwritten by "c", "c"'s slot blanks.
    buffer.scroll_region_up(1, 3);
    assert_eq!(viewport(&buffer), ["a", "c", "", "d"]);
    assert_eq!(
        buffer.history_rows(),
        history_before,
        "a region scroll must not write history"
    );
}

#[test]
fn a_scroll_region_can_scroll_down() {
    let mut buffer = Buffer::new(4, 10, 100 * 64);
    for (row, text) in ["a", "b", "c", "d"].iter().enumerate() {
        write_row(&mut buffer, row, text);
    }

    buffer.scroll_region_down(1, 4);
    assert_eq!(viewport(&buffer), ["a", "", "b", "c"]);
}

#[test]
fn a_scroll_region_carries_wrap_flags_with_the_rows() {
    let mut buffer = Buffer::new(3, 4, 10 * 64);
    write_row(&mut buffer, 1, "abcd");
    buffer.set_row_wrapped(1, true);

    buffer.scroll_region_up(0, 3);

    assert!(buffer.row_wrapped(0), "the flag should move with the row");
    assert!(!buffer.row_wrapped(1));
}

#[test]
fn degenerate_regions_do_nothing() {
    let mut buffer = Buffer::new(3, 4, 10 * 64);
    write_row(&mut buffer, 0, "keep");

    buffer.scroll_region_up(2, 2);
    buffer.scroll_region_up(3, 1);
    buffer.scroll_region_down(9, 9);

    assert_eq!(viewport_text(&buffer, 0), "keep");
}
