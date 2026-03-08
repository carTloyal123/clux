//! Server connection handling for the client.
//!
//! Manages the Unix socket connection to the server, including
//! message sending and receiving.

use std::io::{self, ErrorKind, Read};
use std::os::unix::io::{AsRawFd, RawFd};
use std::os::unix::net::UnixStream;
use std::path::Path;
use std::time::Duration;

use crate::protocol::{
    write_message, ClientMessage, MessageReader, ProtocolError, ProtocolResult, ServerMessage,
};

/// Connection to the clux server.
pub struct ServerConnection {
    /// The Unix stream.
    stream: UnixStream,
    /// Buffer for reading partial messages.
    reader: MessageReader,
}

impl ServerConnection {
    /// Connect to the server at the given socket path.
    pub fn connect(socket_path: &Path) -> ProtocolResult<Self> {
        log::debug!("ServerConnection::connect to {:?}", socket_path);
        let stream = UnixStream::connect(socket_path).map_err(|e| {
            log::debug!("Connection failed: {} (kind={:?})", e, e.kind());
            if e.kind() == ErrorKind::NotFound || e.kind() == ErrorKind::ConnectionRefused {
                ProtocolError::ConnectionClosed
            } else {
                ProtocolError::Io(e)
            }
        })?;

        log::debug!("Unix socket connected successfully");

        // Set read timeout for blocking recv
        stream
            .set_read_timeout(Some(Duration::from_secs(30)))
            .map_err(ProtocolError::Io)?;

        Ok(Self {
            stream,
            reader: MessageReader::new(),
        })
    }

    /// Send a message to the server.
    pub fn send(&mut self, message: &ClientMessage) -> ProtocolResult<()> {
        log::debug!("ServerConnection::send - {:?}", message);
        let result = write_message(&mut self.stream, message);
        if let Err(ref e) = result {
            log::error!("Failed to send message: {}", e);
        }
        result
    }

    /// Receive a message from the server (blocking).
    pub fn recv(&mut self) -> ProtocolResult<ServerMessage> {
        log::debug!("ServerConnection::recv (blocking)");

        // First check if we have a complete message buffered
        if let Some(msg) = self.try_recv_from_buffer()? {
            log::debug!("Got message from buffer: {:?}", msg_type(&msg));
            return Ok(msg);
        }

        // Read until we have a complete message
        let mut buf = [0u8; 4096];
        loop {
            match self.stream.read(&mut buf) {
                Ok(0) => {
                    log::warn!("Connection closed (read returned 0)");
                    return Err(ProtocolError::ConnectionClosed);
                }
                Ok(n) => {
                    log::trace!("Read {} bytes from server", n);
                    if let Some(msg) = self.reader.feed(&buf[..n])? {
                        log::debug!("Received complete message: {:?}", msg_type(&msg));
                        return Ok(msg);
                    }
                }
                Err(e) if e.kind() == ErrorKind::WouldBlock => {
                    // Timeout waiting for data
                    log::trace!("WouldBlock, continuing to wait...");
                    continue;
                }
                Err(e) if e.kind() == ErrorKind::Interrupted => continue,
                Err(e) => {
                    log::error!("Read error: {}", e);
                    return Err(ProtocolError::Io(e));
                }
            }
        }
    }

    /// Try to receive a message (non-blocking).
    /// Returns Ok(None) if no complete message is available.
    pub fn try_recv(&mut self) -> ProtocolResult<Option<ServerMessage>> {
        // Set non-blocking temporarily
        self.stream
            .set_nonblocking(true)
            .map_err(ProtocolError::Io)?;

        let result = self.try_recv_internal();

        // Restore blocking mode
        self.stream
            .set_nonblocking(false)
            .map_err(ProtocolError::Io)?;

        if let Ok(Some(ref msg)) = result {
            log::debug!("try_recv got message: {:?}", msg_type(msg));
        }

        result
    }

    /// Internal non-blocking receive.
    fn try_recv_internal(&mut self) -> ProtocolResult<Option<ServerMessage>> {
        // First check the buffer
        if let Some(msg) = self.try_recv_from_buffer()? {
            return Ok(Some(msg));
        }

        // Try to read more data
        let mut buf = [0u8; 4096];
        loop {
            match self.stream.read(&mut buf) {
                Ok(0) => {
                    log::warn!("try_recv: Connection closed");
                    return Err(ProtocolError::ConnectionClosed);
                }
                Ok(n) => {
                    log::trace!("try_recv: Read {} bytes", n);
                    if let Some(msg) = self.reader.feed(&buf[..n])? {
                        return Ok(Some(msg));
                    }
                }
                Err(e) if e.kind() == ErrorKind::WouldBlock => {
                    // No more data available
                    return Ok(None);
                }
                Err(e) if e.kind() == ErrorKind::Interrupted => continue,
                Err(e) => {
                    log::error!("try_recv error: {}", e);
                    return Err(ProtocolError::Io(e));
                }
            }
        }
    }

    /// Try to parse a message from the buffer.
    fn try_recv_from_buffer(&mut self) -> ProtocolResult<Option<ServerMessage>> {
        self.reader.feed(&[])
    }

    /// Get the raw file descriptor for polling.
    pub fn as_raw_fd(&self) -> RawFd {
        self.stream.as_raw_fd()
    }

    /// Set the read timeout.
    pub fn set_read_timeout(&mut self, timeout: Option<Duration>) -> io::Result<()> {
        self.stream.set_read_timeout(timeout)
    }

    /// Set non-blocking mode.
    pub fn set_nonblocking(&mut self, nonblocking: bool) -> io::Result<()> {
        self.stream.set_nonblocking(nonblocking)
    }
}

impl AsRawFd for ServerConnection {
    fn as_raw_fd(&self) -> RawFd {
        self.stream.as_raw_fd()
    }
}

/// Helper to get a short description of a server message for logging.
fn msg_type(msg: &ServerMessage) -> &'static str {
    match msg {
        ServerMessage::HelloAck { .. } => "HelloAck",
        ServerMessage::Attached { .. } => "Attached",
        ServerMessage::Detached { .. } => "Detached",
        ServerMessage::FullScreen { .. } => "FullScreen",
        ServerMessage::Update { .. } => "Update",
        ServerMessage::SessionList(_) => "SessionList",
        ServerMessage::Error { .. } => "Error",
        ServerMessage::Pong => "Pong",
        ServerMessage::Shutdown => "Shutdown",
        ServerMessage::MouseMode { .. } => "MouseMode",
        ServerMessage::LayoutChanged { .. } => "LayoutChanged",
        ServerMessage::PaneUpdate { .. } => "PaneUpdate",
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::protocol::{read_message, write_message, PROTOCOL_VERSION};
    use std::os::unix::net::UnixListener;
    use std::thread;
    use std::time::Duration;

    fn temp_socket_path() -> std::path::PathBuf {
        let uid = unsafe { nix::libc::getuid() };
        let id = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        std::path::PathBuf::from(format!("/tmp/clux-test-{}-{}.sock", uid, id))
    }

    #[test]
    fn test_connection_send_recv() {
        let socket_path = temp_socket_path();

        // Start a mock server
        let path_clone = socket_path.clone();
        let server_thread = thread::spawn(move || {
            let listener = UnixListener::bind(&path_clone).unwrap();
            let (mut stream, _) = listener.accept().unwrap();

            // Read Hello
            let msg: ClientMessage = read_message(&mut stream).unwrap();
            assert!(matches!(msg, ClientMessage::Hello { .. }));

            // Send HelloAck
            let response = ServerMessage::HelloAck {
                version: PROTOCOL_VERSION,
                server_pid: 12345,
            };
            write_message(&mut stream, &response).unwrap();
        });

        // Give the server time to start
        thread::sleep(Duration::from_millis(50));

        // Connect
        let mut conn = ServerConnection::connect(&socket_path).unwrap();

        // Send Hello
        let hello = ClientMessage::Hello {
            version: PROTOCOL_VERSION,
            term_cols: 80,
            term_rows: 24,
            term_type: "xterm".to_string(),
            capabilities: None,
        };
        conn.send(&hello).unwrap();

        // Receive response
        let response = conn.recv().unwrap();
        assert!(matches!(
            response,
            ServerMessage::HelloAck {
                version: PROTOCOL_VERSION,
                ..
            }
        ));

        server_thread.join().unwrap();

        // Clean up
        let _ = std::fs::remove_file(&socket_path);
    }

    #[test]
    fn test_connection_to_nonexistent() {
        let socket_path = temp_socket_path();

        let result = ServerConnection::connect(&socket_path);
        assert!(result.is_err());
    }

    #[test]
    fn test_connection_multiple_messages() {
        let socket_path = temp_socket_path();

        // Start a mock server
        let path_clone = socket_path.clone();
        let server_thread = thread::spawn(move || {
            let listener = UnixListener::bind(&path_clone).unwrap();
            let (mut stream, _) = listener.accept().unwrap();

            // Echo back whatever we receive as Pong
            for _ in 0..3 {
                let _: ClientMessage = read_message(&mut stream).unwrap();
                write_message(&mut stream, &ServerMessage::Pong).unwrap();
            }
        });

        thread::sleep(Duration::from_millis(50));

        let mut conn = ServerConnection::connect(&socket_path).unwrap();

        // Send and receive multiple messages
        for _ in 0..3 {
            conn.send(&ClientMessage::Ping).unwrap();
            let response = conn.recv().unwrap();
            assert_eq!(response, ServerMessage::Pong);
        }

        server_thread.join().unwrap();
        let _ = std::fs::remove_file(&socket_path);
    }
}
