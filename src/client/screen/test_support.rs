//! Fixtures shared by the screen tests.

use crate::cell::Cell;
use crate::protocol::{PaneLayout, PaneRow, RowLink, WindowLayout};

/// Layout with a single full-screen pane.
pub(super) fn single_pane_layout(cols: u16, rows: u16) -> WindowLayout {
    WindowLayout {
        panes: vec![PaneLayout {
            pane_id: 0,
            x: 0,
            y: 0,
            width: cols,
            height: rows,
            focused: true,
        }],
        screen_cols: cols,
        screen_rows: rows,
    }
}

pub(super) fn text_cells(s: &str) -> Vec<Cell> {
    s.chars().map(Cell::new).collect()
}

/// A link clux detected itself (so it gets link styling).
pub(super) fn link(start_col: u16, end_col: u16, id: u32, url: &str) -> RowLink {
    RowLink {
        start_col,
        end_col,
        id,
        url: url.to_string(),
        detected: true,
    }
}

/// A link the application asked for with OSC 8 (styling left alone).
pub(super) fn app_link(start_col: u16, end_col: u16, id: u32, url: &str) -> RowLink {
    RowLink {
        detected: false,
        ..link(start_col, end_col, id, url)
    }
}

/// Two side-by-side 10-wide panes with a divider at column 10.
pub(super) fn split_layout() -> WindowLayout {
    WindowLayout {
        panes: vec![
            PaneLayout {
                pane_id: 0,
                x: 0,
                y: 0,
                width: 10,
                height: 3,
                focused: true,
            },
            PaneLayout {
                pane_id: 1,
                x: 11,
                y: 0,
                width: 10,
                height: 3,
                focused: false,
            },
        ],
        screen_cols: 21,
        screen_rows: 3,
    }
}

pub(super) fn pane_row(row_idx: u16, text: &str, wrapped: bool) -> PaneRow {
    PaneRow::new(row_idx, text_cells(text)).wrapped(wrapped)
}
