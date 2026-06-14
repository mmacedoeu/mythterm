// SDF shader for MythTerm chrome.
//
// Draws a rounded-rect quad with a per-tab fill, a top border
// band, and a bottom border band. The border bands have an
// asymmetric horizontal brightness gradient: brightest near
// `grad_peak` (typically 0.45 = center-left), dim toward both
// edges. This matches the photographed OLED-tab aesthetic.
//
// Uniform layout (16-byte aligned, std140):
//
//   data[0]  rect             (x, y, w, h)              vec4
//   data[1]  fill_color                                   vec4
//   data[2]  top_peak_color                                vec4
//   data[3]  top_dim_color                                 vec4
//   data[4]  bot_peak_color                                vec4
//   data[5]  bot_dim_color                                 vec4
//   data[6]  gradient_left                                 vec4
//   data[7]  gradient_right                                vec4
//   data[8]  (corner_radius, top_off, top_in, top_out)     vec4
//   data[9]  (bot_off, bot_in, bot_out, grad_peak)         vec4
//   data[10] _pad                                          vec4
//
// The Rust side mirrors this exactly in `wgsl_sdf::SdfParams`.
// Drift between the two is the #1 reproducibility risk; if you
// touch one, touch the other.

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
