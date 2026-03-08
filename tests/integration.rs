//! Integration tests for clux terminal multiplexer.
//!
//! These tests spawn real server processes and verify end-to-end workflows.
//!
//! Run with: cargo test --test integration
//! Run specific test: cargo test --test integration test_horizontal_split -- --nocapture

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

static SSH_ENV_LOCK: Mutex<()> = Mutex::new(());

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
    text_rows: Vec<String>,
    layout: Option<WindowLayout>,
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
    client: Client,
    screen: ScreenBuffer,
    socket_path: PathBuf,
    server_process: Option<Child>,
    timeout: Duration,
    has_layout: bool,
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

    fn handle_message(&mut self, msg: ServerMessage) -> Result<(), TestError> {
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
            ServerMessage::FullScreen { .. } | ServerMessage::Update { .. } => {
                // V1 fallback - ignored in test client
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
    session_name: Option<String>,
    size: Option<(u16, u16)>,
    timeout: Option<Duration>,
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

fn unique_socket_path() -> PathBuf {
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

fn start_server(socket_path: &PathBuf) -> Result<Child, TestError> {
    let server_bin = env!("CARGO_BIN_EXE_clux-server");

    let child = Command::new(server_bin)
        .arg("--socket")
        .arg(socket_path)
        .arg("--no-auto-exit")
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .spawn()?;

    Ok(child)
}

fn wait_for_socket(socket_path: &PathBuf, timeout: Duration) -> Result<(), TestError> {
    let start = Instant::now();
    while start.elapsed() < timeout {
        if socket_path.exists() {
            return Ok(());
        }
        thread::sleep(Duration::from_millis(50));
    }
    Err(TestError::ServerStartTimeout)
}

#[derive(Debug, Clone, Copy)]
enum FakeDownloader {
    Curl,
    Wget,
    None,
}

#[derive(Debug, Clone)]
struct FakeSshOptions {
    os: String,
    arch: String,
    downloader: FakeDownloader,
    artifact_present: bool,
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

struct FakeSshEnv {
    _guard: MutexGuard<'static, ()>,
    temp_dir: PathBuf,
    home_dir: PathBuf,
    remote_socket: PathBuf,
    download_count_path: PathBuf,
    previous_path: Option<std::ffi::OsString>,
}

impl FakeSshEnv {
    fn new() -> Result<Self, TestError> {
        Self::with_options(FakeSshOptions::default())
    }

    fn with_options(options: FakeSshOptions) -> Result<Self, TestError> {
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
        let bridge_bin = env!("CARGO_BIN_EXE_clux-ssh-bridge");
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
  if [ \"$#\" -gt 0 ] && [ \"$1\" = \"-lc\" ]; then\n\
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

    fn remote_config(&self, cols: u16, rows: u16) -> ClientConfig {
        let mut config = ClientConfig::default();
        config.target = ClientTarget::RemoteSsh {
            destination: "fakehost".to_string(),
            socket_path: self.remote_socket.clone(),
        };
        config.term_cols = cols;
        config.term_rows = rows;
        config
    }

    fn remote_socket(&self) -> &PathBuf {
        &self.remote_socket
    }

    fn managed_binary_path(&self) -> PathBuf {
        self.home_dir
            .join(".local")
            .join("share")
            .join("clux")
            .join("server")
            .join(env!("CARGO_PKG_VERSION"))
            .join("clux-server")
    }

    fn install_root(&self) -> PathBuf {
        self.home_dir
            .join(".local")
            .join("share")
            .join("clux")
            .join("server")
    }

    fn download_count(&self) -> usize {
        std::fs::read_to_string(&self.download_count_path)
            .ok()
            .and_then(|s| s.trim().parse::<usize>().ok())
            .unwrap_or(0)
    }

    fn clux_command(&self) -> Command {
        let mut cmd = Command::new(env!("CARGO_BIN_EXE_clux"));
        cmd.env("PATH", std::env::var_os("PATH").unwrap_or_default());
        cmd
    }

    fn shutdown_server(&self) {
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

fn system_command_path(name: &str) -> Result<PathBuf, TestError> {
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

fn populate_fake_bin_dir(
    bin_dir: &PathBuf,
    artifact_path: &PathBuf,
    download_count_path: &PathBuf,
    options: &FakeSshOptions,
) -> Result<(), TestError> {
    for cmd in [
        "mkdir", "mv", "chmod", "tar", "rm", "dirname", "nohup", "cp", "cat",
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

fn create_fake_downloader(
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

fn create_server_artifact(artifact_path: &PathBuf) -> Result<(), TestError> {
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

fn temp_forward_socket_paths() -> Vec<PathBuf> {
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

// Assertion helpers
fn assert_pane_count(client: &TestClient, expected: usize) {
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

fn assert_contains(client: &TestClient, text: &str) {
    let capture = client.capture();
    assert!(
        capture.contains(text),
        "Expected screen to contain '{}'\n\nActual screen content:\n{}",
        text,
        capture.as_text()
    );
}

// ============================================================================
// Pane Tests
// ============================================================================

#[test]
fn test_single_pane_initial_state() {
    let client = TestClient::new()
        .size(80, 24)
        .timeout(Duration::from_secs(10))
        .build()
        .expect("Failed to create test client");

    assert_pane_count(&client, 1);
}

#[test]
fn test_horizontal_split_creates_two_panes() {
    let mut client = TestClient::new()
        .size(80, 24)
        .timeout(Duration::from_secs(10))
        .build()
        .expect("Failed to create test client");

    assert_pane_count(&client, 1);

    client.split_horizontal();
    client.wait_for_update().expect("wait_for_update failed");

    assert_pane_count(&client, 2);
}

#[test]
fn test_vertical_split_creates_two_panes() {
    let mut client = TestClient::new()
        .size(80, 24)
        .timeout(Duration::from_secs(10))
        .build()
        .expect("Failed to create test client");

    client.split_vertical();
    client.wait_for_update().expect("wait_for_update failed");

    assert_pane_count(&client, 2);
}

#[test]
fn test_three_pane_layout() {
    let mut client = TestClient::new()
        .size(100, 40)
        .timeout(Duration::from_secs(10))
        .build()
        .expect("Failed to create test client");

    client.split_vertical();
    // Wait until we have 2 panes
    client
        .wait_until(|s| s.layout().map(|l| l.panes.len() >= 2).unwrap_or(false))
        .expect("wait for 2 panes");
    assert_pane_count(&client, 2);

    client.split_horizontal();
    // Wait until we have 3 panes
    client
        .wait_until(|s| s.layout().map(|l| l.panes.len() >= 3).unwrap_or(false))
        .expect("wait for 3 panes");
    assert_pane_count(&client, 3);
}

#[test]
fn test_close_pane_reduces_count() {
    let mut client = TestClient::new()
        .size(80, 24)
        .timeout(Duration::from_secs(10))
        .build()
        .expect("Failed to create test client");

    client.split_horizontal();
    // Wait until we have 2 panes
    client
        .wait_until(|s| s.layout().map(|l| l.panes.len() >= 2).unwrap_or(false))
        .expect("wait for 2 panes");
    assert_pane_count(&client, 2);

    client.close_pane();
    // Wait until we're back to 1 pane
    client
        .wait_until(|s| s.layout().map(|l| l.panes.len() == 1).unwrap_or(false))
        .expect("wait for 1 pane");
    assert_pane_count(&client, 1);
}

// ============================================================================
// Input/Output Tests
// ============================================================================

#[test]
fn test_type_echo_see_output() {
    let mut client = TestClient::new()
        .size(80, 24)
        .timeout(Duration::from_secs(10))
        .build()
        .expect("Failed to create test client");

    // Let shell initialize
    std::thread::sleep(Duration::from_millis(500));
    client.drain_messages().ok();

    client.type_text("echo TESTOUTPUT123\n");

    client
        .wait_for_text("TESTOUTPUT123")
        .expect("Should see echo output");

    assert_contains(&client, "TESTOUTPUT123");
}

#[test]
fn test_type_in_split_pane() {
    let mut client = TestClient::new()
        .size(80, 24)
        .timeout(Duration::from_secs(10))
        .build()
        .expect("Failed to create test client");

    client.split_horizontal();
    client
        .wait_for_update()
        .expect("wait_for_update after split");

    std::thread::sleep(Duration::from_millis(500));
    client.drain_messages().ok();

    client.type_text("echo PANEB_OUTPUT\n");

    client
        .wait_for_text("PANEB_OUTPUT")
        .expect("Should see output in split pane");
}

// ============================================================================
// Full Workflow Tests
// ============================================================================

#[test]
fn test_full_workflow_split_and_type() {
    let mut client = TestClient::new()
        .size(100, 40)
        .timeout(Duration::from_secs(15))
        .build()
        .expect("Failed to create test client");

    std::thread::sleep(Duration::from_millis(500));
    client.drain_messages().ok();

    // Step 1: Split vertical
    client.split_vertical();
    client
        .wait_until(|s| s.layout().map(|l| l.panes.len() >= 2).unwrap_or(false))
        .expect("wait for 2 panes");
    assert_pane_count(&client, 2);

    // Step 2: Type in focused pane (right pane after split)
    std::thread::sleep(Duration::from_millis(300));
    client.drain_messages().ok();
    client.type_text("echo PANE_OUTPUT\n");
    client
        .wait_for_text("PANE_OUTPUT")
        .expect("Should see PANE_OUTPUT");

    // Step 3: Split again horizontally
    client.split_horizontal();
    client
        .wait_until(|s| s.layout().map(|l| l.panes.len() >= 3).unwrap_or(false))
        .expect("wait for 3 panes");
    assert_pane_count(&client, 3);

    // Step 4: Type in new pane
    std::thread::sleep(Duration::from_millis(300));
    client.drain_messages().ok();
    client.type_text("echo THIRD_PANE\n");
    client
        .wait_for_text("THIRD_PANE")
        .expect("Should see THIRD_PANE");

    // Both outputs should be visible
    let capture = client.capture();
    assert!(capture.contains("PANE_OUTPUT"), "Missing PANE_OUTPUT");
    assert!(capture.contains("THIRD_PANE"), "Missing THIRD_PANE");
}

// ============================================================================
// Shell Exit Tests
// ============================================================================

#[test]
fn test_exit_closes_pane() {
    let mut client = TestClient::new()
        .size(80, 24)
        .timeout(Duration::from_secs(15))
        .build()
        .expect("Failed to create test client");

    // Split to have 2 panes
    client.split_horizontal();
    client
        .wait_until(|s| s.layout().map(|l| l.panes.len() >= 2).unwrap_or(false))
        .expect("wait for 2 panes");
    assert_pane_count(&client, 2);

    // Wait for shell to initialize
    std::thread::sleep(Duration::from_millis(500));
    client.drain_messages().ok();

    // Type exit in focused pane
    client.type_text("exit\n");

    // Wait for pane to close (should go back to 1 pane)
    client
        .wait_until(|s| s.layout().map(|l| l.panes.len() == 1).unwrap_or(false))
        .expect("wait for pane to close after exit");

    assert_pane_count(&client, 1);
}

#[test]
fn test_exit_multiple_panes() {
    let mut client = TestClient::new()
        .size(100, 40)
        .timeout(Duration::from_secs(20))
        .build()
        .expect("Failed to create test client");

    // Create 3 panes
    client.split_vertical();
    client
        .wait_until(|s| s.layout().map(|l| l.panes.len() >= 2).unwrap_or(false))
        .expect("wait for 2 panes");

    client.split_horizontal();
    client
        .wait_until(|s| s.layout().map(|l| l.panes.len() >= 3).unwrap_or(false))
        .expect("wait for 3 panes");
    assert_pane_count(&client, 3);

    // Wait for shells
    std::thread::sleep(Duration::from_millis(500));
    client.drain_messages().ok();

    // Exit first pane
    client.type_text("exit\n");
    client
        .wait_until(|s| s.layout().map(|l| l.panes.len() == 2).unwrap_or(false))
        .expect("wait for 2 panes after first exit");
    assert_pane_count(&client, 2);

    // Wait for new focused pane's shell
    std::thread::sleep(Duration::from_millis(300));
    client.drain_messages().ok();

    // Exit second pane
    client.type_text("exit\n");
    client
        .wait_until(|s| s.layout().map(|l| l.panes.len() == 1).unwrap_or(false))
        .expect("wait for 1 pane after second exit");
    assert_pane_count(&client, 1);
}

// ============================================================================
// Window Tests
// ============================================================================

#[test]
fn test_new_window_triggers_layout_update() {
    let mut client = TestClient::new()
        .size(80, 24)
        .timeout(Duration::from_secs(10))
        .build()
        .expect("Failed to create test client");

    assert_pane_count(&client, 1);

    // Create a new window - should receive layout update
    client.new_window();
    client
        .wait_for_update()
        .expect("Should receive layout update after new window");

    // New window should have 1 pane
    assert_pane_count(&client, 1);
}

#[test]
fn test_window_switch_triggers_layout_update() {
    let mut client = TestClient::new()
        .size(80, 24)
        .timeout(Duration::from_secs(10))
        .build()
        .expect("Failed to create test client");

    // Create second window
    client.new_window();
    client
        .wait_for_update()
        .expect("layout update after new window");

    // Switch back to first window
    client.prev_window();
    client
        .wait_for_update()
        .expect("Should receive layout update after prev_window");

    assert_pane_count(&client, 1);
}

#[test]
fn test_window_with_splits_then_switch() {
    let mut client = TestClient::new()
        .size(80, 24)
        .timeout(Duration::from_secs(15))
        .build()
        .expect("Failed to create test client");

    // Split window 0 into 2 panes
    client.split_horizontal();
    client
        .wait_until(|s| s.layout().map(|l| l.panes.len() >= 2).unwrap_or(false))
        .expect("wait for 2 panes");
    assert_pane_count(&client, 2);

    // Create new window (window 1 with 1 pane)
    // Note: Pane IDs are per-window, so new window will have pane 0
    client.new_window();

    // Wait until we see a layout with 1 pane (the new window)
    // The 2-pane layout message might arrive first due to timing, so wait for 1 pane
    client
        .wait_until(|s| s.layout().map(|l| l.panes.len() == 1).unwrap_or(false))
        .expect("wait for 1 pane in new window");
    assert_pane_count(&client, 1);

    // Switch back to window 0 - should show 2 panes again
    client.prev_window();
    client
        .wait_until(|s| s.layout().map(|l| l.panes.len() >= 2).unwrap_or(false))
        .expect("wait for 2 panes after switching back");
    assert_pane_count(&client, 2);
}

#[test]
fn test_next_prev_window_cycle() {
    let mut client = TestClient::new()
        .size(80, 24)
        .timeout(Duration::from_secs(10))
        .build()
        .expect("Failed to create test client");

    // Create second window
    client.new_window();
    client.wait_for_update().expect("layout after new window");

    // next_window should cycle back to window 0
    client.next_window();
    client
        .wait_for_update()
        .expect("layout update after next_window");
    assert_pane_count(&client, 1);

    // prev_window should go to window 1
    client.prev_window();
    client
        .wait_for_update()
        .expect("layout update after prev_window");
    assert_pane_count(&client, 1);
}

#[test]
fn test_select_window_by_index() {
    let mut client = TestClient::new()
        .size(80, 24)
        .timeout(Duration::from_secs(10))
        .build()
        .expect("Failed to create test client");

    // Create second window with a split
    client.new_window();
    client.wait_for_update().expect("layout after new window");
    client.split_vertical();
    client
        .wait_until(|s| s.layout().map(|l| l.panes.len() >= 2).unwrap_or(false))
        .expect("wait for 2 panes in window 1");
    assert_pane_count(&client, 2);

    // Select window 0 (should have 1 pane)
    client.select_window(0);
    client
        .wait_until(|s| s.layout().map(|l| l.panes.len() == 1).unwrap_or(false))
        .expect("wait for 1 pane in window 0");
    assert_pane_count(&client, 1);

    // Select window 1 (should have 2 panes)
    client.select_window(1);
    client
        .wait_until(|s| s.layout().map(|l| l.panes.len() >= 2).unwrap_or(false))
        .expect("wait for 2 panes in window 1");
    assert_pane_count(&client, 2);
}

#[test]
fn test_type_in_different_windows() {
    let mut client = TestClient::new()
        .size(80, 24)
        .timeout(Duration::from_secs(15))
        .build()
        .expect("Failed to create test client");

    // Wait for initial shell
    std::thread::sleep(Duration::from_millis(500));
    client.drain_messages().ok();

    // Type in window 0
    client.type_text("echo WINDOW_ZERO\n");
    client
        .wait_for_text("WINDOW_ZERO")
        .expect("Should see WINDOW_ZERO");

    // Create new window and wait for layout with 1 pane (fresh window)
    // Note: Window 0 has 1 pane too, but the screen content will be different
    client.new_window();
    client.wait_for_update().expect("layout after new window");

    // Wait for shell in new window
    std::thread::sleep(Duration::from_millis(500));
    client.drain_messages().ok();

    client.type_text("echo WINDOW_ONE\n");
    client
        .wait_for_text("WINDOW_ONE")
        .expect("Should see WINDOW_ONE");

    // Window 1 should show WINDOW_ONE but not WINDOW_ZERO
    let capture = client.capture();
    assert!(capture.contains("WINDOW_ONE"), "Should contain WINDOW_ONE");
    assert!(
        !capture.contains("WINDOW_ZERO"),
        "Should NOT contain WINDOW_ZERO in window 1"
    );

    // Switch back to window 0
    client.prev_window();
    client
        .wait_for_text("WINDOW_ZERO")
        .expect("wait for WINDOW_ZERO after switching back");

    // Window 0 should show WINDOW_ZERO but not WINDOW_ONE
    let capture = client.capture();
    assert!(
        capture.contains("WINDOW_ZERO"),
        "Should contain WINDOW_ZERO in window 0"
    );
    assert!(
        !capture.contains("WINDOW_ONE"),
        "Should NOT contain WINDOW_ONE in window 0"
    );
}

#[test]
fn test_navigate_pane_triggers_update() {
    let mut client = TestClient::new()
        .size(80, 24)
        .timeout(Duration::from_secs(10))
        .build()
        .expect("Failed to create test client");

    // Create 2 panes with vertical split (left/right)
    client.split_vertical();
    client
        .wait_until(|s| s.layout().map(|l| l.panes.len() >= 2).unwrap_or(false))
        .expect("wait for 2 panes");

    // Remember initial focused pane
    let initial_focused = client.capture().focused_pane_id();

    // Navigate left - should trigger layout update with changed focus
    // After vertical split, focus is on right pane, so left should work
    client.navigate(Direction::Left);

    // Wait for the focus to actually change
    client
        .wait_until(|s| {
            s.layout()
                .and_then(|l| l.panes.iter().find(|p| p.focused).map(|p| p.pane_id))
                != initial_focused
        })
        .expect("Should receive layout update with changed focus after navigate");

    // Focus should have changed
    let new_focused = client.capture().focused_pane_id();
    assert_ne!(
        initial_focused, new_focused,
        "Focus should change after navigation"
    );
}

// ============================================================================
// Pane Content Preservation Tests
// ============================================================================

#[test]
fn test_split_preserves_original_pane_content() {
    let mut client = TestClient::new()
        .size(80, 24)
        .timeout(Duration::from_secs(15))
        .build()
        .expect("Failed to create test client");

    // Wait for shell to initialize
    std::thread::sleep(Duration::from_millis(500));
    client.drain_messages().ok();

    // Type some content that should be visible
    client.type_text("echo ORIGINAL_CONTENT\n");
    client
        .wait_for_text("ORIGINAL_CONTENT")
        .expect("Should see original content");

    // Verify content is visible before split
    let capture_before = client.capture();
    assert!(
        capture_before.contains("ORIGINAL_CONTENT"),
        "Content should be visible before split"
    );

    // Now split horizontally - this creates a new pane below
    client.split_horizontal();

    // Wait for both: 2 panes AND the original content to still be visible
    // This ensures we've received the pane updates, not just the layout
    client
        .wait_until(|s| {
            let has_two_panes = s.layout().map(|l| l.panes.len() >= 2).unwrap_or(false);
            if !has_two_panes {
                return false;
            }
            // Check if content is in the buffer
            let (_cols, rows) = s.dimensions();
            for row_idx in 0..rows {
                if let Some(row_cells) = s.get_row(row_idx) {
                    let row_text: String = row_cells.iter().map(|c| c.c).collect();
                    if row_text.contains("ORIGINAL_CONTENT") {
                        return true;
                    }
                }
            }
            false
        })
        .expect("wait for 2 panes with original content preserved");

    assert_pane_count(&client, 2);

    let capture_after = client.capture();
    assert!(
        capture_after.contains("ORIGINAL_CONTENT"),
        "Original content should be preserved after split.\n\nScreen content:\n{}",
        capture_after.as_text()
    );
}

#[test]
fn test_vertical_split_preserves_original_pane_content() {
    let mut client = TestClient::new()
        .size(100, 24)
        .timeout(Duration::from_secs(15))
        .build()
        .expect("Failed to create test client");

    // Wait for shell to initialize
    std::thread::sleep(Duration::from_millis(500));
    client.drain_messages().ok();

    // Type some content
    client.type_text("echo LEFTPANE_TEXT\n");
    client
        .wait_for_text("LEFTPANE_TEXT")
        .expect("Should see left pane content");

    // Verify content is visible before split
    let capture_before = client.capture();
    assert!(
        capture_before.contains("LEFTPANE_TEXT"),
        "Content should be visible before split"
    );

    // Split vertically - creates a new pane on the right
    client.split_vertical();

    // Wait for both: 2 panes AND the original content to still be visible
    client
        .wait_until(|s| {
            let has_two_panes = s.layout().map(|l| l.panes.len() >= 2).unwrap_or(false);
            if !has_two_panes {
                return false;
            }
            // Check if content is in the buffer
            let (_cols, rows) = s.dimensions();
            for row_idx in 0..rows {
                if let Some(row_cells) = s.get_row(row_idx) {
                    let row_text: String = row_cells.iter().map(|c| c.c).collect();
                    if row_text.contains("LEFTPANE_TEXT") {
                        return true;
                    }
                }
            }
            false
        })
        .expect("wait for 2 panes with original content preserved");

    assert_pane_count(&client, 2);

    let capture_after = client.capture();
    assert!(
        capture_after.contains("LEFTPANE_TEXT"),
        "Original content should be preserved after vertical split.\n\nScreen content:\n{}",
        capture_after.as_text()
    );
}

// ============================================================================
// Remote SSH Tests
// ============================================================================

#[test]
fn test_remote_client_can_create_and_list_session_via_ssh() {
    let env = FakeSshEnv::new().expect("fake ssh env");

    let mut client = Client::connect(env.remote_config(80, 24), true).expect("remote connect");
    client
        .attach(Some("remote".to_string()), true)
        .expect("remote attach");
    client.detach().expect("remote detach");

    let mut list_client =
        Client::connect(env.remote_config(80, 24), false).expect("remote list connect");
    let sessions = list_client.list_sessions().expect("remote list sessions");

    assert!(sessions.iter().any(|session| session.name == "remote"));
    assert!(env.managed_binary_path().exists());
    assert_eq!(env.download_count(), 1);
}

#[test]
fn test_remote_client_can_kill_session_via_ssh() {
    let env = FakeSshEnv::new().expect("fake ssh env");

    let mut client = Client::connect(env.remote_config(80, 24), true).expect("remote connect");
    client
        .attach(Some("killme".to_string()), true)
        .expect("remote attach");
    client.detach().expect("remote detach");

    let mut admin = Client::connect(env.remote_config(80, 24), false).expect("remote admin");
    admin.kill_session("killme").expect("kill session");

    let mut check = Client::connect(env.remote_config(80, 24), false).expect("remote check");
    let sessions = check.list_sessions().expect("session list");
    assert!(!sessions.iter().any(|session| session.name == "killme"));
}

#[test]
fn test_remote_bootstrap_reuses_managed_install() {
    let env = FakeSshEnv::new().expect("fake ssh env");

    let mut first = Client::connect(env.remote_config(80, 24), true).expect("first connect");
    first
        .attach(Some("reuse".to_string()), true)
        .expect("first attach");
    first.detach().expect("first detach");

    assert!(env.managed_binary_path().exists());
    assert_eq!(env.download_count(), 1);

    let mut second = Client::connect(env.remote_config(80, 24), true).expect("second connect");
    second
        .attach(Some("reuse".to_string()), false)
        .expect("second attach");
    second.detach().expect("second detach");

    assert_eq!(
        env.download_count(),
        1,
        "bootstrap should reuse installed binary"
    );
}

#[test]
fn test_remote_bootstrap_fails_when_artifact_missing() {
    let env = FakeSshEnv::with_options(FakeSshOptions {
        artifact_present: false,
        ..FakeSshOptions::default()
    })
    .expect("fake ssh env");

    let err = match Client::connect(env.remote_config(80, 24), true) {
        Ok(_) => panic!("expected remote bootstrap to fail"),
        Err(err) => err,
    };
    assert!(matches!(
        err,
        clux::client::ClientError::RemoteArtifactUnavailable { .. }
    ));
}

#[test]
fn test_remote_bootstrap_fails_when_platform_unsupported() {
    let env = FakeSshEnv::with_options(FakeSshOptions {
        arch: "riscv64".to_string(),
        ..FakeSshOptions::default()
    })
    .expect("fake ssh env");

    let err = match Client::connect(env.remote_config(80, 24), true) {
        Ok(_) => panic!("expected unsupported platform failure"),
        Err(err) => err,
    };
    assert!(matches!(
        err,
        clux::client::ClientError::RemotePlatformUnsupported { .. }
    ));
}

#[test]
fn test_remote_bootstrap_fails_without_downloader() {
    let env = FakeSshEnv::with_options(FakeSshOptions {
        downloader: FakeDownloader::None,
        ..FakeSshOptions::default()
    })
    .expect("fake ssh env");

    let err = match Client::connect(env.remote_config(80, 24), true) {
        Ok(_) => panic!("expected missing downloader failure"),
        Err(err) => err,
    };
    assert!(matches!(
        err,
        clux::client::ClientError::RemoteMissingDownloadTool
    ));
}

#[test]
fn test_remote_connect_without_autostart_does_not_bootstrap() {
    let env = FakeSshEnv::new().expect("fake ssh env");

    let err = match Client::connect(env.remote_config(80, 24), false) {
        Ok(_) => panic!("expected connection failure without bootstrap"),
        Err(err) => err,
    };
    assert!(matches!(
        err,
        clux::client::ClientError::ConnectionFailed(_)
    ));
    assert!(!env.install_root().exists());
    assert_eq!(env.download_count(), 0);
}

#[test]
fn test_remote_bootstrap_works_with_wget_fallback() {
    let env = FakeSshEnv::with_options(FakeSshOptions {
        downloader: FakeDownloader::Wget,
        ..FakeSshOptions::default()
    })
    .expect("fake ssh env");

    let mut client = Client::connect(env.remote_config(80, 24), true).expect("remote connect");
    client
        .attach(Some("wget".to_string()), true)
        .expect("remote attach");
    client.detach().expect("remote detach");

    assert!(env.managed_binary_path().exists());
    assert_eq!(env.download_count(), 1);
}

#[test]
fn test_remote_cli_info_and_kill_server() {
    let env = FakeSshEnv::new().expect("fake ssh env");

    let mut client = Client::connect(env.remote_config(80, 24), true).expect("remote connect");
    client
        .attach(Some("info".to_string()), true)
        .expect("remote attach");
    client.detach().expect("remote detach");

    let info_output = env
        .clux_command()
        .args([
            "info",
            "--remote",
            "fakehost",
            "--socket",
            env.remote_socket().to_str().unwrap(),
        ])
        .output()
        .expect("run clux info");
    assert!(info_output.status.success());
    let info_stdout = String::from_utf8_lossy(&info_output.stdout);
    assert!(info_stdout.contains("Server: running"));
    assert!(info_stdout.contains("Mode: remote"));
    assert!(info_stdout.contains("Remote: fakehost"));

    let kill_output = env
        .clux_command()
        .args([
            "kill-server",
            "--remote",
            "fakehost",
            "--socket",
            env.remote_socket().to_str().unwrap(),
        ])
        .output()
        .expect("run clux kill-server");
    assert!(kill_output.status.success());
    let kill_stdout = String::from_utf8_lossy(&kill_output.stdout);
    assert!(kill_stdout.contains("Server stopped"));

    let info_after = env
        .clux_command()
        .args([
            "info",
            "--remote",
            "fakehost",
            "--socket",
            env.remote_socket().to_str().unwrap(),
        ])
        .output()
        .expect("run clux info after shutdown");
    assert!(info_after.status.success());
    let info_after_stdout = String::from_utf8_lossy(&info_after.stdout);
    assert!(info_after_stdout.contains("Server: not running"));
}

#[test]
fn test_remote_tunnel_cleanup_removes_forwarded_socket() {
    let env = FakeSshEnv::new().expect("fake ssh env");
    let before = temp_forward_socket_paths();

    {
        let mut client = Client::connect(env.remote_config(80, 24), true).expect("remote connect");
        client
            .attach(Some("cleanup".to_string()), true)
            .expect("remote attach");
        client.detach().expect("remote detach");
    }

    let after = temp_forward_socket_paths();
    assert_eq!(
        before, after,
        "forwarded ssh socket should be removed after client drop"
    );
}

// ============================================================================
// Debug Test (run with --ignored --nocapture)
// ============================================================================

#[test]
#[ignore]
fn test_debug_dump() {
    let mut client = TestClient::new()
        .size(80, 24)
        .timeout(Duration::from_secs(10))
        .build()
        .expect("Failed to create test client");

    client.split_vertical();
    client.wait_for_update().ok();
    client.split_horizontal();
    client.wait_for_update().ok();

    std::thread::sleep(Duration::from_millis(500));
    client.drain_messages().ok();

    println!("\n{}", client.dump_screen());
    println!("\n=== Server Log (last 30 lines) ===");
    println!("{}", client.dump_server_log(30));
}
