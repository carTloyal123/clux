//! Link resolution for the outer terminal.
//!
//! A multiplexer repaints every row with an absolute cursor move, so the outer
//! terminal never sees our soft-wrap continuations. Terminals that detect URLs
//! themselves (Ghostty, WezTerm, iTerm2) run their regex over *their* grid, so a
//! URL that wraps inside a clux pane looks like two unrelated fragments to them
//! and stops being clickable. Split panes make it worse: a URL is clipped at the
//! pane border, and the terminal happily matches across the divider into the
//! neighbouring pane's text.
//!
//! Clux is the only process that knows where its logical lines really end, so it
//! resolves links itself and hands the outer terminal explicit OSC 8 hyperlinks:
//!
//! - explicit runs: cells the application marked with OSC 8
//! - detected runs: URL-shaped text found in wrap-joined logical lines
//!
//! Every run of a single logical link shares one OSC 8 `id`, which is what lets
//! the outer terminal treat fragments split across rows as one link for hover
//! highlighting (per the OSC 8 spec, and Ghostty groups link cells by `(id, uri)`).

mod detect;
mod runs;
#[cfg(test)]
mod tests;

pub use detect::find_urls;

use std::collections::HashMap;

use crate::buffer::Buffer;
use crate::hyperlink::HyperlinkStore;

/// A run of cells on a single row belonging to one logical link.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LinkRun {
    /// Row index within the grid.
    pub row: u16,
    /// First column of the run.
    pub start_col: u16,
    /// One past the last column of the run.
    pub end_col: u16,
    /// Identifier shared by every run of the same logical link.
    pub id: u32,
    /// Target URL.
    pub url: String,
    /// Whether clux found this link itself instead of the application asking for
    /// it with OSC 8. Detected links are styled as links; an application's own
    /// links keep exactly the styling it printed.
    pub detected: bool,
}

/// Longest URL we are willing to carry for one link.
const MAX_URL_LEN: usize = 4096;

/// Clean a URL that came from application output, or reject it.
///
/// Control characters are stripped: the URL is re-emitted verbatim inside an
/// OSC 8 sequence, so an embedded ESC would let a program's output inject
/// arbitrary escape sequences into the host terminal.
pub fn sanitize_url(url: &str) -> Option<String> {
    let cleaned: String = url
        .chars()
        .filter(|c| !c.is_control())
        .take(MAX_URL_LEN)
        .collect();

    (!cleaned.trim().is_empty()).then_some(cleaned)
}

/// Resolve every link visible in `source`, restricted to the logical lines that
/// contain `rows`.
///
/// `source` is whatever the pane is currently showing: the active screen, or the
/// buffer's viewport, which reads history when scrolled back - so links keep
/// working there.
///
/// `rows` are the rows about to be sent to a client; the scan is widened to the
/// full logical line each one belongs to so a URL is still found when only its
/// tail row is dirty. `salt` scopes generated ids to one pane so two panes never
/// collide on an id.
///
/// Returns runs keyed by row index, which may include rows outside `rows` when a
/// link wraps: those rows have to be repainted too, otherwise the outer terminal
/// keeps the stale fragment it was given before the line grew.
pub fn resolve_links(
    source: &Buffer,
    store: &HyperlinkStore,
    salt: u32,
    detect_plain_urls: bool,
    rows: &[u16],
) -> HashMap<u16, Vec<LinkRun>> {
    let mut by_row: HashMap<u16, Vec<LinkRun>> = HashMap::new();
    if rows.is_empty() {
        return by_row;
    }

    for run in runs::explicit_runs(source, store, salt, rows) {
        by_row.entry(run.row).or_default().push(run);
    }

    if detect_plain_urls {
        for line in runs::logical_lines(source, rows) {
            for run in runs::detect_runs(source, salt, &line) {
                by_row.entry(run.row).or_default().push(run);
            }
        }
    }

    for runs in by_row.values_mut() {
        runs.sort_by_key(|r| r.start_col);
    }

    by_row
}

/// Stable non-zero id from a set of numbers plus a string (FNV-1a).
fn link_id(parts: &[u32], text: &str) -> u32 {
    let mut hash: u32 = 0x811c_9dc5;
    let mut mix = |byte: u8| {
        hash ^= byte as u32;
        hash = hash.wrapping_mul(0x0100_0193);
    };

    for part in parts {
        for byte in part.to_le_bytes() {
            mix(byte);
        }
    }
    for &byte in text.as_bytes() {
        mix(byte);
    }

    // 0 means "no link" on the wire.
    if hash == 0 {
        1
    } else {
        hash
    }
}
