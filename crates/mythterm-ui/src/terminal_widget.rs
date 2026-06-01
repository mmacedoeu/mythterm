//! Terminal widget: egui widget that renders terminal content.
//!
//! Displays the terminal content as an egui widget, handling
//! click events, scroll, and selection.

use egui::{Color32, Response, Sense, Ui, Vec2, Widget};

/// A widget that displays terminal content.
///
/// Renders a terminal grid with text, cursor, and selection.
/// For now, this is a placeholder that will be connected to
/// the actual terminal renderer.
pub struct TerminalWidget {
    /// Width in cells.
    cols: usize,
    /// Height in cells.
    rows: usize,
    /// Cell width in pixels.
    cell_width: f32,
    /// Cell height in pixels.
    cell_height: f32,
    /// Background color.
    bg_color: Color32,
    /// Foreground color.
    fg_color: Color32,
}

impl TerminalWidget {
    /// Create a new terminal widget.
    pub fn new(cols: usize, rows: usize, cell_width: f32, cell_height: f32) -> Self {
        Self {
            cols,
            rows,
            cell_width,
            cell_height,
            bg_color: Color32::from_rgb(30, 30, 30),
            fg_color: Color32::from_rgb(192, 192, 192),
        }
    }

    /// Set the background color.
    pub fn bg_color(mut self, color: Color32) -> Self {
        self.bg_color = color;
        self
    }

    /// Set the foreground color.
    pub fn fg_color(mut self, color: Color32) -> Self {
        self.fg_color = color;
        self
    }
}

impl Widget for TerminalWidget {
    fn ui(self, ui: &mut Ui) -> Response {
        let desired_size = Vec2::new(
            self.cols as f32 * self.cell_width,
            self.rows as f32 * self.cell_height,
        );

        let (rect, response) = ui.allocate_exact_size(desired_size, Sense::click_and_drag());

        if ui.is_rect_visible(rect) {
            // Draw background
            ui.painter().rect_filled(rect, 0.0, self.bg_color);

            // Draw grid lines (for debugging)
            for row in 0..self.rows {
                let y = rect.min.y + row as f32 * self.cell_height;
                ui.painter().line_segment(
                    [
                        egui::pos2(rect.min.x, y),
                        egui::pos2(rect.max.x, y),
                    ],
                    egui::Stroke::new(0.5, Color32::from_rgba_premultiplied(60, 60, 60, 128)),
                );
            }

            for col in 0..self.cols {
                let x = rect.min.x + col as f32 * self.cell_width;
                ui.painter().line_segment(
                    [
                        egui::pos2(x, rect.min.y),
                        egui::pos2(x, rect.max.y),
                    ],
                    egui::Stroke::new(0.5, Color32::from_rgba_premultiplied(60, 60, 60, 128)),
                );
            }

            // TODO: Render actual terminal content here
            // This would involve:
            // 1. Getting the terminal state from the mux
            // 2. Rendering each cell with the appropriate glyph and color
            // 3. Drawing the cursor
            // 4. Drawing selection highlights
        }

        response
    }
}
