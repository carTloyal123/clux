//! Terminal state machine.
//!
//! Implements the VTE Perform trait to handle ANSI escape sequences.
//! Manages cursor position, colors, and grid updates.

use crate::buffer::Buffer;
use crate::cell::{CellFlags, Color, HyperlinkId};
use crate::hyperlink::HyperlinkStore;

/// Memory a pane may spend on history.
///
/// 16 MB is about 8,700 rows at 80 columns with today's 24-byte cell - close to the
/// 10,000 rows this used to keep, but as a ceiling that holds when the window is
/// made wider.
pub const DEFAULT_SCROLLBACK_BYTES: usize = 16 * 1024 * 1024;

mod csi;
mod cursor;
mod modes;
mod osc;
mod perform;
mod resize;
#[cfg(test)]
mod resize_tests;
#[cfg(test)]
mod scroll_tests;
mod sgr;
#[cfg(test)]
mod test_support;
#[cfg(test)]
mod tests;
mod writing;

pub use cursor::Cursor;

/// Terminal state machine implementing VTE's Perform trait.
pub struct Terminal {
    /// Content: history plus the active screen.
    buffer: Buffer,
    /// Current cursor position.
    pub cursor: Cursor,
    /// Hyperlink store for URL interning.
    pub hyperlinks: HyperlinkStore,
    /// Current foreground color.
    fg: Color,
    /// Current background color.
    bg: Color,
    /// Current cell flags.
    flags: CellFlags,
    /// Current hyperlink ID for new cells.
    hyperlink: Option<HyperlinkId>,
    /// Scroll region top (inclusive).
    scroll_top: usize,
    /// Scroll region bottom (exclusive).
    scroll_bottom: usize,
    /// Origin mode (DECOM) - cursor relative to scroll region.
    origin_mode: bool,
    /// Auto-wrap mode.
    auto_wrap: bool,
    /// Pending wrap - cursor at end of line, waiting for next char.
    pending_wrap: bool,
    /// Saved primary content while the alternate screen is active.
    alt_primary: Option<Buffer>,
    /// Whether we're on the alternate screen.
    alt_screen: bool,
    /// Tab stops.
    tabs: Vec<bool>,
    /// Mouse tracking mode (0=off, 1000=normal, 1002=button, 1003=any)
    mouse_mode: u16,
    /// SGR mouse encoding (mode 1006)
    sgr_mouse: bool,
}

impl Terminal {
    /// Create a new terminal with the given dimensions.
    pub fn new(rows: usize, cols: usize) -> Self {
        Self::with_scrollback(rows, cols, DEFAULT_SCROLLBACK_BYTES)
    }

    /// Create a new terminal with a custom history budget, in bytes.
    pub fn with_scrollback(rows: usize, cols: usize, scrollback_bytes: usize) -> Self {
        let mut tabs = vec![false; cols];
        // Default tab stops every 8 columns
        for i in (0..cols).step_by(8) {
            tabs[i] = true;
        }

        Self {
            buffer: Buffer::new(rows, cols, scrollback_bytes),
            cursor: Cursor::default(),
            hyperlinks: HyperlinkStore::new(),
            fg: Color::default(),
            bg: Color::default(),
            flags: CellFlags::empty(),
            hyperlink: None,
            scroll_top: 0,
            scroll_bottom: rows,
            origin_mode: false,
            auto_wrap: true,
            pending_wrap: false,
            alt_primary: None,
            alt_screen: false,
            tabs,
            mouse_mode: 0,
            sgr_mouse: false,
        }
    }

    /// Get the number of rows.
    pub fn rows(&self) -> usize {
        self.buffer.screen_rows()
    }

    /// Get the number of columns.
    pub fn cols(&self) -> usize {
        self.buffer.cols()
    }

    /// Rows of history above the screen.
    pub fn history_rows(&self) -> usize {
        self.buffer.history_rows()
    }

    /// Check if this terminal wants mouse events.
    pub fn wants_mouse(&self) -> bool {
        self.mouse_mode != 0
    }

    /// Get the mouse tracking mode (0, 1000, 1002, or 1003).
    pub fn mouse_mode(&self) -> u16 {
        self.mouse_mode
    }

    /// Check if SGR mouse encoding is enabled.
    pub fn sgr_mouse(&self) -> bool {
        self.sgr_mouse
    }

    /// Get the cursor state.
    pub fn cursor(&self) -> &Cursor {
        &self.cursor
    }

    /// Take dirty row indices and clear the dirty flags.
    pub fn take_dirty_rows(&mut self) -> Vec<u16> {
        self.buffer.take_dirty_rows()
    }
}
