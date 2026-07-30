//! The layout tree: how panes are nested and split.

use super::{PaneId, Rect, SplitDirection};

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
