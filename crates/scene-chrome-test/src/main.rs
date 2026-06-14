//! scene-chrome-test (Phase 2)
//!
//! Mini scene-graph rendering of tab chrome using the WGSL SDF
//! shader from Phase 1 (`sdf-test`). Each tab is a *Node* — a
//! quad mesh + an SDF material with its own uniform buffer.
//! Egui is used *only* for invisible hit-testing (Steps 3+);
//! the visual output is rendered by the scene graph.
//!
//! Usage:
//!   scene-chrome-test --step N
//!     1: one tab quad, no interaction
//!     2: five tabs (one active), no interaction
//!     3: + egui hit overlay; hover brightens glow uniform
//!     4: + click-to-activate, focus ring, animated transition
//!
//! Key bindings:
//!   S : snapshot the current frame to target/snapshots/scene-chrome-test_<step>.png
//!   1..4 : jump to step
//!   Esc / Q : quit

use std::sync::Arc;
use winit::{
    application::ApplicationHandler,
    event::{ElementState, KeyEvent, WindowEvent},
    event_loop::{ActiveEventLoop, EventLoop},
    keyboard::{Key, NamedKey},
    window::{Window, WindowId},
};

use wgpu::util::DeviceExt;

// -----------------------------------------------------------------
// Geometry helpers
// -----------------------------------------------------------------

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
    fn contains(&self, p: [f32; 2]) -> bool {
        p[0] >= self.min[0] && p[0] < self.max[0]
            && p[1] >= self.min[1] && p[1] < self.max[1]
    }
}

// -----------------------------------------------------------------
// SDF params — must match the WGSL uniform layout exactly.
//   16-byte aligned vec4 array, 11 slots = 44 f32s = 176 bytes.
// -----------------------------------------------------------------

#[repr(C)]
#[derive(Copy, Clone, Debug, bytemuck::Pod, bytemuck::Zeroable)]
struct SdfParams {
    data: [f32; 44],
}

impl SdfParams {
    fn rect(&mut self, r: [f32; 4]) { self.data[0..4].copy_from_slice(&r); }
    fn fill(&mut self, c: [f32; 4]) { self.data[4..8].copy_from_slice(&c); }
    fn top_peak(&mut self, c: [f32; 4]) { self.data[8..12].copy_from_slice(&c); }
    fn top_dim(&mut self, c: [f32; 4]) { self.data[12..16].copy_from_slice(&c); }
    fn bot_peak(&mut self, c: [f32; 4]) { self.data[16..20].copy_from_slice(&c); }
    fn bot_dim(&mut self, c: [f32; 4]) { self.data[20..24].copy_from_slice(&c); }
    fn grad_left(&mut self, c: [f32; 4]) { self.data[24..28].copy_from_slice(&c); }
    fn grad_right(&mut self, c: [f32; 4]) { self.data[28..32].copy_from_slice(&c); }
    fn scalars(&mut self, corner_radius: f32, top_off: f32, top_in: f32, top_out: f32,
              bot_off: f32, bot_in: f32, bot_out: f32, grad_peak: f32) {
        self.data[32] = corner_radius;
        self.data[33] = top_off;
        self.data[34] = top_in;
        self.data[35] = top_out;
        self.data[36] = bot_off;
        self.data[37] = bot_in;
        self.data[38] = bot_out;
        self.data[39] = grad_peak;
    }
}

// Tab colors pulled from the goal mockup used in `sdf-test` /
// `tab-test`. Kept identical so the visual target is the same.
fn active_tab_params(rect: Rect, glow_boost: f32) -> SdfParams {
    let mut p = SdfParams { data: [0.0; 44] };
    p.rect([rect.min[0], rect.min[1], rect.width(), rect.height()]);
    p.fill([22.0/255.0, 44.0/255.0, 62.0/255.0, 1.0]);
    p.top_peak([(45.0/255.0) * (1.0 + glow_boost),
                (122.0/255.0) * (1.0 + glow_boost),
                (161.0/255.0) * (1.0 + glow_boost), 1.0]);
    p.top_dim([10.0/255.0, 32.0/255.0, 50.0/255.0, 1.0]);
    p.bot_peak([(28.0/255.0) * (1.0 + glow_boost),
                (135.0/255.0) * (1.0 + glow_boost),
                (185.0/255.0) * (1.0 + glow_boost), 1.0]);
    p.bot_dim([10.0/255.0, 30.0/255.0, 50.0/255.0, 1.0]);
    p.grad_left([37.0/255.0, 100.0/255.0, 136.0/255.0, 1.0]);
    p.grad_right([32.0/255.0, 93.0/255.0, 128.0/255.0, 1.0]);
    p.scalars(0.0, 0.0, 3.0, 1.0, -7.0, 2.0, 2.0, 0.45);
    p
}

fn inactive_tab_params(rect: Rect) -> SdfParams {
    // Subdued version: dimmer fill, no border, no gradient.
    let mut p = SdfParams { data: [0.0; 44] };
    p.rect([rect.min[0], rect.min[1], rect.width(), rect.height()]);
    p.fill([14.0/255.0, 22.0/255.0, 32.0/255.0, 1.0]);
    // All accent colors at fill-equivalent brightness so the
    // border band blends into the fill (inactive = no border).
    let c = [14.0/255.0, 22.0/255.0, 32.0/255.0, 1.0];
    p.top_peak(c);
    p.top_dim(c);
    p.bot_peak(c);
    p.bot_dim(c);
    p.grad_left(c);
    p.grad_right(c);
    p.scalars(0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.5);
    p
}

// -----------------------------------------------------------------
// WGSL shader. Identical to sdf-test/src/main.rs WGSL_SHADER.
// (Duplicated to keep this crate self-contained, per Phase 2
// plan § 11.1. Will be promoted to a shared `wgsl-sdf` crate
// once Phase 3 begins.)
// -----------------------------------------------------------------

const WGSL_SHADER: &str = r#"
struct SdfParams {
    data: array<vec4<f32>, 11>,
}
@group(0) @binding(0) var<uniform> params: SdfParams;

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
            let top_peak_c = peak_at(t, p_top_peak());
            if top_d <= 0.0 {
                let dim_d = top_in * 0.33;
                if -top_d <= dim_d {
                    let f = smoothstep(0.0, dim_d, -top_d);
                    color = mix(top_peak_c, p_top_dim(), f);
                } else {
                    let f = smoothstep(dim_d, top_in, -top_d);
                    color = mix(p_top_dim(), p_fill(), f);
                }
            } else {
                let f = smoothstep(0.0, top_out, top_d);
                color = mix(top_peak_c, bg_color, f);
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
            let bot_peak_c = peak_at(t, p_bot_peak());
            if bot_d <= 0.0 {
                let dim_d = bot_in * 0.33;
                if -bot_d <= dim_d {
                    let f = smoothstep(0.0, dim_d, -bot_d);
                    color = mix(bot_peak_c, p_bot_dim(), f);
                } else {
                    let f = smoothstep(dim_d, bot_in, -bot_d);
                    color = mix(p_bot_dim(), p_fill(), f);
                }
            } else {
                let f = smoothstep(0.0, bot_out, bot_d);
                color = mix(bot_peak_c, bg_color, f);
            }
        }
    }

    return color;
}
"#;

// -----------------------------------------------------------------
// Scene graph
// -----------------------------------------------------------------

/// Per-tab interaction state (Steps 3+).
#[derive(Copy, Clone, Debug, Default)]
struct NodeState {
    /// 0..1, 0 = no hover, 1 = fully hovered. Used to brighten glow.
    hover: f32,
    /// 0..1 focus ring strength (Step 4).
    focus: f32,
}

/// A single scene node. Holds a GPU buffer for its SdfParams
/// uniform and the wgpu-side bind group that binds it.
struct SdfNode {
    #[allow(dead_code)]
    label: String,
    rect: Rect,
    state: NodeState,
    /// CPU-side copy of the SdfParams we wrote last frame.
    last_params: SdfParams,
    uniform_buffer: wgpu::Buffer,
    bind_group: wgpu::BindGroup,
}

impl SdfNode {
    fn new(
        device: &wgpu::Device,
        bind_group_layout: &wgpu::BindGroupLayout,
        label: &str,
        rect: Rect,
        params: SdfParams,
    ) -> Self {
        let uniform_buffer = device.create_buffer_init(
            &wgpu::util::BufferInitDescriptor {
                label: Some(&format!("{label}-params")),
                contents: bytemuck::bytes_of(&params),
                usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            },
        );
        let bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some(&format!("{label}-bg")),
            layout: bind_group_layout,
            entries: &[wgpu::BindGroupEntry {
                binding: 0,
                resource: uniform_buffer.as_entire_binding(),
            }],
        });
        Self {
            label: label.to_string(),
            rect,
            state: NodeState::default(),
            last_params: params,
            uniform_buffer,
            bind_group,
        }
    }

    fn write_params(&mut self, queue: &wgpu::Queue, params: SdfParams) {
        self.last_params = params;
        queue.write_buffer(&self.uniform_buffer, 0, bytemuck::bytes_of(&params));
    }
}

/// The scene owns its nodes. Each frame we walk the nodes and
/// issue one draw call per node. This is the *retained* part of
/// the scene graph — the alternative would be to re-emit
/// draw calls every frame from CPU state, which is what the
/// old `tab-test` egui path effectively did.
struct Scene {
    nodes: Vec<SdfNode>,
    vertex_buffer: wgpu::Buffer,
    /// Cached shader module, exposed for the snapshot pipeline
    /// (which targets a different output format but reuses the
    /// same WGSL).
    shader: wgpu::ShaderModule,
    /// Cached pipeline layout (one bind group: SdfParams).
    pipeline_layout: wgpu::PipelineLayout,
    pipeline: wgpu::RenderPipeline,
}

impl Scene {
    fn new(device: &wgpu::Device, format: wgpu::TextureFormat) -> Self {
        // Single shared vertex buffer with a 800x600 quad sized
        // for the largest expected tab. We scale via the SDF
        // uniform rect, not the mesh, so the same 6-vertex quad
        // works for every tab.
        let quad: [[f32; 2]; 6] = [
            [0.0, 0.0], [800.0, 0.0], [800.0, 600.0],
            [0.0, 0.0], [800.0, 600.0], [0.0, 600.0],
        ];
        let vertex_buffer = device.create_buffer_init(
            &wgpu::util::BufferInitDescriptor {
                label: Some("scene-quad"),
                contents: bytemuck::cast_slice(&quad),
                usage: wgpu::BufferUsages::VERTEX,
            },
        );

        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("scene-sdf-shader"),
            source: wgpu::ShaderSource::Wgsl(WGSL_SHADER.into()),
        });

        let bind_group_layout =
            device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
                label: Some("scene-sdf-bgl"),
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

        let pipeline_layout = device.create_pipeline_layout(
            &wgpu::PipelineLayoutDescriptor {
                label: Some("scene-sdf-pl"),
                bind_group_layouts: &[Some(&bind_group_layout)],
                immediate_size: 0,
            },
        );

        let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("scene-sdf-pipeline"),
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
                    format,
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

        Self {
            nodes: Vec::new(),
            vertex_buffer,
            shader,
            pipeline_layout,
            pipeline,
        }
    }

    fn push(&mut self, device: &wgpu::Device, node: SdfNode) {
        // We need the bind_group_layout, but it's owned by the
        // pipeline; for simplicity, we attach it via a
        // constructor. See `make_node` below.
        let _ = device;
        self.nodes.push(node);
    }

    fn render<'a>(&'a self, rpass: &mut wgpu::RenderPass<'a>) {
        rpass.set_pipeline(&self.pipeline);
        rpass.set_vertex_buffer(0, self.vertex_buffer.slice(..));
        for node in &self.nodes {
            rpass.set_bind_group(0, &node.bind_group, &[]);
            rpass.draw(0..6, 0..1);
        }
    }
}

/// Helper: build a SdfNode in one call. The bind_group_layout
/// is borrowed from the pipeline layout; we recover it via
/// `pipeline.get_bind_group_layout(0)`.
fn make_node(
    device: &wgpu::Device,
    pipeline: &wgpu::RenderPipeline,
    label: &str,
    rect: Rect,
    params: SdfParams,
) -> SdfNode {
    let bgl = pipeline.get_bind_group_layout(0);
    SdfNode::new(device, &bgl, label, rect, params)
}

// -----------------------------------------------------------------
// Tab layout (Step 2+)
// -----------------------------------------------------------------

fn build_tab_bar() -> Vec<(String, Rect, bool)> {
    // Five tabs across the top of an 800x600 window.
    // Tab size: 140 x 50, gap 4, left margin 20, top margin 20.
    let tab_w = 140.0;
    let tab_h = 50.0;
    let gap = 4.0;
    let x0 = 20.0;
    let y0 = 20.0;
    let labels = ["Tab 1", "Tab 2", "Tab 3", "Tab 4", "Tab 5"];
    labels
        .iter()
        .enumerate()
        .map(|(i, label)| {
            let x = x0 + i as f32 * (tab_w + gap);
            (
                label.to_string(),
                Rect::from_min_size([x, y0], [tab_w, tab_h]),
                i == 0, // first is active
            )
        })
        .collect()
}

// -----------------------------------------------------------------
// App
// -----------------------------------------------------------------

struct App {
    step: u32,
    /// Manually-changed active tab index (Step 4+).
    active_idx: usize,
    /// When Some, write a snapshot next frame.
    snapshot_pending: bool,
    /// Auto-snapshot N frames after start (for headless / CI runs).
    auto_snapshot_at: Option<u32>,
    /// Frame counter since start.
    frame_count: u32,

    window: Option<Arc<Window>>,
    device: Option<wgpu::Device>,
    queue: Option<wgpu::Queue>,
    surface: Option<wgpu::Surface<'static>>,
    surface_config: Option<wgpu::SurfaceConfiguration>,

    scene: Option<Scene>,

    egui_ctx: Option<egui::Context>,
    egui_state: Option<egui_winit::State>,
}

impl App {
    fn new(step: u32) -> Self {
        Self {
            step,
            active_idx: 0,
            snapshot_pending: false,
            auto_snapshot_at: parse_auto_snapshot(),
            frame_count: 0,
            window: None,
            device: None,
            queue: None,
            surface: None,
            surface_config: None,
            scene: None,
            egui_ctx: None,
            egui_state: None,
        }
    }

    /// Update the SdfParams for each node based on the current
    /// step, hover state, and active state. This is the only
    /// place that mutates uniform buffers; the scene graph
    /// *retains* the buffers between frames.
    fn update_node_params(&mut self, hovered: Option<usize>) {
        let scene = self.scene.as_mut().unwrap();
        let queue = self.queue.as_ref().unwrap();
        for (i, node) in scene.nodes.iter_mut().enumerate() {
            let is_active = i == self.active_idx;
            // Smooth hover toward target (Step 3+).
            if self.step >= 3 {
                let target = if hovered == Some(i) { 1.0 } else { 0.0 };
                node.state.hover += (target - node.state.hover) * 0.25;
            }
            // Smooth focus (Step 4+).
            if self.step >= 4 {
                let target = if is_active { 1.0 } else { 0.0 };
                node.state.focus += (target - node.state.focus) * 0.15;
            }

            let params = if is_active {
                active_tab_params(node.rect, node.state.hover * 0.3)
            } else {
                let mut p = inactive_tab_params(node.rect);
                if self.step >= 3 && node.state.hover > 0.01 {
                    // Subtle hover brightening for inactive tabs.
                    let b = node.state.hover * 0.18;
                    p.fill([
                        14.0/255.0 + b * 0.5,
                        22.0/255.0 + b * 0.6,
                        32.0/255.0 + b * 0.7,
                        1.0,
                    ]);
                }
                p
            };
            node.write_params(queue, params);
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
                        .with_title(format!(
                            "scene-chrome-test step={}",
                            self.step
                        ))
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
                        label: Some("scene-chrome-test"),
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

        // Build the scene. Step 1 only renders the first tab
        // to demonstrate "one quad, no interaction". Steps 2+
        // render all five.
        let mut scene = Scene::new(&device, surface_config.format);
        let tabs = build_tab_bar();
        let tab_count = if self.step == 1 { 1 } else { tabs.len() };
        for (i, (label, rect, _is_active_init)) in tabs.iter().enumerate() {
            if i >= tab_count {
                break;
            }
            let is_active_init = i == 0;
            let params = if is_active_init {
                active_tab_params(*rect, 0.0)
            } else {
                inactive_tab_params(*rect)
            };
            let node = make_node(
                &device,
                &scene.pipeline,
                label,
                *rect,
                params,
            );
            scene.push(&device, node);
        }

        // Egui is only used for hit-testing in Step 3+, but we
        // create the context for all steps so we can reuse the
        // same winit pipeline.
        let egui_ctx = egui::Context::default();
        let viewport_id = egui_ctx.viewport_id();
        let egui_state = egui_winit::State::new(
            egui_ctx.clone(),
            viewport_id,
            &window,
            None,
            None,
            None,
        );

        self.window = Some(window);
        self.device = Some(device);
        self.queue = Some(queue);
        self.surface = Some(surface);
        self.surface_config = Some(surface_config);
        self.scene = Some(scene);
        self.egui_ctx = Some(egui_ctx);
        self.egui_state = Some(egui_state);
    }

    fn window_event(
        &mut self,
        event_loop: &ActiveEventLoop,
        _id: WindowId,
        event: WindowEvent,
    ) {
        // Forward to egui.
        if let (Some(egui_state), Some(window)) =
            (&mut self.egui_state, &self.window)
        {
            let _ = egui_state.on_window_event(window, &event);
        }

        match event {
            WindowEvent::CloseRequested => event_loop.exit(),
            WindowEvent::KeyboardInput {
                event:
                    KeyEvent {
                        logical_key: Key::Named(NamedKey::Escape),
                        state: ElementState::Pressed,
                        ..
                    },
                ..
            } => event_loop.exit(),
            WindowEvent::KeyboardInput {
                event:
                    KeyEvent {
                        logical_key: Key::Character(s),
                        state: ElementState::Pressed,
                        ..
                    },
                ..
            } => match s.as_str() {
                "q" | "Q" => event_loop.exit(),
                "s" | "S" => self.snapshot_pending = true,
                "1" => self.step = 1,
                "2" => self.step = 2,
                "3" => self.step = 3,
                "4" => self.step = 4,
                _ => {}
            },
            WindowEvent::Resized(size) => {
                if let (Some(device), Some(surface), Some(config)) = (
                    self.device.as_ref(),
                    self.surface.as_ref(),
                    self.surface_config.as_mut(),
                ) {
                    config.width = size.width.max(1);
                    config.height = size.height.max(1);
                    surface.configure(device, config);
                }
            }
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
        self.frame_count = self.frame_count.saturating_add(1);

        // Step 1: gather pointer state from egui (Step 3+ only).
        let mut hover_pos: Option<egui::Pos2> = None;
        let mut primary_pressed = false;
        if self.step >= 3 {
            let window = self.window.as_ref().unwrap();
            let egui_ctx = self.egui_ctx.as_ref().unwrap();
            let egui_state = self.egui_state.as_mut().unwrap();
            let raw_input = egui_state.take_egui_input(window);
            #[allow(deprecated)]
            let _ = egui_ctx.run(raw_input, |ctx| {
                ctx.input(|i| {
                    hover_pos = i.pointer.hover_pos();
                    primary_pressed = i.pointer.primary_pressed();
                });
            });
        }

        // Hit-test against the scene's node rects.
        let hovered: Option<usize> = {
            let scene = self.scene.as_ref().unwrap();
            let mut hov: Option<usize> = None;
            if let Some(p) = hover_pos {
                for (i, node) in scene.nodes.iter().enumerate() {
                    if node.rect.contains([p.x, p.y]) {
                        hov = Some(i);
                        break;
                    }
                }
            }
            hov
        };
        let clicked: Option<usize> =
            if primary_pressed { hovered } else { None };

        // Apply click in Step 4+ and update uniforms (1 mut borrow).
        if self.step >= 4 {
            if let Some(i) = clicked {
                self.active_idx = i;
            }
        }
        self.update_node_params(hovered);

        // Snapshot if requested (also a mut borrow of self).
        if self.snapshot_pending {
            self.snapshot_pending = false;
            self.write_snapshot();
        }
        // Auto-snapshot for headless / CI runs.
        if let Some(target) = self.auto_snapshot_at {
            if self.frame_count == target {
                self.write_snapshot();
                self.auto_snapshot_at = None;
            }
        }

        // ---- Render ----
        let window = self.window.as_ref().unwrap();
        let device = self.device.as_ref().unwrap();
        let queue = self.queue.as_ref().unwrap();
        let surface = self.surface.as_ref().unwrap();
        let scene = self.scene.as_ref().unwrap();

        let output = surface.get_current_texture();
        let surface_texture = match output {
            wgpu::CurrentSurfaceTexture::Success(t)
            | wgpu::CurrentSurfaceTexture::Suboptimal(t) => t,
            other => {
                log::warn!("Surface texture unavailable: {:?}", other);
                return;
            }
        };
        let view = surface_texture
            .texture
            .create_view(&Default::default());

        let mut encoder =
            device.create_command_encoder(&Default::default());
        {
            let mut rpass =
                encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                    label: Some("scene-chrome-test"),
                    color_attachments: &[Some(
                        wgpu::RenderPassColorAttachment {
                            view: &view,
                            resolve_target: None,
                            depth_slice: None,
                            ops: wgpu::Operations {
                                load: wgpu::LoadOp::Clear(wgpu::Color {
                                    r: 0.0,
                                    g: 0.0,
                                    b: 0.0,
                                    a: 1.0,
                                }),
                                store: wgpu::StoreOp::Store,
                            },
                        },
                    )],
                    depth_stencil_attachment: None,
                    timestamp_writes: None,
                    occlusion_query_set: None,
                    multiview_mask: None,
                })
                .forget_lifetime();
            scene.render(&mut rpass);
        }

        queue.submit(std::iter::once(encoder.finish()));
        surface_texture.present();
        // Suppress unused warnings for variables that are only
        // read inside the `if self.step >= 3` block above.
        let _ = window;
    }

    fn write_snapshot(&self) {
        // Render to an *offscreen* RGBA8 texture that has
        // RENDER_ATTACHMENT | COPY_SRC, then copy into a read
        // buffer. We can't copy from the swapchain texture
        // directly because the surface config doesn't include
        // COPY_SRC.
        let device = self.device.as_ref().unwrap();
        let queue = self.queue.as_ref().unwrap();
        let cfg = self.surface_config.as_ref().unwrap();
        let scene = self.scene.as_ref().unwrap();

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

        // Use a non-sRGB RGBA8 unorm texture for the snapshot
        // readback. The swapchain format is sRGB-encoded, which
        // means reading the bytes back gives us sRGB-encoded
        // pixels that look wrong in the saved PNG. By rendering
        // to a plain Rgba8Unorm we round-trip the bytes exactly.
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

        // Build a one-off pipeline that targets the snapshot
        // format. Re-uses the same shader (WGSL has no
        // format-dependent code paths here) and the same vertex
        // buffer layout.
        let snapshot_pipeline = device.create_render_pipeline(
            &wgpu::RenderPipelineDescriptor {
                label: Some("snapshot-pipeline"),
                layout: Some(&scene.pipeline_layout),
                vertex: wgpu::VertexState {
                    module: &scene.shader,
                    entry_point: Some("vs_main"),
                    buffers: &[wgpu::VertexBufferLayout {
                        array_stride: 8,
                        step_mode: wgpu::VertexStepMode::Vertex,
                        attributes: &wgpu::vertex_attr_array![0 => Float32x2],
                    }],
                    compilation_options: wgpu::PipelineCompilationOptions::default(),
                },
                fragment: Some(wgpu::FragmentState {
                    module: &scene.shader,
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
                label: Some("snapshot-pass"),
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
            rpass.set_vertex_buffer(0, scene.vertex_buffer.slice(..));
            for node in &scene.nodes {
                rpass.set_bind_group(0, &node.bind_group, &[]);
                rpass.draw(0..6, 0..1);
            }
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

        // Map + wait.
        let slice = read_buf.slice(..);
        slice.map_async(wgpu::MapMode::Read, |_| {});
        let _ = device.poll(wgpu::PollType::wait_indefinitely());

        let mapped = slice.get_mapped_range();
        let path = format!(
            "target/snapshots/scene-chrome-test_{}.png",
            self.step
        );
        std::fs::create_dir_all("target/snapshots").ok();
        match write_png_rgba(&path, w, h, padded_row, &mapped) {
            Ok(()) => eprintln!("[snapshot] wrote {}", path),
            Err(e) => eprintln!("[snapshot] failed: {}", e),
        }
        drop(mapped);
        read_buf.unmap();
    }
}

// -----------------------------------------------------------------
// Tiny PNG writer (RGBA8) — zero deps.
// Spec: https://www.w3.org/TR/PNG/
// -----------------------------------------------------------------

fn write_png_rgba(
    path: &str,
    w: u32,
    h: u32,
    padded_row: u32,
    rgba: &[u8],
) -> std::io::Result<()> {
    use std::io::Write;

    // 1. Filter type 0 (None) for every row.
    let mut raw = Vec::with_capacity(((padded_row + 1) * h) as usize);
    for y in 0..h {
        raw.push(0u8);
        let start = (y * padded_row) as usize;
        let end = start + (w as usize) * 4;
        raw.extend_from_slice(&rgba[start..end]);
    }

    // 2. zlib-compress (deflate stored blocks — no compression
    //    but valid; we don't need small PNGs for a snapshot).
    let compressed = zlib_store(&raw);

    // 3. Build PNG chunks.
    let mut out = Vec::new();
    out.extend_from_slice(&[137, 80, 78, 71, 13, 10, 26, 10]); // signature
    write_chunk(&mut out, b"IHDR", &{
        let mut v = Vec::with_capacity(13);
        v.extend_from_slice(&w.to_be_bytes());
        v.extend_from_slice(&h.to_be_bytes());
        v.push(8);   // bit depth
        v.push(6);   // color type RGBA
        v.push(0);   // compression
        v.push(0);   // filter
        v.push(0);   // interlace
        v
    });
    write_chunk(&mut out, b"IDAT", &compressed);
    write_chunk(&mut out, b"IEND", &[]);

    let mut f = std::fs::File::create(path)?;
    f.write_all(&out)?;
    Ok(())
}

fn write_chunk(out: &mut Vec<u8>, kind: &[u8; 4], data: &[u8]) {
    let len = data.len() as u32;
    out.extend_from_slice(&len.to_be_bytes());
    out.extend_from_slice(kind);
    out.extend_from_slice(data);
    let mut crc_input = Vec::with_capacity(4 + data.len());
    crc_input.extend_from_slice(kind);
    crc_input.extend_from_slice(data);
    out.extend_from_slice(&crc32(&crc_input).to_be_bytes());
}

fn crc32(buf: &[u8]) -> u32 {
    let mut table = [0u32; 256];
    for n in 0..256u32 {
        let mut c = n;
        for _ in 0..8 {
            c = if c & 1 != 0 { 0xedb8_8320 ^ (c >> 1) } else { c >> 1 };
        }
        table[n as usize] = c;
    }
    let mut crc = 0xffff_ffffu32;
    for &b in buf {
        let idx = ((crc ^ b as u32) & 0xff) as usize;
        crc = table[idx] ^ (crc >> 8);
    }
    crc ^ 0xffff_ffff
}

/// Stored (uncompressed) deflate blocks. Valid zlib stream.
fn zlib_store(data: &[u8]) -> Vec<u8> {
    let mut out = Vec::new();
    out.push(0x78); // CMF: CM=8, CINFO=7
    out.push(0x01); // FLG: FCHECK=1, no dict, level 0

    const MAX_BLOCK: usize = 0xFFFF;
    if data.is_empty() {
        // Empty stored block (BTYPE=00) for an empty stream.
        out.push(0x01);
        out.extend_from_slice(&0u16.to_le_bytes());
        out.extend_from_slice(&adler32(data).to_be_bytes());
        return out;
    }
    let mut blocks = data.chunks(MAX_BLOCK).peekable();
    while let Some(chunk) = blocks.next() {
        let is_last = blocks.peek().is_none();
        out.push(if is_last { 0x01 } else { 0x00 });
        let len = chunk.len() as u16;
        let nlen = !len;
        out.extend_from_slice(&len.to_le_bytes());
        out.extend_from_slice(&nlen.to_le_bytes());
        out.extend_from_slice(chunk);
    }
    out.extend_from_slice(&adler32(data).to_be_bytes());
    out
}

fn adler32(data: &[u8]) -> u32 {
    let mut a: u32 = 1;
    let mut b: u32 = 0;
    const MOD: u32 = 65_521;
    for &x in data {
        a = (a + x as u32) % MOD;
        b = (b + a) % MOD;
    }
    (b << 16) | a
}

// -----------------------------------------------------------------
// Step CLI parsing
// -----------------------------------------------------------------

fn parse_step() -> u32 {
    let args: Vec<String> = std::env::args().collect();
    for arg in args.iter() {
        if let Some(val) = arg.strip_prefix("--step=") {
            return val.parse().unwrap_or(1).clamp(1, 4);
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
    log::info!("scene-chrome-test step: {}", step);
    let event_loop = EventLoop::new()?;
    let mut app = App::new(step);
    event_loop.run_app(&mut app)?;
    Ok(())
}
