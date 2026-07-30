//! Framing tests.

use super::*;
use std::io::Cursor;

// ------------------------------------------------------------------------
// Serialization Round-Trip Tests
// ------------------------------------------------------------------------

#[test]
fn test_write_read_message() {
    let mut buffer = Vec::new();

    let msg = ClientMessage::Hello {
        version: PROTOCOL_VERSION,
        term_cols: 120,
        term_rows: 40,
        term_type: "screen-256color".to_string(),
    };

    write_message(&mut buffer, &msg).unwrap();

    let mut cursor = Cursor::new(buffer);
    let received: ClientMessage = read_message(&mut cursor).unwrap();

    assert_eq!(msg, received);
}

#[test]
fn test_write_read_multiple_messages() {
    let mut buffer = Vec::new();

    let messages = vec![
        ClientMessage::Ping,
        ClientMessage::ShutdownServer,
        ClientMessage::Input(vec![b'a', b'b', b'c']),
        ClientMessage::Resize {
            cols: 100,
            rows: 50,
        },
        ClientMessage::Detach,
    ];

    for msg in &messages {
        write_message(&mut buffer, msg).unwrap();
    }

    let mut cursor = Cursor::new(buffer);

    for expected in &messages {
        let received: ClientMessage = read_message(&mut cursor).unwrap();
        assert_eq!(expected, &received);
    }
}

#[test]
fn test_message_too_large_error() {
    // Create a message that claims to be too large
    let fake_len: u32 = MAX_MESSAGE_SIZE + 1;
    let mut buffer = Vec::new();
    buffer.extend_from_slice(&fake_len.to_le_bytes());
    buffer.extend_from_slice(&[0u8; 10]); // Some payload

    let mut cursor = Cursor::new(buffer);
    let result: ProtocolResult<ClientMessage> = read_message(&mut cursor);

    assert!(matches!(result, Err(ProtocolError::MessageTooLarge { .. })));
}
