//! The remote install script and release-URL resolution.

use std::path::PathBuf;

use super::{ClientError, ClientResult, REMOTE_INSTALL_ROOT, REMOTE_TMP_ROOT};

pub(super) fn remote_bootstrap_script() -> &'static str {
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
/// Resolve the GitHub release URL for a versioned remote artifact.
pub fn resolve_release_url(version: &str, target: &str) -> ClientResult<String> {
    resolve_release_url_with_repo(env!("CARGO_PKG_REPOSITORY"), version, target)
}
pub(super) fn resolve_release_url_with_repo(
    repo: &str,
    version: &str,
    target: &str,
) -> ClientResult<String> {
    let (owner, name) = parse_github_repository(repo)?;
    Ok(format!(
        "https://github.com/{owner}/{name}/releases/download/v{version}/clux-server-v{version}-{target}.tar.gz"
    ))
}
pub(super) fn parse_github_repository(repo: &str) -> ClientResult<(String, String)> {
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
/// Managed installation paths used for remote bootstrapping.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RemoteInstallPaths {
    pub install_root: PathBuf,
    pub version_dir: PathBuf,
    pub binary_path: PathBuf,
    pub temp_root: PathBuf,
}
