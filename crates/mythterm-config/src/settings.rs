use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Settings {
    pub font_family: String,
    pub font_size: f32,
    pub cursor_style: String,
}

impl Default for Settings {
    fn default() -> Self {
        Self { font_family: "monospace".into(), font_size: 14.0, cursor_style: "Block".into() }
    }
}
