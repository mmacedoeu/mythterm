//! mythterm-config: Configuration management
//!
//! Provides TOML-based configuration with live reload and implements
//! WezTerm's `TerminalConfiguration` trait for the terminal core.

pub mod mux_config;
pub mod scheme;
pub mod settings;
pub mod terminal_config;

pub use mux_config::{ExitBehavior, MuxConfig};
pub use settings::Settings;
pub use terminal_config::MythtermConfig;
