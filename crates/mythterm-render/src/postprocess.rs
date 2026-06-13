//! Post-processing pass.
//!
//! Owns the full-resolution HDR "scene" texture that the bloom (or
//! any future scene pass) writes its combined output into, plus the
//! final tonemap pass that takes that HDR scene and writes an
//! sRGB-ready result to the swapchain.
//!
//! Future scene passes (LCD subpixel simulation, glass, reflections)
//! are expected to chain between the bloom output and the tonemap
//! input by reading from / writing to the scene buffer exposed via
//! [`PostProcess::scene_view`].

use wgpu::{Device, Extent3d, TextureDescriptor, TextureDimension, TextureFormat, TextureUsages};

/// Post-processing pipeline.
///
/// Holds an HDR scene texture (the working surface for the post
/// stack) and the tonemap pipeline that writes the final sRGB-ready
/// image to the swapchain.
pub struct PostProcess {
    /// Full-resolution HDR scene texture.
    scene_texture: wgpu::Texture,
    /// View of the scene texture, for use as a render attachment.
    scene_view: wgpu::TextureView,
    /// View of the scene texture, for use as a sampled binding.
    scene_sample_view: wgpu::TextureView,
    /// Sampler for the scene texture.
    scene_sampler: wgpu::Sampler,
    /// Tonemap (ACES + vignette) pipeline.
    tonemap_pipeline: wgpu::RenderPipeline,
    /// Bind group layout for the tonemap pass.
    bind_group_layout: wgpu::BindGroupLayout,
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
        let (scene_texture, scene_view, scene_sample_view, scene_sampler) = Self::create_scene(device, width, height);
        let (bind_group_layout, tonemap_pipeline) = Self::create_pipelines(device, output_format);

        Self {
            scene_texture,
            scene_view,
            scene_sample_view,
            scene_sampler,
            tonemap_pipeline,
            bind_group_layout,
            width,
            height,
        }
    }

    /// Resize the scene texture to new dimensions.
    pub fn resize(&mut self, device: &Device, width: u32, height: u32) {
        if self.width == width && self.height == height {
            return;
        }
        self.width = width;
        self.height = height;
        let (tex, view, sample_view, sampler) = Self::create_scene(device, width, height);
        self.scene_texture = tex;
        self.scene_view = view;
        self.scene_sample_view = sample_view;
        self.scene_sampler = sampler;
    }

    /// View of the HDR scene texture, for use as a render attachment
    /// (e.g. by the bloom combine pass).
    pub fn scene_view(&self) -> &wgpu::TextureView {
        &self.scene_view
    }

    /// View of the HDR scene texture, for use as a sampled binding.
    pub fn scene_sample_view(&self) -> &wgpu::TextureView {
        &self.scene_sample_view
    }

    /// Current scene width.
    pub fn width(&self) -> u32 {
        self.width
    }

    /// Current scene height.
    pub fn height(&self) -> u32 {
        self.height
    }

    /// Run the tonemap pass: read the HDR scene texture, apply ACES
    /// tonemap + vignette, write to `output_view`.
    pub fn render(&self, device: &Device, encoder: &mut wgpu::CommandEncoder, output_view: &wgpu::TextureView) {
        let bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("Tonemap Input"),
            layout: &self.bind_group_layout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: wgpu::BindingResource::TextureView(&self.scene_sample_view),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: wgpu::BindingResource::Sampler(&self.scene_sampler),
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
        rpass.set_bind_group(0, &bind_group, &[]);
        rpass.draw(0..3, 0..1);
    }

    fn create_scene(
        device: &Device,
        width: u32,
        height: u32,
    ) -> (wgpu::Texture, wgpu::TextureView, wgpu::TextureView, wgpu::Sampler) {
        let texture = device.create_texture(&TextureDescriptor {
            label: Some("PostProcess Scene"),
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
        let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("PostProcess Scene Sampler"),
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            ..Default::default()
        });
        (texture, view, sample_view, sampler)
    }

    fn create_pipelines(
        device: &Device,
        output_format: TextureFormat,
    ) -> (wgpu::BindGroupLayout, wgpu::RenderPipeline) {
        let bind_group_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("PostProcess Bind Group Layout"),
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
            label: Some("PostProcess Pipeline Layout"),
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
