//! Length-prefixed framing over the socket.

use std::io::{self, Read, Write};

use serde::{Deserialize, Serialize};

use super::MAX_MESSAGE_SIZE;

/// Error type for protocol operations.
#[derive(Debug, thiserror::Error)]
pub enum ProtocolError {
    #[error("IO error: {0}")]
    Io(#[from] io::Error),

    #[error("Serialization error: {0}")]
    Serialization(#[from] bincode::Error),

    #[error("Message too large: {size} bytes (max {MAX_MESSAGE_SIZE})")]
    MessageTooLarge { size: u32 },

    #[error("Connection closed")]
    ConnectionClosed,
}

/// Result type for protocol operations.
pub type ProtocolResult<T> = Result<T, ProtocolError>;

/// Write a message to a writer with length-prefixed framing.
pub fn write_message<W: Write, M: Serialize>(writer: &mut W, message: &M) -> ProtocolResult<()> {
    let payload = bincode::serialize(message)?;
    let len = payload.len() as u32;

    if len > MAX_MESSAGE_SIZE {
        return Err(ProtocolError::MessageTooLarge { size: len });
    }

    writer.write_all(&len.to_le_bytes())?;
    writer.write_all(&payload)?;
    writer.flush()?;

    Ok(())
}

/// Read a message from a reader with length-prefixed framing.
pub fn read_message<R: Read, M: for<'de> Deserialize<'de>>(reader: &mut R) -> ProtocolResult<M> {
    let mut len_buf = [0u8; 4];

    match reader.read_exact(&mut len_buf) {
        Ok(()) => {}
        Err(e) if e.kind() == io::ErrorKind::UnexpectedEof => {
            return Err(ProtocolError::ConnectionClosed);
        }
        Err(e) => return Err(ProtocolError::Io(e)),
    }

    let len = u32::from_le_bytes(len_buf);

    if len > MAX_MESSAGE_SIZE {
        return Err(ProtocolError::MessageTooLarge { size: len });
    }

    let mut payload = vec![0u8; len as usize];
    match reader.read_exact(&mut payload) {
        Ok(()) => {}
        Err(e) if e.kind() == io::ErrorKind::UnexpectedEof => {
            return Err(ProtocolError::ConnectionClosed);
        }
        Err(e) => return Err(ProtocolError::Io(e)),
    }

    let message = bincode::deserialize(&payload)?;
    Ok(message)
}

// ============================================================================
// Tests
// ============================================================================
