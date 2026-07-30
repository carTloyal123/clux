//! CSI sequence handling.

use super::Terminal;

impl Terminal {
    pub(super) fn dispatch_csi(
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
            ('h', true) => self.set_private_mode(true, &params),
            ('l', true) => self.set_private_mode(false, &params),
            _ => {}
        }
    }
}
