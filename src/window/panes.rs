//! Pane operations on the active window.
//!
//! [`WindowManager`] owns the windows; panes live in each window's `PaneManager`.
//! These methods forward to the active window so callers do not have to reach
//! through two layers for the common case.

use super::WindowManager;
use crate::pane::{Direction, Pane, PaneId, SplitDirection};
use crate::window::WindowId;

impl WindowManager {
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

    /// Get the focused pane in the active window mutably.
    pub fn focused_pane_mut(&mut self) -> Option<&mut Pane> {
        self.active_window_mut().pane_manager.focused_pane_mut()
    }

    /// Get the focused pane ID in the active window.
    pub fn focused_pane_id(&self) -> PaneId {
        self.active_window().pane_manager.focused_id()
    }

    /// Get total pane count across all windows.
    pub fn total_pane_count(&self) -> usize {
        self.windows
            .iter()
            .map(|w| w.pane_manager.pane_count())
            .sum()
    }

    /// Get all panes across all windows.
    pub fn all_panes(&self) -> Vec<&Pane> {
        self.windows
            .iter()
            .flat_map(|w| w.pane_manager.all_panes())
            .collect()
    }

    /// Find a pane by ID across all windows.
    pub fn find_pane_mut(&mut self, pane_id: PaneId) -> Option<&mut Pane> {
        for window in &mut self.windows {
            if let Some(pane) = window.pane_manager.find_pane_mut(pane_id) {
                return Some(pane);
            }
        }
        None
    }
}
