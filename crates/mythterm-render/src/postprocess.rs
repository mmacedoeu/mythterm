//! Post-processing pipeline for cinematic terminal rendering.
//!
//! Applies bloom, LCD subpixel simulation, and filmic tonemapping
//! to the terminal content rendered into the HDR render target.

use wgpu::{Device, RenderPipeline, BindGroupLayout};

/// Post-processing renderer.
pub struct PostProcess {
    /// Bloom threshold extraction pipeline.
    bloom_threshold_pipeline: RenderPipeline,
    /// Bloom blur pipeline.
    bloom_blur_pipeline: RenderPipeline,
    /// LCD subpixel pipeline.
    lcd_pipeline: RenderPipeline,
    /// Tonemap pipeline.
    tonemap_pipeline: RenderPipeline,
    /// Bind group layout for input textures.
    bind_group_layout: BindGroupLayout,
}

impl PostProcess {
    /// Create a new post-processing pipeline.
    pub fn new(device: &Device, format: wgpu::TextureFormat) -> Self {
        // Create bind group layout for texture + sampler
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

        // Create shader module
        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("PostProcess Shader"),
            source: wgpu::ShaderSource::Wgsl(include_str!("postprocess.wgsl").into()),
        });

        // Create pipelines for each pass
        let bloom_threshold_pipeline = Self::create_pipeline(
            device, &pipeline_layout, &shader, format, "bloom_threshold_fs",
        );
        let bloom_blur_pipeline = Self::create_pipeline(
            device, &pipeline_layout, &shader, format, "bloom_blur_fs",
        );
        let lcd_pipeline = Self::create_pipeline(
            device, &pipeline_layout, &shader, format, "lcd_fs",
        );
        let tonemap_pipeline = Self::create_pipeline(
            device, &pipeline_layout, &shader, format, "tonemap_fs",
        );

        Self {
            bloom_threshold_pipeline,
            bloom_blur_pipeline,
            lcd_pipeline,
            tonemap_pipeline,
            bind_group_layout,
        }
    }

    fn create_pipeline(
        device: &Device,
        layout: &wgpu::PipelineLayout,
        shader: &wgpu::ShaderModule,
        format: wgpu::TextureFormat,
        entry_point: &str,
    ) -> RenderPipeline {
        device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some(&format!("PostProcess Pipeline: {}", entry_point)),
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

    /// Get the bind group layout.
    pub fn bind_group_layout(&self) -> &BindGroupLayout {
        &self.bind_group_layout
    }

    /// Render a specific post-processing pass.
    pub fn render(
        &self,
        encoder: &mut wgpu::CommandEncoder,
        input_bind_group: &wgpu::BindGroup,
        output_view: &wgpu::TextureView,
        pass: PostPass,
    ) {
        let pipeline = match pass {
            PostPass::BloomThreshold => &self.bloom_threshold_pipeline,
            PostPass::BloomBlur => &self.bloom_blur_pipeline,
            PostPass::Lcd => &self.lcd_pipeline,
            PostPass::Tonemap => &self.tonemap_pipeline,
        };

        let mut rpass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some(&format!("PostProcess: {:?}", pass)),
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

        rpass.set_pipeline(pipeline);
        rpass.set_bind_group(0, input_bind_group, &[]);
        rpass.draw(0..3, 0..1); // Full-screen triangle
    }
}

/// Post-processing pass type.
#[derive(Debug, Clone, Copy)]
pub enum PostPass {
    /// Bloom: extract bright pixels.
    BloomThreshold,
    /// Bloom: Gaussian blur.
    BloomBlur,
    /// LCD subpixel simulation.
    Lcd,
    /// Filmic tonemapping.
    Tonemap,
}
