//! Connecting to a remote server over ssh.

use std::path::Path;
use std::time::Duration;

use super::connect::should_retry_remote_over_stdio;
use super::remote::{connect_remote_stdio_bridge, SshTunnel};
use super::{ClientError, ClientResult, ClientTarget, ServerConnection};
impl super::Client {
    pub(super) fn try_remote_stdio_fallback(&mut self, err: &ClientError) -> ClientResult<bool> {
        let (destination, socket_path) = match (&self.config.target, self.tunnel.is_some()) {
            (
                ClientTarget::RemoteSsh {
                    destination,
                    socket_path,
                },
                true,
            ) if should_retry_remote_over_stdio(err) => (destination.clone(), socket_path.clone()),
            _ => return Ok(false),
        };

        log::warn!(
            "Remote handshake over SSH tunnel failed ({}), retrying with stdio bridge",
            err
        );
        self.tunnel = None;
        self.connection =
            connect_remote_stdio_bridge(&destination, env!("CARGO_PKG_VERSION"), &socket_path)?;
        Ok(true)
    }
    /// Connect to a remote server through an existing SSH tunnel.
    pub(super) fn connect_remote_with_retry(
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
}
