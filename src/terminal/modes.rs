//! DEC private mode set/reset (`CSI ? Pm h` / `CSI ? Pm l`).

use super::Terminal;

impl Terminal {
    /// Handle a DEC private mode set (`set = true`) or reset.
    pub(super) fn set_private_mode(&mut self, set: bool, params: &[u16]) {
        for &param in params {
            match (param, set) {
                // DECAWM - Auto-wrap mode
                (7, _) => self.auto_wrap = set,
                // DECTCEM - Show/hide cursor
                (25, _) => self.cursor.visible = set,
                // X11 mouse reporting: normal / button-event / any-event tracking
                (1000, true) => self.mouse_mode = 1000,
                (1002, true) => self.mouse_mode = 1002,
                (1003, true) => self.mouse_mode = 1003,
                (1000 | 1002 | 1003, false) => self.mouse_mode = 0,
                // SGR mouse encoding
                (1006, _) => self.sgr_mouse = set,
                // Alternate screen buffer
                (1049, true) => self.enter_alt_screen(),
                (1049, false) => self.exit_alt_screen(),
                // DECCKM (1), cursor blink (12): recognised, no state kept
                _ => {}
            }
        }
    }
}
