//! Pane-manager tests that spawn real PTYs.

#![cfg(unix)]

use clux::pane::{Direction, PaneManager, SplitDirection};

fn can_spawn_shell() -> bool {
    // Check if we can spawn a shell (may fail in some CI environments)
    std::process::Command::new("sh")
        .arg("-c")
        .arg("exit 0")
        .status()
        .map(|s| s.success())
        .unwrap_or(false)
}

#[test]
#[ignore] // Run with --ignored flag when PTY is available
fn test_pane_manager_creation() {
    if !can_spawn_shell() {
        eprintln!("Skipping test: cannot spawn shell");
        return;
    }

    let manager = PaneManager::new(80, 24, "/bin/sh");
    assert!(manager.is_ok());

    let manager = manager.unwrap();
    assert_eq!(manager.pane_count(), 1);
}

#[test]
#[ignore]
fn test_pane_manager_split() {
    if !can_spawn_shell() {
        return;
    }

    let mut manager = PaneManager::new(80, 24, "/bin/sh").unwrap();
    let initial_count = manager.pane_count();

    // Split vertical
    let result = manager.split(SplitDirection::Vertical);
    assert!(result.is_ok());
    assert_eq!(manager.pane_count(), initial_count + 1);

    // Split horizontal
    let result = manager.split(SplitDirection::Horizontal);
    assert!(result.is_ok());
    assert_eq!(manager.pane_count(), initial_count + 2);
}

#[test]
#[ignore]
fn test_pane_manager_navigation() {
    if !can_spawn_shell() {
        return;
    }

    let mut manager = PaneManager::new(80, 24, "/bin/sh").unwrap();

    // Split to create multiple panes
    let _ = manager.split(SplitDirection::Vertical);
    let second_pane = manager.focused_id();

    // Navigate left should go back to first pane
    manager.navigate(Direction::Left);
    let after_nav = manager.focused_id();
    assert_ne!(after_nav, second_pane);

    // Navigate right should go back to second pane
    manager.navigate(Direction::Right);
    assert_eq!(manager.focused_id(), second_pane);
}

#[test]
#[ignore]
fn test_pane_manager_close() {
    if !can_spawn_shell() {
        return;
    }

    let mut manager = PaneManager::new(80, 24, "/bin/sh").unwrap();
    let _ = manager.split(SplitDirection::Vertical);
    assert_eq!(manager.pane_count(), 2);

    // Close one pane
    manager.close_focused();
    assert_eq!(manager.pane_count(), 1);

    // Closing last pane should not work (returns None)
    let result = manager.close_focused();
    assert!(result.is_none());
    assert_eq!(manager.pane_count(), 1);
}

#[test]
#[ignore]
fn test_pane_manager_resize() {
    if !can_spawn_shell() {
        return;
    }

    let mut manager = PaneManager::new(80, 24, "/bin/sh").unwrap();
    let _ = manager.split(SplitDirection::Vertical);

    // Resize screen
    let result = manager.resize_screen(120, 40);
    assert!(result.is_ok());

    // All panes should have been resized
    for pane in manager.panes() {
        assert!(pane.rect.width > 0);
        assert!(pane.rect.height > 0);
    }
}
