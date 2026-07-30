//! Keyboard handling for the attached loop: prefix, command mode, and typing.

use std::io;

use clux::client::{Client, ScreenBuffer};
use clux::config::Config;
use clux::protocol::CommandAction;
use clux::ParsedKey;
use crossterm::event::KeyEvent;

use super::border::repaint;
use super::input::{key_to_bytes, key_to_command_action, InternalAction};

/// Whether the attached loop should keep running after a key.
pub(crate) enum KeyOutcome {
    Continue,
    Stop,
}

/// Handle one key event.
pub(crate) fn handle_key(
    key: KeyEvent,
    stdout: &mut io::Stdout,
    client: &mut Client,
    screen_buffer: &mut ScreenBuffer,
    config: &Config,
    prefix_parsed: &ParsedKey,
    command_mode: &mut bool,
) -> anyhow::Result<KeyOutcome> {
    let mut outcome = KeyOutcome::Continue;

    log::debug!("Key event: {:?} modifiers={:?}", key.code, key.modifiers);

    // Check for prefix key
    if !*command_mode && prefix_parsed.matches(key.code, key.modifiers) {
        log::info!("Prefix key pressed, entering command mode");
        *command_mode = true;
        return Ok(KeyOutcome::Continue);
    }

    if *command_mode {
        log::debug!("In command mode, processing key...");
        *command_mode = false;

        // Handle command-mode key
        if let Some(action) = key_to_command_action(&key, &config) {
            log::info!("Command action: {:?}", action);
            match action {
                InternalAction::Detach => {
                    log::info!("Detaching...");
                    client.detach()?;
                    outcome = KeyOutcome::Stop;
                }
                InternalAction::Quit => {
                    log::info!("Quitting...");
                    client.send_command(CommandAction::Quit)?;
                    outcome = KeyOutcome::Stop;
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
            repaint(stdout, &screen_buffer)?;
        }

        // Send key to PTY
        client.send_input(bytes)?;
    }

    Ok(outcome)
}
