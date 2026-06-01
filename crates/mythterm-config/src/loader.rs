//! Configuration file resolution and loading.
//!
//! Handles finding and loading the TOML config file from
//! `~/.config/mythterm/config.toml`.

use crate::Settings;
use anyhow::{Context, Result};
use std::path::PathBuf;

/// Find the config file path.
///
/// Checks in order:
/// 1. `$MYTHTERM_CONFIG` environment variable
/// 2. `~/.config/mythterm/config.toml`
pub fn config_path() -> Result<PathBuf> {
    // Check environment variable first
    if let Ok(path) = std::env::var("MYTHTERM_CONFIG") {
        let path = PathBuf::from(path);
        if path.exists() {
            return Ok(path);
        }
    }

    // Default location
    let home = dirs_next::home_dir()
        .context("Could not determine home directory")?;

    let config_dir = home.join(".config").join("mythterm");
    let config_file = config_dir.join("config.toml");

    Ok(config_file)
}

/// Load settings from the config file.
///
/// Returns default settings if the file doesn't exist.
pub fn load_settings() -> Result<Settings> {
    let path = config_path()?;

    if !path.exists() {
        log::info!("Config file not found at {:?}, using defaults", path);
        return Ok(Settings::default());
    }

    let content = std::fs::read_to_string(&path)
        .with_context(|| format!("Failed to read config file: {:?}", path))?;

    let settings: Settings = toml::from_str(&content)
        .with_context(|| format!("Failed to parse config file: {:?}", path))?;

    log::info!("Loaded config from {:?}", path);
    Ok(settings)
}

/// Save settings to the config file.
///
/// Creates the config directory if it doesn't exist.
pub fn save_settings(settings: &Settings) -> Result<()> {
    let path = config_path()?;

    // Create config directory if needed
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)
            .with_context(|| format!("Failed to create config directory: {:?}", parent))?;
    }

    let content = toml::to_string_pretty(settings)
        .context("Failed to serialize settings")?;

    std::fs::write(&path, content)
        .with_context(|| format!("Failed to write config file: {:?}", path))?;

    log::info!("Saved config to {:?}", path);
    Ok(())
}

/// Ensure the default config file exists.
///
/// Creates a default config file if it doesn't exist.
pub fn ensure_config_exists() -> Result<()> {
    let path = config_path()?;

    if !path.exists() {
        log::info!("Creating default config at {:?}", path);
        save_settings(&Settings::default())?;
    }

    Ok(())
}
