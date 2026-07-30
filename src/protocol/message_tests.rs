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
