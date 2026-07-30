//! Cell colours.

use serde::{Deserialize, Serialize};

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
