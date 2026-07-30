//! Sending updates to attached clients.

use super::client_conn::ClientState;
use super::{Server, ServerResult};
use crate::protocol::{CursorState, PaneLayout, PaneRow, ServerMessage, WindowLayout};
use crate::session::{ClientId, SessionId};

impl Server {
    pub(super) fn broadcast_to_session(
        &mut self,
        session_id: SessionId,
        message: &ServerMessage,
    ) -> ServerResult<()> {
        // Collect client IDs attached to this session
        let client_ids: Vec<ClientId> = self
            .clients
            .iter()
            .filter_map(|(&id, client)| {
                if let ClientState::Attached(sid) = client.state {
                    if sid == session_id {
                        return Some(id);
                    }
                }
                None
            })
            .collect();

        // Send to each client
        for client_id in client_ids {
            self.send_to_client(client_id, message)?;
        }

        Ok(())
    }

    /// Broadcast a full screen update to all clients attached to a session.
    pub(super) fn broadcast_full_screen(&mut self, session_id: SessionId) -> ServerResult<()> {
        let clients: Vec<ClientId> = self
            .clients
            .iter()
            .filter(|(_, client)| client.state == ClientState::Attached(session_id))
            .map(|(&id, _)| id)
            .collect();

        if clients.is_empty() {
            return Ok(());
        }

        if let Some(layout) = self.build_window_layout(session_id) {
            let layout_msg = ServerMessage::LayoutChanged { layout };
            for &client_id in &clients {
                self.send_to_client(client_id, &layout_msg)?;
            }

            self.send_all_pane_updates(session_id, &clients)?;
        }

        Ok(())
    }

    /// Build a WindowLayout from the current session state.
    pub(super) fn build_window_layout(&self, session_id: SessionId) -> Option<WindowLayout> {
        let session = self.sessions.get(session_id)?;
        let wm = &session.window_manager;
        let active_window = wm.active_window();
        let focused_pane_id = wm.focused_pane_id();

        let panes: Vec<PaneLayout> = active_window
            .pane_manager
            .all_panes()
            .iter()
            .map(|pane| PaneLayout {
                pane_id: pane.id.0,
                x: pane.rect.x,
                y: pane.rect.y,
                width: pane.rect.width,
                height: pane.rect.height,
                focused: pane.id == focused_pane_id,
            })
            .collect();

        Some(WindowLayout {
            panes,
            screen_cols: wm.cols(),
            screen_rows: wm.rows(),
        })
    }

    /// Send full pane content updates to the given clients.
    pub(super) fn send_all_pane_updates(
        &mut self,
        session_id: SessionId,
        client_ids: &[ClientId],
    ) -> ServerResult<()> {
        let detect_plain_urls = self.config.detect_plain_urls;

        // Collect all pane updates first while holding immutable session borrow
        let updates: Vec<ServerMessage> = {
            let session = match self.sessions.get(session_id) {
                Some(s) => s,
                None => return Ok(()),
            };

            let wm = &session.window_manager;
            let focused_pane_id = wm.focused_pane_id();
            let active_window = wm.active_window();

            active_window
                .pane_manager
                .all_panes()
                .iter()
                .map(|pane| {
                    let term_rows = pane.terminal.rows();
                    let rows_to_send = std::cmp::min(pane.rect.height as usize, term_rows);
                    let all_rows: Vec<u16> = (0..rows_to_send as u16).collect();

                    let mut links =
                        pane.terminal
                            .resolve_links(pane.id.0, detect_plain_urls, &all_rows);

                    let changed_rows: Vec<PaneRow> = all_rows
                        .iter()
                        .map(|&row_idx| {
                            let view = pane.terminal.view_row(row_idx);
                            let row_links = links
                                .remove(&row_idx)
                                .unwrap_or_default()
                                .into_iter()
                                .map(Into::into)
                                .collect();
                            PaneRow::with_links(row_idx, view.cells, row_links)
                                .wrapped(view.wrapped)
                        })
                        .collect();

                    // Only include cursor for focused pane
                    let cursor = if pane.id == focused_pane_id {
                        let c = pane.terminal.cursor();
                        Some(CursorState {
                            row: c.row as u16,
                            col: c.col as u16,
                            visible: c.visible,
                        })
                    } else {
                        None
                    };

                    ServerMessage::PaneUpdate {
                        pane_id: pane.id.0,
                        changed_rows,
                        cursor,
                    }
                })
                .collect()
        }; // session borrow ends here

        // Now send all updates to all clients
        for update in &updates {
            for &client_id in client_ids {
                self.send_to_client(client_id, update)?;
            }
        }

        Ok(())
    }
}
