//! Domain abstraction for PTY providers.
//!
//! Defines the `Domain` trait and `LocalDomain` implementation
//! for spawning local PTY sessions.

use crate::localpane::LocalPane;
use crate::pane::{Pane, PaneId};
use anyhow::Result;
use portable_pty::{CommandBuilder, PtySize};
use std::sync::Arc;
use wezterm_term::TerminalConfiguration;
use mythterm_config::MuxConfig;

/// Unique identifier for a domain.
pub type DomainId = usize;

/// State of a domain connection.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DomainState {
    /// Domain is connected and ready.
    Connected,
    /// Domain is disconnected.
    Disconnected,
}

/// Trait for a domain that can spawn PTY sessions.
///
/// A domain represents a source of terminal sessions. The simplest
/// domain is the LocalDomain which spawns processes on the local machine.
pub trait Domain: Send + Sync + std::fmt::Debug {
    /// Returns the domain ID.
    fn domain_id(&self) -> DomainId;

    /// Returns the domain name.
    fn name(&self) -> &str;

    /// Returns the current state of the domain.
    fn state(&self) -> DomainState;

    /// Spawn a new pane in this domain.
    fn spawn(
        &self,
        pane_id: PaneId,
        size: PtySize,
        command: Option<CommandBuilder>,
    ) -> Result<Arc<dyn Pane>>;
}

/// Local domain: spawns processes on the local machine.
///
/// This is the default domain that creates PTY sessions for
/// local shell processes.
#[derive(Debug)]
pub struct LocalDomain {
    domain_id: DomainId,
    config: Arc<dyn TerminalConfiguration>,
    #[allow(dead_code)]
    mux_config: Arc<dyn MuxConfig>,
}

impl LocalDomain {
    /// Create a new local domain.
    pub fn new(
        domain_id: DomainId,
        config: Arc<dyn TerminalConfiguration>,
        mux_config: Arc<dyn MuxConfig>,
    ) -> Self {
        Self {
            domain_id,
            config,
            mux_config,
        }
    }
}

impl Domain for LocalDomain {
    fn domain_id(&self) -> DomainId {
        self.domain_id
    }

    fn name(&self) -> &str {
        "local"
    }

    fn state(&self) -> DomainState {
        DomainState::Connected
    }

    fn spawn(
        &self,
        pane_id: PaneId,
        size: PtySize,
        command: Option<CommandBuilder>,
    ) -> Result<Arc<dyn Pane>> {
        let pane = LocalPane::new(
            pane_id,
            self.domain_id,
            self.config.clone(),
            size,
            command,
        )?;

        Ok(Arc::new(pane))
    }
}
