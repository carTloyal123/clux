//! Scrolling back through history.

mod common;

use common::harness::*;
use std::time::Duration;

// Scrollback Tests
// ============================================================================

#[test]
fn test_scrolling_back_shows_history_and_returning_shows_live_output() {
    let mut client = TestClient::new()
        .size(80, 24)
        .timeout(Duration::from_secs(15))
        .build()
        .expect("Failed to create test client");

    std::thread::sleep(Duration::from_millis(500));
    client.drain_messages().ok();

    // Push well past a screen of output so there is real history.
    client.type_text("for i in $(seq 1 80); do echo \"LINE_$i\"; done\n");
    client
        .wait_for_text("LINE_80")
        .expect("should see the last line");
    std::thread::sleep(Duration::from_millis(300));
    client.drain_messages().ok();

    let live = client.capture().as_text();
    assert!(live.contains("LINE_80"), "live view should show the tail");
    assert!(
        !live.contains("LINE_1\n") && !live.contains("LINE_2\n"),
        "early lines should have scrolled off:\n{}",
        live
    );

    // Scroll back a full screen.
    client.scroll(30);
    client
        .wait_for_text("LINE_50")
        .expect("scrolled view should reveal earlier output");

    let scrolled = client.capture().as_text();
    assert!(
        !scrolled.contains("LINE_80"),
        "scrolled view should no longer show the tail:\n{}",
        scrolled
    );

    // Back to the live view.
    client.scroll(0);
    client
        .wait_for_text("LINE_80")
        .expect("returning to live should show the tail again");
}

#[test]
fn test_links_still_work_after_scrolling_back_to_them() {
    let mut client = TestClient::new()
        .size(80, 24)
        .timeout(Duration::from_secs(15))
        .build()
        .expect("Failed to create test client");

    std::thread::sleep(Duration::from_millis(500));
    client.drain_messages().ok();

    // Print a URL, then push it off the top of the screen.
    client.type_text("printf 'go to https://example.com/in-history now\\n'\n");
    client
        .wait_for_text("https://example.com/in-history")
        .expect("should see the URL");
    client.type_text("for i in $(seq 1 40); do echo \"FILLER_$i\"; done\n");
    client.wait_for_text("FILLER_40").expect("filler output");
    std::thread::sleep(Duration::from_millis(300));
    client.drain_messages().ok();

    let live = hyperlinks_by_row(&client);
    assert!(
        !live.iter().any(|(_, _, url)| url.contains("in-history")),
        "the URL should have scrolled off the live view: {live:?}"
    );

    // Scroll back to it: the link must come back with the text.
    client.scroll(30);
    client
        .wait_for_text("https://example.com/in-history")
        .expect("scrolling back should reveal the URL again");
    std::thread::sleep(Duration::from_millis(200));
    client.drain_messages().ok();

    let scrolled = hyperlinks_by_row(&client);
    assert!(
        scrolled
            .iter()
            .any(|(_, _, url)| url == "https://example.com/in-history"),
        "a URL scrolled back into view should still be a hyperlink; links were {scrolled:?}\n{}",
        client.dump_screen()
    );
}

#[test]
fn test_typing_returns_to_the_live_view() {
    let mut client = TestClient::new()
        .size(80, 24)
        .timeout(Duration::from_secs(15))
        .build()
        .expect("Failed to create test client");

    std::thread::sleep(Duration::from_millis(500));
    client.drain_messages().ok();

    client.type_text("for i in $(seq 1 60); do echo \"ROW_$i\"; done\n");
    client.wait_for_text("ROW_60").expect("initial output");
    std::thread::sleep(Duration::from_millis(300));
    client.drain_messages().ok();

    client.scroll(25);
    client
        .wait_for_text("ROW_20")
        .expect("scrolled view should show earlier output");

    // Typing anything snaps back to the live view.
    client.type_text("echo BACK_TO_LIVE\n");
    client
        .wait_for_text("BACK_TO_LIVE")
        .expect("typing should return to the live view and show the result");
}

// ============================================================================
