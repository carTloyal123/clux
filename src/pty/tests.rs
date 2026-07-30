//! PTY tests.

use super::*;

#[test]
fn test_pty_size() {
    let size = PtySize::new(24, 80);
    assert_eq!(size.rows, 24);
    assert_eq!(size.cols, 80);

    let winsize = size.to_winsize();
    assert_eq!(winsize.ws_row, 24);
    assert_eq!(winsize.ws_col, 80);
}

#[test]
fn test_detect_shell() {
    let shell = detect_shell();
    assert!(!shell.is_empty());
    assert!(shell.starts_with('/'));
}

// Note: spawn tests require actually spawning a shell,
// which is better suited for integration tests.
