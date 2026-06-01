//! Clipboard integration.
//!
//! Bridges WezTerm's clipboard protocol (OSC 52) with the platform
//! clipboard (xclip/wl-copy/pbcopy).

use std::sync::Arc;

/// Platform clipboard operations.
pub trait ClipboardProvider: Send + Sync + std::fmt::Debug {
    /// Get the current clipboard contents.
    fn get(&self) -> Option<String>;

    /// Set the clipboard contents.
    fn set(&self, text: &str);
}

/// Platform clipboard using command-line tools.
#[derive(Debug)]
pub struct PlatformClipboard;

impl PlatformClipboard {
    pub fn new() -> Self {
        Self
    }
}

impl ClipboardProvider for PlatformClipboard {
    fn get(&self) -> Option<String> {
        #[cfg(target_os = "linux")]
        {
            // Try wl-paste first (Wayland), then xclip (X11)
            std::process::Command::new("wl-paste")
                .output()
                .ok()
                .and_then(|o| {
                    if o.status.success() {
                        String::from_utf8(o.stdout).ok()
                    } else {
                        None
                    }
                })
                .or_else(|| {
                    std::process::Command::new("xclip")
                        .args(["-selection", "clipboard", "-o"])
                        .output()
                        .ok()
                        .and_then(|o| {
                            if o.status.success() {
                                String::from_utf8(o.stdout).ok()
                            } else {
                                None
                            }
                        })
                })
        }

        #[cfg(target_os = "macos")]
        {
            std::process::Command::new("pbpaste")
                .output()
                .ok()
                .and_then(|o| String::from_utf8(o.stdout).ok())
        }

        #[cfg(target_os = "windows")]
        {
            // PowerShell clipboard
            std::process::Command::new("powershell")
                .args(["-command", "Get-Clipboard"])
                .output()
                .ok()
                .and_then(|o| String::from_utf8(o.stdout).ok())
        }
    }

    fn set(&self, text: &str) {
        #[cfg(target_os = "linux")]
        {
            // Try wl-copy first (Wayland), then xclip (X11)
            let result = std::process::Command::new("wl-copy")
                .stdin(std::process::Stdio::piped())
                .spawn()
                .and_then(|mut child| {
                    if let Some(stdin) = child.stdin.as_mut() {
                        use std::io::Write;
                        stdin.write_all(text.as_bytes())?;
                    }
                    child.wait()
                });

            if result.is_err() {
                // Fallback to xclip
                let _ = std::process::Command::new("xclip")
                    .args(["-selection", "clipboard"])
                    .stdin(std::process::Stdio::piped())
                    .spawn()
                    .and_then(|mut child| {
                        if let Some(stdin) = child.stdin.as_mut() {
                            use std::io::Write;
                            stdin.write_all(text.as_bytes())?;
                        }
                        child.wait()
                    });
            }
        }

        #[cfg(target_os = "macos")]
        {
            let _ = std::process::Command::new("pbcopy")
                .stdin(std::process::Stdio::piped())
                .spawn()
                .and_then(|mut child| {
                    if let Some(stdin) = child.stdin.as_mut() {
                        use std::io::Write;
                        stdin.write_all(text.as_bytes())?;
                    }
                    child.wait()
                });
        }

        #[cfg(target_os = "windows")]
        {
            let _ = std::process::Command::new("powershell")
                .args(["-command", &format!("Set-Clipboard -Value '{}'", text.replace('\'', "''"))])
                .spawn()
                .and_then(|mut child| child.wait());
        }
    }
}

impl Default for PlatformClipboard {
    fn default() -> Self {
        Self::new()
    }
}

/// Clipboard handler for OSC 52 protocol.
pub struct ClipboardHandler {
    provider: Arc<dyn ClipboardProvider>,
}

impl ClipboardHandler {
    pub fn new(provider: Arc<dyn ClipboardProvider>) -> Self {
        Self { provider }
    }

    /// Handle OSC 52 clipboard request.
    ///
    /// Returns the clipboard contents for queries, or sets the clipboard
    /// for updates.
    pub fn handle_osc52(&self, _selection: &str, data: Option<&str>) -> Option<String> {
        match data {
            Some(text) => {
                // Set clipboard
                if !text.is_empty() {
                    self.provider.set(text);
                    log::debug!("Clipboard set via OSC 52: {} bytes", text.len());
                }
                None
            }
            None => {
                // Query clipboard
                let content = self.provider.get().unwrap_or_default();
                log::debug!("Clipboard queried via OSC 52: {} bytes", content.len());
                Some(content)
            }
        }
    }
}

impl std::fmt::Debug for ClipboardHandler {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ClipboardHandler").finish()
    }
}
