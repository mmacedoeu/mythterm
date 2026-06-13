// Post-processing shader for cinematic terminal rendering.
//
// Full-screen triangle vertex shader shared by all passes.
// Each pass has its own fragment shader entry point.

@group(0) @binding(0)
var input_texture: texture_2d<f32>;
@group(0) @binding(1)
var input_sampler: sampler;

// Bloom combine pass bindings (only used by bloom_combine_fs)
@group(0) @binding(2)
var bloom_texture: texture_2d<f32>;
@group(0) @binding(3)
var bloom_sampler: sampler;

struct VertexOutput {
    @builtin(position) position: vec4<f32>,
    @location(0) uv: vec2<f32>,
};

// Full-screen triangle (no vertex buffer needed)
@vertex
fn vs_main(@builtin(vertex_index) vertex_index: u32) -> VertexOutput {
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
@fragment
fn bloom_threshold_fs(in: VertexOutput) -> @location(0) vec4<f32> {
    let color = textureSample(input_texture, input_sampler, in.uv);
    let brightness = max(color.r, max(color.g, color.b));
    let threshold = 0.8;

    if brightness > threshold {
        // Extract bright pixels with soft knee
        let knee = 0.2;
        let soft = brightness - threshold + knee;
        let contribution = clamp(soft * soft / (4.0 * knee + 0.0001), 0.0, 1.0);
        return color * contribution;
    }
    return vec4<f32>(0.0, 0.0, 0.0, 1.0);
}

// ============================================================
// Bloom Pass 2: Gaussian blur (downsample + blur)
// ============================================================
@fragment
fn bloom_blur_fs(in: VertexOutput) -> @location(0) vec4<f32> {
    let size = vec2<f32>(textureDimensions(input_texture));
    let texel = 1.0 / size;

    // 13-tap Gaussian blur (optimized for large radius)
    var color = vec4<f32>(0.0);
    color += textureSample(input_texture, input_sampler, in.uv + vec2<f32>(-6.0, 0.0) * texel) * 0.002;
    color += textureSample(input_texture, input_sampler, in.uv + vec2<f32>(-5.0, 0.0) * texel) * 0.008;
    color += textureSample(input_texture, input_sampler, in.uv + vec2<f32>(-4.0, 0.0) * texel) * 0.024;
    color += textureSample(input_texture, input_sampler, in.uv + vec2<f32>(-3.0, 0.0) * texel) * 0.056;
    color += textureSample(input_texture, input_sampler, in.uv + vec2<f32>(-2.0, 0.0) * texel) * 0.104;
    color += textureSample(input_texture, input_sampler, in.uv + vec2<f32>(-1.0, 0.0) * texel) * 0.152;
    color += textureSample(input_texture, input_sampler, in.uv) * 0.172;
    color += textureSample(input_texture, input_sampler, in.uv + vec2<f32>(1.0, 0.0) * texel) * 0.152;
    color += textureSample(input_texture, input_sampler, in.uv + vec2<f32>(2.0, 0.0) * texel) * 0.104;
    color += textureSample(input_texture, input_sampler, in.uv + vec2<f32>(3.0, 0.0) * texel) * 0.056;
    color += textureSample(input_texture, input_sampler, in.uv + vec2<f32>(4.0, 0.0) * texel) * 0.024;
    color += textureSample(input_texture, input_sampler, in.uv + vec2<f32>(5.0, 0.0) * texel) * 0.008;
    color += textureSample(input_texture, input_sampler, in.uv + vec2<f32>(6.0, 0.0) * texel) * 0.002;

    return color;
}

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
// Glass Cover Pass: Simulate glass cover layer reflection
// ============================================================
//
// Adds a subtle reflection from a procedural environment on top
// of the LCD output. Models a glass cover layer (like a premium
// laptop or external display) that has:
// - A bright ceiling reflection at the top of the screen
// - A subtle Fresnel brightening at the screen edges
//
// The reflection is additively blended with the scene in HDR space,
// so it's tonemapped together with the rest of the scene. This is
// a procedural environment (vertical gradient + horizontal sine
// variation) — a future pass can swap in a real cubemap.

@group(0) @binding(2)
var<uniform> u_glass: GlassParams;

struct GlassParams {
    /// 0..1: overall reflection intensity.
    intensity: f32,
    /// 0..1: Fresnel F0 (reflection at normal incidence).
    /// 0.04 is the physical value for glass.
    fresnel_bias: f32,
    /// Exponent on the top-gradient falloff. Higher = more localized
    /// at the very top of the screen.
    top_falloff: f32,
    /// 16-byte alignment pad.
    _pad0: f32,
    /// Ceiling reflection color (RGB, warm white by default).
    /// vec4 in uniform is 16 bytes, so the struct is 32 bytes total.
    ceiling_color: vec4<f32>,
}

@fragment
fn glass_fs(in: VertexOutput) -> @location(0) vec4<f32> {
    let scene = textureSample(input_texture, input_sampler, in.uv);

    // Top gradient: bright at the top, fading toward the bottom.
    // pow(t, n) gives a non-linear falloff that looks natural.
    let t = in.uv.y;
    let ceiling = u_glass.ceiling_color.rgb;
    let top_refl = ceiling * pow(t, u_glass.top_falloff);

    // Horizontal variation: simulate a strip light or window.
    // 0.4..1.0 over the screen width — gentle, not distracting.
    let horiz = 0.7 + 0.3 * sin(in.uv.x * 6.28318);
    let top_with_var = top_refl * horiz;

    // Edge Fresnel: stronger reflection at screen edges.
    let dist_from_edge = min(min(in.uv.x, 1.0 - in.uv.x), min(in.uv.y, 1.0 - in.uv.y));
    let edge_factor = 1.0 - clamp(dist_from_edge * 2.0, 0.0, 1.0);
    let fresnel = u_glass.fresnel_bias + (1.0 - u_glass.fresnel_bias) * edge_factor * edge_factor;

    let reflection = top_with_var * fresnel * u_glass.intensity;

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
@fragment
fn bloom_combine_fs(in: VertexOutput) -> @location(0) vec4<f32> {
    let original = textureSample(input_texture, input_sampler, in.uv);
    let bloom = textureSample(bloom_texture, bloom_sampler, in.uv);

    // Bloom intensity multiplier (matches the 0.8 threshold in bloom_threshold_fs)
    let bloom_intensity = 0.6;

    return vec4<f32>(original.rgb + bloom.rgb * bloom_intensity, original.a);
}
