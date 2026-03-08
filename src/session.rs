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

/// Manager for multiple sessions.
pub struct SessionManager {
    /// All sessions indexed by ID.
    sessions: HashMap<SessionId, Session>,
    /// Session lookup by name.
    name_to_id: HashMap<String, SessionId>,
    /// Next session ID to assign.
    next_id: u32,
    /// Default shell to use for new sessions.
    shell: String,
}

impl SessionManager {
    /// Create a new session manager.
    pub fn new(shell: String) -> Self {
        Self {
            sessions: HashMap::new(),
            name_to_id: HashMap::new(),
            next_id: 0,
            shell,
        }
    }

    /// Create a new session with the given name.
    /// Returns the session ID, or an error if creation failed.
    pub fn create_session(
        &mut self,
        name: Option<String>,
        cols: u16,
        rows: u16,
    ) -> Result<SessionId, SessionError> {
        let name = match name {
            Some(name) => Self::normalize_session_name(name)?,
            None => self.generate_session_name(),
        };

        // Check for name collision
        if self.name_to_id.contains_key(&name) {
            return Err(SessionError::NameExists(name));
        }

        let id = SessionId(self.next_id);
        self.next_id += 1;

        let session = Session::new(id, name.clone(), &self.shell, cols, rows)?;

        log::info!("Created session {:?} '{}' ({}x{})", id, name, cols, rows);

        self.name_to_id.insert(name, id);
        self.sessions.insert(id, session);

        Ok(id)
    }

    /// Get a session by ID.
    pub fn get(&self, id: SessionId) -> Option<&Session> {
        self.sessions.get(&id)
    }

    /// Get a mutable session by ID.
    pub fn get_mut(&mut self, id: SessionId) -> Option<&mut Session> {
        self.sessions.get_mut(&id)
    }

    /// Get a session by name.
    pub fn get_by_name(&self, name: &str) -> Option<&Session> {
        self.name_to_id
            .get(name)
            .and_then(|id| self.sessions.get(id))
    }

    /// Get a mutable session by name.
    pub fn get_by_name_mut(&mut self, name: &str) -> Option<&mut Session> {
        self.name_to_id
            .get(name)
            .copied()
            .and_then(|id| self.sessions.get_mut(&id))
    }

    /// Get the session ID for a name.
    pub fn id_for_name(&self, name: &str) -> Option<SessionId> {
        self.name_to_id.get(name).copied()
    }

    /// Close a session by ID.
    /// Returns true if the session existed and was closed.
    pub fn close_session(&mut self, id: SessionId) -> bool {
        if let Some(session) = self.sessions.remove(&id) {
            self.name_to_id.remove(&session.name);
            log::info!("Closed session {:?} '{}'", id, session.name);
            true
        } else {
            false
        }
    }

    /// Close a session by name.
    /// Returns true if the session existed and was closed.
    pub fn close_session_by_name(&mut self, name: &str) -> bool {
        if let Some(id) = self.name_to_id.remove(name) {
            self.sessions.remove(&id);
            log::info!("Closed session {:?} '{}'", id, name);
            true
        } else {
            false
        }
    }

    /// Rename a session.
    pub fn rename_session(&mut self, id: SessionId, new_name: String) -> Result<(), SessionError> {
        let new_name = Self::normalize_session_name(new_name)?;

        if let Some(session) = self.sessions.get(&id) {
            if session.name == new_name {
                return Ok(());
            }
        } else {
            return Err(SessionError::NotFound(id));
        }

        // Check for name collision
        if self.name_to_id.contains_key(&new_name) {
            return Err(SessionError::NameExists(new_name));
        }

        if let Some(session) = self.sessions.get_mut(&id) {
            let old_name = std::mem::replace(&mut session.name, new_name.clone());
            self.name_to_id.remove(&old_name);
            self.name_to_id.insert(new_name.clone(), id);
            log::info!("Renamed session {:?} '{}' -> '{}'", id, old_name, new_name);
            Ok(())
        } else {
            Err(SessionError::NotFound(id))
        }
    }

    /// Get a list of all sessions.
    pub fn list(&self) -> Vec<&Session> {
        self.sessions.values().collect()
    }

    /// Get info for all sessions (for protocol).
    pub fn list_info(&self) -> Vec<SessionInfo> {
        self.sessions.values().map(|s| s.info()).collect()
    }

    /// Get the number of sessions.
    pub fn count(&self) -> usize {
        self.sessions.len()
    }

    /// Check if there are any sessions.
    pub fn is_empty(&self) -> bool {
        self.sessions.is_empty()
    }

    /// Get or create the default session.
    pub fn get_or_create_default(
        &mut self,
        cols: u16,
        rows: u16,
    ) -> Result<SessionId, SessionError> {
        // If any session exists, attach to the oldest session ID for deterministic behavior.
        // This mimics tmux behavior: `tmux attach` attaches to an available session.
        if let Some(id) = self.sessions.keys().min_by_key(|id| id.0).copied() {
            return Ok(id);
        }
        // No sessions exist, create a new "default" session
        self.create_session(Some("default".to_string()), cols, rows)
    }

    /// Iterate over all sessions.
    pub fn iter(&self) -> impl Iterator<Item = &Session> {
        self.sessions.values()
    }

    /// Iterate mutably over all sessions.
    pub fn iter_mut(&mut self) -> impl Iterator<Item = (&SessionId, &mut Session)> {
        self.sessions.iter_mut()
    }

    /// Generate a unique session name.
    fn generate_session_name(&self) -> String {
        let mut n = 0;
        loop {
            let name = if n == 0 {
                "default".to_string()
            } else {
                format!("session-{}", n)
            };

            if !self.name_to_id.contains_key(&name) {
                return name;
            }
            n += 1;
        }
    }

    /// Normalize and validate a user-provided session name.
    pub fn normalize_session_name(name: String) -> Result<String, SessionError> {
        let normalized = name.trim().to_string();

        if normalized.is_empty() {
            return Err(SessionError::InvalidName(
                "session name cannot be empty".to_string(),
            ));
        }

        if normalized.chars().any(|c| c.is_control()) {
            return Err(SessionError::InvalidName(
                "session name cannot contain control characters".to_string(),
            ));
        }

        Ok(normalized)
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

#[cfg(test)]
mod tests {
    use super::*;

    // Helper to create a session manager for testing
    fn test_manager() -> SessionManager {
        SessionManager::new("/bin/sh".to_string())
    }

    // ------------------------------------------------------------------------
    // Session Tests
    // ------------------------------------------------------------------------

    #[test]
    fn test_session_attach_detach() {
        let mut manager = test_manager();
        let session_id = manager
            .create_session(Some("test".to_string()), 80, 24)
            .unwrap();
        let session = manager.get_mut(session_id).unwrap();

        let client1 = ClientId(1);
        let client2 = ClientId(2);

        // Initially no clients
        assert!(!session.has_clients());
        assert_eq!(session.client_count(), 0);

        // Attach first client
        assert!(session.attach_client(client1));
        assert!(session.has_clients());
        assert_eq!(session.client_count(), 1);

        // Attach same client again (should return false)
        assert!(!session.attach_client(client1));
        assert_eq!(session.client_count(), 1);

        // Attach second client
        assert!(session.attach_client(client2));
        assert_eq!(session.client_count(), 2);

        // Detach first client
        assert!(session.detach_client(client1));
        assert_eq!(session.client_count(), 1);
        assert!(session.has_clients());

        // Detach same client again (should return false)
        assert!(!session.detach_client(client1));

        // Detach second client
        assert!(session.detach_client(client2));
        assert!(!session.has_clients());
    }

    #[test]
    fn test_session_info() {
        let mut manager = test_manager();
        let session_id = manager
            .create_session(Some("my-session".to_string()), 80, 24)
            .unwrap();
        let session = manager.get(session_id).unwrap();

        let info = session.info();

        assert_eq!(info.id, session_id.0);
        assert_eq!(info.name, "my-session");
        assert_eq!(info.windows, 1); // Initial window
        assert_eq!(info.attached_clients, 0);
        assert!(info.created_at > 0);
    }

    #[test]
    fn test_session_effective_size() {
        let mut manager = test_manager();
        let session_id = manager
            .create_session(Some("test".to_string()), 100, 50)
            .unwrap();
        let session = manager.get_mut(session_id).unwrap();

        let client1 = ClientId(1);
        let client2 = ClientId(2);
        session.attach_client(client1);
        session.attach_client(client2);

        let mut client_sizes = HashMap::new();
        client_sizes.insert(client1, (120, 40));
        client_sizes.insert(client2, (80, 30));

        // Should use smallest of each dimension
        let (cols, rows) = session.effective_size(&client_sizes);
        assert_eq!(cols, 80);
        assert_eq!(rows, 30);
    }

    #[test]
    fn test_session_effective_size_no_clients() {
        let mut manager = test_manager();
        let session_id = manager
            .create_session(Some("test".to_string()), 100, 50)
            .unwrap();
        let session = manager.get(session_id).unwrap();

        let client_sizes = HashMap::new();

        // Should use current window manager size
        let (cols, rows) = session.effective_size(&client_sizes);
        assert_eq!(cols, 100);
        assert_eq!(rows, 50);
    }

    // ------------------------------------------------------------------------
    // SessionManager Tests
    // ------------------------------------------------------------------------

    #[test]
    fn test_session_manager_create() {
        let mut manager = test_manager();

        let id = manager
            .create_session(Some("work".to_string()), 80, 24)
            .unwrap();

        assert_eq!(manager.count(), 1);
        assert!(manager.get(id).is_some());
        assert!(manager.get_by_name("work").is_some());
    }

    #[test]
    fn test_session_manager_auto_name() {
        let mut manager = test_manager();

        // First session gets "default"
        let id1 = manager.create_session(None, 80, 24).unwrap();
        assert_eq!(manager.get(id1).unwrap().name, "default");

        // Second session gets "session-1"
        let id2 = manager.create_session(None, 80, 24).unwrap();
        assert_eq!(manager.get(id2).unwrap().name, "session-1");

        // Third session gets "session-2"
        let id3 = manager.create_session(None, 80, 24).unwrap();
        assert_eq!(manager.get(id3).unwrap().name, "session-2");
    }

    #[test]
    fn test_session_manager_name_collision() {
        let mut manager = test_manager();

        manager
            .create_session(Some("work".to_string()), 80, 24)
            .unwrap();

        // Try to create another with same name
        let result = manager.create_session(Some("work".to_string()), 80, 24);

        assert!(matches!(result, Err(SessionError::NameExists(_))));
    }

    #[test]
    fn test_session_manager_rejects_empty_or_control_names() {
        let mut manager = test_manager();

        let empty = manager.create_session(Some("   ".to_string()), 80, 24);
        assert!(matches!(empty, Err(SessionError::InvalidName(_))));

        let control = manager.create_session(Some("bad\nname".to_string()), 80, 24);
        assert!(matches!(control, Err(SessionError::InvalidName(_))));
    }

    #[test]
    fn test_session_manager_trims_name_on_create() {
        let mut manager = test_manager();

        let id = manager
            .create_session(Some("  work  ".to_string()), 80, 24)
            .unwrap();
        assert_eq!(manager.get(id).unwrap().name, "work");
        assert!(manager.get_by_name("work").is_some());
    }

    #[test]
    fn test_session_manager_close() {
        let mut manager = test_manager();

        let id = manager
            .create_session(Some("temp".to_string()), 80, 24)
            .unwrap();
        assert_eq!(manager.count(), 1);

        assert!(manager.close_session(id));
        assert_eq!(manager.count(), 0);
        assert!(manager.get(id).is_none());
        assert!(manager.get_by_name("temp").is_none());

        // Closing again should return false
        assert!(!manager.close_session(id));
    }

    #[test]
    fn test_session_manager_close_by_name() {
        let mut manager = test_manager();

        manager
            .create_session(Some("temp".to_string()), 80, 24)
            .unwrap();

        assert!(manager.close_session_by_name("temp"));
        assert_eq!(manager.count(), 0);

        // Closing again should return false
        assert!(!manager.close_session_by_name("temp"));
    }

    #[test]
    fn test_session_manager_rename() {
        let mut manager = test_manager();

        let id = manager
            .create_session(Some("old-name".to_string()), 80, 24)
            .unwrap();

        manager.rename_session(id, "new-name".to_string()).unwrap();

        assert!(manager.get_by_name("old-name").is_none());
        assert!(manager.get_by_name("new-name").is_some());
        assert_eq!(manager.get(id).unwrap().name, "new-name");
    }

    #[test]
    fn test_session_manager_rename_same_name_is_noop() {
        let mut manager = test_manager();

        let id = manager
            .create_session(Some("same".to_string()), 80, 24)
            .unwrap();

        assert!(manager.rename_session(id, "same".to_string()).is_ok());
        assert_eq!(manager.get(id).unwrap().name, "same");
    }

    #[test]
    fn test_session_manager_rename_trims_and_validates() {
        let mut manager = test_manager();

        let id = manager
            .create_session(Some("old".to_string()), 80, 24)
            .unwrap();

        manager
            .rename_session(id, "  new-name  ".to_string())
            .unwrap();
        assert!(manager.get_by_name("old").is_none());
        assert!(manager.get_by_name("new-name").is_some());

        let invalid = manager.rename_session(id, " \n ".to_string());
        assert!(matches!(invalid, Err(SessionError::InvalidName(_))));
    }

    #[test]
    fn test_session_manager_rename_collision() {
        let mut manager = test_manager();

        let id1 = manager
            .create_session(Some("session1".to_string()), 80, 24)
            .unwrap();
        let _id2 = manager
            .create_session(Some("session2".to_string()), 80, 24)
            .unwrap();

        let result = manager.rename_session(id1, "session2".to_string());

        assert!(matches!(result, Err(SessionError::NameExists(_))));
    }

    #[test]
    fn test_session_manager_get_or_create_default() {
        let mut manager = test_manager();

        // First call with no sessions creates the default session
        let id1 = manager.get_or_create_default(80, 24).unwrap();
        assert_eq!(manager.get(id1).unwrap().name, "default");
        assert_eq!(manager.count(), 1);

        // Second call returns the same session
        let id2 = manager.get_or_create_default(100, 50).unwrap();
        assert_eq!(id1, id2);
        assert_eq!(manager.count(), 1);
    }

    #[test]
    fn test_session_manager_get_or_create_default_attaches_to_existing() {
        let mut manager = test_manager();

        // Create a named session first
        let work_id = manager
            .create_session(Some("work".to_string()), 80, 24)
            .unwrap();
        assert_eq!(manager.count(), 1);

        // get_or_create_default should attach to existing "work" session, not create "default"
        let id = manager.get_or_create_default(80, 24).unwrap();
        assert_eq!(id, work_id);
        assert_eq!(manager.count(), 1); // No new session created
    }

    #[test]
    fn test_session_manager_get_or_create_default_picks_lowest_id() {
        let mut manager = test_manager();

        let first = manager
            .create_session(Some("first".to_string()), 80, 24)
            .unwrap();
        let second = manager
            .create_session(Some("second".to_string()), 80, 24)
            .unwrap();
        assert_ne!(first, second);

        // Remove the first to ensure IDs are sparse, then create another.
        assert!(manager.close_session(first));
        let third = manager
            .create_session(Some("third".to_string()), 80, 24)
            .unwrap();
        assert_ne!(second, third);

        // Should pick the lowest available ID among existing sessions.
        let id = manager.get_or_create_default(80, 24).unwrap();
        let expected = if second.0 < third.0 { second } else { third };
        assert_eq!(id, expected);
    }

    #[test]
    fn test_session_manager_list_info() {
        let mut manager = test_manager();

        manager
            .create_session(Some("work".to_string()), 80, 24)
            .unwrap();
        manager
            .create_session(Some("personal".to_string()), 100, 50)
            .unwrap();

        let info = manager.list_info();

        assert_eq!(info.len(), 2);

        let names: Vec<_> = info.iter().map(|i| i.name.as_str()).collect();
        assert!(names.contains(&"work"));
        assert!(names.contains(&"personal"));
    }

    #[test]
    fn test_session_manager_id_for_name() {
        let mut manager = test_manager();

        let id = manager
            .create_session(Some("test".to_string()), 80, 24)
            .unwrap();

        assert_eq!(manager.id_for_name("test"), Some(id));
        assert_eq!(manager.id_for_name("nonexistent"), None);
    }
}
