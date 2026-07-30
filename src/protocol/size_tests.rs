//! Message round-trip tests.

use super::*;

// ------------------------------------------------------------------------
// Serialization Round-Trip Tests
// ------------------------------------------------------------------------

#[test]
fn test_pane_update_message_sizes() {
    use crate::cell::Cell;

    // A typical pane update should be reasonably sized
    let pane_update = ServerMessage::PaneUpdate {
        pane_id: 0,
        changed_rows: vec![PaneRow::new(0, (0..80).map(|_| Cell::new('x')).collect())],
        cursor: Some(CursorState::default()),
    };
    let pane_update_size = bincode::serialize(&pane_update).unwrap().len();
    // 80 cells * ~20 bytes per cell + overhead should be < 2KB
    assert!(
        pane_update_size < 2048,
        "PaneUpdate for 80 cells should be < 2KB, got {} bytes",
        pane_update_size
    );

    // Layout change should be tiny
    let layout_changed = ServerMessage::LayoutChanged {
        layout: WindowLayout {
            panes: vec![
                PaneLayout {
                    pane_id: 0,
                    x: 0,
                    y: 0,
                    width: 40,
                    height: 24,
                    focused: true,
                },
                PaneLayout {
                    pane_id: 1,
                    x: 41,
                    y: 0,
                    width: 39,
                    height: 24,
                    focused: false,
                },
            ],
            screen_cols: 80,
            screen_rows: 24,
        },
    };
    let layout_size = bincode::serialize(&layout_changed).unwrap().len();
    assert!(
        layout_size < 100,
        "LayoutChanged for 2 panes should be < 100 bytes, got {} bytes",
        layout_size
    );
}
