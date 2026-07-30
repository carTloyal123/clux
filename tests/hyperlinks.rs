//! OSC 8 hyperlinks, detected and application-emitted.

mod common;

use common::harness::*;
use std::time::Duration;

// Hyperlink Tests
// ============================================================================

#[test]
fn test_plain_url_becomes_a_real_hyperlink() {
    let mut client = TestClient::new()
        .size(80, 24)
        .timeout(Duration::from_secs(10))
        .build()
        .expect("Failed to create test client");

    std::thread::sleep(Duration::from_millis(500));
    client.drain_messages().ok();

    client.type_text("printf 'go to https://example.com/plain now\\n'\n");
    client
        .wait_for_text("https://example.com/plain")
        .expect("Should see the URL text");

    let links = hyperlinks_in(&client.render_screen_ansi());
    assert!(
        links
            .iter()
            .any(|(_, url)| url == "https://example.com/plain"),
        "no OSC 8 hyperlink for the printed URL; links were {links:?}\n{}",
        client.dump_screen()
    );
    // The link must stop at the URL, not swallow the surrounding words.
    for (_, url) in &links {
        assert!(!url.contains("now"), "link ran into the prose: {url:?}");
        assert!(!url.contains("go"), "link ran into the prose: {url:?}");
    }

    // A URL clux detected itself is underlined so it reads as a link.
    let underlined = (0..client.screen_dimensions().1).any(|row| {
        let ansi = client.render_row_ansi(row);
        ansi.contains("https://example.com/plain") && ansi.contains("\x1b[0;4m")
    });
    assert!(
        underlined,
        "detected URL was not underlined\n{}",
        client.dump_screen()
    );
}

#[test]
fn test_wrapped_url_is_one_link_across_rows() {
    // Narrow pane so the URL has to wrap: this is the case a host terminal
    // cannot resolve on its own, because every row clux paints looks unwrapped.
    let mut client = TestClient::new()
        .size(40, 24)
        .timeout(Duration::from_secs(10))
        .build()
        .expect("Failed to create test client");

    std::thread::sleep(Duration::from_millis(500));
    client.drain_messages().ok();

    let url = "https://example.com/a/quite/long/path/that/must/wrap?q=1";
    assert!(url.len() > 40, "test URL must be wider than the pane");
    client.type_text(&format!("printf '{}\\n'\n", url));
    client
        .wait_for_text("https://example.com/a/quite")
        .expect("Should see the start of the URL");
    // Give the tail row time to arrive as well.
    std::thread::sleep(Duration::from_millis(300));
    client.drain_messages().ok();

    // The URL shows up twice (the echoed command line and the output), and each
    // occurrence is its own link, so group by id before asserting.
    let links = hyperlinks_by_row(&client);
    let mut rows_by_id: std::collections::HashMap<&str, Vec<usize>> =
        std::collections::HashMap::new();
    for (row, id, found) in &links {
        if found == url {
            rows_by_id.entry(id).or_default().push(*row);
        }
    }

    assert!(
        !rows_by_id.is_empty(),
        "wrapped URL produced no hyperlink; links were {links:?}\n{}",
        client.dump_screen()
    );

    let spans_rows = rows_by_id.values().find(|rows| rows.len() >= 2);
    let rows = spans_rows.unwrap_or_else(|| {
        panic!(
            "no link covered more than one row, so the wrap broke it: {rows_by_id:?}\n{}",
            client.dump_screen()
        )
    });

    // Its rows must be adjacent - it is one wrapped line, not two matches.
    let mut sorted = rows.clone();
    sorted.sort();
    assert!(
        sorted.windows(2).all(|w| w[1] == w[0] + 1),
        "link rows are not adjacent: {sorted:?}"
    );
}

#[test]
fn test_application_osc8_hyperlink_reaches_the_host_terminal() {
    // The case tmux drops on Ghostty: the app emits OSC 8 itself.
    let mut client = TestClient::new()
        .size(80, 24)
        .timeout(Duration::from_secs(10))
        .build()
        .expect("Failed to create test client");

    std::thread::sleep(Duration::from_millis(500));
    client.drain_messages().ok();

    client.type_text(
        "printf '\\033]8;;https://example.com/osc8\\033\\\\CLICKME\\033]8;;\\033\\\\\\n'\n",
    );
    client
        .wait_for_text("CLICKME")
        .expect("Should see the link text");

    let links = hyperlinks_in(&client.render_screen_ansi());
    assert!(
        links
            .iter()
            .any(|(_, url)| url == "https://example.com/osc8"),
        "application OSC 8 hyperlink was dropped; links were {links:?}\n{}",
        client.dump_screen()
    );
}

// ============================================================================
