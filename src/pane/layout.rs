//! The layout tree and the pane manager.

use super::tree::LayoutNode;
use super::{Pane, PaneId, Rect, SplitDirection};

/// Manages all panes and their layout.
pub struct PaneManager {
    /// All panes indexed by ID.
    pub(super) panes: std::collections::HashMap<PaneId, Pane>,
    /// The layout tree.
    pub(super) layout: LayoutNode,
    /// Currently focused pane.
    pub(super) focused: PaneId,
    /// Focus history stack (most recent at end, excludes current focused pane).
    pub(super) focus_history: Vec<PaneId>,
    /// Next pane ID to assign.
    pub(super) next_id: u32,
    /// Total screen size.
    pub(super) screen_rect: Rect,
    /// Shell to use for new panes.
    pub(super) shell: String,
}

impl PaneManager {
    /// Create a new pane manager with a single pane (uses pane ID 0).
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

    /// Get all panes.
    pub fn panes(&self) -> impl Iterator<Item = &Pane> {
        self.panes.values()
    }

    /// Check if a pane with the given ID exists.
    pub fn has_pane(&self, id: PaneId) -> bool {
        self.panes.contains_key(&id)
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
