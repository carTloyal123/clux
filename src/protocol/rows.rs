//! Screen content on the wire: rows, cells, cursor, layout.

use serde::{Deserialize, Serialize};

use crate::cell::Cell;

/// Cursor state for rendering.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CursorState {
    /// Row position (0-indexed).
    pub row: u16,
    /// Column position (0-indexed).
    pub col: u16,
    /// Whether the cursor is visible.
    pub visible: bool,
}

impl Default for CursorState {
    fn default() -> Self {
        Self {
            row: 0,
            col: 0,
            visible: true,
        }
    }
}

/// Information about a session for listing.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SessionInfo {
    /// Session identifier.
    pub id: u32,
    /// Session name.
    pub name: String,
    /// Unix timestamp of session creation.
    pub created_at: u64,
    /// Number of windows in the session.
    pub windows: usize,
    /// Number of attached clients.
    pub attached_clients: usize,
}

// ============================================================================
// Pane Rendering Types (hybrid client-server rendering)
// ============================================================================

/// Layout information for a single pane.
/// Used by clients to composite pane content into the screen buffer.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PaneLayout {
    /// Unique identifier for this pane.
    pub pane_id: u32,
    /// X position (column) of the pane's top-left corner in screen coordinates.
    pub x: u16,
    /// Y position (row) of the pane's top-left corner in screen coordinates.
    pub y: u16,
    /// Width of the pane in columns.
    pub width: u16,
    /// Height of the pane in rows.
    pub height: u16,
    /// Whether this pane currently has focus.
    pub focused: bool,
}

/// Layout of all panes in the active window.
/// Sent when layout changes (split, close, resize).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct WindowLayout {
    /// Layout information for each pane.
    pub panes: Vec<PaneLayout>,
    /// Total screen width in columns.
    pub screen_cols: u16,
    /// Total screen height in rows (excluding status line).
    pub screen_rows: u16,
}

/// A single row of pane content with styled cells.
/// Row index is relative to the pane's top-left corner.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PaneRow {
    /// Row index within the pane (0 = top row of pane).
    pub row_idx: u16,
    /// Styled cells for this row.
    pub cells: Vec<Cell>,
    /// Hyperlink runs covering cells in this row, in column order.
    ///
    /// Resolved server-side (explicit OSC 8 plus detected URLs) because only the
    /// server knows where a pane's logical lines end. Carried per row rather than
    /// as an id table so a client never has to hold state that can go stale.
    pub links: Vec<RowLink>,
    /// Whether this row's content continues onto the next row (soft wrap).
    ///
    /// The client needs this to extract selected text: a wrapped row must be
    /// joined to the next one, or copying a long path or URL inserts a newline
    /// in the middle of it. Only the server knows where a logical line ends.
    pub wrapped: bool,
}

impl PaneRow {
    /// Create a new pane row with no hyperlinks.
    pub fn new(row_idx: u16, cells: Vec<Cell>) -> Self {
        Self {
            row_idx,
            cells,
            links: Vec::new(),
            wrapped: false,
        }
    }

    /// Create a new pane row with hyperlink runs.
    pub fn with_links(row_idx: u16, cells: Vec<Cell>, links: Vec<RowLink>) -> Self {
        Self {
            row_idx,
            cells,
            links,
            wrapped: false,
        }
    }

    /// Mark whether this row soft-wraps onto the next one.
    pub fn wrapped(mut self, wrapped: bool) -> Self {
        self.wrapped = wrapped;
        self
    }
}

/// A hyperlink run within a single row of a pane.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RowLink {
    /// First column of the run (pane-local).
    pub start_col: u16,
    /// One past the last column of the run (pane-local).
    pub end_col: u16,
    /// OSC 8 id shared by every run of the same logical link, so the outer
    /// terminal treats fragments split across rows as one link.
    pub id: u32,
    /// Target URL.
    pub url: String,
    /// Whether clux found this link itself rather than the application asking
    /// for it. Detected links get an underline so they read as links; an
    /// application's own OSC 8 links keep exactly the styling it printed.
    pub detected: bool,
}

impl From<crate::urls::LinkRun> for RowLink {
    fn from(run: crate::urls::LinkRun) -> Self {
        Self {
            start_col: run.start_col,
            end_col: run.end_col,
            id: run.id,
            url: run.url,
            detected: run.detected,
        }
    }
}

// ============================================================================
// Wire Protocol Helpers
// ============================================================================
