//! mythterm-mux: Session multiplexer forked from WezTerm.
//!
//! This crate provides tab/pane management, PTY handling, and domain
//! abstraction. Forked from WezTerm's `mux` crate with Lua/SSH/tmux
//! dependencies stripped out.

use crate::pane::{Pane, PaneId};
use crate::tab::{Tab, TabId};
use crate::window::{Window, WindowId};
use parking_lot::RwLock;
use std::collections::HashMap;
use std::sync::Arc;

pub mod activity;
pub mod domain;
pub mod localpane;
pub mod pane;
pub mod renderable;
pub mod tab;
pub mod window;

// Re-export config trait for convenience
pub use mythterm_config::mux_config::{ExitBehavior, MuxConfig};

/// Unique identifier for a mux domain.
pub type DomainId = usize;

/// The top-level multiplexer (session manager).
///
/// Owns all windows, tabs, and panes. This is the central coordination
/// point for the terminal multiplexer.
pub struct Mux {
    windows: RwLock<HashMap<WindowId, Arc<Window>>>,
    tabs: RwLock<HashMap<TabId, Arc<Tab>>>,
    panes: RwLock<HashMap<PaneId, Arc<dyn Pane>>>,
    next_window_id: std::sync::atomic::AtomicUsize,
    next_tab_id: std::sync::atomic::AtomicUsize,
    next_pane_id: std::sync::atomic::AtomicUsize,
}

impl std::fmt::Debug for Mux {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Mux").finish()
    }
}

impl Mux {
    pub fn new() -> Self {
        Self {
            windows: RwLock::new(HashMap::new()),
            tabs: RwLock::new(HashMap::new()),
            panes: RwLock::new(HashMap::new()),
            next_window_id: std::sync::atomic::AtomicUsize::new(0),
            next_tab_id: std::sync::atomic::AtomicUsize::new(0),
            next_pane_id: std::sync::atomic::AtomicUsize::new(0),
        }
    }

    pub fn alloc_window_id(&self) -> WindowId {
        self.next_window_id
            .fetch_add(1, std::sync::atomic::Ordering::Relaxed)
    }

    pub fn alloc_tab_id(&self) -> TabId {
        self.next_tab_id
            .fetch_add(1, std::sync::atomic::Ordering::Relaxed)
    }

    pub fn alloc_pane_id(&self) -> PaneId {
        self.next_pane_id
            .fetch_add(1, std::sync::atomic::Ordering::Relaxed)
    }

    pub fn get_window(&self, id: WindowId) -> Option<Arc<Window>> {
        self.windows.read().get(&id).cloned()
    }

    pub fn get_tab(&self, id: TabId) -> Option<Arc<Tab>> {
        self.tabs.read().get(&id).cloned()
    }

    pub fn get_pane(&self, id: PaneId) -> Option<Arc<dyn Pane>> {
        self.panes.read().get(&id).cloned()
    }

    pub fn insert_window(&self, window: Arc<Window>) {
        self.windows.write().insert(window.window_id(), window);
    }

    pub fn insert_tab(&self, tab: Arc<Tab>) {
        self.tabs.write().insert(tab.tab_id(), tab);
    }

    pub fn insert_pane(&self, pane: Arc<dyn Pane>) {
        self.panes.write().insert(pane.pane_id(), pane);
    }

    pub fn remove_window(&self, id: WindowId) -> Option<Arc<Window>> {
        self.windows.write().remove(&id)
    }

    pub fn remove_tab(&self, id: TabId) -> Option<Arc<Tab>> {
        self.tabs.write().remove(&id)
    }

    pub fn remove_pane(&self, id: PaneId) -> Option<Arc<dyn Pane>> {
        self.panes.write().remove(&id)
    }

    pub fn iter_windows(&self) -> Vec<Arc<Window>> {
        self.windows.read().values().cloned().collect()
    }

    pub fn iter_tabs(&self) -> Vec<Arc<Tab>> {
        self.tabs.read().values().cloned().collect()
    }

    pub fn iter_panes(&self) -> Vec<Arc<dyn Pane>> {
        self.panes.read().values().cloned().collect()
    }
}

impl Default for Mux {
    fn default() -> Self {
        Self::new()
    }
}
