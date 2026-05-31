//! Window: container for tabs.

use crate::tab::TabId;

/// Unique identifier for a window.
pub type WindowId = usize;

/// A window containing one or more tabs.
#[derive(Debug)]
pub struct Window {
    window_id: WindowId,
    /// The tabs in this window.
    tabs: Vec<TabId>,
    /// Index of the active tab.
    active_tab: usize,
}

impl Window {
    pub fn new(window_id: WindowId) -> Self {
        Self {
            window_id,
            tabs: Vec::new(),
            active_tab: 0,
        }
    }

    pub fn window_id(&self) -> WindowId {
        self.window_id
    }

    pub fn tabs(&self) -> &[TabId] {
        &self.tabs
    }

    pub fn active_tab(&self) -> Option<TabId> {
        self.tabs.get(self.active_tab).copied()
    }

    pub fn push_tab(&mut self, tab_id: TabId) {
        self.tabs.push(tab_id);
        self.active_tab = self.tabs.len() - 1;
    }

    pub fn remove_tab(&mut self, tab_id: TabId) {
        if let Some(pos) = self.tabs.iter().position(|&id| id == tab_id) {
            self.tabs.remove(pos);
            if self.active_tab >= self.tabs.len() && self.active_tab > 0 {
                self.active_tab -= 1;
            }
        }
    }
}
