//! Emitting SGR color sequences.

/// OSC 8 sequence that ends the current hyperlink.
const CLOSE_HYPERLINK: &str = "\x1b]8;;\x1b\\";

use crate::cell::Color;

/// Append foreground color escape sequence.
pub(super) fn append_fg_color(output: &mut String, pending_codes: &[u8], color: &Color) {
    use crate::cell::ColorKind;

    // First emit any pending codes
    if !pending_codes.is_empty() {
        output.push_str("\x1b[");
        for (i, code) in pending_codes.iter().enumerate() {
            if i > 0 {
                output.push(';');
            }
            output.push_str(&code.to_string());
        }
        output.push('m');
    }

    match color.kind {
        ColorKind::Default => {
            output.push_str("\x1b[39m");
        }
        ColorKind::Indexed => {
            if color.r < 8 {
                output.push_str(&format!("\x1b[{}m", 30 + color.r));
            } else if color.r < 16 {
                output.push_str(&format!("\x1b[{}m", 90 + color.r - 8));
            } else {
                output.push_str(&format!("\x1b[38;5;{}m", color.r));
            }
        }
        ColorKind::Rgb => {
            output.push_str(&format!("\x1b[38;2;{};{};{}m", color.r, color.g, color.b));
        }
    }
}
/// Append background color escape sequence.
pub(super) fn append_bg_color(output: &mut String, pending_codes: &[u8], color: &Color) {
    use crate::cell::ColorKind;

    // First emit any pending codes
    if !pending_codes.is_empty() {
        output.push_str("\x1b[");
        for (i, code) in pending_codes.iter().enumerate() {
            if i > 0 {
                output.push(';');
            }
            output.push_str(&code.to_string());
        }
        output.push('m');
    }

    match color.kind {
        ColorKind::Default => {
            output.push_str("\x1b[49m");
        }
        ColorKind::Indexed => {
            if color.r < 8 {
                output.push_str(&format!("\x1b[{}m", 40 + color.r));
            } else if color.r < 16 {
                output.push_str(&format!("\x1b[{}m", 100 + color.r - 8));
            } else {
                output.push_str(&format!("\x1b[48;5;{}m", color.r));
            }
        }
        ColorKind::Rgb => {
            output.push_str(&format!("\x1b[48;2;{};{};{}m", color.r, color.g, color.b));
        }
    }
}
