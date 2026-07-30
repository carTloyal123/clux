//! Test error and screen-capture types.

use super::helpers::*;
use clux::client::{Client, ClientConfig, ClientTarget, ScreenBuffer};
use clux::protocol::{CommandAction, Direction, ServerMessage, WindowLayout};
use clux::selection::SelectionMode;
use std::io::{BufRead, BufReader};
use std::os::unix::fs::symlink;
use std::os::unix::fs::PermissionsExt;
use std::path::PathBuf;
use std::process::{Child, Command, Stdio};
use std::sync::{Mutex, MutexGuard};
use std::thread;
use std::time::{Duration, Instant};

/// Errors that can occur during testing.
#[derive(Debug)]
pub enum TestError {
    ServerStartTimeout,
    Timeout,
    Protocol(String),
    Io(std::io::Error),
    Client(String),
}

impl std::fmt::Display for TestError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            TestError::ServerStartTimeout => write!(f, "Server failed to start within timeout"),
            TestError::Timeout => write!(f, "Operation timed out"),
            TestError::Protocol(e) => write!(f, "Protocol error: {}", e),
            TestError::Io(e) => write!(f, "IO error: {}", e),
            TestError::Client(e) => write!(f, "Client error: {}", e),
        }
    }
}

impl From<std::io::Error> for TestError {
    fn from(e: std::io::Error) -> Self {
        TestError::Io(e)
    }
}

/// A snapshot of the screen state for assertions.
pub struct ScreenCapture {
    pub text_rows: Vec<String>,
    pub layout: Option<WindowLayout>,
}

impl ScreenCapture {
    pub fn from_screen_buffer(screen: &ScreenBuffer) -> Self {
        let (_cols, rows) = screen.dimensions();
        let mut text_rows = Vec::with_capacity(rows);

        for row_idx in 0..rows {
            if let Some(row_cells) = screen.get_row(row_idx) {
                let text: String = row_cells.iter().map(|c| c.c).collect();
                text_rows.push(text.trim_end().to_string());
            } else {
                text_rows.push(String::new());
            }
        }

        Self {
            text_rows,
            layout: screen.layout().cloned(),
        }
    }

    pub fn as_text(&self) -> String {
        self.text_rows.join("\n")
    }

    pub fn contains(&self, text: &str) -> bool {
        self.text_rows.iter().any(|row| row.contains(text))
    }

    pub fn pane_count(&self) -> usize {
        self.layout.as_ref().map(|l| l.panes.len()).unwrap_or(1)
    }

    pub fn focused_pane_id(&self) -> Option<u32> {
        self.layout
            .as_ref()?
            .panes
            .iter()
            .find(|p| p.focused)
            .map(|p| p.pane_id)
    }
}
