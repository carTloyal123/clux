//! Server connection handling for the client.
//!
//! Manages the Unix socket connection to the server, including
//! message sending and receiving.

use std::io::{self, ErrorKind, Read};
use std::os::unix::io::{AsRawFd, RawFd};
use std::os::unix::net::UnixStream;
use std::path::Path;
use std::process::{Child, ChildStdin, ChildStdout};
use std::time::Duration;

use nix::fcntl::{fcntl, FcntlArg, OFlag};

use crate::protocol::{
    write_message, ClientMessage, MessageReader, ProtocolError, ProtocolResult, ServerMessage,
};

/// Connection to the clux server.
pub struct ServerConnection {
    transport: ConnectionTransport,
    /// Buffer for reading partial messages.
    reader: MessageReader,
}

enum ConnectionTransport {
    Unix(UnixStream),
    SshStdio {
        child: Child,
        stdin: ChildStdin,
        stdout: ChildStdout,
    },
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
            transport: ConnectionTransport::Unix(stream),
            reader: MessageReader::new(),
        })
    }

    /// Wrap an SSH child process that bridges stdio to a remote server socket.
    pub fn from_ssh_stdio_child(mut child: Child) -> ProtocolResult<Self> {
        let stdin = child
            .stdin
            .take()
            .ok_or_else(|| io::Error::new(ErrorKind::BrokenPipe, "ssh child missing stdin"))?;
        let stdout = child
            .stdout
            .take()
            .ok_or_else(|| io::Error::new(ErrorKind::BrokenPipe, "ssh child missing stdout"))?;

        Ok(Self {
            transport: ConnectionTransport::SshStdio {
                child,
                stdin,
                stdout,
            },
            reader: MessageReader::new(),
        })
    }

    /// Send a message to the server.
    pub fn send(&mut self, message: &ClientMessage) -> ProtocolResult<()> {
        log::debug!("ServerConnection::send - {:?}", message);
        let result = match &mut self.transport {
            ConnectionTransport::Unix(stream) => write_message(stream, message),
            ConnectionTransport::SshStdio { stdin, .. } => write_message(stdin, message),
        };
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
    fn try_recv_internal(&mut self) -> ProtocolResult<Option<ServerMessage>> {
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
    fn try_recv_from_buffer(&mut self) -> ProtocolResult<Option<ServerMessage>> {
        self.reader.feed(&[])
    }

    /// Get the raw file descriptor for polling.
    pub fn as_raw_fd(&self) -> RawFd {
        match &self.transport {
            ConnectionTransport::Unix(stream) => stream.as_raw_fd(),
            ConnectionTransport::SshStdio { stdout, .. } => stdout.as_raw_fd(),
        }
    }

    /// Set the read timeout.
    pub fn set_read_timeout(&mut self, timeout: Option<Duration>) -> io::Result<()> {
        match &mut self.transport {
            ConnectionTransport::Unix(stream) => stream.set_read_timeout(timeout),
            ConnectionTransport::SshStdio { .. } => Ok(()),
        }
    }

    /// Set non-blocking mode.
    pub fn set_nonblocking(&mut self, nonblocking: bool) -> io::Result<()> {
        match &mut self.transport {
            ConnectionTransport::Unix(stream) => stream.set_nonblocking(nonblocking),
            ConnectionTransport::SshStdio { stdout, .. } => set_fd_nonblocking(stdout, nonblocking),
        }
    }

    fn read_into(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        match &mut self.transport {
            ConnectionTransport::Unix(stream) => stream.read(buf),
            ConnectionTransport::SshStdio { stdout, .. } => stdout.read(buf),
        }
    }
}

impl AsRawFd for ServerConnection {
    fn as_raw_fd(&self) -> RawFd {
        ServerConnection::as_raw_fd(self)
    }
}

impl Drop for ServerConnection {
    fn drop(&mut self) {
        if let ConnectionTransport::SshStdio { child, .. } = &mut self.transport {
            let _ = child.kill();
            let _ = child.wait();
        }
    }
}

fn set_fd_nonblocking<T: AsRawFd>(fd_owner: &T, nonblocking: bool) -> io::Result<()> {
    let flags = fcntl(fd_owner.as_raw_fd(), FcntlArg::F_GETFL)
        .map_err(|e| io::Error::from_raw_os_error(e as i32))?;
    let mut flags = OFlag::from_bits_truncate(flags);
    if nonblocking {
        flags.insert(OFlag::O_NONBLOCK);
    } else {
        flags.remove(OFlag::O_NONBLOCK);
    }
    fcntl(fd_owner.as_raw_fd(), FcntlArg::F_SETFL(flags))
        .map(|_| ())
        .map_err(|e| io::Error::from_raw_os_error(e as i32))
}

/// Helper to get a short description of a server message for logging.
fn msg_type(msg: &ServerMessage) -> &'static str {
    match msg {
        ServerMessage::HelloAck { .. } => "HelloAck",
        ServerMessage::Attached { .. } => "Attached",
        ServerMessage::Detached { .. } => "Detached",
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
mod tests;
