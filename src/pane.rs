//! Pane management for split terminal views.
//!
//! Implements a tree-based layout system similar to tmux, where panes can be
//! split horizontally or vertically, and each pane contains its own terminal.

// Some methods are kept for future use / API completeness
#![allow(dead_code)]

use std::os::unix::io::RawFd;

use crate::pty::{Pty, PtySize};
use crate::terminal::Terminal;

/// Unique identifier for a pane.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct PaneId(pub u32);

/// Direction for splitting a pane.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SplitDirection {
    /// Split horizontally (new pane below).
    Horizontal,
    /// Split vertically (new pane to the right).
    Vertical,
}

/// Direction for navigating between panes.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Direction {
    Up,
    Down,
    Left,
    Right,
}

/// Rectangle representing a pane's position and size.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Rect {
    pub x: u16,
    pub y: u16,
    pub width: u16,
    pub height: u16,
}

impl Rect {
    pub fn new(x: u16, y: u16, width: u16, height: u16) -> Self {
        Self {
            x,
            y,
            width,
            height,
        }
    }

    /// Split this rect horizontally, returning (top, bottom).
    pub fn split_horizontal(&self, ratio: f32) -> (Rect, Rect) {
        let top_height = ((self.height as f32) * ratio) as u16;
        let bottom_height = self.height.saturating_sub(top_height).saturating_sub(1); // -1 for border

        let top = Rect::new(self.x, self.y, self.width, top_height);
        let bottom = Rect::new(self.x, self.y + top_height + 1, self.width, bottom_height);

        (top, bottom)
    }

    /// Split this rect vertically, returning (left, right).
    pub fn split_vertical(&self, ratio: f32) -> (Rect, Rect) {
        let left_width = ((self.width as f32) * ratio) as u16;
        let right_width = self.width.saturating_sub(left_width).saturating_sub(1); // -1 for border

        let left = Rect::new(self.x, self.y, left_width, self.height);
        let right = Rect::new(self.x + left_width + 1, self.y, right_width, self.height);

        (left, right)
    }

    /// Check if a point is inside this rect.
    pub fn contains(&self, x: u16, y: u16) -> bool {
        x >= self.x && x < self.x + self.width && y >= self.y && y < self.y + self.height
    }
}

/// A single pane containing a terminal and PTY.
pub struct Pane {
    /// Unique identifier.
    pub id: PaneId,
    /// The terminal state.
    pub terminal: Terminal,
    /// The PTY connection.
    pub pty: Pty,
    /// VTE parser for this pane.
    pub parser: vte::Parser,
    /// Current position and size.
    pub rect: Rect,
    /// Whether this pane is focused.
    pub focused: bool,
    /// Last known mouse mode (for detecting changes).
    pub last_mouse_mode: u16,
}

impl Pane {
    /// Create a new pane with the given shell.
    pub fn new(id: PaneId, rect: Rect, shell: &str) -> anyhow::Result<Self> {
        log::info!(
            "Creating pane {:?} at ({}, {}) size {}x{}",
            id,
            rect.x,
            rect.y,
            rect.width,
            rect.height
        );

        let pty_size = PtySize::new(rect.height, rect.width);
        let pty = Pty::spawn(pty_size, shell)?;
        let terminal = Terminal::new(rect.height as usize, rect.width as usize);
        let parser = vte::Parser::new();

        Ok(Self {
            id,
            terminal,
            pty,
            parser,
            rect,
            focused: false,
            last_mouse_mode: 0,
        })
    }

    /// Resize the pane to a new rect.
    pub fn resize(&mut self, rect: Rect) -> anyhow::Result<()> {
        self.rect = rect;
        self.pty.resize(PtySize::new(rect.height, rect.width))?;
        self.terminal
            .resize(rect.height as usize, rect.width as usize);
        Ok(())
    }

    /// Check if the PTY is still alive.
    pub fn is_alive(&self) -> bool {
        self.pty.is_alive()
    }

    /// Get the raw file descriptor for polling.
    pub fn fd(&self) -> RawFd {
        self.pty.as_raw_fd()
    }
}

/// Layout node in the pane tree.
pub enum LayoutNode {
    /// A leaf node containing a single pane.
    Pane(PaneId),
    /// A split node containing two children.
    Split {
        direction: SplitDirection,
        ratio: f32,
        first: Box<LayoutNode>,
        second: Box<LayoutNode>,
    },
}

impl LayoutNode {
    /// Calculate rectangles for all panes in this layout.
    pub fn calculate_rects(&self, rect: Rect, rects: &mut Vec<(PaneId, Rect)>) {
        match self {
            LayoutNode::Pane(id) => {
                rects.push((*id, rect));
            }
            LayoutNode::Split {
                direction,
                ratio,
                first,
                second,
            } => {
                let (first_rect, second_rect) = match direction {
                    SplitDirection::Horizontal => rect.split_horizontal(*ratio),
                    SplitDirection::Vertical => rect.split_vertical(*ratio),
                };
                first.calculate_rects(first_rect, rects);
                second.calculate_rects(second_rect, rects);
            }
        }
    }

    /// Find and replace a pane with a split.
    pub fn split_pane(
        &mut self,
        target: PaneId,
        new_pane: PaneId,
        direction: SplitDirection,
    ) -> bool {
        match self {
            LayoutNode::Pane(id) if *id == target => {
                let old_node = Box::new(LayoutNode::Pane(target));
                let new_node = Box::new(LayoutNode::Pane(new_pane));
                *self = LayoutNode::Split {
                    direction,
                    ratio: 0.5,
                    first: old_node,
                    second: new_node,
                };
                true
            }
            LayoutNode::Pane(_) => false,
            LayoutNode::Split { first, second, .. } => {
                first.split_pane(target, new_pane, direction)
                    || second.split_pane(target, new_pane, direction)
            }
        }
    }

    /// Remove a pane from the layout, returning the sibling if found.
    pub fn remove_pane(&mut self, target: PaneId) -> Option<Box<LayoutNode>> {
        match self {
            LayoutNode::Pane(_) => None,
            LayoutNode::Split { first, second, .. } => {
                // Check if first child is the target
                if let LayoutNode::Pane(id) = first.as_ref() {
                    if *id == target {
                        return Some(second.clone());
                    }
                }
                // Check if second child is the target
                if let LayoutNode::Pane(id) = second.as_ref() {
                    if *id == target {
                        return Some(first.clone());
                    }
                }
                // Recurse into children
                if let Some(replacement) = first.remove_pane(target) {
                    *first = replacement;
                    return None;
                }
                if let Some(replacement) = second.remove_pane(target) {
                    *second = replacement;
                    return None;
                }
                None
            }
        }
    }

    /// Get all pane IDs in this layout.
    pub fn pane_ids(&self) -> Vec<PaneId> {
        let mut ids = Vec::new();
        self.collect_pane_ids(&mut ids);
        ids
    }

    fn collect_pane_ids(&self, ids: &mut Vec<PaneId>) {
        match self {
            LayoutNode::Pane(id) => ids.push(*id),
            LayoutNode::Split { first, second, .. } => {
                first.collect_pane_ids(ids);
                second.collect_pane_ids(ids);
            }
        }
    }
}

impl Clone for LayoutNode {
    fn clone(&self) -> Self {
        match self {
            LayoutNode::Pane(id) => LayoutNode::Pane(*id),
            LayoutNode::Split {
                direction,
                ratio,
                first,
                second,
            } => LayoutNode::Split {
                direction: *direction,
                ratio: *ratio,
                first: first.clone(),
                second: second.clone(),
            },
        }
    }
}

/// Manages all panes and their layout.
pub struct PaneManager {
    /// All panes indexed by ID.
    panes: std::collections::HashMap<PaneId, Pane>,
    /// The layout tree.
    layout: LayoutNode,
    /// Currently focused pane.
    focused: PaneId,
    /// Focus history stack (most recent at end, excludes current focused pane).
    focus_history: Vec<PaneId>,
    /// Next pane ID to assign.
    next_id: u32,
    /// Total screen size.
    screen_rect: Rect,
    /// Shell to use for new panes.
    shell: String,
}

impl PaneManager {
    /// Create a new pane manager with a single pane (uses pane ID 0).
    #[allow(dead_code)]
    pub fn new(width: u16, height: u16, shell: &str) -> anyhow::Result<Self> {
        Self::new_with_pane_id(width, height, shell, 0)
    }

    /// Create a new pane manager with a single pane using the specified pane ID.
    pub fn new_with_pane_id(
        width: u16,
        height: u16,
        shell: &str,
        pane_id: u32,
    ) -> anyhow::Result<Self> {
        let screen_rect = Rect::new(0, 0, width, height);
        let id = PaneId(pane_id);
        let pane = Pane::new(id, screen_rect, shell)?;

        let mut panes = std::collections::HashMap::new();
        panes.insert(id, pane);

        Ok(Self {
            panes,
            layout: LayoutNode::Pane(id),
            focused: id,
            focus_history: Vec::new(),
            next_id: pane_id + 1,
            screen_rect,
            shell: shell.to_string(),
        })
    }

    /// Get the focused pane.
    pub fn focused_pane(&self) -> Option<&Pane> {
        self.panes.get(&self.focused)
    }

    /// Get the focused pane mutably.
    pub fn focused_pane_mut(&mut self) -> Option<&mut Pane> {
        self.panes.get_mut(&self.focused)
    }

    /// Get a pane by ID.
    pub fn get_pane(&self, id: PaneId) -> Option<&Pane> {
        self.panes.get(&id)
    }

    /// Get a pane mutably by ID.
    pub fn get_pane_mut(&mut self, id: PaneId) -> Option<&mut Pane> {
        self.panes.get_mut(&id)
    }

    /// Get all panes.
    pub fn panes(&self) -> impl Iterator<Item = &Pane> {
        self.panes.values()
    }

    /// Check if a pane with the given ID exists.
    pub fn has_pane(&self, id: PaneId) -> bool {
        self.panes.contains_key(&id)
    }

    /// Get all panes mutably.
    pub fn panes_mut(&mut self) -> impl Iterator<Item = &mut Pane> {
        self.panes.values_mut()
    }

    /// Get all panes as a vector.
    pub fn all_panes(&self) -> Vec<&Pane> {
        self.panes.values().collect()
    }

    /// Find a pane by ID mutably.
    pub fn find_pane_mut(&mut self, pane_id: PaneId) -> Option<&mut Pane> {
        self.panes.get_mut(&pane_id)
    }

    /// Get the focused pane ID.
    pub fn focused_id(&self) -> PaneId {
        self.focused
    }

    /// Split the focused pane using an internally generated ID.
    #[allow(dead_code)]
    pub fn split(&mut self, direction: SplitDirection) -> anyhow::Result<PaneId> {
        let new_id = self.next_id;
        self.next_id += 1;
        self.split_with_id(direction, new_id)
    }

    /// Split the focused pane using the specified pane ID.
    pub fn split_with_id(
        &mut self,
        direction: SplitDirection,
        pane_id: u32,
    ) -> anyhow::Result<PaneId> {
        let new_id = PaneId(pane_id);

        // Update layout
        self.layout.split_pane(self.focused, new_id, direction);

        // Recalculate all rects
        let mut rects = Vec::new();
        self.layout.calculate_rects(self.screen_rect, &mut rects);

        // Create the new pane with its calculated rect
        let new_rect = rects
            .iter()
            .find(|(id, _)| *id == new_id)
            .map(|(_, r)| *r)
            .unwrap_or(self.screen_rect);

        let new_pane = Pane::new(new_id, new_rect, &self.shell)?;
        self.panes.insert(new_id, new_pane);

        // Resize existing panes
        for (id, rect) in rects {
            if let Some(pane) = self.panes.get_mut(&id) {
                if pane.rect != rect {
                    pane.resize(rect)?;
                }
            }
        }

        // Focus the new pane
        self.focus(new_id);

        Ok(new_id)
    }

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

    /// Resize the entire screen.
    pub fn resize_screen(&mut self, width: u16, height: u16) -> anyhow::Result<()> {
        self.screen_rect = Rect::new(0, 0, width, height);

        // Recalculate all rects
        let mut rects = Vec::new();
        self.layout.calculate_rects(self.screen_rect, &mut rects);

        // Resize all panes
        for (id, rect) in rects {
            if let Some(pane) = self.panes.get_mut(&id) {
                pane.resize(rect)?;
            }
        }

        Ok(())
    }

    /// Find the pane at a given screen position.
    pub fn pane_at(&self, x: u16, y: u16) -> Option<PaneId> {
        for (id, pane) in &self.panes {
            if pane.rect.contains(x, y) {
                return Some(*id);
            }
        }
        None
    }

    /// Check if any pane has died.
    pub fn check_dead_panes(&mut self) -> Vec<PaneId> {
        let dead: Vec<PaneId> = self
            .panes
            .iter()
            .filter(|(_, pane)| !pane.is_alive())
            .map(|(id, _)| *id)
            .collect();
        dead
    }

    /// Get pane count.
    pub fn pane_count(&self) -> usize {
        self.panes.len()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_rect_split_horizontal() {
        let rect = Rect::new(0, 0, 80, 24);
        let (top, bottom) = rect.split_horizontal(0.5);

        assert_eq!(top.y, 0);
        assert_eq!(top.height, 12);
        assert_eq!(bottom.y, 13); // 12 + 1 for border
    }

    #[test]
    fn test_rect_split_vertical() {
        let rect = Rect::new(0, 0, 80, 24);
        let (left, right) = rect.split_vertical(0.5);

        assert_eq!(left.x, 0);
        assert_eq!(left.width, 40);
        assert_eq!(right.x, 41); // 40 + 1 for border
    }

    #[test]
    fn test_rect_contains() {
        let rect = Rect::new(10, 10, 20, 10);

        assert!(rect.contains(10, 10));
        assert!(rect.contains(29, 19));
        assert!(!rect.contains(9, 10));
        assert!(!rect.contains(30, 10));
    }
}
