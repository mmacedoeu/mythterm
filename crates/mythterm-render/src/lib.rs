//! mythterm-render: Terminal rendering with Myth engine integration.
//!
//! Provides glyph atlas, text pipeline, and post-processing effects
//! for cinematic terminal rendering.

pub mod atlas;
pub mod bloom;
pub mod environment;
pub mod pipeline;
pub mod postprocess;
pub mod render_target;
pub mod renderer;

pub use bloom::BloomRenderer;
pub use postprocess::{CurvatureParams, GlassParams, LcdParams, PostProcess, TonemapParams};
pub use render_target::RenderTarget;
pub use renderer::TerminalRenderer;
