//! Implementation of WezTerm's `TerminalConfiguration` trait for mythterm.
//!
//! This bridges our TOML-based config system with the terminal core's
//! configuration interface.

use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;

use arc_swap::ArcSwap;
use wezterm_term::color::ColorPalette;
use wezterm_term::config::{BidiMode, NewlineCanon, TerminalConfiguration};
use wezterm_cell::UnicodeVersion;

use crate::Settings;

/// Mythterm's implementation of `TerminalConfiguration`.
///
/// Wraps our TOML `Settings` and provides the interface that
/// `wezterm-term::Terminal` expects. Supports runtime config changes
/// via `arc-swap` for lock-free reads.
#[derive(Debug)]
pub struct MythtermConfig {
    /// The current settings, swap-able at runtime.
    settings: ArcSwap<Settings>,
    /// Generation counter incremented on each config change.
    generation: AtomicUsize,
}

impl MythtermConfig {
    /// Create a new config with the given settings.
    pub fn new(settings: Settings) -> Self {
        Self {
            settings: ArcSwap::from_pointee(settings),
            generation: AtomicUsize::new(1),
        }
    }

    /// Create a config with default settings.
    pub fn default_config() -> Self {
        Self::new(Settings::default())
    }

    /// Update the configuration at runtime.
    /// Increments the generation counter so the terminal can detect changes.
    pub fn update(&self, new_settings: Settings) {
        self.settings.store(Arc::new(new_settings));
        self.generation.fetch_add(1, Ordering::SeqCst);
    }

    /// Get a snapshot of the current settings.
    pub fn get_settings(&self) -> Arc<Settings> {
        self.settings.load_full()
    }
}

impl Default for MythtermConfig {
    fn default() -> Self {
        Self::default_config()
    }
}

impl TerminalConfiguration for MythtermConfig {
    fn generation(&self) -> usize {
        self.generation.load(Ordering::Relaxed)
    }

    fn scrollback_size(&self) -> usize {
        self.settings.load().scrollback_lines
    }

    fn enable_csi_u_key_encoding(&self) -> bool {
        self.settings.load().enable_csi_u
    }

    fn color_palette(&self) -> ColorPalette {
        let settings = self.settings.load();
        settings.color_scheme.to_color_palette()
    }

    fn canonicalize_pasted_newlines(&self) -> NewlineCanon {
        NewlineCanon::default()
    }

    fn alternate_buffer_wheel_scroll_speed(&self) -> u8 {
        3
    }

    fn enq_answerback(&self) -> String {
        String::new()
    }

    fn enable_kitty_graphics(&self) -> bool {
        self.settings.load().enable_kitty_graphics
    }

    fn enable_kitty_keyboard(&self) -> bool {
        self.settings.load().enable_kitty_keyboard
    }

    fn unicode_version(&self) -> UnicodeVersion {
        UnicodeVersion {
            version: 9,
            ambiguous_are_wide: false,
            cell_widths: None,
        }
    }

    fn normalize_output_to_unicode_nfc(&self) -> bool {
        false
    }

    fn debug_key_events(&self) -> bool {
        self.settings.load().debug_key_events
    }

    fn bidi_mode(&self) -> BidiMode {
        BidiMode {
            enabled: self.settings.load().bidi_enabled,
            hint: wezterm_bidi::ParagraphDirectionHint::LeftToRight,
        }
    }

    fn enable_title_reporting(&self) -> bool {
        false
    }

    fn enable_checksum_rectangular_area(&self) -> bool {
        false
    }

    fn log_unknown_escape_sequences(&self) -> bool {
        self.settings.load().debug_escape_sequences
    }
}
