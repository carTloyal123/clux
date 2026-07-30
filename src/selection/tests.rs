//! Selection geometry tests.

use super::*;

#[test]
fn test_point_ordering() {
    let p1 = Point::new(0, 5);
    let p2 = Point::new(0, 10);
    let p3 = Point::new(1, 0);

    assert!(p1 < p2);
    assert!(p2 < p3);
    assert!(p1 < p3);
}

#[test]
fn test_selection_contains() {
    let sel = Selection {
        start: Point::new(0, 5),
        end: Point::new(2, 10),
        mode: SelectionMode::Normal,
        active: true,
    };

    assert!(sel.contains(Point::new(0, 5)));
    assert!(sel.contains(Point::new(1, 0)));
    assert!(sel.contains(Point::new(2, 10)));
    assert!(!sel.contains(Point::new(0, 4)));
    assert!(!sel.contains(Point::new(2, 11)));
    assert!(!sel.contains(Point::new(3, 0)));
}

#[test]
fn test_find_word_bounds() {
    fn make_cells(s: &str) -> Vec<Cell> {
        s.chars()
            .map(|c| Cell {
                c,
                ..Cell::default()
            })
            .collect()
    }

    let cells = make_cells("hello world test");

    // Click on 'e' in "hello"
    assert_eq!(find_word_bounds(&cells, 1), (0, 4));

    // Click on 'w' in "world"
    assert_eq!(find_word_bounds(&cells, 6), (6, 10));

    // Click on space
    assert_eq!(find_word_bounds(&cells, 5), (5, 5));
}
