//! Pane management for split terminal views.
//!
//! Implements a tree-based layout system similar to tmux, where panes can be
//! split horizontally or vertically, and each pane contains its own terminal.

use std::os::unix::io::RawFd;

use crate::pty::{Pty, PtySize};
use crate::terminal::Terminal;

/// Unique identifier for a pane.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct PaneId(pub u32);

/// Direction for splitting a pane.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SplitDirection {
    /// Split horizontally (new pane below).
    Horizontal,
    /// Split vertically (new pane to the right).
    Vertical,
}

/// Direction for navigating between panes.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Direction {
    Up,
    Down,
    Left,
    Right,
}

/// Rectangle representing a pane's position and size.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Rect {
    pub x: u16,
    pub y: u16,
    pub width: u16,
    pub height: u16,
}

impl Rect {
    pub fn new(x: u16, y: u16, width: u16, height: u16) -> Self {
        Self {
            x,
            y,
            width,
            height,
        }
    }

    /// Split this rect horizontally, returning (top, bottom).
    pub fn split_horizontal(&self, ratio: f32) -> (Rect, Rect) {
        let top_height = ((self.height as f32) * ratio) as u16;
        let bottom_height = self.height.saturating_sub(top_height).saturating_sub(1); // -1 for border

        let top = Rect::new(self.x, self.y, self.width, top_height);
        let bottom = Rect::new(self.x, self.y + top_height + 1, self.width, bottom_height);

        (top, bottom)
    }

    /// Split this rect vertically, returning (left, right).
    pub fn split_vertical(&self, ratio: f32) -> (Rect, Rect) {
        let left_width = ((self.width as f32) * ratio) as u16;
        let right_width = self.width.saturating_sub(left_width).saturating_sub(1); // -1 for border

        let left = Rect::new(self.x, self.y, left_width, self.height);
        let right = Rect::new(self.x + left_width + 1, self.y, right_width, self.height);

        (left, right)
    }

    /// Check if a point is inside this rect.
    pub fn contains(&self, x: u16, y: u16) -> bool {
        x >= self.x && x < self.x + self.width && y >= self.y && y < self.y + self.height
    }
}

/// A single pane containing a terminal and PTY.
pub struct Pane {
    /// Unique identifier.
    pub id: PaneId,
    /// The terminal state.
    pub terminal: Terminal,
    /// The PTY connection.
    pub pty: Pty,
    /// VTE parser for this pane.
    pub parser: vte::Parser,
    /// Current position and size.
    pub rect: Rect,
    /// Whether this pane is focused.
    pub focused: bool,
    /// Last known mouse mode (for detecting changes).
    pub last_mouse_mode: u16,
}

impl Pane {
    /// Create a new pane with the given shell.
    pub fn new(id: PaneId, rect: Rect, shell: &str) -> anyhow::Result<Self> {
        log::info!(
            "Creating pane {:?} at ({}, {}) size {}x{}",
            id,
            rect.x,
            rect.y,
            rect.width,
            rect.height
        );

        let pty_size = PtySize::new(rect.height, rect.width);
        let pty = Pty::spawn(pty_size, shell)?;
        let terminal = Terminal::new(rect.height as usize, rect.width as usize);
        let parser = vte::Parser::new();

        Ok(Self {
            id,
            terminal,
            pty,
            parser,
            rect,
            focused: false,
            last_mouse_mode: 0,
        })
    }

    /// Resize the pane to a new rect.
    pub fn resize(&mut self, rect: Rect) -> anyhow::Result<()> {
        self.rect = rect;
        self.pty.resize(PtySize::new(rect.height, rect.width))?;
        self.terminal
            .resize(rect.height as usize, rect.width as usize);
        Ok(())
    }

    /// Check if the PTY is still alive.
    pub fn is_alive(&self) -> bool {
        self.pty.is_alive()
    }

    /// Get the raw file descriptor for polling.
    pub fn fd(&self) -> RawFd {
        self.pty.as_raw_fd()
    }
}

mod layout;
mod navigation;
#[cfg(test)]
mod tests;
mod tree;

pub use layout::*;
