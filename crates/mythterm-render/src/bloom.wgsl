// Bloom shader: threshold + blur + combine, all writing to flat
// intermediate HDR textures. The post-process shader file
// (`postprocess.wgsl`) is separate so this file can use
// @binding(3) for the bloom-result sampler without conflicting
// with the curvature uniform in the post-process vertex shader
// (which also uses @binding(3)).

@group(0) @binding(0)
var input_texture: texture_2d<f32>;
@group(0) @binding(1)
var input_sampler: sampler;

// Combine pass bindings (only used by bloom_combine_fs).
@group(0) @binding(2)
var bloom_texture: texture_2d<f32>;
@group(0) @binding(3)
var bloom_sampler: sampler;

struct VertexOutput {
    @builtin(position) position: vec4<f32>,
    @location(0) uv: vec2<f32>,
};

// Passthrough vertex shader. Bloom writes to flat intermediate
// textures, so screen-curvature would be a no-op here.
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
// Bloom Combine: sum the original HDR + bloom contribution.
// ============================================================
@fragment
fn bloom_combine_fs(in: VertexOutput) -> @location(0) vec4<f32> {
    let original = textureSample(input_texture, input_sampler, in.uv);
    let bloom = textureSample(bloom_texture, bloom_sampler, in.uv);
    return vec4<f32>(original.rgb + bloom.rgb * 0.5, 1.0);
}
