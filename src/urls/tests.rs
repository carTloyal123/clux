//! Tests for link resolution.

use super::*;
use crate::buffer::Buffer;
use crate::cell::Cell;

/// Write `s` into a buffer starting at row 0, wrapping like the terminal does.
fn grid_with(text: &str, cols: usize, rows: usize) -> Buffer {
    let mut buffer = Buffer::new(rows, cols, 0);
    let mut row = 0;
    let mut col = 0;

    for c in text.chars() {
        if col == cols {
            buffer.set_row_wrapped(row, true);
            row += 1;
            col = 0;
        }
        buffer.set_cell(row, col, Cell::new(c));
        col += 1;
    }

    buffer
}

#[test]
fn joins_url_across_soft_wrap() {
    // 20 columns forces the URL to wrap mid-path.
    let grid = grid_with("go to https://example.com/a/very/long/path now", 20, 5);
    let store = HyperlinkStore::new();
    let links = resolve_links(&grid, &store, 7, true, &[0, 1, 2, 3, 4]);

    let mut runs: Vec<&LinkRun> = links.values().flatten().collect();
    runs.sort_by_key(|r| (r.row, r.start_col));

    assert!(
        runs.len() > 1,
        "wrapped URL should produce one run per row, got {runs:?}"
    );
    for run in &runs {
        assert_eq!(run.url, "https://example.com/a/very/long/path");
    }
    // One logical link => one shared OSC 8 id across the rows.
    let ids: Vec<u32> = runs.iter().map(|r| r.id).collect();
    assert!(ids.windows(2).all(|w| w[0] == w[1]), "ids differ: {ids:?}");

    // The run must cover exactly the URL, not the leading "go to ".
    assert_eq!((runs[0].row, runs[0].start_col), (0, 6));
}

#[test]
fn detected_ids_are_stable_across_scans() {
    let grid = grid_with("https://example.com/x", 40, 3);
    let store = HyperlinkStore::new();

    let first = resolve_links(&grid, &store, 3, true, &[0]);
    let second = resolve_links(&grid, &store, 3, true, &[0]);
    assert_eq!(first, second);
}

#[test]
fn different_panes_get_different_ids() {
    let grid = grid_with("https://example.com/x", 40, 3);
    let store = HyperlinkStore::new();

    let a = resolve_links(&grid, &store, 1, true, &[0]);
    let b = resolve_links(&grid, &store, 2, true, &[0]);
    assert_ne!(a[&0][0].id, b[&0][0].id);
}

#[test]
fn two_copies_of_a_url_are_distinct_links() {
    let mut grid = grid_with("https://a.io/x", 40, 3);
    for (col, c) in "https://a.io/x".chars().enumerate() {
        grid.set_cell(1, col, Cell::new(c));
    }

    let store = HyperlinkStore::new();
    let links = resolve_links(&grid, &store, 1, true, &[0, 1]);
    assert_ne!(links[&0][0].id, links[&1][0].id);
    assert_eq!(links[&0][0].url, links[&1][0].url);
}

#[test]
fn explicit_osc8_runs_win_over_detection() {
    let mut store = HyperlinkStore::new();
    let id = store.intern("https://real.example/target");

    // Cells display one URL but are linked to another (the OSC 8 case).
    let mut grid = Buffer::new(3, 40, 0);
    for (col, c) in "https://display.example".chars().enumerate() {
        let mut cell = Cell::new(c);
        cell.hyperlink = Some(id);
        grid.set_cell(0, col, cell);
    }

    let links = resolve_links(&grid, &store, 1, true, &[0]);
    let runs = &links[&0];
    assert_eq!(runs.len(), 1, "detection must not add a second link");
    assert_eq!(runs[0].url, "https://real.example/target");
    assert_eq!(runs[0].start_col, 0);
    assert_eq!(runs[0].end_col, 23);
}

#[test]
fn explicit_runs_share_an_id_across_rows() {
    let mut store = HyperlinkStore::new();
    let id = store.intern("https://example.com/wrapped");

    let mut grid = Buffer::new(3, 10, 0);
    for row in 0..2 {
        for col in 0..10 {
            let mut cell = Cell::new('x');
            cell.hyperlink = Some(id);
            grid.set_cell(row, col, cell);
        }
    }

    let links = resolve_links(&grid, &store, 1, true, &[0, 1]);
    assert_eq!(links[&0][0].id, links[&1][0].id);
}

#[test]
fn sanitize_strips_escape_injection() {
    assert_eq!(
        sanitize_url("https://a.io/\x1b]0;pwned\x07x").as_deref(),
        Some("https://a.io/]0;pwnedx")
    );
    assert_eq!(sanitize_url("").as_deref(), None);
    assert_eq!(sanitize_url("\x1b\x07").as_deref(), None);
    assert_eq!(
        sanitize_url(&"h".repeat(MAX_URL_LEN + 100)).map(|u| u.len()),
        Some(MAX_URL_LEN)
    );
}

#[test]
fn detection_can_be_disabled() {
    let grid = grid_with("https://example.com/x", 40, 3);
    let store = HyperlinkStore::new();
    assert!(resolve_links(&grid, &store, 1, false, &[0]).is_empty());
}

#[test]
fn untouched_logical_lines_are_not_scanned() {
    let mut grid = Buffer::new(3, 40, 0);
    for (col, c) in "https://a.io/x".chars().enumerate() {
        grid.set_cell(2, col, Cell::new(c));
    }

    let store = HyperlinkStore::new();
    let links = resolve_links(&grid, &store, 1, true, &[0, 1]);
    assert!(links.is_empty(), "row 2 was not requested: {links:?}");
    assert!(resolve_links(&grid, &store, 1, true, &[2]).contains_key(&2));
}

#[test]
fn a_dirty_tail_row_reports_the_whole_wrapped_link() {
    // Only the tail row of the wrapped URL is dirty, but the head row has to
    // be repainted as well or its fragment keeps the stale earlier link.
    let grid = grid_with("go to https://example.com/a/very/long/path now", 20, 5);
    let store = HyperlinkStore::new();

    let links = resolve_links(&grid, &store, 7, true, &[1]);
    let mut rows: Vec<u16> = links.keys().copied().collect();
    rows.sort();
    assert!(rows.len() > 1, "expected head row too, got {rows:?}");
    assert_eq!(rows[0], 0);
}
