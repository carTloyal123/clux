//! Painting the screen buffer, the border, and the frame-time readout.

use crate::*;
use std::io::{self, Write};

use crossterm::event::MouseEvent;
use crossterm::{
    cursor::MoveTo,
    queue,
    style::{Color, Print, ResetColor, SetForegroundColor},
};

use clux::client::{ScreenBuffer, BEGIN_SYNC_UPDATE, END_SYNC_UPDATE};
use clux::clipboard;

/// Render the entire screen buffer to stdout.
/// Used after layout changes.
pub(crate) fn render_screen_buffer(
    stdout: &mut io::Stdout,
    screen_buffer: &ScreenBuffer,
) -> anyhow::Result<()> {
    const X_OFFSET: u16 = 1;
    const Y_OFFSET: u16 = 1;

    let (_screen_cols, screen_rows) = screen_buffer.dimensions();

    // One frame for the whole screen: a full repaint is the most visible place
    // for tearing.
    write!(stdout, "{}", BEGIN_SYNC_UPDATE)?;

    for row_idx in 0..screen_rows {
        if screen_buffer.get_row(row_idx).is_some() {
            queue!(stdout, MoveTo(X_OFFSET, Y_OFFSET + row_idx as u16))?;
            let ansi = screen_buffer.render_row_ansi(row_idx);
            write!(stdout, "{}", ansi)?;
        }
    }

    write!(stdout, "{}", END_SYNC_UPDATE)?;

    stdout.flush()?;
    Ok(())
}
/// Render the clux border around the terminal.
pub(crate) fn render_border(
    stdout: &mut io::Stdout,
    cols: u16,
    rows: u16,
    session_name: &str,
    window_info: &str,
) -> io::Result<()> {
    queue!(stdout, SetForegroundColor(BORDER_COLOR))?;

    // Top border with corners
    queue!(stdout, MoveTo(0, 0), Print("╭"))?;

    // Build top border content with window info
    let top_content = if !window_info.is_empty() {
        format!(" {} ", window_info)
    } else {
        String::new()
    };
    let top_chars: Vec<char> = top_content.chars().collect();

    for x in 1..cols.saturating_sub(1) {
        let idx = (x - 1) as usize;
        if idx < top_chars.len() {
            queue!(stdout, Print(top_chars[idx]))?;
        } else {
            queue!(stdout, Print("─"))?;
        }
    }
    queue!(stdout, Print("╮"))?;

    // Side borders
    for row in 1..rows.saturating_sub(1) {
        queue!(stdout, MoveTo(0, row), Print("│"))?;
        queue!(stdout, MoveTo(cols.saturating_sub(1), row), Print("│"))?;
    }

    // Bottom border with corners and "clux" label + session name
    let bottom_row = rows.saturating_sub(1);
    queue!(stdout, MoveTo(0, bottom_row), Print("╰"))?;

    // Build label with session name
    let label = if !session_name.is_empty() {
        format!(" clux:{} ", session_name)
    } else {
        " clux ".to_string()
    };
    let label_chars: Vec<char> = label.chars().collect();
    let label_len = label_chars.len() as u16;
    let border_width = cols.saturating_sub(2);
    let label_start = (border_width.saturating_sub(label_len)) / 2;

    for x in 1..cols.saturating_sub(1) {
        let pos = x - 1;
        if pos >= label_start && pos < label_start + label_len {
            let label_idx = (pos - label_start) as usize;
            queue!(stdout, Print(label_chars[label_idx]))?;
        } else {
            queue!(stdout, Print("─"))?;
        }
    }
    queue!(stdout, Print("╯"))?;

    queue!(stdout, ResetColor)?;
    Ok(())
}
/// Update just the frame time display in the top-right corner of the border.
pub(crate) fn update_frame_time(
    stdout: &mut io::Stdout,
    cols: u16,
    frame_info: &str,
) -> io::Result<()> {
    // Format: " 0.00ms " in top-right corner
    let display = format!(" {} ", frame_info);
    let display_len = display.len() as u16;
    let x_pos = cols.saturating_sub(display_len + 1); // +1 for the corner

    queue!(stdout, crossterm::cursor::SavePosition)?;
    queue!(stdout, SetForegroundColor(Color::DarkGrey))?;
    queue!(stdout, MoveTo(x_pos, 0))?;
    queue!(stdout, Print(&display))?;
    queue!(stdout, ResetColor)?;
    queue!(stdout, crossterm::cursor::RestorePosition)?;
    stdout.flush()?;
    Ok(())
}
/// Translate a mouse event to a position inside the border, or `None` if it
/// landed on the border itself.
pub(crate) fn inner_position(mouse: &MouseEvent) -> Option<(usize, usize)> {
    const X_OFFSET: u16 = 1;
    const Y_OFFSET: u16 = 1;

    let row = mouse.row.checked_sub(Y_OFFSET)?;
    let col = mouse.column.checked_sub(X_OFFSET)?;

    Some((row as usize, col as usize))
}
/// Repaint the screen and put the cursor back where the focused pane wants it.
pub(crate) fn repaint(stdout: &mut io::Stdout, screen_buffer: &ScreenBuffer) -> anyhow::Result<()> {
    render_screen_buffer(stdout, screen_buffer)?;

    let cursor = screen_buffer.cursor();
    if cursor.visible {
        crossterm::execute!(
            stdout,
            crossterm::cursor::MoveTo(cursor.col, cursor.row),
            crossterm::cursor::Show,
        )?;
    }

    Ok(())
}
/// Copy the current selection to the host terminal's clipboard.
///
/// Best-effort: a terminal that refuses clipboard writes should not take the
/// session down with it.
pub(crate) fn copy_selection(stdout: &mut io::Stdout, screen_buffer: &ScreenBuffer) {
    let Some(text) = screen_buffer.selected_text() else {
        return;
    };
    if text.is_empty() {
        return;
    }

    match clipboard::copy_to_host(stdout, &text) {
        Ok(()) => log::info!("Copied {} bytes to the host clipboard", text.len()),
        Err(e) => log::warn!("Clipboard copy failed: {}", e),
    }
}
