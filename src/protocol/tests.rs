//! Protocol round-trip and framing tests.

use super::*;
use crate::cell::Cell;
use std::io::Cursor;

// ------------------------------------------------------------------------
// Serialization Round-Trip Tests
// ------------------------------------------------------------------------

#[test]
fn test_client_message_hello_roundtrip() {
    let msg = ClientMessage::Hello {
        version: PROTOCOL_VERSION,
        term_cols: 80,
        term_rows: 24,
        term_type: "xterm-256color".to_string(),
    };

    let serialized = bincode::serialize(&msg).unwrap();
    let deserialized: ClientMessage = bincode::deserialize(&serialized).unwrap();

    assert_eq!(msg, deserialized);
}

#[test]
fn test_client_message_attach_roundtrip() {
    let msg = ClientMessage::Attach {
        session_name: Some("work".to_string()),
        create: true,
    };

    let serialized = bincode::serialize(&msg).unwrap();
    let deserialized: ClientMessage = bincode::deserialize(&serialized).unwrap();

    assert_eq!(msg, deserialized);
}

#[test]
fn test_client_message_input_roundtrip() {
    let msg = ClientMessage::Input(vec![0x1b, b'[', b'A']); // Up arrow

    let serialized = bincode::serialize(&msg).unwrap();
    let deserialized: ClientMessage = bincode::deserialize(&serialized).unwrap();

    assert_eq!(msg, deserialized);
}

#[test]
fn test_client_message_command_roundtrip() {
    let commands = vec![
        CommandAction::SplitHorizontal,
        CommandAction::SplitVertical,
        CommandAction::ClosePane,
        CommandAction::NavigatePane(Direction::Up),
        CommandAction::NavigatePane(Direction::Down),
        CommandAction::NavigatePane(Direction::Left),
        CommandAction::NavigatePane(Direction::Right),
        CommandAction::NewWindow,
        CommandAction::CloseWindow,
        CommandAction::NextWindow,
        CommandAction::PrevWindow,
        CommandAction::SelectWindow(5),
        CommandAction::Quit,
    ];

    for cmd in commands {
        let msg = ClientMessage::Command(cmd.clone());
        let serialized = bincode::serialize(&msg).unwrap();
        let deserialized: ClientMessage = bincode::deserialize(&serialized).unwrap();
        assert_eq!(msg, deserialized);
    }
}

#[test]
fn test_client_message_shutdown_server_roundtrip() {
    let msg = ClientMessage::ShutdownServer;

    let serialized = bincode::serialize(&msg).unwrap();
    let deserialized: ClientMessage = bincode::deserialize(&serialized).unwrap();

    assert_eq!(msg, deserialized);
}

#[test]
fn test_server_message_hello_ack_roundtrip() {
    let msg = ServerMessage::HelloAck {
        version: PROTOCOL_VERSION,
        server_pid: 12345,
    };

    let serialized = bincode::serialize(&msg).unwrap();
    let deserialized: ServerMessage = bincode::deserialize(&serialized).unwrap();

    assert_eq!(msg, deserialized);
}

#[test]
fn test_server_message_attached_roundtrip() {
    let msg = ServerMessage::Attached {
        session_id: 1,
        session_name: "default".to_string(),
    };

    let serialized = bincode::serialize(&msg).unwrap();
    let deserialized: ServerMessage = bincode::deserialize(&serialized).unwrap();

    assert_eq!(msg, deserialized);
}

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

#[test]
fn test_detach_reasons_roundtrip() {
    let reasons = vec![
        DetachReason::ClientRequested,
        DetachReason::SessionClosed,
        DetachReason::ServerShutdown,
        DetachReason::Replaced,
    ];

    for reason in reasons {
        let msg = ServerMessage::Detached {
            reason: reason.clone(),
        };
        let serialized = bincode::serialize(&msg).unwrap();
        let deserialized: ServerMessage = bincode::deserialize(&serialized).unwrap();
        assert_eq!(msg, deserialized);
    }
}

// ------------------------------------------------------------------------
// Wire Protocol Tests
// ------------------------------------------------------------------------

#[test]
fn test_connection_closed_error() {
    let buffer: Vec<u8> = Vec::new(); // Empty buffer
    let mut cursor = Cursor::new(buffer);

    let result: ProtocolResult<ClientMessage> = read_message(&mut cursor);

    assert!(matches!(result, Err(ProtocolError::ConnectionClosed)));
}

#[test]
fn test_connection_closed_mid_payload_maps_to_connection_closed() {
    let len = 8u32;
    let mut buffer = Vec::new();
    buffer.extend_from_slice(&len.to_le_bytes());
    buffer.extend_from_slice(&[1, 2, 3]); // Truncated payload

    let mut cursor = Cursor::new(buffer);
    let result: ProtocolResult<ClientMessage> = read_message(&mut cursor);
    assert!(matches!(result, Err(ProtocolError::ConnectionClosed)));
}

// ------------------------------------------------------------------------
// Serialization Size Tests (for performance awareness)
// ------------------------------------------------------------------------

#[test]
fn test_serialization_sizes() {
    // These tests document expected sizes for performance monitoring

    let ping = ClientMessage::Ping;
    let ping_size = bincode::serialize(&ping).unwrap().len();
    assert!(
        ping_size < 10,
        "Ping should be tiny, got {} bytes",
        ping_size
    );

    let input = ClientMessage::Input(vec![b'a']);
    let input_size = bincode::serialize(&input).unwrap().len();
    assert!(
        input_size < 20,
        "Single char input should be small, got {} bytes",
        input_size
    );

    // A full 80x24 pane repaint
    let full_pane = ServerMessage::PaneUpdate {
        pane_id: 0,
        changed_rows: (0..24)
            .map(|row| PaneRow::new(row, vec![Cell::new('x'); 80]))
            .collect(),
        cursor: Some(CursorState::default()),
    };
    let full_pane_size = bincode::serialize(&full_pane).unwrap().len();
    assert!(
        full_pane_size < 64 * 1024,
        "Full pane repaint should be < 64KB, got {} bytes",
        full_pane_size
    );

    // Single row update should be much smaller
    let update = ServerMessage::PaneUpdate {
        pane_id: 0,
        changed_rows: vec![PaneRow::new(5, vec![Cell::new('x'); 80])],
        cursor: Some(CursorState::default()),
    };
    let update_size = bincode::serialize(&update).unwrap().len();
    assert!(
        update_size < 4096,
        "Single row update should be < 4KB, got {} bytes",
        update_size
    );

    // Hyperlinks add one run per row, not a URL per cell.
    let linked = ServerMessage::PaneUpdate {
        pane_id: 0,
        changed_rows: vec![PaneRow::with_links(
            5,
            vec![Cell::new('x'); 80],
            vec![RowLink {
                start_col: 0,
                end_col: 40,
                id: 7,
                url: "https://example.com/some/path".to_string(),
                detected: true,
            }],
        )],
        cursor: None,
    };
    let linked_size = bincode::serialize(&linked).unwrap().len();
    assert!(
        linked_size < update_size + 128,
        "A link should cost ~its URL, got {} bytes over {}",
        linked_size - update_size,
        update_size
    );
}

// ------------------------------------------------------------------------
// Edge Case Tests
// ------------------------------------------------------------------------
