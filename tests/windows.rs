//! Windows, and content preservation across switches.

mod common;

use common::harness::*;
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
