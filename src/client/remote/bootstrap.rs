//! Detecting the remote platform and installing clux-server there.

use std::path::PathBuf;

use super::install::{compute_remote_install_paths, remote_bootstrap_script, resolve_release_url};
use super::{
    run_remote_shell, ClientError, ClientResult, ARTIFACT_UNAVAILABLE_EXIT, BOOTSTRAP_FAILED_EXIT,
    DOWNLOAD_TOOL_MISSING_EXIT,
};

/// A normalized remote platform that can be matched to a release artifact.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RemotePlatform {
    pub os: String,
    pub arch: String,
    pub target_triple: String,
}
/// Result of resolving or installing a remote `clux-server`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BootstrapResult {
    pub platform: RemotePlatform,
    pub binary_path: PathBuf,
    pub installed: bool,
}
/// Probe the remote OS and architecture.
pub fn probe_remote_platform(destination: &str) -> ClientResult<RemotePlatform> {
    log::info!("Probing remote platform for {}", destination);
    let output = run_remote_shell(
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
pub(super) fn normalize_remote_platform(os: &str, arch: &str) -> ClientResult<RemotePlatform> {
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
pub(super) fn parse_remote_platform_probe(stdout: &str) -> ClientResult<RemotePlatform> {
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
