//! mythterm-ui: egui-based UI layer for mythterm.
//!
//! Provides the UI chrome using egui:
//! - Tab bar
//! - Split pane layout
//! - Terminal widget
//! - Overlay system (search, command palette)
//! - Toast notifications

pub mod overlay;
pub mod splits;
pub mod tabbar;
pub mod terminal_widget;

pub use tabbar::TabBar;
pub use terminal_widget::TerminalWidget;

/// The main application state for the UI.
pub struct AppState {
    /// Currently active tab index.
    pub active_tab: usize,
    /// Tab titles.
    pub tab_titles: Vec<String>,
    /// Whether the command palette is open.
    pub command_palette_open: bool,
    /// Whether the search overlay is open.
    pub search_open: bool,
    /// Search query.
    pub search_query: String,
}

impl Default for AppState {
    fn default() -> Self {
        Self {
            active_tab: 0,
            tab_titles: vec!["Tab 1".to_string()],
            command_palette_open: false,
            search_open: false,
            search_query: String::new(),
        }
    }
}
