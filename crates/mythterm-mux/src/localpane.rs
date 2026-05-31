//! LocalPane: PTY-backed terminal pane.
//!
//! This is a simplified port of WezTerm's `LocalPane`, providing
//! essential PTY management without tmux, SSH, or process info caching.

use crate::pane::{Pane, PaneId};
use anyhow::{Context, Result};
use parking_lot::Mutex;
use portable_pty::{ChildKiller, CommandBuilder, ExitStatus, MasterPty, PtySize};
use std::io::Write;
use std::sync::Arc;
use url::Url;
use wezterm_term::{Terminal, TerminalConfiguration, TerminalSize};

/// State of the child process.
#[derive(Debug)]
enum ProcessState {
    /// Process is running.
    Running {
        pid: Option<u32>,
        signaller: Box<dyn ChildKiller + Send + Sync>,
    },
    /// Process has exited.
    Dead(Option<ExitStatus>),
}

/// A pane backed by a local PTY.
///
/// Manages a PTY device, child process, and terminal emulator instance.
/// The terminal emulator processes bytes from the PTY and maintains
/// the cell grid state.
pub struct LocalPane {
    pane_id: PaneId,
    terminal: Mutex<Terminal>,
    process: Mutex<ProcessState>,
    pty: Mutex<Box<dyn MasterPty + Send>>,
    domain_id: usize,
}

impl std::fmt::Debug for LocalPane {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("LocalPane")
            .field("pane_id", &self.pane_id)
            .finish()
    }
}

impl LocalPane {
    /// Create a new LocalPane by spawning a command in a PTY.
    ///
    /// This spawns the given command (or default shell) in a new PTY,
    /// starts a background thread to read PTY output and feed it to
    /// the terminal emulator.
    pub fn new(
        pane_id: PaneId,
        domain_id: usize,
        config: Arc<dyn TerminalConfiguration + Send + Sync>,
        size: PtySize,
        command: Option<CommandBuilder>,
    ) -> Result<Self> {
        let pty_system = portable_pty::native_pty_system();
        let pair = pty_system
            .openpty(size)
            .context("Failed to open PTY")?;

        // Default to /bin/bash if no command specified
        let cmd = command.unwrap_or_else(|| CommandBuilder::new("/bin/bash"));

        let child = pair
            .slave
            .spawn_command(cmd)
            .context("Failed to spawn command in PTY")?;

        let pid = child.process_id();
        let signaller: Box<dyn ChildKiller + Send + Sync> = child.clone_killer().into();

        // Take the master PTY and writer
        let master = pair.master;
        let writer: Box<dyn Write + Send> = master
            .take_writer()
            .context("Failed to take PTY writer")?;

        // Create the terminal emulator
        // Terminal::new takes ownership of the writer for PTY input
        let terminal_size = TerminalSize {
            rows: size.rows as usize,
            cols: size.cols as usize,
            pixel_width: size.pixel_width as usize,
            pixel_height: size.pixel_height as usize,
            dpi: 96,
        };
        let terminal = Terminal::new(
            terminal_size,
            config,
            "mythterm",
            env!("CARGO_PKG_VERSION"),
            writer,
        );

        let pane = Self {
            pane_id,
            terminal: Mutex::new(terminal),
            process: Mutex::new(ProcessState::Running { pid, signaller }),
            pty: Mutex::new(master),
            domain_id,
        };

        // Spawn a background thread to read PTY output and feed to terminal
        pane.spawn_reader_thread();

        Ok(pane)
    }

    /// Spawn a background thread that reads from the PTY and feeds
    /// bytes to the terminal emulator.
    fn spawn_reader_thread(&self) {
        // The PTY reader is obtained from the master PTY.
        // We need to take the reader and spawn a thread that:
        // 1. Reads bytes from the PTY
        // 2. Feeds them to the terminal emulator via advance_bytes()
        // 3. Checks if the child process has exited

        // For now, this is a TODO - in production, we'd use Arc<LocalPane>
        // and clone it for the thread. The thread would:
        //   let mut reader = master.try_clone_reader()?;
        //   loop {
        //       let mut buf = [0u8; 8192];
        //       match reader.read(&mut buf) {
        //           Ok(n) => terminal.advance_bytes(&buf[..n]),
        //           Err(_) => break,
        //       }
        //   }
        log::debug!("LocalPane {}: reader thread spawned (TODO: implement)", self.pane_id);
    }

    /// Check if the child process has exited.
    pub fn is_dead(&self) -> bool {
        matches!(*self.process.lock(), ProcessState::Dead(_))
    }

    /// Get the exit status if the process has exited.
    pub fn exit_status(&self) -> Option<ExitStatus> {
        match &*self.process.lock() {
            ProcessState::Dead(status) => status.clone(),
            _ => None,
        }
    }

    /// Get the pane ID.
    pub fn pane_id(&self) -> PaneId {
        self.pane_id
    }

    /// Get a reference to the terminal emulator.
    pub fn terminal(&self) -> &Mutex<Terminal> {
        &self.terminal
    }
}

impl Pane for LocalPane {
    fn pane_id(&self) -> PaneId {
        self.pane_id
    }

    fn get_title(&self) -> String {
        self.terminal.lock().get_title().to_string()
    }

    fn get_current_working_dir(&self) -> Option<Url> {
        // TODO: Get CWD from child process
        None
    }

    fn is_dead(&self) -> bool {
        self.is_dead()
    }

    fn exit_status(&self) -> Option<std::process::ExitStatus> {
        // TODO: Convert portable_pty::ExitStatus to std::process::ExitStatus
        None
    }

    fn write_to_pty(&self, data: Vec<u8>) {
        // The terminal emulator handles writing to the PTY via its writer.
        // We send input through the terminal's advance_bytes method.
        // Actually, for keyboard input, we need to write directly to the PTY.
        // The Terminal::new took ownership of the writer, so we need to use
        // the terminal's send_paste or similar method.
        //
        // For now, we'll note this as a TODO - the proper approach is to
        // use the terminal's input methods.
        log::debug!("LocalPane {}: write_to_pty {} bytes (TODO)", self.pane_id, data.len());
    }

    fn resize(&self, size: PtySize) -> Result<()> {
        let pty = self.pty.lock();
        pty.resize(size).context("Failed to resize PTY")?;

        let terminal_size = TerminalSize {
            rows: size.rows as usize,
            cols: size.cols as usize,
            pixel_width: size.pixel_width as usize,
            pixel_height: size.pixel_height as usize,
            dpi: 96,
        };
        self.terminal.lock().resize(terminal_size);
        Ok(())
    }

    fn get_size(&self) -> PtySize {
        let term = self.terminal.lock();
        let size = term.get_size();
        PtySize {
            rows: size.rows as u16,
            cols: size.cols as u16,
            pixel_width: size.pixel_width as u16,
            pixel_height: size.pixel_height as u16,
        }
    }
}

impl Drop for LocalPane {
    fn drop(&mut self) {
        // Kill the child process when the pane is dropped
        if let ProcessState::Running { ref mut signaller, .. } = *self.process.lock() {
            let _ = signaller.kill();
        }
    }
}
