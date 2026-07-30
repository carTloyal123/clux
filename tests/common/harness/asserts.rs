//! Assertions and hyperlink extraction for tests.

use super::client::*;
use std::io::BufRead;

pub fn assert_pane_count(client: &TestClient, expected: usize) {
    let actual = client.pane_count();
    assert_eq!(
        actual,
        expected,
        "Expected {} panes, got {}\n\nLayout: {:?}",
        expected,
        actual,
        client.layout()
    );
}

pub fn assert_contains(client: &TestClient, text: &str) {
    let capture = client.capture();
    assert!(
        capture.contains(text),
        "Expected screen to contain '{}'\n\nActual screen content:\n{}",
        text,
        capture.as_text()
    );
}

/// The OSC 8 open sequences in one row's ANSI, as (id, url) pairs.
pub fn hyperlinks_in(ansi: &str) -> Vec<(String, String)> {
    let mut links = Vec::new();

    for chunk in ansi.split("\x1b]8;").skip(1) {
        let Some(body) = chunk.split("\x1b\\").next() else {
            continue;
        };
        // "id=<id>;<url>" for an open, "" or ";" for a close.
        let Some((params, url)) = body.split_once(';') else {
            continue;
        };
        if url.is_empty() {
            continue;
        }
        let id = params.strip_prefix("id=").unwrap_or(params).to_string();
        links.push((id, url.to_string()));
    }

    links
}

/// Every hyperlink the client would emit, as (row, id, url).
pub fn hyperlinks_by_row(client: &TestClient) -> Vec<(usize, String, String)> {
    let (_cols, rows) = client.screen_dimensions();
    (0..rows)
        .flat_map(|row| {
            hyperlinks_in(&client.render_row_ansi(row))
                .into_iter()
                .map(move |(id, url)| (row, id, url))
        })
        .collect()
}
