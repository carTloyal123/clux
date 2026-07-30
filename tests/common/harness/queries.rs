//! TestClient query and wait methods.

use clux::client::ScreenBuffer;
use clux::protocol::{CommandAction, ServerMessage, WindowLayout};
use clux::selection::SelectionMode;
use std::io::{BufRead, BufReader};
use std::path::PathBuf;
use std::thread;
use std::time::{Duration, Instant};

use super::client::*;
use super::types::*;

impl TestClient {
    pub fn new() -> TestClientBuilder {
        TestClientBuilder::default()
    }

    pub fn new_window(&mut self) -> &mut Self {
        self.command(CommandAction::NewWindow)
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
