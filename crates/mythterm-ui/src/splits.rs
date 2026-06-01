//! Split pane layout.
//!
//! Manages the layout of multiple terminal panes in a split configuration.

use egui::Rect;

/// Direction of a split.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SplitDirection {
    Horizontal,
    Vertical,
}

/// A node in the split layout tree.
#[derive(Debug)]
pub enum SplitNode {
    /// A leaf node containing a pane.
    Leaf {
        /// The pane index.
        pane_id: usize,
    },
    /// An internal node with two children.
    Split {
        /// Split direction.
        direction: SplitDirection,
        /// Split ratio (0.0 to 1.0).
        ratio: f32,
        /// Left/top child.
        first: Box<SplitNode>,
        /// Right/bottom child.
        second: Box<SplitNode>,
    },
}

/// Split pane layout manager.
pub struct SplitLayout {
    /// The root of the layout tree.
    root: SplitNode,
    /// Currently focused pane.
    focused_pane: usize,
}

impl SplitLayout {
    /// Create a new split layout with a single pane.
    pub fn new(pane_id: usize) -> Self {
        Self {
            root: SplitNode::Leaf { pane_id },
            focused_pane: pane_id,
        }
    }

    /// Split a pane in the given direction.
    pub fn split(&mut self, pane_id: usize, direction: SplitDirection, new_pane_id: usize) {
        self.root = self.split_node(&self.root, pane_id, direction, new_pane_id);
    }

    fn split_node(
        &self,
        node: &SplitNode,
        target_pane: usize,
        direction: SplitDirection,
        new_pane: usize,
    ) -> SplitNode {
        match node {
            SplitNode::Leaf { pane_id } => {
                if *pane_id == target_pane {
                    SplitNode::Split {
                        direction,
                        ratio: 0.5,
                        first: Box::new(SplitNode::Leaf { pane_id: *pane_id }),
                        second: Box::new(SplitNode::Leaf { pane_id: new_pane }),
                    }
                } else {
                    SplitNode::Leaf { pane_id: *pane_id }
                }
            }
            SplitNode::Split {
                direction: dir,
                ratio,
                first,
                second,
            } => SplitNode::Split {
                direction: *dir,
                ratio: *ratio,
                first: Box::new(self.split_node(first, target_pane, direction, new_pane)),
                second: Box::new(self.split_node(second, target_pane, direction, new_pane)),
            },
        }
    }

    /// Get the focused pane.
    pub fn focused_pane(&self) -> usize {
        self.focused_pane
    }

    /// Set the focused pane.
    pub fn set_focused_pane(&mut self, pane_id: usize) {
        self.focused_pane = pane_id;
    }

    /// Calculate the rect for each pane in the layout.
    pub fn layout(&self, rect: Rect) -> Vec<(usize, Rect)> {
        let mut result = Vec::new();
        self.layout_node(&self.root, rect, &mut result);
        result
    }

    fn layout_node(&self, node: &SplitNode, rect: Rect, result: &mut Vec<(usize, Rect)>) {
        match node {
            SplitNode::Leaf { pane_id } => {
                result.push((*pane_id, rect));
            }
            SplitNode::Split {
                direction,
                ratio,
                first,
                second,
            } => {
                match direction {
                    SplitDirection::Horizontal => {
                        let split_x = rect.min.x + rect.width() * ratio;
                        let first_rect = Rect::from_min_max(
                            rect.min,
                            egui::pos2(split_x, rect.max.y),
                        );
                        let second_rect = Rect::from_min_max(
                            egui::pos2(split_x, rect.min.y),
                            rect.max,
                        );
                        self.layout_node(first, first_rect, result);
                        self.layout_node(second, second_rect, result);
                    }
                    SplitDirection::Vertical => {
                        let split_y = rect.min.y + rect.height() * ratio;
                        let first_rect = Rect::from_min_max(
                            rect.min,
                            egui::pos2(rect.max.x, split_y),
                        );
                        let second_rect = Rect::from_min_max(
                            egui::pos2(rect.min.x, split_y),
                            rect.max,
                        );
                        self.layout_node(first, first_rect, result);
                        self.layout_node(second, second_rect, result);
                    }
                }
            }
        }
    }
}
