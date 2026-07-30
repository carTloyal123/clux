//! The VTE `Perform` implementation: escape sequences into buffer edits.

use super::Terminal;

impl vte::Perform for Terminal {
    fn osc_dispatch(&mut self, params: &[&[u8]], bell_terminated: bool) {
        self.dispatch_osc(params, bell_terminated);
    }

    fn csi_dispatch(
        &mut self,
        params: &vte::Params,
        intermediates: &[u8],
        ignore: bool,
        action: char,
    ) {
        self.dispatch_csi(params, intermediates, ignore, action);
    }

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
