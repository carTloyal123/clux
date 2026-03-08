//! Terminal rendering.
//!
//! Renders the terminal grid to the host terminal using crossterm.
//! Implements damage tracking and batched rendering for performance.

// Some methods are kept for future use / API completeness
#![allow(dead_code)]

use std::io::{self, Stdout, Write};
use std::time::{Duration, Instant};

use crossterm::{
    cursor::{Hide, MoveTo, Show},
    execute, queue,
    style::{
        Attribute, Color, Print, ResetColor, SetAttribute, SetBackgroundColor, SetForegroundColor,
    },
    terminal::{self, Clear, ClearType, EnterAlternateScreen, LeaveAlternateScreen},
};

use crate::cell::{Cell, CellFlags, Color as CellColor, ColorKind, HyperlinkId};
use crate::grid::Grid;
use crate::hyperlink::HyperlinkStore;
use crate::scrollback::Scrollback;
use crate::selection::{Point, Selection};
use crate::terminal::{Cursor, Terminal};

/// Frame budget for different refresh rates.
const FRAME_BUDGET_60HZ: Duration = Duration::from_micros(16_667);
const FRAME_BUDGET_120HZ: Duration = Duration::from_micros(8_333);

/// Renderer for outputting the terminal grid.
pub struct Renderer {
    /// Stdout handle for writing.
    stdout: Stdout,
    /// Target refresh rate.
    target_fps: u32,
    /// Frame budget based on target FPS.
    frame_budget: Duration,
    /// Last render time for frame pacing.
    last_render: Instant,
    /// Previous frame's state for differential rendering.
    prev_cells: Vec<Cell>,
    /// Whether we're in alternate screen mode.
    in_alt_screen: bool,
    /// Last cursor position.
    last_cursor: (u16, u16),
    /// Last cursor visibility.
    last_cursor_visible: bool,
}

impl Renderer {
    /// Create a new renderer.
    pub fn new() -> Self {
        let fps = Self::detect_refresh_rate();
        Self {
            stdout: io::stdout(),
            target_fps: fps,
            frame_budget: if fps >= 120 {
                FRAME_BUDGET_120HZ
            } else {
                FRAME_BUDGET_60HZ
            },
            last_render: Instant::now(),
            prev_cells: Vec::new(),
            in_alt_screen: false,
            last_cursor: (0, 0),
            last_cursor_visible: true,
        }
    }

    /// Detect the target refresh rate.
    pub fn detect_refresh_rate() -> u32 {
        // Check for Ghostty (supports 120Hz)
        if std::env::var("GHOSTTY_RESOURCES_DIR").is_ok() {
            return 120;
        }

        // Check for Kitty
        if std::env::var("KITTY_WINDOW_ID").is_ok() {
            return 120;
        }

        // Check for Alacritty
        if std::env::var("ALACRITTY_LOG").is_ok() || std::env::var("ALACRITTY_SOCKET").is_ok() {
            return 120;
        }

        // Default to 60Hz
        60
    }

    /// Get the frame budget for the current refresh rate.
    pub fn frame_budget(&self) -> Duration {
        self.frame_budget
    }

    /// Get the target FPS.
    pub fn target_fps(&self) -> u32 {
        self.target_fps
    }

    /// Enter alternate screen and enable raw mode.
    pub fn enter(&mut self) -> io::Result<()> {
        terminal::enable_raw_mode()?;
        execute!(self.stdout, EnterAlternateScreen, Hide)?;
        self.in_alt_screen = true;
        Ok(())
    }

    /// Leave alternate screen and disable raw mode.
    pub fn leave(&mut self) -> io::Result<()> {
        execute!(self.stdout, Show, LeaveAlternateScreen)?;
        terminal::disable_raw_mode()?;
        self.in_alt_screen = false;
        Ok(())
    }

    /// Render the grid incrementally (only dirty rows).
    pub fn render(&mut self, grid: &Grid, cursor: &Cursor) -> io::Result<()> {
        let rows = grid.rows();
        let cols = grid.cols();

        // Resize previous cells buffer if needed
        let total_cells = rows * cols;
        if self.prev_cells.len() != total_cells {
            self.prev_cells.resize(total_cells, Cell::default());
            // Force full redraw on resize
            self.render_full(grid, cursor)?;
            return Ok(());
        }

        // Begin synchronized update (reduces tearing in supported terminals)
        self.begin_sync_update()?;

        // Hide cursor during render
        queue!(self.stdout, Hide)?;

        // Render dirty rows
        for row_idx in grid.dirty_row_indices() {
            self.render_row(grid, row_idx, cols)?;
        }

        // Update cursor
        self.update_cursor(cursor)?;

        // End synchronized update
        self.end_sync_update()?;

        self.stdout.flush()?;
        self.last_render = Instant::now();

        Ok(())
    }

    /// Render the terminal with scrollback support.
    pub fn render_with_scrollback(&mut self, term: &Terminal) -> io::Result<()> {
        let rows = term.rows();
        let cols = term.cols();
        let scroll_offset = term.scroll_offset;

        // Check if we need a full redraw (resize detected)
        let total_cells = rows * cols;
        let needs_full_redraw = self.prev_cells.len() != total_cells;
        if needs_full_redraw {
            self.prev_cells.resize(total_cells, Cell::default());
        }

        // Begin synchronized update
        self.begin_sync_update()?;
        queue!(self.stdout, Hide)?;

        if scroll_offset == 0 {
            // Not scrolled - render normally
            if needs_full_redraw {
                // Full redraw on resize
                queue!(self.stdout, Clear(ClearType::All))?;
                for row_idx in 0..rows {
                    self.render_row(&term.grid, row_idx, cols)?;
                }
            } else {
                // Incremental render
                for row_idx in term.grid.dirty_row_indices() {
                    self.render_row(&term.grid, row_idx, cols)?;
                }
            }
        } else {
            // Scrolled into history - always render all rows when viewing scrollback
            // (or on resize)
            for row_idx in 0..rows {
                self.render_scrollback_row(
                    &term.grid,
                    &term.scrollback,
                    row_idx,
                    cols,
                    scroll_offset,
                )?;
            }
            // Always render scroll indicator when scrolled
            self.render_scroll_indicator(scroll_offset, term.scrollback.len(), cols)?;
        }

        // Show cursor only when not viewing scrollback
        if scroll_offset == 0 {
            self.update_cursor(&term.cursor)?;
        } else {
            queue!(self.stdout, Hide)?;
        }

        self.end_sync_update()?;
        self.stdout.flush()?;
        self.last_render = Instant::now();

        Ok(())
    }

    /// Render the terminal with selection highlighting.
    pub fn render_with_selection(
        &mut self,
        term: &Terminal,
        selection: Option<&Selection>,
    ) -> io::Result<()> {
        let rows = term.rows();
        let cols = term.cols();
        let scroll_offset = term.scroll_offset;

        // Check if we need a full redraw (resize detected)
        let total_cells = rows * cols;
        let needs_full_redraw = self.prev_cells.len() != total_cells;
        if needs_full_redraw {
            self.prev_cells.resize(total_cells, Cell::default());
        }

        // Begin synchronized update
        self.begin_sync_update()?;
        queue!(self.stdout, Hide)?;

        // Always do full redraw when selection exists for simplicity
        let do_full_redraw = needs_full_redraw || selection.is_some();

        if scroll_offset == 0 {
            // Not scrolled - render normally
            if do_full_redraw {
                queue!(self.stdout, Clear(ClearType::All))?;
                for row_idx in 0..rows {
                    let line = row_idx as i32;
                    self.render_row_with_selection(
                        &term.grid,
                        row_idx,
                        cols,
                        selection,
                        line,
                        &term.hyperlinks,
                    )?;
                }
            } else {
                // Incremental render
                for row_idx in term.grid.dirty_row_indices() {
                    let line = row_idx as i32;
                    self.render_row_with_selection(
                        &term.grid,
                        row_idx,
                        cols,
                        selection,
                        line,
                        &term.hyperlinks,
                    )?;
                }
            }
        } else {
            // Scrolled into history - render with scrollback
            for row_idx in 0..rows {
                let line = self.screen_row_to_line(row_idx, scroll_offset, rows);
                self.render_scrollback_row_with_selection(
                    &term.grid,
                    &term.scrollback,
                    row_idx,
                    cols,
                    scroll_offset,
                    selection,
                    line,
                    &term.hyperlinks,
                )?;
            }
            // Render scroll indicator
            self.render_scroll_indicator(scroll_offset, term.scrollback.len(), cols)?;
        }

        // Show cursor only when not viewing scrollback and no selection
        if scroll_offset == 0 && selection.is_none() {
            self.update_cursor(&term.cursor)?;
        } else {
            queue!(self.stdout, Hide)?;
        }

        self.end_sync_update()?;
        self.stdout.flush()?;
        self.last_render = Instant::now();

        Ok(())
    }

    /// Convert screen row to line coordinate for selection matching.
    fn screen_row_to_line(&self, screen_row: usize, scroll_offset: usize, _rows: usize) -> i32 {
        if scroll_offset == 0 {
            screen_row as i32
        } else {
            let scrollback_rows_visible = scroll_offset;
            if screen_row < scrollback_rows_visible {
                -((scrollback_rows_visible - screen_row) as i32)
            } else {
                (screen_row - scrollback_rows_visible) as i32
            }
        }
    }

    /// Render a grid row with selection highlighting.
    fn render_row_with_selection(
        &mut self,
        grid: &Grid,
        row_idx: usize,
        cols: usize,
        selection: Option<&Selection>,
        line: i32,
        hyperlinks: &HyperlinkStore,
    ) -> io::Result<()> {
        queue!(self.stdout, MoveTo(0, row_idx as u16))?;

        if let Some(row) = grid.row(row_idx) {
            let cells: Vec<Cell> = (0..cols)
                .map(|c| row.get(c).copied().unwrap_or_default())
                .collect();
            self.render_cells_with_selection(&cells, cols, selection, line, hyperlinks)?;
        } else {
            self.render_empty_row(cols)?;
        }

        Ok(())
    }

    /// Render a scrollback row with selection highlighting.
    fn render_scrollback_row_with_selection(
        &mut self,
        grid: &Grid,
        scrollback: &Scrollback,
        row_idx: usize,
        cols: usize,
        scroll_offset: usize,
        selection: Option<&Selection>,
        line: i32,
        hyperlinks: &HyperlinkStore,
    ) -> io::Result<()> {
        queue!(self.stdout, MoveTo(0, row_idx as u16))?;

        let scrollback_lines_to_show = scroll_offset.min(grid.rows());

        if row_idx < scrollback_lines_to_show {
            let sb_idx = scroll_offset - row_idx - 1;
            if let Some(sb_line) = scrollback.get(sb_idx) {
                self.render_cells_with_selection(
                    sb_line.cells(),
                    cols,
                    selection,
                    line,
                    hyperlinks,
                )?;
            } else {
                self.render_empty_row(cols)?;
            }
        } else {
            let grid_row = row_idx - scrollback_lines_to_show;
            if let Some(row) = grid.row(grid_row) {
                let cells: Vec<Cell> = (0..cols)
                    .map(|c| row.get(c).copied().unwrap_or_default())
                    .collect();
                self.render_cells_with_selection(&cells, cols, selection, line, hyperlinks)?;
            } else {
                self.render_empty_row(cols)?;
            }
        }

        Ok(())
    }

    /// Render cells with selection highlighting.
    fn render_cells_with_selection(
        &mut self,
        cells: &[Cell],
        cols: usize,
        selection: Option<&Selection>,
        line: i32,
        hyperlinks: &HyperlinkStore,
    ) -> io::Result<()> {
        let mut current_fg: Option<CellColor> = None;
        let mut current_bg: Option<CellColor> = None;
        let mut current_flags: Option<CellFlags> = None;
        let mut current_selected: Option<bool> = None;
        let mut current_hyperlink_id: Option<HyperlinkId> = None;
        let mut buffer = String::with_capacity(cols);

        for (col_idx, cell) in cells.iter().take(cols).enumerate() {
            let is_selected = selection
                .map(|sel| sel.active && sel.contains(Point::new(line, col_idx)))
                .unwrap_or(false);

            // Check if hyperlink changed (different ID or None vs Some)
            let hyperlink_changed = current_hyperlink_id != cell.hyperlink;

            let style_changed = current_fg.as_ref() != Some(&cell.fg)
                || current_bg.as_ref() != Some(&cell.bg)
                || current_flags.as_ref() != Some(&cell.flags)
                || current_selected != Some(is_selected)
                || hyperlink_changed;

            if style_changed {
                if !buffer.is_empty() {
                    queue!(self.stdout, Print(&buffer))?;
                    buffer.clear();
                }

                // Close previous hyperlink if there was one
                if current_hyperlink_id.is_some() && hyperlink_changed {
                    // OSC 8 close: ESC ] 8 ; ; ESC \
                    write!(self.stdout, "\x1b]8;;\x1b\\")?;
                }

                // Open new hyperlink if this cell has one
                if let Some(id) = cell.hyperlink {
                    if hyperlink_changed {
                        if let Some(url) = hyperlinks.get(id) {
                            // OSC 8 open: ESC ] 8 ; ; URL ESC \
                            write!(self.stdout, "\x1b]8;;{}\x1b\\", url)?;
                        }
                    }
                }

                if is_selected {
                    // Invert colors for selection
                    self.apply_selection_style(cell)?;
                } else if cell.hyperlink.is_some() {
                    // Hyperlinks get underline and bold styling
                    self.apply_hyperlink_style(cell)?;
                } else {
                    self.apply_cell_style(cell)?;
                }
                current_fg = Some(cell.fg);
                current_bg = Some(cell.bg);
                current_flags = Some(cell.flags);
                current_selected = Some(is_selected);
                current_hyperlink_id = cell.hyperlink;
            }
            buffer.push(cell.c);
        }

        // Pad with spaces
        for col_idx in cells.len()..cols {
            let is_selected = selection
                .map(|sel| sel.active && sel.contains(Point::new(line, col_idx)))
                .unwrap_or(false);

            // Close hyperlink before padding if one was open
            if current_hyperlink_id.is_some() {
                if !buffer.is_empty() {
                    queue!(self.stdout, Print(&buffer))?;
                    buffer.clear();
                }
                write!(self.stdout, "\x1b]8;;\x1b\\")?;
                current_hyperlink_id = None;
            }

            if current_selected != Some(is_selected) {
                if !buffer.is_empty() {
                    queue!(self.stdout, Print(&buffer))?;
                    buffer.clear();
                }
                if is_selected {
                    queue!(
                        self.stdout,
                        SetForegroundColor(Color::Black),
                        SetBackgroundColor(Color::White)
                    )?;
                } else {
                    queue!(self.stdout, ResetColor)?;
                }
                current_selected = Some(is_selected);
            }
            buffer.push(' ');
        }

        if !buffer.is_empty() {
            queue!(self.stdout, Print(&buffer))?;
        }

        // Close any open hyperlink at end of line
        if current_hyperlink_id.is_some() {
            write!(self.stdout, "\x1b]8;;\x1b\\")?;
        }

        queue!(self.stdout, ResetColor, SetAttribute(Attribute::Reset))?;
        Ok(())
    }

    /// Apply inverted style for selected cells.
    fn apply_selection_style(&mut self, cell: &Cell) -> io::Result<()> {
        queue!(self.stdout, SetAttribute(Attribute::Reset))?;

        // For selection, use inverted colors (swap fg/bg)
        // If colors are default, use white background with black text
        let (fg_color, bg_color) = match (cell.fg.kind, cell.bg.kind) {
            (ColorKind::Default, ColorKind::Default) => {
                // Default colors - use white on black for selection
                (Color::Black, Color::White)
            }
            _ => {
                // Invert the actual colors
                let fg = match cell.bg.kind {
                    ColorKind::Default => Color::Black,
                    ColorKind::Indexed => Color::AnsiValue(cell.bg.r),
                    ColorKind::Rgb => Color::Rgb {
                        r: cell.bg.r,
                        g: cell.bg.g,
                        b: cell.bg.b,
                    },
                };
                let bg = match cell.fg.kind {
                    ColorKind::Default => Color::White,
                    ColorKind::Indexed => Color::AnsiValue(cell.fg.r),
                    ColorKind::Rgb => Color::Rgb {
                        r: cell.fg.r,
                        g: cell.fg.g,
                        b: cell.fg.b,
                    },
                };
                (fg, bg)
            }
        };

        queue!(
            self.stdout,
            SetForegroundColor(fg_color),
            SetBackgroundColor(bg_color)
        )?;

        // Apply text attributes
        if cell.flags.contains(CellFlags::BOLD) {
            queue!(self.stdout, SetAttribute(Attribute::Bold))?;
        }
        if cell.flags.contains(CellFlags::ITALIC) {
            queue!(self.stdout, SetAttribute(Attribute::Italic))?;
        }
        if cell.flags.contains(CellFlags::UNDERLINE) {
            queue!(self.stdout, SetAttribute(Attribute::Underlined))?;
        }

        Ok(())
    }

    /// Apply style for hyperlinked cells (underline + bold + blue color).
    fn apply_hyperlink_style(&mut self, cell: &Cell) -> io::Result<()> {
        queue!(self.stdout, SetAttribute(Attribute::Reset))?;

        // Use blue foreground for hyperlinks (like web browsers)
        queue!(
            self.stdout,
            SetForegroundColor(Color::Blue),
            SetAttribute(Attribute::Bold),
            SetAttribute(Attribute::Underlined)
        )?;

        // Keep original background
        match cell.bg.kind {
            ColorKind::Default => {
                queue!(self.stdout, SetBackgroundColor(Color::Reset))?;
            }
            ColorKind::Indexed => {
                queue!(self.stdout, SetBackgroundColor(Color::AnsiValue(cell.bg.r)))?;
            }
            ColorKind::Rgb => {
                queue!(
                    self.stdout,
                    SetBackgroundColor(Color::Rgb {
                        r: cell.bg.r,
                        g: cell.bg.g,
                        b: cell.bg.b
                    })
                )?;
            }
        }

        // Apply additional cell flags (italic, etc.)
        if cell.flags.contains(CellFlags::ITALIC) {
            queue!(self.stdout, SetAttribute(Attribute::Italic))?;
        }

        Ok(())
    }

    /// Render a row that may come from scrollback or visible grid.
    fn render_scrollback_row(
        &mut self,
        grid: &Grid,
        scrollback: &Scrollback,
        row_idx: usize,
        cols: usize,
        scroll_offset: usize,
    ) -> io::Result<()> {
        queue!(self.stdout, MoveTo(0, row_idx as u16))?;

        // Calculate which line to show
        // With scroll_offset=N, the top N rows show scrollback
        // row 0 shows scrollback line (scroll_offset - 1)
        // row (scroll_offset) shows grid row 0

        let scrollback_lines_to_show = scroll_offset.min(grid.rows());

        if row_idx < scrollback_lines_to_show {
            // This row shows a scrollback line
            let sb_idx = scroll_offset - row_idx - 1;
            if let Some(sb_line) = scrollback.get(sb_idx) {
                self.render_cells_row(sb_line.cells(), cols)?;
            } else {
                // Empty line
                self.render_empty_row(cols)?;
            }
        } else {
            // This row shows a grid line
            let grid_row = row_idx - scrollback_lines_to_show;
            if let Some(row) = grid.row(grid_row) {
                let cells: Vec<Cell> = (0..cols)
                    .map(|c| row.get(c).copied().unwrap_or_default())
                    .collect();
                self.render_cells_row(&cells, cols)?;
            } else {
                self.render_empty_row(cols)?;
            }
        }

        Ok(())
    }

    /// Render a row from a slice of cells.
    fn render_cells_row(&mut self, cells: &[Cell], cols: usize) -> io::Result<()> {
        let mut current_fg: Option<CellColor> = None;
        let mut current_bg: Option<CellColor> = None;
        let mut current_flags: Option<CellFlags> = None;
        let mut buffer = String::with_capacity(cols);

        for cell in cells.iter().take(cols) {
            let style_changed = current_fg.as_ref() != Some(&cell.fg)
                || current_bg.as_ref() != Some(&cell.bg)
                || current_flags.as_ref() != Some(&cell.flags);

            if style_changed {
                if !buffer.is_empty() {
                    queue!(self.stdout, Print(&buffer))?;
                    buffer.clear();
                }
                self.apply_cell_style(cell)?;
                current_fg = Some(cell.fg);
                current_bg = Some(cell.bg);
                current_flags = Some(cell.flags);
            }
            buffer.push(cell.c);
        }

        // Pad with spaces if line is shorter than cols
        for _ in cells.len()..cols {
            buffer.push(' ');
        }

        if !buffer.is_empty() {
            queue!(self.stdout, Print(&buffer))?;
        }

        queue!(self.stdout, ResetColor, SetAttribute(Attribute::Reset))?;
        Ok(())
    }

    /// Render an empty row.
    fn render_empty_row(&mut self, cols: usize) -> io::Result<()> {
        queue!(self.stdout, ResetColor)?;
        let spaces: String = " ".repeat(cols);
        queue!(self.stdout, Print(&spaces))?;
        Ok(())
    }

    /// Render scroll position indicator.
    fn render_scroll_indicator(
        &mut self,
        offset: usize,
        total: usize,
        rows: usize,
    ) -> io::Result<()> {
        // Show indicator in top-right corner
        let indicator = format!("[{}/{}]", offset, total);
        let col = rows.saturating_sub(indicator.len());

        queue!(
            self.stdout,
            MoveTo(col as u16, 0),
            SetForegroundColor(Color::Black),
            SetBackgroundColor(Color::White),
            Print(&indicator),
            ResetColor
        )?;

        Ok(())
    }

    /// Render the entire grid (for initial render or resize).
    pub fn render_full(&mut self, grid: &Grid, cursor: &Cursor) -> io::Result<()> {
        let rows = grid.rows();
        let cols = grid.cols();

        // Resize previous cells buffer
        let total_cells = rows * cols;
        self.prev_cells.resize(total_cells, Cell::default());

        // Begin synchronized update
        self.begin_sync_update()?;

        // Clear screen and hide cursor
        queue!(self.stdout, Clear(ClearType::All), Hide)?;

        // Render all rows
        for row_idx in 0..rows {
            self.render_row(grid, row_idx, cols)?;
        }

        // Update cursor
        self.update_cursor(cursor)?;

        // End synchronized update
        self.end_sync_update()?;

        self.stdout.flush()?;
        self.last_render = Instant::now();

        Ok(())
    }

    /// Render a single row with batched cell output.
    fn render_row(&mut self, grid: &Grid, row_idx: usize, cols: usize) -> io::Result<()> {
        let row = match grid.row(row_idx) {
            Some(r) => r,
            None => return Ok(()),
        };

        // Move to start of row
        queue!(self.stdout, MoveTo(0, row_idx as u16))?;

        // Track current style for batching
        let mut current_fg: Option<CellColor> = None;
        let mut current_bg: Option<CellColor> = None;
        let mut current_flags: Option<CellFlags> = None;
        let mut buffer = String::with_capacity(cols);

        for (col_idx, cell) in row.iter_enumerated() {
            // Check if style changed
            let style_changed = current_fg.as_ref() != Some(&cell.fg)
                || current_bg.as_ref() != Some(&cell.bg)
                || current_flags.as_ref() != Some(&cell.flags);

            if style_changed {
                // Flush buffer with previous style
                if !buffer.is_empty() {
                    queue!(self.stdout, Print(&buffer))?;
                    buffer.clear();
                }

                // Apply new style
                self.apply_cell_style(cell)?;
                current_fg = Some(cell.fg);
                current_bg = Some(cell.bg);
                current_flags = Some(cell.flags);
            }

            // Add character to buffer
            buffer.push(cell.c);

            // Update previous cells
            let idx = row_idx * cols + col_idx;
            if idx < self.prev_cells.len() {
                self.prev_cells[idx] = *cell;
            }
        }

        // Flush remaining buffer
        if !buffer.is_empty() {
            queue!(self.stdout, Print(&buffer))?;
        }

        // Reset attributes at end of line
        queue!(self.stdout, ResetColor, SetAttribute(Attribute::Reset))?;

        Ok(())
    }

    /// Apply cell style (colors and attributes).
    fn apply_cell_style(&mut self, cell: &Cell) -> io::Result<()> {
        // Reset first to clear previous attributes
        queue!(self.stdout, SetAttribute(Attribute::Reset))?;

        // Set foreground color
        match cell.fg.kind {
            ColorKind::Default => {
                queue!(self.stdout, SetForegroundColor(Color::Reset))?;
            }
            ColorKind::Indexed => {
                queue!(self.stdout, SetForegroundColor(Color::AnsiValue(cell.fg.r)))?;
            }
            ColorKind::Rgb => {
                queue!(
                    self.stdout,
                    SetForegroundColor(Color::Rgb {
                        r: cell.fg.r,
                        g: cell.fg.g,
                        b: cell.fg.b
                    })
                )?;
            }
        }

        // Set background color
        match cell.bg.kind {
            ColorKind::Default => {
                queue!(self.stdout, SetBackgroundColor(Color::Reset))?;
            }
            ColorKind::Indexed => {
                queue!(self.stdout, SetBackgroundColor(Color::AnsiValue(cell.bg.r)))?;
            }
            ColorKind::Rgb => {
                queue!(
                    self.stdout,
                    SetBackgroundColor(Color::Rgb {
                        r: cell.bg.r,
                        g: cell.bg.g,
                        b: cell.bg.b
                    })
                )?;
            }
        }

        // Apply attributes
        if cell.flags.contains(CellFlags::BOLD) {
            queue!(self.stdout, SetAttribute(Attribute::Bold))?;
        }
        if cell.flags.contains(CellFlags::DIM) {
            queue!(self.stdout, SetAttribute(Attribute::Dim))?;
        }
        if cell.flags.contains(CellFlags::ITALIC) {
            queue!(self.stdout, SetAttribute(Attribute::Italic))?;
        }
        if cell.flags.contains(CellFlags::UNDERLINE) {
            queue!(self.stdout, SetAttribute(Attribute::Underlined))?;
        }
        if cell.flags.contains(CellFlags::BLINK) {
            queue!(self.stdout, SetAttribute(Attribute::SlowBlink))?;
        }
        if cell.flags.contains(CellFlags::INVERSE) {
            queue!(self.stdout, SetAttribute(Attribute::Reverse))?;
        }
        if cell.flags.contains(CellFlags::HIDDEN) {
            queue!(self.stdout, SetAttribute(Attribute::Hidden))?;
        }
        if cell.flags.contains(CellFlags::STRIKETHROUGH) {
            queue!(self.stdout, SetAttribute(Attribute::CrossedOut))?;
        }

        Ok(())
    }

    /// Update cursor position and visibility.
    fn update_cursor(&mut self, cursor: &Cursor) -> io::Result<()> {
        let pos = (cursor.col as u16, cursor.row as u16);

        // Move cursor
        if pos != self.last_cursor {
            queue!(self.stdout, MoveTo(pos.0, pos.1))?;
            self.last_cursor = pos;
        }

        // Update visibility
        if cursor.visible != self.last_cursor_visible {
            if cursor.visible {
                queue!(self.stdout, Show)?;
            } else {
                queue!(self.stdout, Hide)?;
            }
            self.last_cursor_visible = cursor.visible;
        } else if cursor.visible {
            queue!(self.stdout, Show)?;
        }

        Ok(())
    }

    /// Begin synchronized update (DCS sequence).
    /// Supported by many modern terminals to reduce flicker.
    fn begin_sync_update(&mut self) -> io::Result<()> {
        // DCS = ESC P, BSU (Begin Synchronized Update) = = 1 s, ST = ESC \
        write!(self.stdout, "\x1bP=1s\x1b\\")?;
        Ok(())
    }

    /// End synchronized update.
    fn end_sync_update(&mut self) -> io::Result<()> {
        // ESU (End Synchronized Update) = = 2 s
        write!(self.stdout, "\x1bP=2s\x1b\\")?;
        Ok(())
    }

    /// Check if enough time has passed for the next frame.
    pub fn should_render(&self) -> bool {
        self.last_render.elapsed() >= self.frame_budget
    }

    /// Get time until next frame.
    pub fn time_until_next_frame(&self) -> Duration {
        let elapsed = self.last_render.elapsed();
        if elapsed >= self.frame_budget {
            Duration::ZERO
        } else {
            self.frame_budget - elapsed
        }
    }
}

impl Default for Renderer {
    fn default() -> Self {
        Self::new()
    }
}

impl Drop for Renderer {
    fn drop(&mut self) {
        if self.in_alt_screen {
            let _ = self.leave();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_detect_refresh_rate() {
        // Default should be 60
        let fps = Renderer::detect_refresh_rate();
        assert!(fps == 60 || fps == 120);
    }

    #[test]
    fn test_frame_budget() {
        let renderer = Renderer::new();
        let budget = renderer.frame_budget();
        // Should be either ~16ms (60Hz) or ~8ms (120Hz)
        assert!(budget <= Duration::from_millis(20));
        assert!(budget >= Duration::from_millis(5));
    }
}
