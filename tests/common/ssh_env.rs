//! The fake ssh environment: sets PATH and runs a server.

use super::harness::{TestError, SSH_ENV_LOCK};
use clux::client::{Client, ClientConfig, ClientTarget};
use std::os::unix::fs::PermissionsExt;
use std::path::PathBuf;
use std::process::Command;

use super::ssh::{FakeSshEnv, FakeSshOptions};
use super::ssh_fixtures::*;

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
