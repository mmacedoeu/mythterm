//! Offscreen render target for the terminal content.
//!
//! Renders egui terminal content into an HDR texture that can be
//! used as input for post-processing effects (bloom, tonemap, etc.)

use wgpu::{Device, Extent3d, TextureDescriptor, TextureDimension, TextureFormat, TextureUsages};

/// Offscreen render target for terminal content.
pub struct RenderTarget {
    /// The HDR texture.
    pub texture: wgpu::Texture,
    /// Texture view for rendering into.
    pub view: wgpu::TextureView,
    /// Texture view for sampling.
    pub sample_view: wgpu::TextureView,
    /// Sampler for reading the texture.
    pub sampler: wgpu::Sampler,
    /// Width in pixels.
    pub width: u32,
    /// Height in pixels.
    pub height: u32,
}

impl RenderTarget {
    /// Create a new render target with the given dimensions.
    pub fn new(device: &Device, width: u32, height: u32) -> Self {
        let texture = device.create_texture(&TextureDescriptor {
            label: Some("Terminal HDR Render Target"),
            size: Extent3d {
                width,
                height,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: TextureDimension::D2,
            format: TextureFormat::Rgba16Float,
            usage: TextureUsages::RENDER_ATTACHMENT | TextureUsages::TEXTURE_BINDING,
            view_formats: &[],
        });

        let view = texture.create_view(&wgpu::TextureViewDescriptor::default());
        let sample_view = texture.create_view(&wgpu::TextureViewDescriptor::default());

        let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("Terminal RT Sampler"),
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            ..Default::default()
        });

        Self {
            texture,
            view,
            sample_view,
            sampler,
            width,
            height,
        }
    }

    /// Resize the render target.
    pub fn resize(&mut self, device: &Device, width: u32, height: u32) {
        if self.width == width && self.height == height {
            return;
        }
        *self = Self::new(device, width, height);
    }
}
