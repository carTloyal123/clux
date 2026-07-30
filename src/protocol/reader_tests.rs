//! Framing tests.

use super::*;
use std::io::{self, Cursor, Write};

// ------------------------------------------------------------------------
// Serialization Round-Trip Tests
// ------------------------------------------------------------------------

#[test]
fn test_message_reader_complete_message() {
    let mut writer = Vec::new();
    let msg = ClientMessage::Ping;
    write_message(&mut writer, &msg).unwrap();

    let mut reader = MessageReader::new();
    let result: Option<ClientMessage> = reader.feed(&writer).unwrap();

    assert_eq!(result, Some(msg));
    assert!(reader.is_empty());
}

#[test]
fn test_message_reader_partial_length() {
    let mut writer = Vec::new();
    let msg = ClientMessage::Ping;
    write_message(&mut writer, &msg).unwrap();

    let mut reader = MessageReader::new();

    // Feed only 2 bytes (partial length)
    let result: Option<ClientMessage> = reader.feed(&writer[..2]).unwrap();
    assert_eq!(result, None);
    assert_eq!(reader.buffered_len(), 2);

    // Feed the rest
    let result: Option<ClientMessage> = reader.feed(&writer[2..]).unwrap();
    assert_eq!(result, Some(msg));
    assert!(reader.is_empty());
}

#[test]
fn test_message_reader_partial_payload() {
    let mut writer = Vec::new();
    let msg = ClientMessage::Input(vec![1, 2, 3, 4, 5, 6, 7, 8, 9, 10]);
    write_message(&mut writer, &msg).unwrap();

    let mut reader = MessageReader::new();

    // Feed length + partial payload
    let result: Option<ClientMessage> = reader.feed(&writer[..6]).unwrap();
    assert_eq!(result, None);

    // Feed more
    let result: Option<ClientMessage> = reader.feed(&writer[6..10]).unwrap();
    assert_eq!(result, None);

    // Feed the rest
    let result: Option<ClientMessage> = reader.feed(&writer[10..]).unwrap();
    assert_eq!(result, Some(msg));
}

#[test]
fn test_message_reader_multiple_messages() {
    let mut writer = Vec::new();
    let msg1 = ClientMessage::Ping;
    let msg2 = ClientMessage::Detach;
    write_message(&mut writer, &msg1).unwrap();
    write_message(&mut writer, &msg2).unwrap();

    let mut reader = MessageReader::new();

    // Feed both messages at once
    let result1: Option<ClientMessage> = reader.feed(&writer).unwrap();
    assert_eq!(result1, Some(msg1));

    // Second message should still be in buffer
    let result2: Option<ClientMessage> = reader.feed(&[]).unwrap();
    assert_eq!(result2, Some(msg2));

    assert!(reader.is_empty());
}

#[test]
fn test_message_writer_queue_and_flush() {
    let mut writer = MessageWriter::new();

    writer.queue(&ClientMessage::Ping).unwrap();
    writer.queue(&ClientMessage::Detach).unwrap();

    assert!(writer.has_pending());

    let mut output = Vec::new();
    let complete = writer.flush(&mut output).unwrap();

    assert!(complete);
    assert!(!writer.has_pending());

    // Verify we can read the messages back
    let mut cursor = Cursor::new(output);
    let msg1: ClientMessage = read_message(&mut cursor).unwrap();
    let msg2: ClientMessage = read_message(&mut cursor).unwrap();

    assert_eq!(msg1, ClientMessage::Ping);
    assert_eq!(msg2, ClientMessage::Detach);
}

#[test]
fn test_message_reader_recovers_after_invalid_frame() {
    let mut bytes = Vec::new();

    // Invalid ClientMessage payload (length=1, payload cannot decode to valid enum)
    bytes.extend_from_slice(&1u32.to_le_bytes());
    bytes.push(0xff);

    // Followed by a valid frame
    write_message(&mut bytes, &ClientMessage::Ping).unwrap();

    let mut reader = MessageReader::new();
    let first: ProtocolResult<Option<ClientMessage>> = reader.feed(&bytes);
    assert!(matches!(first, Err(ProtocolError::Serialization(_))));

    // Reader should have consumed the invalid frame and be able to parse the next one
    let second: ProtocolResult<Option<ClientMessage>> = reader.feed(&[]);
    assert_eq!(second.unwrap(), Some(ClientMessage::Ping));
    assert!(reader.is_empty());
}

#[test]
fn test_message_reader_oversized_length_prefix() {
    let mut reader = MessageReader::new();
    let len = MAX_MESSAGE_SIZE + 1;
    let result: ProtocolResult<Option<ClientMessage>> = reader.feed(&len.to_le_bytes());
    assert!(matches!(result, Err(ProtocolError::MessageTooLarge { .. })));
}

struct ZeroWriter;

impl Write for ZeroWriter {
    fn write(&mut self, _buf: &[u8]) -> io::Result<usize> {
        Ok(0)
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

#[test]
fn test_message_writer_zero_write_is_connection_closed() {
    let mut writer = MessageWriter::new();
    writer.queue(&ClientMessage::Ping).unwrap();

    let mut zero = ZeroWriter;
    let result = writer.flush(&mut zero);
    assert!(matches!(result, Err(ProtocolError::ConnectionClosed)));
    assert!(writer.has_pending());
}

