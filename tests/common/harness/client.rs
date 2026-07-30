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

use super::helpers::*;
use super::types::*;

pub static SSH_ENV_LOCK: Mutex<()> = Mutex::new(());

// ============================================================================
// Test Framework
// ============================================================================

/// Test client wrapper for automated workflow testing.
pub struct TestClient {
    pub client: Client,
    pub screen: ScreenBuffer,
    pub socket_path: PathBuf,
    pub server_process: Option<Child>,
    pub timeout: Duration,
    pub has_layout: bool,
}

impl TestClient {
    pub fn send_input(&mut self, bytes: &[u8]) -> &mut Self {
        if let Err(e) = self.client.send_input(bytes.to_vec()) {
            eprintln!("Failed to send input: {}", e);
        }
        self
    }

    pub fn type_text(&mut self, text: &str) -> &mut Self {
        self.send_input(text.as_bytes())
    }

    pub fn command(&mut self, action: CommandAction) -> &mut Self {
        if let Err(e) = self.client.send_command(action) {
            eprintln!("Failed to send command: {}", e);
        }
        self
    }

    pub fn split_horizontal(&mut self) -> &mut Self {
        self.command(CommandAction::SplitHorizontal)
    }

    pub fn split_vertical(&mut self) -> &mut Self {
        self.command(CommandAction::SplitVertical)
    }

    /// Scroll the focused pane: positive back in history, 0 returns to live.
    pub fn scroll(&mut self, lines: i32) -> &mut Self {
        if let Err(e) = self.client.send_scroll(lines) {
            eprintln!("Failed to send scroll: {}", e);
        }
        self
    }

    pub fn close_pane(&mut self) -> &mut Self {
        self.command(CommandAction::ClosePane)
    }

    pub fn navigate(&mut self, direction: Direction) -> &mut Self {
        self.command(CommandAction::NavigatePane(direction))
    }

    pub fn next_window(&mut self) -> &mut Self {
        self.command(CommandAction::NextWindow)
    }

    pub fn prev_window(&mut self) -> &mut Self {
        self.command(CommandAction::PrevWindow)
    }

    pub fn select_window(&mut self, index: usize) -> &mut Self {
        self.command(CommandAction::SelectWindow(index))
    }

    pub fn close_window(&mut self) -> &mut Self {
        self.command(CommandAction::CloseWindow)
    }
}

impl Drop for TestClient {
    fn drop(&mut self) {
        let _ = self.client.detach();
        if let Some(mut child) = self.server_process.take() {
            let _ = child.kill();
            let _ = child.wait();
        }
        let _ = std::fs::remove_file(&self.socket_path);
    }
}

#[derive(Default)]
pub struct TestClientBuilder {
    pub session_name: Option<String>,
    pub size: Option<(u16, u16)>,
    pub timeout: Option<Duration>,
}

impl TestClientBuilder {
    pub fn size(mut self, cols: u16, rows: u16) -> Self {
        self.size = Some((cols, rows));
        self
    }

    pub fn timeout(mut self, timeout: Duration) -> Self {
        self.timeout = Some(timeout);
        self
    }

    pub fn build(self) -> Result<TestClient, TestError> {
        let (cols, rows) = self.size.unwrap_or((80, 24));
        let timeout = self.timeout.unwrap_or(Duration::from_secs(5));

        let socket_path = unique_socket_path();
        let server_process = start_server(&socket_path)?;
        wait_for_socket(&socket_path, Duration::from_secs(5))?;

        let mut config = ClientConfig::default();
        config.target = ClientTarget::Local {
            socket_path: socket_path.clone(),
        };
        config.term_cols = cols;
        config.term_rows = rows;

        let mut client =
            Client::connect(config, false).map_err(|e| TestError::Client(e.to_string()))?;

        client
            .attach(self.session_name, true)
            .map_err(|e| TestError::Client(e.to_string()))?;

        let screen = ScreenBuffer::new(cols as usize, rows as usize);

        let mut test_client = TestClient {
            client,
            screen,
            socket_path,
            server_process: Some(server_process),
            timeout,
            has_layout: false,
        };

        test_client.wait_for_update()?;

        Ok(test_client)
    }
}
