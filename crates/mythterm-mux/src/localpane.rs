//! LocalPane: PTY-backed terminal pane.

use crate::pane::{Pane, PaneId};

/// A pane backed by a local PTY.
#[derive(Debug)]
pub struct LocalPane {
    pane_id: PaneId,
    // TODO: Add PTY handle, terminal state, etc.
}

impl LocalPane {
    pub fn new(pane_id: PaneId) -> Self {
        Self { pane_id }
    }
}

impl Pane for LocalPane {
    fn pane_id(&self) -> PaneId {
        self.pane_id
    }

    fn get_title(&self) -> String {
        "local".to_string()
    }

    fn get_current_working_dir(&self) -> Option<url::Url> {
        None
    }

    fn is_dead(&self) -> bool {
        false
    }

    fn exit_status(&self) -> Option<std::process::ExitStatus> {
        None
    }

    fn write_to_pty(&self, _data: Vec<u8>) {
        // TODO: Write to PTY
    }

    fn resize(&self, _size: portable_pty::PtySize) -> anyhow::Result<()> {
        // TODO: Resize PTY
        Ok(())
    }

    fn get_size(&self) -> portable_pty::PtySize {
        portable_pty::PtySize {
            rows: 24,
            cols: 80,
            pixel_width: 0,
            pixel_height: 0,
        }
    }
}
