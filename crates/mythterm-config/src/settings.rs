use serde::{Deserialize, Serialize};

use crate::scheme::ColorScheme;

/// Cinematic / post-processing settings.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct CinematicSettings {
    /// LCD subpixel pass strength (0.0 = off, 1.0 = full effect).
    /// Default: 0.35 — subtle but visible color fringing on text edges.
    pub lcd_strength: f32,
    /// Subpixel width as fraction of a pixel.
    /// 0.33 = RGB stripe, 0.5 = RGBG PenTile.
    pub lcd_subpixel_width: f32,
    /// LCD scanline modulation (0.0 = off, 1.0 = full scanline).
    pub lcd_scanline: f32,
    /// Corner vignette strength (0.0 = off, 1.0 = strong).
    /// Default: 0.25 — subtle corner darkening.
    pub vignette: f32,
    /// Edge lighting intensity (backlight bleed halo at screen edges).
    /// Default: 0.15 — subtle warm glow near the perimeter.
    pub edge_intensity: f32,
    /// Edge lighting width as a fraction of the screen edge.
    /// Default: 0.04 — halo extends ~4% inward from the edge.
    pub edge_width: f32,
    /// Edge lighting color (RGB, 0..1 each).
    /// Default: warm white (1.0, 0.85, 0.65) — like an incandescent backlight.
    pub edge_color: [f32; 3],
    /// Micro-contrast strength (S-curve applied to the tonemapped color).
    /// 0.0 = off, 1.0 = full smoothstep S-curve. Default: 0.15 — subtle.
    pub micro_contrast: f32,
    /// Glass cover reflection intensity.
    /// 0.0 = off, 1.0 = strong glass reflection. Default: 0.18.
    pub glass_intensity: f32,
    /// Glass Fresnel F0 (reflection at normal incidence).
    /// 0.04 is the physical value for glass. Default: 0.04.
    pub glass_fresnel_bias: f32,
    /// Glass top-gradient falloff exponent. Higher = more localized
    /// at the very top of the screen. Default: 3.0.
    pub glass_top_falloff: f32,
    /// Glass ceiling reflection color (RGB, 0..1 each).
    /// Default: warm white (1.0, 0.97, 0.92).
    pub glass_ceiling_color: [f32; 3],
    /// Screen curvature (barrel distortion in the shared vertex shader).
    /// 0.0 = flat, 0.05 = subtle curve, 0.1 = noticeable. Default: 0.05.
    pub screen_curvature: f32,
    /// Window corner radius in pixels (rounded outer window corners).
    /// 0.0 = sharp corners. Default: 10.0.
    pub window_corner_radius: f32,
    /// Window border glow color (sRGB, 0..255 each).
    /// Default: bright cyan (96, 188, 232) — matches the goal mockup.
    pub window_border_glow: [u8; 3],
    /// Window border glow width in pixels.
    /// 0.0 = no border. Default: 1.5.
    pub window_border_glow_width: f32,
}

impl Default for CinematicSettings {
    fn default() -> Self {
        Self {
            lcd_strength: 0.35,
            lcd_subpixel_width: 0.33,
            lcd_scanline: 0.0,
            vignette: 0.25,
            edge_intensity: 0.15,
            edge_width: 0.04,
            edge_color: [1.0, 0.85, 0.65],
            micro_contrast: 0.15,
            glass_intensity: 0.18,
            glass_fresnel_bias: 0.04,
            glass_top_falloff: 3.0,
            glass_ceiling_color: [1.0, 0.97, 0.92],
            screen_curvature: 0.05,
            window_corner_radius: 10.0,
            window_border_glow: [96, 188, 232],
            window_border_glow_width: 1.5,
        }
    }
}

/// Top-level mythterm configuration, loaded from TOML.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
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
    /// Cinematic / post-processing settings.
    pub cinematic: CinematicSettings,
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
            background_opacity: 0.8,
            cinematic: CinematicSettings::default(),
        }
    }
}
