//! Window management for tab-like functionality.
//!
//! Windows provide z-stacking of terminal layouts, similar to browser tabs.
//! Each window has its own pane layout tree (managed by PaneManager).
//!
//! ## Clux Window Keybindings
//!
//! Press `Option+C` (or `Alt+C` on Linux) to enter command mode, then:
//!
//! - `n` - New window
//! - `x` - Close current window
//! - `]` - Next window
//! - `'` - Previous window
//! - `1-9` - Select window 1-9
//! - `0` - Select window 10
//! - `,` - Rename window (future)

use std::os::unix::io::RawFd;

use crate::pane::{Direction, PaneId, PaneManager, SplitDirection};

/// Unique identifier for a window.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct WindowId(pub u32);

/// A window containing a pane layout.
pub struct Window {
    /// Unique identifier.
    pub id: WindowId,
    /// Display name for the window.
    pub name: String,
    /// The pane manager for this window's layout.
    pub pane_manager: PaneManager,
}

impl Window {
    /// Create a new window with a single pane (uses pane ID 0).
    #[allow(dead_code)]
    pub fn new(id: WindowId, width: u16, height: u16, shell: &str) -> anyhow::Result<Self> {
        Self::new_with_pane_id(id, width, height, shell, 0)
    }

    /// Create a new window with a single pane using the specified pane ID.
    pub fn new_with_pane_id(
        id: WindowId,
        width: u16,
        height: u16,
        shell: &str,
        pane_id: u32,
    ) -> anyhow::Result<Self> {
        Ok(Self {
            id,
            name: format!("{}", id.0 + 1), // 1-indexed display name
            pane_manager: PaneManager::new_with_pane_id(width, height, shell, pane_id)?,
        })
    }

    /// Rename the window.
    #[allow(dead_code)]
    pub fn rename(&mut self, name: String) {
        self.name = name;
    }

    /// Get all PTY file descriptors in this window.
    #[allow(dead_code)]
    pub fn pty_fds(&self) -> Vec<(PaneId, RawFd)> {
        self.pane_manager.panes().map(|p| (p.id, p.fd())).collect()
    }
}

/// Manages multiple windows (z-stacked terminal layouts).
pub struct WindowManager {
    /// All windows in order.
    windows: Vec<Window>,
    /// Index of the active window.
    active: usize,
    /// Next window ID to assign.
    next_id: u32,
    /// Next global pane ID to assign (unique across all windows).
    next_pane_id: u32,
    /// Screen width (inner, excluding border).
    width: u16,
    /// Screen height (inner, excluding border).
    height: u16,
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

    /// Select a window by ID.
    #[allow(dead_code)]
    pub fn select_window_by_id(&mut self, id: WindowId) {
        if let Some(idx) = self.windows.iter().position(|w| w.id == id) {
            self.active = idx;
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

    /// Get a window by ID.
    pub fn get_window(&self, id: WindowId) -> Option<&Window> {
        self.windows.iter().find(|w| w.id == id)
    }

    /// Get a window mutably by ID.
    pub fn get_window_mut(&mut self, id: WindowId) -> Option<&mut Window> {
        self.windows.iter_mut().find(|w| w.id == id)
    }

    /// Get all windows.
    pub fn windows(&self) -> &[Window] {
        &self.windows
    }

    /// Get the active window index.
    pub fn active_index(&self) -> usize {
        self.active
    }

    /// Get the active window ID.
    pub fn active_id(&self) -> WindowId {
        self.windows[self.active].id
    }

    /// Get the number of windows.
    #[allow(dead_code)]
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

    /// Get all PTY file descriptors across all windows.
    /// Returns (WindowId, PaneId, RawFd) tuples.
    #[allow(dead_code)]
    pub fn all_pty_fds(&self) -> Vec<(WindowId, PaneId, RawFd)> {
        let mut fds = Vec::new();
        for window in &self.windows {
            for (pane_id, fd) in window.pty_fds() {
                fds.push((window.id, pane_id, fd));
            }
        }
        fds
    }

    /// Find a window and pane by PTY file descriptor.
    #[allow(dead_code)]
    pub fn find_by_fd(&self, fd: RawFd) -> Option<(WindowId, PaneId)> {
        for window in &self.windows {
            for pane in window.pane_manager.panes() {
                if pane.fd() == fd {
                    return Some((window.id, pane.id));
                }
            }
        }
        None
    }

    /// Check for dead panes across all windows.
    /// Returns (WindowId, PaneId) pairs for dead panes.
    pub fn check_dead_panes(&mut self) -> Vec<(WindowId, PaneId)> {
        let mut dead = Vec::new();
        for window in &mut self.windows {
            for pane_id in window.pane_manager.check_dead_panes() {
                dead.push((window.id, pane_id));
            }
        }
        dead
    }

    /// Close a specific pane by ID.
    /// Returns true if the pane was closed, false if it was the last pane in its window.
    pub fn close_pane(&mut self, pane_id: PaneId) -> bool {
        for window in &mut self.windows {
            if window.pane_manager.has_pane(pane_id) {
                return window.pane_manager.close_pane(pane_id).is_some();
            }
        }
        false
    }

    /// Get the pane count for a specific pane's window.
    pub fn pane_count_for(&self, pane_id: PaneId) -> Option<usize> {
        for window in &self.windows {
            if window.pane_manager.has_pane(pane_id) {
                return Some(window.pane_manager.pane_count());
            }
        }
        None
    }

    // Delegation methods for active window's pane manager

    /// Split the focused pane in the active window.
    pub fn split(&mut self, direction: SplitDirection) -> anyhow::Result<PaneId> {
        let pane_id = self.next_pane_id;
        self.next_pane_id += 1;
        self.active_window_mut()
            .pane_manager
            .split_with_id(direction, pane_id)
    }

    /// Close the focused pane in the active window.
    pub fn close_focused_pane(&mut self) -> Option<PaneId> {
        self.active_window_mut().pane_manager.close_focused()
    }

    /// Navigate to adjacent pane in the active window.
    pub fn navigate_pane(&mut self, direction: Direction) {
        self.active_window_mut().pane_manager.navigate(direction);
    }

    /// Get the focused pane in the active window.
    pub fn focused_pane(&self) -> Option<&crate::pane::Pane> {
        self.active_window().pane_manager.focused_pane()
    }

    /// Get the focused pane in the active window mutably.
    pub fn focused_pane_mut(&mut self) -> Option<&mut crate::pane::Pane> {
        self.active_window_mut().pane_manager.focused_pane_mut()
    }

    /// Get the focused pane ID in the active window.
    pub fn focused_pane_id(&self) -> PaneId {
        self.active_window().pane_manager.focused_id()
    }

    /// Get pane count in the active window.
    pub fn active_pane_count(&self) -> usize {
        self.active_window().pane_manager.pane_count()
    }

    /// Get total pane count across all windows.
    pub fn total_pane_count(&self) -> usize {
        self.windows
            .iter()
            .map(|w| w.pane_manager.pane_count())
            .sum()
    }

    /// Get all panes across all windows.
    pub fn all_panes(&self) -> Vec<&crate::pane::Pane> {
        self.windows
            .iter()
            .flat_map(|w| w.pane_manager.all_panes())
            .collect()
    }

    /// Find a pane by ID across all windows.
    pub fn find_pane_mut(&mut self, pane_id: PaneId) -> Option<&mut crate::pane::Pane> {
        for window in &mut self.windows {
            if let Some(pane) = window.pane_manager.find_pane_mut(pane_id) {
                return Some(pane);
            }
        }
        None
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
