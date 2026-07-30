//! SGR (Select Graphic Rendition): colors and text attributes.

use crate::cell::{CellFlags, Color};
impl super::Terminal {
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
}
