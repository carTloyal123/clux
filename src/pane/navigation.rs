//! Closing panes and moving focus between them.

use super::{Direction, PaneId};
impl super::PaneManager {
    /// Close the focused pane.
    pub fn close_focused(&mut self) -> Option<PaneId> {
        if self.panes.len() <= 1 {
            return None; // Don't close the last pane
        }

        let closed_id = self.focused;

        // Remove from layout
        if let Some(replacement) = self.layout.remove_pane(closed_id) {
            self.layout = *replacement;
        }

        // Remove the pane
        self.panes.remove(&closed_id);

        // Remove closed pane from focus history
        self.focus_history.retain(|&h| h != closed_id);

        // Recalculate rects
        let mut rects = Vec::new();
        self.layout.calculate_rects(self.screen_rect, &mut rects);

        // Resize remaining panes
        for (id, rect) in &rects {
            if let Some(pane) = self.panes.get_mut(id) {
                if pane.rect != *rect {
                    let _ = pane.resize(*rect);
                }
            }
        }

        // Focus previous pane from history, or fall back to first pane
        let next_focus = self
            .focus_history
            .pop()
            .filter(|id| self.panes.contains_key(id))
            .or_else(|| rects.first().map(|(id, _)| *id));

        if let Some(id) = next_focus {
            // Directly set focus without adding to history (we just popped from it)
            self.focused = id;
            if let Some(pane) = self.panes.get_mut(&id) {
                pane.focused = true;
            }
            Some(id)
        } else {
            None
        }
    }
    /// Close a specific pane by ID.
    pub fn close_pane(&mut self, id: PaneId) -> Option<PaneId> {
        if self.panes.len() <= 1 {
            return None; // Don't close the last pane
        }

        // Remove from layout
        if let Some(replacement) = self.layout.remove_pane(id) {
            self.layout = *replacement;
        }

        // Remove the pane
        self.panes.remove(&id);

        // Remove closed pane from focus history
        self.focus_history.retain(|&h| h != id);

        // Recalculate rects
        let mut rects = Vec::new();
        self.layout.calculate_rects(self.screen_rect, &mut rects);

        // Resize remaining panes
        for (pane_id, rect) in &rects {
            if let Some(pane) = self.panes.get_mut(pane_id) {
                if pane.rect != *rect {
                    let _ = pane.resize(*rect);
                }
            }
        }

        // If we closed the focused pane, focus previous from history or first pane
        if self.focused == id {
            let next_focus = self
                .focus_history
                .pop()
                .filter(|id| self.panes.contains_key(id))
                .or_else(|| rects.first().map(|(id, _)| *id));

            if let Some(new_id) = next_focus {
                // Directly set focus without adding to history
                self.focused = new_id;
                if let Some(pane) = self.panes.get_mut(&new_id) {
                    pane.focused = true;
                }
                return Some(new_id);
            }
        }

        Some(self.focused)
    }
    /// Focus a specific pane.
    pub fn focus(&mut self, id: PaneId) {
        if self.panes.contains_key(&id) && id != self.focused {
            // Unfocus current and add to history
            if let Some(pane) = self.panes.get_mut(&self.focused) {
                pane.focused = false;
            }
            // Remove from history if already present (to avoid duplicates)
            self.focus_history.retain(|&h| h != self.focused);
            // Add current focused to history
            self.focus_history.push(self.focused);
            // Focus new
            self.focused = id;
            if let Some(pane) = self.panes.get_mut(&id) {
                pane.focused = true;
            }
        }
    }
    /// Navigate to an adjacent pane.
    pub fn navigate(&mut self, direction: Direction) {
        let current_rect = match self.panes.get(&self.focused) {
            Some(pane) => pane.rect,
            None => return,
        };

        // Find the center of the current pane
        let cx = current_rect.x + current_rect.width / 2;
        let cy = current_rect.y + current_rect.height / 2;

        // Find the best candidate in the given direction
        let mut best: Option<(PaneId, i32)> = None;

        for (id, pane) in &self.panes {
            if *id == self.focused {
                continue;
            }

            let pr = pane.rect;
            let px = pr.x + pr.width / 2;
            let py = pr.y + pr.height / 2;

            let (is_valid, distance) = match direction {
                Direction::Up => (
                    pr.y + pr.height <= current_rect.y,
                    (cy as i32 - py as i32).abs(),
                ),
                Direction::Down => (
                    pr.y >= current_rect.y + current_rect.height,
                    (py as i32 - cy as i32).abs(),
                ),
                Direction::Left => (
                    pr.x + pr.width <= current_rect.x,
                    (cx as i32 - px as i32).abs(),
                ),
                Direction::Right => (
                    pr.x >= current_rect.x + current_rect.width,
                    (px as i32 - cx as i32).abs(),
                ),
            };

            if is_valid {
                if best.is_none() || distance < best.unwrap().1 {
                    best = Some((*id, distance));
                }
            }
        }

        if let Some((id, _)) = best {
            self.focus(id);
        }
    }
}
