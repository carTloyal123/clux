//! Session management for clux server.
//!
//! A session wraps a WindowManager and tracks attached clients.
//! Sessions persist when clients detach and can be reattached.

use std::collections::HashMap;
use std::time::{Instant, SystemTime, UNIX_EPOCH};

use crate::protocol::SessionInfo;
use crate::window::WindowManager;

/// Unique identifier for a session.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct SessionId(pub u32);

/// Unique identifier for a connected client.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct ClientId(pub u32);

/// A terminal session containing windows, panes, and tracking attached clients.
pub struct Session {
    /// Unique session identifier.
    pub id: SessionId,
    /// Human-readable session name.
    pub name: String,
    /// The window manager containing all windows and panes.
    pub window_manager: WindowManager,
    /// When the session was created.
    pub created_at: Instant,
    /// Unix timestamp of creation (for serialization).
    created_timestamp: u64,
    /// IDs of currently attached clients.
    attached_clients: Vec<ClientId>,
}

impl Session {
    /// Create a new session with the given name and shell.
    pub fn new(
        id: SessionId,
        name: String,
        shell: &str,
        cols: u16,
        rows: u16,
    ) -> Result<Self, SessionError> {
        let window_manager = WindowManager::new(cols, rows, shell)
            .map_err(|e| SessionError::WindowManager(e.to_string()))?;

        let created_timestamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0);

        Ok(Self {
            id,
            name,
            window_manager,
            created_at: Instant::now(),
            created_timestamp,
            attached_clients: Vec::new(),
        })
    }

    /// Attach a client to this session.
    /// Returns true if this is a new attachment, false if already attached.
    pub fn attach_client(&mut self, client_id: ClientId) -> bool {
        if self.attached_clients.contains(&client_id) {
            false
        } else {
            self.attached_clients.push(client_id);
            log::info!(
                "Client {:?} attached to session {:?} '{}' ({} clients now)",
                client_id,
                self.id,
                self.name,
                self.attached_clients.len()
            );
            true
        }
    }

    /// Detach a client from this session.
    /// Returns true if the client was attached, false otherwise.
    pub fn detach_client(&mut self, client_id: ClientId) -> bool {
        if let Some(pos) = self.attached_clients.iter().position(|&id| id == client_id) {
            self.attached_clients.remove(pos);
            log::info!(
                "Client {:?} detached from session {:?} '{}' ({} clients remaining)",
                client_id,
                self.id,
                self.name,
                self.attached_clients.len()
            );
            true
        } else {
            false
        }
    }

    /// Get the list of attached client IDs.
    pub fn attached_clients(&self) -> &[ClientId] {
        &self.attached_clients
    }

    /// Check if any clients are attached.
    pub fn has_clients(&self) -> bool {
        !self.attached_clients.is_empty()
    }

    /// Get the number of attached clients.
    pub fn client_count(&self) -> usize {
        self.attached_clients.len()
    }

    /// Get the number of windows in this session.
    pub fn window_count(&self) -> usize {
        self.window_manager.window_count()
    }

    /// Get session info for protocol messages.
    pub fn info(&self) -> SessionInfo {
        SessionInfo {
            id: self.id.0,
            name: self.name.clone(),
            created_at: self.created_timestamp,
            windows: self.window_count(),
            attached_clients: self.client_count(),
        }
    }

    /// Calculate the effective terminal size for this session.
    /// When multiple clients are attached, use the smallest dimensions.
    pub fn effective_size(&self, client_sizes: &HashMap<ClientId, (u16, u16)>) -> (u16, u16) {
        let mut min_cols = u16::MAX;
        let mut min_rows = u16::MAX;

        for client_id in &self.attached_clients {
            if let Some(&(cols, rows)) = client_sizes.get(client_id) {
                min_cols = min_cols.min(cols);
                min_rows = min_rows.min(rows);
            }
        }

        if min_cols == u16::MAX || min_rows == u16::MAX {
            // No clients or no size info, use current size
            (self.window_manager.cols(), self.window_manager.rows())
        } else {
            (min_cols, min_rows)
        }
    }
}

/// Errors that can occur during session operations.
#[derive(Debug, thiserror::Error)]
pub enum SessionError {
    #[error("Session name already exists: {0}")]
    NameExists(String),

    #[error("Session not found: {0:?}")]
    NotFound(SessionId),

    #[error("Invalid session name: {0}")]
    InvalidName(String),

    #[error("Window manager error: {0}")]
    WindowManager(String),
}

// ============================================================================
// Tests
// ============================================================================

mod lookup;
mod manager;
#[cfg(test)]
mod manager_tests;
#[cfg(test)]
mod rename_tests;
#[cfg(test)]
mod tests;

pub use manager::*;
