//! PTY readability, and registering PTYs with the poll loop.

use mio::Token;

use super::client_conn::ClientState;
use super::{rows_with_links, Server, ServerResult};
use crate::protocol::{CursorState, PaneRow, ServerMessage};
use crate::session::ClientId;

impl Server {
    pub(super) fn handle_pty_event(&mut self, token: Token) -> ServerResult<()> {
        log::trace!("handle_pty_event: token={:?}", token);

        // Look up which session/pane this PTY belongs to
        let (session_id, pane_id) = match self.token_to_pty.get(&token) {
            Some(&ids) => ids,
            None => {
                log::warn!("No session/pane found for PTY token {:?}", token);
                return Ok(());
            }
        };

        log::trace!("PTY event for session={:?}, pane={:?}", session_id, pane_id);

        // Collect all data we need while holding mutable session borrow
        // Then drop the borrow before sending messages
        struct PtyEventData {
            mouse_mode_changed: Option<bool>,
            pane_update: Option<ServerMessage>,
        }

        let detect_plain_urls = self.config.detect_plain_urls;

        let event_data = {
            let mut buf = [0u8; 4096];
            let session = match self.sessions.get_mut(session_id) {
                Some(s) => s,
                None => {
                    log::warn!("Session {:?} not found for PTY event", session_id);
                    return Ok(());
                }
            };

            let focused_pane_id = session.window_manager.focused_pane_id();

            // Find the pane and read from its PTY
            let pane = match session.window_manager.find_pane_mut(pane_id) {
                Some(p) => p,
                None => {
                    log::warn!("Pane {:?} not found in session {:?}", pane_id, session_id);
                    return Ok(());
                }
            };

            // Get pane's screen position before reading
            let pane_rect = pane.rect;

            // Read available data (non-blocking - returns 0 if no data)
            let bytes_read = match pane.pty.read(&mut buf) {
                Ok(n) if n > 0 => n,
                Ok(_) => return Ok(()), // No data available
                Err(e) => {
                    log::warn!("PTY read error for {:?}/{:?}: {}", session_id, pane_id, e);
                    return Ok(());
                }
            };

            log::debug!(
                "PTY read {} bytes from session {:?} pane {:?}",
                bytes_read,
                session_id,
                pane_id
            );

            // Feed bytes through the terminal emulator (VTE parser)
            pane.parser.advance(&mut pane.terminal, &buf[..bytes_read]);

            // Check if mouse mode changed for the focused pane
            let current_mouse_mode = pane.terminal.mouse_mode();
            let mouse_mode_changed =
                if pane_id == focused_pane_id && current_mouse_mode != pane.last_mouse_mode {
                    log::info!(
                        "Mouse mode changed: {} -> {} for pane {:?}",
                        pane.last_mouse_mode,
                        current_mouse_mode,
                        pane_id
                    );
                    pane.last_mouse_mode = current_mouse_mode;
                    Some(current_mouse_mode != 0)
                } else {
                    None
                };

            // Get dirty rows from the terminal's grid
            let dirty_rows = pane.terminal.take_dirty_rows();

            if dirty_rows.is_empty() {
                log::trace!("No dirty rows after PTY read");
                return Ok(());
            }

            log::debug!(
                "PTY event: {} dirty rows for pane at ({}, {})",
                dirty_rows.len(),
                pane_rect.x,
                pane_rect.y
            );

            // Get cursor state from terminal
            let cursor = pane.terminal.cursor();

            // Cursor state in pane-local coordinates
            let pane_cursor = if pane_id == focused_pane_id {
                Some(CursorState {
                    row: cursor.row as u16,
                    col: cursor.col as u16,
                    visible: cursor.visible,
                })
            } else {
                None
            };

            // Resolve hyperlinks for the dirty rows. This can pull in extra rows:
            // when a link wraps, every row it covers has to be repainted or the
            // host terminal keeps the fragment it was given before the line grew.
            let mut links = pane
                .terminal
                .resolve_links(pane_id.0, detect_plain_urls, &dirty_rows);
            let rows_to_send = rows_with_links(&dirty_rows, &links);

            // Pane-local rows (cells without screen offset)
            let changed_rows: Vec<PaneRow> = rows_to_send
                .iter()
                .map(|&row_idx| {
                    let view = pane.terminal.view_row(row_idx);
                    let row_links = links
                        .remove(&row_idx)
                        .unwrap_or_default()
                        .into_iter()
                        .map(Into::into)
                        .collect();
                    PaneRow::with_links(row_idx, view.cells, row_links).wrapped(view.wrapped)
                })
                .collect();

            let pane_update = ServerMessage::PaneUpdate {
                pane_id: pane_id.0,
                changed_rows,
                cursor: pane_cursor,
            };

            PtyEventData {
                mouse_mode_changed,
                pane_update: Some(pane_update),
            }
        }; // session borrow ends here

        // Broadcast mouse mode change first if it changed
        if let Some(enabled) = event_data.mouse_mode_changed {
            let mouse_msg = ServerMessage::MouseMode { enabled };
            self.broadcast_to_session(session_id, &mouse_msg)?;
        }

        if let Some(ref pane_update) = event_data.pane_update {
            let attached: Vec<ClientId> = self
                .clients
                .iter()
                .filter(|(_, client)| client.state == ClientState::Attached(session_id))
                .map(|(&id, _)| id)
                .collect();

            for client_id in attached {
                self.send_to_client(client_id, pane_update)?;
            }
        }

        Ok(())
    }
}
