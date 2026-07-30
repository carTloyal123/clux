//! Windows, and content preservation across switches.

mod common;

use clux::protocol::Direction;
use common::harness::*;
use std::thread;
use std::time::Duration;

// Window Tests
// ============================================================================

#[test]
fn test_new_window_triggers_layout_update() {
    let mut client = TestClient::new()
        .size(80, 24)
        .timeout(Duration::from_secs(10))
        .build()
        .expect("Failed to create test client");

    assert_pane_count(&client, 1);

    // Create a new window - should receive layout update
    client.new_window();
    client
        .wait_for_update()
        .expect("Should receive layout update after new window");

    // New window should have 1 pane
    assert_pane_count(&client, 1);
}

#[test]
fn test_window_switch_triggers_layout_update() {
    let mut client = TestClient::new()
        .size(80, 24)
        .timeout(Duration::from_secs(10))
        .build()
        .expect("Failed to create test client");

    // Create second window
    client.new_window();
    client
        .wait_for_update()
        .expect("layout update after new window");

    // Switch back to first window
    client.prev_window();
    client
        .wait_for_update()
        .expect("Should receive layout update after prev_window");

    assert_pane_count(&client, 1);
}

#[test]
fn test_window_with_splits_then_switch() {
    let mut client = TestClient::new()
        .size(80, 24)
        .timeout(Duration::from_secs(15))
        .build()
        .expect("Failed to create test client");

    // Split window 0 into 2 panes
    client.split_horizontal();
    client
        .wait_until(|s| s.layout().map(|l| l.panes.len() >= 2).unwrap_or(false))
        .expect("wait for 2 panes");
    assert_pane_count(&client, 2);

    // Create new window (window 1 with 1 pane)
    // Note: Pane IDs are per-window, so new window will have pane 0
    client.new_window();

    // Wait until we see a layout with 1 pane (the new window)
    // The 2-pane layout message might arrive first due to timing, so wait for 1 pane
    client
        .wait_until(|s| s.layout().map(|l| l.panes.len() == 1).unwrap_or(false))
        .expect("wait for 1 pane in new window");
    assert_pane_count(&client, 1);

    // Switch back to window 0 - should show 2 panes again
    client.prev_window();
    client
        .wait_until(|s| s.layout().map(|l| l.panes.len() >= 2).unwrap_or(false))
        .expect("wait for 2 panes after switching back");
    assert_pane_count(&client, 2);
}

#[test]
fn test_next_prev_window_cycle() {
    let mut client = TestClient::new()
        .size(80, 24)
        .timeout(Duration::from_secs(10))
        .build()
        .expect("Failed to create test client");

    // Create second window
    client.new_window();
    client.wait_for_update().expect("layout after new window");

    // next_window should cycle back to window 0
    client.next_window();
    client
        .wait_for_update()
        .expect("layout update after next_window");
    assert_pane_count(&client, 1);

    // prev_window should go to window 1
    client.prev_window();
    client
        .wait_for_update()
        .expect("layout update after prev_window");
    assert_pane_count(&client, 1);
}

#[test]
fn test_select_window_by_index() {
    let mut client = TestClient::new()
        .size(80, 24)
        .timeout(Duration::from_secs(10))
        .build()
        .expect("Failed to create test client");

    // Create second window with a split
    client.new_window();
    client.wait_for_update().expect("layout after new window");
    client.split_vertical();
    client
        .wait_until(|s| s.layout().map(|l| l.panes.len() >= 2).unwrap_or(false))
        .expect("wait for 2 panes in window 1");
    assert_pane_count(&client, 2);

    // Select window 0 (should have 1 pane)
    client.select_window(0);
    client
        .wait_until(|s| s.layout().map(|l| l.panes.len() == 1).unwrap_or(false))
        .expect("wait for 1 pane in window 0");
    assert_pane_count(&client, 1);

    // Select window 1 (should have 2 panes)
    client.select_window(1);
    client
        .wait_until(|s| s.layout().map(|l| l.panes.len() >= 2).unwrap_or(false))
        .expect("wait for 2 panes in window 1");
    assert_pane_count(&client, 2);
}

#[test]
fn test_type_in_different_windows() {
    let mut client = TestClient::new()
        .size(80, 24)
        .timeout(Duration::from_secs(15))
        .build()
        .expect("Failed to create test client");

    // Wait for initial shell
    std::thread::sleep(Duration::from_millis(500));
    client.drain_messages().ok();

    // Type in window 0
    client.type_text("echo WINDOW_ZERO\n");
    client
        .wait_for_text("WINDOW_ZERO")
        .expect("Should see WINDOW_ZERO");

    // Create new window and wait for layout with 1 pane (fresh window)
    // Note: Window 0 has 1 pane too, but the screen content will be different
    client.new_window();
    client.wait_for_update().expect("layout after new window");

    // Wait for shell in new window
    std::thread::sleep(Duration::from_millis(500));
    client.drain_messages().ok();

    client.type_text("echo WINDOW_ONE\n");
    client
        .wait_for_text("WINDOW_ONE")
        .expect("Should see WINDOW_ONE");

    // Window 1 should show WINDOW_ONE but not WINDOW_ZERO
    let capture = client.capture();
    assert!(capture.contains("WINDOW_ONE"), "Should contain WINDOW_ONE");
    assert!(
        !capture.contains("WINDOW_ZERO"),
        "Should NOT contain WINDOW_ZERO in window 1"
    );

    // Switch back to window 0
    client.prev_window();
    client
        .wait_for_text("WINDOW_ZERO")
        .expect("wait for WINDOW_ZERO after switching back");

    // Window 0 should show WINDOW_ZERO but not WINDOW_ONE
    let capture = client.capture();
    assert!(
        capture.contains("WINDOW_ZERO"),
        "Should contain WINDOW_ZERO in window 0"
    );
    assert!(
        !capture.contains("WINDOW_ONE"),
        "Should NOT contain WINDOW_ONE in window 0"
    );
}

#[test]
fn test_navigate_pane_triggers_update() {
    let mut client = TestClient::new()
        .size(80, 24)
        .timeout(Duration::from_secs(10))
        .build()
        .expect("Failed to create test client");

    // Create 2 panes with vertical split (left/right)
    client.split_vertical();
    client
        .wait_until(|s| s.layout().map(|l| l.panes.len() >= 2).unwrap_or(false))
        .expect("wait for 2 panes");

    // Remember initial focused pane
    let initial_focused = client.capture().focused_pane_id();

    // Navigate left - should trigger layout update with changed focus
    // After vertical split, focus is on right pane, so left should work
    client.navigate(Direction::Left);

    // Wait for the focus to actually change
    client
        .wait_until(|s| {
            s.layout()
                .and_then(|l| l.panes.iter().find(|p| p.focused).map(|p| p.pane_id))
                != initial_focused
        })
        .expect("Should receive layout update with changed focus after navigate");

    // Focus should have changed
    let new_focused = client.capture().focused_pane_id();
    assert_ne!(
        initial_focused, new_focused,
        "Focus should change after navigation"
    );
}

// ============================================================================
// Pane Content Preservation Tests
// ============================================================================

#[test]
fn test_split_preserves_original_pane_content() {
    let mut client = TestClient::new()
        .size(80, 24)
        .timeout(Duration::from_secs(15))
        .build()
        .expect("Failed to create test client");

    // Wait for shell to initialize
    std::thread::sleep(Duration::from_millis(500));
    client.drain_messages().ok();

    // Type some content that should be visible
    client.type_text("echo ORIGINAL_CONTENT\n");
    client
        .wait_for_text("ORIGINAL_CONTENT")
        .expect("Should see original content");

    // Verify content is visible before split
    let capture_before = client.capture();
    assert!(
        capture_before.contains("ORIGINAL_CONTENT"),
        "Content should be visible before split"
    );

    // Now split horizontally - this creates a new pane below
    client.split_horizontal();

    // Wait for both: 2 panes AND the original content to still be visible
    // This ensures we've received the pane updates, not just the layout
    client
        .wait_until(|s| {
            let has_two_panes = s.layout().map(|l| l.panes.len() >= 2).unwrap_or(false);
            if !has_two_panes {
                return false;
            }
            // Check if content is in the buffer
            let (_cols, rows) = s.dimensions();
            for row_idx in 0..rows {
                if let Some(row_cells) = s.get_row(row_idx) {
                    let row_text: String = row_cells.iter().map(|c| c.c).collect();
                    if row_text.contains("ORIGINAL_CONTENT") {
                        return true;
                    }
                }
            }
            false
        })
        .expect("wait for 2 panes with original content preserved");

    assert_pane_count(&client, 2);

    let capture_after = client.capture();
    assert!(
        capture_after.contains("ORIGINAL_CONTENT"),
        "Original content should be preserved after split.\n\nScreen content:\n{}",
        capture_after.as_text()
    );
}

#[test]
fn test_vertical_split_preserves_original_pane_content() {
    let mut client = TestClient::new()
        .size(100, 24)
        .timeout(Duration::from_secs(15))
        .build()
        .expect("Failed to create test client");

    // Wait for shell to initialize
    std::thread::sleep(Duration::from_millis(500));
    client.drain_messages().ok();

    // Type some content
    client.type_text("echo LEFTPANE_TEXT\n");
    client
        .wait_for_text("LEFTPANE_TEXT")
        .expect("Should see left pane content");

    // Verify content is visible before split
    let capture_before = client.capture();
    assert!(
        capture_before.contains("LEFTPANE_TEXT"),
        "Content should be visible before split"
    );

    // Split vertically - creates a new pane on the right
    client.split_vertical();

    // Wait for both: 2 panes AND the original content to still be visible
    client
        .wait_until(|s| {
            let has_two_panes = s.layout().map(|l| l.panes.len() >= 2).unwrap_or(false);
            if !has_two_panes {
                return false;
            }
            // Check if content is in the buffer
            let (_cols, rows) = s.dimensions();
            for row_idx in 0..rows {
                if let Some(row_cells) = s.get_row(row_idx) {
                    let row_text: String = row_cells.iter().map(|c| c.c).collect();
                    if row_text.contains("LEFTPANE_TEXT") {
                        return true;
                    }
                }
            }
            false
        })
        .expect("wait for 2 panes with original content preserved");

    assert_pane_count(&client, 2);

    let capture_after = client.capture();
    assert!(
        capture_after.contains("LEFTPANE_TEXT"),
        "Original content should be preserved after vertical split.\n\nScreen content:\n{}",
        capture_after.as_text()
    );
}

// ============================================================================
