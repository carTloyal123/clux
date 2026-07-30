//! Selection and copy.

mod common;

use common::harness::*;
use std::time::Duration;

// Selection / Copy Tests
// ============================================================================

#[test]
fn test_selection_copies_a_wrapped_path_as_one_line() {
    // The whole point of shipping the soft-wrap flag: a path too long for the
    // pane must copy back without a newline spliced into it.
    let mut client = TestClient::new()
        .size(40, 24)
        .timeout(Duration::from_secs(10))
        .build()
        .expect("Failed to create test client");

    std::thread::sleep(Duration::from_millis(500));
    client.drain_messages().ok();

    let path = "/tmp/a/deliberately/long/path/that/will/not/fit/in/one/row.txt";
    assert!(path.len() > 40, "path must be wider than the pane");

    client.type_text(&format!("printf '{}\\n'\n", path));
    client
        .wait_for_text("/tmp/a/deliberately")
        .expect("Should see the start of the path");
    std::thread::sleep(Duration::from_millis(300));
    client.drain_messages().ok();

    // Select the whole pane; the wrapped rows must join.
    let (cols, rows) = client.screen_dimensions();
    let copied = client
        .select_text((0, 0), (rows - 1, cols - 1))
        .expect("selection should produce text");

    assert!(
        copied.contains(path),
        "wrapped path came back broken\ncopied:\n{}\n{}",
        copied,
        client.dump_screen()
    );
}

#[test]
fn test_selection_is_confined_to_one_pane() {
    let mut client = TestClient::new()
        .size(80, 24)
        .timeout(Duration::from_secs(15))
        .build()
        .expect("Failed to create test client");

    std::thread::sleep(Duration::from_millis(500));
    client.drain_messages().ok();

    client.type_text("printf 'LEFTSIDE\\n'\n");
    client.wait_for_text("LEFTSIDE").expect("left pane output");

    client.split_vertical();
    client
        .wait_until(|s| s.layout().map(|l| l.panes.len() >= 2).unwrap_or(false))
        .expect("wait for 2 panes");
    std::thread::sleep(Duration::from_millis(500));
    client.drain_messages().ok();

    client.type_text("printf 'RIGHTSIDE\\n'\n");
    client
        .wait_for_text("RIGHTSIDE")
        .expect("right pane output");
    std::thread::sleep(Duration::from_millis(300));
    client.drain_messages().ok();

    // Drag from the left pane across the divider into the right one.
    let (cols, rows) = client.screen_dimensions();
    let copied = client
        .select_text((0, 0), (rows - 1, cols - 1))
        .expect("selection should produce text");

    assert!(
        copied.contains("LEFTSIDE"),
        "selection lost its own pane's text\n{}",
        client.dump_screen()
    );
    assert!(
        !copied.contains("RIGHTSIDE"),
        "selection crossed into the other pane\ncopied:\n{}\n{}",
        copied,
        client.dump_screen()
    );
    assert!(
        !copied.contains('│'),
        "selection included the pane divider\ncopied:\n{}",
        copied
    );
}

// ============================================================================
