//! Client-side screen buffer for hybrid rendering.
//!
//! The ScreenBuffer maintains a grid of styled cells and composites
//! pane content at the correct screen positions. This enables:
//! - Proper isolation between panes (no overwriting adjacent content)
//! - Client-side divider drawing
//! - Efficient partial updates

use crate::cell::{Cell, CellFlags, Color};
use crate::protocol::{PaneRow, WindowLayout};

/// Cursor position in screen coordinates.
#[derive(Debug, Clone, Copy, Default)]
pub struct CursorPosition {
    pub row: u16,
    pub col: u16,
    pub visible: bool,
}

/// Client-side screen buffer for compositing pane content.
pub struct ScreenBuffer {
    /// 2D grid of cells (row-major order).
    cells: Vec<Vec<Cell>>,
    /// Current window layout.
    layout: Option<WindowLayout>,
    /// Screen width in columns.
    cols: usize,
    /// Screen height in rows.
    rows: usize,
    /// Current cursor position (screen coordinates, for focused pane).
    cursor: CursorPosition,
}

impl ScreenBuffer {
    /// Create a new screen buffer with the given dimensions.
    pub fn new(cols: usize, rows: usize) -> Self {
        let cells = vec![vec![Cell::default(); cols]; rows];
        Self {
            cells,
            layout: None,
            cols,
            rows,
            cursor: CursorPosition::default(),
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

        // Apply each row update
        for pane_row in changed_rows {
            let screen_row = pane.y as usize + pane_row.row_idx as usize;

            // Bounds check
            if screen_row >= self.rows {
                continue;
            }

            // Copy cells to the correct screen position
            for (col_offset, cell) in pane_row.cells.iter().enumerate() {
                let screen_col = pane.x as usize + col_offset;

                // Bounds check - don't overflow pane width
                if col_offset >= pane.width as usize {
                    break;
                }
                if screen_col >= self.cols {
                    break;
                }

                self.cells[screen_row][screen_col] = *cell;
            }
        }
    }

    /// Resize the screen buffer.
    /// Clears all content and resets layout.
    pub fn resize(&mut self, cols: usize, rows: usize) {
        self.cols = cols;
        self.rows = rows;
        self.cells = vec![vec![Cell::default(); cols]; rows];
        self.layout = None;
        self.cursor = CursorPosition::default();
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
    }

    /// Get a row of cells.
    pub fn get_row(&self, row_idx: usize) -> Option<&[Cell]> {
        self.cells.get(row_idx).map(|r| r.as_slice())
    }

    /// Render a row to an ANSI escape sequence string.
    pub fn render_row_ansi(&self, row_idx: usize) -> String {
        let Some(row) = self.cells.get(row_idx) else {
            return String::new();
        };

        cells_to_ansi(row)
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
    let mut output = String::with_capacity(cells.len() * 2);

    // Track current state to minimize escape codes
    let mut current_fg = Color::default_color();
    let mut current_bg = Color::default_color();
    let mut current_flags = CellFlags::empty();

    // Reset to known state
    output.push_str("\x1b[0m");

    for cell in cells {
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::protocol::PaneLayout;

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
