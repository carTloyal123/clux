//! Building the fake ssh binary tree and download tools.

use super::harness::TestError;
use super::ssh::{FakeDownloader, FakeSshOptions};

use std::os::unix::fs::symlink;
use std::os::unix::fs::PermissionsExt;
use std::path::PathBuf;
use std::process::Command;

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
