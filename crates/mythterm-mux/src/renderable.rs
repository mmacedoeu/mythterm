//! Renderable pane interface.
//!
//! Provides the interface for rendering pane content.

use crate::pane::PaneId;

/// Trait for renderable content from a pane.
pub trait Renderable: Send + Sync {
    /// Returns the pane ID.
    fn pane_id(&self) -> PaneId;

    /// Poll for dirty lines that need re-rendering.
    fn get_dirty_lines(&self, lines: &mut Vec<(usize, String)>);

    /// Mark lines as clean after rendering.
    fn clean_dirty_lines(&self, lines: &[usize]);
}
