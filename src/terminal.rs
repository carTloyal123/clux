//! Terminal state machine.
//!
//! Implements the VTE Perform trait to handle ANSI escape sequences.
//! Manages cursor position, colors, and grid updates.

use crate::cell::{Cell, CellFlags, Color, HyperlinkId};
use crate::grid::Grid;
use crate::hyperlink::HyperlinkStore;
use crate::scrollback::Scrollback;

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
    /// The terminal grid.
    pub grid: Grid,
    /// Current cursor position.
    pub cursor: Cursor,
    /// Scrollback buffer for history.
    pub scrollback: Scrollback,
    /// Current scroll offset for viewing history (0 = at bottom).
    pub scroll_offset: usize,
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
    /// Alternate screen buffer.
    alt_grid: Option<Grid>,
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
        Self::with_scrollback(rows, cols, 10_000)
    }

    /// Create a new terminal with custom scrollback size.
    pub fn with_scrollback(rows: usize, cols: usize, scrollback_lines: usize) -> Self {
        let mut tabs = vec![false; cols];
        // Default tab stops every 8 columns
        for i in (0..cols).step_by(8) {
            tabs[i] = true;
        }

        Self {
            grid: Grid::new(rows, cols),
            cursor: Cursor::default(),
            scrollback: Scrollback::new(scrollback_lines),
            scroll_offset: 0,
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
            alt_grid: None,
            alt_screen: false,
            tabs,
            mouse_mode: 0,
            sgr_mouse: false,
        }
    }

    /// Get the number of rows.
    pub fn rows(&self) -> usize {
        self.grid.rows()
    }

    /// Get the number of columns.
    pub fn cols(&self) -> usize {
        self.grid.cols()
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
        let dirty: Vec<u16> = self.grid.dirty_row_indices().map(|i| i as u16).collect();
        self.grid.clear_all_dirty();
        dirty
    }

    /// Render a row to a string (ANSI escape sequences for styling).
    pub fn render_row(&self, row_idx: u16) -> String {
        use crate::cell::ColorKind;

        let row = match self.grid.row(row_idx as usize) {
            Some(r) => r,
            None => {
                log::warn!(
                    "render_row: row {} out of bounds (grid has {} rows)",
                    row_idx,
                    self.grid.rows()
                );
                return String::new();
            }
        };

        let cell_count = row.iter().count();

        let mut output = String::new();
        let mut last_fg = Color::default();
        let mut last_bg = Color::default();
        let mut last_flags = CellFlags::empty();

        for cell in row.iter() {
            // Check if style changed
            if cell.fg != last_fg || cell.bg != last_bg || cell.flags != last_flags {
                // Reset and apply new style
                output.push_str("\x1b[0m");

                // Apply foreground color
                match cell.fg.kind {
                    ColorKind::Default => {}
                    ColorKind::Indexed => {
                        let n = cell.fg.r;
                        if n < 8 {
                            output.push_str(&format!("\x1b[{}m", 30 + n));
                        } else if n < 16 {
                            output.push_str(&format!("\x1b[{}m", 90 + n - 8));
                        } else {
                            output.push_str(&format!("\x1b[38;5;{}m", n));
                        }
                    }
                    ColorKind::Rgb => {
                        output.push_str(&format!(
                            "\x1b[38;2;{};{};{}m",
                            cell.fg.r, cell.fg.g, cell.fg.b
                        ));
                    }
                }

                // Apply background color
                match cell.bg.kind {
                    ColorKind::Default => {}
                    ColorKind::Indexed => {
                        let n = cell.bg.r;
                        if n < 8 {
                            output.push_str(&format!("\x1b[{}m", 40 + n));
                        } else if n < 16 {
                            output.push_str(&format!("\x1b[{}m", 100 + n - 8));
                        } else {
                            output.push_str(&format!("\x1b[48;5;{}m", n));
                        }
                    }
                    ColorKind::Rgb => {
                        output.push_str(&format!(
                            "\x1b[48;2;{};{};{}m",
                            cell.bg.r, cell.bg.g, cell.bg.b
                        ));
                    }
                }

                // Apply flags
                if cell.flags.contains(CellFlags::BOLD) {
                    output.push_str("\x1b[1m");
                }
                if cell.flags.contains(CellFlags::ITALIC) {
                    output.push_str("\x1b[3m");
                }
                if cell.flags.contains(CellFlags::UNDERLINE) {
                    output.push_str("\x1b[4m");
                }
                if cell.flags.contains(CellFlags::INVERSE) {
                    output.push_str("\x1b[7m");
                }
                if cell.flags.contains(CellFlags::DIM) {
                    output.push_str("\x1b[2m");
                }

                last_fg = cell.fg;
                last_bg = cell.bg;
                last_flags = cell.flags;
            }

            output.push(cell.c);
        }

        // Reset at end of line
        output.push_str("\x1b[0m");

        log::trace!(
            "render_row {}: {} cells -> {} chars output",
            row_idx,
            cell_count,
            output.len()
        );

        output
    }

    /// Render a row as plain text (no ANSI escape sequences).
    /// Used for compositing multiple panes into a single screen buffer.
    pub fn render_row_plain(&self, row_idx: u16) -> String {
        let row = match self.grid.row(row_idx as usize) {
            Some(r) => r,
            None => return String::new(),
        };

        row.iter().map(|cell| cell.c).collect()
    }

    /// Get cells for a row (for styled compositing).
    pub fn get_row_cells(&self, row_idx: u16) -> Vec<Cell> {
        let row = match self.grid.row(row_idx as usize) {
            Some(r) => r,
            None => return Vec::new(),
        };

        row.iter().cloned().collect()
    }

    /// Resize the terminal.
    pub fn resize(&mut self, rows: usize, cols: usize) {
        let old_rows = self.grid.rows();
        let old_scroll_offset = self.scroll_offset;

        // For main screen (not alt screen), use reflow-aware resize
        if !self.alt_screen {
            // When shrinking height and cursor would be out of bounds,
            // first push overflow content to scrollback
            if rows < old_rows && self.cursor.row >= rows {
                let scroll_amount = self.cursor.row - rows + 1;

                for _ in 0..scroll_amount {
                    if let Some(row) = self.grid.row(0) {
                        let cells: Vec<Cell> = (0..self.grid.cols())
                            .filter_map(|c| row.get(c).copied())
                            .collect();
                        let wrapped = row.is_wrapped();
                        self.scrollback.push(cells, wrapped);
                    }
                    self.grid.scroll_up();
                }

                self.cursor.row = self.cursor.row.saturating_sub(scroll_amount);
            }

            // Use reflow-aware resize for width changes
            let (new_cursor_row, new_cursor_col) =
                self.grid
                    .resize_with_reflow(rows, cols, self.cursor.row, self.cursor.col);
            self.cursor.row = new_cursor_row;
            self.cursor.col = new_cursor_col;
        } else {
            // Alt screen: simple resize without reflow
            self.grid.resize(rows, cols);
        }

        if let Some(ref mut alt) = self.alt_grid {
            alt.resize(rows, cols);
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

        // Ensure cursor is within bounds
        self.cursor.row = self.cursor.row.min(rows.saturating_sub(1));
        self.cursor.col = self.cursor.col.min(cols.saturating_sub(1));

        // Preserve scroll offset (clamp to valid range if needed)
        let max_offset = self.scrollback.len();
        self.scroll_offset = old_scroll_offset.min(max_offset);
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
            if let Some(row) = self.grid.row_mut(self.cursor.row) {
                row.set_wrapped(true);
            }
            self.cursor.col = 0;
            self.linefeed();
        }

        // Write the character
        let cell = self.cell_template(c);
        self.grid.set(self.cursor.row, self.cursor.col, cell);

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
        if self.cursor.row + 1 >= self.scroll_bottom {
            // At bottom of scroll region - scroll up
            // Save the line being scrolled out to scrollback (only for main screen)
            if !self.alt_screen && self.scroll_top == 0 {
                if let Some(row) = self.grid.row(0) {
                    let cells: Vec<Cell> = (0..self.cols())
                        .filter_map(|c| row.get(c).copied())
                        .collect();
                    let wrapped = row.is_wrapped();
                    self.scrollback.push(cells, wrapped);
                }
            }
            self.grid
                .scroll_region_up(self.scroll_top, self.scroll_bottom);
        } else {
            self.cursor.row += 1;
        }
    }

    /// Scroll the view for scrollback navigation.
    /// Positive delta scrolls up (into history), negative scrolls down.
    pub fn scroll_view(&mut self, delta: i32) {
        // delta < 0 means scroll up (view older content, increase offset)
        // delta > 0 means scroll down (view newer content, decrease offset)
        let max_offset = self.scrollback.len();
        let new_offset = (self.scroll_offset as i32 - delta)
            .max(0)
            .min(max_offset as i32) as usize;

        if new_offset != self.scroll_offset {
            self.scroll_offset = new_offset;
            // Mark all rows dirty for re-render
            for i in 0..self.rows() {
                if let Some(row) = self.grid.row_mut(i) {
                    row.mark_dirty();
                }
            }
        }
    }

    /// Check if we're viewing scrollback (not at bottom).
    pub fn is_scrolled(&self) -> bool {
        self.scroll_offset > 0
    }

    /// Scroll to the bottom (exit scrollback view).
    pub fn scroll_to_bottom(&mut self) {
        if self.scroll_offset > 0 {
            self.scroll_offset = 0;
            for i in 0..self.rows() {
                if let Some(row) = self.grid.row_mut(i) {
                    row.mark_dirty();
                }
            }
        }
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
            let main_grid = std::mem::replace(&mut self.grid, Grid::new(rows, cols));
            self.alt_grid = Some(main_grid);
            self.alt_screen = true;
        }
    }

    /// Switch back to main screen buffer.
    fn exit_alt_screen(&mut self) {
        if self.alt_screen {
            if let Some(main_grid) = self.alt_grid.take() {
                self.grid = main_grid;
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
                // params[0] = "8", params[1] = id/params, params[2] = URI
                if params.len() >= 2 {
                    let uri = if params.len() >= 3 {
                        std::str::from_utf8(params[2]).ok()
                    } else {
                        // Empty URI closes hyperlink
                        std::str::from_utf8(params[1]).ok()
                    };

                    match uri {
                        Some(url) if !url.is_empty() => {
                            // Open hyperlink - intern the URL
                            let id = self.hyperlinks.intern(url);
                            self.hyperlink = Some(id);
                        }
                        _ => {
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
                        self.grid.clear_below(self.cursor.row, self.cursor.col);
                    }
                    1 => {
                        // Clear from start of screen to cursor
                        self.grid.clear_above(self.cursor.row, self.cursor.col);
                    }
                    2 | 3 => {
                        // Clear entire screen (3 also clears scrollback, but we don't have that yet)
                        self.grid.clear();
                    }
                    _ => {}
                }
            }
            // EL - Erase in Line
            ('K', false) => {
                let mode = params.first().copied().unwrap_or(0);
                if let Some(row) = self.grid.row_mut(self.cursor.row) {
                    match mode {
                        0 => row.clear_from(self.cursor.col),
                        1 => row.clear_to(self.cursor.col + 1),
                        2 => row.clear(),
                        _ => {}
                    }
                }
            }
            // IL - Insert Lines
            ('L', false) => {
                let n = params.first().copied().unwrap_or(1).max(1) as usize;
                for _ in 0..n {
                    self.grid
                        .scroll_region_down(self.cursor.row, self.scroll_bottom);
                }
            }
            // DL - Delete Lines
            ('M', false) => {
                let n = params.first().copied().unwrap_or(1).max(1) as usize;
                for _ in 0..n {
                    self.grid
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
                    self.grid
                        .scroll_region_up(self.scroll_top, self.scroll_bottom);
                }
            }
            // SD - Scroll Down
            ('T', false) => {
                let n = params.first().copied().unwrap_or(1).max(1) as usize;
                for _ in 0..n {
                    self.grid
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
                    self.grid
                        .scroll_region_down(self.scroll_top, self.scroll_bottom);
                } else {
                    self.cursor.row = self.cursor.row.saturating_sub(1);
                }
            }
            _ => {}
        }
    }
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
        assert_eq!(term.grid.get(0, 0).unwrap().c, 'A');
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
        assert!(term.scrollback.len() >= 5, "Scrollback should have content");

        // Scroll up into history
        term.scroll_view(-5);
        assert_eq!(term.scroll_offset, 5, "Should be scrolled up 5 lines");

        // Resize (same size to test offset preservation)
        term.resize(24, 80);

        // Scroll offset should be preserved
        assert_eq!(
            term.scroll_offset, 5,
            "Scroll offset should be preserved after resize"
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
        assert_eq!(term.scrollback.len(), 0, "No scrollback yet");
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
            term.scrollback.len() > 0,
            "Scrollback should have content after shrinking with cursor below new height"
        );

        // The content from the top rows should now be in scrollback
        // We scrolled up (20 - 10 + 1 = 11) rows to bring cursor into view
        assert!(
            term.scrollback.len() >= 11,
            "Scrollback should have at least 11 lines, got {}",
            term.scrollback.len()
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
        assert_eq!(term.scrollback.len(), 0, "No scrollback yet");

        // Resize to 10 rows - cursor at row 5 is still within bounds
        term.resize(10, 80);

        // Cursor should remain at row 5
        assert_eq!(term.cursor.row, 5, "Cursor should stay at row 5");

        // No scrollback needed since cursor was in bounds
        assert_eq!(
            term.scrollback.len(),
            0,
            "No scrollback needed when cursor stays in bounds"
        );

        // Content should still be there
        let row0 = term.grid.row(0).unwrap();
        assert_eq!(row0.get(0).unwrap().c, 'L', "Content at row 0 preserved");
    }
}
