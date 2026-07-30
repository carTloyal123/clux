//! Resize and paste handling for the attached loop.

use std::io;

use clux::client::{Client, ScreenBuffer};

use super::border::render_border;

/// A terminal resize: tell the server, resize the buffer, redraw the border.
pub(crate) fn handle_resize(
    cols: u16,
    rows: u16,
    stdout: &mut io::Stdout,
    client: &mut Client,
    screen_buffer: &mut ScreenBuffer,
    session_name: &str,
) -> anyhow::Result<()> {
    log::info!("Terminal resized to {}x{}", cols, rows);

    // Send inner dimensions to server (minus border).
    let inner_cols = cols.saturating_sub(2);
    let inner_rows = rows.saturating_sub(2);
    client.send_resize(inner_cols, inner_rows)?;

    // The server will send a new LayoutChanged + PaneUpdate for the new size.
    screen_buffer.resize(inner_cols as usize, inner_rows as usize);

    render_border(stdout, cols, rows, session_name, "")?;
    Ok(())
}

/// A bracketed paste: wrap the text and forward it to the pty.
pub(crate) fn handle_paste(text: String, client: &mut Client) -> anyhow::Result<()> {
    let mut bytes = Vec::new();
    bytes.extend_from_slice(b"\x1b[200~");
    bytes.extend_from_slice(text.as_bytes());
    bytes.extend_from_slice(b"\x1b[201~");
    client.send_input(bytes)?;
    Ok(())
}
