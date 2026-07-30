//! Keyboard handling.

use crate::*;

use crossterm::event::{self, KeyCode, KeyModifiers};

use clux::config::Config;
use clux::protocol::{CommandAction, Direction};

/// Internal action result.
#[derive(Debug)]
pub(crate) enum InternalAction {
    Detach,
    Quit,
    SendPrefix,
    /// Scroll the focused pane: positive back in history, 0 returns to live.
    Scroll(i32),
    Command(CommandAction),
}
/// Convert a command-mode key to an action.
pub(crate) fn key_to_command_action(
    key: &event::KeyEvent,
    config: &Config,
) -> Option<InternalAction> {
    let key_char = match key.code {
        KeyCode::Char(c) => Some(c),
        KeyCode::PageUp => return Some(InternalAction::Scroll(PAGE_LINES)),
        KeyCode::PageDown => return Some(InternalAction::Scroll(-PAGE_LINES)),
        // Back to the live view.
        KeyCode::End => return Some(InternalAction::Scroll(0)),
        KeyCode::Up => {
            return Some(InternalAction::Command(CommandAction::NavigatePane(
                Direction::Up,
            )))
        }
        KeyCode::Down => {
            return Some(InternalAction::Command(CommandAction::NavigatePane(
                Direction::Down,
            )))
        }
        KeyCode::Left => {
            return Some(InternalAction::Command(CommandAction::NavigatePane(
                Direction::Left,
            )))
        }
        KeyCode::Right => {
            return Some(InternalAction::Command(CommandAction::NavigatePane(
                Direction::Right,
            )))
        }
        _ => None,
    };

    let c = key_char?;

    // Check app bindings FIRST (detach, quit, send_prefix take priority)
    if c.to_string() == config.keybindings.app.detach {
        return Some(InternalAction::Detach);
    }
    if c.to_string() == config.keybindings.app.quit {
        return Some(InternalAction::Quit);
    }
    if c.to_string() == config.keybindings.app.send_prefix {
        return Some(InternalAction::SendPrefix);
    }

    // Check pane bindings
    if c.to_string() == config.keybindings.pane.split_horizontal {
        return Some(InternalAction::Command(CommandAction::SplitHorizontal));
    }
    if c.to_string() == config.keybindings.pane.split_vertical {
        return Some(InternalAction::Command(CommandAction::SplitVertical));
    }
    if c.to_string() == config.keybindings.pane.close {
        return Some(InternalAction::Command(CommandAction::ClosePane));
    }
    if c.to_string() == config.keybindings.pane.navigate_up {
        return Some(InternalAction::Command(CommandAction::NavigatePane(
            Direction::Up,
        )));
    }
    if c.to_string() == config.keybindings.pane.navigate_down {
        return Some(InternalAction::Command(CommandAction::NavigatePane(
            Direction::Down,
        )));
    }
    if c.to_string() == config.keybindings.pane.navigate_left {
        return Some(InternalAction::Command(CommandAction::NavigatePane(
            Direction::Left,
        )));
    }
    if c.to_string() == config.keybindings.pane.navigate_right {
        return Some(InternalAction::Command(CommandAction::NavigatePane(
            Direction::Right,
        )));
    }

    // Check window bindings
    if c.to_string() == config.keybindings.window.new {
        return Some(InternalAction::Command(CommandAction::NewWindow));
    }
    if c.to_string() == config.keybindings.window.close {
        return Some(InternalAction::Command(CommandAction::CloseWindow));
    }
    if c.to_string() == config.keybindings.window.next {
        return Some(InternalAction::Command(CommandAction::NextWindow));
    }
    if c.to_string() == config.keybindings.window.previous {
        return Some(InternalAction::Command(CommandAction::PrevWindow));
    }

    // Check window selection (1-9, 0)
    if let Some(n) = c.to_digit(10) {
        let index = if n == 0 { 9 } else { (n - 1) as usize };
        return Some(InternalAction::Command(CommandAction::SelectWindow(index)));
    }

    None
}
/// Convert a key event to bytes to send to PTY.
pub(crate) fn key_to_bytes(key: &event::KeyEvent) -> Option<Vec<u8>> {
    let mut bytes = Vec::new();

    match key.code {
        KeyCode::Char(c) => {
            if key.modifiers.contains(KeyModifiers::CONTROL) {
                // Ctrl+letter sends 1-26
                if c.is_ascii_alphabetic() {
                    let ctrl_char = (c.to_ascii_uppercase() as u8) - b'A' + 1;
                    bytes.push(ctrl_char);
                }
            } else if key.modifiers.contains(KeyModifiers::ALT) {
                // Alt sends ESC prefix
                bytes.push(0x1b);
                bytes.push(c as u8);
            } else {
                let mut buf = [0u8; 4];
                let s = c.encode_utf8(&mut buf);
                bytes.extend_from_slice(s.as_bytes());
            }
        }
        KeyCode::Enter => bytes.push(b'\r'),
        KeyCode::Tab => bytes.push(b'\t'),
        KeyCode::Backspace => bytes.push(0x7f),
        KeyCode::Esc => bytes.push(0x1b),
        KeyCode::Up => bytes.extend_from_slice(b"\x1b[A"),
        KeyCode::Down => bytes.extend_from_slice(b"\x1b[B"),
        KeyCode::Right => bytes.extend_from_slice(b"\x1b[C"),
        KeyCode::Left => bytes.extend_from_slice(b"\x1b[D"),
        KeyCode::Home => bytes.extend_from_slice(b"\x1b[H"),
        KeyCode::End => bytes.extend_from_slice(b"\x1b[F"),
        KeyCode::PageUp => bytes.extend_from_slice(b"\x1b[5~"),
        KeyCode::PageDown => bytes.extend_from_slice(b"\x1b[6~"),
        KeyCode::Insert => bytes.extend_from_slice(b"\x1b[2~"),
        KeyCode::Delete => bytes.extend_from_slice(b"\x1b[3~"),
        KeyCode::F(n) => {
            let seq = match n {
                1 => b"\x1bOP".as_slice(),
                2 => b"\x1bOQ",
                3 => b"\x1bOR",
                4 => b"\x1bOS",
                5 => b"\x1b[15~",
                6 => b"\x1b[17~",
                7 => b"\x1b[18~",
                8 => b"\x1b[19~",
                9 => b"\x1b[20~",
                10 => b"\x1b[21~",
                11 => b"\x1b[23~",
                12 => b"\x1b[24~",
                _ => return None,
            };
            bytes.extend_from_slice(seq);
        }
        _ => return None,
    }

    if bytes.is_empty() {
        None
    } else {
        Some(bytes)
    }
}
