//! Fake ssh fixtures for the remote-mode tests.

#![allow(dead_code)]

pub use super::ssh_fixtures::*;

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
