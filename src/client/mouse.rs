//! Mouse event encoding for forwarding to applications.
//!
//! When the focused pane has enabled mouse reporting, the client re-encodes the
//! host terminal's mouse events as SGR (mode 1006) sequences and sends them to
//! the pty. Selection gestures never reach here - see [`crate::client::screen`]
//! and docs/SELECTION.md.

use crossterm::event::{KeyModifiers, MouseButton, MouseEvent, MouseEventKind};

/// Encode a mouse event as an SGR mouse report (`CSI < Cb ; Cx ; Cy M|m`).
///
/// `press` selects the terminator: `M` for presses and motion, `m` for releases.
pub fn encode_mouse_sgr(mouse: &MouseEvent, press: bool) -> Vec<u8> {
    let button = match mouse.kind {
        MouseEventKind::Down(MouseButton::Left) | MouseEventKind::Up(MouseButton::Left) => 0,
        MouseEventKind::Down(MouseButton::Middle) | MouseEventKind::Up(MouseButton::Middle) => 1,
        MouseEventKind::Down(MouseButton::Right) | MouseEventKind::Up(MouseButton::Right) => 2,
        MouseEventKind::Drag(MouseButton::Left) => 32,
        MouseEventKind::Drag(MouseButton::Middle) => 33,
        MouseEventKind::Drag(MouseButton::Right) => 34,
        MouseEventKind::Moved => 35,
        MouseEventKind::ScrollUp => 64,
        MouseEventKind::ScrollDown => 65,
        MouseEventKind::ScrollLeft => 66,
        MouseEventKind::ScrollRight => 67,
    };

    let mut modifiers = 0;
    if mouse.modifiers.contains(KeyModifiers::SHIFT) {
        modifiers |= 4;
    }
    if mouse.modifiers.contains(KeyModifiers::ALT) {
        modifiers |= 8;
    }
    if mouse.modifiers.contains(KeyModifiers::CONTROL) {
        modifiers |= 16;
    }

    let cb = button | modifiers;
    let cx = mouse.column + 1;
    let cy = mouse.row + 1;
    let suffix = if press { 'M' } else { 'm' };

    format!("\x1b[<{};{};{}{}", cb, cx, cy, suffix).into_bytes()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn event(kind: MouseEventKind, column: u16, row: u16, modifiers: KeyModifiers) -> MouseEvent {
        MouseEvent {
            kind,
            column,
            row,
            modifiers,
        }
    }

    #[test]
    fn encodes_button_presses_and_releases() {
        let press = event(
            MouseEventKind::Down(MouseButton::Left),
            0,
            0,
            KeyModifiers::NONE,
        );
        assert_eq!(encode_mouse_sgr(&press, true), b"\x1b[<0;1;1M".to_vec());

        let release = event(
            MouseEventKind::Up(MouseButton::Left),
            4,
            9,
            KeyModifiers::NONE,
        );
        assert_eq!(encode_mouse_sgr(&release, false), b"\x1b[<0;5;10m".to_vec());
    }

    #[test]
    fn encodes_buttons_drags_and_scrolls() {
        let cases = [
            (MouseEventKind::Down(MouseButton::Middle), 1),
            (MouseEventKind::Down(MouseButton::Right), 2),
            (MouseEventKind::Drag(MouseButton::Left), 32),
            (MouseEventKind::Moved, 35),
            (MouseEventKind::ScrollUp, 64),
            (MouseEventKind::ScrollDown, 65),
        ];

        for (kind, expected_cb) in cases {
            let encoded = encode_mouse_sgr(&event(kind, 0, 0, KeyModifiers::NONE), true);
            let expected = format!("\x1b[<{};1;1M", expected_cb).into_bytes();
            assert_eq!(encoded, expected, "wrong encoding for {kind:?}");
        }
    }

    #[test]
    fn encodes_modifiers_into_the_button_byte() {
        let shift = event(
            MouseEventKind::Down(MouseButton::Left),
            0,
            0,
            KeyModifiers::SHIFT,
        );
        assert_eq!(encode_mouse_sgr(&shift, true), b"\x1b[<4;1;1M".to_vec());

        let all = event(
            MouseEventKind::Down(MouseButton::Left),
            0,
            0,
            KeyModifiers::SHIFT | KeyModifiers::ALT | KeyModifiers::CONTROL,
        );
        assert_eq!(encode_mouse_sgr(&all, true), b"\x1b[<28;1;1M".to_vec());
    }

    #[test]
    fn coordinates_are_one_based() {
        let mouse = event(
            MouseEventKind::Down(MouseButton::Left),
            10,
            20,
            KeyModifiers::NONE,
        );
        assert_eq!(encode_mouse_sgr(&mouse, true), b"\x1b[<0;11;21M".to_vec());
    }
}
