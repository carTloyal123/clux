//! Buffered, non-blocking message reader and writer.

use std::io::{self, Write};

use serde::{Deserialize, Serialize};

use super::{ProtocolError, ProtocolResult, MAX_MESSAGE_SIZE};

/// Non-blocking message reader that handles partial reads.
/// Returns None if not enough data is available yet.
pub struct MessageReader {
    /// Buffer for accumulating data.
    buffer: Vec<u8>,
    /// Expected message length (None if reading length prefix).
    expected_len: Option<u32>,
}
/// Non-blocking message writer that handles partial writes.
pub struct MessageWriter {
    /// Buffer for pending data.
    buffer: Vec<u8>,
    /// Number of bytes already written from the buffer.
    written: usize,
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
