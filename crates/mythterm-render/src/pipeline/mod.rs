//! Terminal render pipeline: WGSL shaders for terminal content rendering.
//!
//! Provides the GPU pipeline for rendering terminal cells as textured quads.

use wgpu::util::DeviceExt;

/// Vertex type for terminal quads.
#[repr(C)]
#[derive(Copy, Clone, Debug, bytemuck::Pod, bytemuck::Zeroable)]
pub struct QuadVertex {
    /// Position in clip space.
    pub position: [f32; 2],
    /// UV coordinates in the atlas texture.
    pub uv: [f32; 2],
    /// Foreground color (RGBA).
    pub fg_color: [f32; 4],
    /// Background color (RGBA).
    pub bg_color: [f32; 4],
}

impl QuadVertex {
    pub fn desc() -> wgpu::VertexBufferLayout<'static> {
        wgpu::VertexBufferLayout {
            array_stride: std::mem::size_of::<QuadVertex>() as wgpu::BufferAddress,
            step_mode: wgpu::VertexStepMode::Vertex,
            attributes: &[
                // position
                wgpu::VertexAttribute {
                    offset: 0,
                    shader_location: 0,
                    format: wgpu::VertexFormat::Float32x2,
                },
                // uv
                wgpu::VertexAttribute {
                    offset: std::mem::size_of::<[f32; 2]>() as wgpu::BufferAddress,
                    shader_location: 1,
                    format: wgpu::VertexFormat::Float32x2,
                },
                // fg_color
                wgpu::VertexAttribute {
                    offset: std::mem::size_of::<[f32; 4]>() as wgpu::BufferAddress,
                    shader_location: 2,
                    format: wgpu::VertexFormat::Float32x4,
                },
                // bg_color
                wgpu::VertexAttribute {
                    offset: std::mem::size_of::<[f32; 8]>() as wgpu::BufferAddress,
                    shader_location: 3,
                    format: wgpu::VertexFormat::Float32x4,
                },
            ],
        }
    }
}

/// The terminal rendering pipeline.
pub struct TerminalPipeline {
    /// The render pipeline.
    pipeline: wgpu::RenderPipeline,
    /// Vertex buffer for quads.
    vertex_buffer: wgpu::Buffer,
    /// Index buffer for quads.
    index_buffer: wgpu::Buffer,
    /// Number of indices to draw.
    num_indices: u32,
    /// Uniform buffer for screen dimensions.
    uniform_buffer: wgpu::Buffer,
    /// Uniform bind group.
    uniform_bind_group: wgpu::BindGroup,
}

/// Uniform data for the terminal shader.
#[repr(C)]
#[derive(Copy, Clone, Debug, bytemuck::Pod, bytemuck::Zeroable)]
pub struct TerminalUniforms {
    /// Screen width in pixels.
    pub screen_width: f32,
    /// Screen height in pixels.
    pub screen_height: f32,
    /// Padding.
    pub _padding: [f32; 2],
}

impl TerminalPipeline {
    /// Create a new terminal pipeline.
    pub fn new(
        device: &wgpu::Device,
        format: wgpu::TextureFormat,
        atlas_bind_group_layout: &wgpu::BindGroupLayout,
    ) -> Self {
        // Create uniform buffer
        let uniform_data = TerminalUniforms {
            screen_width: 800.0,
            screen_height: 600.0,
            _padding: [0.0; 2],
        };

        let uniform_buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("Terminal Uniform Buffer"),
            contents: bytemuck::cast_slice(&[uniform_data]),
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
        });

        // Create uniform bind group layout
        let uniform_bind_group_layout =
            device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
                label: Some("Terminal Uniform Bind Group Layout"),
                entries: &[wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::VERTEX | wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Uniform,
                        has_dynamic_offset: false,
                        min_binding_size: None,
                    },
                    count: None,
                }],
            });

        let uniform_bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("Terminal Uniform Bind Group"),
            layout: &uniform_bind_group_layout,
            entries: &[wgpu::BindGroupEntry {
                binding: 0,
                resource: uniform_buffer.as_entire_binding(),
            }],
        });

        // Create shader module
        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("Terminal Shader"),
            source: wgpu::ShaderSource::Wgsl(include_str!("terminal.wgsl").into()),
        });

        // Create pipeline layout
        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("Terminal Pipeline Layout"),
            bind_group_layouts: &[Some(&uniform_bind_group_layout), Some(atlas_bind_group_layout)],
            immediate_size: 0,
        });

        // Create render pipeline
        let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("Terminal Pipeline"),
            layout: Some(&pipeline_layout),
            vertex: wgpu::VertexState {
                module: &shader,
                entry_point: Some("vs_main"),
                buffers: &[QuadVertex::desc()],
                compilation_options: Default::default(),
            },
            fragment: Some(wgpu::FragmentState {
                module: &shader,
                entry_point: Some("fs_main"),
                targets: &[Some(wgpu::ColorTargetState {
                    format,
                    blend: Some(wgpu::BlendState::ALPHA_BLENDING),
                    write_mask: wgpu::ColorWrites::ALL,
                })],
                compilation_options: Default::default(),
            }),
            primitive: wgpu::PrimitiveState {
                topology: wgpu::PrimitiveTopology::TriangleList,
                strip_index_format: None,
                front_face: wgpu::FrontFace::Ccw,
                cull_mode: None,
                polygon_mode: wgpu::PolygonMode::Fill,
                unclipped_depth: false,
                conservative: false,
            },
            depth_stencil: None,
            multisample: wgpu::MultisampleState::default(),
            multiview_mask: None,
            cache: None,
        });

        // Create empty vertex/index buffers (will be updated each frame)
        let vertex_buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("Terminal Vertex Buffer"),
            size: 1024 * 1024, // 1MB initial
            usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });

        let index_buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("Terminal Index Buffer"),
            size: 256 * 1024, // 256KB initial
            usage: wgpu::BufferUsages::INDEX | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });

        Self {
            pipeline,
            vertex_buffer,
            index_buffer,
            num_indices: 0,
            uniform_buffer,
            uniform_bind_group,
        }
    }

    /// Update the vertex and index buffers with new quad data.
    pub fn update_buffers(&mut self, queue: &wgpu::Queue, vertices: &[QuadVertex], indices: &[u32]) {
        if vertices.is_empty() {
            self.num_indices = 0;
            return;
        }

        queue.write_buffer(
            &self.vertex_buffer,
            0,
            bytemuck::cast_slice(vertices),
        );

        queue.write_buffer(
            &self.index_buffer,
            0,
            bytemuck::cast_slice(indices),
        );

        self.num_indices = indices.len() as u32;
    }

    /// Update screen dimensions in the uniform buffer.
    pub fn update_screen_size(&self, queue: &wgpu::Queue, width: f32, height: f32) {
        let uniforms = TerminalUniforms {
            screen_width: width,
            screen_height: height,
            _padding: [0.0; 2],
        };
        queue.write_buffer(&self.uniform_buffer, 0, bytemuck::cast_slice(&[uniforms]));
    }

    /// Render the terminal content.
    pub fn render<'a>(&'a self, render_pass: &mut wgpu::RenderPass<'a>, atlas_bind_group: &'a wgpu::BindGroup) {
        if self.num_indices == 0 {
            return;
        }

        render_pass.set_pipeline(&self.pipeline);
        render_pass.set_bind_group(0, &self.uniform_bind_group, &[]);
        render_pass.set_bind_group(1, atlas_bind_group, &[]);
        render_pass.set_vertex_buffer(0, self.vertex_buffer.slice(..));
        render_pass.set_index_buffer(self.index_buffer.slice(..), wgpu::IndexFormat::Uint32);
        render_pass.draw_indexed(0..self.num_indices, 0, 0..1);
    }
}
