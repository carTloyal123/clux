//! Integration tests for the pane management system.
//!
//! These tests verify that pane splitting, navigation, and layout
//! work correctly together.

use clux::pane::{Direction, PaneManager, Rect, SplitDirection};

/// Helper to create a PaneManager without actually spawning a shell.
/// For integration tests, we'll test the layout logic directly.

#[test]
fn test_rect_split_creates_valid_rects() {
    let rect = Rect::new(0, 0, 80, 24);

    // Split horizontally
    let (top, bottom) = rect.split_horizontal(0.5);
    assert_eq!(top.x, 0);
    assert_eq!(top.y, 0);
    assert_eq!(top.width, 80);
    assert!(top.height > 0);
    assert_eq!(bottom.x, 0);
    assert!(bottom.y > top.y);
    assert_eq!(bottom.width, 80);
    assert!(bottom.height > 0);

    // Total height should account for border
    assert_eq!(top.height + bottom.height + 1, 24);
}

#[test]
fn test_rect_split_vertical_creates_valid_rects() {
    let rect = Rect::new(0, 0, 80, 24);

    // Split vertically
    let (left, right) = rect.split_vertical(0.5);
    assert_eq!(left.x, 0);
    assert_eq!(left.y, 0);
    assert!(left.width > 0);
    assert_eq!(left.height, 24);
    assert!(right.x > left.x);
    assert_eq!(right.y, 0);
    assert!(right.width > 0);
    assert_eq!(right.height, 24);

    // Total width should account for border
    assert_eq!(left.width + right.width + 1, 80);
}

#[test]
fn test_rect_contains() {
    let rect = Rect::new(10, 5, 30, 15);

    // Inside
    assert!(rect.contains(10, 5)); // top-left corner
    assert!(rect.contains(39, 19)); // bottom-right (exclusive boundary)
    assert!(rect.contains(25, 12)); // middle

    // Outside
    assert!(!rect.contains(9, 5)); // left of rect
    assert!(!rect.contains(10, 4)); // above rect
    assert!(!rect.contains(40, 10)); // right of rect
    assert!(!rect.contains(25, 20)); // below rect
}

#[test]
fn test_rect_nested_splits() {
    let rect = Rect::new(0, 0, 120, 40);

    // First split vertical
    let (left, right) = rect.split_vertical(0.5);

    // Split left side horizontally
    let (top_left, bottom_left) = left.split_horizontal(0.5);

    // Verify all rects are valid
    assert!(top_left.width > 0 && top_left.height > 0);
    assert!(bottom_left.width > 0 && bottom_left.height > 0);
    assert!(right.width > 0 && right.height > 0);

    // Verify no overlap
    assert!(!top_left.contains(right.x, right.y));
    assert!(!bottom_left.contains(right.x, right.y));
    assert!(!right.contains(
        top_left.x + top_left.width / 2,
        top_left.y + top_left.height / 2
    ));
}

#[test]
fn test_rect_minimum_size() {
    let small_rect = Rect::new(0, 0, 10, 5);

    // Even small rects should split without panicking
    let (top, bottom) = small_rect.split_horizontal(0.5);
    let (left, right) = small_rect.split_vertical(0.5);

    // Results might be very small but should be valid (u16 is always >= 0)
    // Just verify the splits completed without panic
    assert!(top.height <= small_rect.height);
    assert!(bottom.height <= small_rect.height);
    assert!(left.width <= small_rect.width);
    assert!(right.width <= small_rect.width);
}

#[test]
fn test_rect_uneven_split() {
    let rect = Rect::new(0, 0, 100, 50);

    // 70/30 split
    let (left, right) = rect.split_vertical(0.7);
    assert!(left.width > right.width);

    // 30/70 split
    let (top, bottom) = rect.split_horizontal(0.3);
    assert!(top.height < bottom.height);
}

// Note: PaneManager tests require spawning actual PTYs, which may not work
// in all test environments. The following tests are designed to be run
// in environments where PTY spawning is available.

#[cfg(unix)]
mod pty_tests {
    use super::*;

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
}
