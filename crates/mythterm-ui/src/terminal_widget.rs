//! Terminal widget: egui widget that renders terminal content.

use egui::{Color32, FontId, Rect, Response, Sense, Ui, Vec2, Widget};

/// Cursor style.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CursorStyle {
    /// Solid block cursor.
    Block,
    /// Vertical beam cursor.
    Beam,
    /// Underline cursor.
    Underline,
}

/// A widget that displays terminal content.
pub struct TerminalWidget {
    /// Lines of text to display.
    lines: Vec<String>,
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
    /// Cursor position (col, row).
    cursor: Option<(usize, usize)>,
    /// Cursor style.
    cursor_style: CursorStyle,
    /// Background opacity (0.0 - 1.0).
    bg_opacity: f32,
    /// Cursor blink rate in milliseconds (0 = no blink).
    cursor_blink_ms: u64,
    /// Cursor color.
    cursor_color: Color32,
}

impl TerminalWidget {
    /// Create a new terminal widget with empty content.
    pub fn new(cols: usize, rows: usize, cell_width: f32, cell_height: f32) -> Self {
        Self {
            lines: vec![String::new(); rows],
            cols,
            rows,
            cell_width,
            cell_height,
            bg_color: Color32::from_rgb(30, 30, 30),
            fg_color: Color32::from_rgb(192, 192, 192),
            cursor: Some((0, 0)),
            cursor_style: CursorStyle::Block,
            bg_opacity: 1.0,
            cursor_blink_ms: 500,
            cursor_color: Color32::from_rgb(200, 200, 200),
        }
    }

    /// Create a terminal widget with content.
    pub fn with_content(lines: Vec<String>, cell_width: f32, cell_height: f32) -> Self {
        let rows = lines.len().max(1);
        let cols = lines.iter().map(|l| l.len()).max().unwrap_or(80);
        Self {
            lines,
            cols,
            rows,
            cell_width,
            cell_height,
            bg_color: Color32::from_rgb(30, 30, 30),
            fg_color: Color32::from_rgb(192, 192, 192),
            cursor: Some((0, 0)),
            cursor_style: CursorStyle::Block,
            bg_opacity: 1.0,
            cursor_blink_ms: 500,
            cursor_color: Color32::from_rgb(200, 200, 200),
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

    /// Set the cursor position.
    pub fn cursor(mut self, col: usize, row: usize) -> Self {
        self.cursor = Some((col, row));
        self
    }

    /// Set the cursor style.
    pub fn cursor_style(mut self, style: CursorStyle) -> Self {
        self.cursor_style = style;
        self
    }

    /// Set background opacity (0.0 = transparent, 1.0 = opaque).
    pub fn bg_opacity(mut self, opacity: f32) -> Self {
        self.bg_opacity = opacity.clamp(0.0, 1.0);
        self
    }

    /// Set cursor blink rate in milliseconds (0 = no blink).
    pub fn cursor_blink_ms(mut self, ms: u64) -> Self {
        self.cursor_blink_ms = ms;
        self
    }

    /// Set cursor color.
    pub fn cursor_color(mut self, color: Color32) -> Self {
        self.cursor_color = color;
        self
    }

    /// Update the content from terminal lines.
    pub fn set_lines(&mut self, lines: Vec<String>) {
        self.lines = lines;
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
            let painter = ui.painter();

            // Draw background with opacity
            let [r, g, b, _] = self.bg_color.to_array();
            let a = (self.bg_opacity * 255.0) as u8;
            let bg = Color32::from_rgba_premultiplied(r, g, b, a);
            painter.rect_filled(rect, 0.0, bg);

            // Draw text lines
            let font_id = FontId::monospace(self.cell_height * 0.8);

            for (row, line) in self.lines.iter().enumerate() {
                let y = rect.min.y + row as f32 * self.cell_height;
                let x = rect.min.x + 2.0;

                painter.text(
                    egui::pos2(x, y),
                    egui::Align2::LEFT_TOP,
                    line,
                    font_id.clone(),
                    self.fg_color,
                );
            }

            // Draw cursor with blink
            if let Some((col, row)) = self.cursor {
                if row < self.rows && col < self.cols {
                    // Calculate blink state using egui time
                    let show_cursor = if self.cursor_blink_ms > 0 {
                        let time_secs = ui.input(|i| i.time);
                        let blink_secs = self.cursor_blink_ms as f64 / 1000.0;
                        let phase = (time_secs / blink_secs) as u64;
                        phase % 2 == 0
                    } else {
                        true
                    };

                    if show_cursor {
                        let cursor_x = rect.min.x + col as f32 * self.cell_width;
                        let cursor_y = rect.min.y + row as f32 * self.cell_height;

                        match self.cursor_style {
                            CursorStyle::Block => {
                                let cursor_rect = Rect::from_min_size(
                                    egui::pos2(cursor_x, cursor_y),
                                    Vec2::new(self.cell_width, self.cell_height),
                                );
                                painter.rect_filled(cursor_rect, 0.0, self.cursor_color);
                            }
                            CursorStyle::Beam => {
                                let cursor_rect = Rect::from_min_size(
                                    egui::pos2(cursor_x, cursor_y),
                                    Vec2::new(2.0, self.cell_height),
                                );
                                painter.rect_filled(cursor_rect, 0.0, self.cursor_color);
                            }
                            CursorStyle::Underline => {
                                let cursor_rect = Rect::from_min_size(
                                    egui::pos2(cursor_x, cursor_y + self.cell_height - 2.0),
                                    Vec2::new(self.cell_width, 2.0),
                                );
                                painter.rect_filled(cursor_rect, 0.0, self.cursor_color);
                            }
                        }
                    }

                    // Request repaint for blink animation
                    if self.cursor_blink_ms > 0 {
                        ui.ctx().request_repaint_after(std::time::Duration::from_millis(self.cursor_blink_ms));
                    }
                }
            }
        }

        response
    }
}
