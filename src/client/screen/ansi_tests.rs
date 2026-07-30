//! ANSI writer tests.

use super::super::*;
use super::test_support::*;
use crate::cell::{Cell, CellFlags, Color};

#[test]
fn test_cells_to_ansi_basic() {
    let cells = vec![Cell::new('H'), Cell::new('i'), Cell::new('!')];

    let ansi = cells_to_ansi(&cells);

    // Should contain the characters
    assert!(ansi.contains('H'));
    assert!(ansi.contains('i'));
    assert!(ansi.contains('!'));
    // Should start with reset
    assert!(ansi.starts_with("\x1b[0m"));
    // Should end with reset
    assert!(ansi.ends_with("\x1b[0m"));
}

#[test]
fn test_cells_to_ansi_has_no_links_by_default() {
    let ansi = cells_to_ansi(&text_cells("https://a.io"));
    assert!(!ansi.contains("\x1b]8;"), "unexpected OSC 8 in {ansi:?}");
}

#[test]
fn test_cells_to_ansi_colors() {
    let cells = vec![
        Cell::styled(
            'R',
            Color::rgb(255, 0, 0),
            Color::default_color(),
            CellFlags::empty(),
        ),
        Cell::styled(
            'G',
            Color::rgb(0, 255, 0),
            Color::default_color(),
            CellFlags::empty(),
        ),
    ];

    let ansi = cells_to_ansi(&cells);

    // Should contain RGB color codes
    assert!(ansi.contains("\x1b[38;2;255;0;0m")); // Red foreground
    assert!(ansi.contains("\x1b[38;2;0;255;0m")); // Green foreground
}

#[test]
fn test_cells_to_ansi_attributes() {
    let cells = vec![Cell::styled(
        'B',
        Color::default_color(),
        Color::default_color(),
        CellFlags::BOLD | CellFlags::UNDERLINE,
    )];

    let ansi = cells_to_ansi(&cells);

    // Should contain attribute codes
    assert!(ansi.contains('1') || ansi.contains("1;")); // Bold
    assert!(ansi.contains('4') || ansi.contains("4;")); // Underline
}
