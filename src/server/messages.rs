//! Dispatching client messages.

use super::client_conn::ClientState;
use super::{Server, ServerResult};
use crate::protocol::{ClientMessage, ServerMessage, PROTOCOL_VERSION};
use crate::session::ClientId;

impl Server {
    pub(super) fn process_client_message(
        &mut self,
        client_id: ClientId,
        message: ClientMessage,
    ) -> ServerResult<()> {
        log::debug!("Client {:?} sent: {:?}", client_id, message);

        match message {
            ClientMessage::Hello {
                version,
                term_cols,
                term_rows,
                term_type: _,
            } => {
                // Store client size
                self.client_sizes.insert(client_id, (term_cols, term_rows));

                // Send HelloAck
                let response = ServerMessage::HelloAck {
                    version: PROTOCOL_VERSION,
                    server_pid: std::process::id(),
                };
                self.send_to_client(client_id, &response)?;

                // Update client state
                if let Some(client) = self.clients.get_mut(&client_id) {
                    client.state = ClientState::Ready;

                    // Check version compatibility
                    if version != PROTOCOL_VERSION {
                        log::warn!(
                            "Client {:?} version mismatch: {} vs {}",
                            client_id,
                            version,
                            PROTOCOL_VERSION
                        );
                    }
                }
            }

            ClientMessage::Attach {
                session_name,
                create,
            } => {
                self.handle_attach(client_id, session_name, create)?;
            }

            ClientMessage::Detach => {
                self.handle_detach(client_id)?;
            }

            ClientMessage::Input(bytes) => {
                self.handle_input(client_id, bytes)?;
            }

            ClientMessage::Resize { cols, rows } => {
                self.handle_resize(client_id, cols, rows)?;
            }

            ClientMessage::Scroll { lines } => {
                self.handle_scroll(client_id, lines)?;
            }

            ClientMessage::Command(action) => {
                self.handle_command(client_id, action)?;
            }

            ClientMessage::ListSessions => {
                let list = self.sessions.list_info();
                self.send_to_client(client_id, &ServerMessage::SessionList(list))?;
            }

            ClientMessage::KillSession { name } => {
                self.handle_kill_session(client_id, &name)?;
            }

            ClientMessage::RenameSession { new_name } => {
                self.handle_rename_session(client_id, new_name)?;
            }

            ClientMessage::Ping => {
                self.send_to_client(client_id, &ServerMessage::Pong)?;
            }

            ClientMessage::ShutdownServer => {
                log::info!("Shutdown requested by client {:?}", client_id);
                self.running = false;
            }
        }

        Ok(())
    }
}
