//! mythterm-font: Font discovery, text shaping, and glyph rasterization.
//!
//! This crate provides a pure Rust font pipeline using:
//! - `fontconfig` for font discovery (Linux)
//! - `rustybuzz` for text shaping (HarfBuzz-compatible)
//! - `ab_glyph` for glyph rasterization

pub mod discovery;
pub mod metrics;
pub mod rasterize;
pub mod shape;

pub use discovery::FontDiscovery;
pub use metrics::FontMetrics;
pub use rasterize::GlyphRasterizer;
pub use shape::TextShaper;

/// A loaded font with its data and metadata.
#[derive(Debug)]
pub struct FontData {
    /// The font data bytes.
    pub data: Vec<u8>,
    /// The font family name.
    pub family: String,
    /// The font index (for TTC/OTC collections).
    pub index: u32,
    /// Whether this is a bold font.
    pub bold: bool,
    /// Whether this is an italic font.
    pub italic: bool,
}

/// A key for identifying a specific glyph in the atlas.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct GlyphKey {
    /// The glyph ID from the shaping engine.
    pub glyph_id: u32,
    /// The font size in pixels.
    pub font_size: u32,
    /// Whether this is a bold glyph.
    pub bold: bool,
}

/// A rasterized glyph with its bitmap and metrics.
#[derive(Debug, Clone)]
pub struct RasterizedGlyph {
    /// The glyph key.
    pub key: GlyphKey,
    /// The glyph bitmap (alpha values, single channel).
    pub bitmap: Vec<u8>,
    /// Width of the bitmap in pixels.
    pub width: u32,
    /// Height of the bitmap in pixels.
    pub height: u32,
    /// X offset from the pen position to the left edge of the bitmap.
    pub left: i32,
    /// Y offset from the baseline to the top edge of the bitmap.
    pub top: i32,
    /// Horizontal advance width in pixels.
    pub advance_x: f32,
}
