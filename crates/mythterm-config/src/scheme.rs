use serde::{Deserialize, Serialize};
use wezterm_term::color::{ColorPalette, SrgbaTuple};

/// A terminal color scheme.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ColorScheme {
    /// Foreground color (R, G, B).
    pub foreground: [u8; 3],
    /// Background color (R, G, B).
    pub background: [u8; 3],
    /// Cursor color (R, G, B).
    pub cursor: [u8; 3],
    /// ANSI palette (16 colors: 8 normal + 8 bright).
    pub ansi: [[u8; 3]; 16],
}

impl Default for ColorScheme {
    fn default() -> Self {
        Self {
            foreground: [0xc0, 0xc0, 0xc0],
            background: [0x1e, 0x1e, 0x1e],
            cursor: [0xff, 0xff, 0xff],
            ansi: [
                // Normal: black, red, green, yellow, blue, magenta, cyan, white
                [0x00, 0x00, 0x00],
                [0xcd, 0x31, 0x31],
                [0x0d, 0xbb, 0x0d],
                [0xe5, 0xe5, 0x10],
                [0x24, 0x72, 0xc8],
                [0xbc, 0x3f, 0xbc],
                [0x0b, 0xb7, 0xb7],
                [0xe5, 0xe5, 0xe5],
                // Bright: black, red, green, yellow, blue, magenta, cyan, white
                [0x66, 0x66, 0x66],
                [0xf1, 0x4c, 0x4c],
                [0x23, 0xd1, 0x23],
                [0xf5, 0xf5, 0x43],
                [0x3b, 0x8e, 0xea],
                [0xd6, 0x70, 0xd6],
                [0x29, 0xb8, 0xdb],
                [0xff, 0xff, 0xff],
            ],
        }
    }
}

/// Helper to convert [u8; 3] + alpha to SrgbaTuple.
fn rgb_to_srgba(c: [u8; 3], alpha: u8) -> SrgbaTuple {
    SrgbaTuple::from((c[0], c[1], c[2], alpha))
}

impl ColorScheme {
    /// Convert to WezTerm's `ColorPalette` for the terminal core.
    pub fn to_color_palette(&self) -> ColorPalette {
        let mut palette = ColorPalette::default();
        palette.foreground = rgb_to_srgba(self.foreground, 0xff);
        palette.background = rgb_to_srgba(self.background, 0xff);
        palette.cursor_fg = rgb_to_srgba(self.background, 0xff);
        palette.cursor_bg = rgb_to_srgba(self.cursor, 0xff);
        palette.cursor_border = palette.cursor_bg;
        for (i, color) in self.ansi.iter().enumerate() {
            palette.colors.0[i] = rgb_to_srgba(*color, 0xff);
        }
        palette
    }
}
