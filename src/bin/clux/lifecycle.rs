//! Entering and leaving the attached terminal, and the detach message.

use std::io;

use crossterm::terminal::{self, disable_raw_mode, enable_raw_mode};

use clux::protocol::DetachReason;

/// Print the "[detached]" / "[exited]" message after the loop ends.
pub(crate) fn print_detach_message(detach_reason: Option<DetachReason>, session_name: &str) {
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
}

/// Enter raw mode and the alternate screen, returning stdout.
pub(crate) fn enter_terminal() -> anyhow::Result<io::Stdout> {
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
    Ok(stdout)
}
