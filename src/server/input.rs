//! Input, scrolling, and resize.

use super::client_conn::ClientState;
use super::{Server, ServerResult};
use crate::protocol::{CursorState, PaneRow, ServerMessage};
use crate::session::{ClientId, SessionId};

impl Server {
    /// Handle input from a client.
    pub(super) fn handle_input(&mut self, client_id: ClientId, bytes: Vec<u8>) -> ServerResult<()> {
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

        // Write to the focused pane's PTY
        let mut left_scrollback = false;
        if let Some(session) = self.sessions.get_mut(session_id) {
            if let Some(pane) = session.window_manager.focused_pane_mut() {
                // Typing returns to the live view, as in any terminal.
                left_scrollback = pane.terminal.reset_scroll();

                if let Err(e) = pane.pty.write_all(&bytes) {
                    log::warn!("Failed to write to PTY: {}", e);
                }
            }
        }

        if left_scrollback {
            self.send_focused_pane_update(session_id)?;
        }

        Ok(())
    }

    /// Scroll the focused pane through its scrollback.
    ///
    /// `lines` is positive to go back in history, negative to come forward, zero
    /// to jump back to the live view.
    pub(super) fn handle_scroll(&mut self, client_id: ClientId, lines: i32) -> ServerResult<()> {
        let session_id = match self.clients.get(&client_id).map(|c| c.state) {
            Some(ClientState::Attached(id)) => id,
            _ => return Ok(()),
        };

        let moved = match self.sessions.get_mut(session_id) {
            Some(session) => match session.window_manager.focused_pane_mut() {
                Some(pane) => {
                    if lines == 0 {
                        pane.terminal.reset_scroll()
                    } else {
                        pane.terminal.scroll_view(lines)
                    }
                }
                None => false,
            },
            None => false,
        };

        if moved {
            self.send_focused_pane_update(session_id)?;
        }

        Ok(())
    }

    /// Resend the focused pane to every attached client.
    ///
    /// Used when the view moved rather than the content: the whole pane is dirty,
    /// and the cursor is hidden while looking at history.
    pub(super) fn send_focused_pane_update(&mut self, session_id: SessionId) -> ServerResult<()> {
        let detect_plain_urls = self.config.detect_plain_urls;

        let update = {
            let Some(session) = self.sessions.get_mut(session_id) else {
                return Ok(());
            };
            let Some(pane) = session.window_manager.focused_pane_mut() else {
                return Ok(());
            };

            let rows_to_send = pane.terminal.take_dirty_rows();
            if rows_to_send.is_empty() {
                return Ok(());
            }

            let mut links =
                pane.terminal
                    .resolve_links(pane.id.0, detect_plain_urls, &rows_to_send);

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

            let cursor = pane.terminal.cursor();
            ServerMessage::PaneUpdate {
                pane_id: pane.id.0,
                changed_rows,
                cursor: Some(CursorState {
                    row: cursor.row as u16,
                    col: cursor.col as u16,
                    // No cursor while viewing history: it belongs to the live view.
                    visible: cursor.visible && !pane.terminal.is_scrolled(),
                }),
            }
        };

        self.broadcast_to_session(session_id, &update)
    }

    /// Handle terminal resize from a client.
    pub(super) fn handle_resize(
        &mut self,
        client_id: ClientId,
        cols: u16,
        rows: u16,
    ) -> ServerResult<()> {
        log::info!(
            "handle_resize: client={:?}, size={}x{}",
            client_id,
            cols,
            rows
        );

        // Update stored client size
        self.client_sizes.insert(client_id, (cols, rows));

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

        // Recalculate effective size and resize session
        if let Some(session) = self.sessions.get_mut(session_id) {
            let (eff_cols, eff_rows) = session.effective_size(&self.client_sizes);
            log::info!(
                "Resizing session {:?} to {}x{}",
                session_id,
                eff_cols,
                eff_rows
            );
            if let Err(e) = session.window_manager.resize(eff_cols, eff_rows) {
                log::warn!("Failed to resize session: {}", e);
            }
        }

        // Send full screen refresh to all clients after resize
        self.broadcast_full_screen(session_id)?;

        Ok(())
    }
}
