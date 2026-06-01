//! Font discovery via fontconfig.
//!
//! Finds system fonts by family name, weight, and style.

use crate::FontData;
use anyhow::{Context, Result};
use std::path::PathBuf;

/// Font discovery system.
///
/// Uses fontconfig on Linux to find fonts matching given criteria.
/// Supports fallback font chains for Nerd Fonts, emoji, CJK, etc.
pub struct FontDiscovery {
    #[cfg(unix)]
    fc: Option<fontconfig::Fontconfig>,
    /// Fallback font families to try when a glyph is missing.
    fallback_families: Vec<String>,
}

impl std::fmt::Debug for FontDiscovery {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("FontDiscovery").finish()
    }
}

impl FontDiscovery {
    /// Create a new font discovery instance.
    pub fn new() -> Self {
        Self {
            #[cfg(unix)]
            fc: fontconfig::Fontconfig::new(),
            fallback_families: vec![
                "Nerd Font Symbols".into(),
                "NerdFont".into(),
                "Symbols Nerd Font".into(),
                "Noto Color Emoji".into(),
                "Noto Sans Symbols".into(),
                "Noto Sans Symbols 2".into(),
            ],
        }
    }

    /// Add a fallback font family.
    pub fn add_fallback(&mut self, family: String) {
        self.fallback_families.push(family);
    }

    /// Get the fallback font families.
    pub fn fallback_families(&self) -> &[String] {
        &self.fallback_families
    }

    /// Find a font by family name.
    ///
    /// Returns the font data and metadata, or an error if not found.
    pub fn find_font(&self, family: &str, bold: bool, italic: bool) -> Result<FontData> {
        #[cfg(unix)]
        {
            if let Some(ref fc) = self.fc {
                let style = match (bold, italic) {
                    (true, true) => "Bold Italic",
                    (true, false) => "Bold",
                    (false, true) => "Italic",
                    (false, false) => "Regular",
                };

                if let Some(font) = fc.find(family, Some(style)) {
                    let data = std::fs::read(&font.path)
                        .with_context(|| format!("Failed to read font file: {:?}", font.path))?;

                    return Ok(FontData {
                        data,
                        family: family.to_string(),
                        index: 0,
                        bold,
                        italic,
                    });
                }
            }
        }

        // Fallback: try common font directories
        self.find_font_fallback(family, bold, italic)
    }

    /// Find all available fallback fonts.
    ///
    /// Returns a list of successfully loaded fallback fonts for symbol/emoji coverage.
    pub fn find_fallback_fonts(&self) -> Vec<FontData> {
        let mut fonts = Vec::new();
        for family in &self.fallback_families {
            if let Ok(font) = self.find_font(family, false, false) {
                fonts.push(font);
            }
        }
        fonts
    }

    /// Fallback font search in common directories.
    fn find_font_fallback(&self, family: &str, bold: bool, italic: bool) -> Result<FontData> {
        let dirs = self.font_directories();

        // Map common family names to file name patterns
        let family_lower = family.to_lowercase();
        let patterns: Vec<&str> = match family_lower.as_str() {
            "monospace" | "mono" | "courier" => vec!["Courier", "Mono", "Consolas", "Liberation Mono", "DejaVu Sans Mono"],
            "sans-serif" | "sans" | "arial" => vec!["Arial", "Helvetica", "Liberation Sans", "DejaVu Sans"],
            "serif" | "times" => vec!["Times", "Liberation Serif", "DejaVu Serif"],
            _ => vec![family],
        };

        for dir in &dirs {
            if let Ok(entries) = std::fs::read_dir(dir) {
                for entry in entries.flatten() {
                    let path = entry.path();
                    let file_name = path.file_name().unwrap_or_default().to_string_lossy().to_lowercase();

                    if let Some(ext) = path.extension() {
                        let ext = ext.to_string_lossy().to_lowercase();
                        if ext != "ttf" && ext != "otf" && ext != "ttc" {
                            continue;
                        }
                    } else {
                        continue;
                    }

                    // Check if file name matches any pattern
                    let matches_pattern = patterns.iter().any(|p| {
                        file_name.contains(&p.to_lowercase())
                    });

                    if !matches_pattern {
                        continue;
                    }

                    // Check bold/italic match
                    let is_bold = file_name.contains("bold");
                    let is_italic = file_name.contains("italic") || file_name.contains("oblique");

                    if is_bold != bold || is_italic != italic {
                        continue;
                    }

                    if let Ok(data) = std::fs::read(&path) {
                        return Ok(FontData {
                            data,
                            family: family.to_string(),
                            index: 0,
                            bold,
                            italic,
                        });
                    }
                }
            }
        }

        // Last resort: return the first font file found
        for dir in &dirs {
            if let Ok(entries) = std::fs::read_dir(dir) {
                for entry in entries.flatten() {
                    let path = entry.path();
                    if let Some(ext) = path.extension() {
                        let ext = ext.to_string_lossy().to_lowercase();
                        if ext == "ttf" || ext == "otf" || ext == "ttc" {
                            if let Ok(data) = std::fs::read(&path) {
                                return Ok(FontData {
                                    data,
                                    family: family.to_string(),
                                    index: 0,
                                    bold,
                                    italic,
                                });
                            }
                        }
                    }
                }
            }
        }

        anyhow::bail!("Font not found: {}", family)
    }

    /// Get the list of font directories to search.
    fn font_directories(&self) -> Vec<PathBuf> {
        let mut dirs = Vec::new();

        #[cfg(unix)]
        {
            dirs.push(PathBuf::from("/usr/share/fonts"));
            dirs.push(PathBuf::from("/usr/local/share/fonts"));
            if let Some(home) = dirs_next::home_dir() {
                dirs.push(home.join(".local/share/fonts"));
                dirs.push(home.join(".fonts"));
            }
        }

        dirs
    }
}

impl Default for FontDiscovery {
    fn default() -> Self {
        Self::new()
    }
}
