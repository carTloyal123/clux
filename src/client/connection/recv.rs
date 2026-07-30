//! Receiving and framing messages from the server.

use std::io::ErrorKind;

use super::msg_type;
use crate::protocol::{ProtocolError, ProtocolResult, ServerMessage};
impl super::ServerConnection {
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
            match self.read_into(&mut buf) {
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
        self.set_nonblocking(true).map_err(ProtocolError::Io)?;

        let result = self.try_recv_internal();

        // Restore blocking mode
        self.set_nonblocking(false).map_err(ProtocolError::Io)?;

        if let Ok(Some(ref msg)) = result {
            log::debug!("try_recv got message: {:?}", msg_type(msg));
        }

        result
    }
    /// Internal non-blocking receive.
    pub(super) fn try_recv_internal(&mut self) -> ProtocolResult<Option<ServerMessage>> {
        // First check the buffer
        if let Some(msg) = self.try_recv_from_buffer()? {
            return Ok(Some(msg));
        }

        // Try to read more data
        let mut buf = [0u8; 4096];
        loop {
            match self.read_into(&mut buf) {
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
    pub(super) fn try_recv_from_buffer(&mut self) -> ProtocolResult<Option<ServerMessage>> {
        self.reader.feed(&[])
    }
}
