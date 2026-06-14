//! display-test (Phase 3)
//!
//! Prototype the "terminal as a display material" pipeline.
//! Each test step adds one layer of the material model:
//!
//!   1. `display-test --step=1`: terminal text → RGBA8 texture,
//!      sample as fullscreen quad (no material).
//!   2. `display-test --step=2`: + RGB stripe subpixel sampling.
//!   3. `display-test --step=3`: + LCD response curve (temporal
//!      buffer; needs the snapshot pipeline to step through frames).
//!   4. `display-test --step=4`: + glass reflection (procedural
//!      cubemap).
//!   5. `display-test --step=5`: + backlight uniformity.
//!   6. `display-test --step=6`: full DisplayMaterial struct, all
//!      parameters hot-swappable via uniform.
//!
//! Key bindings:
//!   1..6 : jump to step
//!   S    : snapshot current frame to target/snapshots/display-test_<step>.png
//!   Esc  : quit
//!
//! The binary is intentionally self-contained: no mythterm-* deps,
//! no MSDF font pipeline. Step 1+2 use procedurally-generated
//! "terminal content" (color stripes + a stylised letter pattern)
//! so the material effects are the only thing changing between
//! snapshots. Real text is added in a later step once the
//! material contract is stable.

use std::sync::Arc;
use winit::{
    application::ApplicationHandler,
    event::{ElementState, KeyEvent, WindowEvent},
    event_loop::{ActiveEventLoop, EventLoop},
    keyboard::{Key, NamedKey},
    window::{Window, WindowId},
};

use wgsl_sdf::png::write_png_rgba;

const TEXTURE_W: u32 = 256;
const TEXTURE_H: u32 = 64;

// -----------------------------------------------------------------
// Procedural "terminal content" — the texture that the display
// material will be applied to. Stripe pattern + a stylised letter
// grid. Bypasses the font pipeline for now (Phase 3 is about the
// material, not the glyphs).
// -----------------------------------------------------------------

fn build_content_texture() -> Vec<u8> {
    let mut buf = vec![0u8; (TEXTURE_W * TEXTURE_H * 4) as usize];

    // Background: subtle dark navy so non-glyph pixels are
    // visible (this is the "off" color of the simulated display).
    for y in 0..TEXTURE_H {
        for x in 0..TEXTURE_W {
            let i = ((y * TEXTURE_W + x) * 4) as usize;
            buf[i] = 14;
            buf[i + 1] = 22;
            buf[i + 2] = 36;
            buf[i + 3] = 255;
        }
    }

    // Solid color stripes — these are the "text" the display
    // material will sample. Five vertical stripes in distinct
    // hues so subpixel sampling (Step 2) produces a visibly
    // different result: at the edge of a stripe, only one
    // sub-pixel receives color, so the hue shifts toward red,
    // green, or blue depending on which sub-pixel lights up.
    let stripe_colors: [[u8; 3]; 5] = [
        [255, 255, 255], // white  → splits into R+G+B
        [255, 80, 80],   // red    → only the R sub-pixel lights
        [80, 255, 80],   // green
        [80, 80, 255],   // blue
        [255, 220, 80],  // yellow → R+G, no B
    ];
    let stripe_w = TEXTURE_W / stripe_colors.len() as u32;
    for (i, c) in stripe_colors.iter().enumerate() {
        let x0 = i as u32 * stripe_w;
        let x1 = x0 + stripe_w;
        for y in 0..TEXTURE_H {
            for x in x0..x1 {
                let p = ((y * TEXTURE_W + x) * 4) as usize;
                buf[p] = c[0];
                buf[p + 1] = c[1];
                buf[p + 2] = c[2];
                buf[p + 3] = 255;
            }
        }
    }

    // A stylised letter pattern in the middle row: a 3x3 grid of
    // "glyphs" drawn as filled rectangles. This proves the
    // material handles detail, not just big color blocks.
    for gy in 0..3 {
        for gx in 0..3 {
            let x0 = 50 + gx * 55;
            let y0 = 18 + gy * 14;
            let w = 40u32;
            let h = 8u32;
            // Letter body
            for y in y0..y0 + h {
                for x in x0..x0 + w {
                    if x < TEXTURE_W && y < TEXTURE_H {
                        let p = ((y * TEXTURE_W + x) * 4) as usize;
                        // Vary brightness per-cell so the grid
                        // is distinguishable.
                        let bright: u8 = match (gx + gy * 3) % 5 {
                            0 => 255,
                            1 => 200,
                            2 => 150,
                            3 => 220,
                            _ => 180,
                        };
                        buf[p] = bright;
                        buf[p + 1] = bright;
                        buf[p + 2] = bright;
                        buf[p + 3] = 255;
                    }
                }
            }
            // Letter "serif" (top and bottom lines, 1 px)
            for x in (x0 - 2)..(x0 + w + 2) {
                if x < TEXTURE_W && y0 >= 1 {
                    let p = (((y0 - 1) * TEXTURE_W + x) * 4) as usize;
                    buf[p] = 255;
                    buf[p + 1] = 255;
                    buf[p + 2] = 255;
                    buf[p + 3] = 255;
                }
                if x < TEXTURE_W && y0 + h < TEXTURE_H {
                    let p = (((y0 + h) * TEXTURE_W + x) * 4) as usize;
                    buf[p] = 255;
                    buf[p + 1] = 255;
                    buf[p + 2] = 255;
                    buf[p + 3] = 255;
                }
            }
        }
    }

    buf
}

// -----------------------------------------------------------------
// Display material (steps 2..6 read from this).
// -----------------------------------------------------------------

#[derive(Copy, Clone, Debug)]
#[repr(C)]
struct DisplayMaterial {
    /// 0 = none, 1 = RGB stripe
    subpixel_layout: u32,
    /// 0 = none, 1 = LCD, 2 = phosphor P22
    response_curve: u32,
    backlight_uniformity: f32,
    glass_thickness: f32,
    reflection_strength: f32,
    bloom_strength: f32,
    /// Direct blend factor for the response curve (0 = full history,
    /// 1 = full current). The "physical" response time (e.g. 8ms for
    /// LCD) is converted to this in `material_for_step`.
    persistence: f32,
    _pad: [u32; 1],
}

impl Default for DisplayMaterial {
    fn default() -> Self {
        Self {
            subpixel_layout: 0,
            response_curve: 0,
            backlight_uniformity: 1.0,
            glass_thickness: 0.0,
            reflection_strength: 0.0,
            bloom_strength: 0.0,
            persistence: 1.0,
            _pad: [0],
        }
    }
}

unsafe impl bytemuck::Pod for DisplayMaterial {}
unsafe impl bytemuck::Zeroable for DisplayMaterial {}

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
    bind_group: Option<wgpu::BindGroup>,
    bind_group_layout: Option<wgpu::BindGroupLayout>,
    pipeline_layout: Option<wgpu::PipelineLayout>,
    content_texture: Option<wgpu::Texture>,
    content_view: Option<wgpu::TextureView>,
    history_texture: Option<wgpu::Texture>,
    history_view: Option<wgpu::TextureView>,
    material_buffer: Option<wgpu::Buffer>,
    step: u32,
    size: (u32, u32),
    auto_snapshot_at: Option<u32>,
    frame_count: u32,
    /// Per-step material parameters.
    material: DisplayMaterial,
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
            bind_group: None,
            bind_group_layout: None,
            pipeline_layout: None,
            content_texture: None,
            content_view: None,
            history_texture: None,
            history_view: None,
            material_buffer: None,
            step,
            size: (800, 600),
            auto_snapshot_at: parse_auto_snapshot(),
            frame_count: 0,
            material: material_for_step(step),
        }
    }
}

fn material_for_step(step: u32) -> DisplayMaterial {
    match step {
        // Step 1: terminal content, no material.
        1 => DisplayMaterial::default(),
        // Step 2: + RGB stripe subpixel sampling.
        2 => DisplayMaterial {
            subpixel_layout: 1,
            ..Default::default()
        },
        // Step 3: + response curve. The "persistence" is the
        // test-time blend factor: a real LCD at 60fps would
        // saturate to 1.0 and be invisible, so we use 0.3 (a
        // noticeable but not extreme trail).
        3 => DisplayMaterial {
            subpixel_layout: 1,
            response_curve: 1,
            persistence: 0.3,
            ..Default::default()
        },
        // Step 4: + glass reflection.
        4 => DisplayMaterial {
            subpixel_layout: 1,
            response_curve: 1,
            persistence: 0.3,
            glass_thickness: 0.5,
            reflection_strength: 0.7,
            ..Default::default()
        },
        // Step 5: + backlight uniformity. full_backlight = 0
        // means no effect; backlight_uniformity = 0.5 means
        // center is 1.0x and corners are ~0.83x.
        5 => DisplayMaterial {
            subpixel_layout: 1,
            response_curve: 1,
            persistence: 0.3,
            glass_thickness: 0.5,
            reflection_strength: 0.7,
            backlight_uniformity: 0.5,
            ..Default::default()
        },
        // Step 6: full DisplayMaterial, hot-swappable.
        6 => DisplayMaterial {
            subpixel_layout: 1,
            response_curve: 1,
            persistence: 0.3,
            glass_thickness: 0.5,
            reflection_strength: 0.7,
            backlight_uniformity: 0.5,
            bloom_strength: 0.3,
            ..Default::default()
        },
        _ => DisplayMaterial::default(),
    }
}

// -----------------------------------------------------------------
// Shaders
// -----------------------------------------------------------------

// Single combined shader module for the display material. All
// six steps use the same fragment shader; the DisplayMaterial
// uniform branches select which effects are active.
const DISPLAY_SHADER: &str = r#"
struct VsIn {
    @location(0) pos: vec2<f32>,
    @location(1) uv: vec2<f32>,
}
struct VsOut {
    @builtin(position) clip_pos: vec4<f32>,
    @location(0) uv: vec2<f32>,
}
@vertex
fn vs_main(in: VsIn) -> VsOut {
    var out: VsOut;
    out.clip_pos = vec4<f32>(in.pos, 0.0, 1.0);
    out.uv = in.uv;
    return out;
}

@group(0) @binding(0) var content_tex: texture_2d<f32>;
@group(0) @binding(1) var content_smp: sampler;
@group(0) @binding(2) var<uniform> material: DisplayMaterial;
@group(0) @binding(3) var history_tex: texture_2d<f32>;

@fragment
fn fs_main(in: VsOut) -> @location(0) vec4<f32> {
    let uv = in.uv;
    var color: vec4<f32>;

    // === Step 1 / 2: source content ===
    if material.subpixel_layout == 0u {
        // Plain sample. Nearest filter on the sampler keeps stripe
        // edges crisp.
        color = textureSample(content_tex, content_smp, uv);
    } else {
        // RGB stripe subpixel: 3 neighbor samples at sub-pixel
        // offsets. The horizontal sub-pixel index is
        //   floor(uv.x * texels_per_stripe * 3) mod 3
        // but we just take r at uv-tx, g at uv, b at uv+tx, which
        // is equivalent up to a phase shift.
        let texel = vec2<f32>(1.0 / 256.0, 1.0 / 64.0);
        let r_uv = uv + vec2<f32>(-texel.x, 0.0);
        let g_uv = uv;
        let b_uv = uv + vec2<f32>(texel.x, 0.0);
        let r = textureSample(content_tex, content_smp, r_uv).r;
        let g = textureSample(content_tex, content_smp, g_uv).g;
        let b = textureSample(content_tex, content_smp, b_uv).b;
        let a = textureSample(content_tex, content_smp, uv).a;
        color = vec4<f32>(r, g, b, a);
    }

    // === Step 3: response curve (temporal blend with history) ===
    if material.response_curve != 0u {
        let hist = textureSample(history_tex, content_smp, uv);
        // `persistence` is the direct blend factor (0 = full
        // history, 1 = full current). A real LCD at 60fps would
        // saturate to 1.0; we use 0.3 for the test to make the
        // effect visible.
        let blended = mix(hist, color, material.persistence);
        if material.response_curve == 2u {
            // Phosphor P22: green tint + slight glow on bright
            // pixels (the famous green trail of CRTs).
            let glow = max(0.0, (blended.r + blended.b) * 0.5 - 0.3) * 0.5;
            color = vec4<f32>(
                blended.r * 0.65,
                blended.g + glow,
                blended.b * 0.55,
                blended.a
            );
        } else {
            // LCD: slight gamma boost (deeper blacks, slightly
            // brighter midtones).
            color = vec4<f32>(
                pow(blended.rgb, vec3<f32>(0.92)),
                blended.a
            );
        }
    }

    // === Step 4: glass reflection ===
    // Procedural reflection: a vertical gradient (light at the
    // top, dark at the bottom), mixed in at
    //   reflection_strength * glass_thickness * 0.3
    // capped at 0.2 per spec to never obscure text.
    if material.glass_thickness > 0.0 && material.reflection_strength > 0.0 {
        let top = vec3<f32>(0.45, 0.55, 0.65);
        let bot = vec3<f32>(0.02, 0.02, 0.04);
        let refl = mix(bot, top, 1.0 - uv.y);
        let mix_factor = min(0.2, material.reflection_strength * material.glass_thickness * 0.3);
        color = vec4<f32>(mix(color.rgb, refl, mix_factor), color.a);
    }

    // === Step 5: backlight uniformity ===
    // Radial gradient: 1.0 at center, dropping to 0.7 at corners.
    if material.backlight_uniformity < 1.0 {
        let centered = uv - vec2<f32>(0.5, 0.5);
        let radial = clamp(1.0 - length(centered) * 0.6, 0.7, 1.0);
        // `backlight_uniformity` interpolates between flat (1.0)
        // and the radial gradient (0.0 = pure radial).
        let backlight = mix(1.0, radial, 1.0 - material.backlight_uniformity);
        color = vec4<f32>(color.rgb * backlight, color.a);
    }

    // === Step 6: bloom (additive bright-pixel bleed) ===
    if material.bloom_strength > 0.0 {
        let luma = dot(color.rgb, vec3<f32>(0.299, 0.587, 0.114));
        let bloom = max(0.0, luma - 0.6) * material.bloom_strength;
        color = vec4<f32>(color.rgb + vec3<f32>(bloom), color.a);
    }

    return color;
}
"#;

// -----------------------------------------------------------------
// ApplicationHandler
// -----------------------------------------------------------------

impl ApplicationHandler for App {
    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        if self.window.is_some() {
            return;
        }
        let window = Arc::new(
            event_loop
                .create_window(
                    Window::default_attributes()
                        .with_title(format!("display-test step={}", self.step))
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
                        label: Some("display-test"),
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

        // Content texture: 256x64 RGBA8, generated procedurally.
        let content_data = build_content_texture();
        let content_texture = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("content-tex"),
            size: wgpu::Extent3d {
                width: TEXTURE_W,
                height: TEXTURE_H,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::Rgba8UnormSrgb,
            usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
            view_formats: &[],
        });
        queue.write_texture(
            wgpu::TexelCopyTextureInfo {
                texture: &content_texture,
                mip_level: 0,
                origin: wgpu::Origin3d::ZERO,
                aspect: wgpu::TextureAspect::All,
            },
            &content_data,
            wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(TEXTURE_W * 4),
                rows_per_image: Some(TEXTURE_H),
            },
            wgpu::Extent3d {
                width: TEXTURE_W,
                height: TEXTURE_H,
                depth_or_array_layers: 1,
            },
        );
        let content_view = content_texture.create_view(&Default::default());

        // History texture: 256x64 Rgba8Unorm, cleared to 0.
        // Used by the response curve in step 3+. Initial state
        // is (0, 0, 0, 0) so the first-frame blend is biased
        // toward black.
        let history_texture = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("history-tex"),
            size: wgpu::Extent3d {
                width: TEXTURE_W,
                height: TEXTURE_H,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::Rgba8Unorm,
            usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
            view_formats: &[],
        });
        // Explicit clear to 0 so the initial state is deterministic.
        let zeros = vec![0u8; (TEXTURE_W * TEXTURE_H * 4) as usize];
        queue.write_texture(
            wgpu::TexelCopyTextureInfo {
                texture: &history_texture,
                mip_level: 0,
                origin: wgpu::Origin3d::ZERO,
                aspect: wgpu::TextureAspect::All,
            },
            &zeros,
            wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(TEXTURE_W * 4),
                rows_per_image: Some(TEXTURE_H),
            },
            wgpu::Extent3d {
                width: TEXTURE_W,
                height: TEXTURE_H,
                depth_or_array_layers: 1,
            },
        );
        let history_view = history_texture.create_view(&Default::default());

        // Material uniform.
        let material_buffer = device.create_buffer_init(
            &wgpu::util::BufferInitDescriptor {
                label: Some("material"),
                contents: bytemuck::bytes_of(&self.material),
                usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            },
        );

        // Fullscreen quad: two triangles covering [-1, 1] NDC.
        // UVs in [0, 1]. We're going to scale the quad so the
        // texture fills the window with a margin.
        let quad_w = 0.9; // 90% of window width
        let quad_h = 0.45; // aspect 256:64 ≈ 4:1
        let verts: [Vertex; 6] = [
            Vertex { pos: [-quad_w, -quad_h], uv: [0.0, 1.0] },
            Vertex { pos: [ quad_w, -quad_h], uv: [1.0, 1.0] },
            Vertex { pos: [ quad_w,  quad_h], uv: [1.0, 0.0] },
            Vertex { pos: [-quad_w, -quad_h], uv: [0.0, 1.0] },
            Vertex { pos: [ quad_w,  quad_h], uv: [1.0, 0.0] },
            Vertex { pos: [-quad_w,  quad_h], uv: [0.0, 0.0] },
        ];
        let vertex_buffer = device.create_buffer_init(
            &wgpu::util::BufferInitDescriptor {
                label: Some("quad"),
                contents: bytemuck::cast_slice(&verts),
                usage: wgpu::BufferUsages::VERTEX,
            },
        );

        // Bind group: content texture + sampler + material +
        // history texture.
        let bind_group_layout =
            device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
                label: Some("display-bgl"),
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
                        ty: wgpu::BindingType::Buffer {
                            ty: wgpu::BufferBindingType::Uniform,
                            has_dynamic_offset: false,
                            min_binding_size: None,
                        },
                        count: None,
                    },
                    wgpu::BindGroupLayoutEntry {
                        binding: 3,
                        visibility: wgpu::ShaderStages::FRAGMENT,
                        ty: wgpu::BindingType::Texture {
                            sample_type: wgpu::TextureSampleType::Float { filterable: true },
                            view_dimension: wgpu::TextureViewDimension::D2,
                            multisampled: false,
                        },
                        count: None,
                    },
                ],
            });

        let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("content-smp"),
            address_mode_u: wgpu::AddressMode::ClampToEdge,
            address_mode_v: wgpu::AddressMode::ClampToEdge,
            address_mode_w: wgpu::AddressMode::ClampToEdge,
            mag_filter: wgpu::FilterMode::Nearest,
            min_filter: wgpu::FilterMode::Nearest,
            mipmap_filter: wgpu::MipmapFilterMode::Nearest,
            ..Default::default()
        });

        let bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("display-bg"),
            layout: &bind_group_layout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: wgpu::BindingResource::TextureView(&content_view),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: wgpu::BindingResource::Sampler(&sampler),
                },
                wgpu::BindGroupEntry {
                    binding: 2,
                    resource: material_buffer.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 3,
                    resource: wgpu::BindingResource::TextureView(&history_view),
                },
            ],
        });

        // Combined shader module. We have two entry points in one
        // module: `vs_main` and `fs_main`.
        let shader_src = DISPLAY_SHADER;
        // Add the DisplayMaterial struct + binding block.
        // We also need to declare it matching the Rust struct.
        let mat_decl = r#"
struct DisplayMaterial {
    subpixel_layout: u32,
    response_curve: u32,
    backlight_uniformity: f32,
    glass_thickness: f32,
    reflection_strength: f32,
    bloom_strength: f32,
    persistence: f32,
    _pad: u32,
}
"#;
        let shader_src = format!("{}{}", mat_decl, shader_src);
        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("display-shader"),
            source: wgpu::ShaderSource::Wgsl(shader_src.into()),
        });

        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("display-pl"),
            bind_group_layouts: &[Some(&bind_group_layout)],
            immediate_size: 0,
        });

        let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("display-pipeline"),
            layout: Some(&pipeline_layout),
            vertex: wgpu::VertexState {
                module: &shader,
                entry_point: Some("vs_main"),
                buffers: &[wgpu::VertexBufferLayout {
                    array_stride: 16,
                    step_mode: wgpu::VertexStepMode::Vertex,
                    attributes: &wgpu::vertex_attr_array![0 => Float32x2, 1 => Float32x2],
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
        self.pipeline = Some(pipeline);
        self.vertex_buffer = Some(vertex_buffer);
        self.bind_group = Some(bind_group);
        self.bind_group_layout = Some(bind_group_layout);
        self.pipeline_layout = Some(pipeline_layout);
        self.content_texture = Some(content_texture);
        self.content_view = Some(content_view);
        self.history_texture = Some(history_texture);
        self.history_view = Some(history_view);
        self.material_buffer = Some(material_buffer);
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
                event: KeyEvent {
                    state: ElementState::Pressed,
                    logical_key,
                    ..
                },
                ..
            } => {
                if let Key::Character(s) = &logical_key {
                    if s == "s" || s == "S" {
                        self.write_snapshot();
                    } else if let Some(d) = s.as_str().chars().next().and_then(|c| c.to_digit(10)) {
                        if (1..=6).contains(&d) {
                            // Hot-swap: pick the new material and
                            // upload it to the existing buffer. No
                            // pipeline re-init needed since all
                            // six steps use the same shader.
                            let new_material = material_for_step(d);
                            self.material = new_material;
                            if let (Some(queue), Some(buf)) = (&self.queue, &self.material_buffer) {
                                queue.write_buffer(buf, 0, bytemuck::bytes_of(&new_material));
                            }
                            self.step = d;
                            log::info!("hot-swapped to step {}", d);
                        }
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

#[repr(C)]
#[derive(Copy, Clone, Debug, bytemuck::Pod, bytemuck::Zeroable)]
struct Vertex {
    pos: [f32; 2],
    uv: [f32; 2],
}

impl App {
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
                label: Some("display-rp"),
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
            rpass.set_pipeline(self.pipeline.as_ref().unwrap());
            rpass.set_bind_group(0, self.bind_group.as_ref().unwrap(), &[]);
            rpass.set_vertex_buffer(0, self.vertex_buffer.as_ref().unwrap().slice(..));
            rpass.draw(0..6, 0..1);
        }

        queue.submit(std::iter::once(encoder.finish()));
        surface_texture.present();

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

        let snapshot_format = wgpu::TextureFormat::Rgba8Unorm;
        let offscreen_tex = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("snapshot-tex"),
            size: wgpu::Extent3d { width: w, height: h, depth_or_array_layers: 1 },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: snapshot_format,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
            view_formats: &[],
        });
        let view = offscreen_tex.create_view(&Default::default());

        // Re-render the same scene into the offscreen texture.
        // The pipeline, bind group, and vertex buffer are all
        // already configured for the surface — the only thing
        // that changes is the target view.
        // Build a one-off pipeline targeting the snapshot format.
        let shader_src = DISPLAY_SHADER;
        let mat_decl = r#"
struct DisplayMaterial {
    subpixel_layout: u32,
    response_curve: u32,
    backlight_uniformity: f32,
    glass_thickness: f32,
    reflection_strength: f32,
    bloom_strength: f32,
    persistence: f32,
    _pad: u32,
}
"#;
        let shader_src = format!("{}{}", mat_decl, shader_src);
        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("display-snapshot-shader"),
            source: wgpu::ShaderSource::Wgsl(shader_src.into()),
        });
        // Use the same pipeline layout as the surface pipeline.
        let pipeline_layout = self.pipeline_layout.as_ref().unwrap();
        let snapshot_pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("display-snapshot-pipeline"),
            layout: Some(&pipeline_layout),
            vertex: wgpu::VertexState {
                module: &shader,
                entry_point: Some("vs_main"),
                buffers: &[wgpu::VertexBufferLayout {
                    array_stride: 16,
                    step_mode: wgpu::VertexStepMode::Vertex,
                    attributes: &wgpu::vertex_attr_array![0 => Float32x2, 1 => Float32x2],
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

        let mut encoder = device.create_command_encoder(&Default::default());
        {
            let mut rpass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("display-snapshot-rp"),
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
        let path = format!("target/snapshots/display-test_{}.png", self.step);
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
            return val.parse().unwrap_or(1).clamp(1, 6);
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
    log::info!("display-test step: {}", step);
    let event_loop = EventLoop::new()?;
    let mut app = App::new(step);
    event_loop.run_app(&mut app)?;
    Ok(())
}
