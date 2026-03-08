//! Client module for clux.
//!
//! The client connects to the server, sends input, and renders
//! screen updates to the host terminal.

mod connection;
mod remote;
pub mod screen;

pub use connection::ServerConnection;
pub use screen::{cells_to_ansi, ScreenBuffer};

use std::io;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::Duration;

use crate::protocol::{ClientCapabilities, ClientMessage, ServerMessage, PROTOCOL_VERSION};
use crate::server::default_socket_path;

use self::remote::{
    bootstrap_remote_server, start_remote_server, start_ssh_tunnel, wait_for_remote_socket,
    SshTunnel,
};

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

/// Client error type.
#[derive(Debug, thiserror::Error)]
pub enum ClientError {
    #[error("IO error: {0}")]
    Io(#[from] io::Error),

    #[error("Protocol error: {0}")]
    Protocol(#[from] crate::protocol::ProtocolError),

    #[error("Server not running and failed to start")]
    ServerNotRunning,

    #[error("Connection failed after {0} attempts")]
    ConnectionFailed(u32),

    #[error("Handshake failed: {0}")]
    HandshakeFailed(String),

    #[error("Session not found: {0}")]
    SessionNotFound(String),

    #[error("Server error: {0}")]
    ServerError(String),

    #[error("Unexpected response: {0:?}")]
    UnexpectedResponse(ServerMessage),

    #[error("Disconnected: {0}")]
    Disconnected(String),

    #[error("ssh is required for --remote mode but was not found in PATH")]
    SshUnavailable,

    #[error("Failed to start remote server: {0}")]
    RemoteStartupFailed(String),

    #[error("SSH tunnel failed: {0}")]
    RemoteTunnelFailed(String),

    #[error("Unsupported remote platform: {os}/{arch}")]
    RemotePlatformUnsupported { os: String, arch: String },

    #[error("No release artifact found for clux-server v{version} ({target}) at {url}")]
    RemoteArtifactUnavailable {
        version: String,
        target: String,
        url: String,
    },

    #[error("Remote bootstrap failed: {0}")]
    RemoteBootstrapFailed(String),

    #[error("Neither curl nor wget is available on the remote host")]
    RemoteMissingDownloadTool,

    #[error("Invalid repository metadata for remote bootstrap: {0}")]
    InvalidRepositoryMetadata(String),

    #[error(
        "Server protocol version {actual} does not support this operation (requires {required})"
    )]
    UnsupportedServerVersion { required: u32, actual: u32 },
}

/// Result type for client operations.
pub type ClientResult<T> = Result<T, ClientError>;

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
    /// Connect to the server, optionally starting it if not running.
    pub fn connect(config: ClientConfig, start_server: bool) -> ClientResult<Self> {
        let (connection, tunnel) = match &config.target {
            ClientTarget::Local { socket_path } => (
                Self::connect_local_with_retry(socket_path, start_server)?,
                None,
            ),
            ClientTarget::RemoteSsh {
                destination,
                socket_path,
            } => {
                if start_server {
                    let bootstrap =
                        bootstrap_remote_server(destination, env!("CARGO_PKG_VERSION"))?;
                    if bootstrap.installed {
                        eprintln!(
                            "Installing clux-server v{} on {} ({})...",
                            env!("CARGO_PKG_VERSION"),
                            destination,
                            bootstrap.platform.target_triple
                        );
                    }
                    start_remote_server(destination, socket_path, &bootstrap.binary_path)?;
                    wait_for_remote_socket(destination, socket_path)?;
                }
                let mut endpoint = start_ssh_tunnel(destination, socket_path)?;
                let connection = Self::connect_remote_with_retry(
                    destination,
                    socket_path,
                    &endpoint.connect_socket_path,
                    &mut endpoint.tunnel,
                    start_server,
                )?;
                (connection, Some(endpoint.tunnel))
            }
        };

        let mut client = Self {
            config,
            connection,
            tunnel,
            server_version: PROTOCOL_VERSION,
            session_id: None,
            session_name: None,
        };

        if let Err(err) = client.handshake() {
            let remote_no_autostart =
                matches!(client.config.target, ClientTarget::RemoteSsh { .. }) && !start_server;
            if remote_no_autostart && is_connection_failure(&err) {
                return Err(ClientError::ConnectionFailed(1));
            }
            return Err(err);
        }

        Ok(client)
    }

    /// Connect to a local server with retry logic.
    fn connect_local_with_retry(
        socket_path: &Path,
        start_server: bool,
    ) -> ClientResult<ServerConnection> {
        const MAX_RETRIES: u32 = 10;
        const RETRY_DELAY: Duration = Duration::from_millis(100);

        for attempt in 0..MAX_RETRIES {
            match ServerConnection::connect(socket_path) {
                Ok(conn) => return Ok(conn),
                Err(e) => {
                    if attempt == 0 && start_server {
                        log::info!("Server not running, attempting to start...");
                        if let Err(err) = Self::start_local_server(socket_path) {
                            log::warn!("Failed to start server: {}", err);
                        }
                    }

                    if attempt < MAX_RETRIES - 1 {
                        log::debug!("Connection attempt {} failed: {}", attempt + 1, e);
                        std::thread::sleep(RETRY_DELAY);
                    }
                }
            }
        }

        Err(ClientError::ConnectionFailed(MAX_RETRIES))
    }

    /// Connect to a remote server through an existing SSH tunnel.
    fn connect_remote_with_retry(
        destination: &str,
        remote_socket_path: &Path,
        local_forward_socket_path: &Path,
        tunnel: &mut SshTunnel,
        start_server: bool,
    ) -> ClientResult<ServerConnection> {
        const MAX_RETRIES: u32 = 10;
        const RETRY_DELAY: Duration = Duration::from_millis(100);

        for attempt in 0..MAX_RETRIES {
            tunnel.ensure_running()?;

            match ServerConnection::connect(local_forward_socket_path) {
                Ok(conn) => return Ok(conn),
                Err(e) => {
                    if attempt < MAX_RETRIES - 1 {
                        log::debug!("Remote connection attempt {} failed: {}", attempt + 1, e);
                        std::thread::sleep(RETRY_DELAY);
                    }
                }
            }
        }

        if start_server {
            Err(ClientError::RemoteStartupFailed(format!(
                "failed to connect to remote server {}:{} after {} attempts",
                destination,
                remote_socket_path.display(),
                MAX_RETRIES
            )))
        } else {
            Err(ClientError::ConnectionFailed(MAX_RETRIES))
        }
    }

    /// Start the local server process in the background.
    fn start_local_server(socket_path: &Path) -> io::Result<()> {
        let server_path = std::env::current_exe()?
            .parent()
            .map(|p| p.join("clux-server"))
            .unwrap_or_else(|| PathBuf::from("clux-server"));

        let socket_arg = socket_path.to_string_lossy().to_string();

        Command::new(&server_path)
            .arg("--socket")
            .arg(&socket_arg)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()?;

        std::thread::sleep(Duration::from_millis(200));

        Ok(())
    }

    /// Perform the initial handshake with the server.
    fn handshake(&mut self) -> ClientResult<()> {
        let hello = ClientMessage::Hello {
            version: PROTOCOL_VERSION,
            term_cols: self.config.term_cols,
            term_rows: self.config.term_rows,
            term_type: self.config.term_type.clone(),
            capabilities: Some(ClientCapabilities {
                supports_pane_updates: true,
            }),
        };
        self.connection.send(&hello)?;

        let response = self.connection.recv()?;
        match response {
            ServerMessage::HelloAck {
                version,
                server_pid,
            } => {
                self.server_version = version;
                log::info!(
                    "Connected to server (pid={}, version={})",
                    server_pid,
                    version
                );

                if version != PROTOCOL_VERSION {
                    log::warn!(
                        "Protocol version mismatch: client={}, server={}",
                        PROTOCOL_VERSION,
                        version
                    );
                }
                Ok(())
            }
            ServerMessage::Error { message } => Err(ClientError::HandshakeFailed(message)),
            other => Err(ClientError::UnexpectedResponse(other)),
        }
    }

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
                needs_full_redraw: _,
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
                ServerMessage::FullScreen { .. }
                | ServerMessage::Update { .. }
                | ServerMessage::MouseMode { .. }
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

    /// Get the target server socket path.
    pub fn socket_path(&self) -> &Path {
        self.config.target.socket_path()
    }

    /// Get the target remote destination, if any.
    pub fn remote_destination(&self) -> Option<&str> {
        self.config.target.remote_destination()
    }

    /// Get the raw file descriptor for polling.
    pub fn as_raw_fd(&self) -> std::os::unix::io::RawFd {
        self.connection.as_raw_fd()
    }
}

fn is_connection_failure(err: &ClientError) -> bool {
    match err {
        ClientError::Protocol(crate::protocol::ProtocolError::ConnectionClosed) => true,
        ClientError::Protocol(crate::protocol::ProtocolError::Io(io_err)) => matches!(
            io_err.kind(),
            io::ErrorKind::BrokenPipe
                | io::ErrorKind::ConnectionRefused
                | io::ErrorKind::ConnectionReset
                | io::ErrorKind::NotFound
        ),
        _ => false,
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
