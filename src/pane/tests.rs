//! Pane layout tests.

use super::*;

#[test]
fn test_rect_split_horizontal() {
    let rect = Rect::new(0, 0, 80, 24);
    let (top, bottom) = rect.split_horizontal(0.5);

    assert_eq!(top.y, 0);
    assert_eq!(top.height, 12);
    assert_eq!(bottom.y, 13); // 12 + 1 for border
}

#[test]
fn test_rect_split_vertical() {
    let rect = Rect::new(0, 0, 80, 24);
    let (left, right) = rect.split_vertical(0.5);

    assert_eq!(left.x, 0);
    assert_eq!(left.width, 40);
    assert_eq!(right.x, 41); // 40 + 1 for border
}

#[test]
fn test_rect_contains() {
    let rect = Rect::new(10, 10, 20, 10);

    assert!(rect.contains(10, 10));
    assert!(rect.contains(29, 19));
    assert!(!rect.contains(9, 10));
    assert!(!rect.contains(30, 10));
}
