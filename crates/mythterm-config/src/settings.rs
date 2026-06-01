use serde::{Deserialize, Serialize};

use crate::scheme::ColorScheme;

/// Top-level mythterm configuration, loaded from TOML.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Settings {
    /// Font family name.
    pub font_family: String,
    /// Font size in points.
    pub font_size: f32,
    /// Cursor style: "Block", "Beam", or "Underline".
    pub cursor_style: String,
    /// Whether the cursor blinks.
    pub cursor_blink: bool,
    /// Number of lines in the scrollback buffer.
    pub scrollback_lines: usize,
    /// Color scheme.
    pub color_scheme: ColorScheme,
    /// Enable CSI-u key encoding for unambiguous key reporting.
    pub enable_csi_u: bool,
    /// Enable Kitty graphics protocol.
    pub enable_kitty_graphics: bool,
    /// Enable Kitty keyboard protocol.
    pub enable_kitty_keyboard: bool,
    /// Enable debug logging of key events.
    pub debug_key_events: bool,
    /// Enable debug logging of unknown escape sequences.
    pub debug_escape_sequences: bool,
    /// Enable bidirectional text support.
    pub bidi_enabled: bool,
    /// Background opacity (0.0 = fully transparent, 1.0 = fully opaque).
    pub background_opacity: f32,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            font_family: "monospace".into(),
            font_size: 13.0,
            cursor_style: "Block".into(),
            cursor_blink: true,
            scrollback_lines: 10000,
            color_scheme: ColorScheme::default(),
            enable_csi_u: false,
            enable_kitty_graphics: false,
            enable_kitty_keyboard: false,
            debug_key_events: false,
            debug_escape_sequences: false,
            bidi_enabled: false,
            background_opacity: 0.85,
        }
    }
}
