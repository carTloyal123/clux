//! Protocol round-trip and framing tests.

use super::*;
use crate::cell::Cell;

// ------------------------------------------------------------------------
// Serialization Round-Trip Tests
// ------------------------------------------------------------------------

#[test]
fn test_serialization_sizes() {
    // These tests document expected sizes for performance monitoring

    let ping = ClientMessage::Ping;
    let ping_size = bincode::serialize(&ping).unwrap().len();
    assert!(
        ping_size < 10,
        "Ping should be tiny, got {} bytes",
        ping_size
    );

    let input = ClientMessage::Input(vec![b'a']);
    let input_size = bincode::serialize(&input).unwrap().len();
    assert!(
        input_size < 20,
        "Single char input should be small, got {} bytes",
        input_size
    );

    // A full 80x24 pane repaint
    let full_pane = ServerMessage::PaneUpdate {
        pane_id: 0,
        changed_rows: (0..24)
            .map(|row| PaneRow::new(row, vec![Cell::new('x'); 80]))
            .collect(),
        cursor: Some(CursorState::default()),
    };
    let full_pane_size = bincode::serialize(&full_pane).unwrap().len();
    assert!(
        full_pane_size < 64 * 1024,
        "Full pane repaint should be < 64KB, got {} bytes",
        full_pane_size
    );

    // Single row update should be much smaller
    let update = ServerMessage::PaneUpdate {
        pane_id: 0,
        changed_rows: vec![PaneRow::new(5, vec![Cell::new('x'); 80])],
        cursor: Some(CursorState::default()),
    };
    let update_size = bincode::serialize(&update).unwrap().len();
    assert!(
        update_size < 4096,
        "Single row update should be < 4KB, got {} bytes",
        update_size
    );

    // Hyperlinks add one run per row, not a URL per cell.
    let linked = ServerMessage::PaneUpdate {
        pane_id: 0,
        changed_rows: vec![PaneRow::with_links(
            5,
            vec![Cell::new('x'); 80],
            vec![RowLink {
                start_col: 0,
                end_col: 40,
                id: 7,
                url: "https://example.com/some/path".to_string(),
                detected: true,
            }],
        )],
        cursor: None,
    };
    let linked_size = bincode::serialize(&linked).unwrap().len();
    assert!(
        linked_size < update_size + 128,
        "A link should cost ~its URL, got {} bytes over {}",
        linked_size - update_size,
        update_size
    );
}

// ------------------------------------------------------------------------
// Edge Case Tests
// ------------------------------------------------------------------------
