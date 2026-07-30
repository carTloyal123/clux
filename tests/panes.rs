//! Pane splitting, closing and I/O.

mod common;

use clux::protocol::Direction;
use common::harness::*;
use std::thread;
use std::time::Duration;

// Pane Tests
// ============================================================================

#[test]
fn test_single_pane_initial_state() {
    let client = TestClient::new()
        .size(80, 24)
        .timeout(Duration::from_secs(10))
        .build()
        .expect("Failed to create test client");

    assert_pane_count(&client, 1);
}

#[test]
fn test_horizontal_split_creates_two_panes() {
    let mut client = TestClient::new()
        .size(80, 24)
        .timeout(Duration::from_secs(10))
        .build()
        .expect("Failed to create test client");

    assert_pane_count(&client, 1);

    client.split_horizontal();
    client.wait_for_update().expect("wait_for_update failed");

    assert_pane_count(&client, 2);
}

#[test]
fn test_vertical_split_creates_two_panes() {
    let mut client = TestClient::new()
        .size(80, 24)
        .timeout(Duration::from_secs(10))
        .build()
        .expect("Failed to create test client");

    client.split_vertical();
    client.wait_for_update().expect("wait_for_update failed");

    assert_pane_count(&client, 2);
}

#[test]
fn test_three_pane_layout() {
    let mut client = TestClient::new()
        .size(100, 40)
        .timeout(Duration::from_secs(10))
        .build()
        .expect("Failed to create test client");

    client.split_vertical();
    // Wait until we have 2 panes
    client
        .wait_until(|s| s.layout().map(|l| l.panes.len() >= 2).unwrap_or(false))
        .expect("wait for 2 panes");
    assert_pane_count(&client, 2);

    client.split_horizontal();
    // Wait until we have 3 panes
    client
        .wait_until(|s| s.layout().map(|l| l.panes.len() >= 3).unwrap_or(false))
        .expect("wait for 3 panes");
    assert_pane_count(&client, 3);
}

#[test]
fn test_close_pane_reduces_count() {
    let mut client = TestClient::new()
        .size(80, 24)
        .timeout(Duration::from_secs(10))
        .build()
        .expect("Failed to create test client");

    client.split_horizontal();
    // Wait until we have 2 panes
    client
        .wait_until(|s| s.layout().map(|l| l.panes.len() >= 2).unwrap_or(false))
        .expect("wait for 2 panes");
    assert_pane_count(&client, 2);

    client.close_pane();
    // Wait until we're back to 1 pane
    client
        .wait_until(|s| s.layout().map(|l| l.panes.len() == 1).unwrap_or(false))
        .expect("wait for 1 pane");
    assert_pane_count(&client, 1);
}

// ============================================================================
// Input/Output Tests
// ============================================================================

#[test]
fn test_type_echo_see_output() {
    let mut client = TestClient::new()
        .size(80, 24)
        .timeout(Duration::from_secs(10))
        .build()
        .expect("Failed to create test client");

    // Let shell initialize
    std::thread::sleep(Duration::from_millis(500));
    client.drain_messages().ok();

    client.type_text("echo TESTOUTPUT123\n");

    client
        .wait_for_text("TESTOUTPUT123")
        .expect("Should see echo output");

    assert_contains(&client, "TESTOUTPUT123");
}

#[test]
fn test_type_in_split_pane() {
    let mut client = TestClient::new()
        .size(80, 24)
        .timeout(Duration::from_secs(10))
        .build()
        .expect("Failed to create test client");

    client.split_horizontal();
    client
        .wait_for_update()
        .expect("wait_for_update after split");

    std::thread::sleep(Duration::from_millis(500));
    client.drain_messages().ok();

    client.type_text("echo PANEB_OUTPUT\n");

    client
        .wait_for_text("PANEB_OUTPUT")
        .expect("Should see output in split pane");
}

// ============================================================================
