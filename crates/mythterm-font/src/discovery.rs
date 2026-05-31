//! Font discovery via fontconfig.
//!
//! Finds system fonts by family name, weight, and style.

use crate::FontData;
use anyhow::{Context, Result};
use std::path::PathBuf;

/// Font discovery system.
///
/// Uses fontconfig on Linux to find fonts matching given criteria.
pub struct FontDiscovery {
    #[cfg(unix)]
    fc: Option<fontconfig::Fontconfig>,
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
        }
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

    /// Fallback font search in common directories.
    fn find_font_fallback(&self, family: &str, bold: bool, italic: bool) -> Result<FontData> {
        let dirs = self.font_directories();

        for dir in &dirs {
            if let Ok(entries) = std::fs::read_dir(dir) {
                for entry in entries.flatten() {
                    let path = entry.path();
                    if let Some(ext) = path.extension() {
                        let ext = ext.to_string_lossy().to_lowercase();
                        if ext == "ttf" || ext == "otf" || ext == "ttc" {
                            // Try to load and check if it matches
                            if let Ok(data) = std::fs::read(&path) {
                                // For now, return the first font found
                                // TODO: Parse font name tables to match family
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
