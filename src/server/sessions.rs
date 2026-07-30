//! Killing and renaming sessions.

use super::client_conn::ClientState;
use super::{Server, ServerResult};
use crate::protocol::{DetachReason, ServerMessage};
use crate::session::{ClientId, SessionManager};

impl Server {
    /// Handle kill session request.
    pub(super) fn handle_kill_session(
        &mut self,
        client_id: ClientId,
        name: &str,
    ) -> ServerResult<()> {
        let normalized_name = match SessionManager::normalize_session_name(name.to_string()) {
            Ok(name) => name,
            Err(err) => {
                self.send_to_client(
                    client_id,
                    &ServerMessage::Error {
                        message: err.to_string(),
                    },
                )?;
                return Ok(());
            }
        };

        // Find session ID by name
        let session_id = self.sessions.id_for_name(&normalized_name);

        if let Some(session_id) = session_id {
            // Notify all attached clients that session is being killed
            let detach_msg = ServerMessage::Detached {
                reason: DetachReason::SessionClosed,
            };

            // Find clients attached to this session
            let attached_clients: Vec<ClientId> = self
                .clients
                .iter()
                .filter_map(|(&cid, client)| {
                    if let ClientState::Attached(sid) = client.state {
                        if sid == session_id {
                            return Some(cid);
                        }
                    }
                    None
                })
                .collect();

            // Send detach notification and update client state
            for cid in attached_clients {
                let _ = self.send_to_client(cid, &detach_msg);
                if let Some(client) = self.clients.get_mut(&cid) {
                    client.state = ClientState::Ready;
                }
            }

            // Remove PTY tokens for this session
            self.token_to_pty.retain(|_, (sid, _)| *sid != session_id);

            // Close the session
            self.sessions.close_session_by_name(&normalized_name);
            log::info!(
                "Session '{}' killed by client {:?}",
                normalized_name,
                client_id
            );
        } else {
            self.send_to_client(
                client_id,
                &ServerMessage::Error {
                    message: format!("Session '{}' not found", normalized_name),
                },
            )?;
        }
        Ok(())
    }
    /// Handle rename session request.
    pub(super) fn handle_rename_session(
        &mut self,
        client_id: ClientId,
        new_name: String,
    ) -> ServerResult<()> {
        // Get the session this client is attached to
        let session_id = {
            let client = match self.clients.get(&client_id) {
                Some(c) => c,
                None => return Ok(()),
            };

            match client.state {
                ClientState::Attached(id) => id,
                _ => {
                    self.send_to_client(
                        client_id,
                        &ServerMessage::Error {
                            message: "Not attached to a session".to_string(),
                        },
                    )?;
                    return Ok(());
                }
            }
        };

        match self.sessions.rename_session(session_id, new_name) {
            Ok(()) => {}
            Err(e) => {
                self.send_to_client(
                    client_id,
                    &ServerMessage::Error {
                        message: e.to_string(),
                    },
                )?;
            }
        }

        Ok(())
    }
}
