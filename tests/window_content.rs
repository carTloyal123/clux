//! Windows, and content preservation across switches.

mod common;

use clux::protocol::Direction;
use common::harness::*;
use std::time::Duration;

// Window Tests
// ============================================================================

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
