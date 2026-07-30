//! Protocol round-trip and framing tests.

use super::*;
use crate::cell::Cell;
use std::io::Cursor;

// ------------------------------------------------------------------------
// Serialization Round-Trip Tests
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

