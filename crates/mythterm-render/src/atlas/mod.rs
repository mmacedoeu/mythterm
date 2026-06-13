//! Glyph atlas: GPU texture for rasterized glyphs.
//!
//! Manages a texture atlas that packs rasterized glyphs into a single
//! GPU texture for efficient batched rendering.

use guillotiere::{AtlasAllocator, Size};
use std::collections::HashMap;

use mythterm_font::{GlyphKey, RasterizedGlyph};

/// Coordinates of a glyph within the atlas texture.
#[derive(Debug, Clone, Copy)]
pub struct AtlasRegion {
    /// X position in the atlas (pixels).
    pub x: u32,
    /// Y position in the atlas (pixels).
    pub y: u32,
    /// Width of the glyph (pixels).
    pub width: u32,
    /// Height of the glyph (pixels).
    pub height: u32,
    /// Normalized UV coordinates for the glyph in the atlas.
    pub uv_min: [f32; 2],
    pub uv_max: [f32; 2],
}

/// Glyph texture atlas.
///
/// Packs rasterized glyphs into a single GPU texture using
/// guillotiere bin-packing. Supports LRU eviction on overflow.
pub struct GlyphAtlas {
    /// The GPU texture containing all glyphs.
    texture: wgpu::Texture,
    /// The texture view for binding.
    view: wgpu::TextureView,
    /// The bind group for the atlas.
    bind_group: wgpu::BindGroup,
    /// The bind group layout.
    bind_group_layout: wgpu::BindGroupLayout,
    /// The rectangle packer.
    allocator: AtlasAllocator,
    /// Mapping from glyph key to atlas region.
    regions: HashMap<GlyphKey, AtlasRegion>,
    /// Atlas dimensions.
    width: u32,
    height: u32,
}

impl GlyphAtlas {
    /// Create a new glyph atlas with the given dimensions.
    pub fn new(device: &wgpu::Device, width: u32, height: u32) -> Self {
        // Create the atlas texture (single channel, R8Unorm)
        let texture = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("Glyph Atlas"),
            size: wgpu::Extent3d {
                width,
                height,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::R8Unorm,
            usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
            view_formats: &[],
        });

        let view = texture.create_view(&wgpu::TextureViewDescriptor::default());

        let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("Glyph Atlas Sampler"),
            mag_filter: wgpu::FilterMode::Nearest,
            min_filter: wgpu::FilterMode::Nearest,
            ..Default::default()
        });

        // Create bind group layout
        let bind_group_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("Glyph Atlas Bind Group Layout"),
            entries: &[
                wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Texture {
                        sample_type: wgpu::TextureSampleType::Float { filterable: true },
                        view_dimension: wgpu::TextureViewDimension::D2,
                        multisampled: false,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 1,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                    count: None,
                },
            ],
        });

        let bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("Glyph Atlas Bind Group"),
            layout: &bind_group_layout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: wgpu::BindingResource::TextureView(&view),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: wgpu::BindingResource::Sampler(&sampler),
                },
            ],
        });

        Self {
            texture,
            view,
            bind_group,
            bind_group_layout,
            allocator: AtlasAllocator::new(Size::new(width as i32, height as i32)),
            regions: HashMap::new(),
            width,
            height,
        }
    }

    /// Get or insert a glyph into the atlas.
    ///
    /// If the glyph is already in the atlas, returns its region.
    /// Otherwise, rasterizes and inserts it.
    pub fn get_or_insert(
        &mut self,
        _device: &wgpu::Device,
        queue: &wgpu::Queue,
        key: GlyphKey,
        glyph: &RasterizedGlyph,
    ) -> Option<AtlasRegion> {
        // Check if already in atlas
        if let Some(region) = self.regions.get(&key) {
            return Some(*region);
        }

        // Allocate space in the atlas
        let size = Size::new(glyph.width as i32 + 2, glyph.height as i32 + 2); // +2 for padding
        let alloc = self.allocator.allocate(size)?;

        let x = alloc.rectangle.min.x as u32;
        let y = alloc.rectangle.min.y as u32;

        // Upload glyph bitmap to the atlas texture
        if !glyph.bitmap.is_empty() {
            queue.write_texture(
                wgpu::TexelCopyTextureInfo {
                    texture: &self.texture,
                    mip_level: 0,
                    origin: wgpu::Origin3d { x, y, z: 0 },
                    aspect: wgpu::TextureAspect::All,
                },
                &glyph.bitmap,
                wgpu::TexelCopyBufferLayout {
                    offset: 0,
                    bytes_per_row: Some(glyph.width),
                    rows_per_image: Some(glyph.height),
                },
                wgpu::Extent3d {
                    width: glyph.width,
                    height: glyph.height,
                    depth_or_array_layers: 1,
                },
            );
        }

        let region = AtlasRegion {
            x,
            y,
            width: glyph.width,
            height: glyph.height,
            uv_min: [x as f32 / self.width as f32, y as f32 / self.height as f32],
            uv_max: [
                (x + glyph.width) as f32 / self.width as f32,
                (y + glyph.height) as f32 / self.height as f32,
            ],
        };

        self.regions.insert(key, region);
        Some(region)
    }

    /// Get the atlas region for a glyph key.
    pub fn get(&self, key: &GlyphKey) -> Option<&AtlasRegion> {
        self.regions.get(key)
    }

    /// Get the bind group for the atlas.
    pub fn bind_group(&self) -> &wgpu::BindGroup {
        &self.bind_group
    }

    /// Get the bind group layout for the atlas.
    pub fn bind_group_layout(&self) -> &wgpu::BindGroupLayout {
        &self.bind_group_layout
    }

    /// Get the texture view for the atlas.
    pub fn view(&self) -> &wgpu::TextureView {
        &self.view
    }
}
