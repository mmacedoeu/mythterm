//! Bloom renderer with blur chain.
//!
//! Implements a multi-pass bloom effect using intermediate textures
//! at different resolutions (1/2, 1/4, 1/8).

use wgpu::{Device, Extent3d, TextureDescriptor, TextureDimension, TextureFormat, TextureUsages};

/// Intermediate texture for bloom blur passes.
#[allow(dead_code)]
struct BloomTexture {
    texture: wgpu::Texture,
    view: wgpu::TextureView,
    sampler: wgpu::Sampler,
    width: u32,
    height: u32,
}

/// Bloom renderer with multi-resolution blur chain.
#[allow(dead_code)]
pub struct BloomRenderer {
    /// Threshold extraction pipeline.
    threshold_pipeline: wgpu::RenderPipeline,
    /// Blur pipeline (used for each mip level).
    blur_pipeline: wgpu::RenderPipeline,
    /// Combine pipeline.
    combine_pipeline: wgpu::RenderPipeline,
    /// Intermediate textures at different resolutions.
    textures: Vec<BloomTexture>,
    /// Bind group layout.
    bind_group_layout: wgpu::BindGroupLayout,
    /// Combine bind group layout (needs 2 textures).
    combine_bind_group_layout: wgpu::BindGroupLayout,
    /// Cached bind groups for each texture.
    bind_groups: Vec<wgpu::BindGroup>,
}

impl BloomRenderer {
    /// Create a new bloom renderer.
    pub fn new(device: &Device, format: TextureFormat, width: u32, height: u32) -> Self {
        // Create bind group layout for single texture + sampler
        let bind_group_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("Bloom Bind Group Layout"),
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

        // Create bind group layout for combine (2 textures)
        let combine_bind_group_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("Bloom Combine Bind Group Layout"),
            entries: &[
                // Original texture
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
                // Bloom texture
                wgpu::BindGroupLayoutEntry {
                    binding: 2,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Texture {
                        sample_type: wgpu::TextureSampleType::Float { filterable: true },
                        view_dimension: wgpu::TextureViewDimension::D2,
                        multisampled: false,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 3,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                    count: None,
                },
            ],
        });

        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("Bloom Pipeline Layout"),
            bind_group_layouts: &[Some(&bind_group_layout)],
            immediate_size: 0,
        });

        let combine_pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("Bloom Combine Pipeline Layout"),
            bind_group_layouts: &[Some(&combine_bind_group_layout)],
            immediate_size: 0,
        });

        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("Bloom Shader"),
            source: wgpu::ShaderSource::Wgsl(include_str!("postprocess.wgsl").into()),
        });

        let threshold_pipeline = Self::create_pipeline(device, &pipeline_layout, &shader, format, "bloom_threshold_fs");
        let blur_pipeline = Self::create_pipeline(device, &pipeline_layout, &shader, format, "bloom_blur_fs");
        let combine_pipeline = Self::create_pipeline(device, &combine_pipeline_layout, &shader, format, "bloom_combine_fs");

        // Create intermediate textures at 1/2, 1/4, 1/8 resolution
        let mut textures = Vec::new();
        let mut w = width / 2;
        let mut h = height / 2;
        for _i in 0..3 {
            textures.push(Self::create_bloom_texture(device, w.max(1), h.max(1)));
            w /= 2;
            h /= 2;
        }

        // Create bind groups for each texture
        let mut bind_groups = Vec::new();
        for tex in &textures {
            let bg = device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some("Bloom Texture Bind Group"),
                layout: &bind_group_layout,
                entries: &[
                    wgpu::BindGroupEntry {
                        binding: 0,
                        resource: wgpu::BindingResource::TextureView(&tex.view),
                    },
                    wgpu::BindGroupEntry {
                        binding: 1,
                        resource: wgpu::BindingResource::Sampler(&tex.sampler),
                    },
                ],
            });
            bind_groups.push(bg);
        }

        Self {
            threshold_pipeline,
            blur_pipeline,
            combine_pipeline,
            textures,
            bind_group_layout,
            combine_bind_group_layout,
            bind_groups,
        }
    }

    fn create_pipeline(
        device: &Device,
        layout: &wgpu::PipelineLayout,
        shader: &wgpu::ShaderModule,
        format: TextureFormat,
        entry_point: &str,
    ) -> wgpu::RenderPipeline {
        device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some(&format!("Bloom Pipeline: {}", entry_point)),
            layout: Some(layout),
            vertex: wgpu::VertexState {
                module: shader,
                entry_point: Some("vs_main"),
                buffers: &[],
                compilation_options: Default::default(),
            },
            fragment: Some(wgpu::FragmentState {
                module: shader,
                entry_point: Some(entry_point),
                targets: &[Some(wgpu::ColorTargetState {
                    format,
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
        })
    }

    fn create_bloom_texture(device: &Device, width: u32, height: u32) -> BloomTexture {
        let texture = device.create_texture(&TextureDescriptor {
            label: Some("Bloom Texture"),
            size: Extent3d { width, height, depth_or_array_layers: 1 },
            mip_level_count: 1,
            sample_count: 1,
            dimension: TextureDimension::D2,
            format: TextureFormat::Rgba16Float,
            usage: TextureUsages::RENDER_ATTACHMENT | TextureUsages::TEXTURE_BINDING,
            view_formats: &[],
        });
        let view = texture.create_view(&wgpu::TextureViewDescriptor::default());
        let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("Bloom Sampler"),
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            ..Default::default()
        });
        BloomTexture { texture, view, sampler, width, height }
    }

    /// Get the bind group layout for single texture passes.
    pub fn bind_group_layout(&self) -> &wgpu::BindGroupLayout {
        &self.bind_group_layout
    }

    /// Get the combine bind group layout.
    pub fn combine_bind_group_layout(&self) -> &wgpu::BindGroupLayout {
        &self.combine_bind_group_layout
    }

    /// Render the full bloom pipeline.
    pub fn render(
        &self,
        encoder: &mut wgpu::CommandEncoder,
        input_bind_group: &wgpu::BindGroup,
        _output_view: &wgpu::TextureView,
    ) {
        if self.textures.is_empty() {
            return;
        }

        // Step 1: Extract bright pixels into first bloom texture
        {
            let mut rpass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("Bloom Threshold"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &self.textures[0].view,
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
            rpass.set_pipeline(&self.threshold_pipeline);
            rpass.set_bind_group(0, input_bind_group, &[]);
            rpass.draw(0..3, 0..1);
        }

        // Step 2: Blur chain - each level blurs from previous
        for i in 0..self.bind_groups.len() - 1 {
            let mut rpass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some(&format!("Bloom Blur {}", i)),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &self.textures[i + 1].view,
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
            rpass.set_pipeline(&self.blur_pipeline);
            rpass.set_bind_group(0, &self.bind_groups[i], &[]);
            rpass.draw(0..3, 0..1);
        }

        // Step 3: Upsample back (additive blend)
        for i in (1..self.bind_groups.len()).rev() {
            let mut rpass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some(&format!("Bloom Upsample {}", i)),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &self.textures[i - 1].view,
                    resolve_target: None,
                    depth_slice: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Load,
                        store: wgpu::StoreOp::Store,
                    },
                })],
                depth_stencil_attachment: None,
                timestamp_writes: None,
                occlusion_query_set: None,
                multiview_mask: None,
            }).forget_lifetime();
            rpass.set_pipeline(&self.blur_pipeline);
            rpass.set_bind_group(0, &self.bind_groups[i], &[]);
            rpass.draw(0..3, 0..1);
        }

        // Step 4: The bloom result is now in self.textures[0]
        // It will be combined with the original in the tonemap pass
    }

    /// Get the bloom result texture view for combining with original.
    pub fn bloom_view(&self) -> Option<&wgpu::TextureView> {
        self.textures.first().map(|t| &t.view)
    }

    /// Get the bloom result sampler.
    pub fn bloom_sampler(&self) -> Option<&wgpu::Sampler> {
        self.textures.first().map(|t| &t.sampler)
    }
}
