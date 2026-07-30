//! Protocol round-trip and framing tests.

use super::*;

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
