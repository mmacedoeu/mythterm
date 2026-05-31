//! Tab: container for one or more panes in a layout tree.

use crate::pane::{Pane, PaneId};
use std::sync::Arc;

/// Unique identifier for a tab.
pub type TabId = usize;

/// A split direction for pane splitting.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SplitDirection {
    Horizontal,
    Vertical,
}

/// A request to split a pane.
#[derive(Debug, Clone)]
pub struct SplitRequest {
    pub pane_id: PaneId,
    pub direction: SplitDirection,
}

/// A tab containing one or more panes in a layout tree.
#[derive(Debug)]
pub struct Tab {
    tab_id: TabId,
    /// The panes in this tab.
    panes: Vec<Arc<dyn Pane>>,
    /// Index of the active (focused) pane.
    active_pane: usize,
}

impl Tab {
    pub fn new(tab_id: TabId, pane: Arc<dyn Pane>) -> Self {
        Self {
            tab_id,
            panes: vec![pane],
            active_pane: 0,
        }
    }

    pub fn tab_id(&self) -> TabId {
        self.tab_id
    }

    pub fn get_active_pane(&self) -> Option<Arc<dyn Pane>> {
        self.panes.get(self.active_pane).cloned()
    }

    pub fn panes(&self) -> &[Arc<dyn Pane>] {
        &self.panes
    }

    pub fn pane_count(&self) -> usize {
        self.panes.len()
    }

    pub fn is_zoomed(&self) -> bool {
        false // TODO: implement zoom
    }
}
