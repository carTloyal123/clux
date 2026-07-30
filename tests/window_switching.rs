//! Windows, and content preservation across switches.

mod common;

use common::harness::*;
use std::time::Duration;

// Window Tests
// ============================================================================

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
