//! Accepting connections and reading from them.

use std::io::ErrorKind;
use std::os::unix::io::AsRawFd;
use std::path::PathBuf;

use mio::unix::SourceFd;
use mio::{Interest, Token};

use super::client_conn::{ClientConnection, ClientState};
use super::message_type;
use super::{Server, ServerError, ServerResult, CLIENT_TOKEN_BASE};
use crate::protocol::{ProtocolError, ServerMessage};
use crate::session::ClientId;

impl Server {
    pub(super) fn accept_client(&mut self) -> ServerResult<()> {
        match self.listener.accept() {
            Ok(stream) => {
                let client_id = ClientId(self.next_client_id);
                self.next_client_id += 1;

                let token = Token(CLIENT_TOKEN_BASE + client_id.0 as usize);

                // Register the client socket for reading
                self.poll.registry().register(
                    &mut SourceFd(&stream.as_raw_fd()),
                    token,
                    Interest::READABLE,
                )?;

                let conn = ClientConnection::new(client_id, stream);
                self.clients.insert(client_id, conn);
                self.token_to_client.insert(token, client_id);

                log::info!("Accepted client {:?}", client_id);
                Ok(())
            }
            Err(e) if e.kind() == ErrorKind::WouldBlock => Ok(()),
            Err(e) => Err(ServerError::Io(e)),
        }
    }

    /// Handle an event from a client socket.
    pub(super) fn handle_client_event(&mut self, client_id: ClientId) -> ServerResult<()> {
        // Read message from client
        let message = {
            let client = match self.clients.get_mut(&client_id) {
                Some(c) => c,
                None => return Ok(()),
            };

            match client.try_read_message() {
                Ok(Some(msg)) => msg,
                Ok(None) => return Ok(()), // No complete message yet
                Err(ProtocolError::ConnectionClosed) => {
                    log::info!("Client {:?} disconnected", client_id);
                    self.remove_client(client_id);
                    return Ok(());
                }
                Err(e) => {
                    log::warn!("Client {:?} protocol error: {}", client_id, e);
                    self.remove_client(client_id);
                    return Ok(());
                }
            }
        };

        // Process the message
        self.process_client_message(client_id, message)
    }

    /// Process a message from a client.
    /// Send a message to a specific client.
    pub(super) fn send_to_client(
        &mut self,
        client_id: ClientId,
        message: &ServerMessage,
    ) -> ServerResult<()> {
        log::debug!(
            "send_to_client: {:?} -> {}",
            client_id,
            message_type(message)
        );
        if let Some(client) = self.clients.get_mut(&client_id) {
            if let Err(e) = client.send_message(message) {
                log::warn!("Failed to send to client {:?}: {}", client_id, e);
                // Don't remove client here - let cleanup_dead_clients handle it
            } else {
                log::trace!("Message sent successfully to {:?}", client_id);
            }
        } else {
            log::warn!("send_to_client: client {:?} not found", client_id);
        }
        Ok(())
    }

    /// Remove a client and clean up.
    pub(super) fn remove_client(&mut self, client_id: ClientId) {
        // Detach from any session
        if let Some(client) = self.clients.get(&client_id) {
            if let ClientState::Attached(session_id) = client.state {
                if let Some(session) = self.sessions.get_mut(session_id) {
                    session.detach_client(client_id);
                }
            }
        }

        // Remove from token mapping
        let token = Token(CLIENT_TOKEN_BASE + client_id.0 as usize);
        self.token_to_client.remove(&token);

        // Remove client size
        self.client_sizes.remove(&client_id);

        // Remove the client (socket is closed on drop)
        self.clients.remove(&client_id);

        log::info!("Removed client {:?}", client_id);
    }

    /// Clean up dead/disconnected clients.
    pub(super) fn cleanup_dead_clients(&mut self) {
        let dead_clients: Vec<ClientId> = self
            .clients
            .iter()
            .filter(|(_, client)| !client.is_alive())
            .map(|(&id, _)| id)
            .collect();

        for client_id in dead_clients {
            self.remove_client(client_id);
        }
    }

    /// Signal the server to stop.
    pub fn stop(&mut self) {
        self.running = false;
    }

    /// Check if the server is still running.
    /// Returns false if the server stopped itself (e.g., via auto-shutdown).
    pub fn is_running(&self) -> bool {
        self.running
    }

    /// Get the socket path.
    pub fn socket_path(&self) -> &PathBuf {
        &self.config.socket_path
    }

    /// Get the number of connected clients.
    pub fn client_count(&self) -> usize {
        self.clients.len()
    }

    /// Get the number of sessions.
    pub fn session_count(&self) -> usize {
        self.sessions.count()
    }
}
