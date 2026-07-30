//! Draining server messages and painting each frame.

use std::io::{self, Write};
use std::time::Instant;

use clux::client::ScreenBuffer;
use clux::protocol::DetachReason;

use super::border::{render_screen_buffer, update_frame_time};
use super::render::{handle_server_message, msg_summary, MessageResult};
use clux::client::Client;

/// Drain all pending server messages. Returns whether the loop should keep
/// running.
pub(crate) fn drain_server_messages(
    client: &mut Client,
    stdout: &mut io::Stdout,
    screen_buffer: &mut ScreenBuffer,
    term_cols: u16,
    detach_reason: &mut Option<DetachReason>,
    mouse_mode_enabled: &mut bool,
) -> anyhow::Result<bool> {
    log::debug!("Server socket is readable, trying to receive messages...");
    let frame_start = Instant::now();
    let mut did_render = false;
    loop {
        match client.try_recv() {
            Ok(Some(msg)) => {
                log::info!("Received server message: {:?}", msg_summary(&msg));
                match handle_server_message(msg, stdout, screen_buffer)? {
                    MessageResult::Continue => {
                        log::debug!("Message handled, continuing");
                    }
                    MessageResult::Detached(reason) => {
                        log::info!("Detached: {:?}", reason);
                        *detach_reason = Some(reason);
                        return Ok(false);
                    }
                    MessageResult::Shutdown => {
                        log::info!("Server shutdown");
                        return Ok(false);
                    }
                    MessageResult::MouseModeChanged(enabled) => {
                        log::info!("Client mouse mode updated: {}", enabled);
                        *mouse_mode_enabled = enabled;
                    }
                    MessageResult::LayoutChanged(layout) => {
                        log::info!("New layout, {} panes", layout.panes.len());
                        screen_buffer.set_layout(layout);
                        // Render immediately to show dividers
                        render_screen_buffer(stdout, &screen_buffer)?;
                        did_render = true;
                    }
                    MessageResult::PaneUpdated => {
                        // Screen buffer already updated, flush and position cursor
                        stdout.flush()?;
                        // Position cursor after all rendering is done
                        let cursor = screen_buffer.cursor();
                        if cursor.visible {
                            crossterm::execute!(
                                stdout,
                                crossterm::cursor::MoveTo(cursor.col, cursor.row),
                                crossterm::cursor::Show,
                            )?;
                        }
                        did_render = true;
                    }
                }
            }
            Ok(None) => {
                log::trace!("No more messages available");
                break;
            }
            Err(e) => {
                log::error!("Error receiving from server: {}", e);
                return Ok(false);
            }
        }
    }
    // Update frame timing if we rendered something
    if did_render {
        let frame_time_us = frame_start.elapsed().as_micros() as u64;
        // Update the frame time display in the border
        let frame_info = format!("{:.2}ms", frame_time_us as f64 / 1000.0);
        update_frame_time(stdout, term_cols, &frame_info)?;
    }

    Ok(true)
}
