//! The attached event loop.

use crate::border::*;
use crate::events::*;
use crate::keys::*;
use crate::lifecycle::*;
use crate::messages::*;
use crate::mouse::*;
use crate::*;
use std::io::Write;
use std::time::Duration;

use crossterm::event::{self, Event};
use mio::unix::SourceFd;
use mio::{Events, Interest, Poll};

use clux::client::{Client, ScreenBuffer};
use clux::config::Config;
use clux::protocol::DetachReason;

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
    let prefix_parsed = clux::config::ParsedKey::parse(&prefix_key)
        .map_err(|e| anyhow::anyhow!("Invalid prefix key: {}", e))?;

    // Get terminal size for border
    let (term_cols, term_rows) = crossterm::terminal::size()?;
    log::info!("Terminal size: {}x{}", term_cols, term_rows);

    // Get session name for border display
    let session_name = client.session_name().unwrap_or("").to_string();
    log::info!("Session name: {}", session_name);

    let mut stdout = enter_terminal()?;

    let run_result = (|| -> anyhow::Result<Option<DetachReason>> {
        // Draw initial border
        render_border(&mut stdout, term_cols, term_rows, &session_name, "")?;
        stdout.flush()?;
        log::info!("Terminal setup complete, border drawn");

        // Set up polling
        let mut poll = Poll::new()?;
        let mut events = Events::with_capacity(128);

        // Register server connection
        let fd = client.as_raw_fd();
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
            if loop_count % 100 == 1 {}

            // Resize can interrupt mio with SIGWINCH.
            match poll.poll(&mut events, Some(timeout)) {
                Ok(()) => {}
                Err(e) if is_interrupted_io(&e) => continue,
                Err(e) => return Err(e.into()),
            }

            // Handle server messages
            for event in events.iter() {
                if event.token() == SERVER_TOKEN {
                    if !drain_server_messages(
                        client,
                        &mut stdout,
                        &mut screen_buffer,
                        term_cols,
                        &mut detach_reason,
                        &mut mouse_mode_enabled,
                    )? {
                        running = false;
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
                        if let KeyOutcome::Stop = handle_key(
                            key,
                            &mut stdout,
                            client,
                            &mut screen_buffer,
                            &config,
                            &prefix_parsed,
                            &mut command_mode,
                        )? {
                            running = false;
                        }
                    }
                    Event::Mouse(mouse) => {
                        handle_mouse(
                            mouse,
                            &mut stdout,
                            client,
                            &mut screen_buffer,
                            &config,
                            mouse_mode_enabled,
                        )?;
                    }
                    Event::Resize(cols, rows) => {
                        handle_resize(
                            cols,
                            rows,
                            &mut stdout,
                            client,
                            &mut screen_buffer,
                            &session_name,
                        )?;
                    }
                    Event::Paste(text) => {
                        handle_paste(text, client)?;
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

    print_detach_message(detach_reason, &session_name);

    Ok(())
}
