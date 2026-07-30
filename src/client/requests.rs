//! The client request/response API.

use super::{Client, ClientError, ClientResult};
use crate::protocol::{ClientMessage, ServerMessage};

impl Client {
    /// Attach to a session.
    pub fn attach(&mut self, session_name: Option<String>, create: bool) -> ClientResult<()> {
        self.connection.send(&ClientMessage::Attach {
            session_name: session_name.clone(),
            create,
        })?;

        let response = self.connection.recv()?;
        match response {
            ServerMessage::Attached {
                session_id,
                session_name,
            } => {
                log::info!("Attached to session '{}' (id={})", session_name, session_id);
                self.session_id = Some(session_id);
                self.session_name = Some(session_name);
                Ok(())
            }
            ServerMessage::Error { message } => {
                if message.contains("not found") {
                    Err(ClientError::SessionNotFound(
                        session_name.unwrap_or_else(|| "default".to_string()),
                    ))
                } else {
                    Err(ClientError::ServerError(message))
                }
            }
            other => Err(ClientError::UnexpectedResponse(other)),
        }
    }
    /// Detach from the current session.
    pub fn detach(&mut self) -> ClientResult<()> {
        self.connection.send(&ClientMessage::Detach)?;

        loop {
            match self.connection.recv()? {
                ServerMessage::Detached { reason } => {
                    log::info!("Detached from session: {:?}", reason);
                    self.session_id = None;
                    self.session_name = None;
                    return Ok(());
                }
                ServerMessage::Error { message } => return Err(ClientError::ServerError(message)),
                ServerMessage::MouseMode { .. }
                | ServerMessage::LayoutChanged { .. }
                | ServerMessage::PaneUpdate { .. } => {
                    log::debug!("Ignoring async message while waiting for detach confirmation");
                }
                other => return Err(ClientError::UnexpectedResponse(other)),
            }
        }
    }
    /// Send input to the server.
    pub fn send_input(&mut self, bytes: Vec<u8>) -> ClientResult<()> {
        self.connection.send(&ClientMessage::Input(bytes))?;
        Ok(())
    }
    /// Send a resize notification.
    pub fn send_resize(&mut self, cols: u16, rows: u16) -> ClientResult<()> {
        self.config.term_cols = cols;
        self.config.term_rows = rows;
        self.connection
            .send(&ClientMessage::Resize { cols, rows })?;
        Ok(())
    }
    /// Scroll the focused pane's view.
    ///
    /// Positive goes back in history, negative comes forward, zero returns to the
    /// live view.
    pub fn send_scroll(&mut self, lines: i32) -> ClientResult<()> {
        self.connection.send(&ClientMessage::Scroll { lines })?;
        Ok(())
    }
    /// Send a command action.
    pub fn send_command(&mut self, action: crate::protocol::CommandAction) -> ClientResult<()> {
        self.connection.send(&ClientMessage::Command(action))?;
        Ok(())
    }
    /// List all sessions.
    pub fn list_sessions(&mut self) -> ClientResult<Vec<crate::protocol::SessionInfo>> {
        self.connection.send(&ClientMessage::ListSessions)?;

        let response = self.connection.recv()?;
        match response {
            ServerMessage::SessionList(sessions) => Ok(sessions),
            ServerMessage::Error { message } => Err(ClientError::ServerError(message)),
            other => Err(ClientError::UnexpectedResponse(other)),
        }
    }
    /// Kill a session by name.
    pub fn kill_session(&mut self, name: &str) -> ClientResult<()> {
        self.connection.send(&ClientMessage::KillSession {
            name: name.to_string(),
        })?;
        Ok(())
    }
    /// Shut the server down cleanly.
    pub fn shutdown_server(&mut self) -> ClientResult<()> {
        if self.server_version < 3 {
            return Err(ClientError::UnsupportedServerVersion {
                required: 3,
                actual: self.server_version,
            });
        }

        self.connection.send(&ClientMessage::ShutdownServer)?;

        match self.connection.recv() {
            Ok(ServerMessage::Shutdown) => Ok(()),
            Ok(ServerMessage::Error { message }) => Err(ClientError::ServerError(message)),
            Ok(other) => Err(ClientError::UnexpectedResponse(other)),
            Err(crate::protocol::ProtocolError::ConnectionClosed) => Ok(()),
            Err(e) => Err(ClientError::Protocol(e)),
        }
    }
    /// Send a ping and wait for pong.
    pub fn ping(&mut self) -> ClientResult<()> {
        self.connection.send(&ClientMessage::Ping)?;

        let response = self.connection.recv()?;
        match response {
            ServerMessage::Pong => Ok(()),
            ServerMessage::Error { message } => Err(ClientError::ServerError(message)),
            other => Err(ClientError::UnexpectedResponse(other)),
        }
    }
    /// Try to receive a message (non-blocking).
    pub fn try_recv(&mut self) -> ClientResult<Option<ServerMessage>> {
        if let Some(tunnel) = self.tunnel.as_mut() {
            tunnel.ensure_running()?;
        }
        Ok(self.connection.try_recv()?)
    }
    /// Receive a message (blocking).
    pub fn recv(&mut self) -> ClientResult<ServerMessage> {
        if let Some(tunnel) = self.tunnel.as_mut() {
            tunnel.ensure_running()?;
        }
        Ok(self.connection.recv()?)
    }
    /// Check if connected and attached to a session.
    pub fn is_attached(&self) -> bool {
        self.session_id.is_some()
    }
    /// Get the current session name.
    pub fn session_name(&self) -> Option<&str> {
        self.session_name.as_deref()
    }

    /// Get the raw file descriptor for polling.
    pub fn as_raw_fd(&self) -> std::os::unix::io::RawFd {
        self.connection.as_raw_fd()
    }
}
