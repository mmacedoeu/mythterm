//! Procedural environment cubemap used by the glass reflection pass.
//!
//! Designed as a soft uniform gradient (like a softbox studio light) so
//! the reflection adds a subtle highlight without revealing a
//! recognisable "3D room" pattern on the screen.
//!
//!   - +Y face: soft warm (slightly brighter — the implied "key light")
//!   - -Y face: soft cool (slightly darker — the implied "floor bounce")
//!   - ±X, ±Z faces: medium neutral with a very gentle vertical
//!     gradient (only ~20% range, so no visible "wall vs ceiling" edge)
//!
//! The cubemap is generated once at init time and never modified. A
//! real application would load a HDRi file (and convert to 6 faces),
//! but a soft procedural cubemap is good enough for a subtle glass
//! reflection.
//!
//! Sampling convention: the cubemap is in wgpu's standard layout
//!   array layer 0 = +X
//!   array layer 1 = -X
//!   array layer 2 = +Y
//!   array layer 3 = -Y
//!   array layer 4 = +Z
//!   array layer 5 = -Z
//!
//! The glass shader computes a reflection direction (the line of sight
//! from the camera to the surface, extended behind) and samples the
//! cubemap at that direction. The result is a gentle gradient that
//! varies with screen position, with the warm bias toward the top
//! (where you'd expect a ceiling light) and cool bias toward the
//! bottom (where you'd expect a desk surface).

use wgpu::{
    Device, Extent3d, Origin3d, Queue, Sampler, TexelCopyBufferLayout, TexelCopyTextureInfo,
    Texture, TextureAspect, TextureDescriptor, TextureDimension, TextureFormat, TextureUsages,
    TextureView, TextureViewDescriptor, TextureViewDimension,
};

/// A 6-face cubemap plus a filtering sampler, ready to bind to a
/// `texture_cube<f32>` / `sampler` pair in WGSL.
pub struct EnvironmentMap {
    /// Keep the texture alive while the view borrows it.
    _texture: Texture,
    /// View with `dimension: Cube` for sampling in shaders.
    pub view: TextureView,
    /// Linear filtering sampler.
    pub sampler: Sampler,
}

impl EnvironmentMap {
    /// Create a new procedural environment cubemap. Allocates the GPU
    /// texture and uploads 6 procedural face images.
    pub fn new(device: &Device, queue: &Queue) -> Self {
        const FACE_SIZE: u32 = 256;
        let faces = generate_procedural_faces(FACE_SIZE);

        let texture = device.create_texture(&TextureDescriptor {
            label: Some("Environment Cubemap"),
            size: Extent3d {
                width: FACE_SIZE,
                height: FACE_SIZE,
                depth_or_array_layers: 6,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: TextureDimension::D2,
            format: TextureFormat::Rgba8Unorm,
            usage: TextureUsages::TEXTURE_BINDING | TextureUsages::COPY_DST,
            view_formats: &[],
        });

        // Upload each face to its array layer.
        for (layer, face_data) in faces.iter().enumerate() {
            queue.write_texture(
                TexelCopyTextureInfo {
                    texture: &texture,
                    mip_level: 0,
                    origin: Origin3d {
                        x: 0,
                        y: 0,
                        z: layer as u32,
                    },
                    aspect: TextureAspect::All,
                },
                face_data,
                TexelCopyBufferLayout {
                    offset: 0,
                    bytes_per_row: Some(FACE_SIZE * 4),
                    rows_per_image: Some(FACE_SIZE),
                },
                Extent3d {
                    width: FACE_SIZE,
                    height: FACE_SIZE,
                    depth_or_array_layers: 1,
                },
            );
        }

        let view = texture.create_view(&TextureViewDescriptor {
            label: Some("Environment Cubemap View"),
            dimension: Some(TextureViewDimension::Cube),
            ..Default::default()
        });

        let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("Environment Cubemap Sampler"),
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            ..Default::default()
        });

        Self {
            _texture: texture,
            view,
            sampler,
        }
    }
}

/// Generate 6 procedural face images. Returns `[+X, -X, +Y, -Y, +Z, -Z]`
/// in wgpu cubemap array-layer order.
///
/// All six faces share a single soft uniform gradient — a medium
/// neutral with a gentle warm bias at the top of each face and a cool
/// bias at the bottom, plus a very subtle horizontal shimmer. There
/// is **no** face-specific "room" structure: no bright ceiling, no
/// dark floor. This prevents the glass reflection from showing a
/// recognisable "3D box" on the screen.
fn generate_procedural_faces(size: u32) -> [Vec<u8>; 6] {
    let mut faces: [Vec<u8>; 6] = std::array::from_fn(|_| vec![0u8; (size * size * 4) as usize]);

    for y in 0..size {
        for x in 0..size {
            // v=0 at the top of the face, v=1 at the bottom.
            let v = y as f32 / size as f32;
            // Gentle vertical brightness range (~20% top to bottom).
            let v_factor = 1.0 - v * 0.2;
            // Warm bias at the top, cool bias at the bottom.
            let warmth = 1.0 - v;

            // Base medium neutral.
            let base_r = 0.50;
            let base_g = 0.52;
            let base_b = 0.55;
            // Apply vertical brightness and warm/cool tint (small
            // offsets so the total range stays in roughly [0.40, 0.65]).
            let r = base_r * v_factor + 0.05 * warmth;
            let g = base_g * v_factor;
            let b = base_b * v_factor - 0.05 * warmth;

            // Very subtle horizontal shimmer (±3%) so the gradient
            // doesn't look like a perfectly flat ramp.
            let u = x as f32 / size as f32;
            let horiz = 0.97 + 0.03 * (u * std::f32::consts::PI * 2.0).sin();
            let r = (r * horiz).clamp(0.0, 1.0);
            let g = (g * horiz).clamp(0.0, 1.0);
            let b = (b * horiz).clamp(0.0, 1.0);

            // All six faces share the same color — no per-face "room"
            // structure. The cubemap is a soft uniform gradient.
            let r_byte = (r * 255.0) as u8;
            let g_byte = (g * 255.0) as u8;
            let b_byte = (b * 255.0) as u8;
            for face in 0..6 {
                let idx = ((y * size + x) * 4) as usize;
                faces[face][idx] = r_byte;
                faces[face][idx + 1] = g_byte;
                faces[face][idx + 2] = b_byte;
                faces[face][idx + 3] = 255;
            }
        }
    }

    faces
}
