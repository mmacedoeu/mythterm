//! Terminal renderer: orchestrates the rendering pipeline.
//!
//! Combines the glyph atlas, terminal pipeline, and quad generation
//! to render terminal content to a wgpu surface.

use crate::atlas::GlyphAtlas;
use crate::pipeline::{QuadVertex, TerminalPipeline};
use mythterm_font::{FontData, FontDiscovery, FontMetrics, GlyphRasterizer, TextShaper, GlyphKey};
use std::collections::HashMap;

/// Terminal renderer.
///
/// Manages the complete rendering pipeline for terminal content:
/// - Glyph atlas for efficient text rendering
/// - Terminal pipeline with WGSL shaders
/// - Quad generation from terminal cell data
pub struct TerminalRenderer {
    /// The GPU device.
    device: wgpu::Device,
    /// The GPU queue.
    queue: wgpu::Queue,
    /// The glyph atlas.
    atlas: GlyphAtlas,
    /// The terminal rendering pipeline.
    pipeline: TerminalPipeline,
    /// The font rasterizer.
    rasterizer: GlyphRasterizer,
    /// The text shaper.
    shaper: TextShaper,
    /// The font discovery system.
    font_discovery: FontDiscovery,
    /// Loaded fonts.
    fonts: HashMap<String, FontData>,
    /// Font metrics.
    metrics: FontMetrics,
    /// Screen dimensions.
    screen_width: u32,
    screen_height: u32,
}

impl TerminalRenderer {
    /// Create a new terminal renderer with an existing wgpu device and queue.
    ///
    /// This allows integration with Myth's existing GPU context.
    pub fn new(
        device: wgpu::Device,
        queue: wgpu::Queue,
        format: wgpu::TextureFormat,
        width: u32,
        height: u32,
    ) -> anyhow::Result<Self> {
        // Create glyph atlas (2048x2048 should be enough for most use cases)
        let atlas = GlyphAtlas::new(&device, 2048, 2048);

        // Create terminal pipeline
        let pipeline = TerminalPipeline::new(&device, format, atlas.bind_group_layout());

        // Create font tools
        let rasterizer = GlyphRasterizer::new();
        let shaper = TextShaper::new();
        let font_discovery = FontDiscovery::new();
        let metrics = FontMetrics::default();

        Ok(Self {
            device,
            queue,
            atlas,
            pipeline,
            rasterizer,
            shaper,
            font_discovery,
            fonts: HashMap::new(),
            metrics,
            screen_width: width,
            screen_height: height,
        })
    }

    /// Load a font by family name.
    pub fn load_font(&mut self, family: &str, bold: bool, italic: bool) -> anyhow::Result<()> {
        let font = self.font_discovery.find_font(family, bold, italic)?;
        self.fonts.insert(family.to_string(), font);
        Ok(())
    }

    /// Render terminal text to the given render pass.
    ///
    /// Takes a string and position, shapes and rasterizes the text,
    /// generates quads, and renders them.
    pub fn render_text<'a>(
        &'a mut self,
        render_pass: &mut wgpu::RenderPass<'a>,
        text: &str,
        x: f32,
        y: f32,
        fg_color: [f32; 4],
        bg_color: [f32; 4],
    ) -> anyhow::Result<()> {
        // Get the default font
        let font = self.fonts.values().next()
            .ok_or_else(|| anyhow::anyhow!("No fonts loaded"))?;

        // Shape the text
        let shaped = self.shaper.shape(font, self.metrics.cell_height, text)?;

        let mut vertices = Vec::new();
        let mut indices = Vec::new();
        let mut cursor_x = x;

        for glyph in &shaped.glyphs {
            let glyph_key = GlyphKey {
                glyph_id: glyph.glyph_id,
                font_size: self.metrics.cell_height as u32,
                bold: font.bold,
            };

            // Rasterize the glyph if not in atlas
            if self.atlas.get(&glyph_key).is_none() {
                let rasterized = self.rasterizer.rasterize(
                    font,
                    glyph.glyph_id,
                    self.metrics.cell_height,
                )?;
                self.atlas.get_or_insert(&self.device, &self.queue, glyph_key, &rasterized);
            }

            // Get the atlas region
            if let Some(region) = self.atlas.get(&glyph_key) {
                let x_pos = cursor_x + glyph.x_offset;
                let y_pos = y;
                let w = self.metrics.cell_width;
                let h = self.metrics.cell_height;

                // Convert to clip space (-1 to 1)
                let x0 = (x_pos / self.screen_width as f32) * 2.0 - 1.0;
                let y0 = 1.0 - (y_pos / self.screen_height as f32) * 2.0;
                let x1 = ((x_pos + w) / self.screen_width as f32) * 2.0 - 1.0;
                let y1 = 1.0 - ((y_pos + h) / self.screen_height as f32) * 2.0;

                let base = vertices.len() as u32;

                // Background quad (solid color)
                vertices.extend_from_slice(&[
                    QuadVertex { position: [x0, y0], uv: [0.0, 0.0], fg_color, bg_color },
                    QuadVertex { position: [x1, y0], uv: [0.0, 0.0], fg_color, bg_color },
                    QuadVertex { position: [x1, y1], uv: [0.0, 0.0], fg_color, bg_color },
                    QuadVertex { position: [x0, y1], uv: [0.0, 0.0], fg_color, bg_color },
                ]);

                // Foreground (glyph) quad with atlas UVs
                vertices.extend_from_slice(&[
                    QuadVertex { position: [x0, y0], uv: region.uv_min, fg_color, bg_color },
                    QuadVertex { position: [x1, y0], uv: [region.uv_max[0], region.uv_min[1]], fg_color, bg_color },
                    QuadVertex { position: [x1, y1], uv: region.uv_max, fg_color, bg_color },
                    QuadVertex { position: [x0, y1], uv: [region.uv_min[0], region.uv_max[1]], fg_color, bg_color },
                ]);

                // Indices for both quads
                indices.extend_from_slice(&[
                    base, base + 1, base + 2, base, base + 2, base + 3, // bg
                    base + 4, base + 5, base + 6, base + 4, base + 6, base + 7, // fg
                ]);
            }

            cursor_x += glyph.advance_x;
        }

        // Update buffers and render
        self.pipeline.update_buffers(&self.queue, &vertices, &indices);
        self.pipeline.update_screen_size(&self.queue, self.screen_width as f32, self.screen_height as f32);
        self.pipeline.render(render_pass, self.atlas.bind_group());

        Ok(())
    }

    /// Resize the renderer.
    pub fn resize(&mut self, width: u32, height: u32) {
        self.screen_width = width;
        self.screen_height = height;
    }

    /// Get the font metrics.
    pub fn metrics(&self) -> &FontMetrics {
        &self.metrics
    }

    /// Get a reference to the GPU device.
    pub fn device(&self) -> &wgpu::Device {
        &self.device
    }

    /// Get a reference to the GPU queue.
    pub fn queue(&self) -> &wgpu::Queue {
        &self.queue
    }
}
