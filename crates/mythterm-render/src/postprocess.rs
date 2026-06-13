//! Post-processing pass.
//!
//! Owns two full-resolution HDR "scene" textures that the bloom (or
//! any future scene pass) writes its combined output into, plus the
//! LCD subpixel pass and the final tonemap pass that writes an
//! sRGB-ready result to the swapchain.
//!
//! Render chain:
//!
//! ```text
//! bloom (or any scene pass)  →  scene_a  (HDR, sampled by LCD)
//! LCD subpixel pass           →  scene_b  (HDR, sampled by tonemap)
//! Tonemap pass                →  swapchain
//! ```
//!
//! Future scene passes (curved display mesh, glass, reflections, edge
//! lighting) are expected to chain in by reading from / writing to
//! the scene textures exposed via [`PostProcess::scene_view`] and
//! [`PostProcess::scene_b_view`], or by inserting between the LCD and
//! tonemap passes.

use wgpu::{Device, Extent3d, TextureDescriptor, TextureDimension, TextureFormat, TextureUsages};

/// LCD subpixel pass parameters (mirrors the WGSL `LcdParams` struct).
///
/// 16 bytes — keep this layout in sync with the WGSL.
#[repr(C)]
#[derive(Debug, Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
pub struct LcdParams {
    /// 0..1: blend between original and subpixel-sampled result.
    pub strength: f32,
    /// Subpixel width as fraction of a pixel.
    /// 0.33 for RGB stripe, 0.5 for RGBG PenTile, etc.
    pub subpixel_width: f32,
    /// 0..1: scanline modulation amplitude. 0 disables.
    pub scanline: f32,
    /// 16-byte alignment pad.
    pub _pad: f32,
}

impl Default for LcdParams {
    fn default() -> Self {
        Self {
            strength: 0.35,
            subpixel_width: 0.33,
            scanline: 0.0,
            _pad: 0.0,
        }
    }
}

/// Post-processing pipeline.
///
/// Holds two HDR scene textures (the working surface for the post
/// stack), the LCD subpixel pipeline, and the tonemap pipeline that
/// writes the final sRGB-ready image to the swapchain.
pub struct PostProcess {
    /// First HDR scene texture: bloom writes here, LCD reads from here.
    scene_a_texture: wgpu::Texture,
    scene_a_view: wgpu::TextureView,
    scene_a_sample_view: wgpu::TextureView,

    /// Second HDR scene texture: LCD writes here, tonemap reads from here.
    scene_b_texture: wgpu::Texture,
    scene_b_view: wgpu::TextureView,
    scene_b_sample_view: wgpu::TextureView,

    /// Shared sampler for scene_a → LCD read.
    sampler: wgpu::Sampler,

    /// LCD subpixel pipeline.
    lcd_pipeline: wgpu::RenderPipeline,
    /// Bind group layout for the LCD pass.
    lcd_bind_group_layout: wgpu::BindGroupLayout,
    /// Uniform buffer holding [`LcdParams`].
    lcd_uniform_buffer: wgpu::Buffer,

    /// Tonemap (ACES + vignette) pipeline.
    tonemap_pipeline: wgpu::RenderPipeline,
    /// Bind group layout for the tonemap pass.
    tonemap_bind_group_layout: wgpu::BindGroupLayout,

    /// Current scene width.
    width: u32,
    /// Current scene height.
    height: u32,
}

impl PostProcess {
    /// Create a new post-processing pipeline.
    ///
    /// `output_format` is the format of the swapchain (e.g.
    /// `Bgra8UnormSrgb`). The tonemap pass writes into it.
    pub fn new(device: &Device, output_format: TextureFormat, width: u32, height: u32) -> Self {
        let (scene_a_texture, scene_a_view, scene_a_sample_view, scene_b_texture, scene_b_view, scene_b_sample_view, sampler) =
            Self::create_scenes(device, width, height);
        let (lcd_bind_group_layout, lcd_pipeline, lcd_uniform_buffer) =
            Self::create_lcd_pipeline(device, &sampler);
        let (tonemap_bind_group_layout, tonemap_pipeline) = Self::create_tonemap_pipeline(device, output_format);

        Self {
            scene_a_texture,
            scene_a_view,
            scene_a_sample_view,
            scene_b_texture,
            scene_b_view,
            scene_b_sample_view,
            sampler,
            lcd_pipeline,
            lcd_bind_group_layout,
            lcd_uniform_buffer,
            tonemap_pipeline,
            tonemap_bind_group_layout,
            width,
            height,
        }
    }

    /// Resize the scene textures to new dimensions.
    pub fn resize(&mut self, device: &Device, width: u32, height: u32) {
        if self.width == width && self.height == height {
            return;
        }
        self.width = width;
        self.height = height;
        let (a_tex, a_view, a_sample_view, b_tex, b_view, b_sample_view, sampler) =
            Self::create_scenes(device, width, height);
        self.scene_a_texture = a_tex;
        self.scene_a_view = a_view;
        self.scene_a_sample_view = a_sample_view;
        self.scene_b_texture = b_tex;
        self.scene_b_view = b_view;
        self.scene_b_sample_view = b_sample_view;
        self.sampler = sampler;
    }

    /// View of the first HDR scene texture, for use as a render
    /// attachment (e.g. by the bloom combine pass).
    pub fn scene_view(&self) -> &wgpu::TextureView {
        &self.scene_a_view
    }

    /// View of the first HDR scene texture, for use as a sampled
    /// binding.
    pub fn scene_sample_view(&self) -> &wgpu::TextureView {
        &self.scene_a_sample_view
    }

    /// View of the second HDR scene texture (LCD output), for use
    /// as a render attachment by any future pass inserted between
    /// the LCD and the tonemap.
    pub fn scene_b_view(&self) -> &wgpu::TextureView {
        &self.scene_b_view
    }

    /// View of the second HDR scene texture (LCD output), for use as
    /// a sampled binding.
    pub fn scene_b_sample_view(&self) -> &wgpu::TextureView {
        &self.scene_b_sample_view
    }

    /// Current scene width.
    pub fn width(&self) -> u32 {
        self.width
    }

    /// Current scene height.
    pub fn height(&self) -> u32 {
        self.height
    }

    /// Update the LCD subpixel pass parameters.
    pub fn set_lcd_params(&self, queue: &wgpu::Queue, params: LcdParams) {
        queue.write_buffer(&self.lcd_uniform_buffer, 0, bytemuck::cast_slice(&[params]));
    }

    /// Run the LCD subpixel + tonemap passes.
    ///
    /// Reads from scene_a (typically written by bloom), applies the
    /// LCD subpixel effect, then tonemaps the result to `output_view`.
    pub fn render(&self, device: &Device, encoder: &mut wgpu::CommandEncoder, output_view: &wgpu::TextureView) {
        // LCD pass: scene_a → scene_b
        let lcd_bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("LCD Input"),
            layout: &self.lcd_bind_group_layout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: wgpu::BindingResource::TextureView(&self.scene_a_sample_view),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: wgpu::BindingResource::Sampler(&self.sampler),
                },
                wgpu::BindGroupEntry {
                    binding: 2,
                    resource: self.lcd_uniform_buffer.as_entire_binding(),
                },
            ],
        });

        {
            let mut rpass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("PostProcess: LCD"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &self.scene_b_view,
                    resolve_target: None,
                    depth_slice: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(wgpu::Color::BLACK),
                        store: wgpu::StoreOp::Store,
                    },
                })],
                depth_stencil_attachment: None,
                timestamp_writes: None,
                occlusion_query_set: None,
                multiview_mask: None,
            }).forget_lifetime();

            rpass.set_pipeline(&self.lcd_pipeline);
            rpass.set_bind_group(0, &lcd_bind_group, &[]);
            rpass.draw(0..3, 0..1);
        }

        // Tonemap pass: scene_b → swapchain
        let tonemap_bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("Tonemap Input"),
            layout: &self.tonemap_bind_group_layout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: wgpu::BindingResource::TextureView(&self.scene_b_sample_view),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: wgpu::BindingResource::Sampler(&self.sampler),
                },
            ],
        });

        let mut rpass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("PostProcess: Tonemap"),
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                view: output_view,
                resolve_target: None,
                depth_slice: None,
                ops: wgpu::Operations {
                    load: wgpu::LoadOp::Clear(wgpu::Color::BLACK),
                    store: wgpu::StoreOp::Store,
                },
            })],
            depth_stencil_attachment: None,
            timestamp_writes: None,
            occlusion_query_set: None,
            multiview_mask: None,
        }).forget_lifetime();

        rpass.set_pipeline(&self.tonemap_pipeline);
        rpass.set_bind_group(0, &tonemap_bind_group, &[]);
        rpass.draw(0..3, 0..1);
    }

    fn create_scenes(
        device: &Device,
        width: u32,
        height: u32,
    ) -> (
        wgpu::Texture,
        wgpu::TextureView,
        wgpu::TextureView,
        wgpu::Texture,
        wgpu::TextureView,
        wgpu::TextureView,
        wgpu::Sampler,
    ) {
        let a = Self::create_scene(device, "PostProcess Scene A", width, height);
        let b = Self::create_scene(device, "PostProcess Scene B", width, height);
        let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("PostProcess Scene Sampler"),
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            ..Default::default()
        });
        (a.0, a.1, a.2, b.0, b.1, b.2, sampler)
    }

    fn create_scene(
        device: &Device,
        label: &str,
        width: u32,
        height: u32,
    ) -> (wgpu::Texture, wgpu::TextureView, wgpu::TextureView) {
        let texture = device.create_texture(&TextureDescriptor {
            label: Some(label),
            size: Extent3d { width, height, depth_or_array_layers: 1 },
            mip_level_count: 1,
            sample_count: 1,
            dimension: TextureDimension::D2,
            format: TextureFormat::Rgba16Float,
            usage: TextureUsages::RENDER_ATTACHMENT | TextureUsages::TEXTURE_BINDING,
            view_formats: &[],
        });
        let view = texture.create_view(&wgpu::TextureViewDescriptor::default());
        let sample_view = texture.create_view(&wgpu::TextureViewDescriptor::default());
        (texture, view, sample_view)
    }

    fn create_lcd_pipeline(
        device: &Device,
        _sampler: &wgpu::Sampler,
    ) -> (wgpu::BindGroupLayout, wgpu::RenderPipeline, wgpu::Buffer) {
        let bind_group_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("LCD Bind Group Layout"),
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
                wgpu::BindGroupLayoutEntry {
                    binding: 2,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Uniform,
                        has_dynamic_offset: false,
                        min_binding_size: wgpu::BufferSize::new(16),
                    },
                    count: None,
                },
            ],
        });

        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("LCD Pipeline Layout"),
            bind_group_layouts: &[Some(&bind_group_layout)],
            immediate_size: 0,
        });

        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("PostProcess Shader"),
            source: wgpu::ShaderSource::Wgsl(include_str!("postprocess.wgsl").into()),
        });

        let lcd_pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("PostProcess Pipeline: lcd_fs"),
            layout: Some(&pipeline_layout),
            vertex: wgpu::VertexState {
                module: &shader,
                entry_point: Some("vs_main"),
                buffers: &[],
                compilation_options: Default::default(),
            },
            fragment: Some(wgpu::FragmentState {
                module: &shader,
                entry_point: Some("lcd_fs"),
                targets: &[Some(wgpu::ColorTargetState {
                    format: TextureFormat::Rgba16Float,
                    blend: Some(wgpu::BlendState::REPLACE),
                    write_mask: wgpu::ColorWrites::ALL,
                })],
                compilation_options: Default::default(),
            }),
            primitive: wgpu::PrimitiveState {
                topology: wgpu::PrimitiveTopology::TriangleList,
                ..Default::default()
            },
            depth_stencil: None,
            multisample: wgpu::MultisampleState::default(),
            multiview_mask: None,
            cache: None,
        });

        let lcd_uniform_buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("LCD Uniform Buffer"),
            size: 16,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });

        (bind_group_layout, lcd_pipeline, lcd_uniform_buffer)
    }

    fn create_tonemap_pipeline(
        device: &Device,
        output_format: TextureFormat,
    ) -> (wgpu::BindGroupLayout, wgpu::RenderPipeline) {
        let bind_group_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("Tonemap Bind Group Layout"),
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

        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("Tonemap Pipeline Layout"),
            bind_group_layouts: &[Some(&bind_group_layout)],
            immediate_size: 0,
        });

        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("PostProcess Shader"),
            source: wgpu::ShaderSource::Wgsl(include_str!("postprocess.wgsl").into()),
        });

        let tonemap_pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("PostProcess Pipeline: tonemap_fs"),
            layout: Some(&pipeline_layout),
            vertex: wgpu::VertexState {
                module: &shader,
                entry_point: Some("vs_main"),
                buffers: &[],
                compilation_options: Default::default(),
            },
            fragment: Some(wgpu::FragmentState {
                module: &shader,
                entry_point: Some("tonemap_fs"),
                targets: &[Some(wgpu::ColorTargetState {
                    format: output_format,
                    blend: Some(wgpu::BlendState::REPLACE),
                    write_mask: wgpu::ColorWrites::ALL,
                })],
                compilation_options: Default::default(),
            }),
            primitive: wgpu::PrimitiveState {
                topology: wgpu::PrimitiveTopology::TriangleList,
                ..Default::default()
            },
            depth_stencil: None,
            multisample: wgpu::MultisampleState::default(),
            multiview_mask: None,
            cache: None,
        });

        (bind_group_layout, tonemap_pipeline)
    }
}
