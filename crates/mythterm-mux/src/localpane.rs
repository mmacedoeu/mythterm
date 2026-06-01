//! LocalPane: PTY-backed terminal pane.
//!
//! Manages a PTY device, child process, and terminal emulator instance.
//! A background thread reads PTY output and feeds it to the terminal.
//! Input is sent to the PTY via a channel.

use crate::pane::{Pane, PaneId};
use anyhow::{Context, Result};
use parking_lot::Mutex;
use portable_pty::{ChildKiller, CommandBuilder, ExitStatus, MasterPty, PtySize};
use std::io::Write;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::thread::JoinHandle;
use url::Url;
use wezterm_term::{Terminal, TerminalConfiguration, TerminalSize};
use std::sync::Mutex as StdMutex;

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

/// Writer that sends bytes through a channel to the PTY writer thread.
struct ChannelWriter {
    sender: flume::Sender<Vec<u8>>,
}

impl Write for ChannelWriter {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        self.sender
            .send(buf.to_vec())
            .map_err(|e| std::io::Error::new(std::io::ErrorKind::BrokenPipe, e))?;
        Ok(buf.len())
    }

    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

/// A pane backed by a local PTY.
///
/// Manages a PTY device, child process, and terminal emulator instance.
/// The terminal emulator processes bytes from the PTY and maintains
/// the cell grid state.
pub struct LocalPane {
    pane_id: PaneId,
    terminal: Arc<Mutex<Terminal>>,
    process: Arc<Mutex<ProcessState>>,
    /// Channel to send input to the PTY.
    input_sender: flume::Sender<Vec<u8>>,
    /// Flag to signal threads to stop.
    running: Arc<AtomicBool>,
    /// Reader thread handle.
    reader_handle: Option<JoinHandle<()>>,
    /// Writer thread handle.
    writer_handle: Option<JoinHandle<()>>,
    /// Resize sender - sends new size to the resize handler thread.
    resize_sender: flume::Sender<PtySize>,
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
    /// starts background threads to read PTY output and write PTY input.
    pub fn new(
        pane_id: PaneId,
        _domain_id: usize,
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

        let mut master = pair.master;

        // Create channel for PTY input
        let (input_sender, input_receiver) = flume::unbounded::<Vec<u8>>();

        // Take the writer from the master PTY
        let mut pty_writer: Box<dyn Write + Send> = master
            .take_writer()
            .context("Failed to take PTY writer")?;

        // Wrap master in Arc<StdMutex> for sharing between threads
        let master = Arc::new(StdMutex::new(master));

        // Create the terminal emulator with a ChannelWriter
        let terminal_size = TerminalSize {
            rows: size.rows as usize,
            cols: size.cols as usize,
            pixel_width: size.pixel_width as usize,
            pixel_height: size.pixel_height as usize,
            dpi: 96,
        };

        let channel_writer = ChannelWriter {
            sender: input_sender.clone(),
        };
        let terminal = Arc::new(Mutex::new(Terminal::new(
            terminal_size,
            config,
            "mythterm",
            env!("CARGO_PKG_VERSION"),
            Box::new(channel_writer),
        )));

        let process = Arc::new(Mutex::new(ProcessState::Running { pid, signaller }));
        let running = Arc::new(AtomicBool::new(true));

        // Spawn writer thread: reads from channel, writes to PTY
        let writer_running = running.clone();
        let writer_handle = std::thread::Builder::new()
            .name(format!("pty-writer-{}", pane_id))
            .spawn(move || {
                while writer_running.load(Ordering::Relaxed) {
                    match input_receiver.recv_timeout(std::time::Duration::from_millis(100)) {
                        Ok(data) => {
                            if let Err(e) = pty_writer.write_all(&data) {
                                log::error!("PTY writer {}: write error: {}", pane_id, e);
                                break;
                            }
                        }
                        Err(flume::RecvTimeoutError::Timeout) => continue,
                        Err(flume::RecvTimeoutError::Disconnected) => break,
                    }
                }
                log::debug!("PTY writer {} thread exiting", pane_id);
            })
            .context("Failed to spawn PTY writer thread")?;

        // Spawn reader thread: reads from PTY, feeds to terminal
        let reader_terminal = terminal.clone();
        let reader_process = process.clone();
        let reader_running = running.clone();
        let reader_master = master.clone();
        let reader_handle = std::thread::Builder::new()
            .name(format!("pty-reader-{}", pane_id))
            .spawn(move || {
                let mut reader = {
                    let master = reader_master.lock().unwrap();
                    match master.try_clone_reader() {
                        Ok(r) => r,
                        Err(e) => {
                            log::error!("PTY reader {}: failed to clone reader: {}", pane_id, e);
                            return;
                        }
                    }
                };

                let mut buf = [0u8; 8192];
                loop {
                    if !reader_running.load(Ordering::Relaxed) {
                        break;
                    }

                    match std::io::Read::read(&mut reader, &mut buf) {
                        Ok(0) => {
                            log::debug!("PTY reader {}: EOF", pane_id);
                            break;
                        }
                        Ok(n) => {
                            eprintln!("[PTY-READ] pane {}: {} bytes", pane_id, n);
                            reader_terminal.lock().advance_bytes(&buf[..n]);
                        }
                        Err(e) => {
                            if e.kind() == std::io::ErrorKind::WouldBlock
                                || e.kind() == std::io::ErrorKind::Interrupted
                            {
                                continue;
                            }
                            log::error!("PTY reader {}: read error: {}", pane_id, e);
                            break;
                        }
                    }
                }

                *reader_process.lock() = ProcessState::Dead(None);
                log::debug!("PTY reader {} thread exiting", pane_id);
            })
            .context("Failed to spawn PTY reader thread")?;

        // Create resize channel
        let (resize_sender, resize_receiver) = flume::unbounded::<PtySize>();

        // Spawn resize handler thread that owns the master PTY
        let resize_terminal = terminal.clone();
        let resize_running = running.clone();
        let resize_master = master.clone();
        std::thread::Builder::new()
            .name(format!("pty-resize-{}", pane_id))
            .spawn(move || {
                while resize_running.load(Ordering::Relaxed) {
                    match resize_receiver.recv_timeout(std::time::Duration::from_millis(100)) {
                        Ok(new_size) => {
                            {
                                let master = resize_master.lock().unwrap();
                                if let Err(e) = master.resize(new_size) {
                                    log::error!("PTY resize {}: error: {}", pane_id, e);
                                }
                            }
                            let terminal_size = TerminalSize {
                                rows: new_size.rows as usize,
                                cols: new_size.cols as usize,
                                pixel_width: new_size.pixel_width as usize,
                                pixel_height: new_size.pixel_height as usize,
                                dpi: 96,
                            };
                            resize_terminal.lock().resize(terminal_size);
                        }
                        Err(flume::RecvTimeoutError::Timeout) => continue,
                        Err(flume::RecvTimeoutError::Disconnected) => break,
                    }
                }
                log::debug!("PTY resize {} thread exiting", pane_id);
            })
            .context("Failed to spawn PTY resize thread")?;

        Ok(Self {
            pane_id,
            terminal,
            process,
            input_sender,
            running,
            reader_handle: Some(reader_handle),
            writer_handle: Some(writer_handle),
            resize_sender,
        })
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
        self.terminal.lock().get_current_dir().cloned()
    }

    fn is_dead(&self) -> bool {
        self.is_dead()
    }

    fn exit_status(&self) -> Option<std::process::ExitStatus> {
        match &*self.process.lock() {
            ProcessState::Dead(Some(status)) => {
                let code = status.exit_code();
                log::debug!("Process exited with code: {}", code);
                None // std::process::ExitStatus doesn't have a public constructor
            }
            _ => None,
        }
    }

    fn write_to_pty(&self, data: Vec<u8>) {
        if let Err(e) = self.input_sender.send(data) {
            log::error!("Failed to send input to PTY pane {}: {}", self.pane_id, e);
        }
    }

    fn resize(&self, size: PtySize) -> Result<()> {
        // Send resize to the resize handler thread
        if let Err(e) = self.resize_sender.send(size) {
            log::error!("Failed to send resize for pane {}: {}", self.pane_id, e);
            return Err(anyhow::anyhow!("Resize channel closed"));
        }
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

    fn get_visible_lines(&self) -> Vec<String> {
        let term = self.terminal.lock();
        let screen = term.screen();
        let scrollback = screen.scrollback_rows();
        let visible = screen.physical_rows;
        let total_lines = scrollback + visible;

        eprintln!("[LINES] scrollback={}, visible={}, total={}", scrollback, visible, total_lines);

        if total_lines == 0 {
            return Vec::new();
        }

        // Get the visible lines (last `visible` rows)
        let start = if total_lines >= visible { total_lines - visible } else { 0 };
        let lines = screen.lines_in_phys_range(start..total_lines);

        let result: Vec<String> = lines.iter().enumerate().map(|(i, line)| {
            let s = line.as_str().into_owned();
            if !s.trim().is_empty() {
                eprintln!("[LINES] row {}: {:?}", start + i, s);
            }
            s
        }).collect();

        eprintln!("[LINES] returning {} lines", result.len());
        result
    }

    fn get_cursor_position(&self) -> (usize, usize) {
        let term = self.terminal.lock();
        let cursor = term.cursor_pos();
        (cursor.x, cursor.y.max(0) as usize)
    }
}

impl Drop for LocalPane {
    fn drop(&mut self) {
        // Signal threads to stop
        self.running.store(false, Ordering::Relaxed);

        // Kill the child process
        if let ProcessState::Running { ref mut signaller, .. } = *self.process.lock() {
            let _ = signaller.kill();
        }

        // Wait for threads to finish
        if let Some(handle) = self.reader_handle.take() {
            let _ = handle.join();
        }
        if let Some(handle) = self.writer_handle.take() {
            let _ = handle.join();
        }
    }
}
