//! Terminal state machine.
//!
//! Implements the VTE Perform trait to handle ANSI escape sequences.
//! Manages cursor position, colors, and grid updates.

use std::collections::HashMap;

use crate::buffer::{Buffer, ViewRow};
use crate::cell::{Cell, CellFlags, Color, HyperlinkId};
use crate::hyperlink::HyperlinkStore;
use crate::urls::LinkRun;

/// Memory a pane may spend on history.
///
/// 16 MB is about 8,700 rows at 80 columns with today's 24-byte cell - close to the
/// 10,000 rows this used to keep, but as a ceiling that holds when the window is
/// made wider.
pub const DEFAULT_SCROLLBACK_BYTES: usize = 16 * 1024 * 1024;

/// Cursor position and state.
#[derive(Clone, Copy, Debug)]
pub struct Cursor {
    /// Row position (0-indexed).
    pub row: usize,
    /// Column position (0-indexed).
    pub col: usize,
    /// Whether the cursor is visible.
    pub visible: bool,
    /// Saved cursor position for DECSC/DECRC.
    saved: Option<(usize, usize)>,
}

impl Default for Cursor {
    fn default() -> Self {
        Self {
            row: 0,
            col: 0,
            visible: true,
            saved: None,
        }
    }
}

impl Cursor {
    /// Save the current cursor position.
    pub fn save(&mut self) {
        self.saved = Some((self.row, self.col));
    }

    /// Restore the saved cursor position.
    pub fn restore(&mut self) {
        if let Some((row, col)) = self.saved {
            self.row = row;
            self.col = col;
        }
    }
}

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

    /// The row the user sees at this screen position.
    ///
    /// Comes from the scrollback when the view is scrolled back, otherwise from
    /// the live grid - see [`crate::scrollview`]. Everything that serializes a
    /// pane row goes through here so the two cannot drift apart.
    pub fn view_row(&self, row_idx: u16) -> ViewRow {
        self.buffer.view_row(row_idx)
    }

    /// Resolve the hyperlinks covering `rows`, following soft-wrap continuations.
    ///
    /// Resolves against whatever the pane is showing, so links keep working while
    /// scrolled back through history. `salt` scopes the generated OSC 8 ids to this
    /// pane. The result can cover rows outside `rows` when a link wraps onto them;
    /// those rows need repainting too. See [`crate::urls`] for why the
    /// multiplexer, not the outer terminal, has to do this.
    pub fn resolve_links(
        &self,
        salt: u32,
        detect_plain_urls: bool,
        rows: &[u16],
    ) -> HashMap<u16, Vec<LinkRun>> {
        crate::urls::resolve_links(
            &self.buffer,
            &self.hyperlinks,
            salt,
            detect_plain_urls,
            rows,
        )
    }

    /// Resize the terminal.
    ///
    /// The buffer re-wraps its content, history included, and reports where the
    /// cursor's character ended up.
    pub fn resize(&mut self, rows: usize, cols: usize) {
        let (cursor_row, cursor_col) =
            self.buffer
                .resize(rows, cols, (self.cursor.row, self.cursor.col));
        self.cursor.row = cursor_row.min(rows.saturating_sub(1));
        self.cursor.col = cursor_col.min(cols.saturating_sub(1));

        if let Some(alt) = self.alt_primary.as_mut() {
            alt.resize(rows, cols, (0, 0));
        }

        // Update scroll region
        self.scroll_bottom = rows;
        if self.scroll_top >= rows {
            self.scroll_top = 0;
        }

        // Update tab stops
        self.tabs.resize(cols, false);
        for i in (0..cols).step_by(8) {
            self.tabs[i] = true;
        }
    }

    /// Create the current cell template with colors and flags.
    fn cell_template(&self, c: char) -> Cell {
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
    fn tab(&mut self) {
        let cols = self.cols();
        let mut col = self.cursor.col + 1;
        while col < cols && !self.tabs[col] {
            col += 1;
        }
        self.cursor.col = col.min(cols - 1);
    }

    /// Handle SGR (Select Graphic Rendition) parameters.
    fn handle_sgr(&mut self, params: &[u16]) {
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
    fn enter_alt_screen(&mut self) {
        if !self.alt_screen {
            let rows = self.rows();
            let cols = self.cols();
            let alt = Buffer::new(rows, cols, 0);
            self.alt_primary = Some(std::mem::replace(&mut self.buffer, alt));
            self.alt_screen = true;
        }
    }

    /// Switch back to main screen buffer.
    fn exit_alt_screen(&mut self) {
        if self.alt_screen {
            if let Some(primary) = self.alt_primary.take() {
                self.buffer = primary;
                self.buffer.mark_all_dirty();
            }
            self.alt_screen = false;
        }
    }
}

impl vte::Perform for Terminal {
    fn print(&mut self, c: char) {
        self.put_char(c);
    }

    fn execute(&mut self, byte: u8) {
        match byte {
            0x07 => {
                // BEL - bell, ignored for now
            }
            0x08 => {
                // BS - backspace
                self.cursor.col = self.cursor.col.saturating_sub(1);
                self.pending_wrap = false;
            }
            0x09 => {
                // HT - horizontal tab
                self.tab();
            }
            0x0A | 0x0B | 0x0C => {
                // LF, VT, FF - line feed
                self.linefeed();
            }
            0x0D => {
                // CR - carriage return
                self.cursor.col = 0;
                self.pending_wrap = false;
            }
            _ => {}
        }
    }

    fn hook(&mut self, _params: &vte::Params, _intermediates: &[u8], _ignore: bool, _action: char) {
        // DCS sequences - not implemented yet
    }

    fn put(&mut self, _byte: u8) {
        // DCS data - not implemented yet
    }

    fn unhook(&mut self) {
        // End DCS - not implemented yet
    }

    fn osc_dispatch(&mut self, params: &[&[u8]], _bell_terminated: bool) {
        if params.is_empty() {
            return;
        }

        // Parse first parameter as command number
        let cmd = std::str::from_utf8(params[0])
            .ok()
            .and_then(|s| s.parse::<u32>().ok());

        match cmd {
            // Set window title (OSC 0, 1, 2)
            Some(0) | Some(1) | Some(2) => {
                // Title setting - we could emit an event here
            }
            // OSC 8 - hyperlinks
            Some(8) => {
                // OSC 8 format: ESC ] 8 ; params ; URI ST
                // params[0] = "8", params[1] = id/params, params[2..] = URI
                //
                // The parser splits on ';', but ';' is legal inside a URI
                // (matrix parameters, mailto headers), so rejoin the tail
                // instead of truncating the URL at the first one.
                if params.len() >= 2 {
                    let uri = if params.len() >= 3 {
                        join_semicolons(&params[2..])
                    } else {
                        // Empty URI closes hyperlink
                        std::str::from_utf8(params[1]).ok().map(str::to_string)
                    };

                    match uri.as_deref().and_then(crate::urls::sanitize_url) {
                        Some(url) => {
                            // Open hyperlink - intern the URL
                            let id = self.hyperlinks.intern(&url);
                            self.hyperlink = Some(id);
                        }
                        None => {
                            // Close hyperlink
                            self.hyperlink = None;
                        }
                    }
                } else {
                    // Malformed - close hyperlink
                    self.hyperlink = None;
                }
            }
            _ => {}
        }
    }

    fn csi_dispatch(
        &mut self,
        params: &vte::Params,
        intermediates: &[u8],
        _ignore: bool,
        action: char,
    ) {
        let params: Vec<u16> = params.iter().map(|p| p[0]).collect();
        let private = intermediates.first() == Some(&b'?');

        match (action, private) {
            // CUU - Cursor Up
            ('A', false) => {
                let n = params.first().copied().unwrap_or(1).max(1) as usize;
                self.cursor.row = self.cursor.row.saturating_sub(n);
                self.pending_wrap = false;
            }
            // CUD - Cursor Down
            ('B', false) => {
                let n = params.first().copied().unwrap_or(1).max(1) as usize;
                self.cursor.row = (self.cursor.row + n).min(self.rows() - 1);
                self.pending_wrap = false;
            }
            // CUF - Cursor Forward
            ('C', false) => {
                let n = params.first().copied().unwrap_or(1).max(1) as usize;
                self.cursor.col = (self.cursor.col + n).min(self.cols() - 1);
                self.pending_wrap = false;
            }
            // CUB - Cursor Backward
            ('D', false) => {
                let n = params.first().copied().unwrap_or(1).max(1) as usize;
                self.cursor.col = self.cursor.col.saturating_sub(n);
                self.pending_wrap = false;
            }
            // CNL - Cursor Next Line
            ('E', false) => {
                let n = params.first().copied().unwrap_or(1).max(1) as usize;
                self.cursor.row = (self.cursor.row + n).min(self.rows() - 1);
                self.cursor.col = 0;
                self.pending_wrap = false;
            }
            // CPL - Cursor Previous Line
            ('F', false) => {
                let n = params.first().copied().unwrap_or(1).max(1) as usize;
                self.cursor.row = self.cursor.row.saturating_sub(n);
                self.cursor.col = 0;
                self.pending_wrap = false;
            }
            // CHA - Cursor Horizontal Absolute
            ('G', false) => {
                let col = params.first().copied().unwrap_or(1).max(1) as usize - 1;
                self.cursor.col = col.min(self.cols() - 1);
                self.pending_wrap = false;
            }
            // CUP / HVP - Cursor Position
            ('H', false) | ('f', false) => {
                let row = params.first().copied().unwrap_or(1).max(1) as usize - 1;
                let col = params.get(1).copied().unwrap_or(1).max(1) as usize - 1;
                self.cursor.row = row.min(self.rows() - 1);
                self.cursor.col = col.min(self.cols() - 1);
                self.pending_wrap = false;
            }
            // ED - Erase in Display
            ('J', false) => {
                let mode = params.first().copied().unwrap_or(0);
                match mode {
                    0 => {
                        // Clear from cursor to end of screen
                        self.buffer.clear_below(self.cursor.row, self.cursor.col);
                    }
                    1 => {
                        // Clear from start of screen to cursor
                        self.buffer.clear_above(self.cursor.row, self.cursor.col);
                    }
                    2 | 3 => {
                        // Clear entire screen (3 also clears scrollback, but we don't have that yet)
                        self.buffer.clear_screen();
                    }
                    _ => {}
                }
            }
            // EL - Erase in Line
            ('K', false) => {
                let mode = params.first().copied().unwrap_or(0);
                let row = self.cursor.row;
                let cols = self.cols();
                match mode {
                    0 => self
                        .buffer
                        .clear_active_row_range(row, self.cursor.col, cols),
                    1 => self
                        .buffer
                        .clear_active_row_range(row, 0, self.cursor.col + 1),
                    2 => self.buffer.clear_active_row(row),
                    _ => {}
                }
            }
            // IL - Insert Lines
            ('L', false) => {
                let n = params.first().copied().unwrap_or(1).max(1) as usize;
                for _ in 0..n {
                    self.buffer
                        .scroll_region_down(self.cursor.row, self.scroll_bottom);
                }
            }
            // DL - Delete Lines
            ('M', false) => {
                let n = params.first().copied().unwrap_or(1).max(1) as usize;
                for _ in 0..n {
                    self.buffer
                        .scroll_region_up(self.cursor.row, self.scroll_bottom);
                }
            }
            // DCH - Delete Characters
            ('P', false) => {
                let _n = params.first().copied().unwrap_or(1).max(1) as usize;
                // TODO: Implement character deletion
            }
            // SU - Scroll Up
            ('S', false) => {
                let n = params.first().copied().unwrap_or(1).max(1) as usize;
                for _ in 0..n {
                    self.buffer
                        .scroll_region_up(self.scroll_top, self.scroll_bottom);
                }
            }
            // SD - Scroll Down
            ('T', false) => {
                let n = params.first().copied().unwrap_or(1).max(1) as usize;
                for _ in 0..n {
                    self.buffer
                        .scroll_region_down(self.scroll_top, self.scroll_bottom);
                }
            }
            // ICH - Insert Characters
            ('@', false) => {
                let _n = params.first().copied().unwrap_or(1).max(1) as usize;
                // TODO: Implement character insertion
            }
            // SGR - Select Graphic Rendition
            ('m', false) => {
                if params.is_empty() {
                    self.handle_sgr(&[0]);
                } else {
                    self.handle_sgr(&params);
                }
            }
            // DSR - Device Status Report
            ('n', false) => {
                // TODO: Respond to status queries
            }
            // DECSTBM - Set Top and Bottom Margins
            ('r', false) => {
                let top = params.first().copied().unwrap_or(1).max(1) as usize - 1;
                let bottom = params
                    .get(1)
                    .copied()
                    .map(|b| b as usize)
                    .unwrap_or(self.rows());
                if top < bottom && bottom <= self.rows() {
                    self.scroll_top = top;
                    self.scroll_bottom = bottom;
                    // Move cursor to home position
                    self.cursor.row = if self.origin_mode { top } else { 0 };
                    self.cursor.col = 0;
                }
            }
            // DECSC - Save Cursor
            ('s', false) => {
                self.cursor.save();
            }
            // DECRC - Restore Cursor
            ('u', false) => {
                self.cursor.restore();
            }
            // Private mode set/reset
            ('h', true) => {
                for &param in &params {
                    match param {
                        1 => {
                            // DECCKM - Application Cursor Keys
                        }
                        7 => {
                            // DECAWM - Auto-wrap mode
                            self.auto_wrap = true;
                        }
                        12 => {
                            // Cursor blink
                        }
                        25 => {
                            // DECTCEM - Show cursor
                            self.cursor.visible = true;
                        }
                        1000 => {
                            // X11 mouse reporting (normal tracking mode)
                            self.mouse_mode = 1000;
                        }
                        1002 => {
                            // X11 mouse reporting (button-event tracking)
                            self.mouse_mode = 1002;
                        }
                        1003 => {
                            // X11 mouse reporting (any-event tracking)
                            self.mouse_mode = 1003;
                        }
                        1006 => {
                            // SGR mouse encoding
                            self.sgr_mouse = true;
                        }
                        1049 => {
                            // Alternate screen buffer
                            self.enter_alt_screen();
                        }
                        _ => {}
                    }
                }
            }
            ('l', true) => {
                for &param in &params {
                    match param {
                        7 => {
                            self.auto_wrap = false;
                        }
                        25 => {
                            self.cursor.visible = false;
                        }
                        1000 | 1002 | 1003 => {
                            // Disable mouse tracking
                            self.mouse_mode = 0;
                        }
                        1006 => {
                            // Disable SGR mouse encoding
                            self.sgr_mouse = false;
                        }
                        1049 => {
                            self.exit_alt_screen();
                        }
                        _ => {}
                    }
                }
            }
            _ => {}
        }
    }

    fn esc_dispatch(&mut self, intermediates: &[u8], _ignore: bool, byte: u8) {
        match (byte, intermediates) {
            // RIS - Reset to Initial State
            (b'c', []) => {
                let rows = self.rows();
                let cols = self.cols();
                *self = Terminal::new(rows, cols);
            }
            // DECSC - Save Cursor (ESC 7)
            (b'7', []) => {
                self.cursor.save();
            }
            // DECRC - Restore Cursor (ESC 8)
            (b'8', []) => {
                self.cursor.restore();
            }
            // IND - Index (move down one line, scroll if needed)
            (b'D', []) => {
                self.linefeed();
            }
            // NEL - Next Line
            (b'E', []) => {
                self.cursor.col = 0;
                self.linefeed();
            }
            // RI - Reverse Index (move up one line, scroll if needed)
            (b'M', []) => {
                if self.cursor.row == self.scroll_top {
                    self.buffer
                        .scroll_region_down(self.scroll_top, self.scroll_bottom);
                } else {
                    self.cursor.row = self.cursor.row.saturating_sub(1);
                }
            }
            _ => {}
        }
    }
}

/// Rejoin OSC parameters that the parser split on a ';' belonging to the payload.
fn join_semicolons(params: &[&[u8]]) -> Option<String> {
    let parts: Option<Vec<&str>> = params.iter().map(|p| std::str::from_utf8(p).ok()).collect();

    Some(parts?.join(";"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cell::ColorKind;

    #[test]
    fn test_terminal_creation() {
        let term = Terminal::new(24, 80);
        assert_eq!(term.rows(), 24);
        assert_eq!(term.cols(), 80);
        assert_eq!(term.cursor.row, 0);
        assert_eq!(term.cursor.col, 0);
    }

    #[test]
    fn test_put_char() {
        let mut term = Terminal::new(24, 80);
        term.put_char('A');
        assert_eq!(term.buffer.cell(0, 0).unwrap().c, 'A');
        assert_eq!(term.cursor.col, 1);
    }

    #[test]
    fn test_linefeed() {
        let mut term = Terminal::new(24, 80);
        term.cursor.row = 5;
        term.linefeed();
        assert_eq!(term.cursor.row, 6);
    }

    #[test]
    fn test_cursor_movement_via_parser() {
        let mut term = Terminal::new(24, 80);
        let mut parser = vte::Parser::new();

        term.cursor.row = 10;
        term.cursor.col = 10;

        // CSI A = cursor up: ESC [ A
        let seq = b"\x1b[A";
        parser.advance(&mut term, seq);
        assert_eq!(term.cursor.row, 9);
    }

    /// Feed bytes through a real VTE parser, as the server does.
    fn feed(term: &mut Terminal, bytes: &[u8]) {
        let mut parser = vte::Parser::new();
        parser.advance(term, bytes);
    }

    /// URL of the hyperlink on the cell at (row, col), if any.
    fn link_at(term: &Terminal, row: usize, col: usize) -> Option<&str> {
        let id = term.buffer.cell(row, col)?.hyperlink?;
        term.hyperlinks.get(id)
    }

    #[test]
    fn test_osc8_hyperlink_marks_cells() {
        let mut term = Terminal::new(24, 80);
        feed(
            &mut term,
            b"\x1b]8;;https://example.com\x1b\\link\x1b]8;;\x1b\\ plain",
        );

        assert_eq!(link_at(&term, 0, 0), Some("https://example.com"));
        assert_eq!(link_at(&term, 0, 3), Some("https://example.com"));
        assert_eq!(link_at(&term, 0, 4), None, "closed link leaked");
    }

    #[test]
    fn test_osc8_keeps_semicolons_in_url() {
        let mut term = Terminal::new(24, 80);
        // ';' is legal in a URI; the parser splits on it, so it must be rejoined.
        feed(
            &mut term,
            b"\x1b]8;;https://example.com/a;b=1;c=2\x1b\\x\x1b]8;;\x1b\\",
        );

        assert_eq!(link_at(&term, 0, 0), Some("https://example.com/a;b=1;c=2"));
    }

    #[test]
    fn test_osc8_rejects_control_characters_in_url() {
        let mut term = Terminal::new(24, 80);
        // A URL is re-emitted inside OSC 8, so control bytes must never survive.
        feed(&mut term, b"\x1b]8;;https://a.io/\x07x\x1b\\y");

        let url = link_at(&term, 0, 0).expect("link");
        assert!(
            !url.chars().any(|c| c.is_control()),
            "control char in {url:?}"
        );
    }

    #[test]
    fn test_sgr() {
        let mut term = Terminal::new(24, 80);

        // Set bold
        term.handle_sgr(&[1]);
        assert!(term.flags.contains(CellFlags::BOLD));

        // Reset
        term.handle_sgr(&[0]);
        assert!(!term.flags.contains(CellFlags::BOLD));

        // Set foreground color
        term.handle_sgr(&[31]);
        assert_eq!(term.fg.kind, ColorKind::Indexed);
    }

    #[test]
    fn test_resize() {
        let mut term = Terminal::new(24, 80);
        term.put_char('A');
        term.cursor.row = 10;
        term.cursor.col = 40;

        term.resize(48, 120);
        assert_eq!(term.rows(), 48);
        assert_eq!(term.cols(), 120);
        // Cursor should be preserved
        assert_eq!(term.cursor.row, 10);
        assert_eq!(term.cursor.col, 40);
    }

    #[test]
    fn test_scroll_offset_preserved_on_resize() {
        let mut term = Terminal::new(24, 80);

        // Fill terminal with content and force scrollback by going past the bottom
        // First fill the screen
        for row in 0..24 {
            term.cursor.row = row;
            term.cursor.col = 0;
            for c in format!("Line {}", row).chars() {
                term.put_char(c);
            }
        }

        // Now add more lines to push content into scrollback
        // Each linefeed at row 23 will scroll content up
        for i in 0..10 {
            term.cursor.row = 23;
            term.cursor.col = 0;
            term.linefeed(); // This pushes row 0 to scrollback
            for c in format!("New line {}", i).chars() {
                term.put_char(c);
            }
        }

        // Verify scrollback has content
        assert!(term.history_rows() >= 5, "Scrollback should have content");

        // Scroll back into history (positive = older)
        term.scroll_view(5);
        assert_eq!(
            term.buffer.scroll_offset(),
            5,
            "Should be scrolled up 5 lines"
        );

        // Resize (same size to test offset preservation)
        term.resize(24, 80);

        // Scroll offset should be preserved
        assert_eq!(
            term.buffer.scroll_offset(),
            5,
            "Scroll offset should be preserved after resize"
        );
    }

    /// Fill the scrollback by pushing `count` lines out of the top of the grid.
    fn fill_scrollback(term: &mut Terminal, count: usize) {
        for i in 0..count {
            term.cursor.row = term.rows() - 1;
            term.cursor.col = 0;
            term.linefeed();
            for c in format!("line {}", i).chars() {
                term.put_char(c);
            }
        }
    }

    #[test]
    fn test_scroll_view_clamps_to_recorded_history() {
        let mut term = Terminal::new(24, 80);
        fill_scrollback(&mut term, 10);

        // Cannot scroll past the oldest recorded line...
        term.scroll_view(1000);
        assert_eq!(term.buffer.scroll_offset(), term.history_rows());

        // ...nor forward past the live view.
        term.scroll_view(-1000);
        assert_eq!(term.buffer.scroll_offset(), 0);
        assert!(!term.is_scrolled());
    }

    #[test]
    fn test_scroll_view_reports_whether_it_moved() {
        let mut term = Terminal::new(24, 80);
        fill_scrollback(&mut term, 4);

        assert!(term.scroll_view(2), "moving back should report a change");
        assert!(!term.scroll_view(0), "a zero move changes nothing");
        assert!(term.reset_scroll(), "returning to live is a change");
        assert!(!term.reset_scroll(), "already live, nothing to do");
    }

    #[test]
    fn test_scrolled_view_stays_pinned_as_output_arrives() {
        let mut term = Terminal::new(24, 80);
        fill_scrollback(&mut term, 10);

        term.scroll_view(5);
        let pinned = term.view_row(0);

        // More output pushes another line into the scrollback; the view must show
        // the same content rather than drifting a line at a time.
        fill_scrollback(&mut term, 1);
        assert_eq!(term.buffer.scroll_offset(), 6);
        assert_eq!(term.view_row(0), pinned);
    }

    #[test]
    fn test_view_row_reads_history_when_scrolled() {
        let mut term = Terminal::new(24, 80);
        fill_scrollback(&mut term, 30);

        let live_top: String = term.view_row(0).cells.iter().map(|c| c.c).collect();
        term.scroll_view(3);
        let scrolled_top: String = term.view_row(0).cells.iter().map(|c| c.c).collect();

        assert_ne!(
            live_top.trim(),
            scrolled_top.trim(),
            "scrolling should show different content"
        );
    }

    #[test]
    fn test_links_resolve_in_scrolled_back_history() {
        let mut term = Terminal::new(4, 40);

        // Put a URL on screen, then push it up into the scrollback.
        feed(&mut term, b"see https://example.com/history\n");
        fill_scrollback(&mut term, 8);

        let live = term.resolve_links(1, true, &[0, 1, 2, 3]);
        assert!(
            live.is_empty(),
            "the URL should have scrolled off the live view: {live:?}"
        );

        // Scroll back far enough to bring it into view.
        let mut found = None;
        for offset in 1..=term.history_rows() as i32 {
            term.reset_scroll();
            term.scroll_view(offset);
            let rows: Vec<u16> = (0..term.rows() as u16).collect();
            let links = term.resolve_links(1, true, &rows);
            if let Some(run) = links.values().flatten().next() {
                found = Some(run.url.clone());
                break;
            }
        }

        assert_eq!(
            found.as_deref(),
            Some("https://example.com/history"),
            "a URL in the scrollback should still resolve to a link"
        );
    }

    #[test]
    fn test_explicit_osc8_links_resolve_in_history() {
        let mut term = Terminal::new(4, 40);
        feed(
            &mut term,
            b"\x1b]8;;https://example.com/app\x1b\\CLICKME\x1b]8;;\x1b\\\n",
        );
        fill_scrollback(&mut term, 8);

        let mut found = None;
        for offset in 1..=term.history_rows() as i32 {
            term.reset_scroll();
            term.scroll_view(offset);
            let rows: Vec<u16> = (0..term.rows() as u16).collect();
            let links = term.resolve_links(1, true, &rows);
            if let Some(run) = links.values().flatten().find(|r| !r.detected) {
                found = Some(run.url.clone());
                break;
            }
        }

        assert_eq!(
            found.as_deref(),
            Some("https://example.com/app"),
            "an application's OSC 8 link should survive into the scrollback"
        );
    }

    #[test]
    fn test_resize_shrink_preserves_content_via_scrollback() {
        let mut term = Terminal::new(24, 80);

        // Fill terminal with content - put cursor at row 20
        for row in 0..21 {
            term.cursor.row = row;
            term.cursor.col = 0;
            for c in format!("Line {}", row).chars() {
                term.put_char(c);
            }
        }
        term.cursor.row = 20;
        term.cursor.col = 7; // After "Line 20"

        // Verify initial state
        assert_eq!(term.history_rows(), 0, "No scrollback yet");
        assert_eq!(term.cursor.row, 20);

        // Now resize to only 10 rows - cursor at row 20 would be out of bounds
        term.resize(10, 80);

        // Cursor should now be within bounds
        assert!(
            term.cursor.row < 10,
            "Cursor row {} should be < 10 after resize",
            term.cursor.row
        );

        // Content should have been pushed to scrollback
        assert!(
            term.history_rows() > 0,
            "Scrollback should have content after shrinking with cursor below new height"
        );

        // The content from the top rows should now be in scrollback
        // We scrolled up (20 - 10 + 1 = 11) rows to bring cursor into view
        assert!(
            term.history_rows() >= 11,
            "Scrollback should have at least 11 lines, got {}",
            term.history_rows()
        );
    }

    #[test]
    fn test_resize_shrink_cursor_in_bounds_no_scroll() {
        let mut term = Terminal::new(24, 80);

        // Put content at the top, cursor at row 5
        for row in 0..6 {
            term.cursor.row = row;
            term.cursor.col = 0;
            for c in format!("Line {}", row).chars() {
                term.put_char(c);
            }
        }
        term.cursor.row = 5;
        term.cursor.col = 7;

        // Verify initial state
        assert_eq!(term.history_rows(), 0, "No scrollback yet");

        // Resize to 10 rows - cursor at row 5 is still within bounds
        term.resize(10, 80);

        // Cursor should remain at row 5
        assert_eq!(term.cursor.row, 5, "Cursor should stay at row 5");

        // No scrollback needed since cursor was in bounds
        assert_eq!(
            term.history_rows(),
            0,
            "No scrollback needed when cursor stays in bounds"
        );

        // Content should still be there
        assert_eq!(
            term.buffer.cell(0, 0).map(|c| c.c),
            Some('L'),
            "Content at row 0 preserved"
        );
    }
}
