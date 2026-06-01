//! Colored terminal content types.

use egui::Color32;

/// A single colored character in the terminal.
#[derive(Debug, Clone)]
pub struct ColoredChar {
    /// The character(s) at this position.
    pub text: String,
    /// Foreground color.
    pub fg: Color32,
    /// Background color.
    pub bg: Color32,
}

/// A line of colored text from the terminal.
#[derive(Debug, Clone)]
pub struct ColoredLine {
    /// The colored characters in this line.
    pub chars: Vec<ColoredChar>,
}

impl ColoredLine {
    /// Create an empty line.
    pub fn new() -> Self {
        Self { chars: Vec::new() }
    }

    /// Get the plain text of the line.
    pub fn text(&self) -> String {
        self.chars.iter().map(|c| c.text.as_str()).collect()
    }

    /// Check if the line is empty (all spaces).
    pub fn is_empty(&self) -> bool {
        self.chars.iter().all(|c| c.text.trim().is_empty())
    }
}

impl Default for ColoredLine {
    fn default() -> Self {
        Self::new()
    }
}
