//! Mouse handling: selection gestures, wheel scrolling, and forwarding.

use std::io;

use crossterm::event::{KeyModifiers, MouseButton, MouseEvent, MouseEventKind};

use clux::client::{encode_mouse_sgr, Client, ScreenBuffer};
use clux::config::Config;
use clux::selection::SelectionMode;

use super::border::{copy_selection, inner_position, repaint};
use super::WHEEL_LINES;

/// Handle one mouse event: select, scroll, or forward to the application.
pub(crate) fn handle_mouse(
    mouse: MouseEvent,
    stdout: &mut io::Stdout,
    client: &mut Client,
    screen_buffer: &mut ScreenBuffer,
    config: &Config,
    mouse_mode_enabled: bool,
) -> anyhow::Result<()> {
    // Left-button gestures select text, unless the focused
    // application has grabbed the mouse - then Shift overrides
    // it, the same way xterm and Ghostty do. A drag already in
    // progress keeps selecting even if Shift is released.
    let left_gesture = matches!(
        mouse.kind,
        MouseEventKind::Down(MouseButton::Left)
            | MouseEventKind::Drag(MouseButton::Left)
            | MouseEventKind::Up(MouseButton::Left)
    );
    let continuing_drag = screen_buffer.has_selection()
        && matches!(
            mouse.kind,
            MouseEventKind::Drag(MouseButton::Left) | MouseEventKind::Up(MouseButton::Left)
        );
    let selecting = left_gesture
        && (!mouse_mode_enabled
            || mouse.modifiers.contains(KeyModifiers::SHIFT)
            || continuing_drag);

    if selecting {
        if let Some((row, col)) = inner_position(&mouse) {
            match mouse.kind {
                MouseEventKind::Down(_) => {
                    // Alt+drag selects a rectangle.
                    let mode = if mouse.modifiers.contains(KeyModifiers::ALT) {
                        SelectionMode::Block
                    } else {
                        SelectionMode::Normal
                    };
                    screen_buffer.begin_selection(row, col, mode);
                    repaint(stdout, &screen_buffer)?;
                }
                MouseEventKind::Drag(_) => {
                    if screen_buffer.extend_selection(row, col) {
                        repaint(stdout, &screen_buffer)?;
                    }
                }
                MouseEventKind::Up(_) => {
                    if config.selection.copy_on_select {
                        copy_selection(stdout, &screen_buffer);
                    }
                }
                _ => {}
            }
        } else if matches!(mouse.kind, MouseEventKind::Down(_)) {
            // Clicked the border: drop any old selection.
            screen_buffer.clear_selection();
            repaint(stdout, &screen_buffer)?;
        }
        return Ok(());
    }

    // The wheel scrolls the pane's history, unless the
    // application asked for the mouse - then Shift overrides.
    let wheel = matches!(
        mouse.kind,
        MouseEventKind::ScrollUp | MouseEventKind::ScrollDown
    );
    if wheel && (!mouse_mode_enabled || mouse.modifiers.contains(KeyModifiers::SHIFT)) {
        let lines = if matches!(mouse.kind, MouseEventKind::ScrollUp) {
            WHEEL_LINES
        } else {
            -WHEEL_LINES
        };
        client.send_scroll(lines)?;
        return Ok(());
    }

    // Only forward mouse events if the focused pane has enabled mouse mode
    if !mouse_mode_enabled {
        return Ok(());
    }

    // Only forward button press/release and scroll events
    // Motion events (Moved, Drag) require mode 1002/1003
    let dominated_event = matches!(
        mouse.kind,
        MouseEventKind::Down(_)
            | MouseEventKind::Up(_)
            | MouseEventKind::ScrollUp
            | MouseEventKind::ScrollDown
            | MouseEventKind::ScrollLeft
            | MouseEventKind::ScrollRight
    );

    if !dominated_event {
        // Skip motion events for now
        return Ok(());
    }

    // Determine if this is a press or release event
    // Up events use 'm' suffix, all others use 'M' suffix
    let is_press = !matches!(mouse.kind, MouseEventKind::Up(_));

    // Adjust coordinates for border (subtract 1 for inner area)
    // The border is 1 cell wide on each side
    let adjusted = MouseEvent {
        kind: mouse.kind,
        column: mouse.column.saturating_sub(1),
        row: mouse.row.saturating_sub(1),
        modifiers: mouse.modifiers,
    };

    // Encode as SGR mouse protocol and send to server
    let bytes = encode_mouse_sgr(&adjusted, is_press);
    client.send_input(bytes)?;

    Ok(())
}
