//! Remote bootstrap tests.

use super::bootstrap::*;
use super::install::*;
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
