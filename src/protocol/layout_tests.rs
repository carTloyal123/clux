//! Message round-trip tests.

use super::*;
use crate::cell::Cell;

// ------------------------------------------------------------------------
// Serialization Round-Trip Tests
// ------------------------------------------------------------------------

#[test]
fn test_unicode_in_pane_row() {
    let msg = ServerMessage::PaneUpdate {
        pane_id: 0,
        changed_rows: vec![PaneRow::new(
            0,
            "Hello 世界 🦀 émojis".chars().map(Cell::new).collect(),
        )],
        cursor: None,
    };

    let serialized = bincode::serialize(&msg).unwrap();
    let deserialized: ServerMessage = bincode::deserialize(&serialized).unwrap();

    assert_eq!(msg, deserialized);
}

#[test]
fn test_pane_layout_roundtrip() {
    let layout = PaneLayout {
        pane_id: 42,
        x: 10,
        y: 5,
        width: 80,
        height: 24,
        focused: true,
    };

    let serialized = bincode::serialize(&layout).unwrap();
    let deserialized: PaneLayout = bincode::deserialize(&serialized).unwrap();

    assert_eq!(layout, deserialized);
}

#[test]
fn test_window_layout_roundtrip() {
    let layout = WindowLayout {
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
                x: 41,
                y: 0,
                width: 39,
                height: 24,
                focused: false,
            },
        ],
        screen_cols: 80,
        screen_rows: 24,
    };

    let serialized = bincode::serialize(&layout).unwrap();
    let deserialized: WindowLayout = bincode::deserialize(&serialized).unwrap();

    assert_eq!(layout, deserialized);
}

#[test]
fn test_pane_row_roundtrip() {
    use crate::cell::{Cell, CellFlags, Color};

    let row = PaneRow::new(
        5,
        vec![
            Cell::styled(
                'H',
                Color::rgb(255, 0, 0),
                Color::default_color(),
                CellFlags::BOLD,
            ),
            Cell::styled(
                'i',
                Color::indexed(2),
                Color::default_color(),
                CellFlags::empty(),
            ),
            Cell::default(),
        ],
    );

    let serialized = bincode::serialize(&row).unwrap();
    let deserialized: PaneRow = bincode::deserialize(&serialized).unwrap();

    assert_eq!(row, deserialized);
}

#[test]
fn test_layout_changed_message_roundtrip() {
    let msg = ServerMessage::LayoutChanged {
        layout: WindowLayout {
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
        },
    };

    let serialized = bincode::serialize(&msg).unwrap();
    let deserialized: ServerMessage = bincode::deserialize(&serialized).unwrap();

    assert_eq!(msg, deserialized);
}

#[test]
fn test_pane_row_links_roundtrip() {
    let msg = ServerMessage::PaneUpdate {
        pane_id: 3,
        changed_rows: vec![PaneRow::with_links(
            2,
            "https://example.com/x".chars().map(Cell::new).collect(),
            vec![
                RowLink {
                    start_col: 0,
                    end_col: 21,
                    id: 4242,
                    url: "https://example.com/x".to_string(),
                    detected: true,
                },
                RowLink {
                    start_col: 30,
                    end_col: 37,
                    id: 99,
                    url: "https://example.com/osc8".to_string(),
                    detected: false,
                },
            ],
        )],
        cursor: None,
    };

    let serialized = bincode::serialize(&msg).unwrap();
    let deserialized: ServerMessage = bincode::deserialize(&serialized).unwrap();

    assert_eq!(msg, deserialized);
}

