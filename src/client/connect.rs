//! Connecting to the server: local spawn and remote tunnel.

use std::io;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::Duration;

use super::remote::{
    bootstrap_remote_server, start_remote_server, start_ssh_tunnel, wait_for_remote_socket,
};
use super::{
    ClientConfig, ClientError, ClientResult, ClientTarget, ServerConnection, SERVER_BINARY,
};
use crate::protocol::{ClientMessage, ServerMessage, PROTOCOL_VERSION};
impl super::Client {
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
            if start_server && client.try_remote_stdio_fallback(&err)? {
                client.handshake()?;
                return Ok(client);
            }
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
    pub(super) fn connect_local_with_retry(
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
    /// Start the local server process in the background.
    ///
    /// The server is a sibling of the client binary in a normal install, but not
    /// when the client runs from somewhere else (a test harness, a symlink into a
    /// bin directory), so fall back to `$PATH` rather than failing outright.
    pub(super) fn start_local_server(socket_path: &Path) -> io::Result<()> {
        let server_path = Self::server_binary_path();
        let socket_arg = socket_path.to_string_lossy().to_string();

        log::info!("Starting local server: {:?}", server_path);

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
    /// Locate the `clux-server` binary: next to this executable if it is there,
    /// otherwise by name so `$PATH` resolves it.
    pub(super) fn server_binary_path() -> PathBuf {
        let sibling = std::env::current_exe()
            .ok()
            .and_then(|exe| exe.parent().map(|dir| dir.join(SERVER_BINARY)));

        match sibling {
            Some(path) if path.is_file() => path,
            _ => PathBuf::from(SERVER_BINARY),
        }
    }
    /// Perform the initial handshake with the server.
    pub(super) fn handshake(&mut self) -> ClientResult<()> {
        let hello = ClientMessage::Hello {
            version: PROTOCOL_VERSION,
            term_cols: self.config.term_cols,
            term_rows: self.config.term_rows,
            term_type: self.config.term_type.clone(),
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
}

pub(super) fn is_connection_failure(err: &ClientError) -> bool {
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

pub(super) fn should_retry_remote_over_stdio(err: &ClientError) -> bool {
    matches!(
        err,
        ClientError::Protocol(crate::protocol::ProtocolError::ConnectionClosed)
            | ClientError::RemoteTunnelFailed(_)
            | ClientError::RemoteStartupFailed(_)
    )
}
