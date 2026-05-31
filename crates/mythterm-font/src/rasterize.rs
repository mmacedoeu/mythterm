//! Glyph rasterization with ab_glyph.
//!
//! Converts glyph IDs into bitmaps that can be uploaded to the GPU atlas.

use crate::{FontData, GlyphKey, RasterizedGlyph};
use ab_glyph::{Font, FontRef, PxScale, ScaleFont};
use anyhow::Result;

/// Glyph rasterizer using ab_glyph.
pub struct GlyphRasterizer;

impl GlyphRasterizer {
    /// Create a new glyph rasterizer.
    pub fn new() -> Self {
        Self
    }

    /// Rasterize a single glyph.
    ///
    /// Takes font data, a glyph ID, and font size, returns the rasterized bitmap.
    pub fn rasterize(
        &self,
        font_data: &FontData,
        glyph_id: u32,
        font_size: f32,
    ) -> Result<RasterizedGlyph> {
        let font = FontRef::try_from_slice(&font_data.data)
            .map_err(|e| anyhow::anyhow!("Failed to parse font: {:?}", e))?;

        let scale = PxScale::from(font_size);
        let scaled = font.as_scaled(scale);

        let glyph = ab_glyph::GlyphId(glyph_id as u16).with_scale(scale);

        // Get the outline
        let outline = font.outline_glyph(glyph)
            .ok_or_else(|| anyhow::anyhow!("Glyph {} not found", glyph_id))?;

        let bounds = outline.px_bounds();

        // Rasterize the glyph
        let width = (bounds.width().ceil() as u32).max(1);
        let height = (bounds.height().ceil() as u32).max(1);
        let mut bitmap = vec![0u8; (width * height) as usize];

        outline.draw(|x, y, coverage| {
            let x = x as u32;
            let y = y as u32;
            if x < width && y < height {
                let idx = (y * width + x) as usize;
                bitmap[idx] = (coverage * 255.0) as u8;
            }
        });

        let advance_x = scaled.h_advance(ab_glyph::GlyphId(glyph_id as u16));

        Ok(RasterizedGlyph {
            key: GlyphKey {
                glyph_id,
                font_size: font_size as u32,
                bold: font_data.bold,
            },
            bitmap,
            width,
            height,
            left: bounds.min.x as i32,
            top: bounds.min.y as i32,
            advance_x,
        })
    }

    /// Rasterize multiple glyphs in batch.
    pub fn rasterize_batch(
        &self,
        font_data: &FontData,
        glyph_ids: &[u32],
        font_size: f32,
    ) -> Result<Vec<RasterizedGlyph>> {
        glyph_ids
            .iter()
            .map(|&id| self.rasterize(font_data, id, font_size))
            .collect()
    }
}

impl Default for GlyphRasterizer {
    fn default() -> Self {
        Self::new()
    }
}
