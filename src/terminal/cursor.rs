//! The cursor.

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
