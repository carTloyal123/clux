//! Remote SSH transport helpers for the client.

use std::io::{self, Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Output, Stdio};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use super::{connection::ServerConnection, ClientError, ClientResult};

const TUNNEL_START_TIMEOUT: Duration = Duration::from_secs(5);
const REMOTE_SOCKET_WAIT_TIMEOUT: Duration = Duration::from_secs(5);
const REMOTE_INSTALL_ROOT: &str = "~/.local/share/clux/server";
const REMOTE_TMP_ROOT: &str = "~/.local/share/clux/server/.tmp";
const REMOTE_STDIO_BRIDGE_NAME: &str = "clux-stdio-bridge";
const DOWNLOAD_TOOL_MISSING_EXIT: i32 = 42;
const BOOTSTRAP_FAILED_EXIT: i32 = 43;
const ARTIFACT_UNAVAILABLE_EXIT: i32 = 44;

/// A normalized remote platform that can be matched to a release artifact.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RemotePlatform {
    pub os: String,
    pub arch: String,
    pub target_triple: String,
}

/// Managed installation paths used for remote bootstrapping.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RemoteInstallPaths {
    pub install_root: PathBuf,
    pub version_dir: PathBuf,
    pub binary_path: PathBuf,
    pub temp_root: PathBuf,
}

/// Result of resolving or installing a remote `clux-server`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BootstrapResult {
    pub platform: RemotePlatform,
    pub binary_path: PathBuf,
    pub installed: bool,
}

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

/// Probe the remote OS and architecture.
pub fn probe_remote_platform(destination: &str) -> ClientResult<RemotePlatform> {
    log::info!("Probing remote platform for {}", destination);
    let output = run_remote_shell_capture(
        destination,
        concat!(
            "printf 'CLUX_PROBE_OS=%s\\n' \"$(uname -s)\"\n",
            "printf 'CLUX_PROBE_ARCH=%s\\n' \"$(uname -m)\"\n"
        ),
        &[],
    )?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
        let stdout = String::from_utf8_lossy(&output.stdout).trim().to_string();
        let details = if !stderr.is_empty() {
            stderr
        } else if !stdout.is_empty() {
            stdout
        } else {
            format!("ssh exited with status {}", output.status)
        };
        return Err(ClientError::RemoteBootstrapFailed(format!(
            "remote platform probe failed: {}",
            details
        )));
    }

    let stdout = String::from_utf8_lossy(&output.stdout);
    let platform = parse_remote_platform_probe(&stdout)?;
    log::info!(
        "Remote platform detected for {}: {}/{} -> {}",
        destination,
        platform.os,
        platform.arch,
        platform.target_triple
    );
    Ok(platform)
}

/// Compute the logical managed install paths for a given version.
pub fn compute_remote_install_paths(version: &str) -> RemoteInstallPaths {
    let install_root = PathBuf::from(REMOTE_INSTALL_ROOT);
    let version_dir = install_root.join(version);
    let binary_path = version_dir.join("clux-server");
    let temp_root = PathBuf::from(REMOTE_TMP_ROOT);

    RemoteInstallPaths {
        install_root,
        version_dir,
        binary_path,
        temp_root,
    }
}

/// Resolve the GitHub release URL for a versioned remote artifact.
pub fn resolve_release_url(version: &str, target: &str) -> ClientResult<String> {
    resolve_release_url_with_repo(env!("CARGO_PKG_REPOSITORY"), version, target)
}

/// Ensure a managed remote `clux-server` is available, downloading it if needed.
pub fn bootstrap_remote_server(destination: &str, version: &str) -> ClientResult<BootstrapResult> {
    let platform = probe_remote_platform(destination)?;
    let paths = compute_remote_install_paths(version);
    let release_url = resolve_release_url(version, &platform.target_triple)?;

    log::info!(
        "Resolved remote bootstrap artifact for {}: {}",
        platform.target_triple,
        release_url
    );

    let script = remote_bootstrap_script();
    let args = vec![
        release_url.clone(),
        version.to_string(),
        platform.target_triple.clone(),
    ];
    let output = run_remote_shell(destination, &script, &args)?;

    match output.status.code() {
        Some(0) => {
            let stdout = String::from_utf8_lossy(&output.stdout);
            let line = stdout
                .lines()
                .find(|line| !line.trim().is_empty())
                .ok_or_else(|| {
                    ClientError::RemoteBootstrapFailed(
                        "bootstrap succeeded but returned no managed binary path".to_string(),
                    )
                })?;
            let (state, path) = line.split_once('\t').ok_or_else(|| {
                ClientError::RemoteBootstrapFailed(format!(
                    "unexpected bootstrap output: {}",
                    line.trim()
                ))
            })?;
            let installed = match state {
                "INSTALLED" => true,
                "REUSED" => false,
                other => {
                    return Err(ClientError::RemoteBootstrapFailed(format!(
                        "unexpected bootstrap state: {}",
                        other
                    )))
                }
            };
            let binary_path = PathBuf::from(path.trim());
            log::info!(
                "Remote bootstrap {} for {} using {}",
                if installed { "installed" } else { "reused" },
                destination,
                binary_path.display()
            );
            log::debug!(
                "Managed remote install root={}, version_dir={}, temp_root={}",
                paths.install_root.display(),
                paths.version_dir.display(),
                paths.temp_root.display()
            );
            Ok(BootstrapResult {
                platform,
                binary_path,
                installed,
            })
        }
        Some(DOWNLOAD_TOOL_MISSING_EXIT) => Err(ClientError::RemoteMissingDownloadTool),
        Some(ARTIFACT_UNAVAILABLE_EXIT) => Err(ClientError::RemoteArtifactUnavailable {
            version: version.to_string(),
            target: platform.target_triple,
            url: release_url,
        }),
        Some(BOOTSTRAP_FAILED_EXIT) | Some(_) | None => {
            let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
            let details = if stderr.is_empty() {
                "remote bootstrap failed".to_string()
            } else {
                stderr
            };
            Err(ClientError::RemoteBootstrapFailed(details))
        }
    }
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

/// Connect to the remote server over SSH stdio via a small remote bridge helper.
pub fn connect_remote_stdio_bridge(
    destination: &str,
    version: &str,
    remote_socket_path: &Path,
) -> ClientResult<ServerConnection> {
    let bridge_path = remote_stdio_bridge_shell_path(version);
    ensure_remote_stdio_bridge(destination, version)?;

    log::info!(
        "Starting SSH stdio bridge to {} for remote socket {} using {}",
        destination,
        remote_socket_path.display(),
        bridge_path
    );

    let mut cmd = Command::new("ssh");
    cmd.arg("-T")
        .arg(destination)
        .arg(format!(
            "exec \"{}\" \"{}\"",
            bridge_path,
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

fn run_remote_shell(destination: &str, script: &str, args: &[String]) -> ClientResult<Output> {
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

fn ensure_remote_stdio_bridge(destination: &str, version: &str) -> ClientResult<()> {
    let script = format!(
        concat!(
            "version=\"$1\"\n",
            "bridge_path=\"$HOME/.local/share/clux/server/$version/{bridge_name}\"\n",
            "mkdir -p \"$(dirname \"$bridge_path\")\" || exit 43\n",
            "cat > \"$bridge_path\" <<'PY'\n",
            "#!/usr/bin/env python3\n",
            "import os\n",
            "import selectors\n",
            "import socket\n",
            "import sys\n",
            "\n",
            "sock = socket.socket(socket.AF_UNIX, socket.SOCK_STREAM)\n",
            "sock.connect(sys.argv[1])\n",
            "selector = selectors.DefaultSelector()\n",
            "selector.register(0, selectors.EVENT_READ)\n",
            "selector.register(sock, selectors.EVENT_READ)\n",
            "stdin_open = True\n",
            "\n",
            "while True:\n",
            "    for key, _ in selector.select():\n",
            "        if key.fileobj == 0:\n",
            "            data = os.read(0, 65536)\n",
            "            if not data:\n",
            "                if stdin_open:\n",
            "                    stdin_open = False\n",
            "                    selector.unregister(0)\n",
            "                    try:\n",
            "                        sock.shutdown(socket.SHUT_WR)\n",
            "                    except OSError:\n",
            "                        pass\n",
            "            else:\n",
            "                sock.sendall(data)\n",
            "        else:\n",
            "            data = sock.recv(65536)\n",
            "            if not data:\n",
            "                raise SystemExit(0)\n",
            "            os.write(1, data)\n",
            "PY\n",
            "chmod +x \"$bridge_path\" || exit 43\n"
        ),
        bridge_name = REMOTE_STDIO_BRIDGE_NAME
    );
    let args = vec![version.to_string()];
    let output = run_remote_shell(destination, &script, &args)?;
    if output.status.success() {
        Ok(())
    } else {
        let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
        let details = if stderr.is_empty() {
            "failed to install remote stdio bridge".to_string()
        } else {
            stderr
        };
        Err(ClientError::RemoteBootstrapFailed(details))
    }
}

fn remote_stdio_bridge_shell_path(version: &str) -> String {
    format!(
        "$HOME/.local/share/clux/server/{}/{}",
        version, REMOTE_STDIO_BRIDGE_NAME
    )
}

fn spawn_ssh(mut cmd: Command) -> ClientResult<Child> {
    cmd.spawn().map_err(|e| {
        if e.kind() == io::ErrorKind::NotFound {
            ClientError::SshUnavailable
        } else {
            ClientError::Io(e)
        }
    })
}

fn wait_for_local_socket(
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

fn read_child_stderr(child: &mut Child) -> String {
    let mut stderr = String::new();
    if let Some(mut pipe) = child.stderr.take() {
        let _ = pipe.read_to_string(&mut stderr);
    }
    stderr
}

fn temp_forward_socket_path() -> PathBuf {
    let pid = std::process::id();
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos();
    std::env::temp_dir().join(format!("clux-ssh-{}-{}.sock", pid, nanos))
}

fn normalize_remote_platform(os: &str, arch: &str) -> ClientResult<RemotePlatform> {
    let target_triple = match (os, arch) {
        ("Linux", "x86_64") | ("Linux", "amd64") => "x86_64-unknown-linux-gnu",
        ("Linux", "aarch64") | ("Linux", "arm64") => "aarch64-unknown-linux-gnu",
        _ => {
            return Err(ClientError::RemotePlatformUnsupported {
                os: os.to_string(),
                arch: arch.to_string(),
            })
        }
    };

    Ok(RemotePlatform {
        os: os.to_string(),
        arch: arch.to_string(),
        target_triple: target_triple.to_string(),
    })
}

fn parse_remote_platform_probe(stdout: &str) -> ClientResult<RemotePlatform> {
    let os = stdout
        .lines()
        .find_map(|line| line.trim().strip_prefix("CLUX_PROBE_OS="))
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .ok_or_else(|| {
            ClientError::RemoteBootstrapFailed("remote platform probe returned no OS".to_string())
        })?;
    let arch = stdout
        .lines()
        .find_map(|line| line.trim().strip_prefix("CLUX_PROBE_ARCH="))
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .ok_or_else(|| {
            ClientError::RemoteBootstrapFailed("remote platform probe returned no arch".to_string())
        })?;

    normalize_remote_platform(os, arch)
}

fn resolve_release_url_with_repo(repo: &str, version: &str, target: &str) -> ClientResult<String> {
    let (owner, name) = parse_github_repository(repo)?;
    Ok(format!(
        "https://github.com/{owner}/{name}/releases/download/v{version}/clux-server-v{version}-{target}.tar.gz"
    ))
}

fn parse_github_repository(repo: &str) -> ClientResult<(String, String)> {
    let trimmed = repo.trim().trim_end_matches('/').trim_end_matches(".git");
    let prefix = "https://github.com/";
    if !trimmed.starts_with(prefix) {
        return Err(ClientError::InvalidRepositoryMetadata(repo.to_string()));
    }

    let path = &trimmed[prefix.len()..];
    let mut parts = path.split('/');
    let owner = parts.next().unwrap_or_default();
    let name = parts.next().unwrap_or_default();

    if owner.is_empty() || name.is_empty() || parts.next().is_some() || owner == "yourusername" {
        return Err(ClientError::InvalidRepositoryMetadata(repo.to_string()));
    }

    Ok((owner.to_string(), name.to_string()))
}

fn remote_bootstrap_script() -> &'static str {
    concat!(
        "url=\"$1\"\n",
        "version=\"$2\"\n",
        "target=\"$3\"\n",
        "install_root=\"$HOME/.local/share/clux/server\"\n",
        "version_dir=\"$install_root/$version\"\n",
        "binary_path=\"$version_dir/clux-server\"\n",
        "temp_root=\"$install_root/.tmp\"\n",
        "if [ -x \"$binary_path\" ]; then\n",
        "  printf 'REUSED\\t%s\\n' \"$binary_path\"\n",
        "  exit 0\n",
        "fi\n",
        "for tool in tar chmod mkdir mv rm dirname; do\n",
        "  command -v \"$tool\" >/dev/null 2>&1 || { echo \"missing required remote tool: $tool\" >&2; exit 43; }\n",
        "done\n",
        "downloader=\"\"\n",
        "if command -v curl >/dev/null 2>&1; then\n",
        "  downloader=\"curl\"\n",
        "elif command -v wget >/dev/null 2>&1; then\n",
        "  downloader=\"wget\"\n",
        "else\n",
        "  echo \"neither curl nor wget is available on the remote host\" >&2\n",
        "  exit 42\n",
        "fi\n",
        "mkdir -p \"$temp_root\"\n",
        "tmp_dir=\"$temp_root/install-$version-$target-$$\"\n",
        "archive=\"$tmp_dir/archive.tar.gz\"\n",
        "extract_dir=\"$tmp_dir/extract\"\n",
        "staging_dir=\"$tmp_dir/version\"\n",
        "rm -rf \"$tmp_dir\"\n",
        "mkdir -p \"$extract_dir\"\n",
        "if [ \"$downloader\" = \"curl\" ]; then\n",
        "  curl -fsSL -o \"$archive\" \"$url\" || { echo \"failed to download artifact: $url\" >&2; exit 44; }\n",
        "else\n",
        "  wget -q -O \"$archive\" \"$url\" || { echo \"failed to download artifact: $url\" >&2; exit 44; }\n",
        "fi\n",
        "tar -xzf \"$archive\" -C \"$extract_dir\" || { echo \"failed to extract artifact\" >&2; exit 43; }\n",
        "test -f \"$extract_dir/clux-server\" || { echo \"artifact missing clux-server binary\" >&2; exit 43; }\n",
        "mkdir -p \"$staging_dir\"\n",
        "mv \"$extract_dir/clux-server\" \"$staging_dir/clux-server\" || { echo \"failed to stage clux-server\" >&2; exit 43; }\n",
        "chmod +x \"$staging_dir/clux-server\" || { echo \"failed to chmod clux-server\" >&2; exit 43; }\n",
        "printf 'version=%s\\ntarget=%s\\nurl=%s\\n' \"$version\" \"$target\" \"$url\" > \"$staging_dir/INSTALL_META\"\n",
        "if [ -e \"$version_dir\" ] && [ ! -x \"$binary_path\" ]; then\n",
        "  echo \"existing remote install is incomplete: $version_dir\" >&2\n",
        "  exit 43\n",
        "fi\n",
        "if [ ! -e \"$version_dir\" ]; then\n",
        "  mv \"$staging_dir\" \"$version_dir\" 2>/dev/null || true\n",
        "fi\n",
        "rm -rf \"$tmp_dir\"\n",
        "test -x \"$binary_path\" || { echo \"managed remote binary missing after install\" >&2; exit 43; }\n",
        "printf 'INSTALLED\\t%s\\n' \"$binary_path\"\n"
    )
}

fn tunnel_ssh_args(destination: &str, local_socket: &Path, remote_socket: &Path) -> Vec<String> {
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_temp_forward_socket_path() {
        let path = temp_forward_socket_path();
        assert!(path.to_string_lossy().contains("clux-ssh"));
        assert_eq!(path.extension().and_then(|ext| ext.to_str()), Some("sock"));
    }

    #[test]
    fn test_tunnel_ssh_args_include_required_options() {
        let args = tunnel_ssh_args(
            "devbox",
            Path::new("/tmp/local.sock"),
            Path::new("/tmp/remote.sock"),
        );

        assert!(args.contains(&"ExitOnForwardFailure=yes".to_string()));
        assert!(args.contains(&"StreamLocalBindUnlink=yes".to_string()));
        assert!(args.contains(&"devbox".to_string()));
        assert!(args.contains(&"-N".to_string()));
        assert!(args.contains(&"-L".to_string()));
        assert!(args.contains(&"/tmp/local.sock:/tmp/remote.sock".to_string()));
    }

    #[test]
    fn test_normalize_remote_platform_linux_x86_64() {
        let platform = normalize_remote_platform("Linux", "x86_64").unwrap();
        assert_eq!(platform.target_triple, "x86_64-unknown-linux-gnu");
    }

    #[test]
    fn test_normalize_remote_platform_linux_arm64() {
        let platform = normalize_remote_platform("Linux", "arm64").unwrap();
        assert_eq!(platform.target_triple, "aarch64-unknown-linux-gnu");
    }

    #[test]
    fn test_parse_remote_platform_probe_with_noise() {
        let stdout = "warning from profile\nCLUX_PROBE_OS=Linux\nCLUX_PROBE_ARCH=x86_64\n";
        let platform = parse_remote_platform_probe(stdout).unwrap();
        assert_eq!(platform.os, "Linux");
        assert_eq!(platform.arch, "x86_64");
        assert_eq!(platform.target_triple, "x86_64-unknown-linux-gnu");
    }

    #[test]
    fn test_parse_remote_platform_probe_missing_arch() {
        let err = parse_remote_platform_probe("CLUX_PROBE_OS=Linux\n").unwrap_err();
        assert!(matches!(err, ClientError::RemoteBootstrapFailed(_)));
        assert_eq!(
            err.to_string(),
            "Remote bootstrap failed: remote platform probe returned no arch"
        );
    }

    #[test]
    fn test_normalize_remote_platform_unsupported() {
        let err = normalize_remote_platform("FreeBSD", "x86_64").unwrap_err();
        assert!(matches!(err, ClientError::RemotePlatformUnsupported { .. }));
    }

    #[test]
    fn test_compute_remote_install_paths() {
        let paths = compute_remote_install_paths("0.1.0");
        assert_eq!(
            paths.install_root,
            PathBuf::from("~/.local/share/clux/server")
        );
        assert_eq!(
            paths.binary_path,
            PathBuf::from("~/.local/share/clux/server/0.1.0/clux-server")
        );
        assert_eq!(
            paths.temp_root,
            PathBuf::from("~/.local/share/clux/server/.tmp")
        );
    }

    #[test]
    fn test_resolve_release_url_with_repo() {
        let url = resolve_release_url_with_repo(
            "https://github.com/carTloyal123/clux",
            "0.1.0",
            "x86_64-unknown-linux-gnu",
        )
        .unwrap();
        assert_eq!(
            url,
            "https://github.com/carTloyal123/clux/releases/download/v0.1.0/clux-server-v0.1.0-x86_64-unknown-linux-gnu.tar.gz"
        );
    }

    #[test]
    fn test_invalid_repository_metadata_rejected() {
        let err = parse_github_repository("https://github.com/yourusername/clux").unwrap_err();
        assert!(matches!(err, ClientError::InvalidRepositoryMetadata(_)));
    }

    #[test]
    fn test_remote_bootstrap_script_mentions_downloaders() {
        let script = remote_bootstrap_script();
        assert!(script.contains("curl"));
        assert!(script.contains("wget"));
        assert!(script.contains("INSTALL_META"));
    }
}
