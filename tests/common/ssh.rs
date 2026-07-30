//! Fake ssh fixtures for the remote-mode tests.

#![allow(dead_code)]

use super::harness::{start_server, unique_socket_path, wait_for_socket, TestError, SSH_ENV_LOCK};

use std::io::{BufRead, BufReader};
use std::os::unix::fs::symlink;
use std::os::unix::fs::PermissionsExt;
use std::path::PathBuf;
use std::process::{Child, Command, Stdio};
use std::sync::{Mutex, MutexGuard};
use std::thread;
use std::time::{Duration, Instant};

use clux::client::{Client, ClientConfig, ClientTarget, ScreenBuffer};
use clux::protocol::{CommandAction, Direction, ServerMessage, WindowLayout};
use clux::selection::SelectionMode;

#[derive(Debug, Clone, Copy)]
pub enum FakeDownloader {
    Curl,
    Wget,
    None,
}
#[derive(Debug, Clone)]
pub struct FakeSshOptions {
    pub os: String,
    pub arch: String,
    pub downloader: FakeDownloader,
    pub artifact_present: bool,
}
pub struct FakeSshEnv {
    pub _guard: MutexGuard<'static, ()>,
    pub temp_dir: PathBuf,
    pub home_dir: PathBuf,
    pub remote_socket: PathBuf,
    pub download_count_path: PathBuf,
    pub previous_path: Option<std::ffi::OsString>,
}
pub fn system_command_path(name: &str) -> Result<PathBuf, TestError> {
    for base in ["/bin", "/usr/bin"] {
        let path = PathBuf::from(base).join(name);
        if path.exists() {
            return Ok(path);
        }
    }
    Err(TestError::Client(format!(
        "required system command not found: {}",
        name
    )))
}
pub fn populate_fake_bin_dir(
    bin_dir: &PathBuf,
    artifact_path: &PathBuf,
    download_count_path: &PathBuf,
    options: &FakeSshOptions,
) -> Result<(), TestError> {
    for cmd in [
        "mkdir", "mv", "chmod", "tar", "gzip", "rm", "dirname", "nohup", "cp", "cat",
    ] {
        let target = system_command_path(cmd)?;
        symlink(target, bin_dir.join(cmd))?;
    }

    let uname_path = bin_dir.join("uname");
    let uname_script = format!(
        "#!/bin/sh\n\
set -eu\n\
case \"${{1:-}}\" in\n\
  -s) printf '%s\\n' \"{}\" ;;\n\
  -m) printf '%s\\n' \"{}\" ;;\n\
  *) /usr/bin/uname \"$@\" ;;\n\
esac\n",
        options.os, options.arch
    );
    std::fs::write(&uname_path, uname_script)?;
    let mut perms = std::fs::metadata(&uname_path)?.permissions();
    perms.set_mode(0o755);
    std::fs::set_permissions(&uname_path, perms)?;

    match options.downloader {
        FakeDownloader::Curl => {
            create_fake_downloader(bin_dir, "curl", artifact_path, download_count_path, true)?;
        }
        FakeDownloader::Wget => {
            create_fake_downloader(bin_dir, "wget", artifact_path, download_count_path, false)?;
        }
        FakeDownloader::None => {}
    }

    Ok(())
}
pub fn create_fake_downloader(
    bin_dir: &PathBuf,
    name: &str,
    artifact_path: &PathBuf,
    download_count_path: &PathBuf,
    is_curl: bool,
) -> Result<(), TestError> {
    let path = bin_dir.join(name);
    let parser = if is_curl {
        "while [ \"$#\" -gt 0 ]; do\n\
  case \"$1\" in\n\
    -o)\n\
      out=\"$2\"\n\
      shift 2\n\
      ;;\n\
    -f|-s|-S|-L|-fsSL)\n\
      shift\n\
      ;;\n\
    *)\n\
      url=\"$1\"\n\
      shift\n\
      ;;\n\
  esac\n\
done\n"
    } else {
        "while [ \"$#\" -gt 0 ]; do\n\
  case \"$1\" in\n\
    -O)\n\
      out=\"$2\"\n\
      shift 2\n\
      ;;\n\
    -q)\n\
      shift\n\
      ;;\n\
    *)\n\
      url=\"$1\"\n\
      shift\n\
      ;;\n\
  esac\n\
done\n"
    };
    let script = format!(
        "#!/bin/sh\n\
set -eu\n\
out=\"\"\n\
url=\"\"\n\
{}\
count=0\n\
if [ -f \"{}\" ]; then\n\
  count=$(cat \"{}\")\n\
fi\n\
printf '%s\\n' \"$((count + 1))\" > \"{}\"\n\
if [ ! -f \"{}\" ]; then\n\
  echo \"missing artifact: $url\" >&2\n\
  exit 22\n\
fi\n\
cp \"{}\" \"$out\"\n",
        parser,
        download_count_path.display(),
        download_count_path.display(),
        download_count_path.display(),
        artifact_path.display(),
        artifact_path.display(),
    );
    std::fs::write(&path, script)?;
    let mut perms = std::fs::metadata(&path)?.permissions();
    perms.set_mode(0o755);
    std::fs::set_permissions(&path, perms)?;
    Ok(())
}
pub fn create_server_artifact(artifact_path: &PathBuf) -> Result<(), TestError> {
    let content_dir = artifact_path.parent().unwrap().join("artifact-content");
    std::fs::create_dir_all(&content_dir)?;
    let server_bin = env!("CARGO_BIN_EXE_clux-server");
    std::fs::copy(server_bin, content_dir.join("clux-server"))?;

    let tar = system_command_path("tar")?;
    let status = Command::new(tar)
        .arg("-czf")
        .arg(artifact_path)
        .arg("-C")
        .arg(&content_dir)
        .arg("clux-server")
        .status()?;
    if !status.success() {
        return Err(TestError::Client(format!(
            "failed to create fake artifact archive: {}",
            status
        )));
    }
    Ok(())
}
pub fn temp_forward_socket_paths() -> Vec<PathBuf> {
    let mut paths = Vec::new();
    if let Ok(entries) = std::fs::read_dir(std::env::temp_dir()) {
        for entry in entries.flatten() {
            let path = entry.path();
            if let Some(name) = path.file_name().and_then(|n| n.to_str()) {
                if name.starts_with("clux-ssh-") && name.ends_with(".sock") {
                    paths.push(path);
                }
            }
        }
    }
    paths.sort();
    paths
}

impl Default for FakeSshOptions {
    fn default() -> Self {
        Self {
            os: "Linux".to_string(),
            arch: "x86_64".to_string(),
            downloader: FakeDownloader::Curl,
            artifact_present: true,
        }
    }
}

impl FakeSshEnv {
    pub fn new() -> Result<Self, TestError> {
        Self::with_options(FakeSshOptions::default())
    }

    pub fn with_options(options: FakeSshOptions) -> Result<Self, TestError> {
        let guard = SSH_ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let temp_dir = std::env::temp_dir().join(format!(
            "clux-fake-ssh-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(&temp_dir)?;

        let home_dir = temp_dir.join("home");
        let bin_dir = temp_dir.join("bin");
        let remote_socket = temp_dir.join("remote.sock");
        let artifact_path = temp_dir.join("artifact.tar.gz");
        let download_count_path = temp_dir.join("download-count");
        let ssh_path = temp_dir.join("ssh");
        let bridge_bin = env!("CARGO_BIN_EXE_clux-test-forwarder");
        std::fs::create_dir_all(&home_dir)?;
        std::fs::create_dir_all(&bin_dir)?;

        populate_fake_bin_dir(&bin_dir, &artifact_path, &download_count_path, &options)?;
        if options.artifact_present {
            create_server_artifact(&artifact_path)?;
        }

        let script = format!(
            "#!/bin/sh\n\
set -eu\n\
spec=\"\"\n\
while [ \"$#\" -gt 0 ]; do\n\
  case \"$1\" in\n\
    -o)\n\
      shift 2\n\
      ;;\n\
    -N)\n\
      shift\n\
      ;;\n\
    -L)\n\
      spec=\"$2\"\n\
      shift 2\n\
      ;;\n\
    sh)\n\
      break\n\
      ;;\n\
    *)\n\
      if [ -z \"${{dest:-}}\" ]; then\n\
        dest=\"$1\"\n\
        shift\n\
      else\n\
        break\n\
      fi\n\
      ;;\n\
  esac\n\
done\n\
\n\
if [ -n \"$spec\" ]; then\n\
  local_socket=${{spec%%:*}}\n\
  remote_socket=${{spec#*:}}\n\
  exec \"{}\" \"$local_socket\" \"$remote_socket\"\n\
fi\n\
\n\
export HOME=\"{}\"\n\
export PATH=\"{}\"\n\
\n\
if [ \"$#\" -gt 0 ] && [ \"$1\" = \"sh\" ]; then\n\
  shift\n\
  if [ \"$#\" -gt 0 ] && ( [ \"$1\" = \"-c\" ] || [ \"$1\" = \"-lc\" ] ); then\n\
    shift\n\
    exec /bin/sh -c \"$@\"\n\
  fi\n\
  exec /bin/sh \"$@\"\n\
fi\n\
\n\
exec \"$@\"\n",
            bridge_bin,
            home_dir.display(),
            bin_dir.display(),
        );
        std::fs::write(&ssh_path, script)?;
        let mut perms = std::fs::metadata(&ssh_path)?.permissions();
        perms.set_mode(0o755);
        std::fs::set_permissions(&ssh_path, perms)?;

        let previous_path = std::env::var_os("PATH");
        let mut new_path = std::ffi::OsString::from(&temp_dir);
        if let Some(ref old) = previous_path {
            new_path.push(":");
            new_path.push(old);
        }
        std::env::set_var("PATH", new_path);

        Ok(Self {
            _guard: guard,
            temp_dir,
            home_dir,
            remote_socket,
            download_count_path,
            previous_path,
        })
    }

    pub fn remote_config(&self, cols: u16, rows: u16) -> ClientConfig {
        let mut config = ClientConfig::default();
        config.target = ClientTarget::RemoteSsh {
            destination: "fakehost".to_string(),
            socket_path: self.remote_socket.clone(),
        };
        config.term_cols = cols;
        config.term_rows = rows;
        config
    }

    pub fn remote_socket(&self) -> &PathBuf {
        &self.remote_socket
    }

    pub fn managed_binary_path(&self) -> PathBuf {
        self.home_dir
            .join(".local")
            .join("share")
            .join("clux")
            .join("server")
            .join(env!("CARGO_PKG_VERSION"))
            .join("clux-server")
    }

    pub fn install_root(&self) -> PathBuf {
        self.home_dir
            .join(".local")
            .join("share")
            .join("clux")
            .join("server")
    }

    pub fn download_count(&self) -> usize {
        std::fs::read_to_string(&self.download_count_path)
            .ok()
            .and_then(|s| s.trim().parse::<usize>().ok())
            .unwrap_or(0)
    }

    pub fn clux_command(&self) -> Command {
        let mut cmd = Command::new(env!("CARGO_BIN_EXE_clux"));
        cmd.env("PATH", std::env::var_os("PATH").unwrap_or_default());
        cmd
    }

    pub fn shutdown_server(&self) {
        let config = self.remote_config(80, 24);
        if let Ok(mut client) = Client::connect(config, false) {
            let _ = client.shutdown_server();
        }
    }
}

impl Drop for FakeSshEnv {
    fn drop(&mut self) {
        self.shutdown_server();
        match &self.previous_path {
            Some(path) => std::env::set_var("PATH", path),
            None => std::env::remove_var("PATH"),
        }
        let _ = std::fs::remove_dir_all(&self.temp_dir);
    }
}
