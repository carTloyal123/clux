//! Growing the buffer, scrolling it, and moving the viewport.

use super::page::Page;
use super::pin::{Pin, Viewport};
use super::Buffer;
use crate::cell::Cell;

impl Buffer {
    /// Append a blank row at the end, sliding the active area down by one.
    ///
    /// This is what a full-screen scroll costs now: one row appended, and the row
    /// that leaves the active area becomes history where it already sits. The old
    /// grid had to shift every row and copy one into a separate history buffer.
    pub(super) fn append_blank_row(&mut self) {
        if self.pages.back().map(|p| !p.has_room()).unwrap_or(true) {
            self.pages.push_back(Page::new(self.cols));
        }
        // Safe: a page with room was just ensured.
        self.pages.back_mut().unwrap().push_blank_row();
        self.stored_rows += 1;
        self.enforce_budget();
    }

    /// Scroll the whole screen up by one: the top active row becomes history.
    pub fn scroll_up(&mut self) {
        self.append_blank_row();
        self.mark_all_dirty();
    }

    /// Scroll rows `[top, bottom)` of the active area up by one.
    ///
    /// A scroll region is a window inside the screen, so its rows rotate in place
    /// and nothing enters history - matching what DECSTBM means.
    pub fn scroll_region_up(&mut self, top: usize, bottom: usize) {
        let bottom = bottom.min(self.screen_rows);
        if top >= bottom {
            return;
        }

        for row in top..bottom.saturating_sub(1) {
            self.copy_active_row(row + 1, row);
        }
        self.clear_active_row(bottom - 1);
        self.mark_rows_dirty(top, bottom);
    }

    /// Scroll rows `[top, bottom)` of the active area down by one.
    pub fn scroll_region_down(&mut self, top: usize, bottom: usize) {
        let bottom = bottom.min(self.screen_rows);
        if top >= bottom {
            return;
        }

        for row in (top + 1..bottom).rev() {
            self.copy_active_row(row - 1, row);
        }
        self.clear_active_row(top);
        self.mark_rows_dirty(top, bottom);
    }

    /// Copy one active row's cells and wrap flag over another.
    fn copy_active_row(&mut self, from: usize, to: usize) {
        let Some((cells, wrapped)) = self.active_row(from) else {
            return;
        };
        let cells = cells.to_vec();
        if let Some(target) = self.active_cells_mut(to) {
            target.copy_from_slice(&cells);
        }
        self.set_row_wrapped(to, wrapped);
    }

    /// Append a row with content, allocating a page if needed.
    pub(super) fn push_row(&mut self, cells: &[Cell], wrapped: bool) {
        if self.pages.back().map(|p| !p.has_room()).unwrap_or(true) {
            self.pages.push_back(Page::new(self.cols));
        }
        self.pages.back_mut().unwrap().push_row(cells, wrapped);
        self.stored_rows += 1;
    }

    /// Drop the last row if it is blank. Used to place the active area after a
    /// reflow without leaving padding below the cursor.
    pub(super) fn pop_blank_tail_row(&mut self) -> bool {
        let Some(last) = self.stored_rows.checked_sub(1) else {
            return false;
        };
        let Some((cells, wrapped)) = self.row_at(last) else {
            return false;
        };
        if wrapped || cells.iter().any(|cell| !cell.is_empty()) {
            return false;
        }

        if let Some(page) = self.pages.back_mut() {
            page.pop_row();
            if page.len() == 0 {
                self.pages.pop_back();
            }
        }
        self.stored_rows -= 1;
        true
    }

    /// Drop history beyond the row budget, releasing whole pages when they empty.
    pub(super) fn enforce_budget_now(&mut self) {
        self.enforce_budget();
    }

    /// Drop history beyond the row budget, releasing whole pages when they empty.
    fn enforce_budget(&mut self) {
        if self.stored_rows > self.max_rows() {
            let excess = self.stored_rows - self.max_rows();
            self.stored_rows -= excess;
            self.first_abs += excess as u64;
            self.front_skip += excess;

            // Release pages once every row in them is dead: one deallocation for
            // many rows, and no copying.
            while self
                .pages
                .front()
                .map(|page| self.front_skip >= page.len())
                .unwrap_or(false)
            {
                let dropped = self.pages.pop_front().map(|p| p.len()).unwrap_or(0);
                self.front_skip -= dropped;
            }
        }

        // A pinned viewport may now point at evicted history.
        if let Viewport::Pinned(pin) = self.viewport {
            if pin.abs() < self.first_abs {
                self.viewport = Viewport::Pinned(Pin(self.first_abs));
            }
        }
    }

    /// Move the viewport through history: positive goes back, negative forward.
    /// Returns whether it moved.
    pub fn scroll_view(&mut self, lines: i32) -> bool {
        let top = self.viewport_top_abs();
        let target = if lines >= 0 {
            top.saturating_sub(lines as u64)
        } else {
            top.saturating_add(lines.unsigned_abs() as u64)
        };

        self.set_viewport_top(target.clamp(self.first_abs, self.active_start_abs()))
    }

    /// Return to following new output. Returns whether the view moved.
    pub fn reset_scroll(&mut self) -> bool {
        if self.viewport == Viewport::Active {
            return false;
        }
        self.viewport = Viewport::Active;
        self.mark_all_dirty();
        true
    }

    /// Whether the viewport is showing history.
    pub fn is_scrolled(&self) -> bool {
        self.viewport_top_abs() < self.active_start_abs()
    }

    /// How far back the viewport is, in rows.
    pub fn scroll_offset(&self) -> usize {
        (self.active_start_abs() - self.viewport_top_abs()) as usize
    }

    /// Absolute row at the top of the viewport.
    pub(super) fn viewport_top_abs(&self) -> u64 {
        match self.viewport {
            Viewport::Active => self.active_start_abs(),
            Viewport::Pinned(pin) => pin.abs(),
        }
    }

    fn set_viewport_top(&mut self, abs: u64) -> bool {
        let next = if abs >= self.active_start_abs() {
            Viewport::Active
        } else {
            Viewport::Pinned(Pin(abs))
        };

        if next == self.viewport {
            return false;
        }
        self.viewport = next;
        self.mark_all_dirty();
        true
    }
}
