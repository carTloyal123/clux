//! Client connection management for the server.
//!
//! Each connected client has a ClientConnection that tracks its state
//! and handles message serialization/deserialization.

use std::io::{self, Read};
use std::os::unix::io::{AsRawFd, RawFd};
use std::os::unix::net::UnixStream;

use crate::protocol::{
    write_message, ClientCapabilities, ClientMessage, MessageReader, ProtocolError, ProtocolResult,
    ServerMessage,
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
    /// Client capabilities (set after Hello handshake).
    pub capabilities: Option<ClientCapabilities>,
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
            capabilities: None,
        }
    }

    /// Check if the connection is still alive.
    pub fn is_alive(&self) -> bool {
        self.alive
    }

    /// Check if this client supports pane updates (v2 protocol).
    pub fn supports_pane_updates(&self) -> bool {
        self.capabilities
            .as_ref()
            .map(|c| c.supports_pane_updates)
            .unwrap_or(false)
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
mod tests {
    use super::*;
    use crate::protocol::{read_message, write_message, PROTOCOL_VERSION};
    use std::io::Write;
    use std::os::unix::net::UnixStream;

    fn create_test_pair() -> (ClientConnection, UnixStream) {
        let (server_stream, client_stream) = UnixStream::pair().unwrap();
        let conn = ClientConnection::new(ClientId(0), server_stream);
        (conn, client_stream)
    }

    #[test]
    fn test_client_connection_creation() {
        let (conn, _client) = create_test_pair();

        assert_eq!(conn.id, ClientId(0));
        assert_eq!(conn.state, ClientState::Connected);
        assert!(conn.is_alive());
    }

    #[test]
    fn test_client_send_message() {
        let (mut conn, mut client) = create_test_pair();
        client.set_nonblocking(false).unwrap();

        // Send a message
        let msg = ServerMessage::Pong;
        conn.send_message(&msg).unwrap();

        // Read it on the client side
        let received: ServerMessage = read_message(&mut client).unwrap();
        assert_eq!(received, ServerMessage::Pong);
    }

    #[test]
    fn test_client_receive_message() {
        let (mut conn, mut client) = create_test_pair();
        client.set_nonblocking(false).unwrap();

        // Send a message from client
        let msg = ClientMessage::Ping;
        write_message(&mut client, &msg).unwrap();

        // Receive it on the server side
        // Need a small delay for the data to arrive
        std::thread::sleep(std::time::Duration::from_millis(10));

        let received = conn.try_read_message().unwrap();
        assert_eq!(received, Some(ClientMessage::Ping));
    }

    #[test]
    fn test_client_partial_message() {
        let (mut conn, mut client) = create_test_pair();
        client.set_nonblocking(false).unwrap();

        // Serialize a message
        let msg = ClientMessage::Hello {
            version: PROTOCOL_VERSION,
            term_cols: 80,
            term_rows: 24,
            term_type: "xterm".to_string(),
            capabilities: None,
        };
        let mut buf = Vec::new();
        write_message(&mut buf, &msg).unwrap();

        // Send first half
        client.write_all(&buf[..buf.len() / 2]).unwrap();
        std::thread::sleep(std::time::Duration::from_millis(10));

        // Should return None (incomplete)
        let result = conn.try_read_message().unwrap();
        assert!(result.is_none());

        // Send second half
        client.write_all(&buf[buf.len() / 2..]).unwrap();
        std::thread::sleep(std::time::Duration::from_millis(10));

        // Should return the complete message
        let result = conn.try_read_message().unwrap();
        assert_eq!(result, Some(msg));
    }

    #[test]
    fn test_client_connection_closed() {
        let (mut conn, client) = create_test_pair();

        // Close the client side
        drop(client);

        // Try to read - should get ConnectionClosed
        std::thread::sleep(std::time::Duration::from_millis(10));
        let result = conn.try_read_message();
        assert!(matches!(result, Err(ProtocolError::ConnectionClosed)));
        assert!(!conn.is_alive());
    }

    #[test]
    fn test_client_state_transitions() {
        let (mut conn, _client) = create_test_pair();

        assert_eq!(conn.state, ClientState::Connected);

        conn.state = ClientState::Ready;
        assert_eq!(conn.state, ClientState::Ready);

        let session_id = crate::session::SessionId(1);
        conn.state = ClientState::Attached(session_id);
        assert_eq!(conn.state, ClientState::Attached(session_id));
    }
}
