//! Mux configuration trait.
//!
//! Defines the configuration interface that the mux crate needs,
//! decoupled from WezTerm's Lua-based config system.

use serde::{Deserialize, Serialize};

/// Exit behavior when a pane's process exits.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ExitBehavior {
    /// Close the pane automatically.
    Close,
    /// Keep the pane open and show exit status.
    CloseOnCleanExit,
    /// Never close automatically.
    NeverClose,
}

impl Default for ExitBehavior {
    fn default() -> Self {
        Self::CloseOnCleanExit
    }
}

/// Configuration interface for the multiplexer.
///
/// This trait replaces WezTerm's `config::configuration()` calls in the
/// mux crate. Implementations provide the config values that the mux
/// needs to function.
pub trait MuxConfig: Send + Sync + std::fmt::Debug {
    /// What to do when a pane's process exits.
    fn exit_behavior(&self) -> ExitBehavior;

    /// Default shell command to spawn.
    fn default_shell(&self) -> &str;

    /// Whether to enable the SSH agent.
    fn mux_enable_ssh_agent(&self) -> bool {
        false
    }

    /// Whether to switch to the last active tab when closing a tab.
    fn switch_to_last_active_tab_when_closing(&self) -> bool {
        true
    }

    /// Default working directory for new panes.
    fn default_cwd(&self) -> Option<&std::path::Path> {
        None
    }
}
