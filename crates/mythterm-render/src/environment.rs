//! Procedural environment cubemap used by the glass reflection pass.
//!
//! Represents a simple indoor environment:
//!   - +Y face: warm white ceiling
//!   - -Y face: dark cool floor
//!   - ±X, ±Z faces: dim neutral walls with a vertical gradient
//!
//! The cubemap is generated once at init time and never modified. A
//! real application would load a HDRi file (and convert to 6 faces),
//! but a procedural cubemap is good enough for a subtle reflection
//! that hints at a "room" surrounding the display.
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
//! cubemap at that direction. The result is a "looking at the room"
//! reflection that varies with screen position.

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
fn generate_procedural_faces(size: u32) -> [Vec<u8>; 6] {
    let mut faces: [Vec<u8>; 6] = std::array::from_fn(|_| vec![0u8; (size * size * 4) as usize]);

    // Ceiling: warm white.
    let ceiling = [1.0_f32, 0.95, 0.85];
    // Floor: dark cool.
    let floor = [0.04, 0.05, 0.08];
    // Walls: dim neutral with a vertical gradient (brighter near top).
    let wall_base = [0.25, 0.27, 0.30];

    for y in 0..size {
        for x in 0..size {
            // v=0 at the top of the face, v=1 at the bottom. For
            // walls we use this for the vertical brightness gradient.
            let v = y as f32 / size as f32;
            let vertical_factor = (1.0 - v).powf(1.5);
            let wall = [
                wall_base[0] * (0.5 + 0.5 * vertical_factor),
                wall_base[1] * (0.5 + 0.5 * vertical_factor),
                wall_base[2] * (0.5 + 0.5 * vertical_factor),
            ];
            // Subtle horizontal variation so the walls aren't flat.
            let u = x as f32 / size as f32;
            let horiz = 0.92 + 0.08 * (u * std::f32::consts::PI * 2.0).sin();
            let wall = [wall[0] * horiz, wall[1] * horiz, wall[2] * horiz];

            // wgpu cubemap face order: +X, -X, +Y, -Y, +Z, -Z
            let face_colors = [wall, wall, ceiling, floor, wall, wall];
            for (face, color) in face_colors.iter().enumerate() {
                let idx = ((y * size + x) * 4) as usize;
                faces[face][idx] = (color[0] * 255.0) as u8;
                faces[face][idx + 1] = (color[1] * 255.0) as u8;
                faces[face][idx + 2] = (color[2] * 255.0) as u8;
                faces[face][idx + 3] = 255;
            }
        }
    }

    faces
}
