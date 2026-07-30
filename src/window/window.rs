//! A window: a named pane layout, one of several a session can hold.

use crate::pane::PaneManager;

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
}
