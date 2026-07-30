//! Client connection management for the server.
//!
//! Each connected client has a ClientConnection that tracks its state
//! and handles message serialization/deserialization.

use std::io::{self, Read};
use std::os::unix::io::{AsRawFd, RawFd};
use std::os::unix::net::UnixStream;

use crate::protocol::{
    write_message, ClientMessage, MessageReader, ProtocolError, ProtocolResult, ServerMessage,
};
use crate::session::{ClientId, SessionId};

/// State of a connected client.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ClientState {
    /// Just connected, waiting for Hello.
    Connected,
    /// Received Hello, ready to attach.
    Ready,
    /// Attached to a session.
    Attached(SessionId),
}

/// A connected client.
pub struct ClientConnection {
    /// Client identifier.
    pub id: ClientId,
    /// The Unix stream for this client.
    stream: UnixStream,
    /// Current state.
    pub state: ClientState,
    /// Buffer for reading partial messages.
    reader: MessageReader,
    /// Whether the connection is still alive.
    alive: bool,
}

impl ClientConnection {
    /// Create a new client connection.
    pub fn new(id: ClientId, stream: UnixStream) -> Self {
        // Set to non-blocking mode
        if let Err(e) = stream.set_nonblocking(true) {
            log::warn!("Failed to set non-blocking mode for client {:?}: {}", id, e);
        }

        Self {
            id,
            stream,
            state: ClientState::Connected,
            reader: MessageReader::new(),
            alive: true,
        }
    }

    /// Check if the connection is still alive.
    pub fn is_alive(&self) -> bool {
        self.alive
    }

    /// Try to read a complete message from the client.
    /// Returns Ok(Some(message)) if a complete message was received,
    /// Ok(None) if more data is needed, or Err on error.
    pub fn try_read_message(&mut self) -> ProtocolResult<Option<ClientMessage>> {
        // Read available data
        let mut buf = [0u8; 4096];

        loop {
            match self.stream.read(&mut buf) {
                Ok(0) => {
                    // Connection closed
                    self.alive = false;
                    return Err(ProtocolError::ConnectionClosed);
                }
                Ok(n) => {
                    // Feed data to the reader
                    if let Some(msg) = self.reader.feed(&buf[..n])? {
                        return Ok(Some(msg));
                    }
                    // Continue reading if more data might be available
                }
                Err(e) if e.kind() == io::ErrorKind::WouldBlock => {
                    // No more data available right now
                    // Check if we have a complete message in the buffer
                    return self.reader.feed(&[]);
                }
                Err(e) if e.kind() == io::ErrorKind::Interrupted => {
                    // Interrupted, retry
                    continue;
                }
                Err(e) => {
                    self.alive = false;
                    return Err(ProtocolError::Io(e));
                }
            }
        }
    }

    /// Send a message to this client.
    /// Uses blocking mode for writes to handle large messages that exceed socket buffer.
    pub fn send_message(&mut self, message: &ServerMessage) -> ProtocolResult<()> {
        if !self.alive {
            return Err(ProtocolError::ConnectionClosed);
        }

        // Temporarily set to blocking mode for writes to handle large messages
        if let Err(e) = self.stream.set_nonblocking(false) {
            log::warn!("Failed to set blocking mode for write: {}", e);
        }

        let result = write_message(&mut self.stream, message);

        // Restore non-blocking mode for reads
        if let Err(e) = self.stream.set_nonblocking(true) {
            log::warn!("Failed to restore non-blocking mode: {}", e);
        }

        match result {
            Ok(()) => Ok(()),
            Err(e) => {
                self.alive = false;
                Err(e)
            }
        }
    }

    /// Get the raw file descriptor for polling.
    pub fn as_raw_fd(&self) -> RawFd {
        self.stream.as_raw_fd()
    }
}

impl AsRawFd for ClientConnection {
    fn as_raw_fd(&self) -> RawFd {
        self.stream.as_raw_fd()
    }
}

#[cfg(test)]
mod tests;
