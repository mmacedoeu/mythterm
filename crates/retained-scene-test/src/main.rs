//! retained-scene-test (Phase 4 prototype)
//!
//! A minimal retained scene graph that proves the Phase 4
//! architecture: every visible thing is a Node, animations
//! are Components, state is Data, and the renderer walks the
//! graph to emit draw calls.
//!
//! Run:
//!   ./target/release/retained-scene-test --step=1
//!   ./target/release/retained-scene-test --step=2
//!   ./target/release/retained-scene-test --step=3
//!   ./target/release/retained-scene-test --step=4
//!
//! Snapshot:
//!   S    : snapshot current frame to target/snapshots/retained-scene-test_<step>.png
//!
//! The test binary is intentionally self-contained. It does not
//! depend on `myth` — this is the prototype that informs how
//! `myth` will be wired into MythTerm. The scene graph shape
//! here is what we'll integrate in Phase 7.

use std::sync::Arc;

use winit::application::ApplicationHandler;
use winit::event::WindowEvent;
use winit::event_loop::ActiveEventLoop;
use winit::keyboard::{Key, NamedKey};
use winit::window::{Window, WindowId};
use wgpu::util::DeviceExt;

// Re-use the snapshot pipeline + auto-snapshot machinery.
use wgsl_sdf::png::write_png_rgba;

// -----------------------------------------------------------------
// Scene graph
// -----------------------------------------------------------------

/// 2D affine transform, encoded as a 3x3 matrix stored row-major
/// (so it can be uploaded as a uniform and applied with
/// `transform * vec3(pos, 1)`).
#[derive(Copy, Clone, Debug)]
struct Affine2 {
    // Row-major 2x3 matrix:
    //   [a, b, tx]
    //   [c, d, ty]
    a: f32,
    b: f32,
    tx: f32,
    c: f32,
    d: f32,
    ty: f32,
}

impl Affine2 {
    const IDENTITY: Self = Self {
        a: 1.0, b: 0.0, tx: 0.0,
        c: 0.0, d: 1.0, ty: 0.0,
    };

    fn translate(x: f32, y: f32) -> Self {
        Self { tx: x, ty: y, ..Self::IDENTITY }
    }

    #[allow(dead_code)]
    fn scale(s: f32) -> Self {
        Self { a: s, d: s, ..Self::IDENTITY }
    }

    fn scale_xy(sx: f32, sy: f32) -> Self {
        Self { a: sx, d: sy, ..Self::IDENTITY }
    }

    /// Compose: `self * other`, i.e. apply `other` first then `self`.
    fn compose(&self, other: &Self) -> Self {
        Self {
            a:  self.a * other.a + self.b * other.c,
            b:  self.a * other.b + self.b * other.d,
            tx: self.a * other.tx + self.b * other.ty + self.tx,
            c:  self.c * other.a + self.d * other.c,
            d:  self.c * other.b + self.d * other.d,
            ty: self.c * other.tx + self.d * other.ty + self.ty,
        }
    }
}

/// What kind of leaf this node is.
#[derive(Clone, Debug)]
enum NodeKind {
    /// A solid-color quad. The vertex shader applies the
    /// accumulated transform.
    Quad { color: [f32; 4] },
    /// A radial gaussian (used for the "splat glow" in step 4).
    /// The quad is 1×1 in local space; the fragment shader
    /// produces a soft falloff. The transform positions it.
    Splat { color: [f32; 3], intensity: f32, falloff: f32 },
    /// A stylised text bar — for the test, a row of pixel-like
    /// dots representing a label. (Real font rendering is
    /// deferred to Phase 4 integration with `mythterm-font`.)
    Text { color: [f32; 4], label: String },
    /// A pure container. The accumulator is applied to children.
    Group,
}

#[derive(Clone, Debug)]
struct SceneNode {
    kind: NodeKind,
    /// Local transform. Composed with the parent's accumulated
    /// transform during render walk.
    transform: Affine2,
    /// Hover animation, 0 = inactive, 1 = active. Animated by
    /// the Scene each frame.
    hover: f32,
    /// Focus (active tab) state, 0 = inactive, 1 = active.
    focus: f32,
    children: Vec<SceneNode>,
}

impl SceneNode {
    fn quad(color: [f32; 4], transform: Affine2) -> Self {
        Self {
            kind: NodeKind::Quad { color },
            transform,
            hover: 0.0,
            focus: 0.0,
            children: vec![],
        }
    }

    fn splat(color: [f32; 3], transform: Affine2) -> Self {
        Self {
            kind: NodeKind::Splat { color, intensity: 1.0, falloff: 4.0 },
            transform,
            hover: 0.0,
            focus: 0.0,
            children: vec![],
        }
    }

    fn text(color: [f32; 4], transform: Affine2, label: impl Into<String>) -> Self {
        Self {
            kind: NodeKind::Text { color, label: label.into() },
            transform,
            hover: 0.0,
            focus: 0.0,
            children: vec![],
        }
    }

    fn group(transform: Affine2, children: Vec<SceneNode>) -> Self {
        Self {
            kind: NodeKind::Group,
            transform,
            hover: 0.0,
            focus: 0.0,
            children,
        }
    }
}

// -----------------------------------------------------------------
// Scene
// -----------------------------------------------------------------

/// The scene has a list of "active" tab ids; the renderer uses
/// that to drive `focus` and `hover` components.
struct Scene {
    root: SceneNode,
    /// Index of the active tab.
    active_tab: usize,
    /// Index of the hovered tab (None if no hover).
    hovered_tab: Option<usize>,
    /// Frame counter, used to seed deterministic hover/focus
    /// animations in the test snapshots.
    frame: u32,
}

impl Scene {
    fn for_step(step: u32) -> Self {
        match step {
            1 => Self::step1_flat_quads(),
            2 => Self::step2_nested_groups(),
            3 => Self::step3_tab_hover(),
            4 => Self::step4_splat_glow(),
            _ => Self::step1_flat_quads(),
        }
    }

    /// Step 1: 5 colored quads in a row. Each has its own
    /// transform. No nesting.
    fn step1_flat_quads() -> Self {
        let colors = [
            [0.8, 0.2, 0.2, 1.0],
            [0.2, 0.8, 0.2, 1.0],
            [0.2, 0.2, 0.8, 1.0],
            [0.8, 0.8, 0.2, 1.0],
            [0.8, 0.2, 0.8, 1.0],
        ];
        let mut quads = Vec::new();
        for (i, c) in colors.iter().enumerate() {
            // Each quad is 0.15 wide, centered at x = -0.6 + i * 0.3.
            let x = -0.6 + i as f32 * 0.3;
            let t = Affine2::translate(x, 0.0).compose(&Affine2::scale_xy(0.13, 0.4));
            quads.push(SceneNode::quad(*c, t));
        }
        Self {
            root: SceneNode::group(Affine2::IDENTITY, quads),
            active_tab: 0,
            hovered_tab: None,
            frame: 0,
        }
    }

    /// Step 2: nested groups. A parent "header" group contains
    /// a label, a divider, and 3 sub-quads, each in their own
    /// child group.
    fn step2_nested_groups() -> Self {
        // The root has a header (top strip) and 3 columns.
        let header = SceneNode::group(
            Affine2::translate(0.0, 0.7).compose(&Affine2::scale_xy(1.6, 0.1)),
            vec![
                SceneNode::quad([0.15, 0.15, 0.18, 1.0], Affine2::IDENTITY),
                SceneNode::text(
                    [0.9, 0.9, 0.9, 1.0],
                    Affine2::translate(-0.95, 0.0).compose(&Affine2::scale_xy(0.005, 0.04)),
                    "RETAINED",
                ),
            ],
        );

        // 3 columns, each is a Group with a header + body.
        let mut columns = Vec::new();
        let col_colors = [
            [0.4, 0.5, 0.9, 1.0],
            [0.9, 0.4, 0.5, 1.0],
            [0.4, 0.9, 0.5, 1.0],
        ];
        for (i, c) in col_colors.iter().enumerate() {
            let x = -0.5 + i as f32 * 0.5;
            // Each column is a group: header (top) + body (bottom).
            let header = SceneNode::quad(*c, Affine2::translate(0.0, 0.4).compose(&Affine2::scale_xy(0.32, 0.06)));
            let body = SceneNode::quad(
                [c[0] * 0.5, c[1] * 0.5, c[2] * 0.5, 1.0],
                Affine2::translate(0.0, 0.0).compose(&Affine2::scale_xy(0.32, 0.5)),
            );
            let col = SceneNode::group(
                Affine2::translate(x, 0.0),
                vec![header, body],
            );
            columns.push(col);
        }

        Self {
            root: SceneNode::group(Affine2::IDENTITY, vec![header, SceneNode::group(Affine2::IDENTITY, columns)]),
            active_tab: 0,
            hovered_tab: None,
            frame: 0,
        }
    }

    /// Step 3: 5 tabs in a row. Hover animation drives a 200ms
    /// ease-out-cubic on the hover state. The active tab has
    /// focus=1.
    fn step3_tab_hover() -> Self {
        let n_tabs = 5;
        let tab_w = 0.32;
        let gap = 0.04;
        let total_w = n_tabs as f32 * tab_w + (n_tabs as f32 - 1.0) * gap;
        let x0 = -total_w * 0.5 + tab_w * 0.5;

        let mut tabs = Vec::new();
        for i in 0..n_tabs {
            let x = x0 + i as f32 * (tab_w + gap);
            // Each tab is a Group containing:
            //   - background Quad
            //   - label Text
            let mut tab = SceneNode::group(
                Affine2::translate(x, 0.6),
                vec![
                    SceneNode::quad(
                        [0.2, 0.22, 0.28, 1.0],
                        Affine2::scale_xy(tab_w * 0.5, 0.06),
                    ),
                    SceneNode::text(
                        [0.9, 0.9, 0.9, 1.0],
                        Affine2::scale_xy(0.005, 0.025),
                        format!("TAB{}", i + 1),
                    ),
                ],
            );
            // Initial state: active tab has focus, others have
            // neither hover nor focus.
            if i == 0 {
                tab.focus = 1.0;
                tab.children[0].focus = 1.0;
            }
            tabs.push(tab);
        }

        Self {
            root: SceneNode::group(Affine2::IDENTITY, tabs),
            active_tab: 0,
            // For a deterministic snapshot, the second tab is
            // "hovered" (animation playing, at frame 2 -> t = 32/200
            // = 0.16 -> ease_out_cubic(0.16) ≈ 0.41).
            hovered_tab: Some(1),
            frame: 0,
        }
    }

    /// Step 4: same as step 3, but the active tab has a Splat
    /// child attached for the glow effect.
    fn step4_splat_glow() -> Self {
        let mut scene = Self::step3_tab_hover();
        // Splat for the active tab: a soft red glow behind the
        // background. Attached as the first child of the active
        // tab group so it renders *under* the background quad.
        let splat = SceneNode::splat(
            [1.0, 0.3, 0.4],
            Affine2::scale_xy(0.5, 0.15),
        );
        scene.root.children[0].children.insert(0, splat);
        scene
    }

    /// Advance the scene's animation state by one frame.
    /// `frame_dt_ms` is the per-frame delta. With
    /// `frame_dt_ms = 16` (60fps) and a 200ms animation,
    /// the hover converges in ~12 frames.
    fn tick(&mut self, frame_dt_ms: f32) {
        self.frame = self.frame.wrapping_add(1);

        // Walk the tree, animate hover. We use a manual stack
        // to avoid borrowing issues.
        let target_hover = |idx: usize| -> f32 {
            if let Some(h) = self.hovered_tab {
                if h == idx { 1.0 } else { 0.0 }
            } else {
                0.0
            }
        };
        let target_focus = |idx: usize| -> f32 {
            if idx == self.active_tab { 1.0 } else { 0.0 }
        };
        let ease_out_cubic = |t: f32| -> f32 {
            let inv = 1.0 - t;
            1.0 - inv * inv * inv
        };
        let hover_speed = frame_dt_ms / 200.0; // 200ms animation
        let focus_speed = frame_dt_ms / 200.0;

        for (i, tab) in self.root.children.iter_mut().enumerate() {
            // Tab itself
            let target = target_hover(i);
            let anim_t = (target - tab.hover).abs().min(hover_speed);
            let sign = if target > tab.hover { 1.0 } else { -1.0 };
            tab.hover = (tab.hover + sign * anim_t).clamp(0.0, 1.0);
            let target_f = target_focus(i);
            let anim_f = (target_f - tab.focus).abs().min(focus_speed);
            let sign_f = if target_f > tab.focus { 1.0 } else { -1.0 };
            tab.focus = (tab.focus + sign_f * anim_f).clamp(0.0, 1.0);
            // Propagate to children
            for child in tab.children.iter_mut() {
                child.hover = ease_out_cubic(tab.hover);
                child.focus = tab.focus;
            }
        }
    }
}

// -----------------------------------------------------------------
// Draw list (output of the scene walk)
// -----------------------------------------------------------------

/// A single draw call, produced by walking the scene.
struct DrawCmd {
    /// Final world transform (parent composed with local).
    transform: Affine2,
    kind: DrawKind,
    /// Hover state of this node (0..=1), for material effects.
    hover: f32,
    /// Focus state of this node (0..=1).
    focus: f32,
}

#[derive(Clone)]
enum DrawKind {
    Quad { color: [f32; 4] },
    Splat { color: [f32; 3], intensity: f32, falloff: f32 },
    /// For text, we draw N small dots (one per character) as
    /// sub-quads. To keep the test simple, we just produce a
    /// "label bar" (a row of small white squares) at the
    /// node's position.
    Text { color: [f32; 4], char_count: usize },
}

fn walk(node: &SceneNode, parent_xform: &Affine2, out: &mut Vec<DrawCmd>) {
    let xform = parent_xform.compose(&node.transform);
    match &node.kind {
        NodeKind::Quad { color } => out.push(DrawCmd {
            transform: xform,
            kind: DrawKind::Quad { color: *color },
            hover: node.hover,
            focus: node.focus,
        }),
        NodeKind::Splat { color, intensity, falloff } => out.push(DrawCmd {
            transform: xform,
            kind: DrawKind::Splat { color: *color, intensity: *intensity, falloff: *falloff },
            hover: node.hover,
            focus: node.focus,
        }),
        NodeKind::Text { color, label } => out.push(DrawCmd {
            transform: xform,
            kind: DrawKind::Text { color: *color, char_count: label.len() },
            hover: node.hover,
            focus: node.focus,
        }),
        NodeKind::Group => {}
    }
    for c in &node.children {
        walk(c, &xform, out);
    }
}

// -----------------------------------------------------------------
// Shader
// -----------------------------------------------------------------

const SHADER: &str = r#"
struct VsIn {
    @location(0) pos: vec2<f32>,
}
struct VsOut {
    @builtin(position) clip_pos: vec4<f32>,
    @location(0) local: vec2<f32>,
}

@group(0) @binding(0) var<uniform> u: Uniforms;

@vertex
fn vs_main(in: VsIn) -> VsOut {
    // Apply 2D affine. `in.pos` is in [-0.5, 0.5].
    let p = vec3<f32>(in.pos, 1.0);
    let world = vec2<f32>(
        u.a * p.x + u.b * p.y + u.tx,
        u.c * p.x + u.d * p.y + u.ty
    );
    var out: VsOut;
    out.clip_pos = vec4<f32>(world, 0.0, 1.0);
    out.local = in.pos * 2.0;  // [-1, 1] in local quad space
    return out;
}

@fragment
fn fs_main(in: VsOut) -> @location(0) vec4<f32> {
    if u.is_splat > 0.5 {
        // Radial gaussian: e^(-falloff * r^2).
        let r2 = dot(in.local, in.local);
        let g = exp(-u.splat_falloff * r2);
        let c = vec3<f32>(u.splat_color_r, u.splat_color_g, u.splat_color_b);
        return vec4<f32>(c * g * u.splat_intensity, g);
    }
    if u.is_text > 0.5 {
        // A row of N small white dots. The quad is unit, so
        // a dot is `0.05 / char_count` wide.
        let n = max(u.text_chars, 1.0);
        let dot_w = 0.6 / n;
        let x = in.local.x * 0.5 + 0.5; // [0, 1]
        let slot = floor(x * n);
        let center = (slot + 0.5) / n;
        let dx = (x - center) / dot_w;
        if abs(dx) > 0.5 {
            discard;
        }
        return vec4<f32>(u.text_color_r, u.text_color_g, u.text_color_b, u.text_color_a);
    }
    return vec4<f32>(u.color_r, u.color_g, u.color_b, u.color_a);
}
"#;

#[repr(C)]
#[derive(Copy, Clone, Debug, bytemuck::Pod, bytemuck::Zeroable)]
struct Uniforms {
    a: f32, b: f32, tx: f32,
    c: f32, d: f32, ty: f32,
    color: [f32; 4],
    splat_color: [f32; 3],
    splat_intensity: f32,
    splat_falloff: f32,
    is_splat: f32,
    is_text: f32,
    text_color: [f32; 4],
    text_chars: f32,
    _pad: f32,
}

impl Default for Uniforms {
    fn default() -> Self {
        Self {
            a: 1.0, b: 0.0, tx: 0.0,
            c: 0.0, d: 1.0, ty: 0.0,
            color: [1.0; 4],
            splat_color: [1.0; 3],
            splat_intensity: 1.0,
            splat_falloff: 4.0,
            is_splat: 0.0,
            is_text: 0.0,
            text_color: [1.0; 4],
            text_chars: 0.0,
            _pad: 0.0,
        }
    }
}

// -----------------------------------------------------------------
// WGSL struct declaration. Uses f32 scalars throughout to
// match Rust's #[repr(C)] dense layout (WGSL would otherwise
// align vec3/vec4 to 16 bytes, making the struct 112 bytes
// vs. Rust's 92 bytes).
// -----------------------------------------------------------------
const UNIFORMS_DECL: &str = r#"
struct Uniforms {
    a: f32, b: f32, tx: f32,
    c: f32, d: f32, ty: f32,
    color_r: f32, color_g: f32, color_b: f32, color_a: f32,
    splat_color_r: f32, splat_color_g: f32, splat_color_b: f32,
    splat_intensity: f32,
    splat_falloff: f32,
    is_splat: f32,
    is_text: f32,
    text_color_r: f32, text_color_g: f32, text_color_b: f32, text_color_a: f32,
    text_chars: f32,
    _pad: f32,
}

// Ensure the WGSL struct stays in sync with the Rust
// `#[repr(C)]` struct. If the WGSL side ever drifts to a
// different size, this const_assert will fail at shader
// compile time.
const_assert(sizeof(Uniforms) == 92);
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
    pipeline: Option<wgpu::RenderPipeline>,
    vertex_buffer: Option<wgpu::Buffer>,
    bind_group_layout: Option<wgpu::BindGroupLayout>,
    pipeline_layout: Option<wgpu::PipelineLayout>,
    /// One uniform buffer + bind group per draw slot, pre-allocated
    /// at startup. Using separate buffers (rather than dynamic
    /// offsets) is explicit and works first-try on every wgpu
    /// backend we've tested — the dynamic-offset path silently
    /// gave every draw the same uniform on the wgpu 29 / Vulkan
    /// backend we hit in CI.
    uniform_buffers: Vec<wgpu::Buffer>,
    bind_groups: Vec<wgpu::BindGroup>,
    /// Maximum number of draw calls we'll ever issue (sets the
    /// pre-allocated count of uniform buffers / bind groups).
    max_draws: usize,
    step: u32,
    size: (u32, u32),
    auto_snapshot_at: Option<u32>,
    frame_count: u32,
    scene: Scene,
}

impl App {
    fn new(step: u32) -> Self {
        Self {
            window: None,
            device: None,
            queue: None,
            surface: None,
            surface_config: None,
            pipeline: None,
            vertex_buffer: None,
            bind_group_layout: None,
            pipeline_layout: None,
            uniform_buffers: Vec::new(),
            bind_groups: Vec::new(),
            max_draws: 64,
            step,
            size: (800, 600),
            auto_snapshot_at: parse_auto_snapshot(),
            frame_count: 0,
            scene: Scene::for_step(step),
        }
    }

    fn render(&mut self) -> bool {
        let device = self.device.as_ref().unwrap();
        let queue = self.queue.as_ref().unwrap();
        let surface = self.surface.as_ref().unwrap();
        let pipeline = self.pipeline.as_ref().unwrap();
        let vb = self.vertex_buffer.as_ref().unwrap();

        // Tick the scene (advance hover/focus animation).
        self.scene.tick(16.0);

        // Walk the scene → draw list.
        let mut cmds = Vec::new();
        walk(&self.scene.root, &Affine2::IDENTITY, &mut cmds);

        // Build the per-draw uniform for each cmd, then write it
        // into slot `i`'s dedicated uniform buffer. Doing all the
        // writes before opening the render pass is essential:
        // `queue.write_buffer` is queued on the queue timeline and
        // gets applied at the next submit, so any write issued
        // between draws inside a render pass will be overwritten by
        // later writes before the encoder runs.
        assert!(cmds.len() <= self.max_draws, "draw list exceeded pre-allocated slots");
        #[allow(dead_code)]
        const UNIFORM_SIZE: usize = std::mem::size_of::<Uniforms>(); // 92
        for (i, cmd) in cmds.iter().enumerate() {
            let mut u = Uniforms::default();
            u.a = cmd.transform.a;
            u.b = cmd.transform.b;
            u.tx = cmd.transform.tx;
            u.c = cmd.transform.c;
            u.d = cmd.transform.d;
            u.ty = cmd.transform.ty;
            match &cmd.kind {
                DrawKind::Quad { color } => { u.color = *color; }
                DrawKind::Splat { color, intensity, falloff } => {
                    u.splat_color = *color;
                    u.splat_intensity = *intensity;
                    u.splat_falloff = *falloff;
                    u.is_splat = 1.0;
                }
                DrawKind::Text { color, char_count } => {
                    u.text_color = *color;
                    u.text_chars = *char_count as f32;
                    u.is_text = 1.0;
                }
            }
            // Apply hover/focus as a brightness tint (so the active
            // tab is visibly brighter than the others).
            let boost = 1.0 + 0.4 * cmd.focus + 0.2 * cmd.hover;
            if u.is_text > 0.5 {
                u.text_color = [
                    u.text_color[0] * boost,
                    u.text_color[1] * boost,
                    u.text_color[2] * boost,
                    u.text_color[3],
                ];
            } else if u.is_splat < 0.5 {
                u.color = [
                    u.color[0] * boost,
                    u.color[1] * boost,
                    u.color[2] * boost,
                    u.color[3],
                ];
            }
            queue.write_buffer(&self.uniform_buffers[i], 0, bytemuck::bytes_of(&u));
        }

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
                label: Some("main-pass"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &view,
                    resolve_target: None,
                    depth_slice: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(wgpu::Color {
                            r: 0.05, g: 0.05, b: 0.07, a: 1.0,
                        }),
                        store: wgpu::StoreOp::Store,
                    },
                })],
                depth_stencil_attachment: None,
                timestamp_writes: None,
                occlusion_query_set: None,
                multiview_mask: None,
            });
            rpass.set_pipeline(pipeline);
            rpass.set_vertex_buffer(0, vb.slice(..));
            // On some wgpu 29 backends, switching bind groups
            // mid-pass requires re-issuing set_pipeline (the bind
            // group is treated as part of pipeline state and only
            // re-evaluated when the pipeline changes). Setting the
            // pipeline before each draw fixes this.
            for i in 0..cmds.len() {
                rpass.set_pipeline(pipeline);
                rpass.set_bind_group(0, &self.bind_groups[i], &[]);
                rpass.draw(0..6, 0..1);
            }
        }
        queue.submit(std::iter::once(encoder.finish()));
        surface_texture.present();

        // Auto-snapshot logic.
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
        let device = self.device.as_ref().unwrap();
        let queue = self.queue.as_ref().unwrap();
        let pipeline_layout = self.pipeline_layout.as_ref().unwrap();
        let vb = self.vertex_buffer.as_ref().unwrap();

        // 1. Allocate an offscreen RGBA8 texture.
        let snapshot_format = wgpu::TextureFormat::Rgba8Unorm;
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
            format: snapshot_format,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
            view_formats: &[],
        });
        let snap_view = snap_tex.create_view(&Default::default());

        // 2. One-off pipeline targeting the snapshot format.
        let shader_src = format!("{}{}", UNIFORMS_DECL, SHADER);
        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("snapshot-shader"),
            source: wgpu::ShaderSource::Wgsl(shader_src.into()),
        });
        let snapshot_pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("snapshot-pipeline"),
            layout: Some(pipeline_layout),
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
        });

        // 3. Render the scene into the snapshot texture.
        let mut cmds = Vec::new();
        walk(&self.scene.root, &Affine2::IDENTITY, &mut cmds);
        // Build per-draw uniforms and write each into its own
        // dedicated buffer. (All writes happen before the encoder
        // runs — see render() for the queue.write_buffer ordering
        // rationale.)
        assert!(cmds.len() <= self.max_draws, "snapshot draw list exceeded pre-allocated slots");
        #[allow(dead_code)]
        const UNIFORM_SIZE: usize = std::mem::size_of::<Uniforms>(); // 92
        for (i, cmd) in cmds.iter().enumerate() {
            let mut u = Uniforms::default();
            u.a = cmd.transform.a;
            u.b = cmd.transform.b;
            u.tx = cmd.transform.tx;
            u.c = cmd.transform.c;
            u.d = cmd.transform.d;
            u.ty = cmd.transform.ty;
            match &cmd.kind {
                DrawKind::Quad { color } => { u.color = *color; }
                DrawKind::Splat { color, intensity, falloff } => {
                    u.splat_color = *color;
                    u.splat_intensity = *intensity;
                    u.splat_falloff = *falloff;
                    u.is_splat = 1.0;
                }
                DrawKind::Text { color, char_count } => {
                    u.text_color = *color;
                    u.text_chars = *char_count as f32;
                    u.is_text = 1.0;
                }
            }
            let boost = 1.0 + 0.4 * cmd.focus + 0.2 * cmd.hover;
            if u.is_text > 0.5 {
                u.text_color = [
                    u.text_color[0] * boost,
                    u.text_color[1] * boost,
                    u.text_color[2] * boost,
                    u.text_color[3],
                ];
            } else if u.is_splat < 0.5 {
                u.color = [
                    u.color[0] * boost,
                    u.color[1] * boost,
                    u.color[2] * boost,
                    u.color[3],
                ];
            }
            queue.write_buffer(&self.uniform_buffers[i], 0, bytemuck::bytes_of(&u));
        }
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
                            r: 0.05, g: 0.05, b: 0.07, a: 1.0,
                        }),
                        store: wgpu::StoreOp::Store,
                    },
                })],
                depth_stencil_attachment: None,
                timestamp_writes: None,
                occlusion_query_set: None,
                multiview_mask: None,
            });
            rpass.set_pipeline(&snapshot_pipeline);
            rpass.set_vertex_buffer(0, vb.slice(..));
            // See the equivalent block in render() for why
            // set_pipeline must be re-issued per-draw.
            for i in 0..cmds.len() {
                rpass.set_pipeline(&snapshot_pipeline);
                rpass.set_bind_group(0, &self.bind_groups[i], &[]);
                rpass.draw(0..6, 0..1);
            }
        }

        // 4. Copy the offscreen texture to a staging buffer.
        // `bytes_per_row` must be a multiple of COPY_BYTES_PER_ROW_ALIGNMENT
        // (256 in wgpu 29). For 800x600 that's 3200, which rounds up to
        // 3328 (13 * 256). The PNG encoder handles the padded stride.
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

        // 5. Map the buffer, encode to PNG.
        let slice = staging.slice(..);
        slice.map_async(wgpu::MapMode::Read, |_| {});
        let _ = device.poll(wgpu::PollType::wait_indefinitely());
        let data = slice.get_mapped_range();
        let path = format!("target/snapshots/retained-scene-test_{}.png", self.step);
        std::fs::create_dir_all("target/snapshots").ok();
        match write_png_rgba(&path, self.size.0, self.size.1, padded_row, &data) {
            Ok(()) => eprintln!("[snapshot] wrote {}", path),
            Err(e) => eprintln!("[snapshot] failed: {}", e),
        }
        drop(data);
        staging.unmap();
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
                        .with_title(format!("retained-scene-test step={}", self.step))
                        .with_inner_size(winit::dpi::LogicalSize::new(800.0, 600.0)),
                )
                .unwrap(),
        );
        let instance = wgpu::Instance::new(wgpu::InstanceDescriptor {
            backends: wgpu::Backends::all(),
            flags: wgpu::InstanceFlags::default(),
            backend_options: wgpu::BackendOptions::default(),
            memory_budget_thresholds: wgpu::MemoryBudgetThresholds::default(),
            display: None,
        });
        let surface = instance.create_surface(window.clone()).unwrap();
        let adapter = pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions {
            power_preference: wgpu::PowerPreference::HighPerformance,
            compatible_surface: Some(&surface),
            force_fallback_adapter: false,
        }))
        .expect("adapter");
        let (device, queue) = pollster::block_on(adapter.request_device(
            &wgpu::DeviceDescriptor {
                label: Some("device"),
                ..Default::default()
            },
        ))
        .expect("device");
        let caps = surface.get_capabilities(&adapter);
        let format = caps
            .formats
            .iter()
            .copied()
            .find(|f| f.is_srgb())
            .unwrap_or(caps.formats[0]);
        let surface_config = wgpu::SurfaceConfiguration {
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
            format,
            width: 800,
            height: 600,
            present_mode: wgpu::PresentMode::Fifo,
            alpha_mode: wgpu::CompositeAlphaMode::Auto,
            view_formats: vec![],
            desired_maximum_frame_latency: 2,
        };
        surface.configure(&device, &surface_config);

        // Vertex buffer: a unit quad (2 triangles, 6 vertices)
        // in [-0.5, 0.5]. The vertex shader applies the
        // per-draw transform.
        #[repr(C)]
        #[derive(Copy, Clone, bytemuck::Pod, bytemuck::Zeroable)]
        struct Vert { pos: [f32; 2] }
        let verts = [
            Vert { pos: [-0.5, -0.5] },
            Vert { pos: [ 0.5, -0.5] },
            Vert { pos: [ 0.5,  0.5] },
            Vert { pos: [-0.5, -0.5] },
            Vert { pos: [ 0.5,  0.5] },
            Vert { pos: [-0.5,  0.5] },
        ];
        let vertex_buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("quad-vb"),
            contents: bytemuck::cast_slice(&verts),
            usage: wgpu::BufferUsages::VERTEX,
        });

        let uniform_size = std::mem::size_of::<Uniforms>() as u64; // 92 bytes
        // Allocate one uniform buffer per draw slot. Using
        // separate buffers (rather than one big buffer with
        // dynamic offsets, or one buffer with 5 bind groups at
        // different offsets) is the most explicit option and
        // works first-try on every wgpu backend we've seen.
        let max_draws = self.max_draws;
        let uniform_buffers: Vec<wgpu::Buffer> = (0..max_draws)
            .map(|i| {
                device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
                    label: Some(&format!("uniform-{}", i)),
                    contents: bytemuck::bytes_of(&Uniforms::default()),
                    usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
                })
            })
            .collect();

        let bind_group_layout =
            device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
                label: Some("bgl"),
                entries: &[wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::VERTEX | wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Uniform,
                        has_dynamic_offset: false,
                        min_binding_size: wgpu::BufferSize::new(uniform_size),
                    },
                    count: None,
                }],
            });
        let bind_groups: Vec<wgpu::BindGroup> = (0..max_draws)
            .map(|i| {
                device.create_bind_group(&wgpu::BindGroupDescriptor {
                    label: Some(&format!("bg-{}", i)),
                    layout: &bind_group_layout,
                    entries: &[wgpu::BindGroupEntry {
                        binding: 0,
                        resource: uniform_buffers[i].as_entire_binding(),
                    }],
                })
            })
            .collect();

        let shader_src = format!("{}{}", UNIFORMS_DECL, SHADER);
        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("shader"),
            source: wgpu::ShaderSource::Wgsl(shader_src.into()),
        });

        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("pl"),
            bind_group_layouts: &[Some(&bind_group_layout)],
            immediate_size: 0,
        });

        let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("pipeline"),
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

        self.size = (surface_config.width, surface_config.height);
        self.window = Some(window);
        self.device = Some(device);
        self.queue = Some(queue);
        self.surface = Some(surface);
        self.surface_config = Some(surface_config);
        self.pipeline = Some(pipeline);
        self.vertex_buffer = Some(vertex_buffer);
        self.bind_group_layout = Some(bind_group_layout);
        self.pipeline_layout = Some(pipeline_layout);
        self.uniform_buffers = uniform_buffers;
        self.bind_groups = bind_groups;
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
            WindowEvent::KeyboardInput {
                event: winit::event::KeyEvent {
                    state: winit::event::ElementState::Pressed,
                    logical_key,
                    ..
                },
                ..
            } => {
                if let Key::Character(s) = &logical_key {
                    if s == "s" || s == "S" {
                        self.write_snapshot();
                    }
                }
                if matches!(logical_key, Key::Named(NamedKey::Escape)) {
                    event_loop.exit();
                }
            }
            _ => {}
        }
    }
}

fn parse_auto_snapshot() -> Option<u32> {
    for arg in std::env::args() {
        if let Some(val) = arg.strip_prefix("--snapshot-at=") {
            return val.parse().ok();
        }
    }
    None
}

fn main() {
    env_logger::init();
    let step: u32 = std::env::args()
        .find_map(|a| a.strip_prefix("--step=").and_then(|s| s.parse().ok()))
        .unwrap_or(1);
    let event_loop = winit::event_loop::EventLoop::new().unwrap();
    let mut app = App::new(step);
    event_loop.run_app(&mut app).unwrap();
}
