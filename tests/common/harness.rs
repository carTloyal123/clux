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

pub static SSH_ENV_LOCK: Mutex<()> = Mutex::new(());

// ============================================================================
// Test Framework
// ============================================================================

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
    pub fn new() -> TestClientBuilder {
        TestClientBuilder::default()
    }

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

    pub fn new_window(&mut self) -> &mut Self {
        self.command(CommandAction::NewWindow)
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

    pub fn drain_messages(&mut self) -> Result<usize, TestError> {
        let mut count = 0;
        loop {
            match self.client.try_recv() {
                Ok(Some(msg)) => {
                    self.handle_message(msg)?;
                    count += 1;
                }
                Ok(None) => break,
                Err(e) => return Err(TestError::Protocol(e.to_string())),
            }
        }
        Ok(count)
    }

    pub fn wait_for_update(&mut self) -> Result<(), TestError> {
        self.wait_until(|_| true)
    }

    pub fn wait_until<F>(&mut self, condition: F) -> Result<(), TestError>
    where
        F: Fn(&ScreenBuffer) -> bool,
    {
        let start = Instant::now();
        let mut interval = Duration::from_millis(10);
        let mut received_any = false;

        while start.elapsed() < self.timeout {
            loop {
                match self.client.try_recv() {
                    Ok(Some(msg)) => {
                        self.handle_message(msg)?;
                        received_any = true;
                    }
                    Ok(None) => break,
                    Err(e) => return Err(TestError::Protocol(e.to_string())),
                }
            }

            if received_any && condition(&self.screen) {
                return Ok(());
            }

            thread::sleep(interval);
            interval = std::cmp::min(interval * 2, Duration::from_millis(100));
        }

        Err(TestError::Timeout)
    }

    pub fn wait_for_text(&mut self, text: &str) -> Result<(), TestError> {
        let text = text.to_string();
        self.wait_until(|screen| {
            let (_cols, rows) = screen.dimensions();
            for row_idx in 0..rows {
                if let Some(row_cells) = screen.get_row(row_idx) {
                    let row_text: String = row_cells.iter().map(|c| c.c).collect();
                    if row_text.contains(&text) {
                        return true;
                    }
                }
            }
            false
        })
    }

    pub fn capture(&self) -> ScreenCapture {
        ScreenCapture::from_screen_buffer(&self.screen)
    }

    pub fn layout(&self) -> Option<&WindowLayout> {
        self.screen.layout()
    }

    pub fn pane_count(&self) -> usize {
        self.screen.layout().map(|l| l.panes.len()).unwrap_or(1)
    }

    /// The ANSI the client would write for one screen row, hyperlinks included.
    pub fn render_row_ansi(&self, row_idx: usize) -> String {
        self.screen.render_row_ansi(row_idx)
    }

    /// Every screen row's ANSI, joined - what the host terminal actually sees.
    pub fn render_screen_ansi(&self) -> String {
        let (_cols, rows) = self.screen.dimensions();
        (0..rows)
            .map(|row| self.screen.render_row_ansi(row))
            .collect::<Vec<_>>()
            .join("\n")
    }

    /// Hyperlink at a screen position, if any.
    pub fn link_at(&self, row: usize, col: usize) -> Option<String> {
        self.screen.link_at(row, col).map(str::to_string)
    }

    pub fn screen_dimensions(&self) -> (usize, usize) {
        self.screen.dimensions()
    }

    /// Drag-select between two screen positions and return the copied text.
    pub fn select_text(&mut self, from: (usize, usize), to: (usize, usize)) -> Option<String> {
        self.screen
            .begin_selection(from.0, from.1, SelectionMode::Normal);
        self.screen.extend_selection(to.0, to.1);
        self.screen.selected_text()
    }

    pub fn dump_screen(&self) -> String {
        let capture = self.capture();
        format!(
            "=== Screen ({} panes) ===\n{}\n=== Layout ===\n{:?}",
            self.pane_count(),
            capture.as_text(),
            self.layout()
        )
    }

    pub fn dump_server_log(&self, lines: usize) -> String {
        let path = dirs::state_dir()
            .unwrap_or_else(|| PathBuf::from("/tmp"))
            .join("clux")
            .join("clux-server.log");

        match std::fs::File::open(&path) {
            Ok(file) => {
                let reader = BufReader::new(file);
                let all_lines: Vec<String> = reader.lines().filter_map(|l| l.ok()).collect();
                let start = all_lines.len().saturating_sub(lines);
                all_lines[start..].join("\n")
            }
            Err(e) => format!("Failed to read log file {:?}: {}", path, e),
        }
    }

    pub fn handle_message(&mut self, msg: ServerMessage) -> Result<(), TestError> {
        match msg {
            ServerMessage::LayoutChanged { layout } => {
                self.screen.set_layout(layout);
                self.has_layout = true;
            }
            ServerMessage::PaneUpdate {
                pane_id,
                changed_rows,
                cursor: _,
            } => {
                self.screen.apply_pane_update(pane_id, &changed_rows);
            }
            ServerMessage::Detached { .. } | ServerMessage::Shutdown => {}
            _ => {}
        }
        Ok(())
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
pub fn assert_pane_count(client: &TestClient, expected: usize) {
    let actual = client.pane_count();
    assert_eq!(
        actual,
        expected,
        "Expected {} panes, got {}\n\nLayout: {:?}",
        expected,
        actual,
        client.layout()
    );
}

pub fn assert_contains(client: &TestClient, text: &str) {
    let capture = client.capture();
    assert!(
        capture.contains(text),
        "Expected screen to contain '{}'\n\nActual screen content:\n{}",
        text,
        capture.as_text()
    );
}

/// The OSC 8 open sequences in one row's ANSI, as (id, url) pairs.
pub fn hyperlinks_in(ansi: &str) -> Vec<(String, String)> {
    let mut links = Vec::new();

    for chunk in ansi.split("\x1b]8;").skip(1) {
        let Some(body) = chunk.split("\x1b\\").next() else {
            continue;
        };
        // "id=<id>;<url>" for an open, "" or ";" for a close.
        let Some((params, url)) = body.split_once(';') else {
            continue;
        };
        if url.is_empty() {
            continue;
        }
        let id = params.strip_prefix("id=").unwrap_or(params).to_string();
        links.push((id, url.to_string()));
    }

    links
}

/// Every hyperlink the client would emit, as (row, id, url).
pub fn hyperlinks_by_row(client: &TestClient) -> Vec<(usize, String, String)> {
    let (_cols, rows) = client.screen_dimensions();
    (0..rows)
        .flat_map(|row| {
            hyperlinks_in(&client.render_row_ansi(row))
                .into_iter()
                .map(move |(id, url)| (row, id, url))
        })
        .collect()
}
