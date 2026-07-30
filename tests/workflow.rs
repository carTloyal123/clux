//! Full workflows, and shells exiting.

mod common;

use common::harness::*;
use std::thread;
use std::time::Duration;

// Full Workflow Tests
// ============================================================================

#[test]
fn test_full_workflow_split_and_type() {
    let mut client = TestClient::new()
        .size(100, 40)
        .timeout(Duration::from_secs(15))
        .build()
        .expect("Failed to create test client");

    std::thread::sleep(Duration::from_millis(500));
    client.drain_messages().ok();

    // Step 1: Split vertical
    client.split_vertical();
    client
        .wait_until(|s| s.layout().map(|l| l.panes.len() >= 2).unwrap_or(false))
        .expect("wait for 2 panes");
    assert_pane_count(&client, 2);

    // Step 2: Type in focused pane (right pane after split)
    std::thread::sleep(Duration::from_millis(300));
    client.drain_messages().ok();
    client.type_text("echo PANE_OUTPUT\n");
    client
        .wait_for_text("PANE_OUTPUT")
        .expect("Should see PANE_OUTPUT");

    // Step 3: Split again horizontally
    client.split_horizontal();
    client
        .wait_until(|s| s.layout().map(|l| l.panes.len() >= 3).unwrap_or(false))
        .expect("wait for 3 panes");
    assert_pane_count(&client, 3);

    // Step 4: Type in new pane
    std::thread::sleep(Duration::from_millis(300));
    client.drain_messages().ok();
    client.type_text("echo THIRD_PANE\n");
    client
        .wait_for_text("THIRD_PANE")
        .expect("Should see THIRD_PANE");

    // Both outputs should be visible
    let capture = client.capture();
    assert!(capture.contains("PANE_OUTPUT"), "Missing PANE_OUTPUT");
    assert!(capture.contains("THIRD_PANE"), "Missing THIRD_PANE");
}

// ============================================================================
// Shell Exit Tests
// ============================================================================

#[test]
fn test_exit_closes_pane() {
    let mut client = TestClient::new()
        .size(80, 24)
        .timeout(Duration::from_secs(15))
        .build()
        .expect("Failed to create test client");

    // Split to have 2 panes
    client.split_horizontal();
    client
        .wait_until(|s| s.layout().map(|l| l.panes.len() >= 2).unwrap_or(false))
        .expect("wait for 2 panes");
    assert_pane_count(&client, 2);

    // Wait for shell to initialize
    std::thread::sleep(Duration::from_millis(500));
    client.drain_messages().ok();

    // Type exit in focused pane
    client.type_text("exit\n");

    // Wait for pane to close (should go back to 1 pane)
    client
        .wait_until(|s| s.layout().map(|l| l.panes.len() == 1).unwrap_or(false))
        .expect("wait for pane to close after exit");

    assert_pane_count(&client, 1);
}

#[test]
fn test_exit_multiple_panes() {
    let mut client = TestClient::new()
        .size(100, 40)
        .timeout(Duration::from_secs(20))
        .build()
        .expect("Failed to create test client");

    // Create 3 panes
    client.split_vertical();
    client
        .wait_until(|s| s.layout().map(|l| l.panes.len() >= 2).unwrap_or(false))
        .expect("wait for 2 panes");

    client.split_horizontal();
    client
        .wait_until(|s| s.layout().map(|l| l.panes.len() >= 3).unwrap_or(false))
        .expect("wait for 3 panes");
    assert_pane_count(&client, 3);

    // Wait for shells
    std::thread::sleep(Duration::from_millis(500));
    client.drain_messages().ok();

    // Exit first pane
    client.type_text("exit\n");
    client
        .wait_until(|s| s.layout().map(|l| l.panes.len() == 2).unwrap_or(false))
        .expect("wait for 2 panes after first exit");
    assert_pane_count(&client, 2);

    // Wait for new focused pane's shell
    std::thread::sleep(Duration::from_millis(300));
    client.drain_messages().ok();

    // Exit second pane
    client.type_text("exit\n");
    client
        .wait_until(|s| s.layout().map(|l| l.panes.len() == 1).unwrap_or(false))
        .expect("wait for 1 pane after second exit");
    assert_pane_count(&client, 1);
}

// ============================================================================
