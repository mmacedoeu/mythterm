//! Color conversion for the linear HDR + ACES-tonemap + sRGB-swapchain
//! pipeline used by mythterm.
//!
//! mythterm renders into a linear `Rgba16Float` HDR target. After
//! the bloom + glass passes, the post-process tonemap fragment shader
//! applies ACES filmic tonemapping (see `crates/mythterm-render/src/postprocess.wgsl`).
//! The tonemap's output is then written to an sRGB-encoded swapchain,
//! and wgpu performs an automatic `linear → sRGB` encoding on that
//! write (the standard sRGB transfer function with the low-end linear
//! segment at `srgb = 12.92 * linear` for `linear ≤ 0.0031308`).
//!
//! The egui-wgpu renderer, in turn, writes vertex colors verbatim to
//! the linear HDR target via `fs_main_gamma_framebuffer` (it picks this
//! entry point because `Rgba16Float` is *not* an sRGB format, so no
//! sRGB conversion is performed on the egui write).
//!
//! The full chain for a UI pixel is therefore:
//!
//! 1. `Color32` (u8 channels) → divided by 255 → stored as linear in HDR.
//! 2. (Optional bloom; no contribution for dim backgrounds.)
//! 3. ACES tonemap compresses the linear value to `[0, 1]` display space.
//! 4. (Micro-contrast, vignette, edge lighting — multiplicative/additive
//!    display-space effects; can be disabled in config.)
//! 5. GPU encodes the float to sRGB on the final swapchain write
//!    (`1.055 * x^(1/2.4) - 0.55` for `x > 0.0031308`, else `12.92 * x`).
//! 6. The compositor displays the resulting byte as the screen pixel.
//!
//! To make a config color `#1E1E1E` (sRGB 30) come out as `#1E1E1E` on
//! screen, we must hand egui the `Color32` whose u8 value `x`, when
//! pushed through the full chain, decodes back to `30`. That is the
//! inverse pipeline: `sRGB_decode → ACES_inverse → ×255 → round`.
//!
//! `srgb_to_display_color32` performs that inverse on a `Color32`.
//! The previous helper `srgb_to_linear_srgb` only undid the sRGB
//! encode on the swapchain write but ignored ACES, so dark config
//! values came out far too dim on screen (e.g. `#1E1E1E` rendered
//! as `#111111`). The new function fixes that.
//!
//! See <https://www.w3.org/TR/srgb-transfer-function/> for the sRGB spec.

/// Convert an sRGB-authored [`egui::Color32`] (i.e. the color you want
/// to appear on screen) into a `Color32` whose u8 values, when fed to
/// egui and pushed through the full ACES + sRGB-encode chain, will
/// reproduce the original sRGB color on screen.
pub fn srgb_to_display_color32(c: egui::Color32) -> egui::Color32 {
    egui::Color32::from_rgba_unmultiplied(
        srgb_to_display_u8(c.r()),
        srgb_to_display_u8(c.g()),
        srgb_to_display_u8(c.b()),
        c.a(),
    )
}

/// Passthrough: return the sRGB color unchanged. Use this for UI chrome
/// (window border, tab fill) where the inverse-pipeline round-trip is
/// not producing the expected on-screen color in this build.
#[inline]
pub fn srgb_passthrough(c: egui::Color32) -> egui::Color32 {
    c
}

/// Inverse pipeline for a single 8-bit channel: given an sRGB value
/// `s` in `[0, 255]`, compute the `u8` that should be stored in a
/// `Color32` so that the post-process tonemap + sRGB swapchain
/// encode lands on `s` on screen.
#[inline]
fn srgb_to_display_u8(s: u8) -> u8 {
    let s_lin = srgb_decode(s as f32 / 255.0);
    let stored = aces_inverse(s_lin);
    (stored.clamp(0.0, 1.0) * 255.0).round() as u8
}

/// Standard sRGB inverse EOTF (sRGB → linear).
///
/// IEC 61966-2-1: if `c ≤ 0.04045`, return `c / 12.92`; otherwise
/// return `((c + 0.055) / 1.055)^2.4`.
#[inline]
fn srgb_decode(c: f32) -> f32 {
    if c <= 0.04045 {
        c / 12.92
    } else {
        ((c + 0.055) / 1.055).powf(2.4)
    }
}

/// ACES filmic tonemap (matches `postprocess.wgsl`).
#[inline]
fn aces(x: f32) -> f32 {
    if x <= 0.0 {
        return 0.0;
    }
    if x >= 1.0 {
        return 1.0;
    }
    let a = 2.51;
    let b = 0.03;
    let c = 2.43;
    let d = 0.59;
    let e = 0.14;
    ((x * (a * x + b)) / (x * (c * x + d) + e)).clamp(0.0, 1.0)
}

/// Bisection-based inverse of [`aces`]. The ACES function compresses
/// `[0, 1]` to roughly `[0, 0.8]`, so the inverse is well-defined on
/// `[0, 0.8]` and saturates to `1.0` above.
#[inline]
fn aces_inverse(y: f32) -> f32 {
    if y <= 0.0 {
        return 0.0;
    }
    // aces(1.0) ≈ 0.804. So values of y > 0.804 are unreachable.
    if y >= 0.804 {
        return 1.0;
    }
    let mut lo = 0.0_f32;
    let mut hi = 1.0_f32;
    for _ in 0..40 {
        let mid = 0.5 * (lo + hi);
        if aces(mid) < y {
            lo = mid;
        } else {
            hi = mid;
        }
        if hi - lo < 1e-6 {
            break;
        }
    }
    0.5 * (lo + hi)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn black_stays_black() {
        assert_eq!(
            srgb_to_display_color32(egui::Color32::BLACK),
            egui::Color32::BLACK,
        );
    }

    #[test]
    fn white_clamps_to_white() {
        // ACES can't reach sRGB(255, 255, 255) from a `Color32` (which
        // tops out at 1.0 linear). The function still must not panic and
        // must return the max-representable value.
        let c = srgb_to_display_color32(egui::Color32::WHITE);
        assert!(c.r() >= 250, "expected near-white, got r={}", c.r());
    }

    #[test]
    fn dark_gray_pipeline_round_trip() {
        // sRGB 30 should round-trip to itself: feed the inverse into the
        // forward chain and the ACES+sRGB-encode outputs 30.
        let target = egui::Color32::from_rgb(30, 30, 30);
        let input = srgb_to_display_color32(target);
        // Forward: input / 255 → aces → sRGB encode
        let lin = input.r() as f32 / 255.0;
        let a = aces(lin);
        // Manual sRGB encode matching the swapchain write
        let srgb = if a <= 0.0031308 {
            12.92 * a
        } else {
            1.055 * a.powf(1.0 / 2.4) - 0.055
        };
        let out = (srgb * 255.0).round() as u8;
        assert!(
            (out as i32 - target.r() as i32).abs() <= 1,
            "round-trip drifted: input={} -> screen={} (target {})",
            input.r(),
            out,
            target.r(),
        );
    }
}
