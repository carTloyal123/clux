//! Creating, finding and closing sessions.

use std::collections::HashMap;

use super::{Session, SessionError, SessionId};

use crate::protocol::SessionInfo;

/// Manager for multiple sessions.
pub struct SessionManager {
    /// All sessions indexed by ID.
    pub(super) sessions: HashMap<SessionId, Session>,
    /// Session lookup by name.
    pub(super) name_to_id: HashMap<String, SessionId>,
    /// Next session ID to assign.
    pub(super) next_id: u32,
    /// Default shell to use for new sessions.
    pub(super) shell: String,
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
