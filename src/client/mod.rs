//! Client module for clux.
//!
//! The client connects to the server, sends input, and renders
//! screen updates to the host terminal.

mod connect;
mod connect_remote;
mod connection;
mod error;
pub mod mouse;
mod remote;
mod requests;
pub mod screen;

pub use connection::ServerConnection;
pub use error::{ClientError, ClientResult};
pub use mouse::encode_mouse_sgr;
pub use screen::{
    cells_to_ansi, cells_to_ansi_with_links, ScreenBuffer, BEGIN_SYNC_UPDATE, END_SYNC_UPDATE,
};

use std::path::{Path, PathBuf};

use crate::server::default_socket_path;

/// Name of the server binary the client spawns on first use.
const SERVER_BINARY: &str = "clux-server";

use self::remote::SshTunnel;

/// Where the client should connect.
#[derive(Debug, Clone)]
pub enum ClientTarget {
    /// Connect to a local server socket.
    Local { socket_path: PathBuf },
    /// Reach a remote server over SSH via a forwarded local Unix socket.
    RemoteSsh {
        destination: String,
        socket_path: PathBuf,
    },
}

impl ClientTarget {
    /// Get the target server socket path.
    pub fn socket_path(&self) -> &Path {
        match self {
            Self::Local { socket_path } | Self::RemoteSsh { socket_path, .. } => socket_path,
        }
    }

    /// Get the remote SSH destination if configured.
    pub fn remote_destination(&self) -> Option<&str> {
        match self {
            Self::RemoteSsh { destination, .. } => Some(destination),
            Self::Local { .. } => None,
        }
    }
}

/// Client configuration.
#[derive(Debug, Clone)]
pub struct ClientConfig {
    /// Connection target.
    pub target: ClientTarget,
    /// Terminal type ($TERM).
    pub term_type: String,
    /// Terminal dimensions.
    pub term_cols: u16,
    pub term_rows: u16,
}

impl Default for ClientConfig {
    fn default() -> Self {
        let (cols, rows) = crossterm::terminal::size().unwrap_or((80, 24));

        Self {
            target: ClientTarget::Local {
                socket_path: default_socket_path(),
            },
            term_type: std::env::var("TERM").unwrap_or_else(|_| "xterm-256color".to_string()),
            term_cols: cols,
            term_rows: rows,
        }
    }
}

/// The clux client.
pub struct Client {
    /// Client configuration.
    config: ClientConfig,
    /// Connection to the server.
    connection: ServerConnection,
    /// Active SSH tunnel for remote mode.
    tunnel: Option<SshTunnel>,
    /// Negotiated server protocol version.
    server_version: u32,
    /// Current session ID (if attached).
    session_id: Option<u32>,
    /// Current session name (if attached).
    session_name: Option<String>,
}

impl Client {
    /// Get the target server socket path.
    pub fn socket_path(&self) -> &Path {
        self.config.target.socket_path()
    }

    /// Get the target remote destination, if any.
    pub fn remote_destination(&self) -> Option<&str> {
        self.config.target.remote_destination()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_client_config_default() {
        let config = ClientConfig::default();
        assert!(!config.term_type.is_empty());
        assert!(config.term_cols > 0);
        assert!(config.term_rows > 0);
    }

    #[test]
    fn test_socket_path_default() {
        let config = ClientConfig::default();
        assert!(config
            .target
            .socket_path()
            .to_string_lossy()
            .contains("clux"));
    }

    #[test]
    fn test_remote_target_accessors() {
        let target = ClientTarget::RemoteSsh {
            destination: "devbox".to_string(),
            socket_path: PathBuf::from("/tmp/clux.sock"),
        };

        assert_eq!(target.remote_destination(), Some("devbox"));
        assert_eq!(target.socket_path(), Path::new("/tmp/clux.sock"));
    }
}
