//! Starting the remote server and connecting to it.

use std::path::Path;
use std::process::{Command, Stdio};

use super::bootstrap::bootstrap_remote_server;
use super::ssh::{run_remote_shell, spawn_ssh};
use super::{ClientError, ClientResult, REMOTE_SOCKET_WAIT_TIMEOUT};
use crate::client::ServerConnection;

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
