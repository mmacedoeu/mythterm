//! SDF Shader-Driven Chrome Test (Phase 1)
//!
//! Renders a single quad with a WGSL SDF shader that computes the
//! signed distance to a rounded rectangle and outputs fill + border
//! + glow. All visuals are in the fragment shader — no CPU painting.
//!
//! Run with --step N to test different SDF configurations:
//!   1: Solid fill (SDF validation)
//!   2: Fill + border
//!   3: Fill + border + glow
//!   4: Goal tab (sharp corners, navy fill, cyan border, horizontal
//!      brightness gradient)

use std::sync::Arc;
use winit::{
    application::ApplicationHandler,
    event::WindowEvent,
    event_loop::{ActiveEventLoop, EventLoop},
    window::{Window, WindowId},
};

use wgsl_sdf::{png::write_png_rgba, SdfParams, SHADER_SRC};

#[derive(Copy, Clone, Debug)]
struct Rect {
    min: [f32; 2],
    max: [f32; 2],
}

impl Rect {
    fn from_min_size(min: [f32; 2], size: [f32; 2]) -> Self {
        Self { min, max: [min[0] + size[0], min[1] + size[1]] }
    }
    fn width(&self) -> f32 { self.max[0] - self.min[0] }
    fn height(&self) -> f32 { self.max[1] - self.min[1] }
}

/// Build the goal active-tab style from the goal mockup.
fn goal_active_tab(rect: Rect) -> SdfParams {
    let mut s = SdfParams::zeroed();
    let r = [rect.min[0], rect.min[1], rect.width(), rect.height()];
    s.rect(r);
    s.fill([22.0/255.0, 44.0/255.0, 62.0/255.0, 1.0]);
    s.top_peak([45.0/255.0, 122.0/255.0, 161.0/255.0, 1.0]);
    s.top_dim([10.0/255.0, 32.0/255.0, 50.0/255.0, 1.0]);
    s.bot_peak([28.0/255.0, 135.0/255.0, 185.0/255.0, 1.0]);
    s.bot_dim([10.0/255.0, 30.0/255.0, 50.0/255.0, 1.0]);
    s.grad_left([37.0/255.0, 100.0/255.0, 136.0/255.0, 1.0]);
    s.grad_right([32.0/255.0, 93.0/255.0, 128.0/255.0, 1.0]);
    // corner_radius, top_off, top_in, top_out,
    // bot_off,     bot_in, bot_out, grad_peak
    s.scalars(0.0, 0.0, 3.0, 1.0, -7.0, 2.0, 2.0, 0.45);
    s
}

fn solid_fill(rect: Rect) -> SdfParams {
    let mut s = SdfParams::zeroed();
    let r = [rect.min[0], rect.min[1], rect.width(), rect.height()];
    s.rect(r);
    s.fill([22.0/255.0, 44.0/255.0, 62.0/255.0, 1.0]);
    s
}


struct App {
    window: Option<Arc<Window>>,
    device: Option<wgpu::Device>,
    queue: Option<wgpu::Queue>,
    surface: Option<wgpu::Surface<'static>>,
    surface_config: Option<wgpu::SurfaceConfiguration>,
    render_pipeline: Option<wgpu::RenderPipeline>,
    vertex_buffer: Option<wgpu::Buffer>,
    params_buffer: Option<wgpu::Buffer>,
    bind_group: Option<wgpu::BindGroup>,
    bind_group_layout: Option<wgpu::BindGroupLayout>,
    pipeline_layout: Option<wgpu::PipelineLayout>,
    step: u32,
    size: (u32, u32),
    /// Auto-snapshot N frames after start (headless/CI runs).
    auto_snapshot_at: Option<u32>,
    frame_count: u32,
}

impl App {
    fn new(step: u32) -> Self {
        Self {
            window: None,
            device: None,
            queue: None,
            surface: None,
            surface_config: None,
            render_pipeline: None,
            vertex_buffer: None,
            params_buffer: None,
            bind_group: None,
            bind_group_layout: None,
            pipeline_layout: None,
            step,
            size: (800, 600),
            auto_snapshot_at: parse_auto_snapshot(),
            frame_count: 0,
        }
    }
}

impl ApplicationHandler for App {
    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        if self.window.is_some() {
            return;
        }
        let window = Arc::new(
            event_loop
                .create_window(
                    Window::default_attributes()
                        .with_title(format!("sdf-test step={}", self.step))
                        .with_inner_size(winit::dpi::LogicalSize::new(800.0, 600.0))
                        .with_decorations(false),
                )
                .expect("Failed to create window"),
        );

        let instance = wgpu::Instance::new(wgpu::InstanceDescriptor {
            backends: wgpu::Backends::all(),
            flags: wgpu::InstanceFlags::default(),
            backend_options: wgpu::BackendOptions::default(),
            memory_budget_thresholds: wgpu::MemoryBudgetThresholds::default(),
            display: None,
        });
        let surface = instance
            .create_surface(window.clone())
            .expect("Failed to create surface");

        let (device, queue, surface_config) = pollster::block_on(async {
            let adapter = instance
                .request_adapter(&wgpu::RequestAdapterOptions {
                    power_preference: wgpu::PowerPreference::HighPerformance,
                    compatible_surface: Some(&surface),
                    force_fallback_adapter: false,
                })
                .await
                .expect("Failed to find adapter");

            let (device, queue) = adapter
                .request_device(
                    &wgpu::DeviceDescriptor {
                        label: Some("sdf-test"),
                        ..Default::default()
                    },
                )
                .await
                .expect("Failed to create device");

            let size = window.inner_size();
            let caps = surface.get_capabilities(&adapter);
            let format = caps.formats[0];
            let config = wgpu::SurfaceConfiguration {
                usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
                format,
                width: size.width.max(1),
                height: size.height.max(1),
                present_mode: wgpu::PresentMode::AutoVsync,
                alpha_mode: wgpu::CompositeAlphaMode::Auto,
                view_formats: vec![],
                desired_maximum_frame_latency: 2,
            };
            surface.configure(&device, &config);
            (device, queue, config)
        });

        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("sdf-shader"),
            source: wgpu::ShaderSource::Wgsl(SHADER_SRC.into()),
        });

        // Tab rect in window coords (centered, 300x60)
        let tab_rect = Rect::from_min_size([100.0, 100.0], [600.0, 400.0]);

        let params = match self.step {
            1 => solid_fill(tab_rect),
            _ => goal_active_tab(tab_rect),
        };
        let params_bytes = bytemuck::bytes_of(&params);
        eprintln!("DEBUG: sizeof SdfParams = {}", std::mem::size_of::<SdfParams>());

        let params_buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("params"),
            contents: params_bytes,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
        });

        // Quad covering the tab area + glow margin
        let margin = 8.0;
        let x0 = tab_rect.min[0] - margin;
        let y0 = tab_rect.min[1] - margin;
        let x1 = tab_rect.max[0] + margin;
        let y1 = tab_rect.max[1] + margin;
        let vertices: [[f32; 2]; 6] = [
            [x0, y0], [x1, y0], [x1, y1],
            [x0, y0], [x1, y1], [x0, y1],
        ];
        let vertex_buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("quad"),
            contents: bytemuck::cast_slice(&vertices),
            usage: wgpu::BufferUsages::VERTEX,
        });

        let bind_group_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("sdf-bind-group-layout"),
            entries: &[wgpu::BindGroupLayoutEntry {
                binding: 0,
                visibility: wgpu::ShaderStages::FRAGMENT | wgpu::ShaderStages::VERTEX,
                ty: wgpu::BindingType::Buffer {
                    ty: wgpu::BufferBindingType::Uniform,
                    has_dynamic_offset: false,
                    min_binding_size: None,
                },
                count: None,
            }],
        });

        let bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("sdf-bind-group"),
            layout: &bind_group_layout,
            entries: &[wgpu::BindGroupEntry {
                binding: 0,
                resource: params_buffer.as_entire_binding(),
            }],
        });

        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("sdf-pipeline-layout"),
            bind_group_layouts: &[Some(&bind_group_layout)],
            immediate_size: 0,
        });

        let render_pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("sdf-pipeline"),
            layout: Some(&pipeline_layout),
            vertex: wgpu::VertexState {
                module: &shader,
                entry_point: Some("vs_main"),
                buffers: &[wgpu::VertexBufferLayout {
                    array_stride: 8,
                    step_mode: wgpu::VertexStepMode::Vertex,
                    attributes: &wgpu::vertex_attr_array![0 => Float32x2],
                }],
                compilation_options: wgpu::PipelineCompilationOptions::default(),
            },
            fragment: Some(wgpu::FragmentState {
                module: &shader,
                entry_point: Some("fs_main"),
                targets: &[Some(wgpu::ColorTargetState {
                    format: surface_config.format,
                    blend: Some(wgpu::BlendState::ALPHA_BLENDING),
                    write_mask: wgpu::ColorWrites::ALL,
                })],
                compilation_options: wgpu::PipelineCompilationOptions::default(),
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

        self.size = (surface_config.width, surface_config.height);
        self.window = Some(window);
        self.device = Some(device);
        self.queue = Some(queue);
        self.surface = Some(surface);
        self.surface_config = Some(surface_config);
        self.render_pipeline = Some(render_pipeline);
        self.vertex_buffer = Some(vertex_buffer);
        self.params_buffer = Some(params_buffer);
        self.bind_group = Some(bind_group);
        self.bind_group_layout = Some(bind_group_layout);
        self.pipeline_layout = Some(pipeline_layout);
    }

    fn window_event(&mut self, event_loop: &ActiveEventLoop, _id: WindowId, event: WindowEvent) {
        match event {
            WindowEvent::CloseRequested => event_loop.exit(),
            WindowEvent::RedrawRequested => {
                let should_exit = self.render();
                if should_exit {
                    event_loop.exit();
                } else if let Some(window) = &self.window {
                    window.request_redraw();
                }
            }
            _ => {}
        }
    }
}

impl App {
    /// Returns true if the event loop should exit (i.e. an
    /// auto-snapshot was just written).
    fn render(&mut self) -> bool {
        let _window = self.window.as_ref().unwrap();
        let device = self.device.as_ref().unwrap();
        let queue = self.queue.as_ref().unwrap();
        let surface = self.surface.as_ref().unwrap();

        let output = surface.get_current_texture();
        let surface_texture = match output {
            wgpu::CurrentSurfaceTexture::Success(t) | wgpu::CurrentSurfaceTexture::Suboptimal(t) => t,
            other => {
                log::warn!("Surface texture unavailable: {:?}", other);
                return false;
            }
        };
        let view = surface_texture.texture.create_view(&Default::default());

        let mut encoder = device.create_command_encoder(&Default::default());
        {
            let mut rpass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("sdf-test"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &view,
                    resolve_target: None,
                    depth_slice: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(wgpu::Color {
                            r: 0.0, g: 0.0, b: 0.0, a: 1.0,
                        }),
                        store: wgpu::StoreOp::Store,
                    },
                })],
                depth_stencil_attachment: None,
                timestamp_writes: None,
                occlusion_query_set: None,
                multiview_mask: None,
            }).forget_lifetime();

            rpass.set_pipeline(self.render_pipeline.as_ref().unwrap());
            rpass.set_bind_group(0, self.bind_group.as_ref().unwrap(), &[]);
            rpass.set_vertex_buffer(0, self.vertex_buffer.as_ref().unwrap().slice(..));
            rpass.draw(0..6, 0..1);
        }

        queue.submit(std::iter::once(encoder.finish()));
        surface_texture.present();

        // Auto-snapshot: render the same scene into a non-sRGB
        // Rgba8Unorm texture, read it back, and write a PNG.
        // The swapchain format is sRGB-encoded so reading from
        // it directly would give the wrong color values.
        if let Some(target) = self.auto_snapshot_at {
            if self.frame_count == target {
                self.write_snapshot();
                return true;
            }
        }

        self.frame_count += 1;
        false
    }

    fn write_snapshot(&self) {
        let device = self.device.as_ref().unwrap();
        let queue = self.queue.as_ref().unwrap();
        let cfg = self.surface_config.as_ref().unwrap();
        let w = cfg.width;
        let h = cfg.height;
        let bytes_per_pixel = 4u32;
        let row_size = w * bytes_per_pixel;
        let padded_row = (row_size + wgpu::COPY_BYTES_PER_ROW_ALIGNMENT - 1)
            & !(wgpu::COPY_BYTES_PER_ROW_ALIGNMENT - 1);
        let buf_size = (padded_row * h) as u64;

        let read_buf = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("snapshot-read"),
            size: buf_size,
            usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
            mapped_at_creation: false,
        });

        // Render into a non-sRGB Rgba8Unorm texture so the readback
        // bytes match the sRGB-encoded goal PNG exactly.
        let snapshot_format = wgpu::TextureFormat::Rgba8Unorm;
        let offscreen_tex = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("snapshot-tex"),
            size: wgpu::Extent3d { width: w, height: h, depth_or_array_layers: 1 },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: snapshot_format,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT
                | wgpu::TextureUsages::COPY_SRC,
            view_formats: &[],
        });
        let view = offscreen_tex.create_view(&Default::default());

        // Snapshot pipeline: same shader, but the same bind group
        // layout (the WGSL has a module-scope `params` uniform).
        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("sdf-snapshot-shader"),
            source: wgpu::ShaderSource::Wgsl(SHADER_SRC.into()),
        });
        let snapshot_pipeline = device.create_render_pipeline(
            &wgpu::RenderPipelineDescriptor {
                label: Some("sdf-snapshot-pipeline"),
                layout: Some(self.pipeline_layout.as_ref().unwrap()),
                vertex: wgpu::VertexState {
                    module: &shader,
                    entry_point: Some("vs_main"),
                    buffers: &[wgpu::VertexBufferLayout {
                        array_stride: 8,
                        step_mode: wgpu::VertexStepMode::Vertex,
                        attributes: &wgpu::vertex_attr_array![0 => Float32x2],
                    }],
                    compilation_options: wgpu::PipelineCompilationOptions::default(),
                },
                fragment: Some(wgpu::FragmentState {
                    module: &shader,
                    entry_point: Some("fs_main"),
                    targets: &[Some(wgpu::ColorTargetState {
                        format: snapshot_format,
                        blend: Some(wgpu::BlendState::ALPHA_BLENDING),
                        write_mask: wgpu::ColorWrites::ALL,
                    })],
                    compilation_options: wgpu::PipelineCompilationOptions::default(),
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
            },
        );

        let mut encoder = device.create_command_encoder(&Default::default());
        {
            let mut rpass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("sdf-snapshot-pass"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &view,
                    resolve_target: None,
                    depth_slice: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(wgpu::Color {
                            r: 0.0, g: 0.0, b: 0.0, a: 1.0,
                        }),
                        store: wgpu::StoreOp::Store,
                    },
                })],
                depth_stencil_attachment: None,
                timestamp_writes: None,
                occlusion_query_set: None,
                multiview_mask: None,
            })
            .forget_lifetime();
            rpass.set_pipeline(&snapshot_pipeline);
            rpass.set_bind_group(0, self.bind_group.as_ref().unwrap(), &[]);
            rpass.set_vertex_buffer(0, self.vertex_buffer.as_ref().unwrap().slice(..));
            rpass.draw(0..6, 0..1);
        }
        encoder.copy_texture_to_buffer(
            wgpu::TexelCopyTextureInfo {
                texture: &offscreen_tex,
                mip_level: 0,
                origin: wgpu::Origin3d::ZERO,
                aspect: wgpu::TextureAspect::All,
            },
            wgpu::TexelCopyBufferInfo {
                buffer: &read_buf,
                layout: wgpu::TexelCopyBufferLayout {
                    offset: 0,
                    bytes_per_row: Some(padded_row),
                    rows_per_image: Some(h),
                },
            },
            wgpu::Extent3d { width: w, height: h, depth_or_array_layers: 1 },
        );
        queue.submit(std::iter::once(encoder.finish()));

        let slice = read_buf.slice(..);
        slice.map_async(wgpu::MapMode::Read, |_| {});
        let _ = device.poll(wgpu::PollType::wait_indefinitely());

        let mapped = slice.get_mapped_range();
        let path = format!("target/snapshots/sdf-test_{}.png", self.step);
        std::fs::create_dir_all("target/snapshots").ok();
        match write_png_rgba(&path, w, h, padded_row, &mapped) {
            Ok(()) => eprintln!("[snapshot] wrote {}", path),
            Err(e) => eprintln!("[snapshot] failed: {}", e),
        }
        drop(mapped);
        read_buf.unmap();
    }
}

use wgpu::util::DeviceExt;

fn parse_step() -> u32 {
    let args: Vec<String> = std::env::args().collect();
    for arg in args.iter() {
        if let Some(val) = arg.strip_prefix("--step=") {
            return val.parse().unwrap_or(1);
        }
    }
    1
}

fn parse_auto_snapshot() -> Option<u32> {
    let args: Vec<String> = std::env::args().collect();
    for arg in args.iter() {
        if let Some(val) = arg.strip_prefix("--snapshot-at=") {
            return val.parse().ok();
        }
    }
    None
}

fn main() -> anyhow::Result<()> {
    env_logger::init();
    let step = parse_step();
    log::info!("sdf-test step: {}", step);
    let event_loop = EventLoop::new()?;
    let mut app = App::new(step);
    event_loop.run_app(&mut app)?;
    Ok(())
}
