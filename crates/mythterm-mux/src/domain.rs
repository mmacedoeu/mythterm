//! Domain abstraction for PTY providers.

use crate::pane::Pane;
use portable_pty::PtySize;
use std::sync::Arc;

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
#[async_trait::async_trait]
pub trait Domain: Send + Sync + std::fmt::Debug {
    /// Returns the domain ID.
    fn domain_id(&self) -> DomainId;

    /// Returns the domain name.
    fn name(&self) -> &str;

    /// Returns the current state of the domain.
    fn state(&self) -> DomainState;

    /// Spawn a new pane in this domain.
    async fn spawn(
        &self,
        size: PtySize,
        command: Option<portable_pty::CommandBuilder>,
    ) -> anyhow::Result<Arc<dyn Pane>>;
}
