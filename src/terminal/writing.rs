//! Writing to the screen: characters, scrolling, styling, modes.

use crate::buffer::Buffer;
use crate::cell::{Cell, CellFlags, Color};

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

    /// Handle SGR (Select Graphic Rendition) parameters.
    pub(super) fn handle_sgr(&mut self, params: &[u16]) {
        let mut iter = params.iter().copied().peekable();

        while let Some(param) = iter.next() {
            match param {
                0 => {
                    // Reset
                    self.fg = Color::default();
                    self.bg = Color::default();
                    self.flags = CellFlags::empty();
                }
                1 => self.flags.insert(CellFlags::BOLD),
                2 => self.flags.insert(CellFlags::DIM),
                3 => self.flags.insert(CellFlags::ITALIC),
                4 => self.flags.insert(CellFlags::UNDERLINE),
                5 => self.flags.insert(CellFlags::BLINK),
                7 => self.flags.insert(CellFlags::INVERSE),
                8 => self.flags.insert(CellFlags::HIDDEN),
                9 => self.flags.insert(CellFlags::STRIKETHROUGH),
                21 => self.flags.remove(CellFlags::BOLD),
                22 => {
                    self.flags.remove(CellFlags::BOLD);
                    self.flags.remove(CellFlags::DIM);
                }
                23 => self.flags.remove(CellFlags::ITALIC),
                24 => self.flags.remove(CellFlags::UNDERLINE),
                25 => self.flags.remove(CellFlags::BLINK),
                27 => self.flags.remove(CellFlags::INVERSE),
                28 => self.flags.remove(CellFlags::HIDDEN),
                29 => self.flags.remove(CellFlags::STRIKETHROUGH),
                // Standard foreground colors (30-37)
                30..=37 => {
                    if let Some(color) = Color::from_ansi(param) {
                        self.fg = color;
                    }
                }
                // 256-color foreground (38;5;n)
                38 => {
                    if iter.next() == Some(5) {
                        if let Some(n) = iter.next() {
                            self.fg = Color::indexed(n as u8);
                        }
                    } else if iter.peek() == Some(&2) {
                        // True color (38;2;r;g;b)
                        iter.next(); // consume 2
                        let r = iter.next().unwrap_or(0) as u8;
                        let g = iter.next().unwrap_or(0) as u8;
                        let b = iter.next().unwrap_or(0) as u8;
                        self.fg = Color::rgb(r, g, b);
                    }
                }
                // Default foreground
                39 => self.fg = Color::default(),
                // Standard background colors (40-47)
                40..=47 => {
                    if let Some(color) = Color::from_ansi(param) {
                        self.bg = color;
                    }
                }
                // 256-color background (48;5;n)
                48 => {
                    if iter.next() == Some(5) {
                        if let Some(n) = iter.next() {
                            self.bg = Color::indexed(n as u8);
                        }
                    } else if iter.peek() == Some(&2) {
                        // True color (48;2;r;g;b)
                        iter.next(); // consume 2
                        let r = iter.next().unwrap_or(0) as u8;
                        let g = iter.next().unwrap_or(0) as u8;
                        let b = iter.next().unwrap_or(0) as u8;
                        self.bg = Color::rgb(r, g, b);
                    }
                }
                // Default background
                49 => self.bg = Color::default(),
                // Bright foreground colors (90-97)
                90..=97 => {
                    if let Some(color) = Color::from_ansi(param) {
                        self.fg = color;
                    }
                }
                // Bright background colors (100-107)
                100..=107 => {
                    if let Some(color) = Color::from_ansi(param) {
                        self.bg = color;
                    }
                }
                _ => {}
            }
        }
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
