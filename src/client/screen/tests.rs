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
fn test_single_pane_update() {
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

    // Apply an update
    buffer.apply_pane_update(0, &[PaneRow::new(0, vec![Cell::new('H'), Cell::new('i')])]);

    // Check the cells were written
    let row = buffer.get_row(0).unwrap();
    assert_eq!(row[0].c, 'H');
    assert_eq!(row[1].c, 'i');
    assert_eq!(row[2].c, ' '); // Rest should be default
}

#[test]
fn test_vertical_split_isolation() {
    let mut buffer = ScreenBuffer::new(81, 24); // 40 + 1 divider + 40

    buffer.set_layout(WindowLayout {
        panes: vec![
            PaneLayout {
                pane_id: 0,
                x: 0,
                y: 0,
                width: 40,
                height: 24,
                focused: true,
            },
            PaneLayout {
                pane_id: 1,
                x: 41, // After divider column
                y: 0,
                width: 40,
                height: 24,
                focused: false,
            },
        ],
        screen_cols: 81,
        screen_rows: 24,
    });

    // Update left pane with full-width content
    let left_row: Vec<Cell> = (0..40).map(|_| Cell::new('L')).collect();
    buffer.apply_pane_update(0, &[PaneRow::new(0, left_row)]);

    // Update right pane with full-width content
    let right_row: Vec<Cell> = (0..40).map(|_| Cell::new('R')).collect();
    buffer.apply_pane_update(1, &[PaneRow::new(0, right_row)]);

    // Check isolation - left pane content
    let row = buffer.get_row(0).unwrap();
    for i in 0..40 {
        assert_eq!(row[i].c, 'L', "Left pane cell {} should be 'L'", i);
    }

    // Divider at column 40
    assert_eq!(row[40].c, '│', "Divider should be at column 40");

    // Right pane content
    for i in 41..81 {
        assert_eq!(row[i].c, 'R', "Right pane cell {} should be 'R'", i);
    }
}

#[test]
fn test_horizontal_split_isolation() {
    let mut buffer = ScreenBuffer::new(80, 25); // 12 + 1 divider + 12

    buffer.set_layout(WindowLayout {
        panes: vec![
            PaneLayout {
                pane_id: 0,
                x: 0,
                y: 0,
                width: 80,
                height: 12,
                focused: true,
            },
            PaneLayout {
                pane_id: 1,
                x: 0,
                y: 13, // After divider row
                width: 80,
                height: 12,
                focused: false,
            },
        ],
        screen_cols: 80,
        screen_rows: 25,
    });

    // Update top pane
    let top_row: Vec<Cell> = (0..80).map(|_| Cell::new('T')).collect();
    buffer.apply_pane_update(0, &[PaneRow::new(11, top_row)]); // Last row of top pane

    // Update bottom pane
    let bottom_row: Vec<Cell> = (0..80).map(|_| Cell::new('B')).collect();
    buffer.apply_pane_update(1, &[PaneRow::new(0, bottom_row)]); // First row of bottom pane

    // Check isolation
    let row11 = buffer.get_row(11).unwrap();
    assert_eq!(row11[0].c, 'T', "Row 11 should have top pane content");

    let row12 = buffer.get_row(12).unwrap();
    assert_eq!(row12[0].c, '─', "Row 12 should be divider");

    let row13 = buffer.get_row(13).unwrap();
    assert_eq!(row13[0].c, 'B', "Row 13 should have bottom pane content");
}

#[test]
fn test_three_pane_layout() {
    // Layout:
    // +--------+--------+
    // |   0    |   1    |
    // +--------+--------+
    // |        2        |
    // +-----------------+
    let mut buffer = ScreenBuffer::new(81, 25);

    buffer.set_layout(WindowLayout {
        panes: vec![
            PaneLayout {
                pane_id: 0,
                x: 0,
                y: 0,
                width: 40,
                height: 12,
                focused: true,
            },
            PaneLayout {
                pane_id: 1,
                x: 41,
                y: 0,
                width: 40,
                height: 12,
                focused: false,
            },
            PaneLayout {
                pane_id: 2,
                x: 0,
                y: 13,
                width: 81,
                height: 12,
                focused: false,
            },
        ],
        screen_cols: 81,
        screen_rows: 25,
    });

    // Update all three panes
    buffer.apply_pane_update(0, &[PaneRow::new(0, vec![Cell::new('A'); 40])]);
    buffer.apply_pane_update(1, &[PaneRow::new(0, vec![Cell::new('B'); 40])]);
    buffer.apply_pane_update(2, &[PaneRow::new(0, vec![Cell::new('C'); 81])]);

    // Check pane 0
    let row0 = buffer.get_row(0).unwrap();
    assert_eq!(row0[0].c, 'A');
    assert_eq!(row0[39].c, 'A');
    assert_eq!(row0[40].c, '│'); // Vertical divider
    assert_eq!(row0[41].c, 'B');

    // Check pane 2
    let row13 = buffer.get_row(13).unwrap();
    assert_eq!(row13[0].c, 'C');
    assert_eq!(row13[40].c, 'C');
    assert_eq!(row13[80].c, 'C');
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
fn test_update_without_layout() {
    let mut buffer = ScreenBuffer::new(80, 24);

    // No layout set - update should be ignored
    buffer.apply_pane_update(0, &[PaneRow::new(0, vec![Cell::new('X')])]);

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
