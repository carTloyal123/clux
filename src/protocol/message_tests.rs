//! Message round-trip tests.

use super::*;
use crate::cell::Cell;

// ------------------------------------------------------------------------
// Serialization Round-Trip Tests
// ------------------------------------------------------------------------

#[test]
fn test_empty_session_name() {
    let msg = ClientMessage::Attach {
        session_name: Some("".to_string()),
        create: false,
    };

    let serialized = bincode::serialize(&msg).unwrap();
    let deserialized: ClientMessage = bincode::deserialize(&serialized).unwrap();

    assert_eq!(msg, deserialized);
}

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
fn test_large_input_buffer() {
    // Test with 64KB of input (e.g., large paste)
    let large_input = vec![b'x'; 64 * 1024];
    let msg = ClientMessage::Input(large_input.clone());

    let serialized = bincode::serialize(&msg).unwrap();
    let deserialized: ClientMessage = bincode::deserialize(&serialized).unwrap();

    assert_eq!(msg, deserialized);
}

// ------------------------------------------------------------------------
// Pane Rendering Tests (hybrid client-server rendering)
// ------------------------------------------------------------------------

#[test]
fn test_cell_serialization_roundtrip() {
    use crate::cell::{Cell, CellFlags, Color};

    let cell = Cell::styled(
        'A',
        Color::rgb(255, 0, 0),
        Color::indexed(4),
        CellFlags::BOLD | CellFlags::UNDERLINE,
    );

    let serialized = bincode::serialize(&cell).unwrap();
    let deserialized: Cell = bincode::deserialize(&serialized).unwrap();

    assert_eq!(cell, deserialized);
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
fn test_pane_update_message_roundtrip() {
    use crate::cell::{Cell, Color};

    let msg = ServerMessage::PaneUpdate {
        pane_id: 1,
        changed_rows: vec![
            PaneRow::new(0, vec![Cell::new('$'), Cell::new(' ')]),
            PaneRow::new(
                1,
                vec![Cell::styled(
                    'X',
                    Color::rgb(0, 255, 0),
                    Color::default_color(),
                    crate::cell::CellFlags::ITALIC,
                )],
            ),
        ],
        cursor: Some(CursorState {
            row: 1,
            col: 5,
            visible: true,
        }),
    };

    let serialized = bincode::serialize(&msg).unwrap();
    let deserialized: ServerMessage = bincode::deserialize(&serialized).unwrap();

    assert_eq!(msg, deserialized);
}

#[test]
fn test_pane_update_without_cursor() {
    let msg = ServerMessage::PaneUpdate {
        pane_id: 0,
        changed_rows: vec![PaneRow::new(0, vec![Cell::default()])],
        cursor: None,
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

#[test]
fn test_pane_update_message_sizes() {
    use crate::cell::Cell;

    // A typical pane update should be reasonably sized
    let pane_update = ServerMessage::PaneUpdate {
        pane_id: 0,
        changed_rows: vec![PaneRow::new(0, (0..80).map(|_| Cell::new('x')).collect())],
        cursor: Some(CursorState::default()),
    };
    let pane_update_size = bincode::serialize(&pane_update).unwrap().len();
    // 80 cells * ~20 bytes per cell + overhead should be < 2KB
    assert!(
        pane_update_size < 2048,
        "PaneUpdate for 80 cells should be < 2KB, got {} bytes",
        pane_update_size
    );

    // Layout change should be tiny
    let layout_changed = ServerMessage::LayoutChanged {
        layout: WindowLayout {
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
        },
    };
    let layout_size = bincode::serialize(&layout_changed).unwrap().len();
    assert!(
        layout_size < 100,
        "LayoutChanged for 2 panes should be < 100 bytes, got {} bytes",
        layout_size
    );
}
