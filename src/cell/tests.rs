//! Cell tests.

use super::*;

#[test]
fn test_cell_default() {
    let cell = Cell::default();
    assert_eq!(cell.c, ' ');
    assert!(cell.fg.is_default());
    assert!(cell.bg.is_default());
    assert!(cell.flags.is_empty());
    assert!(cell.is_empty());
}

#[test]
fn test_cell_styled() {
    let cell = Cell::styled(
        'A',
        Color::rgb(255, 0, 0),
        Color::default_color(),
        CellFlags::BOLD,
    );
    assert_eq!(cell.c, 'A');
    assert_eq!(cell.fg.kind, ColorKind::Rgb);
    assert!(cell.flags.contains(CellFlags::BOLD));
    assert!(!cell.is_empty());
}

#[test]
fn test_color_from_ansi() {
    // Standard colors (30-37)
    assert_eq!(Color::from_ansi(31).unwrap().r, 1); // Red
    assert_eq!(Color::from_ansi(32).unwrap().r, 2); // Green

    // Bright colors (90-97)
    assert_eq!(Color::from_ansi(91).unwrap().r, 9); // Bright red

    // Invalid
    assert!(Color::from_ansi(0).is_none());
}

#[test]
fn test_cell_flags() {
    let flags = CellFlags::BOLD | CellFlags::UNDERLINE;
    assert!(flags.contains(CellFlags::BOLD));
    assert!(flags.contains(CellFlags::UNDERLINE));
    assert!(!flags.contains(CellFlags::ITALIC));
}

/// The cell is copied per character into every pane's history, so its size is
/// the dominant memory cost of the whole program. Guard it deliberately.
#[test]
fn a_cell_is_twenty_bytes() {
    assert_eq!(std::mem::size_of::<Cell>(), 20);
    assert_eq!(
        std::mem::size_of::<Option<HyperlinkId>>(),
        4,
        "the non-zero niche must keep the hyperlink field at four bytes"
    );
}
