//! Turning rows of cells into link runs.
//!
//! Explicit runs come from cells the application marked with OSC 8; detected runs
//! come from URL-shaped text in wrap-joined logical lines. Both read through
//! the buffer's viewport, so a scrolled pane resolves exactly like a live one.

use super::{detect::find_urls, link_id, LinkRun};
use crate::buffer::Buffer;
use crate::hyperlink::HyperlinkStore;

/// Runs formed by cells the application marked with an OSC 8 hyperlink.
pub(super) fn explicit_runs(
    source: &Buffer,
    store: &HyperlinkStore,
    salt: u32,
    rows: &[u16],
) -> Vec<LinkRun> {
    let mut runs = Vec::new();

    for &row_idx in rows {
        let Some((cells, _)) = source.row_cells(row_idx as usize) else {
            continue;
        };

        let mut col = 0;
        while col < cells.len() {
            let Some(id) = cells[col].hyperlink else {
                col += 1;
                continue;
            };

            let start = col;
            while col < cells.len() && cells[col].hyperlink == Some(id) {
                col += 1;
            }

            if let Some(url) = store.get(id) {
                // Keyed on the store id, not the row, so the runs of a link that
                // wraps across rows share one OSC 8 id.
                runs.push(LinkRun {
                    row: row_idx,
                    start_col: start as u16,
                    end_col: col as u16,
                    id: link_id(&[salt, 1, id.get()], ""),
                    url: url.to_string(),
                    detected: false,
                });
            }
        }
    }

    runs
}

/// One wrap-joined logical line: its text plus the row/column of each char.
pub(super) struct LogicalLine {
    text: Vec<char>,
    positions: Vec<(u16, u16)>,
}

/// Build the logical lines covering `rows`, following soft-wrap continuations.
///
/// Cells already carrying an explicit OSC 8 hyperlink are blanked out so
/// detection never fights with what the application asked for.
pub(super) fn logical_lines(source: &Buffer, rows: &[u16]) -> Vec<LogicalLine> {
    let total_rows = source.row_count();
    let mut lines = Vec::new();
    let mut start = 0;

    while start < total_rows {
        // A row is flagged wrapped when its content continues onto the next row.
        let mut end = start;
        while end < total_rows
            && source
                .row_cells(end)
                .map(|(_, wrapped)| wrapped)
                .unwrap_or(false)
            && end + 1 < total_rows
        {
            end += 1;
        }

        let covers_requested_row = rows.iter().any(|&r| {
            let r = r as usize;
            r >= start && r <= end
        });

        if covers_requested_row && line_may_hold_url(source, start, end) {
            let mut text = Vec::new();
            let mut positions = Vec::new();

            for row_idx in start..=end {
                let Some((cells, _)) = source.row_cells(row_idx) else {
                    continue;
                };
                for (col, cell) in cells.iter().enumerate() {
                    // Blank explicit links and control chars so they can never
                    // form or extend a detected URL.
                    let c = if cell.hyperlink.is_some() || cell.c.is_control() {
                        ' '
                    } else {
                        cell.c
                    };
                    text.push(c);
                    positions.push((row_idx as u16, col as u16));
                }
            }

            lines.push(LogicalLine { text, positions });
        }

        start = end + 1;
    }

    lines
}

/// Cheap pre-filter: every scheme we detect contains ':', so a line without one
/// cannot hold a URL. Runs on every PTY read, so it avoids the per-line
/// allocation for the common case of output with no links in it.
fn line_may_hold_url(source: &Buffer, start: usize, end: usize) -> bool {
    (start..=end).any(|row_idx| {
        source
            .row_cells(row_idx)
            .map(|(cells, _)| {
                cells
                    .iter()
                    .any(|cell| cell.c == ':' && cell.hyperlink.is_none())
            })
            .unwrap_or(false)
    })
}

/// Detected runs for one logical line, split back into per-row runs.
pub(super) fn detect_runs(source: &Buffer, salt: u32, line: &LogicalLine) -> Vec<LinkRun> {
    let mut runs = Vec::new();

    for (start, end) in find_urls(&line.text) {
        let url: String = line.text[start..end].iter().collect();
        let (anchor_row, anchor_col) = line.positions[start];
        // Anchored at the first cell of this occurrence so two copies of the same
        // URL on screen stay distinct links, and so a partial repaint of the same
        // text regenerates the same id.
        let id = link_id(&[salt, 2, anchor_row as u32, anchor_col as u32], &url);

        // Consecutive chars can span rows; emit one run per row.
        let mut idx = start;
        while idx < end {
            let (row, first_col) = line.positions[idx];
            let mut last_col = first_col;
            while idx + 1 < end && line.positions[idx + 1].0 == row {
                idx += 1;
                last_col = line.positions[idx].1;
            }
            idx += 1;

            // Skip runs whose row vanished (the view shrank mid-scan).
            if source.row_cells(row as usize).is_some() {
                runs.push(LinkRun {
                    row,
                    start_col: first_col,
                    end_col: last_col + 1,
                    id,
                    url: url.clone(),
                    detected: true,
                });
            }
        }
    }

    runs
}
