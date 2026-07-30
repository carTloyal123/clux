//! Remote SSH transport helpers for the client.

use std::path::{Path, PathBuf};
use std::process::{Child, Command, Output, Stdio};
use std::time::Duration;

use super::{connection::ServerConnection, ClientError, ClientResult};

mod install;
mod ssh;
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

/// Start the remote server over SSH using a managed binary path.
pub fn start_remote_server(
    destination: &str,
    remote_socket_path: &Path,
    server_bin_path: &Path,
) -> ClientResult<()> {
    log::info!(
        "Starting managed remote clux-server on {} using {}",
        destination,
        server_bin_path.display()
    );
    let script = concat!(
        "socket=\"$1\"\n",
        "server_bin=\"$2\"\n",
        "mkdir -p \"$(dirname \"$socket\")\" &&\n",
        "(nohup \"$server_bin\" --socket \"$socket\" </dev/null >/dev/null 2>&1 &)\n"
    );
    let args = vec![
        remote_socket_path.display().to_string(),
        server_bin_path.display().to_string(),
    ];
    let output = run_remote_shell(destination, &script, &args)?;

    if output.status.success() {
        Ok(())
    } else {
        let stderr = String::from_utf8_lossy(&output.stderr);
        let details = if stderr.trim().is_empty() {
            format!("ssh exited with status {}", output.status)
        } else {
            format!(
                "ssh exited with status {}: {}",
                output.status,
                stderr.trim()
            )
        };
        Err(ClientError::RemoteStartupFailed(details))
    }
}

/// Wait until the remote Unix socket exists.
pub fn wait_for_remote_socket(destination: &str, remote_socket_path: &Path) -> ClientResult<()> {
    log::info!(
        "Waiting for remote socket {} on {}",
        remote_socket_path.display(),
        destination
    );
    let script = concat!(
        "socket=\"$1\"\n",
        "i=0\n",
        "while [ \"$i\" -lt 50 ]; do\n",
        "  if [ -S \"$socket\" ]; then\n",
        "    exit 0\n",
        "  fi\n",
        "  /bin/sleep 0.1\n",
        "  i=$((i + 1))\n",
        "done\n",
        "echo \"remote socket did not appear: $socket\" >&2\n",
        "exit 1\n"
    );
    let args = vec![remote_socket_path.display().to_string()];
    let output = run_remote_shell(destination, script, &args)?;
    if output.status.success() {
        Ok(())
    } else {
        let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
        let details = if stderr.is_empty() {
            format!(
                "remote socket {} did not become ready within {:?}",
                remote_socket_path.display(),
                REMOTE_SOCKET_WAIT_TIMEOUT
            )
        } else {
            stderr
        };
        Err(ClientError::RemoteStartupFailed(details))
    }
}

/// Connect to the remote server over SSH stdio, used when ssh cannot forward a
/// Unix socket.
///
/// The remote end is `clux-server --stdio-bridge`, which the bootstrap has already
/// installed - there is no separate helper to install.
pub fn connect_remote_stdio_bridge(
    destination: &str,
    version: &str,
    remote_socket_path: &Path,
) -> ClientResult<ServerConnection> {
    // Idempotent: reuses the existing install and just reports its path.
    let bootstrap = bootstrap_remote_server(destination, version)?;
    let server_path = bootstrap.binary_path;

    log::info!(
        "Starting SSH stdio bridge to {} for remote socket {} using {}",
        destination,
        remote_socket_path.display(),
        server_path.display()
    );

    let mut cmd = Command::new("ssh");
    cmd.arg("-T")
        .arg(destination)
        .arg(format!(
            "exec \"{}\" --stdio-bridge \"{}\"",
            server_path.display(),
            remote_socket_path.display()
        ))
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());

    let child = spawn_ssh(cmd)?;
    ServerConnection::from_ssh_stdio_child(child).map_err(|err| match err {
        crate::protocol::ProtocolError::Io(io_err) => ClientError::Io(io_err),
        other => ClientError::RemoteTunnelFailed(other.to_string()),
    })
}

fn run_remote_shell_capture(
    destination: &str,
    script: &str,
    args: &[String],
) -> ClientResult<Output> {
    run_remote_shell(destination, script, args)
}

#[cfg(test)]
mod tests;
