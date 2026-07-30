//! Protocol round-trip and framing tests.

use super::*;
use crate::cell::Cell;
use std::io::Cursor;

// ------------------------------------------------------------------------
// Serialization Round-Trip Tests
// ------------------------------------------------------------------------

#[test]
fn test_server_message_session_list_roundtrip() {
    let msg = ServerMessage::SessionList(vec![
        SessionInfo {
            id: 1,
            name: "default".to_string(),
            created_at: 1700000000,
            windows: 2,
            attached_clients: 1,
        },
        SessionInfo {
            id: 2,
            name: "work".to_string(),
            created_at: 1700001000,
            windows: 3,
            attached_clients: 0,
        },
    ]);

    let serialized = bincode::serialize(&msg).unwrap();
    let deserialized: ServerMessage = bincode::deserialize(&serialized).unwrap();

    assert_eq!(msg, deserialized);
}

