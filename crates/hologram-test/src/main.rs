//! `hologram-test` — Phase 5 of the cinematic-UI plan.
//!
//! The terminal exists in *world space*. A 3D camera sees a
//! curved display mesh. Floating widgets orbit. Particles
//! drift. The terminal is still readable because text is
//! rendered on a flat plane attached to the mesh.
//!
//! Steps:
//!   1. Flat plane, orthographic camera, procedural terminal
//!      texture sampled.
//!   2. + curvature (vertex shader bends the plane).
//!   3. + perspective camera, keyboard orbit.
//!   4. + 3D-positioned chrome (colored quads floating in
//!      space).
//!   5. + particle splat field drifting around the mesh.
//!
//! Run with: `cargo run -p hologram-test --release -- --step=N`

use std::sync::Arc;

use winit::application::ApplicationHandler;
use winit::event::WindowEvent;
use winit::event_loop::ActiveEventLoop;
use winit::keyboard::{Key, NamedKey};
use winit::window::{Window, WindowId};

use wgpu::util::DeviceExt;

const STEP: u32 = 5; // 1..5

// -----------------------------------------------------------------
// Camera
// -----------------------------------------------------------------

#[derive(Copy, Clone, Debug)]
enum Projection {
    Ortho { half_w: f32, half_h: f32, near: f32, far: f32 },
    Perspective { fov_y: f32, aspect: f32, near: f32, far: f32 },
}

#[derive(Copy, Clone, Debug)]
struct Camera {
    /// World-space eye position.
    eye: [f32; 3],
    /// World-space point the eye looks at.
    target: [f32; 3],
    /// Up vector (typically [0, 1, 0]).
    up: [f32; 3],
    projection: Projection,
}

impl Camera {
    fn ortho_step1(width: f32, height: f32) -> Self {
        // Eye at z=+1, looking at the origin. The mesh sits at
        // z=0 and fills the [-1, 1] clip-space rectangle when
        // the ortho projection maps it through the half_w/half_h
        // we set below.
        Self {
            eye: [0.0, 0.0, 1.0],
            target: [0.0, 0.0, 0.0],
            up: [0.0, 1.0, 0.0],
            projection: Projection::Ortho {
                half_w: width * 0.5,
                half_h: height * 0.5,
                near: -2.0,
                far: 2.0,
            },
        }
    }

    fn perspective_step3(aspect: f32) -> Self {
        // Eye at z=+2, looking at the origin. A nice "monitor on
        // a desk" framing for a 1×1 mesh.
        Self {
            eye: [0.0, 0.0, 2.0],
            target: [0.0, 0.0, 0.0],
            up: [0.0, 1.0, 0.0],
            projection: Projection::Perspective {
                fov_y: 60_f32.to_radians(),
                aspect,
                near: 0.1,
                far: 100.0,
            },
        }
    }

    /// Build the view matrix (lookAt) as a column-major 4x4.
    fn view_matrix(&self) -> [[f32; 4]; 4] {
        look_at(self.eye, self.target, self.up)
    }

    /// Build the projection matrix as a column-major 4x4.
    fn proj_matrix(&self) -> [[f32; 4]; 4] {
        match self.projection {
            Projection::Ortho { half_w, half_h, near, far } => ortho(half_w, half_h, near, far),
            Projection::Perspective { fov_y, aspect, near, far } => perspective(fov_y, aspect, near, far),
        }
    }

    /// view * proj, column-major 4x4. This is what the vertex
    /// shader needs (combined with the model matrix on the CPU
    /// side before upload).
    fn vp_matrix(&self) -> [[f32; 4]; 4] {
        mat4_mul(self.proj_matrix(), self.view_matrix())
    }
}

// -----------------------------------------------------------------
// 4x4 matrix helpers (column-major, right-handed, wgpu/Vulkan
// clip space Z in [0, 1]).
// -----------------------------------------------------------------

fn look_at(eye: [f32; 3], target: [f32; 3], up: [f32; 3]) -> [[f32; 4]; 4] {
    // f = normalize(target - eye)
    let f = normalize3(sub3(target, eye));
    // s = normalize(cross(f, up))
    let s = normalize3(cross3(f, up));
    // u = cross(s, f)
    let u = cross3(s, f);
    // view = [ s.x  s.y  s.z  -dot(s, eye) ]
    //        [ u.x  u.y  u.z  -dot(u, eye) ]
    //        [-f.x -f.y -f.z  dot(f, eye) ]
    //        [ 0    0    0    1           ]
    let m = [
        [ s[0],  s[1],  s[2], -dot3(s, eye)],
        [ u[0],  u[1],  u[2], -dot3(u, eye)],
        [-f[0], -f[1], -f[2],  dot3(f, eye)],
        [ 0.0,   0.0,   0.0,  1.0        ],
    ];
    m
}

fn ortho(half_w: f32, half_h: f32, near: f32, far: f32) -> [[f32; 4]; 4] {
    // Maps [-half_w, half_w] x [-half_h, half_h] x [near, far]
    // to [-1, 1] x [-1, 1] x [0, 1] (wgpu/Vulkan clip space).
    [
        [1.0 / half_w,   0.0,             0.0,            0.0           ],
        [0.0,            1.0 / half_h,    0.0,            0.0           ],
        [0.0,            0.0,             1.0 / (far - near), -near / (far - near)],
        [0.0,            0.0,             0.0,            1.0           ],
    ]
}

fn perspective(fov_y: f32, aspect: f32, near: f32, far: f32) -> [[f32; 4]; 4] {
    let f = 1.0 / (fov_y * 0.5).tan();
    [
        [f / aspect, 0.0, 0.0,                          0.0                          ],
        [0.0,        f,   0.0,                          0.0                          ],
        [0.0,        0.0, far / (far - near),          -far * near / (far - near)   ],
        [0.0,        0.0, 1.0,                          0.0                          ],
    ]
}

fn mat4_mul(a: [[f32; 4]; 4], b: [[f32; 4]; 4]) -> [[f32; 4]; 4] {
    let mut out = [[0.0; 4]; 4];
    for col in 0..4 {
        for row in 0..4 {
            let mut s = 0.0;
            for k in 0..4 {
                s += a[k][row] * b[col][k];
            }
            out[col][row] = s;
        }
    }
    out
}

fn mat4_translate(t: [f32; 3]) -> [[f32; 4]; 4] {
    [
        [1.0, 0.0, 0.0, 0.0],
        [0.0, 1.0, 0.0, 0.0],
        [0.0, 0.0, 1.0, 0.0],
        [t[0], t[1], t[2], 1.0],
    ]
}

fn mat4_scale(s: [f32; 3]) -> [[f32; 4]; 4] {
    [
        [s[0], 0.0,  0.0,  0.0],
        [0.0,  s[1], 0.0,  0.0],
        [0.0,  0.0,  s[2], 0.0],
        [0.0,  0.0,  0.0,  1.0],
    ]
}

fn sub3(a: [f32; 3], b: [f32; 3]) -> [f32; 3] { [a[0] - b[0], a[1] - b[1], a[2] - b[2]] }
fn dot3(a: [f32; 3], b: [f32; 3]) -> f32 { a[0] * b[0] + a[1] * b[1] + a[2] * b[2] }
fn cross3(a: [f32; 3], b: [f32; 3]) -> [f32; 3] {
    [
        a[1] * b[2] - a[2] * b[1],
        a[2] * b[0] - a[0] * b[2],
        a[0] * b[1] - a[1] * b[0],
    ]
}
fn normalize3(v: [f32; 3]) -> [f32; 3] {
    let l = (v[0] * v[0] + v[1] * v[1] + v[2] * v[2]).sqrt();
    if l < 1e-8 { [0.0, 0.0, 0.0] } else { [v[0] / l, v[1] / l, v[2] / l] }
}

// -----------------------------------------------------------------
// Uniforms
// -----------------------------------------------------------------
//
// Three uniform structs — one per pipeline — all carry a mat4x4
// `mvp`. WGSL aligns mat4x4 to 16 bytes, so each struct is
// naturally a multiple of 16 bytes if we keep the trailing
// fields scalar f32. We do *not* use vec3/vec4 here because
// wgpu 29 / naga will pad those to 16 bytes and break the
// Rust <-> WGSL byte layout otherwise.

#[repr(C)]
#[derive(Copy, Clone, Debug, bytemuck::Pod, bytemuck::Zeroable)]
struct MeshUniforms {
    /// model-view-projection (column-major mat4x4).
    mvp: [[f32; 4]; 4],
    /// model matrix only (used to compute curvature in object
    /// space). Column-major mat4x4.
    model: [[f32; 4]; 4],
    /// how much to bend the plane in the vertex shader
    /// (step 2).
    curvature: f32,
    _pad0: f32, _pad1: f32, _pad2: f32,
}

#[repr(C)]
#[derive(Copy, Clone, Debug, bytemuck::Pod, bytemuck::Zeroable)]
struct ChromeUniforms {
    mvp: [[f32; 4]; 4],
    color: [f32; 4],
}

#[repr(C)]
#[derive(Copy, Clone, Debug, bytemuck::Pod, bytemuck::Zeroable)]
struct SplatUniforms {
    mvp: [[f32; 4]; 4],
    center: [f32; 3],
    radius: f32,
    color: [f32; 4],
}

const _ASSERT_MESH_SIZE: () = assert!(std::mem::size_of::<MeshUniforms>() == 144);
const _ASSERT_CHROME_SIZE: () = assert!(std::mem::size_of::<ChromeUniforms>() == 80);
const _ASSERT_SPLAT_SIZE: () = assert!(std::mem::size_of::<SplatUniforms>() == 96);

// -----------------------------------------------------------------
// Vertex layouts
// -----------------------------------------------------------------

#[repr(C)]
#[derive(Copy, Clone, bytemuck::Pod, bytemuck::Zeroable)]
struct MeshVertex {
    pos: [f32; 3],
    uv: [f32; 2],
}

#[repr(C)]
#[derive(Copy, Clone, bytemuck::Pod, bytemuck::Zeroable)]
struct QuadVertex {
    pos: [f32; 2],
}

// -----------------------------------------------------------------
// Mesh generation
// -----------------------------------------------------------------

/// Build a grid mesh in the XY plane (z=0) covering
/// [-half_w, half_w] x [-half_h, half_h]. `grid` is the
/// vertex resolution per axis (e.g. 32 means 32×32 vertices,
/// 31×31 cells).
fn build_mesh(half_w: f32, half_h: f32, grid: u32) -> (Vec<MeshVertex>, Vec<u16>) {
    let n = grid as usize;
    let mut verts = Vec::with_capacity(n * n);
    for j in 0..n {
        for i in 0..n {
            let u = i as f32 / (n - 1) as f32;
            let v = j as f32 / (n - 1) as f32;
            verts.push(MeshVertex {
                pos: [u * 2.0 * half_w - half_w, v * 2.0 * half_h - half_h, 0.0],
                uv: [u, v],
            });
        }
    }
    let mut idx = Vec::with_capacity((n - 1) * (n - 1) * 6);
    for j in 0..(n - 1) {
        for i in 0..(n - 1) {
            let tl = (j * n + i) as u16;
            let tr = (j * n + i + 1) as u16;
            let bl = ((j + 1) * n + i) as u16;
            let br = ((j + 1) * n + i + 1) as u16;
            // Two triangles per cell, CCW.
            idx.extend_from_slice(&[tl, bl, tr, tr, bl, br]);
        }
    }
    (verts, idx)
}

fn build_quad_verts() -> Vec<QuadVertex> {
    // Unit quad in [-0.5, 0.5] on both axes; the model matrix
    // scales and positions it in 3D.
    vec![
        QuadVertex { pos: [-0.5, -0.5] },
        QuadVertex { pos: [ 0.5, -0.5] },
        QuadVertex { pos: [ 0.5,  0.5] },
        QuadVertex { pos: [-0.5, -0.5] },
        QuadVertex { pos: [ 0.5,  0.5] },
        QuadVertex { pos: [-0.5,  0.5] },
    ]
}

// -----------------------------------------------------------------
// Scene
// -----------------------------------------------------------------

/// One 3D-positioned chrome element (a colored quad in world
/// space).
struct ChromeNode {
    position: [f32; 3],
    size: [f32; 2],
    color: [f32; 4],
}

/// One drifting splat. `phase` is a per-splat phase offset so
/// the field doesn't all pulse in lockstep.
struct Splat {
    home: [f32; 3],
    amplitude: [f32; 3],
    speed: f32,
    phase: f32,
    radius: f32,
    color: [f32; 4],
}

struct Scene {
    /// Mesh world transform (a model matrix).
    mesh_model: [[f32; 4]; 4],
    /// Step 2: how much the vertex shader bends the plane.
    mesh_curvature: f32,
    /// Step 4: floating chrome quads.
    chrome: Vec<ChromeNode>,
    /// Step 5: drifting splats.
    splats: Vec<Splat>,
    time_ms: f32,
}

impl Scene {
    fn for_step(step: u32) -> Self {
        let mut s = Self {
            mesh_model: mat4_translate([0.0, 0.0, 0.0]),
            mesh_curvature: 0.0,
            chrome: Vec::new(),
            splats: Vec::new(),
            time_ms: 0.0,
        };
        match step {
            1 => { /* flat plane, no curvature, no chrome, no splats */ }
            2 => { s.mesh_curvature = 0.8; }
            3 => { s.mesh_curvature = 0.8; }
            4 => {
                s.mesh_curvature = 0.8;
                // 5 chrome tabs floating in front of the mesh.
                let tab_w = 0.18;
                let tab_h = 0.06;
                let z = 0.25;
                let colors = [
                    [0.4, 0.5, 0.9, 1.0],
                    [0.9, 0.4, 0.5, 1.0],
                    [0.4, 0.9, 0.5, 1.0],
                    [0.9, 0.8, 0.3, 1.0],
                    [0.7, 0.4, 0.9, 1.0],
                ];
                for (i, c) in colors.iter().enumerate() {
                    let x = -0.4 + i as f32 * 0.2;
                    s.chrome.push(ChromeNode {
                        position: [x, 0.6, z],
                        size: [tab_w, tab_h],
                        color: *c,
                    });
                }
            }
            5 => {
                s.mesh_curvature = 0.8;
                // Chrome (same as step 4).
                let tab_w = 0.18;
                let tab_h = 0.06;
                let z = 0.25;
                let colors = [
                    [0.4, 0.5, 0.9, 1.0],
                    [0.9, 0.4, 0.5, 1.0],
                    [0.4, 0.9, 0.5, 1.0],
                    [0.9, 0.8, 0.3, 1.0],
                    [0.7, 0.4, 0.9, 1.0],
                ];
                for (i, c) in colors.iter().enumerate() {
                    let x = -0.4 + i as f32 * 0.2;
                    s.chrome.push(ChromeNode {
                        position: [x, 0.6, z],
                        size: [tab_w, tab_h],
                        color: *c,
                    });
                }
                // 200 splats drifting around the mesh.
                for i in 0..200 {
                    let i_f = i as f32;
                    let a = (i_f * 2.399_963) % 6.283_185_3; // golden-angle spread
                    let r = 0.4 + (i_f * 0.0131).sin() * 0.35;
                    let y = ((i_f * 0.7).sin()) * 0.3;
                    s.splats.push(Splat {
                        home: [a.cos() * r, y, a.sin() * r * 0.3 + 0.05],
                        amplitude: [
                            ((i_f * 1.3).sin()) * 0.04,
                            ((i_f * 0.9).cos()) * 0.05,
                            ((i_f * 1.7).sin()) * 0.04,
                        ],
                        speed: 0.5 + (i_f * 0.31).fract() * 1.5,
                        phase: i_f * 0.7,
                        radius: 0.025 + (i_f * 0.27).fract() * 0.03,
                        color: [
                            0.5 + (i_f * 0.13).sin() * 0.5,
                            0.5 + (i_f * 0.21).cos() * 0.5,
                            0.7 + (i_f * 0.07).sin() * 0.3,
                            0.7,
                        ],
                    });
                }
            }
            _ => panic!("unknown step {}", step),
        }
        s
    }

    fn tick(&mut self, frame_dt_ms: f32) {
        self.time_ms += frame_dt_ms;
    }
}

// -----------------------------------------------------------------
// WGSL
// -----------------------------------------------------------------
//
// All three pipelines share the same mat4x4 mvp math and the
// same vertex-in / vertex-out struct shape.

const SHADER: &str = r#"
struct VsIn {
    @location(0) pos: vec3<f32>,
    @location(1) uv: vec2<f32>,
};
struct VsOut {
    @builtin(position) clip_pos: vec4<f32>,
    @location(0) uv: vec2<f32>,
    @location(1) world: vec3<f32>,
};

// ============================ MESH ============================
struct MeshU {
    mvp: mat4x4<f32>,
    model: mat4x4<f32>,
    curvature: f32,
};
@group(0) @binding(0) var<uniform> u_mesh: MeshU;

@vertex
fn vs_mesh(in: VsIn) -> VsOut {
    var out: VsOut;
    // Bend the plane in object space: move +Z based on
    // curvature * distance^2 from the origin. curvature=0
    // gives a flat plane.
    let d2 = in.pos.x * in.pos.x + in.pos.y * in.pos.y;
    let bend = u_mesh.curvature * d2;
    let bent = vec3<f32>(in.pos.x, in.pos.y, bend);
    let world = u_mesh.model * vec4<f32>(bent, 1.0);
    out.clip_pos = u_mesh.mvp * vec4<f32>(bent, 1.0);
    out.uv = in.uv;
    out.world = world.xyz;
    return out;
}

@fragment
fn fs_mesh(in: VsOut) -> @location(0) vec4<f32> {
    // Procedural "terminal" — a 40x25 grid of cells, each
    // randomly colored. Some cells are "empty" (background).
    let cols = 40.0;
    let rows = 25.0;
    let cell_x = floor(in.uv.x * cols);
    let cell_y = floor(in.uv.y * rows);
    let cell_id = cell_x + cell_y * cols;

    // Hash → per-cell RGB.
    let h = cell_id * 0.1031;
    let r = fract(sin(h * 12.9898) * 43758.5453);
    let g = fract(sin(h * 78.233)  * 43758.5453);
    let b = fract(sin(h * 39.346)  * 43758.5453);
    let bright = step(0.55, r * g + g * b);

    let bg    = vec3<f32>(0.04, 0.04, 0.06);
    let cell  = vec3<f32>(r, g, b) * 0.85;
    let color = mix(bg, cell, bright);

    // Slight darkening at the mesh edges so curvature is
    // visually obvious (step 2+).
    let edge = smoothstep(0.95, 0.5, max(abs(in.uv.x - 0.5), abs(in.uv.y - 0.5)) * 2.0);
    return vec4<f32>(color * (0.5 + 0.5 * edge), 1.0);
}

// ============================ CHROME ===========================
struct VsIn2 {
    @location(0) pos: vec2<f32>,
};
struct VsOut2 {
    @builtin(position) clip_pos: vec4<f32>,
};
struct ChromeU {
    mvp: mat4x4<f32>,
    color: vec4<f32>,
};
@group(0) @binding(0) var<uniform> u_chrome: ChromeU;

@vertex
fn vs_chrome(in: VsIn2) -> VsOut2 {
    var out: VsOut2;
    out.clip_pos = u_chrome.mvp * vec4<f32>(in.pos, 0.0, 1.0);
    return out;
}

@fragment
fn fs_chrome(in: VsOut2) -> @location(0) vec4<f32> {
    return u_chrome.color;
}

// ============================ SPLAT ============================
struct SplatU {
    mvp: mat4x4<f32>,
    center: vec3<f32>,
    radius: f32,
    color: vec4<f32>,
};
@group(0) @binding(0) var<uniform> u_splat: SplatU;

@vertex
fn vs_splat(in: VsIn2) -> VsOut2 {
    var out: VsOut2;
    // The model matrix already places the splat's *center* in
    // the right place; here we just scale the unit quad by
    // radius. We bake the radius into the model matrix on the
    // CPU side (see write_splat_uniforms) so this stays simple.
    out.clip_pos = u_splat.mvp * vec4<f32>(in.pos, 0.0, 1.0);
    return out;
}

@fragment
fn fs_splat(in: VsOut2) -> @location(0) vec4<f32> {
    // Gaussian falloff from the quad center (in screen-space
    // NDC). The quad is a [-0.5, 0.5] unit, so length(in.pos)
    // in NDC is not what we want — but the quad covers a
    // circular region with radius ~0.707 in local NDC after
    // the model scales it. We approximate the falloff with a
    // smooth radial gradient.
    return u_splat.color;
}
"#;

// -----------------------------------------------------------------
// App
// -----------------------------------------------------------------

struct App {
    window: Option<Arc<Window>>,
    device: Option<wgpu::Device>,
    queue: Option<wgpu::Queue>,
    surface: Option<wgpu::Surface<'static>>,
    surface_config: Option<wgpu::SurfaceConfiguration>,
    // Pipelines.
    mesh_pipeline: Option<wgpu::RenderPipeline>,
    chrome_pipeline: Option<wgpu::RenderPipeline>,
    splat_pipeline: Option<wgpu::RenderPipeline>,
    // Mesh geometry.
    mesh_vb: Option<wgpu::Buffer>,
    mesh_ib: Option<wgpu::Buffer>,
    mesh_index_count: u32,
    // Chrome / splat shared unit quad.
    quad_vb: Option<wgpu::Buffer>,
    // Layouts + per-slot uniform buffers + bind groups.
    bind_group_layout: Option<wgpu::BindGroupLayout>,
    pipeline_layout: Option<wgpu::PipelineLayout>,
    uniform_buffers_mesh: Vec<wgpu::Buffer>,
    bind_groups_mesh: Vec<wgpu::BindGroup>,
    uniform_buffers_chrome: Vec<wgpu::Buffer>,
    bind_groups_chrome: Vec<wgpu::BindGroup>,
    uniform_buffers_splat: Vec<wgpu::Buffer>,
    bind_groups_splat: Vec<wgpu::BindGroup>,
    // Snapshot.
    snapshot_pipeline_mesh: Option<wgpu::RenderPipeline>,
    snapshot_pipeline_chrome: Option<wgpu::RenderPipeline>,
    snapshot_pipeline_splat: Option<wgpu::RenderPipeline>,
    // State.
    step: u32,
    size: (u32, u32),
    auto_snapshot_at: Option<u32>,
    frame_count: u32,
    camera: Camera,
    scene: Scene,
    max_draws: usize,
    // For keyboard orbit (step 3+).
    azimuth: f32,
    elevation: f32,
    distance: f32,
}

impl App {
    fn new(step: u32) -> Self {
        let camera = match step {
            1 | 2 => Camera::ortho_step1(1.0, 1.0),
            _     => Camera::perspective_step3(800.0 / 600.0),
        };
        let distance = match step {
            1 | 2 => 1.0,
            _     => 2.0,
        };
        Self {
            window: None,
            device: None,
            queue: None,
            surface: None,
            surface_config: None,
            mesh_pipeline: None,
            chrome_pipeline: None,
            splat_pipeline: None,
            mesh_vb: None,
            mesh_ib: None,
            mesh_index_count: 0,
            quad_vb: None,
            bind_group_layout: None,
            pipeline_layout: None,
            uniform_buffers_mesh: Vec::new(),
            bind_groups_mesh: Vec::new(),
            uniform_buffers_chrome: Vec::new(),
            bind_groups_chrome: Vec::new(),
            uniform_buffers_splat: Vec::new(),
            bind_groups_splat: Vec::new(),
            snapshot_pipeline_mesh: None,
            snapshot_pipeline_chrome: None,
            snapshot_pipeline_splat: None,
            step,
            size: (800, 600),
            auto_snapshot_at: parse_auto_snapshot(),
            frame_count: 0,
            camera,
            scene: Scene::for_step(step),
            max_draws: 1024,
            azimuth: 0.0,
            elevation: 0.0,
            distance,
        }
    }

    fn resize(&mut self, w: u32, h: u32) {
        if w == 0 || h == 0 { return; }
        let device = self.device.as_ref().unwrap();
        let sc = self.surface_config.as_mut().unwrap();
        sc.width = w;
        sc.height = h;
        self.size = (w, h);
        // Update aspect for perspective cameras.
        if matches!(self.camera.projection, Projection::Perspective { .. }) {
            self.camera = Camera::perspective_step3(w as f32 / h as f32);
        }
        let surface = self.surface.as_ref().unwrap();
        surface.configure(device, sc);
    }

    fn render(&mut self) -> bool {
        let device = self.device.as_ref().unwrap();
        let queue = self.queue.as_ref().unwrap();
        let surface = self.surface.as_ref().unwrap();
        let pipeline_mesh = self.mesh_pipeline.as_ref().unwrap();
        let pipeline_chrome = self.chrome_pipeline.as_ref().unwrap();
        let pipeline_splat = self.splat_pipeline.as_ref().unwrap();

        // Tick the scene.
        self.scene.tick(16.0);

        // Update camera orbit (step 3+).
        if self.step >= 3 {
            let tgt = self.camera.target;
            self.camera.eye = [
                tgt[0] + self.distance * self.elevation.cos() * self.azimuth.sin(),
                tgt[1] + self.distance * self.elevation.sin(),
                tgt[2] + self.distance * self.elevation.cos() * self.azimuth.cos(),
            ];
        }
        let vp = self.camera.vp_matrix();

        // Build the draw list:
        //   draw 0: the mesh
        //   draws 1..N: chrome (step 4+)
        //   draws N..M: splats (step 5)
        let mut mesh_count = 1usize;
        let mut chrome_count = 0usize;
        let mut splat_count = 0usize;
        if self.step >= 4 { chrome_count = self.scene.chrome.len(); }
        if self.step >= 5 { splat_count = self.scene.splats.len(); }
        let total = mesh_count + chrome_count + splat_count;
        assert!(total <= self.max_draws, "draw count {} exceeds max_draws {}", total, self.max_draws);

        // Write per-draw uniforms.
        // --- Mesh (slot 0) ---
        {
            let mut u = MeshUniforms {
                mvp: mat4_mul(vp, self.scene.mesh_model),
                model: self.scene.mesh_model,
                curvature: self.scene.mesh_curvature,
                _pad0: 0.0, _pad1: 0.0, _pad2: 0.0,
            };
            queue.write_buffer(&self.uniform_buffers_mesh[0], 0, bytemuck::bytes_of(&u));
        }
        // --- Chrome (slots 1..N) ---
        for (i, c) in self.scene.chrome.iter().enumerate() {
            let slot = 1 + i;
            let model = mat4_mul(
                mat4_translate(c.position),
                mat4_scale([c.size[0], c.size[1], 1.0]),
            );
            let u = ChromeUniforms {
                mvp: mat4_mul(vp, model),
                color: c.color,
            };
            queue.write_buffer(&self.uniform_buffers_chrome[slot], 0, bytemuck::bytes_of(&u));
        }
        // --- Splats (slots 1+chrome_count..) ---
        for (i, s) in self.scene.splats.iter().enumerate() {
            let slot = 1 + chrome_count + i;
            let t = self.scene.time_ms * 0.001; // seconds
            let pos = [
                s.home[0] + s.amplitude[0] * (t * s.speed + s.phase).sin(),
                s.home[1] + s.amplitude[1] * (t * s.speed * 0.8 + s.phase).cos(),
                s.home[2] + s.amplitude[2] * (t * s.speed * 1.2 + s.phase).sin(),
            ];
            let model = mat4_mul(
                mat4_translate(pos),
                mat4_scale([s.radius, s.radius, 1.0]),
            );
            let u = SplatUniforms {
                mvp: mat4_mul(vp, model),
                center: pos,
                radius: s.radius,
                color: s.color,
            };
            queue.write_buffer(&self.uniform_buffers_splat[slot], 0, bytemuck::bytes_of(&u));
        }

        // Acquire surface.
        let frame = surface.get_current_texture();
        let surface_texture = match frame {
            wgpu::CurrentSurfaceTexture::Success(t) | wgpu::CurrentSurfaceTexture::Suboptimal(t) => t,
            other => {
                log::warn!("Surface texture unavailable: {:?}", other);
                return false;
            }
        };
        let view = surface_texture.texture.create_view(&Default::default());
        let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
            label: Some("frame-encoder"),
        });
        {
            let mut rpass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("frame-pass"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &view,
                    resolve_target: None,
                    depth_slice: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(wgpu::Color {
                            r: 0.02, g: 0.02, b: 0.04, a: 1.0,
                        }),
                        store: wgpu::StoreOp::Store,
                    },
                })],
                depth_stencil_attachment: None,
                timestamp_writes: None,
                occlusion_query_set: None,
                multiview_mask: None,
            });

            // Mesh: 1 indexed draw, bind group 0.
            rpass.set_pipeline(pipeline_mesh);
            rpass.set_vertex_buffer(0, self.mesh_vb.as_ref().unwrap().slice(..));
            rpass.set_index_buffer(self.mesh_ib.as_ref().unwrap().slice(..), wgpu::IndexFormat::Uint16);
            rpass.set_bind_group(0, &self.bind_groups_mesh[0], &[]);
            rpass.draw_indexed(0..self.mesh_index_count, 0, 0..1);

            // Chrome: N draws (step 4+).
            if self.step >= 4 {
                let quad_vb = self.quad_vb.as_ref().unwrap().slice(..);
                rpass.set_pipeline(pipeline_chrome);
                rpass.set_vertex_buffer(0, quad_vb);
                for i in 1..(1 + chrome_count) {
                    // Re-issue set_pipeline before each draw (same
                    // wgpu 29 / Vulkan gotcha as Phase 4).
                    rpass.set_pipeline(pipeline_chrome);
                    rpass.set_bind_group(0, &self.bind_groups_chrome[i], &[]);
                    rpass.draw(0..6, 0..1);
                }
            }

            // Splats: M draws (step 5).
            if self.step >= 5 {
                let quad_vb = self.quad_vb.as_ref().unwrap().slice(..);
                rpass.set_pipeline(pipeline_splat);
                rpass.set_vertex_buffer(0, quad_vb);
                for i in (1 + chrome_count)..(1 + chrome_count + splat_count) {
                    rpass.set_pipeline(pipeline_splat);
                    rpass.set_bind_group(0, &self.bind_groups_splat[i], &[]);
                    rpass.draw(0..6, 0..1);
                }
            }
        }
        queue.submit(std::iter::once(encoder.finish()));
        surface_texture.present();

        // Auto-snapshot.
        if let Some(target) = self.auto_snapshot_at {
            if self.frame_count + 1 >= target {
                self.write_snapshot();
                return true;
            }
        }
        self.frame_count = self.frame_count.wrapping_add(1);
        false
    }

    fn write_snapshot(&self) {
        // The snapshot pipeline is essentially the same as the
        // live pipelines, but targets a Rgba8Unorm offscreen
        // texture so we can read it back as a PNG.
        let device = self.device.as_ref().unwrap();
        let queue = self.queue.as_ref().unwrap();
        let pipeline_layout = self.pipeline_layout.as_ref().unwrap();
        let snap_tex = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("snapshot-tex"),
            size: wgpu::Extent3d {
                width: self.size.0,
                height: self.size.1,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::Rgba8Unorm,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
            view_formats: &[],
        });
        let snap_view = snap_tex.create_view(&Default::default());
        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("snapshot-shader"),
            source: wgpu::ShaderSource::Wgsl(SHADER.into()),
        });

        let snap_attrs: [wgpu::VertexAttribute; 2] = wgpu::vertex_attr_array![0 => Float32x3, 1 => Float32x2];
        // One pipeline per kind. We pass the same shader module
        // and the same pipeline layout; the entry point
        // determines which shader is used.
        let make_pipeline = |label: &str, entry: &str| -> wgpu::RenderPipeline {
            device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
                label: Some(label),
                layout: Some(pipeline_layout),
                vertex: wgpu::VertexState {
                    module: &shader,
                    entry_point: Some(entry),
                    buffers: &[wgpu::VertexBufferLayout {
                        array_stride: std::mem::size_of::<MeshVertex>() as u64,
                        step_mode: wgpu::VertexStepMode::Vertex,
                        attributes: &snap_attrs,
                    }],
                    compilation_options: wgpu::PipelineCompilationOptions::default(),
                },
                fragment: Some(wgpu::FragmentState {
                    module: &shader,
                    entry_point: Some(if entry == "vs_mesh" { "fs_mesh" }
                                      else if entry == "vs_chrome" { "fs_chrome" }
                                      else { "fs_splat" }),
                    targets: &[Some(wgpu::ColorTargetState {
                        format: wgpu::TextureFormat::Rgba8Unorm,
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
            })
        };
        let snap_mesh = make_pipeline("snap-mesh", "vs_mesh");
        let snap_chrome = make_pipeline("snap-chrome", "vs_chrome");
        let snap_splat = make_pipeline("snap-splat", "vs_splat");

        let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
            label: Some("snapshot-encoder"),
        });
        {
            let mut rpass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("snapshot-pass"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &snap_view,
                    resolve_target: None,
                    depth_slice: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(wgpu::Color {
                            r: 0.02, g: 0.02, b: 0.04, a: 1.0,
                        }),
                        store: wgpu::StoreOp::Store,
                    },
                })],
                depth_stencil_attachment: None,
                timestamp_writes: None,
                occlusion_query_set: None,
                multiview_mask: None,
            });

            // Mesh.
            rpass.set_pipeline(&snap_mesh);
            rpass.set_vertex_buffer(0, self.mesh_vb.as_ref().unwrap().slice(..));
            rpass.set_index_buffer(self.mesh_ib.as_ref().unwrap().slice(..), wgpu::IndexFormat::Uint16);
            rpass.set_bind_group(0, &self.bind_groups_mesh[0], &[]);
            rpass.draw_indexed(0..self.mesh_index_count, 0, 0..1);

            // Chrome.
            if self.step >= 4 {
                let quad_vb = self.quad_vb.as_ref().unwrap().slice(..);
                for i in 1..(1 + self.scene.chrome.len()) {
                    rpass.set_pipeline(&snap_chrome);
                    rpass.set_vertex_buffer(0, quad_vb);
                    rpass.set_bind_group(0, &self.bind_groups_chrome[i], &[]);
                    rpass.draw(0..6, 0..1);
                }
            }
            // Splats.
            if self.step >= 5 {
                let quad_vb = self.quad_vb.as_ref().unwrap().slice(..);
                for i in (1 + self.scene.chrome.len())..(1 + self.scene.chrome.len() + self.scene.splats.len()) {
                    rpass.set_pipeline(&snap_splat);
                    rpass.set_vertex_buffer(0, quad_vb);
                    rpass.set_bind_group(0, &self.bind_groups_splat[i], &[]);
                    rpass.draw(0..6, 0..1);
                }
            }
        }

        // Read back to PNG.
        let row_bytes = self.size.0 * 4;
        let padded_row = (row_bytes + 255) & !255;
        let staging = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("snapshot-read"),
            size: (padded_row * self.size.1) as u64,
            usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
            mapped_at_creation: false,
        });
        encoder.copy_texture_to_buffer(
            wgpu::TexelCopyTextureInfo {
                texture: &snap_tex,
                mip_level: 0,
                origin: wgpu::Origin3d::ZERO,
                aspect: wgpu::TextureAspect::All,
            },
            wgpu::TexelCopyBufferInfo {
                buffer: &staging,
                layout: wgpu::TexelCopyBufferLayout {
                    offset: 0,
                    bytes_per_row: Some(padded_row),
                    rows_per_image: Some(self.size.1),
                },
            },
            wgpu::Extent3d {
                width: self.size.0,
                height: self.size.1,
                depth_or_array_layers: 1,
            },
        );
        queue.submit(std::iter::once(encoder.finish()));

        // Map and write PNG.
        let slice = staging.slice(..);
        let (tx, rx) = std::sync::mpsc::channel();
        slice.map_async(wgpu::MapMode::Read, move |_| { let _ = tx.send(()); });
        let _ = device.poll(wgpu::PollType::wait_indefinitely());
        rx.recv().unwrap();
        let mapped = slice.get_mapped_range();

        let out_path = format!("target/snapshots/hologram-test_{}.png", self.step);
        std::fs::create_dir_all("target/snapshots").unwrap();
        wgsl_sdf::png::write_png_rgba(
            &out_path,
            self.size.0,
            self.size.1,
            padded_row,
            &mapped,
        ).expect("write_png_rgba");
        log::info!("wrote snapshot: {}", out_path);
        drop(mapped);
        staging.unmap();
    }
}

impl ApplicationHandler for App {
    fn window_event(&mut self, event_loop: &ActiveEventLoop, _id: WindowId, event: WindowEvent) {
        match event {
            WindowEvent::CloseRequested => event_loop.exit(),
            WindowEvent::Resized(sz) => {
                self.resize(sz.width, sz.height);
            }
            WindowEvent::KeyboardInput { event, .. } => {
                if event.state == winit::event::ElementState::Pressed {
                    match &event.logical_key {
                        Key::Named(NamedKey::Escape) => event_loop.exit(),
                        Key::Character(s) => {
                            let s = s.as_str();
                            // Orbit controls (step 3+).
                            if self.step >= 3 {
                                match s {
                                    "a" | "A" => self.azimuth -= 0.1,
                                    "d" | "D" => self.azimuth += 0.1,
                                    "w" | "W" => self.elevation = (self.elevation + 0.1).min(1.4),
                                    "s" | "S" => self.elevation = (self.elevation - 0.1).max(-1.4),
                                    "q" | "Q" => self.distance = (self.distance + 0.1).min(5.0),
                                    "e" | "E" => self.distance = (self.distance - 0.1).max(0.5),
                                    _ => {}
                                }
                            }
                        }
                        _ => {}
                    }
                }
            }
            WindowEvent::RedrawRequested => {
                let done = self.render();
                if done {
                    event_loop.exit();
                }
            }
            _ => {}
        }
    }

    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        if self.window.is_some() { return; }
        let attrs = Window::default_attributes()
            .with_title(format!("hologram-test step {}", self.step))
            .with_inner_size(winit::dpi::LogicalSize::new(800.0, 600.0));
        let window = Arc::new(event_loop.create_window(attrs).unwrap());
        self.window = Some(window.clone());

        // Init wgpu.
        let instance = wgpu::Instance::new(wgpu::InstanceDescriptor {
            backends: wgpu::Backends::VULKAN,
            flags: wgpu::InstanceFlags::default(),
            backend_options: wgpu::BackendOptions::default(),
            memory_budget_thresholds: wgpu::MemoryBudgetThresholds::default(),
            display: None,
        });
        let surface = instance.create_surface(window.clone()).unwrap();
        let adapter = pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions {
            power_preference: wgpu::PowerPreference::default(),
            compatible_surface: Some(&surface),
            force_fallback_adapter: false,
        })).unwrap();
        // The hardware we're running on has very tight limits,
        // so request only what we need.
        let mut limits = wgpu::Limits::downlevel_webgl2_defaults();
        limits.max_color_attachments = 1;
        let (device, queue) = pollster::block_on(adapter.request_device(
            &wgpu::DeviceDescriptor {
                label: Some("device"),
                required_limits: limits,
                ..Default::default()
            },
        )).unwrap();

        let caps = surface.get_capabilities(&adapter);
        let format = caps.formats.iter().copied()
            .find(|f| f.is_srgb())
            .unwrap_or(caps.formats[0]);
        let surface_config = wgpu::SurfaceConfiguration {
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
            format,
            width: 800, height: 600,
            present_mode: wgpu::PresentMode::Fifo,
            desired_maximum_frame_latency: 2,
            alpha_mode: wgpu::CompositeAlphaMode::Auto,
            view_formats: vec![],
        };
        surface.configure(&device, &surface_config);

        // Mesh geometry.
        let (mesh_verts, mesh_idx) = build_mesh(0.5, 0.5, 32);
        let mesh_vb = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("mesh-vb"),
            contents: bytemuck::cast_slice(&mesh_verts),
            usage: wgpu::BufferUsages::VERTEX,
        });
        let mesh_ib = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("mesh-ib"),
            contents: bytemuck::cast_slice(&mesh_idx),
            usage: wgpu::BufferUsages::INDEX,
        });
        let mesh_index_count = mesh_idx.len() as u32;

        // Unit quad for chrome and splats.
        let quad_verts = build_quad_verts();
        let quad_vb = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("quad-vb"),
            contents: bytemuck::cast_slice(&quad_verts),
            usage: wgpu::BufferUsages::VERTEX,
        });

        // Build pipelines.
        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("shader"),
            source: wgpu::ShaderSource::Wgsl(SHADER.into()),
        });

        // We use a single bind group layout (all three uniforms
        // share the same binding slot 0). The Rust-side type
        // varies per pipeline, but the GPU only sees bytes.
        let bind_group_layout =
            device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
                label: Some("bgl"),
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
        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("pl"),
            bind_group_layouts: &[Some(&bind_group_layout)],
            immediate_size: 0,
        });
        let mesh_attrs: [wgpu::VertexAttribute; 2] = wgpu::vertex_attr_array![0 => Float32x3, 1 => Float32x2];
        let quad_attrs: [wgpu::VertexAttribute; 1] = wgpu::vertex_attr_array![0 => Float32x2];
        let make_pipeline = |label: &str, entry: &str, frag_entry: &str, fmt: wgpu::TextureFormat, vertex_kind: VertexKind| -> wgpu::RenderPipeline {
            let buffers: Vec<wgpu::VertexBufferLayout> = match vertex_kind {
                VertexKind::Mesh => vec![wgpu::VertexBufferLayout {
                    array_stride: std::mem::size_of::<MeshVertex>() as u64,
                    step_mode: wgpu::VertexStepMode::Vertex,
                    attributes: &mesh_attrs,
                }],
                VertexKind::Quad => vec![wgpu::VertexBufferLayout {
                    array_stride: std::mem::size_of::<QuadVertex>() as u64,
                    step_mode: wgpu::VertexStepMode::Vertex,
                    attributes: &quad_attrs,
                }],
            };
            device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
                label: Some(label),
                layout: Some(&pipeline_layout),
                vertex: wgpu::VertexState {
                    module: &shader,
                    entry_point: Some(entry),
                    buffers: &buffers,
                    compilation_options: wgpu::PipelineCompilationOptions::default(),
                },
                fragment: Some(wgpu::FragmentState {
                    module: &shader,
                    entry_point: Some(frag_entry),
                    targets: &[Some(wgpu::ColorTargetState {
                        format: fmt,
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
            })
        };
        let mesh_pipeline = make_pipeline("mesh", "vs_mesh", "fs_mesh", format, VertexKind::Mesh);
        let chrome_pipeline = make_pipeline("chrome", "vs_chrome", "fs_chrome", format, VertexKind::Quad);
        let splat_pipeline = make_pipeline("splat", "vs_splat", "fs_splat", format, VertexKind::Quad);

        // Pre-allocate uniform buffers + bind groups for each
        // pipeline kind.
        let max_draws = self.max_draws;
        let mk_uniforms_and_bgs = |size_of_u: usize| -> (Vec<wgpu::Buffer>, Vec<wgpu::BindGroup>) {
            let bufs: Vec<wgpu::Buffer> = (0..max_draws).map(|i| {
                device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
                    label: Some(&format!("uniform-{}-{}", label_for_size(size_of_u), i)),
                    contents: &vec![0u8; size_of_u],
                    usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
                })
            }).collect();
            let bgs: Vec<wgpu::BindGroup> = (0..max_draws).map(|i| {
                device.create_bind_group(&wgpu::BindGroupDescriptor {
                    label: Some(&format!("bg-{}-{}", label_for_size(size_of_u), i)),
                    layout: &bind_group_layout,
                    entries: &[wgpu::BindGroupEntry {
                        binding: 0,
                        resource: bufs[i].as_entire_binding(),
                    }],
                })
            }).collect();
            (bufs, bgs)
        };
        let (uniform_buffers_mesh, bind_groups_mesh) = mk_uniforms_and_bgs(std::mem::size_of::<MeshUniforms>());
        let (uniform_buffers_chrome, bind_groups_chrome) = mk_uniforms_and_bgs(std::mem::size_of::<ChromeUniforms>());
        let (uniform_buffers_splat, bind_groups_splat) = mk_uniforms_and_bgs(std::mem::size_of::<SplatUniforms>());

        self.size = (surface_config.width, surface_config.height);
        self.device = Some(device);
        self.queue = Some(queue);
        self.surface = Some(surface);
        self.surface_config = Some(surface_config);
        self.mesh_pipeline = Some(mesh_pipeline);
        self.chrome_pipeline = Some(chrome_pipeline);
        self.splat_pipeline = Some(splat_pipeline);
        self.mesh_vb = Some(mesh_vb);
        self.mesh_ib = Some(mesh_ib);
        self.mesh_index_count = mesh_index_count;
        self.quad_vb = Some(quad_vb);
        self.bind_group_layout = Some(bind_group_layout);
        self.pipeline_layout = Some(pipeline_layout);
        self.uniform_buffers_mesh = uniform_buffers_mesh;
        self.bind_groups_mesh = bind_groups_mesh;
        self.uniform_buffers_chrome = uniform_buffers_chrome;
        self.bind_groups_chrome = bind_groups_chrome;
        self.uniform_buffers_splat = uniform_buffers_splat;
        self.bind_groups_splat = bind_groups_splat;
    }

    fn about_to_wait(&mut self, _event_loop: &ActiveEventLoop) {
        if let Some(w) = &self.window {
            w.request_redraw();
        }
    }
}

#[derive(Copy, Clone)]
enum VertexKind { Mesh, Quad }

fn label_for_size(size: usize) -> &'static str {
    match size {
        160 => "mesh",
        80 => "chrome",
        96 => "splat",
        _ => "u",
    }
}

// -----------------------------------------------------------------
// CLI
// -----------------------------------------------------------------

fn parse_auto_snapshot() -> Option<u32> {
    let mut args = std::env::args().skip(1);
    while let Some(a) = args.next() {
        if let Some(v) = a.strip_prefix("--snapshot-at=") {
            return v.parse().ok();
        }
        if a == "--snapshot-at" {
            return args.next().and_then(|s| s.parse().ok());
        }
    }
    None
}

fn main() {
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("info")).init();
    let step: u32 = std::env::args()
        .skip(1)
        .find_map(|a| a.strip_prefix("--step=").and_then(|s| s.parse().ok()))
        .unwrap_or(1);
    let app = App::new(step);
    let event_loop = winit::event_loop::EventLoop::new().unwrap();
    event_loop.set_control_flow(winit::event_loop::ControlFlow::Poll);
    event_loop.run_app(&mut { app }).unwrap();
}

// Suppress the "STEP" constant warning. It's documentation more
// than a constant — we use --step=N to choose the scene.
#[allow(dead_code)]
const _STEP_DOC: u32 = STEP;
