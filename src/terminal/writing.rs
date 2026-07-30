//! Writing to the screen: characters, scrolling, styling, modes.

use crate::buffer::Buffer;
use crate::cell::Cell;

use super::Terminal;

impl Terminal {
    /// Create the current cell template with colors and flags.
    pub(super) fn cell_template(&self, c: char) -> Cell {
        Cell {
            c,
            fg: self.fg,
            bg: self.bg,
            flags: self.flags,
            hyperlink: self.hyperlink,
        }
    }

    /// Write a character at the cursor position.
    pub fn put_char(&mut self, c: char) {
        // Handle pending wrap
        if self.pending_wrap {
            self.pending_wrap = false;
            self.buffer.set_row_wrapped(self.cursor.row, true);
            self.cursor.col = 0;
            self.linefeed();
        }

        // Write the character
        let cell = self.cell_template(c);
        self.buffer.set_cell(self.cursor.row, self.cursor.col, cell);

        // Advance cursor
        if self.cursor.col + 1 >= self.cols() {
            if self.auto_wrap {
                self.pending_wrap = true;
            }
        } else {
            self.cursor.col += 1;
        }
    }

    /// Perform a linefeed (move cursor down, scroll if needed).
    pub fn linefeed(&mut self) {
        if self.cursor.row + 1 < self.scroll_bottom {
            self.cursor.row += 1;
            return;
        }

        if self.scroll_top == 0 && self.scroll_bottom == self.rows() {
            // A full-screen scroll: the top row becomes history where it sits.
            // A pinned view keeps showing the same rows on its own, because it
            // holds an absolute position rather than an offset.
            self.buffer.scroll_up();
        } else {
            // A scroll region is a window inside the screen; nothing leaves it.
            self.buffer
                .scroll_region_up(self.scroll_top, self.scroll_bottom);
        }
    }

    /// Scroll the view through the scrollback.
    ///
    /// `lines` is positive to go back in history and negative to come forward;
    /// the result is clamped to the recorded scrollback. Returns whether the view
    /// moved.
    pub fn scroll_view(&mut self, lines: i32) -> bool {
        self.buffer.scroll_view(lines)
    }

    /// Jump back to the live view. Returns whether the view moved.
    pub fn reset_scroll(&mut self) -> bool {
        self.buffer.reset_scroll()
    }

    /// Check if we're viewing scrollback (not at bottom).
    pub fn is_scrolled(&self) -> bool {
        self.buffer.is_scrolled()
    }

    /// Move cursor to the next tab stop.
    pub(super) fn tab(&mut self) {
        let cols = self.cols();
        let mut col = self.cursor.col + 1;
        while col < cols && !self.tabs[col] {
            col += 1;
        }
        self.cursor.col = col.min(cols - 1);
    }

    /// Switch to alternate screen buffer.
    pub(super) fn enter_alt_screen(&mut self) {
        if !self.alt_screen {
            let rows = self.rows();
            let cols = self.cols();
            let alt = Buffer::new(rows, cols, 0);
            self.alt_primary = Some(std::mem::replace(&mut self.buffer, alt));
            self.alt_screen = true;
        }
    }

    /// Switch back to main screen buffer.
    pub(super) fn exit_alt_screen(&mut self) {
        if self.alt_screen {
            if let Some(primary) = self.alt_primary.take() {
                self.buffer = primary;
                self.buffer.mark_all_dirty();
            }
            self.alt_screen = false;
        }
    }
}
