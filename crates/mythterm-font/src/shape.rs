//! Text shaping with rustybuzz.
//!
//! Converts text + font into positioned glyphs using HarfBuzz-compatible shaping.

use crate::FontData;
use anyhow::Result;
use rustybuzz::Face;
use std::str::FromStr;

/// A shaped glyph with position and advance information.
#[derive(Debug, Clone)]
pub struct ShapedGlyph {
    /// The glyph ID in the font.
    pub glyph_id: u32,
    /// X offset from the pen position.
    pub x_offset: f32,
    /// Y offset from the baseline.
    pub y_offset: f32,
    /// Horizontal advance width.
    pub advance_x: f32,
    /// Vertical advance height.
    pub advance_y: f32,
    /// The cluster index in the original text.
    pub cluster: u32,
}

/// Result of shaping a run of text.
#[derive(Debug, Clone)]
pub struct ShapedRun {
    /// The shaped glyphs.
    pub glyphs: Vec<ShapedGlyph>,
    /// The font size used for shaping.
    pub font_size: f32,
}

/// Text shaper using rustybuzz.
pub struct TextShaper;

impl TextShaper {
    /// Create a new text shaper.
    pub fn new() -> Self {
        Self
    }

    /// Shape text using the given font data.
    ///
    /// Takes font data and text, returns positioned glyphs.
    pub fn shape(
        &self,
        font_data: &FontData,
        font_size: f32,
        text: &str,
    ) -> Result<ShapedRun> {
        let face = Face::from_slice(&font_data.data, font_data.index)
            .ok_or_else(|| anyhow::anyhow!("Failed to create font face"))?;

        let _scale = (font_size * 64.0) as i32; // 26.6 fixed point (unused, rustybuzz handles it)
        let mut buffer = rustybuzz::UnicodeBuffer::new();
        buffer.push_str(text);
        buffer.set_direction(rustybuzz::Direction::LeftToRight);
        buffer.set_script(rustybuzz::script::LATIN);
        buffer.set_language(rustybuzz::Language::from_str("en").unwrap());

        let output = rustybuzz::shape(&face, &[], buffer);

        let mut glyphs = Vec::with_capacity(output.len());
        let positions = output.glyph_positions();
        let infos = output.glyph_infos();

        for (pos, info) in positions.iter().zip(infos.iter()) {
            glyphs.push(ShapedGlyph {
                glyph_id: info.glyph_id,
                x_offset: pos.x_offset as f32 / 64.0,
                y_offset: pos.y_offset as f32 / 64.0,
                advance_x: pos.x_advance as f32 / 64.0,
                advance_y: pos.y_advance as f32 / 64.0,
                cluster: info.cluster,
            });
        }

        Ok(ShapedRun {
            glyphs,
            font_size,
        })
    }
}

impl Default for TextShaper {
    fn default() -> Self {
        Self::new()
    }
}
