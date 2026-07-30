//! Running commands over ssh, and the local-socket tunnel plumbing.

use std::io::{self, Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Output, Stdio};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use super::{ClientError, ClientResult};

pub(super) fn run_remote_shell(
    destination: &str,
    script: &str,
    args: &[String],
) -> ClientResult<Output> {
    let mut cmd = Command::new("ssh");
    let mut ssh_args = vec![
        destination.to_string(),
        "sh".to_string(),
        "-s".to_string(),
        "--".to_string(),
    ];
    ssh_args.extend(args.iter().cloned());

    cmd.args(ssh_args)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());

    let mut child = spawn_ssh(cmd)?;
    if let Some(mut stdin) = child.stdin.take() {
        stdin.write_all(script.as_bytes())?;
    }
    child.wait_with_output().map_err(ClientError::Io)
}
pub(super) fn spawn_ssh(mut cmd: Command) -> ClientResult<Child> {
    cmd.spawn().map_err(|e| {
        if e.kind() == io::ErrorKind::NotFound {
            ClientError::SshUnavailable
        } else {
            ClientError::Io(e)
        }
    })
}
pub(super) fn wait_for_local_socket(
    child: &mut Child,
    socket_path: &Path,
    timeout: Duration,
) -> ClientResult<()> {
    let start = std::time::Instant::now();
    while start.elapsed() < timeout {
        if socket_path.exists() {
            return Ok(());
        }

        if let Some(status) = child.try_wait()? {
            let stderr = read_child_stderr(child);
            let details = if stderr.is_empty() {
                format!("ssh exited with status {}", status)
            } else {
                format!("ssh exited with status {}: {}", status, stderr.trim())
            };
            return Err(ClientError::RemoteTunnelFailed(details));
        }

        std::thread::sleep(Duration::from_millis(25));
    }

    Err(ClientError::RemoteTunnelFailed(format!(
        "timed out waiting for forwarded socket {:?}",
        socket_path
    )))
}
pub(super) fn read_child_stderr(child: &mut Child) -> String {
    let mut stderr = String::new();
    if let Some(mut pipe) = child.stderr.take() {
        let _ = pipe.read_to_string(&mut stderr);
    }
    stderr
}
pub(super) fn temp_forward_socket_path() -> PathBuf {
    let pid = std::process::id();
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos();
    std::env::temp_dir().join(format!("clux-ssh-{}-{}.sock", pid, nanos))
}
pub(super) fn tunnel_ssh_args(
    destination: &str,
    local_socket: &Path,
    remote_socket: &Path,
) -> Vec<String> {
    vec![
        "-o".to_string(),
        "ExitOnForwardFailure=yes".to_string(),
        "-o".to_string(),
        "StreamLocalBindUnlink=yes".to_string(),
        destination.to_string(),
        "-N".to_string(),
        "-L".to_string(),
        format!("{}:{}", local_socket.display(), remote_socket.display()),
    ]
}
