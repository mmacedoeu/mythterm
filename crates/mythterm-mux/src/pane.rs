//! Pane trait and types.


/// Unique identifier for a pane.
pub type PaneId = usize;

/// Cache policy for renderable content.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CachePolicy {
    /// Cache is valid.
    Cache,
    /// Cache is not valid, re-render.
    NoCache,
}

/// Trait for a terminal pane.
///
/// A pane wraps a PTY and provides the terminal emulator interface.
pub trait Pane: Send + Sync + std::fmt::Debug {
    /// Returns the unique pane ID.
    fn pane_id(&self) -> PaneId;

    /// Returns the pane title (usually the process name or command).
    fn get_title(&self) -> String;

    /// Returns the current working directory of the pane's process.
    fn get_current_working_dir(&self) -> Option<url::Url>;

    /// Returns true if the pane's process has exited.
    fn is_dead(&self) -> bool;

    /// Returns the exit status of the pane's process, if it has exited.
    fn exit_status(&self) -> Option<std::process::ExitStatus>;

    /// Write input data to the pane's PTY.
    fn write_to_pty(&self, data: Vec<u8>);

    /// Resize the pane's PTY.
    fn resize(&self, size: portable_pty::PtySize) -> anyhow::Result<()>;

    /// Returns the pane's terminal size.
    fn get_size(&self) -> portable_pty::PtySize;

    /// Get the visible lines from the terminal as strings.
    fn get_visible_lines(&self) -> Vec<String> {
        Vec::new()
    }

    /// Get the cursor position (col, row).
    fn get_cursor_position(&self) -> (usize, usize) {
        (0, 0)
    }

    /// Get visible lines with color information.
    /// Returns a Vec of (text, Vec<(fg_r, fg_g, fg_b, bg_r, bg_g, bg_b)>) per line.
    /// Default implementation returns empty (no colors).
    fn get_colored_lines(&self) -> Vec<(String, Vec<([u8; 3], [u8; 3])>)> {
        Vec::new()
    }
}
