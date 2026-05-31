//! Activity tracking for panes and tabs.

use std::time::Instant;

/// Tracks activity (output, bell) in a pane/tab.
#[derive(Debug, Clone)]
pub struct Activity {
    last_output: Instant,
    has_bell: bool,
}

impl Activity {
    pub fn new() -> Self {
        Self {
            last_output: Instant::now(),
            has_bell: false,
        }
    }

    pub fn mark_output(&mut self) {
        self.last_output = Instant::now();
    }

    pub fn mark_bell(&mut self) {
        self.has_bell = true;
    }

    pub fn clear_bell(&mut self) {
        self.has_bell = false;
    }

    pub fn has_bell(&self) -> bool {
        self.has_bell
    }

    pub fn last_output(&self) -> Instant {
        self.last_output
    }
}

impl Default for Activity {
    fn default() -> Self {
        Self::new()
    }
}
