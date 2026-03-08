//! Cell representation for terminal grid.
//!
//! Each cell stores a character and its attributes (colors, styling).
//! Memory-optimized for high refresh rate rendering.

// Many methods will be used in later phases (selection, hyperlinks)
#![allow(dead_code)]

use bitflags::bitflags;
use serde::{Deserialize, Serialize};

/// A single cell in the terminal grid.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Cell {
    /// The character displayed in this cell.
    pub c: char,
    /// Foreground color.
    pub fg: Color,
    /// Background color.
    pub bg: Color,
    /// Style flags (bold, italic, etc.).
    pub flags: CellFlags,
    /// Hyperlink ID (Phase 3 - for now always None).
    pub hyperlink: Option<HyperlinkId>,
}

impl Default for Cell {
    fn default() -> Self {
        Self {
            c: ' ',
            fg: Color::default(),
            bg: Color::default(),
            flags: CellFlags::empty(),
            hyperlink: None,
        }
    }
}

impl Cell {
    /// Create a new cell with a character and default styling.
    pub fn new(c: char) -> Self {
        Self {
            c,
            ..Default::default()
        }
    }

    /// Create a cell with full styling.
    pub fn styled(c: char, fg: Color, bg: Color, flags: CellFlags) -> Self {
        Self {
            c,
            fg,
            bg,
            flags,
            hyperlink: None,
        }
    }

    /// Check if this cell is a space with default colors (can skip rendering).
    #[inline]
    pub fn is_empty(&self) -> bool {
        self.c == ' ' && self.fg.is_default() && self.bg.is_default() && self.flags.is_empty()
    }

    /// Reset cell to default state.
    pub fn reset(&mut self) {
        *self = Self::default();
    }
}

/// Color representation supporting default, indexed (256), and true color.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Color {
    pub r: u8,
    pub g: u8,
    pub b: u8,
    pub kind: ColorKind,
}

impl Default for Color {
    fn default() -> Self {
        Self {
            r: 0,
            g: 0,
            b: 0,
            kind: ColorKind::Default,
        }
    }
}

impl Color {
    /// Create a default (terminal default) color.
    pub const fn default_color() -> Self {
        Self {
            r: 0,
            g: 0,
            b: 0,
            kind: ColorKind::Default,
        }
    }

    /// Create an RGB true color.
    pub const fn rgb(r: u8, g: u8, b: u8) -> Self {
        Self {
            r,
            g,
            b,
            kind: ColorKind::Rgb,
        }
    }

    /// Create a 256-color palette index.
    pub const fn indexed(index: u8) -> Self {
        Self {
            r: index,
            g: 0,
            b: 0,
            kind: ColorKind::Indexed,
        }
    }

    /// Check if this is the default color.
    #[inline]
    pub fn is_default(&self) -> bool {
        self.kind == ColorKind::Default
    }

    /// Convert to crossterm color for rendering.
    pub fn to_crossterm(&self) -> crossterm::style::Color {
        match self.kind {
            ColorKind::Default => crossterm::style::Color::Reset,
            ColorKind::Indexed => crossterm::style::Color::AnsiValue(self.r),
            ColorKind::Rgb => crossterm::style::Color::Rgb {
                r: self.r,
                g: self.g,
                b: self.b,
            },
        }
    }

    /// Create from ANSI SGR color parameter (30-37, 40-47, 90-97, 100-107).
    pub fn from_ansi(code: u16) -> Option<Self> {
        let index = match code {
            30..=37 => code - 30,
            40..=47 => code - 40,
            90..=97 => code - 90 + 8,
            100..=107 => code - 100 + 8,
            _ => return None,
        };
        Some(Self::indexed(index as u8))
    }
}

/// The kind of color (default, indexed 256, or RGB true color).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum ColorKind {
    /// Use terminal's default foreground/background.
    #[default]
    Default,
    /// 256-color palette index (stored in r field).
    Indexed,
    /// 24-bit RGB true color.
    Rgb,
}

bitflags! {
    /// Cell style attributes.
    #[derive(Clone, Copy, Debug, PartialEq, Eq, Default, Serialize, Deserialize)]
    #[serde(transparent)]
    pub struct CellFlags: u8 {
        const BOLD          = 0b0000_0001;
        const DIM           = 0b0000_0010;
        const ITALIC        = 0b0000_0100;
        const UNDERLINE     = 0b0000_1000;
        const BLINK         = 0b0001_0000;
        const INVERSE       = 0b0010_0000;
        const HIDDEN        = 0b0100_0000;
        const STRIKETHROUGH = 0b1000_0000;
    }
}

impl CellFlags {
    /// Convert to crossterm attributes for rendering.
    pub fn to_crossterm_attributes(&self) -> crossterm::style::Attributes {
        use crossterm::style::Attribute;

        let mut attrs = crossterm::style::Attributes::default();

        if self.contains(CellFlags::BOLD) {
            attrs.set(Attribute::Bold);
        }
        if self.contains(CellFlags::DIM) {
            attrs.set(Attribute::Dim);
        }
        if self.contains(CellFlags::ITALIC) {
            attrs.set(Attribute::Italic);
        }
        if self.contains(CellFlags::UNDERLINE) {
            attrs.set(Attribute::Underlined);
        }
        if self.contains(CellFlags::BLINK) {
            attrs.set(Attribute::SlowBlink);
        }
        if self.contains(CellFlags::INVERSE) {
            attrs.set(Attribute::Reverse);
        }
        if self.contains(CellFlags::HIDDEN) {
            attrs.set(Attribute::Hidden);
        }
        if self.contains(CellFlags::STRIKETHROUGH) {
            attrs.set(Attribute::CrossedOut);
        }

        attrs
    }
}

/// Hyperlink ID for memory-efficient URL storage (Phase 3).
/// Uses u32 to reference interned URLs instead of storing full URL per cell.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct HyperlinkId(pub u32);

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_cell_default() {
        let cell = Cell::default();
        assert_eq!(cell.c, ' ');
        assert!(cell.fg.is_default());
        assert!(cell.bg.is_default());
        assert!(cell.flags.is_empty());
        assert!(cell.is_empty());
    }

    #[test]
    fn test_cell_styled() {
        let cell = Cell::styled(
            'A',
            Color::rgb(255, 0, 0),
            Color::default_color(),
            CellFlags::BOLD,
        );
        assert_eq!(cell.c, 'A');
        assert_eq!(cell.fg.kind, ColorKind::Rgb);
        assert!(cell.flags.contains(CellFlags::BOLD));
        assert!(!cell.is_empty());
    }

    #[test]
    fn test_color_from_ansi() {
        // Standard colors (30-37)
        assert_eq!(Color::from_ansi(31).unwrap().r, 1); // Red
        assert_eq!(Color::from_ansi(32).unwrap().r, 2); // Green

        // Bright colors (90-97)
        assert_eq!(Color::from_ansi(91).unwrap().r, 9); // Bright red

        // Invalid
        assert!(Color::from_ansi(0).is_none());
    }

    #[test]
    fn test_cell_flags() {
        let flags = CellFlags::BOLD | CellFlags::UNDERLINE;
        assert!(flags.contains(CellFlags::BOLD));
        assert!(flags.contains(CellFlags::UNDERLINE));
        assert!(!flags.contains(CellFlags::ITALIC));
    }
}
