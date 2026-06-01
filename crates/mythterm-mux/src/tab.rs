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
    /// Index of the zoomed pane, if any.
    zoomed_pane: Option<usize>,
}

impl Tab {
    pub fn new(tab_id: TabId, pane: Arc<dyn Pane>) -> Self {
        Self {
            tab_id,
            panes: vec![pane],
            active_pane: 0,
            zoomed_pane: None,
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

    /// Check if a pane is zoomed (temporarily fullscreen).
    pub fn is_zoomed(&self) -> bool {
        self.zoomed_pane.is_some()
    }

    /// Get the zoomed pane index, if any.
    pub fn zoomed_pane(&self) -> Option<usize> {
        self.zoomed_pane
    }

    /// Toggle zoom on the active pane.
    ///
    /// If the active pane is zoomed, unzoom it.
    /// If no pane is zoomed, zoom the active pane.
    pub fn toggle_zoom(&mut self) {
        if self.zoomed_pane == Some(self.active_pane) {
            self.zoomed_pane = None;
        } else {
            self.zoomed_pane = Some(self.active_pane);
        }
    }

    /// Set the active pane by index.
    pub fn set_active_pane(&mut self, index: usize) {
        if index < self.panes.len() {
            self.active_pane = index;
        }
    }

    /// Add a pane to the tab.
    pub fn add_pane(&mut self, pane: Arc<dyn Pane>) {
        self.panes.push(pane);
        self.active_pane = self.panes.len() - 1;
    }

    /// Remove a pane by index. Returns the removed pane.
    pub fn remove_pane(&mut self, index: usize) -> Option<Arc<dyn Pane>> {
        if index < self.panes.len() {
            let pane = self.panes.remove(index);
            // Adjust active pane index
            if self.active_pane >= self.panes.len() && self.active_pane > 0 {
                self.active_pane -= 1;
            }
            // Clear zoom if the zoomed pane was removed
            if self.zoomed_pane == Some(index) {
                self.zoomed_pane = None;
            } else if let Some(z) = self.zoomed_pane {
                if z > index {
                    self.zoomed_pane = Some(z - 1);
                }
            }
            Some(pane)
        } else {
            None
        }
    }
}
