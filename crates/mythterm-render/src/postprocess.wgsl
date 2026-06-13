// Post-processing shader for cinematic terminal rendering.
//
// Two vertex shader entry points are provided:
//   - `vs_main`     : applies the optional screen-curvature barrel
//                     distortion. Used by the post-process passes
//                     (LCD, glass, tonemap) so the final image is
//                     curved.
//   - `bloom_vs_main`: passthrough (no curvature). Used by the
//                     bloom passes (threshold, blur, combine),
//                     which write to flat intermediate textures.
//                     Curvature is a no-op on intermediate writes
//                     and would only cost an extra uniform binding.
//
// Each pass has its own fragment shader entry point.
//
// Note: the bloom shaders (threshold, blur, combine) live in
// `bloom.wgsl` because they have different bind-group layout
// requirements (they need @binding(3) for the bloom-result
// sampler, which conflicts with the curvature uniform at
// @binding(3) used by this file's vertex shader).

@group(0) @binding(0)
var input_texture: texture_2d<f32>;
@group(0) @binding(1)
var input_sampler: sampler;

struct VertexOutput {
    @builtin(position) position: vec4<f32>,
    @location(0) uv: vec2<f32>,
};

@group(0) @binding(3) var<uniform> u_curvature: CurvatureParams;

struct CurvatureParams {
    /// 0..1: barrel-distortion strength.
    /// 0.0 = flat screen, 0.05 = subtle curve, 0.1 = noticeable.
    strength: f32,
    /// 16-byte alignment pads.
    _pad0: f32,
    _pad1: f32,
    _pad2: f32,
}

// Full-screen triangle (no vertex buffer needed).
// Applies an optional screen-curvature barrel distortion to the UV.
// With `u_curvature.strength = 0.0` the warp is a no-op and we get
// a flat screen.
@vertex
fn vs_main(@builtin(vertex_index) vertex_index: u32) -> VertexOutput {
    var out: VertexOutput;
    let x = f32(i32(vertex_index) / 2) * 4.0 - 1.0;
    let y = f32(i32(vertex_index) % 2) * 4.0 - 1.0;
    out.position = vec4<f32>(x, y, 0.0, 1.0);
    let uv = vec2<f32>((x + 1.0) * 0.5, (1.0 - y) * 0.5);

    // Barrel distortion: r2-based radial outward warp. At the
    // center of the screen, r2=0, no distortion. At the corners,
    // r2=2, so the UV is shifted outward by ~2*strength*centered.
    let centered = uv * 2.0 - 1.0;
    let r2 = dot(centered, centered);
    let warped = centered * (1.0 + r2 * u_curvature.strength);
    out.uv = warped * 0.5 + 0.5;
    return out;
}

// Passthrough vertex shader for the bloom passes. Bloom writes to
// flat intermediate textures, so the screen-curvature barrel
// distortion would be a wasted uniform binding. The output UVs
// are passed through unchanged.
@vertex
fn bloom_vs_main(@builtin(vertex_index) vertex_index: u32) -> VertexOutput {
    var out: VertexOutput;
    let x = f32(i32(vertex_index) / 2) * 4.0 - 1.0;
    let y = f32(i32(vertex_index) % 2) * 4.0 - 1.0;
    out.position = vec4<f32>(x, y, 0.0, 1.0);
    out.uv = vec2<f32>((x + 1.0) * 0.5, (1.0 - y) * 0.5);
    return out;
}

// ============================================================
// ACES Tonemapping
// ============================================================
fn aces(x: vec3<f32>) -> vec3<f32> {
    let a = 2.51;
    let b = 0.03;
    let c = 2.43;
    let d = 0.59;
    let e = 0.14;
    return clamp((x * (a * x + b)) / (x * (c * x + d) + e), vec3<f32>(0.0), vec3<f32>(1.0));
}

// ============================================================
// Bloom Pass 1: Extract bright pixels (threshold)
// ============================================================
// (Moved to `bloom.wgsl` to avoid @binding(3) conflict with the
// curvature uniform in this file.)
//
// ============================================================
// Bloom Pass 2: Gaussian blur (downsample + blur)
// ============================================================
// (Moved to `bloom.wgsl`.)
//

// ============================================================
// LCD Subpixel Pass: Simulate RGB subpixel rendering
// ============================================================
//
// Reads the HDR scene, samples the R, G, B channels at slightly
// shifted UVs to mimic a physical RGB-stripe LCD's per-subpixel
// addressing. This produces the subtle color fringing on text edges
// (a la ClearType / Apple Retina LCD look) without actually
// rasterizing glyphs at subpixel resolution.
//
// Strength is exposed via a uniform so it can be tuned at runtime
// (or driven from a config setting). At strength=0 the pass is a
// no-op (just passes through the input).

@group(0) @binding(2)
var<uniform> u_lcd: LcdParams;

struct LcdParams {
    /// 0..1: blend between original and subpixel-sampled result.
    strength: f32,
    /// Subpixel width as fraction of a pixel. 0.33 for RGB stripe,
    /// 0.5 for RGBG PenTile, etc.
    subpixel_width: f32,
    /// 0..1: scanline modulation amplitude. 0 disables.
    scanline: f32,
    /// 16-byte alignment pad.
    _pad: f32,
}

@fragment
fn lcd_fs(in: VertexOutput) -> @location(0) vec4<f32> {
    let orig = textureSample(input_texture, input_sampler, in.uv);
    let texel = 1.0 / vec2<f32>(textureDimensions(input_texture));

    // Sample R, G, B at their subpixel positions.
    // RGB-stripe layout: R left, G center, B right.
    let sub = u_lcd.subpixel_width * texel.x;
    let r = textureSample(input_texture, input_sampler, in.uv + vec2<f32>(-sub, 0.0)).r;
    let g = textureSample(input_texture, input_sampler, in.uv).g;
    let b = textureSample(input_texture, input_sampler, in.uv + vec2<f32>(sub, 0.0)).b;
    let subpixel = vec4<f32>(r, g, b, orig.a);

    // Blend between original and subpixel-sampled.
    var color = mix(orig, subpixel, u_lcd.strength);

    // Optional subtle scanline (modern LCD, not CRT).
    if u_lcd.scanline > 0.0 {
        let screen_h = f32(textureDimensions(input_texture).y);
        let pixel_y = in.uv.y * screen_h;
        // Soft per-row brightness dip. sin gives a value in [-1, 1];
        // remap to [1-scanline, 1].
        let dip = 0.5 - 0.5 * sin(pixel_y * 3.14159265);
        // WGSL forbids swizzle assignment, so build the new vec4.
        let scale = 1.0 - u_lcd.scanline * dip;
        color = vec4<f32>(color.rgb * scale, color.a);
    }

    return color;
}

// ============================================================
// Glass Cover Pass: Real environment reflection
// ============================================================
//
// Samples a procedural environment cubemap at the reflection
// direction derived from the screen position. The reflection
// direction is the line of sight from the camera to the surface
// point, extended behind the screen — physically what a flat
// mirror would reflect.
//
// The cubemap is a simple indoor "room":
//   +Y face: warm white ceiling
//   -Y face: dark cool floor
//   ±X, ±Z: dim neutral walls
//
// Combined with a Fresnel edge falloff so reflections are
// strongest at the screen edges (where a real glass cover would
// reflect at grazing angles) and weakest in the center.

@group(0) @binding(2)
var<uniform> u_glass: GlassParams;

@group(0) @binding(4)
var env_map: texture_cube<f32>;

@group(0) @binding(5)
var env_sampler: sampler;

struct GlassParams {
    /// 0..1: overall reflection intensity.
    intensity: f32,
    /// 0..1: Fresnel F0 (reflection at normal incidence).
    /// 0.04 is the physical value for glass.
    fresnel_bias: f32,
    /// Unused — kept for ABI compatibility with the previous
    /// procedural version of the glass pass.
    top_falloff: f32,
    /// 16-byte alignment pad.
    _pad0: f32,
    /// Unused — the environment cubemap provides the ceiling
    /// color. Kept for ABI compatibility.
    ceiling_color: vec4<f32>,
}

@fragment
fn glass_fs(in: VertexOutput) -> @location(0) vec4<f32> {
    let scene = textureSample(input_texture, input_sampler, in.uv);

    // Compute the reflection direction in world space.
    //
    // We treat the screen as a flat plane in the x-y plane at z=0,
    // with the camera at +z. The reflection direction at each
    // screen point is the line of sight from the camera to that
    // point, extended behind the screen (i.e. -z). For a perspective
    // camera this is just (screen_x, screen_y, -1) where screen_x
    // and screen_y are normalized screen coordinates.
    //
    // The cubemap is sampled at this direction. At the top of the
    // screen, screen_y > 0, so we sample near the +Y face (ceiling).
    // At the bottom, near -Y (floor). At the sides, the walls.
    let dims = vec2<f32>(textureDimensions(input_texture));
    let aspect = dims.x / dims.y;
    let screen_x = (in.uv.x - 0.5) * 2.0 * aspect;
    let screen_y = (0.5 - in.uv.y) * 2.0;
    let env_dir = vec3<f32>(screen_x, screen_y, -1.0);

    let env_color = textureSample(env_map, env_sampler, env_dir).rgb;

    // Edge Fresnel: stronger reflection at the screen edges. At
    // the very edge, edge_factor = 1 and fresnel = 1.0; at the
    // center, edge_factor = 0 and fresnel = fresnel_bias (0.04).
    let dist_from_edge = min(min(in.uv.x, 1.0 - in.uv.x), min(in.uv.y, 1.0 - in.uv.y));
    let edge_factor = 1.0 - clamp(dist_from_edge * 2.0, 0.0, 1.0);
    let fresnel = u_glass.fresnel_bias + (1.0 - u_glass.fresnel_bias) * edge_factor * edge_factor;

    let reflection = env_color * fresnel * u_glass.intensity;

    return vec4<f32>(scene.rgb + reflection, scene.a);
}

// ============================================================
// Tonemap Pass: Filmic ACES tonemapping + micro-contrast +
// vignette + edge lighting
// ============================================================
//
// All four effects are display-space (post-tonemap), so they live
// in a single fragment shader to avoid an extra full-resolution
// scene texture. The cost is a few extra ALU ops per fragment.

@group(0) @binding(2)
var<uniform> u_tonemap: TonemapParams;

struct TonemapParams {
    /// 0..1: micro-contrast strength (S-curve amount applied to the
    /// tonemapped color). 0 = no change, 1 = full smoothstep S-curve.
    micro_contrast: f32,
    /// 0..1: vignette strength (corner darkening).
    vignette: f32,
    /// 0..1: edge-light intensity (backlight bleed halo).
    edge_intensity: f32,
    /// Width of the edge halo as a fraction of the screen edge.
    /// 0.05 = 5% from the edge inward, 0.0 disables.
    edge_width: f32,
    /// Edge halo color (typically warm white). RGB used, A ignored.
    /// vec4 in uniform is 16 bytes, so the struct is 32 bytes total
    /// — matches the Rust `TonemapParams` with `[f32; 4]` for the color.
    edge_color: vec4<f32>,
}

/// Smoothstep-based S-curve for micro-contrast.
///
/// `x` is the input value (typically in [0,1] after ACES tonemap).
/// `strength` is the blend amount: 0 = identity, 1 = full S-curve.
///
/// The S-curve is `3x² - 2x³`, which preserves black/white and adds
/// contrast around the midtones (0.5). Cheap, looks natural, and
/// doesn't introduce ringing or color shifts.
fn micro_contrast(x: f32, strength: f32) -> f32 {
    let s = x * x * (3.0 - 2.0 * x);
    return mix(x, s, strength);
}

@fragment
fn tonemap_fs(in: VertexOutput) -> @location(0) vec4<f32> {
    let color = textureSample(input_texture, input_sampler, in.uv);

    // Filmic ACES tonemapping
    let mapped = aces(color.rgb);

    // Micro-contrast: subtle S-curve applied per channel after ACES.
    // Per-channel keeps the implementation trivial; visually
    // indistinguishable from a luminance-based S-curve at small
    // strengths (<0.3) which is the operating range.
    let mc = u_tonemap.micro_contrast;
    let contrasted = vec3<f32>(
        micro_contrast(mapped.r, mc),
        micro_contrast(mapped.g, mc),
        micro_contrast(mapped.b, mc),
    );

    // Corner vignette
    let center = vec2<f32>(0.5, 0.5);
    let dist = distance(in.uv, center);
    let vignette = 1.0 - smoothstep(0.4, 0.9, dist) * u_tonemap.vignette;

    // Edge lighting: warm halo near the screen perimeter, simulating
    // backlight bleed on a premium edge-lit LCD / OLED display.
    let dist_from_edge = min(min(in.uv.x, 1.0 - in.uv.x), min(in.uv.y, 1.0 - in.uv.y));
    let edge = 1.0 - smoothstep(0.0, u_tonemap.edge_width, dist_from_edge);
    let edge_contrib = edge * u_tonemap.edge_intensity * u_tonemap.edge_color.rgb;

    let lit = contrasted * vignette + edge_contrib;

    return vec4<f32>(lit, color.a);
}

// ============================================================
// Bloom Combine Pass: Add bloom result back onto original.
// HDR output — to be followed by tonemap_fs in PostProcess.
// ============================================================
// (Moved to `bloom.wgsl`.)

