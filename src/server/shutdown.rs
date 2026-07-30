//! Reaping dead panes, and session-driven shutdown.

use std::time::Instant;

use super::Server;
use crate::pane::PaneId;
use crate::protocol::{DetachReason, ServerMessage};
use crate::session::{ClientId, SessionId};

impl Server {
    /// Check for and clean up dead panes across all sessions.
    pub(super) fn cleanup_dead_panes(&mut self) {
        // Collect dead panes from all sessions
        let mut dead_panes: Vec<(SessionId, PaneId)> = Vec::new();

        for (session_id, session) in self.sessions.iter_mut() {
            for (_, pane_id) in session.window_manager.check_dead_panes() {
                dead_panes.push((*session_id, pane_id));
            }
        }

        if dead_panes.is_empty() {
            return;
        }

        // Track which sessions need screen refresh
        let mut sessions_to_refresh: Vec<SessionId> = Vec::new();
        // Track sessions to close (last pane died)
        let mut sessions_to_close: Vec<SessionId> = Vec::new();

        // Close dead panes and deregister their PTYs
        for (session_id, pane_id) in dead_panes {
            // First deregister the PTY
            self.deregister_pane_pty(session_id, pane_id);

            // Then close the pane in the window manager
            if let Some(session) = self.sessions.get_mut(session_id) {
                // Check if this is the last pane in the entire session
                let total_panes = session.window_manager.total_pane_count();

                if total_panes == 1 {
                    // This is the last pane in the session - close the session
                    log::info!(
                        "Last pane {:?} in session {:?} died, closing session",
                        pane_id,
                        session_id
                    );
                    if !sessions_to_close.contains(&session_id) {
                        sessions_to_close.push(session_id);
                    }
                } else if session.window_manager.close_pane(pane_id) {
                    log::info!("Closed dead pane {:?} in session {:?}", pane_id, session_id);
                    if !sessions_to_refresh.contains(&session_id) {
                        sessions_to_refresh.push(session_id);
                    }
                }
            }
        }

        // Close sessions where the last pane died
        for session_id in sessions_to_close {
            // Notify all attached clients
            let attached_clients: Vec<ClientId> = self
                .sessions
                .get(session_id)
                .map(|s| s.attached_clients().to_vec())
                .unwrap_or_default();

            for client_id in attached_clients {
                let _ = self.send_to_client(
                    client_id,
                    &ServerMessage::Detached {
                        reason: DetachReason::SessionClosed,
                    },
                );
            }

            // Clean up PTY mappings for this session
            self.token_to_pty.retain(|_, (sid, _)| *sid != session_id);

            // Close the session
            self.sessions.close_session(session_id);
            log::info!("Session {:?} closed (last pane exited)", session_id);
        }

        // Send screen refresh to affected sessions
        for session_id in sessions_to_refresh {
            if let Err(e) = self.broadcast_full_screen(session_id) {
                log::warn!(
                    "Failed to refresh screen after pane cleanup for session {:?}: {}",
                    session_id,
                    e
                );
            }
        }
    }
    /// Check auto-shutdown conditions and stop the server if met.
    ///
    /// This implements session-driven server lifetime:
    /// - Server shuts down when all sessions are closed (after grace period)
    /// - Server shuts down if no session is created within first_session_timeout
    pub(super) fn check_auto_shutdown(&mut self) {
        if !self.auto_shutdown.enabled {
            return;
        }

        let now = Instant::now();
        let has_sessions = self.sessions.count() > 0;

        if has_sessions {
            // Sessions exist - cancel any pending shutdown and mark that we've had sessions
            self.shutdown_pending_since = None;
            self.session_ever_created = true;
            return;
        }

        // No sessions exist - check shutdown conditions

        // Case 1: First-session timeout (server started but no session ever created)
        if !self.session_ever_created {
            let elapsed = now.duration_since(self.started_at);
            if elapsed >= self.auto_shutdown.first_session_timeout {
                log::info!(
                    "No session created after {:?}, shutting down (first-session timeout)",
                    elapsed
                );
                self.running = false;
                return;
            }
            // Still waiting for first session
            return;
        }

        // Case 2: Grace period after last session closed
        match self.shutdown_pending_since {
            Some(pending_since) => {
                let elapsed = now.duration_since(pending_since);
                if elapsed >= self.auto_shutdown.grace_period {
                    log::info!(
                        "Last session closed {:?} ago, shutting down (grace period expired)",
                        elapsed
                    );
                    self.running = false;
                }
                // Still within grace period
            }
            None => {
                // Start the grace period countdown
                log::info!(
                    "All sessions closed, starting {:?} grace period before shutdown",
                    self.auto_shutdown.grace_period
                );
                self.shutdown_pending_since = Some(now);
            }
        }
    }
}
