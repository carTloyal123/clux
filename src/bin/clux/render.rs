//! Painting the host terminal, and handling server messages.

use std::io::{self, Write};

use crossterm::{cursor::MoveTo, queue};

use clux::client::{ScreenBuffer, BEGIN_SYNC_UPDATE, END_SYNC_UPDATE};
use clux::protocol::{DetachReason, ServerMessage, WindowLayout};

/// Result of handling a server message.
pub(crate) enum MessageResult {
    Continue,
    Detached(DetachReason),
    Shutdown,
    MouseModeChanged(bool),
    LayoutChanged(WindowLayout),
    PaneUpdated,
}
/// Get a summary of a server message for logging.
pub(crate) fn msg_summary(msg: &ServerMessage) -> String {
    match msg {
        ServerMessage::HelloAck {
            version,
            server_pid,
        } => {
            format!("HelloAck(version={}, pid={})", version, server_pid)
        }
        ServerMessage::Attached {
            session_id,
            session_name,
        } => {
            format!("Attached(session={}, name={})", session_id, session_name)
        }
        ServerMessage::Detached { reason } => {
            format!("Detached(reason={:?})", reason)
        }
        ServerMessage::SessionList(sessions) => {
            format!("SessionList(count={})", sessions.len())
        }
        ServerMessage::Error { message } => {
            format!("Error({})", message)
        }
        ServerMessage::Pong => "Pong".to_string(),
        ServerMessage::Shutdown => "Shutdown".to_string(),
        ServerMessage::MouseMode { enabled } => format!("MouseMode(enabled={})", enabled),
        ServerMessage::LayoutChanged { layout } => {
            format!("LayoutChanged(panes={})", layout.panes.len())
        }
        ServerMessage::PaneUpdate {
            pane_id,
            changed_rows,
            cursor,
        } => {
            format!(
                "PaneUpdate(pane={}, rows={}, cursor={:?})",
                pane_id,
                changed_rows.len(),
                cursor.as_ref().map(|c| (c.row, c.col))
            )
        }
    }
}
/// Handle a message from the server.
/// Content is rendered with offset (1, 1) to account for the border; pane
/// updates are composited into the screen_buffer.
pub(crate) fn handle_server_message(
    msg: ServerMessage,
    stdout: &mut io::Stdout,
    screen_buffer: &mut ScreenBuffer,
) -> anyhow::Result<MessageResult> {
    log::debug!("handle_server_message: {}", msg_summary(&msg));

    // Content is rendered inside the border, offset by 1 in each direction
    const X_OFFSET: u16 = 1;
    const Y_OFFSET: u16 = 1;

    match msg {
        ServerMessage::LayoutChanged { layout } => {
            log::info!(
                "Layout changed: {} panes, screen {}x{}",
                layout.panes.len(),
                layout.screen_cols,
                layout.screen_rows
            );
            Ok(MessageResult::LayoutChanged(layout))
        }
        ServerMessage::PaneUpdate {
            pane_id,
            changed_rows,
            cursor,
        } => {
            log::debug!(
                "PaneUpdate: pane={}, rows={}, cursor={:?}",
                pane_id,
                changed_rows.len(),
                cursor.as_ref().map(|c| (c.row, c.col))
            );

            // Apply update to screen buffer
            screen_buffer.apply_pane_update(pane_id, &changed_rows);

            // Present the whole update as one frame, so a repaint spanning
            // several rows is never shown half-drawn.
            write!(stdout, "{}", BEGIN_SYNC_UPDATE)?;

            // Render the changed rows from the screen buffer
            for pane_row in &changed_rows {
                // Find pane position in layout to compute screen row
                if let Some(layout) = screen_buffer.layout() {
                    if let Some(pane) = layout.panes.iter().find(|p| p.pane_id == pane_id) {
                        let screen_row = pane.y + pane_row.row_idx;

                        // Get the full row from screen buffer and render it
                        // (render_row_ansi also emits OSC 8 hyperlinks).
                        if screen_buffer.get_row(screen_row as usize).is_some() {
                            queue!(stdout, MoveTo(X_OFFSET, Y_OFFSET + screen_row))?;
                            let ansi = screen_buffer.render_row_ansi(screen_row as usize);
                            write!(stdout, "{}", ansi)?;
                        }
                    }
                }
            }

            write!(stdout, "{}", END_SYNC_UPDATE)?;

            // Store cursor position if provided (for focused pane)
            // We don't position it immediately because subsequent pane updates might
            // move the terminal cursor while rendering their rows. The cursor will be
            // positioned after all messages are processed.
            if let Some(c) = cursor {
                // Cursor is in pane-local coordinates, need to translate to screen
                if let Some(layout) = screen_buffer.layout() {
                    if let Some(pane) = layout.panes.iter().find(|p| p.pane_id == pane_id) {
                        let cursor_col = X_OFFSET + pane.x + c.col;
                        let cursor_row = Y_OFFSET + pane.y + c.row;
                        screen_buffer.set_cursor(cursor_row, cursor_col, c.visible);
                    }
                }
            }

            Ok(MessageResult::PaneUpdated)
        }
        ServerMessage::Detached { reason } => Ok(MessageResult::Detached(reason)),
        ServerMessage::Shutdown => Ok(MessageResult::Shutdown),
        ServerMessage::Error { message } => {
            log::error!("Server error: {}", message);
            Ok(MessageResult::Continue)
        }
        ServerMessage::MouseMode { enabled } => {
            log::info!("Mouse mode changed: enabled={}", enabled);
            Ok(MessageResult::MouseModeChanged(enabled))
        }
        _ => {
            // Ignore other messages
            Ok(MessageResult::Continue)
        }
    }
}
