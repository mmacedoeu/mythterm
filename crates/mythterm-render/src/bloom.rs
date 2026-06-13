//! Bloom renderer with blur chain.
//!
//! Implements a multi-pass bloom effect using intermediate textures
//! at different resolutions (1/2, 1/4, 1/8) followed by a final
//! combine pass that adds the bloom back onto the original HDR input.

use wgpu::{Device, Extent3d, TextureDescriptor, TextureDimension, TextureFormat, TextureUsages};

/// Intermediate texture for bloom blur passes.
///
/// `texture`, `width`, and `height` are kept alive for the lifetime
/// of the `view` (the texture must outlive the view that references
/// it). They are not read directly.
struct BloomTexture {
    #[allow(dead_code)]
    texture: wgpu::Texture,
    view: wgpu::TextureView,
    sampler: wgpu::Sampler,
    #[allow(dead_code)]
    width: u32,
    #[allow(dead_code)]
    height: u32,
}

/// Bloom renderer with multi-resolution blur chain.
///
/// Owns the mip-chain of HDR textures and the render pipelines needed
/// to run the threshold + blur + upsample + combine sequence. Bind
/// groups are constructed on the fly during `render()` (cheap) using
/// the caller-provided device.
pub struct BloomRenderer {
    /// Threshold extraction pipeline.
    threshold_pipeline: wgpu::RenderPipeline,
    /// Blur pipeline (used for each mip level).
    blur_pipeline: wgpu::RenderPipeline,
    /// Combine pipeline.
    combine_pipeline: wgpu::RenderPipeline,
    /// Intermediate textures at different resolutions.
    textures: Vec<BloomTexture>,
    /// Bind group layout for single texture + sampler passes.
    bind_group_layout: wgpu::BindGroupLayout,
    /// Combine bind group layout (needs 2 textures + 2 samplers).
    combine_bind_group_layout: wgpu::BindGroupLayout,
    /// Cached bind groups for each intermediate texture.
    bind_groups: Vec<wgpu::BindGroup>,
    /// Current target width (window size, not downsampled).
    width: u32,
    /// Current target height.
    height: u32,
}

impl BloomRenderer {
    /// Create a new bloom renderer.
    ///
    /// All bloom passes write to HDR textures (`Rgba16Float`); the
    /// renderer never touches the swapchain. A downstream post-process
    /// pass (see `PostProcess`) is expected to tonemap the bloom
    /// output to the swapchain format.
    pub fn new(device: &Device, width: u32, height: u32) -> Self {
        let (bind_group_layout, combine_bind_group_layout, threshold_pipeline, blur_pipeline, combine_pipeline) =
            Self::create_pipelines(device);
        let textures = Self::create_textures(device, width, height);
        let bind_groups = Self::create_texture_bind_groups(device, &bind_group_layout, &textures);

        Self {
            threshold_pipeline,
            blur_pipeline,
            combine_pipeline,
            textures,
            bind_group_layout,
            combine_bind_group_layout,
            bind_groups,
            width,
            height,
        }
    }

    /// Resize the bloom renderer's intermediate textures.
    ///
    /// Recreates the mip chain to match the new dimensions. Pipelines
    /// and bind group layouts are reused.
    pub fn resize(&mut self, device: &Device, width: u32, height: u32) {
        if self.width == width && self.height == height {
            return;
        }
        self.width = width;
        self.height = height;
        self.textures = Self::create_textures(device, width, height);
        self.bind_groups = Self::create_texture_bind_groups(device, &self.bind_group_layout, &self.textures);
    }

    fn create_pipelines(
        device: &Device,
    ) -> (
        wgpu::BindGroupLayout,
        wgpu::BindGroupLayout,
        wgpu::RenderPipeline,
        wgpu::RenderPipeline,
        wgpu::RenderPipeline,
    ) {
        // Bind group layout for single texture + sampler (threshold + blur passes).
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

        // Bind group layout for combine (original + bloom).
        let combine_bind_group_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("Bloom Combine Bind Group Layout"),
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

        // Threshold + blur write to the HDR mip chain (Rgba16Float).
        // The combine pass also writes to an HDR intermediate
        // (Rgba16Float) so a downstream tonemap pass can finish the
        // pipeline. The bloom renderer never touches the swapchain.
        let threshold_pipeline = Self::create_pipeline(device, &pipeline_layout, &shader, TextureFormat::Rgba16Float, "bloom_threshold_fs");
        let blur_pipeline = Self::create_pipeline(device, &pipeline_layout, &shader, TextureFormat::Rgba16Float, "bloom_blur_fs");
        let combine_pipeline = Self::create_pipeline(device, &combine_pipeline_layout, &shader, TextureFormat::Rgba16Float, "bloom_combine_fs");

        (
            bind_group_layout,
            combine_bind_group_layout,
            threshold_pipeline,
            blur_pipeline,
            combine_pipeline,
        )
    }

    fn create_textures(device: &Device, width: u32, height: u32) -> Vec<BloomTexture> {
        // Create intermediate textures at 1/2, 1/4, 1/8 resolution
        let mut textures = Vec::new();
        let mut w = (width / 2).max(1);
        let mut h = (height / 2).max(1);
        for _i in 0..3 {
            textures.push(Self::create_bloom_texture(device, w, h));
            w = (w / 2).max(1);
            h = (h / 2).max(1);
        }
        textures
    }

    fn create_texture_bind_groups(
        device: &Device,
        layout: &wgpu::BindGroupLayout,
        textures: &[BloomTexture],
    ) -> Vec<wgpu::BindGroup> {
        textures
            .iter()
            .map(|tex| {
                device.create_bind_group(&wgpu::BindGroupDescriptor {
                    label: Some("Bloom Texture Bind Group"),
                    layout,
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
                })
            })
            .collect()
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

    /// Get the current target width.
    pub fn width(&self) -> u32 {
        self.width
    }

    /// Get the current target height.
    pub fn height(&self) -> u32 {
        self.height
    }

    /// Render the full bloom pipeline.
    ///
    /// Runs threshold -> downsample blur chain -> upsample -> combine.
    /// The combine step writes the HDR result `original + bloom * intensity`
    /// to `output_view`. The output is always HDR; the bloom renderer
    /// never touches the swapchain. A downstream post-process pass
    /// (see [`PostProcess`]) is expected to tonemap the HDR output
    /// to the swapchain.
    pub fn render(
        &self,
        device: &Device,
        encoder: &mut wgpu::CommandEncoder,
        original_view: &wgpu::TextureView,
        original_sampler: &wgpu::Sampler,
        output_view: &wgpu::TextureView,
    ) {
        if self.textures.is_empty() || self.bind_groups.is_empty() {
            return;
        }

        // Bind group for the original HDR input (threshold pass).
        let input_bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("Bloom Input Bind Group"),
            layout: &self.bind_group_layout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: wgpu::BindingResource::TextureView(original_view),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: wgpu::BindingResource::Sampler(original_sampler),
                },
            ],
        });

        // Step 1: Extract bright pixels into the first (largest) bloom texture.
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
            rpass.set_bind_group(0, &input_bind_group, &[]);
            rpass.draw(0..3, 0..1);
        }

        // Step 2: Downsample blur chain. textures[i] -> textures[i+1].
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

        // Step 3: Upsample chain (additive). textures[i] -> textures[i-1].
        // Final bloom result ends up in textures[0].
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

        // Step 4: Combine the bloom result with the original HDR input.
        let combine_bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("Bloom Combine Bind Group"),
            layout: &self.combine_bind_group_layout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: wgpu::BindingResource::TextureView(original_view),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: wgpu::BindingResource::Sampler(original_sampler),
                },
                wgpu::BindGroupEntry {
                    binding: 2,
                    resource: wgpu::BindingResource::TextureView(&self.textures[0].view),
                },
                wgpu::BindGroupEntry {
                    binding: 3,
                    resource: wgpu::BindingResource::Sampler(&self.textures[0].sampler),
                },
            ],
        });
        {
            let mut rpass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("Bloom Combine"),
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
            rpass.set_pipeline(&self.combine_pipeline);
            rpass.set_bind_group(0, &combine_bind_group, &[]);
            rpass.draw(0..3, 0..1);
        }
    }

    /// Get the bloom result texture view (largest mip, after upsample).
    pub fn bloom_view(&self) -> Option<&wgpu::TextureView> {
        self.textures.first().map(|t| &t.view)
    }

    /// Get the bloom result sampler.
    pub fn bloom_sampler(&self) -> Option<&wgpu::Sampler> {
        self.textures.first().map(|t| &t.sampler)
    }
}
