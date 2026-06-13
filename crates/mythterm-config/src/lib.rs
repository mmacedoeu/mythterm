//! mythterm-config: Configuration management
//!
//! Provides TOML-based configuration with live reload and implements
//! WezTerm's `TerminalConfiguration` trait for the terminal core.

pub mod loader;
pub mod mux_config;
pub mod reload;
pub mod scheme;
pub mod settings;
pub mod terminal_config;

pub use loader::{config_path, load_settings, save_settings, ensure_config_exists};
pub use mux_config::{ExitBehavior, MuxConfig};
pub use reload::ConfigWatcher;
pub use scheme::{srgb_to_linear_channel, srgb_to_linear_rgb, ColorScheme};
pub use settings::Settings;
pub use terminal_config::MythtermConfig;
