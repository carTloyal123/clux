//! Client-side screen buffer for hybrid rendering.
//!
//! The ScreenBuffer maintains a grid of styled cells and composites
//! pane content at the correct screen positions. This enables:
//! - Proper isolation between panes (no overwriting adjacent content)
//! - Client-side divider drawing
//! - Efficient partial updates

use std::collections::HashMap;
use std::sync::Arc;

use crate::cell::{Cell, CellFlags, Color};
use crate::protocol::{PaneLayout, PaneRow, WindowLayout};
use crate::selection::{Point, Selection, SelectionMode};

/// Cursor position in screen coordinates.
#[derive(Debug, Clone, Copy, Default)]
pub struct CursorPosition {
    pub row: u16,
    pub col: u16,
    pub visible: bool,
}

/// Link id meaning "this cell is not part of a hyperlink".
const NO_LINK: u32 = 0;

/// Begin a synchronized update (DECSET 2026).
///
/// The host terminal holds off presenting anything until the matching end, so a
/// repaint spanning several rows is never shown half-drawn. This is the form both
/// Ghostty and tmux advertise as the `Sync` terminfo capability; the older
/// iTerm2 `DCS = 1 s` form is not worth carrying.
pub const BEGIN_SYNC_UPDATE: &str = "\x1b[?2026h";

/// End a synchronized update (DECRST 2026), presenting the frame.
pub const END_SYNC_UPDATE: &str = "\x1b[?2026l";

/// An active selection, anchored in one pane.
///
/// Selections never span panes: the pane is fixed when the drag starts and every
/// later point is clamped into it, so dragging across a divider extends within
/// the original pane instead of splicing in the neighbour's text.
#[derive(Clone, Debug)]
struct PaneSelection {
    pane_id: u32,
    selection: Selection,
}

/// Client-side screen buffer for compositing pane content.
pub struct ScreenBuffer {
    /// 2D grid of cells (row-major order).
    cells: Vec<Vec<Cell>>,
    /// Link id per cell, mirroring `cells`. `NO_LINK` means no hyperlink.
    link_ids: Vec<Vec<u32>>,
    /// Whether the pane row owning each cell continues onto the next row.
    ///
    /// Per cell rather than per screen row because side-by-side panes share a
    /// screen row and wrap independently.
    row_continues: Vec<Vec<bool>>,
    /// URL for each link id currently on screen.
    urls: HashMap<u32, Arc<str>>,
    /// Current window layout.
    layout: Option<WindowLayout>,
    /// Screen width in columns.
    cols: usize,
    /// Screen height in rows.
    rows: usize,
    /// Current cursor position (screen coordinates, for focused pane).
    cursor: CursorPosition,
    /// Active text selection, if any.
    selection: Option<PaneSelection>,
}

impl ScreenBuffer {
    /// Create a new screen buffer with the given dimensions.
    pub fn new(cols: usize, rows: usize) -> Self {
        let cells = vec![vec![Cell::default(); cols]; rows];
        Self {
            cells,
            link_ids: vec![vec![NO_LINK; cols]; rows],
            row_continues: vec![vec![false; cols]; rows],
            urls: HashMap::new(),
            layout: None,
            cols,
            rows,
            cursor: CursorPosition::default(),
            selection: None,
        }
    }

    /// Get the current dimensions.
    pub fn dimensions(&self) -> (usize, usize) {
        (self.cols, self.rows)
    }

    /// Set the window layout and draw dividers.
    pub fn set_layout(&mut self, layout: WindowLayout) {
        // Clear buffer before applying new layout
        self.clear();

        // Store layout
        self.layout = Some(layout);

        // Draw dividers between panes
        self.draw_dividers();
    }

    /// Get the current layout.
    pub fn layout(&self) -> Option<&WindowLayout> {
        self.layout.as_ref()
    }

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
    fn prune_urls(&mut self) {
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

    /// Get the hyperlink URL at a screen position, if any.
    pub fn link_at(&self, row: usize, col: usize) -> Option<&str> {
        let id = *self.link_ids.get(row)?.get(col)?;
        self.urls.get(&id).map(|u| u.as_ref())
    }

    // ------------------------------------------------------------------------
    // Selection
    // ------------------------------------------------------------------------

    /// Start a selection at a screen position. Does nothing outside a pane (on a
    /// divider, or past the layout), which is what makes stray clicks harmless.
    ///
    /// Returns whether a selection was started.
    pub fn begin_selection(&mut self, row: usize, col: usize, mode: SelectionMode) -> bool {
        let Some(pane) = self.pane_at(row, col).cloned() else {
            self.selection = None;
            return false;
        };

        self.selection = Some(PaneSelection {
            pane_id: pane.pane_id,
            selection: Selection::start(Point::new(row as i32, col), mode),
        });
        true
    }

    /// Extend the active selection to a screen position, clamped into the pane
    /// the selection started in.
    pub fn extend_selection(&mut self, row: usize, col: usize) -> bool {
        let Some(active) = &self.selection else {
            return false;
        };
        let Some(pane) = self.pane_by_id(active.pane_id).cloned() else {
            return false;
        };

        let point = Self::clamp_to_pane(&pane, row, col);
        if let Some(active) = &mut self.selection {
            active.selection.extend(point);
        }
        true
    }

    /// Drop any active selection.
    pub fn clear_selection(&mut self) {
        self.selection = None;
    }

    /// Whether a selection is active and covers at least one cell.
    pub fn has_selection(&self) -> bool {
        self.selection
            .as_ref()
            .map(|active| active.selection.active)
            .unwrap_or(false)
    }

    /// The selected text, or `None` when nothing is selected.
    ///
    /// Rows that soft-wrap are joined without a newline, so copying a long path
    /// or URL out of a pane gives back one unbroken string. Hard line ends have
    /// their trailing blank cells trimmed.
    pub fn selected_text(&self) -> Option<String> {
        let active = self.selection.as_ref()?;
        if !active.selection.active {
            return None;
        }

        let pane = self.pane_by_id(active.pane_id)?;
        let selection = &active.selection;
        let (start, end) = selection.normalized();

        let pane_first_row = pane.y as usize;
        let pane_last_row = (pane.y as usize + pane.height as usize).min(self.rows);
        let first_row = (start.line.max(0) as usize).max(pane_first_row);
        let last_row = (end.line.max(0) as usize + 1).min(pane_last_row);

        let mut text = String::new();
        let mut pending_newline = false;

        for row in first_row..last_row {
            // Selected cells on a row are contiguous for every mode we support,
            // and asking the selection itself keeps the copied text identical to
            // what is highlighted.
            let mut segment = String::new();
            let mut last_col = None;

            for col in self.pane_columns(pane) {
                if selection.contains(Point::new(row as i32, col)) {
                    segment.push(self.cells[row][col].c);
                    last_col = Some(col);
                }
            }

            if segment.is_empty() {
                continue;
            }

            if pending_newline {
                text.push('\n');
            }

            // Only a hard line end gets trimmed; trailing blanks in the middle of
            // a wrapped line are real content as far as the join is concerned.
            let continues = last_col
                .map(|col| self.row_continues[row][col])
                .unwrap_or(false)
                && selection.mode != SelectionMode::Block;

            if continues {
                text.push_str(&segment);
                pending_newline = false;
            } else {
                text.push_str(segment.trim_end());
                pending_newline = true;
            }
        }

        Some(text)
    }

    /// The pane covering a screen position, if any.
    fn pane_at(&self, row: usize, col: usize) -> Option<&PaneLayout> {
        self.layout.as_ref()?.panes.iter().find(|pane| {
            row >= pane.y as usize
                && row < pane.y as usize + pane.height as usize
                && col >= pane.x as usize
                && col < pane.x as usize + pane.width as usize
        })
    }

    fn pane_by_id(&self, pane_id: u32) -> Option<&PaneLayout> {
        self.layout
            .as_ref()?
            .panes
            .iter()
            .find(|pane| pane.pane_id == pane_id)
    }

    /// Screen columns belonging to a pane, clipped to the buffer.
    fn pane_columns(&self, pane: &PaneLayout) -> std::ops::Range<usize> {
        let start = pane.x as usize;
        let end = (start + pane.width as usize).min(self.cols);
        start..end.max(start)
    }

    /// Clamp a screen position into a pane's rectangle.
    fn clamp_to_pane(pane: &PaneLayout, row: usize, col: usize) -> Point {
        let last_row = (pane.y as usize + pane.height as usize).saturating_sub(1);
        let last_col = (pane.x as usize + pane.width as usize).saturating_sub(1);

        Point::new(
            row.clamp(pane.y as usize, last_row) as i32,
            col.clamp(pane.x as usize, last_col),
        )
    }

    /// Resize the screen buffer.
    /// Clears all content and resets layout.
    pub fn resize(&mut self, cols: usize, rows: usize) {
        self.cols = cols;
        self.rows = rows;
        self.cells = vec![vec![Cell::default(); cols]; rows];
        self.link_ids = vec![vec![NO_LINK; cols]; rows];
        self.row_continues = vec![vec![false; cols]; rows];
        self.urls.clear();
        self.layout = None;
        self.cursor = CursorPosition::default();
        self.selection = None;
    }

    /// Set the cursor position (in screen coordinates).
    pub fn set_cursor(&mut self, row: u16, col: u16, visible: bool) {
        self.cursor = CursorPosition { row, col, visible };
    }

    /// Get the current cursor position.
    pub fn cursor(&self) -> CursorPosition {
        self.cursor
    }

    /// Clear the screen buffer to default cells.
    pub fn clear(&mut self) {
        for row in &mut self.cells {
            for cell in row {
                *cell = Cell::default();
            }
        }
        for row in &mut self.link_ids {
            for id in row {
                *id = NO_LINK;
            }
        }
        for row in &mut self.row_continues {
            for wrapped in row {
                *wrapped = false;
            }
        }
        self.urls.clear();
        self.selection = None;
    }

    /// Get a row of cells.
    pub fn get_row(&self, row_idx: usize) -> Option<&[Cell]> {
        self.cells.get(row_idx).map(|r| r.as_slice())
    }

    /// Render a row to an ANSI escape sequence string, including OSC 8
    /// hyperlinks and any selection highlight on that row.
    pub fn render_row_ansi(&self, row_idx: usize) -> String {
        let Some(row) = self.cells.get(row_idx) else {
            return String::new();
        };

        // Selection is transient, so it is applied here rather than baked into
        // the stored cells: clearing it needs no restore.
        let highlighted = self.highlight_selection(row_idx, row);
        let row = highlighted.as_deref().unwrap_or(row);

        match self.link_ids.get(row_idx) {
            Some(ids) => cells_to_ansi_with_links(row, ids, &self.urls),
            None => cells_to_ansi(row),
        }
    }

    /// A copy of `row` with selected cells inverted, or `None` if this row has no
    /// selected cells.
    fn highlight_selection(&self, row_idx: usize, row: &[Cell]) -> Option<Vec<Cell>> {
        let active = self.selection.as_ref()?;
        if !active.selection.active {
            return None;
        }

        let pane = self.pane_by_id(active.pane_id)?;
        let mut highlighted: Option<Vec<Cell>> = None;

        for col in self.pane_columns(pane) {
            if !active.selection.contains(Point::new(row_idx as i32, col)) {
                continue;
            }
            let cells = highlighted.get_or_insert_with(|| row.to_vec());
            if let Some(cell) = cells.get_mut(col) {
                cell.flags.insert(CellFlags::INVERSE);
            }
        }

        highlighted
    }

    /// Draw dividers between panes based on the current layout.
    fn draw_dividers(&mut self) {
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

/// Create a divider cell with default styling.
fn divider_cell(c: char) -> Cell {
    Cell::styled(
        c,
        Color::indexed(8),
        Color::default_color(),
        CellFlags::empty(),
    )
}

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

/// OSC 8 sequence that ends the current hyperlink.
const CLOSE_HYPERLINK: &str = "\x1b]8;;\x1b\\";

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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::protocol::{PaneLayout, RowLink};

    /// Layout with a single full-screen pane.
    fn single_pane_layout(cols: u16, rows: u16) -> WindowLayout {
        WindowLayout {
            panes: vec![PaneLayout {
                pane_id: 0,
                x: 0,
                y: 0,
                width: cols,
                height: rows,
                focused: true,
            }],
            screen_cols: cols,
            screen_rows: rows,
        }
    }

    fn text_cells(s: &str) -> Vec<Cell> {
        s.chars().map(Cell::new).collect()
    }

    /// A link clux detected itself (so it gets link styling).
    fn link(start_col: u16, end_col: u16, id: u32, url: &str) -> RowLink {
        RowLink {
            start_col,
            end_col,
            id,
            url: url.to_string(),
            detected: true,
        }
    }

    /// A link the application asked for with OSC 8 (styling left alone).
    fn app_link(start_col: u16, end_col: u16, id: u32, url: &str) -> RowLink {
        RowLink {
            detected: false,
            ..link(start_col, end_col, id, url)
        }
    }

    #[test]
    fn test_screen_buffer_creation() {
        let buffer = ScreenBuffer::new(80, 24);
        assert_eq!(buffer.dimensions(), (80, 24));
        assert!(buffer.layout().is_none());
    }

    #[test]
    fn test_screen_buffer_resize() {
        let mut buffer = ScreenBuffer::new(80, 24);

        // Set a layout
        buffer.set_layout(WindowLayout {
            panes: vec![PaneLayout {
                pane_id: 0,
                x: 0,
                y: 0,
                width: 80,
                height: 24,
                focused: true,
            }],
            screen_cols: 80,
            screen_rows: 24,
        });

        assert!(buffer.layout().is_some());

        // Resize clears layout
        buffer.resize(100, 30);
        assert_eq!(buffer.dimensions(), (100, 30));
        assert!(buffer.layout().is_none());
    }

    #[test]
    fn test_single_pane_update() {
        let mut buffer = ScreenBuffer::new(80, 24);

        buffer.set_layout(WindowLayout {
            panes: vec![PaneLayout {
                pane_id: 0,
                x: 0,
                y: 0,
                width: 80,
                height: 24,
                focused: true,
            }],
            screen_cols: 80,
            screen_rows: 24,
        });

        // Apply an update
        buffer.apply_pane_update(0, &[PaneRow::new(0, vec![Cell::new('H'), Cell::new('i')])]);

        // Check the cells were written
        let row = buffer.get_row(0).unwrap();
        assert_eq!(row[0].c, 'H');
        assert_eq!(row[1].c, 'i');
        assert_eq!(row[2].c, ' '); // Rest should be default
    }

    #[test]
    fn test_vertical_split_isolation() {
        let mut buffer = ScreenBuffer::new(81, 24); // 40 + 1 divider + 40

        buffer.set_layout(WindowLayout {
            panes: vec![
                PaneLayout {
                    pane_id: 0,
                    x: 0,
                    y: 0,
                    width: 40,
                    height: 24,
                    focused: true,
                },
                PaneLayout {
                    pane_id: 1,
                    x: 41, // After divider column
                    y: 0,
                    width: 40,
                    height: 24,
                    focused: false,
                },
            ],
            screen_cols: 81,
            screen_rows: 24,
        });

        // Update left pane with full-width content
        let left_row: Vec<Cell> = (0..40).map(|_| Cell::new('L')).collect();
        buffer.apply_pane_update(0, &[PaneRow::new(0, left_row)]);

        // Update right pane with full-width content
        let right_row: Vec<Cell> = (0..40).map(|_| Cell::new('R')).collect();
        buffer.apply_pane_update(1, &[PaneRow::new(0, right_row)]);

        // Check isolation - left pane content
        let row = buffer.get_row(0).unwrap();
        for i in 0..40 {
            assert_eq!(row[i].c, 'L', "Left pane cell {} should be 'L'", i);
        }

        // Divider at column 40
        assert_eq!(row[40].c, '│', "Divider should be at column 40");

        // Right pane content
        for i in 41..81 {
            assert_eq!(row[i].c, 'R', "Right pane cell {} should be 'R'", i);
        }
    }

    #[test]
    fn test_horizontal_split_isolation() {
        let mut buffer = ScreenBuffer::new(80, 25); // 12 + 1 divider + 12

        buffer.set_layout(WindowLayout {
            panes: vec![
                PaneLayout {
                    pane_id: 0,
                    x: 0,
                    y: 0,
                    width: 80,
                    height: 12,
                    focused: true,
                },
                PaneLayout {
                    pane_id: 1,
                    x: 0,
                    y: 13, // After divider row
                    width: 80,
                    height: 12,
                    focused: false,
                },
            ],
            screen_cols: 80,
            screen_rows: 25,
        });

        // Update top pane
        let top_row: Vec<Cell> = (0..80).map(|_| Cell::new('T')).collect();
        buffer.apply_pane_update(0, &[PaneRow::new(11, top_row)]); // Last row of top pane

        // Update bottom pane
        let bottom_row: Vec<Cell> = (0..80).map(|_| Cell::new('B')).collect();
        buffer.apply_pane_update(1, &[PaneRow::new(0, bottom_row)]); // First row of bottom pane

        // Check isolation
        let row11 = buffer.get_row(11).unwrap();
        assert_eq!(row11[0].c, 'T', "Row 11 should have top pane content");

        let row12 = buffer.get_row(12).unwrap();
        assert_eq!(row12[0].c, '─', "Row 12 should be divider");

        let row13 = buffer.get_row(13).unwrap();
        assert_eq!(row13[0].c, 'B', "Row 13 should have bottom pane content");
    }

    #[test]
    fn test_three_pane_layout() {
        // Layout:
        // +--------+--------+
        // |   0    |   1    |
        // +--------+--------+
        // |        2        |
        // +-----------------+
        let mut buffer = ScreenBuffer::new(81, 25);

        buffer.set_layout(WindowLayout {
            panes: vec![
                PaneLayout {
                    pane_id: 0,
                    x: 0,
                    y: 0,
                    width: 40,
                    height: 12,
                    focused: true,
                },
                PaneLayout {
                    pane_id: 1,
                    x: 41,
                    y: 0,
                    width: 40,
                    height: 12,
                    focused: false,
                },
                PaneLayout {
                    pane_id: 2,
                    x: 0,
                    y: 13,
                    width: 81,
                    height: 12,
                    focused: false,
                },
            ],
            screen_cols: 81,
            screen_rows: 25,
        });

        // Update all three panes
        buffer.apply_pane_update(0, &[PaneRow::new(0, vec![Cell::new('A'); 40])]);
        buffer.apply_pane_update(1, &[PaneRow::new(0, vec![Cell::new('B'); 40])]);
        buffer.apply_pane_update(2, &[PaneRow::new(0, vec![Cell::new('C'); 81])]);

        // Check pane 0
        let row0 = buffer.get_row(0).unwrap();
        assert_eq!(row0[0].c, 'A');
        assert_eq!(row0[39].c, 'A');
        assert_eq!(row0[40].c, '│'); // Vertical divider
        assert_eq!(row0[41].c, 'B');

        // Check pane 2
        let row13 = buffer.get_row(13).unwrap();
        assert_eq!(row13[0].c, 'C');
        assert_eq!(row13[40].c, 'C');
        assert_eq!(row13[80].c, 'C');
    }

    #[test]
    fn test_cells_to_ansi_basic() {
        let cells = vec![Cell::new('H'), Cell::new('i'), Cell::new('!')];

        let ansi = cells_to_ansi(&cells);

        // Should contain the characters
        assert!(ansi.contains('H'));
        assert!(ansi.contains('i'));
        assert!(ansi.contains('!'));
        // Should start with reset
        assert!(ansi.starts_with("\x1b[0m"));
        // Should end with reset
        assert!(ansi.ends_with("\x1b[0m"));
    }

    #[test]
    fn test_row_emits_osc8_hyperlink() {
        let mut buffer = ScreenBuffer::new(20, 3);
        buffer.set_layout(single_pane_layout(20, 3));

        buffer.apply_pane_update(
            0,
            &[PaneRow::with_links(
                0,
                text_cells("go https://a.io x"),
                vec![link(3, 15, 42, "https://a.io")],
            )],
        );

        let ansi = buffer.render_row_ansi(0);
        assert!(
            ansi.contains("\x1b]8;id=42;https://a.io\x1b\\"),
            "no OSC 8 open in {ansi:?}"
        );
        assert!(
            ansi.contains("\x1b]8;;\x1b\\"),
            "no OSC 8 close in {ansi:?}"
        );

        // The link must cover exactly the URL text.
        let opened = ansi
            .split("\x1b]8;id=42;https://a.io\x1b\\")
            .nth(1)
            .unwrap();
        let linked: String = opened
            .split("\x1b]8;;\x1b\\")
            .next()
            .unwrap()
            .chars()
            .filter(|c| !c.is_control() && *c != '[' || c.is_alphanumeric())
            .collect();
        assert!(
            linked.contains("https://a.io"),
            "linked text was {linked:?}"
        );
        assert!(!linked.contains(" x"), "link ran past the URL: {linked:?}");
    }

    #[test]
    fn test_link_closes_at_end_of_row() {
        // A link running to the last column must be closed, or it bleeds into the
        // next row the client paints.
        let mut buffer = ScreenBuffer::new(12, 2);
        buffer.set_layout(single_pane_layout(12, 2));

        buffer.apply_pane_update(
            0,
            &[PaneRow::with_links(
                0,
                text_cells("https://a.io"),
                vec![link(0, 12, 7, "https://a.io")],
            )],
        );

        assert!(
            buffer.render_row_ansi(0).ends_with("\x1b]8;;\x1b[0m")
                || buffer.render_row_ansi(0).contains("\x1b]8;;\x1b\\\x1b[0m")
        );
    }

    #[test]
    fn test_wrapped_link_shares_one_id_across_rows() {
        let mut buffer = ScreenBuffer::new(10, 3);
        buffer.set_layout(single_pane_layout(10, 3));

        // One logical link split over two rows, as the server sends it.
        buffer.apply_pane_update(
            0,
            &[
                PaneRow::with_links(
                    0,
                    text_cells("https://a."),
                    vec![link(0, 10, 99, "https://a.io/x")],
                ),
                PaneRow::with_links(
                    1,
                    text_cells("io/x"),
                    vec![link(0, 4, 99, "https://a.io/x")],
                ),
            ],
        );

        for row in 0..2 {
            assert!(
                buffer.render_row_ansi(row).contains("\x1b]8;id=99;"),
                "row {row} lost the shared link id"
            );
        }
        assert_eq!(buffer.link_at(1, 0), Some("https://a.io/x"));
    }

    #[test]
    fn test_links_are_replaced_when_a_row_is_repainted() {
        let mut buffer = ScreenBuffer::new(20, 2);
        buffer.set_layout(single_pane_layout(20, 2));

        buffer.apply_pane_update(
            0,
            &[PaneRow::with_links(
                0,
                text_cells("https://old.example"),
                vec![link(0, 19, 1, "https://old.example")],
            )],
        );
        buffer.apply_pane_update(0, &[PaneRow::new(0, text_cells("plain text"))]);

        assert_eq!(buffer.link_at(0, 0), None, "stale link survived a repaint");
        assert!(!buffer.render_row_ansi(0).contains("\x1b]8;"));
    }

    #[test]
    fn test_link_is_clipped_to_its_pane() {
        let mut buffer = ScreenBuffer::new(20, 2);
        buffer.set_layout(WindowLayout {
            panes: vec![
                PaneLayout {
                    pane_id: 0,
                    x: 0,
                    y: 0,
                    width: 10,
                    height: 2,
                    focused: true,
                },
                PaneLayout {
                    pane_id: 1,
                    x: 11,
                    y: 0,
                    width: 9,
                    height: 2,
                    focused: false,
                },
            ],
            screen_cols: 20,
            screen_rows: 2,
        });

        // A run wider than the pane must not reach the divider or the neighbour.
        buffer.apply_pane_update(
            0,
            &[PaneRow::with_links(
                0,
                text_cells("https://a.io"),
                vec![link(0, 30, 5, "https://a.io")],
            )],
        );

        assert_eq!(buffer.link_at(0, 9), Some("https://a.io"));
        assert_eq!(buffer.link_at(0, 10), None, "link reached the divider");
        assert_eq!(buffer.link_at(0, 12), None, "link reached the next pane");
    }

    #[test]
    fn test_url_with_control_characters_is_dropped() {
        let mut buffer = ScreenBuffer::new(12, 2);
        buffer.set_layout(single_pane_layout(12, 2));

        buffer.apply_pane_update(
            0,
            &[PaneRow::with_links(
                0,
                text_cells("click me"),
                vec![link(0, 8, 3, "https://a.io\x1b]0;pwned\x07")],
            )],
        );

        assert_eq!(buffer.link_at(0, 0), None);
        assert!(!buffer.render_row_ansi(0).contains("\x1b]8;"));
    }

    #[test]
    fn test_detected_url_is_underlined() {
        let mut buffer = ScreenBuffer::new(20, 2);
        buffer.set_layout(single_pane_layout(20, 2));

        buffer.apply_pane_update(
            0,
            &[PaneRow::with_links(
                0,
                text_cells("go https://a.io x"),
                vec![link(3, 15, 1, "https://a.io")],
            )],
        );

        let row = buffer.get_row(0).unwrap();
        assert!(
            row[3].flags.contains(CellFlags::UNDERLINE),
            "detected URL should be underlined"
        );
        assert!(
            !row[0].flags.contains(CellFlags::UNDERLINE),
            "underline leaked outside the link"
        );
        assert!(
            !row[16].flags.contains(CellFlags::UNDERLINE),
            "underline leaked past the link"
        );
        // ...and the underline reaches the host terminal as an SGR.
        let ansi = buffer.render_row_ansi(0);
        assert!(ansi.contains("\x1b[0;4m"), "no underline SGR in {ansi:?}");
    }

    #[test]
    fn test_application_link_keeps_its_own_styling() {
        let mut buffer = ScreenBuffer::new(20, 2);
        buffer.set_layout(single_pane_layout(20, 2));

        // The application printed plain, unstyled text and asked for a link on it.
        buffer.apply_pane_update(
            0,
            &[PaneRow::with_links(
                0,
                text_cells("CLICKME"),
                vec![app_link(0, 7, 2, "https://a.io/osc8")],
            )],
        );

        let row = buffer.get_row(0).unwrap();
        assert!(
            !row[0].flags.contains(CellFlags::UNDERLINE),
            "clux must not restyle an application's own link"
        );
        // It is still a real hyperlink, just not restyled.
        assert_eq!(buffer.link_at(0, 0), Some("https://a.io/osc8"));
    }

    #[test]
    fn test_underline_is_dropped_when_the_link_goes_away() {
        let mut buffer = ScreenBuffer::new(20, 2);
        buffer.set_layout(single_pane_layout(20, 2));

        buffer.apply_pane_update(
            0,
            &[PaneRow::with_links(
                0,
                text_cells("https://a.io"),
                vec![link(0, 12, 1, "https://a.io")],
            )],
        );
        buffer.apply_pane_update(0, &[PaneRow::new(0, text_cells("plain text"))]);

        let row = buffer.get_row(0).unwrap();
        assert!(!row[0].flags.contains(CellFlags::UNDERLINE));
    }

    // ------------------------------------------------------------------------
    // Selection
    // ------------------------------------------------------------------------

    /// Two side-by-side 10-wide panes with a divider at column 10.
    fn split_layout() -> WindowLayout {
        WindowLayout {
            panes: vec![
                PaneLayout {
                    pane_id: 0,
                    x: 0,
                    y: 0,
                    width: 10,
                    height: 3,
                    focused: true,
                },
                PaneLayout {
                    pane_id: 1,
                    x: 11,
                    y: 0,
                    width: 10,
                    height: 3,
                    focused: false,
                },
            ],
            screen_cols: 21,
            screen_rows: 3,
        }
    }

    fn pane_row(row_idx: u16, text: &str, wrapped: bool) -> PaneRow {
        PaneRow::new(row_idx, text_cells(text)).wrapped(wrapped)
    }

    #[test]
    fn test_selection_extracts_text_on_one_row() {
        let mut buffer = ScreenBuffer::new(20, 2);
        buffer.set_layout(single_pane_layout(20, 2));
        buffer.apply_pane_update(0, &[pane_row(0, "hello world", false)]);

        assert!(buffer.begin_selection(0, 0, SelectionMode::Normal));
        buffer.extend_selection(0, 4);

        assert_eq!(buffer.selected_text().as_deref(), Some("hello"));
        assert!(buffer.has_selection());
    }

    #[test]
    fn test_selection_joins_wrapped_rows_without_a_newline() {
        // The point of shipping the wrap flag: a path broken across rows must come
        // back as one string.
        let mut buffer = ScreenBuffer::new(10, 3);
        buffer.set_layout(single_pane_layout(10, 3));
        buffer.apply_pane_update(
            0,
            &[
                pane_row(0, "/very/long", true),
                pane_row(1, "/path/here", false),
            ],
        );

        buffer.begin_selection(0, 0, SelectionMode::Normal);
        buffer.extend_selection(1, 9);

        assert_eq!(
            buffer.selected_text().as_deref(),
            Some("/very/long/path/here")
        );
    }

    #[test]
    fn test_selection_breaks_unwrapped_rows_with_a_newline() {
        let mut buffer = ScreenBuffer::new(10, 3);
        buffer.set_layout(single_pane_layout(10, 3));
        buffer.apply_pane_update(
            0,
            &[pane_row(0, "first", false), pane_row(1, "second", false)],
        );

        buffer.begin_selection(0, 0, SelectionMode::Normal);
        buffer.extend_selection(1, 9);

        assert_eq!(buffer.selected_text().as_deref(), Some("first\nsecond"));
    }

    #[test]
    fn test_selection_trims_trailing_blanks_at_hard_line_ends() {
        let mut buffer = ScreenBuffer::new(20, 2);
        buffer.set_layout(single_pane_layout(20, 2));
        buffer.apply_pane_update(0, &[pane_row(0, "text", false)]);

        buffer.begin_selection(0, 0, SelectionMode::Normal);
        buffer.extend_selection(0, 19);

        assert_eq!(buffer.selected_text().as_deref(), Some("text"));
    }

    #[test]
    fn test_selection_stays_inside_its_pane() {
        let mut buffer = ScreenBuffer::new(21, 3);
        buffer.set_layout(split_layout());
        buffer.apply_pane_update(0, &[pane_row(0, "LEFTPANE00", false)]);
        buffer.apply_pane_update(1, &[pane_row(0, "RIGHTPANE0", false)]);

        // Start in the left pane, drag well into the right one.
        buffer.begin_selection(0, 0, SelectionMode::Normal);
        buffer.extend_selection(0, 20);

        let text = buffer.selected_text().expect("selection");
        assert_eq!(text, "LEFTPANE00");
        assert!(!text.contains('│'), "selection swallowed the divider");
        assert!(!text.contains("RIGHT"), "selection crossed into pane 1");
    }

    #[test]
    fn test_selection_on_a_divider_does_not_start() {
        let mut buffer = ScreenBuffer::new(21, 3);
        buffer.set_layout(split_layout());

        assert!(!buffer.begin_selection(0, 10, SelectionMode::Normal));
        assert!(!buffer.has_selection());
        assert_eq!(buffer.selected_text(), None);
    }

    #[test]
    fn test_block_selection_keeps_rows_separate() {
        let mut buffer = ScreenBuffer::new(10, 3);
        buffer.set_layout(single_pane_layout(10, 3));
        // Wrapped rows, but a block selection is columnar: rows stay separate.
        buffer.apply_pane_update(
            0,
            &[
                pane_row(0, "abcdefghij", true),
                pane_row(1, "klmnopqrst", true),
            ],
        );

        buffer.begin_selection(0, 2, SelectionMode::Block);
        buffer.extend_selection(1, 4);

        assert_eq!(buffer.selected_text().as_deref(), Some("cde\nmno"));
    }

    #[test]
    fn test_selected_cells_are_inverted_in_the_rendered_row() {
        let mut buffer = ScreenBuffer::new(20, 2);
        buffer.set_layout(single_pane_layout(20, 2));
        buffer.apply_pane_update(0, &[pane_row(0, "hello world", false)]);

        buffer.begin_selection(0, 0, SelectionMode::Normal);
        buffer.extend_selection(0, 4);

        let ansi = buffer.render_row_ansi(0);
        assert!(ansi.contains("\x1b[0;7m"), "no inverse SGR in {ansi:?}");

        // The stored cells are untouched, so clearing needs no restore.
        assert!(!buffer.get_row(0).unwrap()[0]
            .flags
            .contains(CellFlags::INVERSE));

        buffer.clear_selection();
        assert!(!buffer.render_row_ansi(0).contains("\x1b[0;7m"));
    }

    #[test]
    fn test_selection_survives_a_pane_update_but_not_a_layout_change() {
        let mut buffer = ScreenBuffer::new(20, 2);
        buffer.set_layout(single_pane_layout(20, 2));
        buffer.apply_pane_update(0, &[pane_row(0, "hello world", false)]);

        buffer.begin_selection(0, 0, SelectionMode::Normal);
        buffer.extend_selection(0, 4);

        // New output on another row must not drop what the user selected.
        buffer.apply_pane_update(0, &[pane_row(1, "more output", false)]);
        assert_eq!(buffer.selected_text().as_deref(), Some("hello"));

        // A layout change moves everything, so the selection is meaningless.
        buffer.set_layout(single_pane_layout(20, 2));
        assert!(!buffer.has_selection());
    }

    #[test]
    fn test_cells_to_ansi_has_no_links_by_default() {
        let ansi = cells_to_ansi(&text_cells("https://a.io"));
        assert!(!ansi.contains("\x1b]8;"), "unexpected OSC 8 in {ansi:?}");
    }

    #[test]
    fn test_cells_to_ansi_colors() {
        let cells = vec![
            Cell::styled(
                'R',
                Color::rgb(255, 0, 0),
                Color::default_color(),
                CellFlags::empty(),
            ),
            Cell::styled(
                'G',
                Color::rgb(0, 255, 0),
                Color::default_color(),
                CellFlags::empty(),
            ),
        ];

        let ansi = cells_to_ansi(&cells);

        // Should contain RGB color codes
        assert!(ansi.contains("\x1b[38;2;255;0;0m")); // Red foreground
        assert!(ansi.contains("\x1b[38;2;0;255;0m")); // Green foreground
    }

    #[test]
    fn test_cells_to_ansi_attributes() {
        let cells = vec![Cell::styled(
            'B',
            Color::default_color(),
            Color::default_color(),
            CellFlags::BOLD | CellFlags::UNDERLINE,
        )];

        let ansi = cells_to_ansi(&cells);

        // Should contain attribute codes
        assert!(ansi.contains('1') || ansi.contains("1;")); // Bold
        assert!(ansi.contains('4') || ansi.contains("4;")); // Underline
    }

    #[test]
    fn test_update_nonexistent_pane() {
        let mut buffer = ScreenBuffer::new(80, 24);

        buffer.set_layout(WindowLayout {
            panes: vec![PaneLayout {
                pane_id: 0,
                x: 0,
                y: 0,
                width: 80,
                height: 24,
                focused: true,
            }],
            screen_cols: 80,
            screen_rows: 24,
        });

        // Try to update a non-existent pane - should not panic
        buffer.apply_pane_update(99, &[PaneRow::new(0, vec![Cell::new('X')])]);

        // Original content should be unchanged (default spaces)
        let row = buffer.get_row(0).unwrap();
        assert_eq!(row[0].c, ' ');
    }

    #[test]
    fn test_update_without_layout() {
        let mut buffer = ScreenBuffer::new(80, 24);

        // No layout set - update should be ignored
        buffer.apply_pane_update(0, &[PaneRow::new(0, vec![Cell::new('X')])]);

        let row = buffer.get_row(0).unwrap();
        assert_eq!(row[0].c, ' ');
    }

    #[test]
    fn test_bounds_checking() {
        let mut buffer = ScreenBuffer::new(80, 24);

        buffer.set_layout(WindowLayout {
            panes: vec![PaneLayout {
                pane_id: 0,
                x: 0,
                y: 0,
                width: 80,
                height: 24,
                focused: true,
            }],
            screen_cols: 80,
            screen_rows: 24,
        });

        // Try to update row beyond pane height - should not panic
        buffer.apply_pane_update(0, &[PaneRow::new(100, vec![Cell::new('X')])]);

        // Try to update with cells beyond pane width - should truncate
        let wide_row: Vec<Cell> = (0..200).map(|_| Cell::new('W')).collect();
        buffer.apply_pane_update(0, &[PaneRow::new(0, wide_row)]);

        // Should have written up to column 80
        let row = buffer.get_row(0).unwrap();
        assert_eq!(row[79].c, 'W');
    }
}
