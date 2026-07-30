//! Server configuration and auto-shutdown policy.

use std::path::PathBuf;
use std::time::Duration;

use super::default_socket_path;
use crate::pty::detect_shell;

/// Server configuration.
#[derive(Debug, Clone)]
pub struct ServerConfig {
    /// Path to the Unix socket.
    pub socket_path: PathBuf,
    /// Shell to use for new sessions.
    pub shell: String,
    /// Default terminal dimensions.
    pub default_cols: u16,
    pub default_rows: u16,
    /// Turn URL-shaped text into real hyperlinks for the host terminal.
    ///
    /// Host terminals detect URLs against their own grid, where every clux row
    /// looks like a hard-wrapped line, so they cannot follow a URL that wraps
    /// inside a pane. Clux knows where its logical lines end, so it resolves
    /// those links itself. See [`crate::urls`].
    pub detect_plain_urls: bool,
}
impl Default for ServerConfig {
    fn default() -> Self {
        Self {
            socket_path: default_socket_path(),
            shell: detect_shell(),
            default_cols: 80,
            default_rows: 24,
            detect_plain_urls: true,
        }
    }
}
/// Configuration for automatic server shutdown.
#[derive(Debug, Clone)]
pub struct AutoShutdownConfig {
    /// Whether auto-shutdown is enabled.
    pub enabled: bool,
    /// Grace period before shutdown after last session closes.
    /// This allows for rapid "close session, create new session" workflows.
    pub grace_period: Duration,
    /// Timeout for first session creation after server start.
    /// If no session is created within this time, the server shuts down.
    /// This handles orphaned servers from failed client startup.
    pub first_session_timeout: Duration,
}

impl Default for AutoShutdownConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            grace_period: Duration::from_secs(1),
            first_session_timeout: Duration::from_secs(30),
        }
    }
}
