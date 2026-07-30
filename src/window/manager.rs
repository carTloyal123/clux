//! Window management for tab-like functionality.
//!
//! Windows are z-stacked pane layouts, similar to browser tabs: each holds its own
//! [`PaneManager`]. Keybindings live in the client (`<prefix> n/x/]/'/1-9`).

use super::{Window, WindowId};

/// Manages multiple windows (z-stacked terminal layouts).
pub struct WindowManager {
    /// All windows in order.
    pub(super) windows: Vec<Window>,
    /// Index of the active window.
    pub(super) active: usize,
    /// Next window ID to assign.
    pub(super) next_id: u32,
    /// Next global pane ID to assign (unique across all windows).
    pub(super) next_pane_id: u32,
    /// Screen width (inner, excluding border).
    pub(super) width: u16,
    /// Screen height (inner, excluding border).
    pub(super) height: u16,
    /// Shell to use for new panes.
    shell: String,
}

impl WindowManager {
    /// Create a new window manager with a single window.
    pub fn new(width: u16, height: u16, shell: &str) -> anyhow::Result<Self> {
        let mut wm = Self {
            windows: Vec::new(),
            active: 0,
            next_id: 0,
            next_pane_id: 0,
            width,
            height,
            shell: shell.to_string(),
        };
        wm.create_window()?;
        Ok(wm)
    }

    /// Create a new window and make it active.
    pub fn create_window(&mut self) -> anyhow::Result<WindowId> {
        let id = WindowId(self.next_id);
        self.next_id += 1;
        let pane_id = self.next_pane_id;
        self.next_pane_id += 1;
        let window = Window::new_with_pane_id(id, self.width, self.height, &self.shell, pane_id)?;
        self.windows.push(window);
        self.active = self.windows.len() - 1;
        log::info!(
            "Created window {:?} with pane {}, now have {} windows",
            id,
            pane_id,
            self.windows.len()
        );
        Ok(id)
    }

    /// Close a window by ID.
    /// Returns true if closed, false if it was the last window.
    pub fn close_window(&mut self, id: WindowId) -> bool {
        if self.windows.len() <= 1 {
            return false; // Don't close the last window
        }

        if let Some(idx) = self.windows.iter().position(|w| w.id == id) {
            self.windows.remove(idx);
            // Adjust active index if needed
            if self.active >= self.windows.len() {
                self.active = self.windows.len() - 1;
            } else if self.active > idx {
                self.active -= 1;
            }
            log::info!(
                "Closed window {:?}, {} windows remaining",
                id,
                self.windows.len()
            );
            true
        } else {
            false
        }
    }

    /// Close the active window.
    /// Returns the ID of the closed window, or None if it was the last window.
    pub fn close_active_window(&mut self) -> Option<WindowId> {
        if self.windows.len() <= 1 {
            return None;
        }
        let id = self.windows[self.active].id;
        if self.close_window(id) {
            Some(id)
        } else {
            None
        }
    }

    /// Switch to the next window.
    pub fn next_window(&mut self) {
        if self.windows.len() > 1 {
            self.active = (self.active + 1) % self.windows.len();
            log::debug!("Switched to window {}", self.active);
        }
    }

    /// Switch to the previous window.
    pub fn prev_window(&mut self) {
        if self.windows.len() > 1 {
            self.active = if self.active == 0 {
                self.windows.len() - 1
            } else {
                self.active - 1
            };
            log::debug!("Switched to window {}", self.active);
        }
    }

    /// Select a window by index (0-based).
    pub fn select_window(&mut self, index: usize) {
        if index < self.windows.len() {
            self.active = index;
            log::debug!("Selected window {}", self.active);
        }
    }

    /// Get the active window.
    pub fn active_window(&self) -> &Window {
        &self.windows[self.active]
    }

    /// Get the active window mutably.
    pub fn active_window_mut(&mut self) -> &mut Window {
        &mut self.windows[self.active]
    }

    /// Get all windows.
    pub fn windows(&self) -> &[Window] {
        &self.windows
    }

    /// Get the number of windows.
    pub fn window_count(&self) -> usize {
        self.windows.len()
    }

    /// Get the current width (columns).
    pub fn cols(&self) -> u16 {
        self.width
    }

    /// Get the current height (rows).
    pub fn rows(&self) -> u16 {
        self.height
    }

    /// Resize all windows to new dimensions.
    pub fn resize(&mut self, width: u16, height: u16) -> anyhow::Result<()> {
        self.width = width;
        self.height = height;
        for window in &mut self.windows {
            window.pane_manager.resize_screen(width, height)?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // Note: These tests don't spawn actual PTYs, so we test the data structure logic only

    #[test]
    fn test_window_id() {
        let id1 = WindowId(0);
        let id2 = WindowId(1);
        let id3 = WindowId(0);

        assert_ne!(id1, id2);
        assert_eq!(id1, id3);
    }

    #[test]
    fn test_window_manager_navigation() {
        // This test would require mocking PaneManager since it spawns PTYs
        // For now, we just test the ID logic
        let id1 = WindowId(0);
        let id2 = WindowId(1);

        assert_ne!(id1, id2);
    }
}
