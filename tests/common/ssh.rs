//! Fake ssh fixtures for the remote-mode tests.

#![allow(dead_code)]
#![allow(unused_imports)]

pub use super::ssh_fixtures::*;

use std::path::PathBuf;
use std::sync::MutexGuard;

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
