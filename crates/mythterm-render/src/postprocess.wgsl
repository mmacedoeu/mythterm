// Post-processing shader for cinematic terminal rendering.
//
// Full-screen triangle vertex shader shared by all passes.
// Each pass has its own fragment shader entry point.

@group(0) @binding(0)
var input_texture: texture_2d<f32>;
@group(0) @binding(1)
var input_sampler: sampler;

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
// LCD Subpixel Pass: Simulate RGB subpixels
// ============================================================
@fragment
fn lcd_fs(in: VertexOutput) -> @location(0) vec4<f32> {
    let color = textureSample(input_texture, input_sampler, in.uv);

    let screen_size = vec2<f32>(textureDimensions(input_texture));
    let pixel_x = in.uv.x * screen_size.x;

    // RGB subpixel stripe
    let stripe = fract(pixel_x * 3.0);

    var result = color;

    // Subtle channel bias based on subpixel position
    if stripe < 0.333 {
        result.g *= 0.97;
        result.b *= 0.94;
    } else if stripe < 0.666 {
        result.r *= 0.97;
        result.b *= 0.97;
    } else {
        result.r *= 0.94;
        result.g *= 0.97;
    }

    // Subtle scanline effect (modern LCD, not CRT)
    let pixel_y = in.uv.y * screen_size.y;
    let scanline = 1.0 - 0.015 * sin(pixel_y * 3.14159);
    result *= scanline;

    return vec4<f32>(result.rgb, color.a);
}

// ============================================================
// Tonemap Pass: Filmic ACES tonemapping + vignette
// ============================================================
@fragment
fn tonemap_fs(in: VertexOutput) -> @location(0) vec4<f32> {
    let color = textureSample(input_texture, input_sampler, in.uv);

    // Apply ACES tonemapping
    let mapped = aces(color.rgb);

    // Slight vignette effect
    let center = vec2<f32>(0.5, 0.5);
    let dist = distance(in.uv, center);
    let vignette = 1.0 - smoothstep(0.4, 0.9, dist) * 0.25;

    return vec4<f32>(mapped * vignette, color.a);
}
