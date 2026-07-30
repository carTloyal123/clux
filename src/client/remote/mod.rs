//! Remote SSH transport helpers for the client.

use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::time::Duration;

use super::{ClientError, ClientResult};

mod install;
mod server;
mod ssh;
pub use server::{connect_remote_stdio_bridge, start_remote_server, wait_for_remote_socket};
use ssh::{
    read_child_stderr, run_remote_shell, spawn_ssh, temp_forward_socket_path, tunnel_ssh_args,
    wait_for_local_socket,
};

mod bootstrap;
pub use bootstrap::bootstrap_remote_server;

pub(super) const TUNNEL_START_TIMEOUT: Duration = Duration::from_secs(5);
pub(super) const REMOTE_SOCKET_WAIT_TIMEOUT: Duration = Duration::from_secs(5);
pub(super) const REMOTE_INSTALL_ROOT: &str = "~/.local/share/clux/server";
pub(super) const REMOTE_TMP_ROOT: &str = "~/.local/share/clux/server/.tmp";
pub(super) const DOWNLOAD_TOOL_MISSING_EXIT: i32 = 42;
pub(super) const BOOTSTRAP_FAILED_EXIT: i32 = 43;
pub(super) const ARTIFACT_UNAVAILABLE_EXIT: i32 = 44;

/// A persistent SSH tunnel process backing a remote client connection.
#[derive(Debug)]
pub struct SshTunnel {
    local_socket_path: PathBuf,
    child: Child,
}

impl SshTunnel {
    /// Ensure the tunnel is still running.
    pub fn ensure_running(&mut self) -> ClientResult<()> {
        if let Some(status) = self.child.try_wait()? {
            let stderr = read_child_stderr(&mut self.child);
            let details = if stderr.is_empty() {
                format!("ssh exited with status {}", status)
            } else {
                format!("ssh exited with status {}: {}", status, stderr.trim())
            };
            return Err(ClientError::RemoteTunnelFailed(details));
        }
        Ok(())
    }
}

impl Drop for SshTunnel {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
        let _ = std::fs::remove_file(&self.local_socket_path);
    }
}

/// The local endpoint a client should connect to.
#[derive(Debug)]
pub struct ResolvedClientEndpoint {
    pub connect_socket_path: PathBuf,
    pub tunnel: SshTunnel,
}

/// Start a persistent SSH tunnel forwarding a local Unix socket to a remote Unix socket.
pub fn start_ssh_tunnel(
    destination: &str,
    remote_socket_path: &Path,
) -> ClientResult<ResolvedClientEndpoint> {
    let local_socket_path = temp_forward_socket_path();
    if let Some(parent) = local_socket_path.parent() {
        std::fs::create_dir_all(parent)?;
    }

    log::info!(
        "Starting SSH tunnel to {} for remote socket {}",
        destination,
        remote_socket_path.display()
    );

    let mut cmd = Command::new("ssh");
    cmd.args(tunnel_ssh_args(
        destination,
        &local_socket_path,
        remote_socket_path,
    ))
    .stdin(Stdio::null())
    .stdout(Stdio::null())
    .stderr(Stdio::piped());

    let mut child = spawn_ssh(cmd)?;
    wait_for_local_socket(&mut child, &local_socket_path, TUNNEL_START_TIMEOUT)?;

    Ok(ResolvedClientEndpoint {
        connect_socket_path: local_socket_path.clone(),
        tunnel: SshTunnel {
            local_socket_path,
            child,
        },
    })
}

#[cfg(test)]
mod tests;
