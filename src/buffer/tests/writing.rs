//! Writing to the active area, clearing, and dirty tracking.

use super::*;

#[test]
fn cells_written_are_read_back() {
    let mut buffer = Buffer::new(4, 10, 100 * 64);
    write_row(&mut buffer, 1, "hello");

    assert_eq!(viewport_text(&buffer, 1), "hello");
    assert_eq!(buffer.cell(1, 0).unwrap().c, 'h');
    assert_eq!(buffer.cell(1, 4).unwrap().c, 'o');
    assert_eq!(buffer.cell(1, 5).unwrap().c, ' ');
}

#[test]
fn writes_outside_the_screen_are_ignored() {
    let mut buffer = Buffer::new(2, 4, 10 * 64);
    buffer.set_cell(99, 0, Cell::new('x'));
    buffer.set_cell(0, 99, Cell::new('x'));

    assert_eq!(viewport(&buffer), ["", ""]);
    assert!(buffer.cell(99, 0).is_none());
}

#[test]
fn wrap_flags_are_per_row() {
    let mut buffer = Buffer::new(3, 4, 10 * 64);
    buffer.set_row_wrapped(0, true);

    assert!(buffer.row_wrapped(0));
    assert!(!buffer.row_wrapped(1));
    assert!(buffer.row_cells(0).unwrap().1, "viewport reports the flag");
}

#[test]
fn clearing_blanks_the_right_region() {
    let mut buffer = Buffer::new(3, 6, 10 * 64);
    for row in 0..3 {
        write_row(&mut buffer, row, "abcdef");
    }

    buffer.clear_below(1, 3);
    assert_eq!(viewport(&buffer), ["abcdef", "abc", ""]);

    for row in 0..3 {
        write_row(&mut buffer, row, "abcdef");
    }
    buffer.clear_above(1, 2);
    assert_eq!(viewport(&buffer), ["", "   def", "abcdef"]);

    buffer.clear_screen();
    assert_eq!(viewport(&buffer), ["", "", ""]);
}

#[test]
fn clearing_a_row_drops_its_wrap_flag() {
    let mut buffer = Buffer::new(2, 4, 10 * 64);
    buffer.set_row_wrapped(0, true);
    buffer.clear_active_row(0);

    assert!(!buffer.row_wrapped(0));
}

#[test]
fn only_changed_rows_are_reported_dirty() {
    let mut buffer = Buffer::new(3, 6, 10 * 64);
    buffer.take_dirty_rows(); // a fresh buffer starts dirty

    write_row(&mut buffer, 1, "x");
    assert_eq!(buffer.take_dirty_rows(), vec![1]);
    assert!(
        buffer.take_dirty_rows().is_empty(),
        "flags clear once reported"
    );

    // Writing the same content again is not a change.
    write_row(&mut buffer, 1, "x");
    assert!(buffer.take_dirty_rows().is_empty());
}

#[test]
fn scrolling_marks_the_screen_dirty() {
    let mut buffer = Buffer::new(3, 6, 10 * 64);
    buffer.take_dirty_rows();

    buffer.scroll_up();
    assert_eq!(buffer.take_dirty_rows(), vec![0, 1, 2]);
}
