//! Compositing tests.

use super::super::*;
use crate::cell::Cell;
use crate::protocol::{PaneLayout, PaneRow, WindowLayout};

#[test]
fn test_screen_buffer_creation() {
    let buffer = ScreenBuffer::new(80, 24);
    assert_eq!(buffer.dimensions(), (80, 24));
    assert!(buffer.layout().is_none());
}

#[test]
fn test_screen_buffer_resize() {
    let mut buffer = ScreenBuffer::new(80, 24);

    // Set a layout
    buffer.set_layout(WindowLayout {
        panes: vec![PaneLayout {
            pane_id: 0,
            x: 0,
            y: 0,
            width: 80,
            height: 24,
            focused: true,
        }],
        screen_cols: 80,
        screen_rows: 24,
    });

    assert!(buffer.layout().is_some());

    // Resize clears layout
    buffer.resize(100, 30);
    assert_eq!(buffer.dimensions(), (100, 30));
    assert!(buffer.layout().is_none());
}

#[test]
fn test_update_nonexistent_pane() {
    let mut buffer = ScreenBuffer::new(80, 24);

    buffer.set_layout(WindowLayout {
        panes: vec![PaneLayout {
            pane_id: 0,
            x: 0,
            y: 0,
            width: 80,
            height: 24,
            focused: true,
        }],
        screen_cols: 80,
        screen_rows: 24,
    });

    // Try to update a non-existent pane - should not panic
    buffer.apply_pane_update(99, &[PaneRow::new(0, vec![Cell::new('X')])]);

    // Original content should be unchanged (default spaces)
    let row = buffer.get_row(0).unwrap();
    assert_eq!(row[0].c, ' ');
}

#[test]
fn test_bounds_checking() {
    let mut buffer = ScreenBuffer::new(80, 24);

    buffer.set_layout(WindowLayout {
        panes: vec![PaneLayout {
            pane_id: 0,
            x: 0,
            y: 0,
            width: 80,
            height: 24,
            focused: true,
        }],
        screen_cols: 80,
        screen_rows: 24,
    });

    // Try to update row beyond pane height - should not panic
    buffer.apply_pane_update(0, &[PaneRow::new(100, vec![Cell::new('X')])]);

    // Try to update with cells beyond pane width - should truncate
    let wide_row: Vec<Cell> = (0..200).map(|_| Cell::new('W')).collect();
    buffer.apply_pane_update(0, &[PaneRow::new(0, wide_row)]);

    // Should have written up to column 80
    let row = buffer.get_row(0).unwrap();
    assert_eq!(row[79].c, 'W');
}
