//! Finding, closing and renaming sessions.

use super::{Session, SessionError, SessionId};
impl super::SessionManager {
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
}
