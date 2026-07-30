//! Message round-trip tests.

use super::*;
use crate::cell::Cell;

// ------------------------------------------------------------------------
// Serialization Round-Trip Tests
// ------------------------------------------------------------------------

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

