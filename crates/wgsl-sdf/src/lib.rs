//! WGSL SDF shader and matching `SdfParams` uniform type, shared
//! by every test crate that renders MythTerm chrome (sdf-test,
//! scene-chrome-test, display-test, ...).
//!
//! Also provides a zero-dependency RGBA8 PNG encoder in [`png`]
//! for snapshot pipelines.
//!
//! ## Why this crate exists
//!
//! The SDF shader + uniform layout is *the* contract between the
//! scene graph and the GPU. Duplicating it in every test crate
//! is the kind of drift that breaks reproducibility. See
//! `docs/cinematic-ui-plan.md` § 11.1.
//!
//! ## Uniform layout (std140, 16-byte aligned)
//!
//! ```text
//! data[0]  rect             (x, y, w, h)              vec4
//! data[1]  fill_color                                   vec4
//! data[2]  top_peak_color                                vec4
//! data[3]  top_dim_color                                 vec4
//! data[4]  bot_peak_color                                vec4
//! data[5]  bot_dim_color                                 vec4
//! data[6]  gradient_left                                 vec4
//! data[7]  gradient_right                                vec4
//! data[8]  (corner_radius, top_off, top_in, top_out)     vec4
//! data[9]  (bot_off, bot_in, bot_out, grad_peak)         vec4
//! data[10] _pad                                          vec4
//! ```
//!
//! Total: 11 vec4 = 44 f32 = 176 bytes.
//!
//! ## Shader entry points
//!
//! - `vs_main` (vertex): takes a 2D clip-space position, emits
//!   `clip_pos`, `local_pos` (centered on the rect), and `uv`
//!   (0..1 inside the rect).
//! - `fs_main` (fragment): computes `sd_rounded_box` of the
//!   rect, then layers fill, top border band, and bottom
//!   border band. The border band is asymmetric: the brightest
//!   line of the gradient sits at `top_off` (or `bot_off`) and
//!   falls off in both directions with a smoothstep.

#![doc(html_root_url = "https://github.com/mmacedoeu/mythterm")]

use bytemuck::{Pod, Zeroable};

/// WGSL source. Loaded once at startup; bind groups are reused.
///
/// Includes accessors `p_rect()`, `p_fill()`, ..., `p_grad_peak()`
/// that match the WGSL struct layout above.
pub const SHADER_SRC: &str = include_str!("../shaders/sdf.wgsl");

pub mod png;

/// 16-byte aligned uniform struct. `Pod + Zeroable` so it can
/// be uploaded via `queue.write_buffer` or `create_buffer_init`
/// without intermediate copies.
///
/// **Layout MUST match the WGSL struct.** Both sides use an
/// `array<vec4<f32>, 11>` (44 f32 = 176 bytes) so the std140
/// rules align.
#[repr(C)]
#[derive(Copy, Clone, Debug, Pod, Zeroable)]
pub struct SdfParams {
    /// Raw f32 array. Use the field accessors below; do not
    /// index `data` directly outside of those.
    pub data: [f32; 44],
}

impl SdfParams {
    /// A zero-initialized SdfParams. The shader will render a
    /// zero-size rect, so this is mostly useful as a starting
    /// point that you mutate with the field setters.
    pub const fn zeroed() -> Self {
        Self { data: [0.0; 44] }
    }

    // -- Field accessors --
    // Indices match the WGSL struct comment in `shaders/sdf.wgsl`.
    // 0..4 = rect (x, y, w, h)
    // 4..8 = fill_color
    // 8..12 = top_peak_color
    // 12..16 = top_dim_color
    // 16..20 = bot_peak_color
    // 20..24 = bot_dim_color
    // 24..28 = gradient_left
    // 28..32 = gradient_right
    // 32 = corner_radius
    // 33 = top_line_offset
    // 34 = top_inner_width
    // 35 = top_outer_width
    // 36 = bot_line_offset
    // 37 = bot_inner_width
    // 38 = bot_outer_width
    // 39 = gradient_peak
    // 40..44 = _pad

    #[inline]
    pub fn rect(&mut self, r: [f32; 4]) { self.data[0..4].copy_from_slice(&r); }
    #[inline]
    pub fn fill(&mut self, c: [f32; 4]) { self.data[4..8].copy_from_slice(&c); }
    #[inline]
    pub fn top_peak(&mut self, c: [f32; 4]) { self.data[8..12].copy_from_slice(&c); }
    #[inline]
    pub fn top_dim(&mut self, c: [f32; 4]) { self.data[12..16].copy_from_slice(&c); }
    #[inline]
    pub fn bot_peak(&mut self, c: [f32; 4]) { self.data[16..20].copy_from_slice(&c); }
    #[inline]
    pub fn bot_dim(&mut self, c: [f32; 4]) { self.data[20..24].copy_from_slice(&c); }
    #[inline]
    pub fn grad_left(&mut self, c: [f32; 4]) { self.data[24..28].copy_from_slice(&c); }
    #[inline]
    pub fn grad_right(&mut self, c: [f32; 4]) { self.data[28..32].copy_from_slice(&c); }

    /// Pack the 8 scalar uniforms into `data[32..40]`.
    /// `gradient_peak` is in 0..1; `top_off` / `bot_off` are
    /// pixel offsets from the rect's top/bottom edge.
    #[inline]
    pub fn scalars(
        &mut self,
        corner_radius: f32,
        top_off: f32,
        top_in: f32,
        top_out: f32,
        bot_off: f32,
        bot_in: f32,
        bot_out: f32,
        grad_peak: f32,
    ) {
        self.data[32] = corner_radius;
        self.data[33] = top_off;
        self.data[34] = top_in;
        self.data[35] = top_out;
        self.data[36] = bot_off;
        self.data[37] = bot_in;
        self.data[38] = bot_out;
        self.data[39] = grad_peak;
    }

    /// Uniform size in bytes. 176.
    pub const SIZE: usize = std::mem::size_of::<Self>();
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn size_is_176_bytes() {
        assert_eq!(SdfParams::SIZE, 176);
    }

    #[test]
    fn accessor_indices_are_stable() {
        // This is the contract: any change to these indices
        // requires a matching change in shaders/sdf.wgsl.
        let mut p = SdfParams::zeroed();
        p.rect([1.0, 2.0, 3.0, 4.0]);
        p.fill([0.1, 0.2, 0.3, 1.0]);
        p.top_peak([0.4, 0.5, 0.6, 1.0]);
        p.top_dim([0.7, 0.8, 0.9, 1.0]);
        p.bot_peak([1.0, 1.0, 1.0, 1.0]);
        p.bot_dim([0.0, 0.0, 0.0, 1.0]);
        p.grad_left([0.1, 0.2, 0.3, 1.0]);
        p.grad_right([0.4, 0.5, 0.6, 1.0]);
        p.scalars(0.0, 0.0, 3.0, 1.0, -7.0, 2.0, 2.0, 0.45);
        // Spot-check a few bytes.
        assert_eq!(p.data[0], 1.0);
        assert_eq!(p.data[1], 2.0);
        assert_eq!(p.data[4], 0.1);
        assert_eq!(p.data[8], 0.4);
        assert_eq!(p.data[32], 0.0);
        assert_eq!(p.data[34], 3.0);
        assert_eq!(p.data[36], -7.0);
        assert_eq!(p.data[39], 0.45);
    }
}
