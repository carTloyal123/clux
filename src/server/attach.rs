//! Attaching and detaching clients.

use super::client_conn::ClientState;
use super::{Server, ServerResult};
use crate::protocol::{DetachReason, ServerMessage};
use crate::session::{ClientId, SessionManager};

impl Server {
    pub(super) fn handle_attach(
        &mut self,
        client_id: ClientId,
        session_name: Option<String>,
        create: bool,
    ) -> ServerResult<()> {
        let (cols, rows) = self
            .client_sizes
            .get(&client_id)
            .copied()
            .unwrap_or((self.config.default_cols, self.config.default_rows));

        // Find or create the session
        let normalized_session_name = match session_name {
            Some(name) => match SessionManager::normalize_session_name(name) {
                Ok(name) => Some(name),
                Err(err) => {
                    self.send_to_client(
                        client_id,
                        &ServerMessage::Error {
                            message: err.to_string(),
                        },
                    )?;
                    return Ok(());
                }
            },
            None => None,
        };

        let (session_id, newly_created) = if let Some(ref name) = normalized_session_name {
            if let Some(id) = self.sessions.id_for_name(name) {
                (id, false)
            } else if create {
                let id = self
                    .sessions
                    .create_session(Some(name.clone()), cols, rows)?;
                (id, true)
            } else {
                self.send_to_client(
                    client_id,
                    &ServerMessage::Error {
                        message: format!("Session '{}' not found", name),
                    },
                )?;
                return Ok(());
            }
        } else {
            // Attach to default session (may create)
            let had_sessions = self.sessions.count() > 0;
            let id = self.sessions.get_or_create_default(cols, rows)?;
            (id, !had_sessions)
        };

        // Register PTYs if session was just created
        if newly_created {
            self.register_session_ptys(session_id)?;
        }

        // Attach client to session and get session name
        let session_name = if let Some(session) = self.sessions.get_mut(session_id) {
            session.attach_client(client_id);

            // Recalculate effective size (smallest-client-wins)
            let (eff_cols, eff_rows) = session.effective_size(&self.client_sizes);
            if let Err(e) = session.window_manager.resize(eff_cols, eff_rows) {
                log::warn!("Failed to resize session after attach: {}", e);
            }

            session.name.clone()
        } else {
            return Ok(());
        };

        // Update client state
        if let Some(client) = self.clients.get_mut(&client_id) {
            client.state = ClientState::Attached(session_id);
        }

        // Send attached confirmation
        self.send_to_client(
            client_id,
            &ServerMessage::Attached {
                session_id: session_id.0,
                session_name: session_name.clone(),
            },
        )?;

        // Send the initial screen: layout, then every pane's content
        if let Some(layout) = self.build_window_layout(session_id) {
            let layout_msg = ServerMessage::LayoutChanged { layout };
            self.send_to_client(client_id, &layout_msg)?;
            self.send_all_pane_updates(session_id, &[client_id])?;
        }

        log::info!(
            "Client {:?} attached to session {:?} '{}'",
            client_id,
            session_id,
            session_name
        );

        Ok(())
    }

    /// Handle client detach request.
    pub(super) fn handle_detach(&mut self, client_id: ClientId) -> ServerResult<()> {
        // Get the session this client is attached to
        let session_id = {
            let client = match self.clients.get(&client_id) {
                Some(c) => c,
                None => return Ok(()),
            };

            match client.state {
                ClientState::Attached(id) => id,
                _ => return Ok(()),
            }
        };

        // Detach from session
        if let Some(session) = self.sessions.get_mut(session_id) {
            session.detach_client(client_id);

            // Recalculate effective size after client leaves
            let (eff_cols, eff_rows) = session.effective_size(&self.client_sizes);
            if let Err(e) = session.window_manager.resize(eff_cols, eff_rows) {
                log::warn!("Failed to resize session after detach: {}", e);
            }
        }

        // Update client state
        if let Some(client) = self.clients.get_mut(&client_id) {
            client.state = ClientState::Ready;
        }

        // Remove client size
        self.client_sizes.remove(&client_id);

        // Send detached confirmation
        self.send_to_client(
            client_id,
            &ServerMessage::Detached {
                reason: DetachReason::ClientRequested,
            },
        )?;

        Ok(())
    }
}
