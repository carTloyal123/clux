//! Turning cells into ANSI for the host terminal.

use super::NO_LINK;

/// OSC 8 sequence that ends the current hyperlink.
const CLOSE_HYPERLINK: &str = "\x1b]8;;\x1b\\";

use std::collections::HashMap;
use std::sync::Arc;

use crate::cell::{Cell, CellFlags, Color};

/// Convert a slice of cells to an ANSI escape sequence string.
/// Optimizes by only emitting escape codes when attributes change.
pub fn cells_to_ansi(cells: &[Cell]) -> String {
    cells_to_ansi_with_links(cells, &[], &HashMap::new())
}
/// Convert a slice of cells to ANSI, wrapping linked runs in OSC 8.
///
/// `link_ids` is parallel to `cells`; ids index into `urls`. Runs of one logical
/// link share an id, and that id is emitted as the OSC 8 `id=` parameter so the
/// host terminal can join fragments split across rows into one link.
pub fn cells_to_ansi_with_links(
    cells: &[Cell],
    link_ids: &[u32],
    urls: &HashMap<u32, Arc<str>>,
) -> String {
    let mut output = String::with_capacity(cells.len() * 2);

    // Track current state to minimize escape codes
    let mut current_fg = Color::default_color();
    let mut current_bg = Color::default_color();
    let mut current_flags = CellFlags::empty();

    // Reset to known state
    output.push_str("\x1b[0m");

    let mut open_link = NO_LINK;

    for (col, cell) in cells.iter().enumerate() {
        let link_id = link_ids
            .get(col)
            .copied()
            .filter(|id| *id != NO_LINK && urls.contains_key(id))
            .unwrap_or(NO_LINK);

        if link_id != open_link {
            if open_link != NO_LINK {
                output.push_str(CLOSE_HYPERLINK);
            }
            if link_id != NO_LINK {
                // ESC ] 8 ; id=<id> ; <url> ST
                output.push_str("\x1b]8;id=");
                output.push_str(&link_id.to_string());
                output.push(';');
                output.push_str(&urls[&link_id]);
                output.push_str("\x1b\\");
            }
            open_link = link_id;
        }

        let mut need_sgr = false;
        let mut sgr_codes: Vec<u8> = Vec::new();

        // Check if we need to reset
        if cell.flags != current_flags {
            // Reset and reapply all attributes
            sgr_codes.push(0);
            current_fg = Color::default_color();
            current_bg = Color::default_color();
            need_sgr = true;

            // Apply flags
            if cell.flags.contains(CellFlags::BOLD) {
                sgr_codes.push(1);
            }
            if cell.flags.contains(CellFlags::DIM) {
                sgr_codes.push(2);
            }
            if cell.flags.contains(CellFlags::ITALIC) {
                sgr_codes.push(3);
            }
            if cell.flags.contains(CellFlags::UNDERLINE) {
                sgr_codes.push(4);
            }
            if cell.flags.contains(CellFlags::BLINK) {
                sgr_codes.push(5);
            }
            if cell.flags.contains(CellFlags::INVERSE) {
                sgr_codes.push(7);
            }
            if cell.flags.contains(CellFlags::HIDDEN) {
                sgr_codes.push(8);
            }
            if cell.flags.contains(CellFlags::STRIKETHROUGH) {
                sgr_codes.push(9);
            }
            current_flags = cell.flags;
        }

        // Check foreground color
        if cell.fg != current_fg {
            need_sgr = true;
            append_fg_color(&mut output, &sgr_codes, &cell.fg);
            sgr_codes.clear();
            current_fg = cell.fg;
        }

        // Check background color
        if cell.bg != current_bg {
            need_sgr = true;
            append_bg_color(&mut output, &sgr_codes, &cell.bg);
            sgr_codes.clear();
            current_bg = cell.bg;
        }

        // Emit any remaining SGR codes
        if need_sgr && !sgr_codes.is_empty() {
            output.push_str("\x1b[");
            for (i, code) in sgr_codes.iter().enumerate() {
                if i > 0 {
                    output.push(';');
                }
                output.push_str(&code.to_string());
            }
            output.push('m');
        }

        // Output the character
        output.push(cell.c);
    }

    // Close the link before leaving the row; the next row is painted with its
    // own absolute cursor move, so an open link would leak into it.
    if open_link != NO_LINK {
        output.push_str(CLOSE_HYPERLINK);
    }

    // Reset at end of row
    output.push_str("\x1b[0m");
    output
}
/// Append foreground color escape sequence.
fn append_fg_color(output: &mut String, pending_codes: &[u8], color: &Color) {
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
fn append_bg_color(output: &mut String, pending_codes: &[u8], color: &Color) {
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
