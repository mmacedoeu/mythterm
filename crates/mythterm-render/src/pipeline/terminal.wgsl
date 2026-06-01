// Terminal rendering shader.
//
// Renders terminal cells as textured quads with foreground/background colors.
// Background colors are rendered as solid quads, text glyphs as alpha-blended
// textured quads on top.

struct Uniforms {
    screen_width: f32,
    screen_height: f32,
    _padding: vec2<f32>,
};

@group(0) @binding(0)
var<uniform> uniforms: Uniforms;

@group(1) @binding(0)
var atlas_texture: texture_2d<f32>;
@group(1) @binding(1)
var atlas_sampler: sampler;

struct VertexInput {
    @location(0) position: vec2<f32>,
    @location(1) uv: vec2<f32>,
    @location(2) fg_color: vec4<f32>,
    @location(3) bg_color: vec4<f32>,
};

struct VertexOutput {
    @builtin(position) clip_position: vec4<f32>,
    @location(0) uv: vec2<f32>,
    @location(1) fg_color: vec4<f32>,
    @location(2) bg_color: vec4<f32>,
};

@vertex
fn vs_main(in: VertexInput) -> VertexOutput {
    var out: VertexOutput;
    out.clip_position = vec4<f32>(in.position, 0.0, 1.0);
    out.uv = in.uv;
    out.fg_color = in.fg_color;
    out.bg_color = in.bg_color;
    return out;
}

@fragment
fn fs_main(in: VertexOutput) -> @location(0) vec4<f32> {
    // Sample the glyph alpha from the atlas texture
    let glyph_alpha = textureSample(atlas_texture, atlas_sampler, in.uv).r;

    // Mix background and foreground colors based on glyph alpha
    // Background is always drawn, foreground is blended on top
    let color = mix(in.bg_color, in.fg_color, glyph_alpha);

    return color;
}
