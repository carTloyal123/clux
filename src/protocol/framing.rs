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

    #[error("Protocol version mismatch: client={client}, server={server}")]
    VersionMismatch { client: u32, server: u32 },
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

/// Non-blocking message reader that handles partial reads.
/// Returns None if not enough data is available yet.
pub struct MessageReader {
    /// Buffer for accumulating data.
    buffer: Vec<u8>,
    /// Expected message length (None if reading length prefix).
    expected_len: Option<u32>,
}

impl MessageReader {
    /// Create a new message reader.
    pub fn new() -> Self {
        Self {
            buffer: Vec::new(),
            expected_len: None,
        }
    }

    /// Feed bytes into the reader and try to parse a complete message.
    /// Returns Ok(Some(message)) if a complete message was parsed,
    /// Ok(None) if more data is needed, or Err on protocol errors.
    pub fn feed<M: for<'de> Deserialize<'de>>(&mut self, data: &[u8]) -> ProtocolResult<Option<M>> {
        self.buffer.extend_from_slice(data);

        // Try to read length prefix if we don't have it yet
        if self.expected_len.is_none() && self.buffer.len() >= 4 {
            let mut len_bytes = [0u8; 4];
            len_bytes.copy_from_slice(&self.buffer[..4]);
            let len = u32::from_le_bytes(len_bytes);

            if len > MAX_MESSAGE_SIZE {
                return Err(ProtocolError::MessageTooLarge { size: len });
            }

            self.expected_len = Some(len);
        }

        // Try to read the message payload
        if let Some(len) = self.expected_len {
            let total_needed = 4 + len as usize;

            if self.buffer.len() >= total_needed {
                // We have a complete message
                let payload = &self.buffer[4..total_needed];
                let message = bincode::deserialize(payload);

                // Consume the complete frame regardless of decode success so callers
                // can continue reading subsequent frames from the stream.
                self.buffer.drain(..total_needed);
                self.expected_len = None;

                return match message {
                    Ok(message) => Ok(Some(message)),
                    Err(e) => Err(ProtocolError::Serialization(e)),
                };
            }
        }

        // Need more data
        Ok(None)
    }

    /// Check if the buffer is empty.
    pub fn is_empty(&self) -> bool {
        self.buffer.is_empty()
    }

    /// Get the number of bytes currently buffered.
    pub fn buffered_len(&self) -> usize {
        self.buffer.len()
    }
}

impl Default for MessageReader {
    fn default() -> Self {
        Self::new()
    }
}

/// Non-blocking message writer that handles partial writes.
pub struct MessageWriter {
    /// Buffer for pending data.
    buffer: Vec<u8>,
    /// Number of bytes already written from the buffer.
    written: usize,
}

impl MessageWriter {
    /// Create a new message writer.
    pub fn new() -> Self {
        Self {
            buffer: Vec::new(),
            written: 0,
        }
    }

    /// Queue a message for writing.
    pub fn queue<M: Serialize>(&mut self, message: &M) -> ProtocolResult<()> {
        let payload = bincode::serialize(message)?;
        let len = payload.len() as u32;

        if len > MAX_MESSAGE_SIZE {
            return Err(ProtocolError::MessageTooLarge { size: len });
        }

        self.buffer.extend_from_slice(&len.to_le_bytes());
        self.buffer.extend_from_slice(&payload);

        Ok(())
    }

    /// Try to write pending data to the writer.
    /// Returns Ok(true) if all data was written, Ok(false) if more writes needed.
    pub fn flush<W: Write>(&mut self, writer: &mut W) -> ProtocolResult<bool> {
        while self.written < self.buffer.len() {
            match writer.write(&self.buffer[self.written..]) {
                Ok(0) => {
                    // Zero write means the stream is closed.
                    return Err(ProtocolError::ConnectionClosed);
                }
                Ok(n) => {
                    self.written += n;
                }
                Err(e) if e.kind() == io::ErrorKind::WouldBlock => {
                    return Ok(false);
                }
                Err(e) => return Err(ProtocolError::Io(e)),
            }
        }

        // All data written, clear buffer
        self.buffer.clear();
        self.written = 0;

        Ok(true)
    }

    /// Check if there's pending data to write.
    pub fn has_pending(&self) -> bool {
        self.written < self.buffer.len()
    }

    /// Get the number of bytes pending to be written.
    pub fn pending_len(&self) -> usize {
        self.buffer.len() - self.written
    }
}

impl Default for MessageWriter {
    fn default() -> Self {
        Self::new()
    }
}

// ============================================================================
// Tests
// ============================================================================
