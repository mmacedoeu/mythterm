//! Font metrics: cell size, ascent, descent, line gap.
//!
//! Computes the metrics needed for terminal grid layout.

use ab_glyph::{Font, ScaleFont};

/// Font metrics for terminal rendering.
#[derive(Debug, Clone, Copy)]
pub struct FontMetrics {
    /// Width of a single cell in pixels.
    pub cell_width: f32,
    /// Height of a single cell in pixels.
    pub cell_height: f32,
    /// Distance from baseline to top of cell in pixels.
    pub ascent: f32,
    /// Distance from baseline to bottom of cell in pixels (positive value).
    pub descent: f32,
    /// Line gap in pixels.
    pub line_gap: f32,
    /// Underline position (offset from baseline).
    pub underline_position: f32,
    /// Underline thickness in pixels.
    pub underline_thickness: f32,
}

impl FontMetrics {
    /// Compute metrics from an ab_glyph font at the given size.
    pub fn from_font(font: &ab_glyph::FontRef<'_>, font_size: f32) -> Self {
        let scale = ab_glyph::PxScale::from(font_size);
        let scaled = font.as_scaled(scale);

        let ascent = scaled.ascent();
        let descent = scaled.descent();
        let line_gap = scaled.line_gap();

        // Cell height is ascent + descent + line_gap
        let cell_height = ascent - descent + line_gap;

        // Cell width is typically the advance width of 'M' or 'W'
        let cell_width = scaled.h_advance(scaled.glyph_id('M'))
            .max(scaled.h_advance(scaled.glyph_id('W')));

        // Underline metrics - use reasonable defaults
        let underline_position = -descent * 0.2;
        let underline_thickness = 1.0;

        Self {
            cell_width,
            cell_height,
            ascent,
            descent: -descent, // Make positive
            line_gap,
            underline_position,
            underline_thickness,
        }
    }

    /// Create metrics with manual values.
    pub fn new(cell_width: f32, cell_height: f32, ascent: f32, descent: f32) -> Self {
        Self {
            cell_width,
            cell_height,
            ascent,
            descent,
            line_gap: 0.0,
            underline_position: -descent * 0.2,
            underline_thickness: 1.0,
        }
    }

    /// Compute cell size for a given font size.
    /// Uses a heuristic based on typical monospace font metrics.
    pub fn estimate_cell_size(font_size: f32) -> (f32, f32) {
        // Typical monospace font: width ≈ 0.6 * height
        let cell_height = font_size * 1.2;
        let cell_width = cell_height * 0.6;
        (cell_width, cell_height)
    }
}

impl Default for FontMetrics {
    fn default() -> Self {
        let (cell_width, cell_height) = Self::estimate_cell_size(14.0);
        Self::new(cell_width, cell_height, cell_height * 0.8, cell_height * 0.2)
    }
}
