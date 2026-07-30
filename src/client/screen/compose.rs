//! Compositing pane updates and drawing dividers into the screen.

use std::sync::Arc;

use super::{divider_cell, NO_LINK};
use crate::cell::CellFlags;
use crate::protocol::PaneRow;
impl super::ScreenBuffer {
    /// Apply a pane update to the screen buffer.
    /// Translates pane-local coordinates to screen coordinates.
    pub fn apply_pane_update(&mut self, pane_id: u32, changed_rows: &[PaneRow]) {
        let Some(layout) = &self.layout else {
            return;
        };

        // Find the pane in the layout
        let Some(pane) = layout.panes.iter().find(|p| p.pane_id == pane_id) else {
            return;
        };

        let pane_x = pane.x as usize;
        let pane_width = pane.width as usize;

        // Apply each row update
        for pane_row in changed_rows {
            let screen_row = pane.y as usize + pane_row.row_idx as usize;

            // Bounds check
            if screen_row >= self.rows {
                continue;
            }

            // Links arrive as the complete set for the row, so drop the pane's
            // previous ones across its full width - not just the columns this
            // update rewrites, or a shrinking link leaves a stale tail behind.
            let pane_end = (pane_x + pane_width).min(self.cols);
            for screen_col in pane_x..pane_end {
                self.link_ids[screen_row][screen_col] = NO_LINK;
            }

            // The wrap flag belongs to the pane row, so record it across the
            // pane's columns: neighbouring panes on this screen row wrap
            // independently.
            for screen_col in pane_x..pane_end {
                self.row_continues[screen_row][screen_col] = pane_row.wrapped;
            }

            // Copy cells to the correct screen position
            for (col_offset, cell) in pane_row.cells.iter().enumerate() {
                let screen_col = pane_x + col_offset;

                // Bounds check - don't overflow pane width
                if col_offset >= pane_width {
                    break;
                }
                if screen_col >= self.cols {
                    break;
                }

                self.cells[screen_row][screen_col] = *cell;
            }

            for link in &pane_row.links {
                if link.url.chars().any(|c| c.is_control()) {
                    // Never let a URL smuggle escape bytes into the host terminal.
                    continue;
                }

                let start = pane_x + link.start_col as usize;
                let end = (pane_x + link.end_col as usize)
                    .min(pane_x + pane_width)
                    .min(self.cols);

                if link.id == NO_LINK || start >= end {
                    continue;
                }

                // Refresh the target if this id now points somewhere else, but
                // avoid reallocating on the common case of an unchanged repaint.
                match self.urls.get(&link.id) {
                    Some(known) if known.as_ref() == link.url.as_str() => {}
                    _ => {
                        self.urls.insert(link.id, Arc::from(link.url.as_str()));
                    }
                }

                for screen_col in start..end {
                    self.link_ids[screen_row][screen_col] = link.id;

                    // A URL clux found itself gets an underline so it reads as a
                    // link. An application's own OSC 8 link is left exactly as it
                    // styled it - it already decided how its links should look.
                    if link.detected {
                        self.cells[screen_row][screen_col]
                            .flags
                            .insert(CellFlags::UNDERLINE);
                    }
                }
            }
        }

        self.prune_urls();
    }
    /// Drop URLs whose link id is no longer on screen.
    ///
    /// Only worth the scan once the table has grown; screens rarely hold more
    /// than a handful of distinct links.
    pub(super) fn prune_urls(&mut self) {
        const PRUNE_THRESHOLD: usize = 256;

        if self.urls.len() <= PRUNE_THRESHOLD {
            return;
        }

        let live: std::collections::HashSet<u32> = self
            .link_ids
            .iter()
            .flatten()
            .copied()
            .filter(|&id| id != NO_LINK)
            .collect();

        self.urls.retain(|id, _| live.contains(id));
    }
    /// Draw dividers between panes based on the current layout.
    pub(super) fn draw_dividers(&mut self) {
        let Some(layout) = &self.layout else {
            return;
        };

        // For each pane, check if we need to draw dividers
        // We draw dividers to the LEFT and ABOVE each pane (except the first)
        for pane in &layout.panes {
            // Draw left vertical divider if pane doesn't start at column 0
            if pane.x > 0 {
                let divider_col = pane.x as usize - 1;
                for row in pane.y as usize..(pane.y as usize + pane.height as usize) {
                    if row < self.rows && divider_col < self.cols {
                        self.cells[row][divider_col] = divider_cell('│');
                    }
                }
            }

            // Draw top horizontal divider if pane doesn't start at row 0
            if pane.y > 0 {
                let divider_row = pane.y as usize - 1;
                if divider_row < self.rows {
                    for col in pane.x as usize..(pane.x as usize + pane.width as usize) {
                        if col < self.cols {
                            // Check for intersection with vertical divider
                            let existing = self.cells[divider_row][col].c;
                            let ch = if existing == '│' {
                                '┼' // Intersection
                            } else {
                                '─'
                            };
                            self.cells[divider_row][col] = divider_cell(ch);
                        }
                    }
                }
            }
        }
    }
}
