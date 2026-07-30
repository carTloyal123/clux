//! Server module for clux.
//!
//! The server manages sessions, handles client connections, and routes
//! PTY I/O between shells and connected clients.

mod attach;
mod broadcast;
mod client_conn;
mod clients;
mod commands;
mod config;
mod input;
mod lifecycle;
mod listener;
mod messages;
mod pty_events;
mod pty_registry;
mod sessions;
mod shutdown;
pub mod stdio_bridge;

pub use client_conn::{ClientConnection, ClientState};
pub use config::{AutoShutdownConfig, ServerConfig};
pub use listener::SocketListener;

use std::collections::HashMap;
use std::io::{self};
use std::path::PathBuf;
use std::time::Instant;

use nix::libc;

use mio::{Poll, Token};

use crate::protocol::{ProtocolError, ServerMessage};
use crate::session::{ClientId, SessionManager};

/// Token for the listener socket.
const LISTENER_TOKEN: Token = Token(0);

/// Base token for client connections (CLIENT_BASE + client_id).
const CLIENT_TOKEN_BASE: usize = 1000;

/// Base token for PTY file descriptors (PTY_BASE + session_id * 1000 + pane_index).
const PTY_TOKEN_BASE: usize = 100_000;

/// Get the default socket path for the current user.
pub fn default_socket_path() -> PathBuf {
    let uid = unsafe { libc::getuid() };
    let dir = PathBuf::from(format!("/tmp/clux-{}", uid));
    dir.join("clux.sock")
}

/// Server error type.
#[derive(Debug, thiserror::Error)]
pub enum ServerError {
    #[error("IO error: {0}")]
    Io(#[from] io::Error),

    #[error("Protocol error: {0}")]
    Protocol(#[from] ProtocolError),

    #[error("Session error: {0}")]
    Session(#[from] crate::session::SessionError),

    #[error("Failed to create socket directory: {0}")]
    SocketDir(io::Error),
}

/// Result type for server operations.
pub type ServerResult<T> = Result<T, ServerError>;

/// The clux server.
pub struct Server {
    /// Server configuration.
    config: ServerConfig,
    /// mio poll instance.
    poll: Poll,
    /// Socket listener.
    listener: SocketListener,
    /// Session manager.
    sessions: SessionManager,
    /// Connected clients.
    clients: HashMap<ClientId, ClientConnection>,
    /// Mapping from client token to client ID.
    token_to_client: HashMap<Token, ClientId>,
    /// Mapping from PTY token to (SessionId, PaneId).
    token_to_pty: HashMap<Token, (crate::session::SessionId, crate::pane::PaneId)>,
    /// Client terminal sizes (for multi-client resize calculation).
    client_sizes: HashMap<ClientId, (u16, u16)>,
    /// Next client ID to assign.
    next_client_id: u32,
    /// Whether the server should continue running.
    running: bool,
    /// Auto-shutdown configuration.
    auto_shutdown: AutoShutdownConfig,
    /// Time when the server started (for first-session timeout).
    started_at: Instant,
    /// Time when shutdown became pending (last session closed).
    /// None if sessions exist or shutdown not pending.
    shutdown_pending_since: Option<Instant>,
    /// Whether a session has ever been created (to distinguish startup from post-session state).
    session_ever_created: bool,
}

impl Drop for Server {
    fn drop(&mut self) {
        // Notify all clients of shutdown
        for client_id in self.clients.keys().copied().collect::<Vec<_>>() {
            let _ = self.send_to_client(client_id, &ServerMessage::Shutdown);
        }

        // Socket file is cleaned up by SocketListener drop
    }
}

/// The rows a pane update has to carry: the dirty rows plus any extra row a

/// resolved link reaches onto.
pub(super) fn rows_with_links(
    dirty_rows: &[u16],
    links: &std::collections::HashMap<u16, Vec<crate::urls::LinkRun>>,
) -> Vec<u16> {
    let mut rows: Vec<u16> = dirty_rows.to_vec();
    for &row in links.keys() {
        if !rows.contains(&row) {
            rows.push(row);
        }
    }
    rows.sort_unstable();
    rows.dedup();
    rows
}

/// Helper to get a short description of a server message for logging.
pub(super) fn message_type(msg: &ServerMessage) -> &'static str {
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
