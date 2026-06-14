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

/// Flat f32 array — guaranteed to match WGSL uniform layout byte-for-byte.
/// Indices match the WGSL struct comment in WGSL_SHADER.
#[repr(C)]
#[derive(Copy, Clone, Debug, bytemuck::Pod, bytemuck::Zeroable)]
struct SdfParams {
    pub data: [f32; 44],
}

impl SdfParams {
    // Field accessors — index must match WGSL
    fn rect(&self) -> [f32; 4] { [self.data[0], self.data[1], self.data[2], self.data[3]] }
    fn fill_color(&self) -> [f32; 4] { [self.data[4], self.data[5], self.data[6], self.data[7]] }
    fn top_peak_color(&self) -> [f32; 4] { [self.data[8], self.data[9], self.data[10], self.data[11]] }
    fn top_dim_color(&self) -> [f32; 4] { [self.data[12], self.data[13], self.data[14], self.data[15]] }
    fn bot_peak_color(&self) -> [f32; 4] { [self.data[16], self.data[17], self.data[18], self.data[19]] }
    fn bot_dim_color(&self) -> [f32; 4] { [self.data[20], self.data[21], self.data[22], self.data[23]] }
    fn gradient_left(&self) -> [f32; 4] { [self.data[24], self.data[25], self.data[26], self.data[27]] }
    fn gradient_right(&self) -> [f32; 4] { [self.data[28], self.data[29], self.data[30], self.data[31]] }
    fn corner_radius(&self) -> f32 { self.data[32] }
    fn top_line_offset(&self) -> f32 { self.data[33] }
    fn top_inner_width(&self) -> f32 { self.data[34] }
    fn top_outer_width(&self) -> f32 { self.data[35] }
    fn bot_line_offset(&self) -> f32 { self.data[36] }
    fn bot_inner_width(&self) -> f32 { self.data[37] }
    fn bot_outer_width(&self) -> f32 { self.data[38] }
    fn gradient_peak(&self) -> f32 { self.data[39] }

    fn goal_active_tab(rect: Rect) -> Self {
        let mut s = SdfParams { data: [0.0; 44] };
        let r = [rect.min[0], rect.min[1], rect.width(), rect.height()];
        s.data[0..4].copy_from_slice(&r);
        s.data[4..8].copy_from_slice(&[22.0/255.0, 44.0/255.0, 62.0/255.0, 1.0]);
        s.data[8..12].copy_from_slice(&[45.0/255.0, 122.0/255.0, 161.0/255.0, 1.0]);
        s.data[12..16].copy_from_slice(&[10.0/255.0, 32.0/255.0, 50.0/255.0, 1.0]);
        s.data[16..20].copy_from_slice(&[28.0/255.0, 135.0/255.0, 185.0/255.0, 1.0]);
        s.data[20..24].copy_from_slice(&[10.0/255.0, 30.0/255.0, 50.0/255.0, 1.0]);
        s.data[24..28].copy_from_slice(&[37.0/255.0, 100.0/255.0, 136.0/255.0, 1.0]);
        s.data[28..32].copy_from_slice(&[32.0/255.0, 93.0/255.0, 128.0/255.0, 1.0]);
        s.data[32] = 0.0;            // corner_radius
        s.data[33] = 0.0;            // top_line_offset
        s.data[34] = 3.0;            // top_inner_width
        s.data[35] = 1.0;            // top_outer_width
        s.data[36] = -7.0;            // bot_line_offset: 7px above the bottom edge
        s.data[37] = 2.0;            // bot_inner_width
        s.data[38] = 2.0;            // bot_outer_width
        s.data[39] = 0.45;           // gradient_peak
        s
    }

    fn solid_fill(rect: Rect) -> Self {
        let r = [rect.min[0], rect.min[1], rect.width(), rect.height()];
        let mut s = SdfParams { data: [0.0; 44] };
        s.data[0..4].copy_from_slice(&r);
        s.data[4..8].copy_from_slice(&[22.0/255.0, 44.0/255.0, 62.0/255.0, 1.0]);
        s
    }
}

const WGSL_SHADER: &str = r#"
struct SdfParams {
    // Uniform address space requires 16-byte alignment, so we use 11 vec4s
    // (= 44 f32s = 176 bytes total) which naga/naga-vk/std140 all agree on.
    // Layout MUST match SdfParams::data in main.rs:
    //   data[0] = rect            (x, y, w, h)
    //   data[1] = fill_color
    //   data[2] = top_peak_color
    //   data[3] = top_dim_color
    //   data[4] = bot_peak_color
    //   data[5] = bot_dim_color
    //   data[6] = gradient_left
    //   data[7] = gradient_right
    //   data[8] = (corner_radius, top_line_offset, top_inner_width, top_outer_width)
    //   data[9] = (bot_line_offset, bot_inner_width, bot_outer_width, gradient_peak)
    //   data[10] = _pad
    data: array<vec4<f32>, 11>,
}

@group(0) @binding(0)
var<uniform> params: SdfParams;

fn p_rect()           -> vec4<f32> { return params.data[0]; }
fn p_fill()           -> vec4<f32> { return params.data[1]; }
fn p_top_peak()       -> vec4<f32> { return params.data[2]; }
fn p_top_dim()        -> vec4<f32> { return params.data[3]; }
fn p_bot_peak()       -> vec4<f32> { return params.data[4]; }
fn p_bot_dim()        -> vec4<f32> { return params.data[5]; }
fn p_grad_left()      -> vec4<f32> { return params.data[6]; }
fn p_grad_right()     -> vec4<f32> { return params.data[7]; }
fn p_corner_radius()  -> f32       { return params.data[8].x; }
fn p_top_off()        -> f32       { return params.data[8].y; }
fn p_top_in()         -> f32       { return params.data[8].z; }
fn p_top_out()        -> f32       { return params.data[8].w; }
fn p_bot_off()        -> f32       { return params.data[9].x; }
fn p_bot_in()         -> f32       { return params.data[9].y; }
fn p_bot_out()        -> f32       { return params.data[9].z; }
fn p_grad_peak()      -> f32       { return params.data[9].w; }

struct VertexOutput {
    @builtin(position) clip_pos: vec4<f32>,
    @location(0) local_pos: vec2<f32>,
    @location(1) uv: vec2<f32>,
}

@vertex
fn vs_main(@location(0) pos: vec2<f32>) -> VertexOutput {
    var out: VertexOutput;
    let r = p_rect();
    out.clip_pos = vec4<f32>(
        (pos.x / 800.0) * 2.0 - 1.0,
        1.0 - (pos.y / 600.0) * 2.0,
        0.0, 1.0
    );
    let center = vec2<f32>(r.x + r.z * 0.5, r.y + r.w * 0.5);
    out.local_pos = pos - center;
    out.uv = vec2<f32>((pos.x - r.x) / r.z, (pos.y - r.y) / r.w);
    return out;
}

fn sd_rounded_box(p: vec2<f32>, b: vec2<f32>, r: f32) -> f32 {
    let q = abs(p) - b + vec2<f32>(r);
    return min(max(q.x, q.y), 0.0) + length(max(q, vec2<f32>(0.0))) - r;
}

fn peak_at(x_t: f32, peak_color: vec4<f32>) -> vec4<f32> {
    let gp = p_grad_peak();
    let denom = max(gp, 1.0 - gp);
    let d_left = abs(x_t - gp) / denom;
    let bell = clamp(1.0 - d_left, 0.0, 1.0);
    let bell_smooth = bell * bell * (3.0 - 2.0 * bell);
    return mix(
        mix(p_grad_left(), peak_color, bell_smooth),
        p_grad_right(),
        smoothstep(gp, 1.0, x_t) * 0.3
    );
}

@fragment
fn fs_main(in: VertexOutput) -> @location(0) vec4<f32> {
    let r = p_rect();
    let half_size = vec2<f32>(r.z * 0.5, r.w * 0.5);
    let d = sd_rounded_box(in.local_pos, half_size, p_corner_radius());

    let margin = max(max(p_top_out(), p_bot_out()), 1.0) + 1.0;
    if d > margin {
        discard;
    }

    let bg_color = vec4<f32>(0.0, 0.0, 0.0, 1.0);
    var color = p_fill();
    if d > 0.0 {
        color = bg_color;
    }

    let t = clamp(in.uv.x, 0.0, 1.0);

    // === TOP BORDER ===
    let top_line_y = -half_size.y + p_top_off();
    let top_d = in.local_pos.y - top_line_y;
    let top_in = p_top_in();
    let top_out = p_top_out();
    if top_in > 0.0 {
        if top_d >= -top_in && top_d <= top_out {
            let top_peak = peak_at(t, p_top_peak());
            if top_d <= 0.0 {
                let dim_d = top_in * 0.33;
                if -top_d <= dim_d {
                    let f = smoothstep(0.0, dim_d, -top_d);
                    color = mix(top_peak, p_top_dim(), f);
                } else {
                    let f = smoothstep(dim_d, top_in, -top_d);
                    color = mix(p_top_dim(), p_fill(), f);
                }
            } else {
                let f = smoothstep(0.0, top_out, top_d);
                color = mix(top_peak, bg_color, f);
            }
        }
    }

    // === BOTTOM BORDER ===
    let bot_line_y = half_size.y + p_bot_off();
    let bot_d = in.local_pos.y - bot_line_y;
    let bot_in = p_bot_in();
    let bot_out = p_bot_out();
    if bot_in > 0.0 {
        if bot_d >= -bot_in && bot_d <= bot_out {
            let bot_peak = peak_at(t, p_bot_peak());
            if bot_d <= 0.0 {
                let dim_d = bot_in * 0.33;
                if -bot_d <= dim_d {
                    let f = smoothstep(0.0, dim_d, -bot_d);
                    color = mix(bot_peak, p_bot_dim(), f);
                } else {
                    let f = smoothstep(dim_d, bot_in, -bot_d);
                    color = mix(p_bot_dim(), p_fill(), f);
                }
            } else {
                let f = smoothstep(0.0, bot_out, bot_d);
                color = mix(bot_peak, bg_color, f);
            }
        }
    }

    return color;
}
"#;

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
    step: u32,
    size: (u32, u32),
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
            step,
            size: (800, 600),
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
            source: wgpu::ShaderSource::Wgsl(WGSL_SHADER.into()),
        });

        // Tab rect in window coords (centered, 300x60)
        let tab_rect = Rect::from_min_size([100.0, 100.0], [600.0, 400.0]);

        let params = match self.step {
            1 => SdfParams::solid_fill(tab_rect),
            _ => SdfParams::goal_active_tab(tab_rect),
        };
        let params_bytes = bytemuck::bytes_of(&params);
        eprintln!("DEBUG: sizeof SdfParams = {}", std::mem::size_of::<SdfParams>());
        eprintln!("DEBUG: top_outer_width = {}, bytes at 140..144 = {:02x?}", params.top_outer_width(), &params_bytes[140..144]);
        eprintln!("DEBUG: top_inner_width = {}, bytes at 136..140 = {:02x?}", params.top_inner_width(), &params_bytes[136..140]);
        eprintln!("DEBUG: gradient_peak = {}, bytes at 156..160 = {:02x?}", params.gradient_peak(), &params_bytes[156..160]);
        eprintln!("DEBUG: top_line_offset = {}, bytes at 132..136 = {:02x?}", params.top_line_offset(), &params_bytes[132..136]);

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
    }

    fn window_event(&mut self, event_loop: &ActiveEventLoop, _id: WindowId, event: WindowEvent) {
        match event {
            WindowEvent::CloseRequested => event_loop.exit(),
            WindowEvent::RedrawRequested => {
                self.render();
                if let Some(window) = &self.window {
                    window.request_redraw();
                }
            }
            _ => {}
        }
    }
}

impl App {
    fn render(&mut self) {
        let window = self.window.as_ref().unwrap();
        let device = self.device.as_ref().unwrap();
        let queue = self.queue.as_ref().unwrap();
        let surface = self.surface.as_ref().unwrap();

        let output = surface.get_current_texture();
        let surface_texture = match output {
            wgpu::CurrentSurfaceTexture::Success(t) | wgpu::CurrentSurfaceTexture::Suboptimal(t) => t,
            other => {
                log::warn!("Surface texture unavailable: {:?}", other);
                return;
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

fn main() -> anyhow::Result<()> {
    env_logger::init();
    let step = parse_step();
    log::info!("sdf-test step: {}", step);
    let event_loop = EventLoop::new()?;
    let mut app = App::new(step);
    event_loop.run_app(&mut app)?;
    Ok(())
}
