//! The end-to-end test harness: a client driving a real server process.

#![allow(dead_code)]

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

use super::client::*;
use super::types::*;

pub fn unique_socket_path() -> PathBuf {
    let pid = std::process::id();
    let timestamp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let tid = format!("{:?}", std::thread::current().id());

    PathBuf::from(format!(
        "/tmp/clux-test-{}-{}-{}.sock",
        pid,
        tid.replace(|c: char| !c.is_alphanumeric(), ""),
        timestamp
    ))
}

pub fn start_server(socket_path: &PathBuf) -> Result<Child, TestError> {
    start_server_with_auto_exit(socket_path, false)
}

/// Start a server, optionally leaving session-driven auto-shutdown enabled.
///
/// Most tests pass `false` so the server cannot vanish mid-test; the lifecycle
/// tests pass `true` because auto-shutdown is what they are checking.
pub fn start_server_with_auto_exit(
    socket_path: &PathBuf,
    auto_exit: bool,
) -> Result<Child, TestError> {
    let server_bin = env!("CARGO_BIN_EXE_clux-server");

    let mut command = Command::new(server_bin);
    command.arg("--socket").arg(socket_path);
    if !auto_exit {
        command.arg("--no-auto-exit");
    }

    let child = command
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .spawn()?;

    Ok(child)
}

/// A client attached straight to a socket, with no server started for it.
pub fn attach_client(
    socket_path: &PathBuf,
    session: &str,
    create: bool,
    start_server: bool,
) -> Result<Client, TestError> {
    let mut config = ClientConfig::default();
    config.target = ClientTarget::Local {
        socket_path: socket_path.clone(),
    };
    config.term_cols = 80;
    config.term_rows = 24;

    let mut client =
        Client::connect(config, start_server).map_err(|e| TestError::Client(e.to_string()))?;
    client
        .attach(Some(session.to_string()), create)
        .map_err(|e| TestError::Client(e.to_string()))?;

    Ok(client)
}

/// Drain messages until `text` shows up in a pane update, or time out.
pub fn wait_for_text_on(
    client: &mut Client,
    text: &str,
    timeout: Duration,
) -> Result<(), TestError> {
    let start = Instant::now();

    while start.elapsed() < timeout {
        while let Ok(Some(msg)) = client.try_recv() {
            if let ServerMessage::PaneUpdate { changed_rows, .. } = msg {
                for row in &changed_rows {
                    let row_text: String = row.cells.iter().map(|c| c.c).collect();
                    if row_text.contains(text) {
                        return Ok(());
                    }
                }
            }
        }
        thread::sleep(Duration::from_millis(20));
    }

    Err(TestError::Timeout)
}

/// Keep draining server messages for a while, as a real client's loop does.
///
/// This matters: the server writes to clients synchronously, so a client that
/// stops reading stalls it.
pub fn drain_for(client: &mut Client, duration: Duration) {
    let start = Instant::now();
    while start.elapsed() < duration {
        while matches!(client.try_recv(), Ok(Some(_))) {}
        thread::sleep(Duration::from_millis(10));
    }
}

/// Wait for a process to exit on its own.
pub fn wait_for_exit(child: &mut Child, timeout: Duration) -> bool {
    let start = Instant::now();

    while start.elapsed() < timeout {
        match child.try_wait() {
            Ok(Some(_)) => return true,
            Ok(None) => thread::sleep(Duration::from_millis(50)),
            Err(_) => return false,
        }
    }

    false
}

pub fn wait_for_socket(socket_path: &PathBuf, timeout: Duration) -> Result<(), TestError> {
    let start = Instant::now();
    while start.elapsed() < timeout {
        if socket_path.exists() {
            return Ok(());
        }
        thread::sleep(Duration::from_millis(50));
    }
    Err(TestError::ServerStartTimeout)
}

// Assertion helpers
