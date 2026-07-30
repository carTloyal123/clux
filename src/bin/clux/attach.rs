//! The attached event loop.

use crate::input::*;
use crate::render::*;
use crate::*;
use std::io::{self, Write};
use std::time::{Duration, Instant};

use crossterm::event::{self, Event, KeyModifiers, MouseButton, MouseEvent, MouseEventKind};
use crossterm::terminal::{self, disable_raw_mode, enable_raw_mode};
use mio::unix::SourceFd;
use mio::{Events, Interest, Poll};

use clux::client::{encode_mouse_sgr, Client, ScreenBuffer};
use clux::config::Config;
use clux::protocol::{CommandAction, DetachReason};
use clux::selection::SelectionMode;

/// Options for run_attached
#[derive(Default)]
pub(crate) struct RunOptions {
    /// Run only one iteration of the event loop (for debugging)
    pub(crate) once: bool,
}
/// Run the main event loop while attached to a session.
pub(crate) fn run_attached(client: &mut Client) -> anyhow::Result<()> {
    run_attached_with_options(client, RunOptions::default())
}
/// Run the main event loop while attached to a session with options.
pub(crate) fn run_attached_with_options(
    client: &mut Client,
    options: RunOptions,
) -> anyhow::Result<()> {
    log::info!("=== run_attached starting ===");

    // Load configuration for keybindings
    let (config, _) = Config::load();
    let prefix_key = config.prefix.key.clone();
    log::debug!("Prefix key configured as: {}", prefix_key);
    let prefix_parsed = clux::config::ParsedKey::parse(&prefix_key)
        .map_err(|e| anyhow::anyhow!("Invalid prefix key: {}", e))?;

    // Get terminal size for border
    let (term_cols, term_rows) = crossterm::terminal::size()?;
    log::info!("Terminal size: {}x{}", term_cols, term_rows);

    // Get session name for border display
    let session_name = client.session_name().unwrap_or("").to_string();
    log::info!("Session name: {}", session_name);

    // Set up terminal
    log::info!("Setting up terminal (raw mode, alternate screen)...");
    enable_raw_mode()?;
    let mut stdout = io::stdout();
    if let Err(err) = crossterm::execute!(
        stdout,
        terminal::EnterAlternateScreen,
        crossterm::cursor::Hide,
        crossterm::event::EnableMouseCapture,
    ) {
        let _ = disable_raw_mode();
        return Err(err.into());
    }

    let run_result = (|| -> anyhow::Result<Option<DetachReason>> {
        // Draw initial border
        render_border(&mut stdout, term_cols, term_rows, &session_name, "")?;
        stdout.flush()?;
        log::info!("Terminal setup complete, border drawn");

        // Set up polling
        log::debug!("Setting up mio poll...");
        let mut poll = Poll::new()?;
        let mut events = Events::with_capacity(128);

        // Register server connection
        let fd = client.as_raw_fd();
        log::debug!("Registering server connection fd={} with mio", fd);
        poll.registry()
            .register(&mut SourceFd(&fd), SERVER_TOKEN, Interest::READABLE)?;

        // State
        let mut running = true;
        let mut command_mode = false;
        let mut mouse_mode_enabled = false; // Track if focused pane wants mouse events
        let mut detach_reason: Option<DetachReason> = None; // Track why we detached
        let timeout = Duration::from_millis(50);
        let mut loop_count = 0u64;

        // Inner dimensions (excluding border)
        let inner_cols = term_cols.saturating_sub(2) as usize;
        let inner_rows = term_rows.saturating_sub(2) as usize;
        let mut screen_buffer = ScreenBuffer::new(inner_cols, inner_rows);

        log::info!("Entering main event loop...");

        // Main event loop
        while running {
            loop_count += 1;
            if loop_count % 100 == 1 {
                log::trace!("Event loop iteration {}", loop_count);
            }

            // Resize can interrupt mio with SIGWINCH.
            match poll.poll(&mut events, Some(timeout)) {
                Ok(()) => {}
                Err(e) if is_interrupted_io(&e) => continue,
                Err(e) => return Err(e.into()),
            }

            let event_count = events.iter().count();
            if event_count > 0 {
                log::debug!("Got {} events from poll", event_count);
            }

            // Handle server messages
            for event in events.iter() {
                log::debug!(
                    "Processing event: token={:?}, readable={}",
                    event.token(),
                    event.is_readable()
                );
                if event.token() == SERVER_TOKEN {
                    log::debug!("Server socket is readable, trying to receive messages...");
                    let frame_start = Instant::now();
                    let mut did_render = false;
                    loop {
                        match client.try_recv() {
                            Ok(Some(msg)) => {
                                log::info!("Received server message: {:?}", msg_summary(&msg));
                                match handle_server_message(msg, &mut stdout, &mut screen_buffer)? {
                                    MessageResult::Continue => {
                                        log::debug!("Message handled, continuing");
                                    }
                                    MessageResult::Detached(reason) => {
                                        log::info!("Detached: {:?}", reason);
                                        detach_reason = Some(reason);
                                        running = false;
                                        break;
                                    }
                                    MessageResult::Shutdown => {
                                        log::info!("Server shutdown");
                                        running = false;
                                        break;
                                    }
                                    MessageResult::MouseModeChanged(enabled) => {
                                        log::info!("Client mouse mode updated: {}", enabled);
                                        mouse_mode_enabled = enabled;
                                    }
                                    MessageResult::LayoutChanged(layout) => {
                                        log::info!("New layout, {} panes", layout.panes.len());
                                        screen_buffer.set_layout(layout);
                                        // Render immediately to show dividers
                                        render_screen_buffer(&mut stdout, &screen_buffer)?;
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
                                running = false;
                                break;
                            }
                        }
                    }
                    // Update frame timing if we rendered something
                    if did_render {
                        let frame_time_us = frame_start.elapsed().as_micros() as u64;
                        // Update the frame time display in the border
                        let frame_info = format!("{:.2}ms", frame_time_us as f64 / 1000.0);
                        update_frame_time(&mut stdout, term_cols, &frame_info)?;
                    }
                }
            }

            // Handle keyboard/mouse input
            loop {
                let has_input = match event::poll(Duration::ZERO) {
                    Ok(ready) => ready,
                    Err(e) if is_interrupted_io(&e) => continue,
                    Err(e) => return Err(e.into()),
                };
                if !has_input {
                    break;
                }

                let terminal_event = match event::read() {
                    Ok(ev) => ev,
                    Err(e) if is_interrupted_io(&e) => continue,
                    Err(e) => return Err(e.into()),
                };

                match terminal_event {
                    Event::Key(key) => {
                        log::debug!("Key event: {:?} modifiers={:?}", key.code, key.modifiers);

                        // Check for prefix key
                        if !command_mode && prefix_parsed.matches(key.code, key.modifiers) {
                            log::info!("Prefix key pressed, entering command mode");
                            command_mode = true;
                            continue;
                        }

                        if command_mode {
                            log::debug!("In command mode, processing key...");
                            command_mode = false;

                            // Handle command-mode key
                            if let Some(action) = key_to_command_action(&key, &config) {
                                log::info!("Command action: {:?}", action);
                                match action {
                                    InternalAction::Detach => {
                                        log::info!("Detaching...");
                                        client.detach()?;
                                        running = false;
                                    }
                                    InternalAction::Quit => {
                                        log::info!("Quitting...");
                                        client.send_command(CommandAction::Quit)?;
                                        running = false;
                                    }
                                    InternalAction::SendPrefix => {
                                        // Send the prefix key itself to the PTY
                                        if let Some(bytes) = key_to_bytes(&key) {
                                            client.send_input(bytes)?;
                                        }
                                    }
                                    InternalAction::Scroll(lines) => {
                                        client.send_scroll(lines)?;
                                    }
                                    InternalAction::Command(cmd) => {
                                        client.send_command(cmd)?;
                                    }
                                }
                            }
                        } else if let Some(bytes) = key_to_bytes(&key) {
                            // Typing replaces the selection, as in any terminal.
                            if screen_buffer.has_selection() {
                                screen_buffer.clear_selection();
                                repaint(&mut stdout, &screen_buffer)?;
                            }

                            // Send key to PTY
                            client.send_input(bytes)?;
                        }
                    }
                    Event::Mouse(mouse) => {
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
                                MouseEventKind::Drag(MouseButton::Left)
                                    | MouseEventKind::Up(MouseButton::Left)
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
                                        repaint(&mut stdout, &screen_buffer)?;
                                    }
                                    MouseEventKind::Drag(_) => {
                                        if screen_buffer.extend_selection(row, col) {
                                            repaint(&mut stdout, &screen_buffer)?;
                                        }
                                    }
                                    MouseEventKind::Up(_) => {
                                        if config.selection.copy_on_select {
                                            copy_selection(&mut stdout, &screen_buffer);
                                        }
                                    }
                                    _ => {}
                                }
                            } else if matches!(mouse.kind, MouseEventKind::Down(_)) {
                                // Clicked the border: drop any old selection.
                                screen_buffer.clear_selection();
                                repaint(&mut stdout, &screen_buffer)?;
                            }
                            continue;
                        }

                        // The wheel scrolls the pane's history, unless the
                        // application asked for the mouse - then Shift overrides.
                        let wheel = matches!(
                            mouse.kind,
                            MouseEventKind::ScrollUp | MouseEventKind::ScrollDown
                        );
                        if wheel
                            && (!mouse_mode_enabled
                                || mouse.modifiers.contains(KeyModifiers::SHIFT))
                        {
                            let lines = if matches!(mouse.kind, MouseEventKind::ScrollUp) {
                                WHEEL_LINES
                            } else {
                                -WHEEL_LINES
                            };
                            client.send_scroll(lines)?;
                            continue;
                        }

                        // Only forward mouse events if the focused pane has enabled mouse mode
                        if !mouse_mode_enabled {
                            continue;
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
                            continue;
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
                    }
                    Event::Resize(cols, rows) => {
                        log::info!("Terminal resized to {}x{}", cols, rows);

                        // Send inner dimensions to server (minus border)
                        let inner_cols = cols.saturating_sub(2);
                        let inner_rows = rows.saturating_sub(2);
                        client.send_resize(inner_cols, inner_rows)?;

                        // Resize the screen buffer; the server will send a new
                        // LayoutChanged + PaneUpdate for the new size.
                        screen_buffer.resize(inner_cols as usize, inner_rows as usize);

                        // Redraw border
                        render_border(&mut stdout, cols, rows, &session_name, "")?;
                    }
                    Event::Paste(text) => {
                        // Send bracketed paste
                        let mut bytes = Vec::new();
                        bytes.extend_from_slice(b"\x1b[200~");
                        bytes.extend_from_slice(text.as_bytes());
                        bytes.extend_from_slice(b"\x1b[201~");
                        client.send_input(bytes)?;
                    }
                    _ => {}
                }
            }

            stdout.flush()?;

            // If --once mode, exit after first iteration that received messages
            if options.once && loop_count > 0 {
                log::info!("--once mode: exiting after first iteration");
                // Wait a moment to let any rendering complete
                std::thread::sleep(Duration::from_millis(100));
                break;
            }
        }

        Ok(detach_reason)
    })();

    let cleanup_result = restore_terminal(&mut stdout);
    let detach_reason = match run_result {
        Ok(reason) => reason,
        Err(err) => {
            let _ = cleanup_result;
            return Err(err);
        }
    };

    cleanup_result?;

    // Print detach message if we were detached
    if let Some(reason) = detach_reason {
        match reason {
            DetachReason::ClientRequested => {
                if !session_name.is_empty() {
                    println!("[detached (from session {})]", session_name);
                } else {
                    println!("[detached]");
                }
            }
            DetachReason::SessionClosed => {
                println!("[exited]");
            }
            DetachReason::ServerShutdown => {
                println!("[server shutting down]");
            }
            DetachReason::Replaced => {
                println!("[detached (replaced by another client)]");
            }
        }
    }

    Ok(())
}
