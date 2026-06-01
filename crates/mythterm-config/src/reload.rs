//! Live config reload using notify file watcher.
//!
//! Watches the config file for changes and reloads settings automatically.

use crate::Settings;
use anyhow::Result;
use notify::{Event, RecursiveMode, Watcher};
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

/// Callback for config changes.
pub type ConfigCallback = Arc<dyn Fn(Settings) + Send + Sync>;

/// Config file watcher with live reload.
pub struct ConfigWatcher {
    /// The file watcher.
    _watcher: notify::RecommendedWatcher,
}

impl ConfigWatcher {
    /// Create a new config watcher.
    ///
    /// Watches the config file for changes and calls the callback
    /// with the new settings when changes are detected.
    pub fn new(
        config_path: PathBuf,
        callback: ConfigCallback,
    ) -> Result<Self> {
        let (tx, rx) = std::sync::mpsc::channel();

        let mut watcher = notify::recommended_watcher(move |res: Result<Event, notify::Error>| {
            if let Ok(event) = res {
                if event.kind.is_modify() {
                    let _ = tx.send(());
                }
            }
        })?;

        watcher.watch(&config_path, RecursiveMode::NonRecursive)?;

        // Spawn a thread to handle reload events
        let path = config_path.clone();
        std::thread::spawn(move || {
            let mut last_reload = std::time::Instant::now();

            loop {
                if rx.recv().is_ok() {
                    // Debounce: wait a bit after receiving event
                    std::thread::sleep(Duration::from_millis(100));

                    // Avoid reloading too frequently
                    if last_reload.elapsed() < Duration::from_secs(1) {
                        continue;
                    }

                    match crate::loader::load_settings() {
                        Ok(settings) => {
                            log::info!("Config reloaded from {:?}", path);
                            callback(settings);
                            last_reload = std::time::Instant::now();
                        }
                        Err(e) => {
                            log::error!("Failed to reload config: {}", e);
                        }
                    }
                } else {
                    break;
                }
            }
        });

        Ok(Self { _watcher: watcher })
    }
}

impl std::fmt::Debug for ConfigWatcher {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ConfigWatcher").finish()
    }
}
